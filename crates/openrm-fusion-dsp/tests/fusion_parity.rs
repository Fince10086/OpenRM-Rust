use openrm_fusion_dsp::{
    delay_norm_from_param, render_offline, FusionCore, BAND_SIZES, HISTORY_SAMPLES,
};

fn sine(freq: f32, sr: f32, len: usize) -> Vec<f32> {
    (0..len)
        .map(|i| (2.0 * std::f32::consts::PI * freq * i as f32 / sr).sin() * 0.5)
        .collect()
}

fn rms(x: &[f32]) -> f32 {
    (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
}

#[test]
fn band_sizes_and_history_depth() {
    for (i, &n) in BAND_SIZES.iter().enumerate() {
        assert_eq!(n, 256usize << i);
        let frames = (HISTORY_SAMPLES / n as f64) as usize;
        assert_eq!(frames, (1_764_000 / n).max(1), "N={n} frames");
    }
    assert_eq!((HISTORY_SAMPLES / 1024.0) as usize, 1722);
    assert_eq!((HISTORY_SAMPLES / 131072.0) as usize, 13);
}

#[test]
fn hann_squared_cola_is_1_4985() {
    let n = 1024usize;
    let hop = n / 4;
    let win: Vec<f32> = (0..n)
        .map(|k| 0.5 * (1.0 - (2.0 * std::f32::consts::PI * k as f32 / (n - 1) as f32).cos()))
        .collect();
    let mut acc = vec![0.0f32; n * 8];
    for p in (0..n * 7).step_by(hop) {
        for k in 0..n {
            acc[p + k] += win[k] * win[k];
        }
    }
    let mid = &acc[n * 4..n * 5];
    let mean = mid.iter().sum::<f32>() / mid.len() as f32;
    assert!((mean - 1.4985).abs() < 1e-3, "COLA mean={mean}");
}

#[test]
fn default_amplitude_is_minus_four_db() {
    let amp = openrm_fusion_dsp::amplitude_from_slider(0.9);
    assert!((20.0 * amp.log10() + 4.0).abs() < 0.05, "amp={amp}");
}

#[test]
fn gain_extremes() {
    let mut core = FusionCore::new();
    core.set_gain(0.0);
    assert!((core.gain_linear() - 10f32.powf(-4.8)).abs() < 1e-7);
    core.set_gain(0.8);
    assert!((core.gain_linear() - 1.0).abs() < 1e-6);
}

#[test]
fn full_band_is_unity_sine_with_cola_gain() {
    let sr = 44_100.0;
    let mut core = FusionCore::new();
    core.set_gain(0.8);
    core.set_mix(1.0);
    core.bars[0].active = true;
    core.bars[0].f1 = 0.0;
    core.bars[0].f2 = 1.0;
    core.bars[0].a1 = 0.0;
    core.bars[0].a2 = 0.0;
    core.bars[0].amp_slider = 1.0;
    core.bars[0].bar_amp = 1.0;
    core.bars[0].smooth = 0.0;
    core.bars[0].pan = 0.5;
    core.bars[0].width = 0.0;
    for bar in core.bars.iter_mut().skip(1) {
        bar.active = false;
    }
    let len = sr as usize * 3;
    let x = sine(1000.0, sr, len);
    let (y, _) = render_offline(&mut core, &x, &x);
    let steady = &y[len / 2..];
    let ratio = rms(steady) / rms(&x[len / 2..]);
    assert!((ratio - 1.495).abs() < 0.05, "identity ratio={ratio}");
}

#[test]
fn frame_delay_is_quantized_to_hop() {
    let sr = 44_100.0;
    let mut core = FusionCore::new();
    core.set_gain(0.8);
    core.set_mix(1.0);
    core.bars[0].active = true;
    core.bars[0].f1 = 0.0;
    core.bars[0].f2 = 1.0;
    core.bars[0].a1 = delay_norm_from_param(0.25);
    core.bars[0].a2 = core.bars[0].a1;
    core.bars[0].amp_slider = 1.0;
    core.bars[0].bar_amp = 1.0;
    core.bars[0].smooth = 0.0;
    core.bars[0].pan = 0.5;
    core.bars[0].width = 0.0;
    let len = sr as usize * 4;
    let mut x = vec![0.0f32; len];
    x[1000] = 1.0;
    let (y, _) = render_offline(&mut core, &x, &x);
    let peak = y
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.abs().partial_cmp(&b.1.abs()).unwrap())
        .unwrap()
        .0;
    let expected = 1000 + 110_080;
    assert!(
        (peak as i64 - expected as i64).abs() < 300,
        "peak={peak} expected≈{expected}"
    );
}

#[test]
fn default_core_passes_375hz() {
    let sr = 48_000.0f32;
    let mut core = FusionCore::new();
    core.set_gain(0.8);
    core.set_mix(1.0);
    let len = sr as usize * 2;
    let x = sine(375.0, sr, len);
    let (y, _) = render_offline(&mut core, &x, &x);
    let peak = y.iter().cloned().fold(0.0f32, |a, b| a.max(b.abs()));
    let nz = y.iter().position(|v| v.abs() > 1e-6).unwrap_or(999999);
    println!(
        "default ratio={} peak={peak} first={nz} len={}",
        rms(&y[len / 2..]) / rms(&x[len / 2..]),
        y.len()
    );
    assert!(rms(&y[len / 2..]) > 0.1);
}

#[test]
fn pan_left_feeds_only_left_output() {
    let sr = 48_000.0f32;
    let mut core = FusionCore::new();
    core.set_gain(0.8);
    core.set_mix(1.0);
    core.bars[0].f1 = 0.0;
    core.bars[0].f2 = 1.0;
    core.bars[0].a1 = 0.0;
    core.bars[0].a2 = 0.0;
    core.bars[0].amp_slider = 1.0;
    core.bars[0].bar_amp = 1.0;
    core.bars[0].smooth = 0.0;
    core.bars[0].pan = 0.0;
    core.bars[0].width = 0.0;
    let len = sr as usize * 2;
    let x = sine(1000.0, sr, len);
    let zeros = vec![0.0f32; len];
    let (yl, yr) = render_offline(&mut core, &zeros, &x);
    let tail = len / 2;
    assert!(rms(&yl[tail..]) > 0.1, "left should carry the band");
    assert!(rms(&yr[tail..]) < 1e-5, "right must stay silent");
}

#[test]
fn changing_fft_size_keeps_processing() {
    let sr = 48_000.0f32;
    let mut core = FusionCore::new();
    core.set_gain(0.8);
    core.set_mix(1.0);
    core.bars[0].f1 = 0.0;
    core.bars[0].f2 = 1.0;
    core.bars[0].a1 = 0.0;
    core.bars[0].a2 = 0.0;
    core.bars[0].amp_slider = 1.0;
    core.bars[0].bar_amp = 1.0;
    core.bars[0].smooth = 0.0;
    core.bars[0].pan = 0.5;
    core.bars[0].width = 0.0;
    for idx in [2usize, 3] {
        core.set_bands_index(idx);
        let len = sr as usize * 2;
        let x = sine(1000.0, sr, len);
        let (y, _) = render_offline(&mut core, &x, &x);
        assert!(rms(&y[len / 2..]) > 0.2, "N={}", BAND_SIZES[idx]);
    }
}

#[test]
fn mix_zero_is_exact_passthrough() {
    let sr = 48_000.0f32;
    let mut core = FusionCore::new();
    core.set_gain(0.8);
    core.set_mix(0.0);
    let len = sr as usize;
    let x = sine(1000.0, sr, len);
    let (y, _) = render_offline(&mut core, &x, &x);
    assert_eq!(y, x);
}

#[test]
fn reset_clears_feedback_history() {
    let sr = 48_000.0f32;
    let mut core = FusionCore::new();
    core.set_gain(0.8);
    core.set_mix(1.0);
    core.bars[0].f1 = 0.0;
    core.bars[0].f2 = 1.0;
    core.bars[0].a1 = 0.0;
    core.bars[0].a2 = 0.0;
    core.bars[0].amp_slider = 1.0;
    core.bars[0].bar_amp = 1.0;
    core.bars[0].smooth = 0.0;
    core.bars[0].pan = 0.5;
    core.bars[0].width = 0.0;
    core.bars[0].feedback = 0.8;
    let len = sr as usize * 2;
    let x = sine(1000.0, sr, len);
    let _ = render_offline(&mut core, &x, &x);
    core.reset();
    let silence = vec![0.0f32; sr as usize * 2];
    let (y, _) = render_offline(&mut core, &silence, &silence);
    assert!(
        y.iter().all(|v| v.abs() < 1e-7),
        "reset left energy in history"
    );
}
