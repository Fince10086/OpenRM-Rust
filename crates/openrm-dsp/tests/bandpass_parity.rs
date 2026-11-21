//! BandPass 核心的移植回归测试。
//!
//! 断言集合与 C++ `tests/dsp_test.cpp` 一一对应，参考数值取自 C++ 版在同一
//! fs=48000 / block=512 下的实测输出（见各测试里的注释）。判定阈值沿用原测试，
//! 移植是否"走样"由这些阈值兜底，而不是靠人耳。

use openrm_dsp::bandpass::{
    BandPassCore, Params, K_RANDOM_BW_L, K_RANDOM_FREQ_L, K_RANDOM_GAIN_L, K_RANDOM_MIX,
};
use std::f64::consts::PI;

const FS: f64 = 48000.0;
const BLOCK: usize = 512;

fn make_params() -> Params {
    Params {
        freq_l: 1000.0,
        bw_l: 1.0,
        gain_l: 1.0,
        freq_r: 1000.0,
        bw_r: 1.0,
        gain_r: 1.0,
        mix: 1.0,
        ..Default::default()
    }
}

fn measure_gain(core: &mut BandPassCore, freq_hz: f64, seconds: f64) -> f64 {
    let n = (FS * seconds) as usize;
    let mut input = vec![0.0f32; n];
    let mut out = vec![0.0f32; n];
    for (i, s) in input.iter_mut().enumerate() {
        *s = (2.0 * PI * freq_hz * i as f64 / FS).sin() as f32;
    }
    let mut pos = 0;
    while pos < n {
        let b = BLOCK.min(n - pos);
        core.update_smoothing(b);
        core.process_mono(&input[pos..pos + b], &mut out[pos..pos + b], None, b);
        pos += b;
    }
    let skip = (FS * 0.1) as usize;
    let mut in_rms = 0.0f64;
    let mut out_rms = 0.0f64;
    for i in skip..n {
        in_rms += (input[i] as f64) * (input[i] as f64);
        out_rms += (out[i] as f64) * (out[i] as f64);
    }
    let m = (n - skip) as f64;
    (out_rms / m).sqrt() / ((in_rms / m).sqrt() + 1e-12)
}

fn fresh_core(p: &Params) -> BandPassCore {
    let mut core = BandPassCore::new();
    core.prepare(FS);
    core.set_params(p);
    core
}

#[test]
fn peak_at_center() {
    let p = Params {
        freq_l: 2000.0,
        ..make_params()
    };
    let mut core = fresh_core(&p);
    let g_c = measure_gain(&mut core, 2000.0, 1.0);
    let g_h = measure_gain(&mut core, 1000.0, 1.0);
    let g_d = measure_gain(&mut core, 4000.0, 1.0);
    println!("gains: 1k={g_h:.3} 2k={g_c:.3} 4k={g_d:.3}");
    assert!(g_c >= g_h && g_c >= g_d, "峰值应在中心 2k");
    assert!(g_h < g_c * 0.5, "f0/2 处衰减不足");
}

#[test]
fn bandwidth_narrower_than_wide() {
    // C++ 实测: 0.2oct = 277.4 Hz, 2oct = 1587.4 Hz 量级（此处断言相对关系）
    let measure_bw = |bw_oct: f64| {
        let p = Params {
            freq_l: 2000.0,
            bw_l: bw_oct,
            ..make_params()
        };
        let mut core = fresh_core(&p);
        let edge = |low: bool, core: &mut BandPassCore| {
            let (mut lo, mut hi) = if low {
                (20.0, 2000.0)
            } else {
                (2000.0, 10000.0)
            };
            for _ in 0..50 {
                let mid = 0.5 * (lo + hi);
                let g = measure_gain(core, mid, 1.0);
                if low {
                    if g > 0.7071 {
                        hi = mid
                    } else {
                        lo = mid
                    }
                } else if g > 0.7071 {
                    lo = mid
                } else {
                    hi = mid
                }
            }
            0.5 * (lo + hi)
        };
        let low = edge(true, &mut core);
        let high = edge(false, &mut core);
        high - low
    };

    let bw_narrow = measure_bw(0.2);
    let bw_wide = measure_bw(2.0);
    println!("-3dB BW: 0.2oct={bw_narrow:.1} Hz, 2oct={bw_wide:.1} Hz");
    assert!(bw_narrow < bw_wide * 0.6);
    assert!(
        (180.0..320.0).contains(&bw_narrow),
        "窄带 -3dB 带宽应约 277 Hz"
    );
}

#[test]
fn link_follows_left() {
    let p = Params {
        freq_l: 3000.0,
        bw_l: 0.5,
        freq_r: 500.0,
        bw_r: 0.5,
        linked: true,
        ..make_params()
    };
    let mut core = fresh_core(&p);
    let n = (FS * 0.5) as usize;
    let mut input = vec![0.0f32; n];
    for (i, s) in input.iter_mut().enumerate() {
        let t = i as f64 / FS;
        *s = ((2.0 * PI * 3000.0 * t).sin() * 0.5 + (2.0 * PI * 500.0 * t).sin() * 0.5) as f32;
    }
    let mut ol = vec![0.0f32; n];
    let mut or = vec![0.0f32; n];
    let mut pos = 0;
    while pos < n {
        let b = BLOCK.min(n - pos);
        core.update_smoothing(b);
        core.process_stereo(
            &input[pos..pos + b],
            &input[pos..pos + b],
            &mut ol[pos..pos + b],
            &mut or[pos..pos + b],
            None,
            None,
            b,
        );
        pos += b;
    }
    let skip = (FS * 0.1) as usize;
    let rms = |v: &[f32]| {
        let s: f64 = v[skip..].iter().map(|x| (*x as f64) * (*x as f64)).sum();
        (s / (n - skip) as f64).sqrt()
    };
    let rl = rms(&ol);
    let rr = rms(&or);
    println!("linked RMS: L={rl:.4} R={rr:.4}");
    assert!((rr - rl).abs() / (rl + 1e-9) < 0.05);
}

#[test]
fn mix_dry_wet() {
    let p = Params {
        freq_l: 1000.0,
        mix: 0.0,
        ..make_params()
    };
    let mut core = fresh_core(&p);
    let dry = measure_gain(&mut core, 2000.0, 1.0);
    assert!((dry - 1.0).abs() < 0.01, "mix=0 应全干直通");

    let p = Params { mix: 1.0, ..p };
    core.set_params(&p);
    let wet = measure_gain(&mut core, 2000.0, 1.0);
    println!("mix: dry={dry:.3} wet={wet:.3}");
    assert!(wet < 0.9, "mix=1 应只剩滤波输出");
}

#[test]
fn center_gain_normalised() {
    let mut core = BandPassCore::new();
    core.prepare(FS);
    for bw in [0.2, 1.0, 2.0] {
        let p = Params {
            freq_l: 2000.0,
            bw_l: bw,
            ..make_params()
        };
        core.set_params(&p);
        let g = measure_gain(&mut core, 2000.0, 1.0);
        println!("bw={bw:.1} oct: center gain={g:.3}");
        if bw >= 1.0 {
            assert!((g - 1.0).abs() < 0.05, "宽带中心增益应归一");
        } else {
            assert!(g >= 0.85, "窄带中心允许轻微下垂但不低于 0.85");
        }
    }
}

#[test]
fn stable_under_random_modulation() {
    let mut p = make_params();
    p.freq_l = 4000.0;
    p.bw_l = 0.05;
    p.freq_r = 250.0;
    p.bw_r = 0.1;
    p.random_amount = [0.5, 1.0, 0.3, 0.7];
    p.random_period_sec = [0.125, 2.0, 0.02, 10.0];
    p.random_enable_freq_l = true;
    p.random_enable_bw_l = true;
    p.random_enable_gain_l = true;
    p.random_enable_freq_r = true;
    p.random_enable_bw_r = true;
    p.random_enable_gain_r = true;
    p.random_enable_mix = true;
    p.random_color_freq_l = 0;
    p.random_color_bw_l = 1;
    p.random_color_gain_l = 2;
    p.random_color_freq_r = 3;
    p.random_color_bw_r = 2;
    p.random_color_gain_r = 1;
    p.random_color_mix = 0;

    let mut core = fresh_core(&p);
    let n = (FS * 10.0) as usize;
    let mut input = vec![0.0f32; n];
    let mut out = vec![0.0f32; n];
    let mut s: u32 = 12345;
    for v in input.iter_mut() {
        s = s.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        *v = (s as f64 / 4_294_967_296.0 * 2.0 - 1.0) as f32;
    }
    let mut pos = 0;
    while pos < n {
        let b = BLOCK.min(n - pos);
        core.update_smoothing(b);
        core.process_mono(&input[pos..pos + b], &mut out[pos..pos + b], None, b);
        pos += b;
    }
    let mut peak = 0.0f64;
    for (i, v) in out.iter().enumerate() {
        assert!(v.is_finite(), "第 {i} 个样本不是有限值");
        peak = peak.max(v.abs() as f64);
    }
    println!("peak |out| = {peak:.3}");
    assert!(peak < 16.0, "窄带 + 全随机映射不应发散");
}

#[test]
fn random_maps_change_output() {
    let base = Params {
        freq_l: 1000.0,
        bw_l: 1.0,
        mix: 0.5,
        ..make_params()
    };
    let mut core_none = fresh_core(&base);
    let g_none = measure_gain(&mut core_none, 1000.0, 1.0);

    let mk = |stream: usize| {
        let mut p = base;
        let col = 0u8;
        match stream {
            K_RANDOM_FREQ_L => p.random_enable_freq_l = true,
            K_RANDOM_GAIN_L => p.random_enable_gain_l = true,
            K_RANDOM_MIX => p.random_enable_mix = true,
            _ => unreachable!(),
        }
        p.random_amount[col as usize] = 1.0;
        p.random_period_sec[col as usize] = 0.02;
        p
    };

    let mut core_freq = fresh_core(&mk(K_RANDOM_FREQ_L));
    let g_freq = measure_gain(&mut core_freq, 1000.0, 1.0);
    let mut core_gain = fresh_core(&mk(K_RANDOM_GAIN_L));
    let g_gain = measure_gain(&mut core_gain, 1000.0, 1.0);
    let mut core_mix = fresh_core(&mk(K_RANDOM_MIX));
    let g_mix = measure_gain(&mut core_mix, 1000.0, 1.0);

    println!("gains: none={g_none:.3} freqMap={g_freq:.3} gainMap={g_gain:.3} mixMap={g_mix:.3}");
    assert!((g_freq - g_none).abs() > 0.05);
    assert!((g_gain - g_none).abs() > 0.05);
    assert!((g_mix - g_none).abs() > 0.05);
}

#[test]
fn random_walk_survives_host_reset() {
    let mut p = make_params();
    p.random_enable_freq_l = true;
    p.random_color_freq_l = 0;
    p.random_amount[0] = 1.0;
    p.random_period_sec[0] = 0.01;
    let mut core = BandPassCore::new();
    core.prepare(FS);
    core.set_params(&p);
    core.prepare(FS); // 复现宿主 Reset: prepare 落在 setParams 之后

    let n = (FS * 1.0) as usize;
    let input = vec![0.0f32; n];
    let mut out = vec![0.0f32; n];
    let mut max_delta = 0.0f32;
    let mut pos = 0;
    while pos < n {
        let b = BLOCK.min(n - pos);
        core.update_smoothing(b);
        core.process_mono(&input[pos..pos + b], &mut out[pos..pos + b], None, b);
        max_delta = max_delta.max(core.random_deltas().freq_oct[0].abs());
        pos += b;
    }
    println!("max |freq delta| after reset = {max_delta:.3} oct");
    assert!(max_delta > 0.05, "reset 后随机游走必须继续推进");
}

#[test]
fn slope_default_and_bandwidth_preserved() {
    assert_eq!(Params::default().slope_db_l, 96.0);
    assert_eq!(Params::default().slope_db_r, 96.0);

    let bw_at = |slope_db: f64| {
        let p = Params {
            freq_l: 1000.0,
            bw_l: 1.0,
            slope_db_l: slope_db,
            ..make_params()
        };
        let mut core = fresh_core(&p);
        let mut low_edge = {
            let (mut lo, mut hi) = (20.0, 1000.0);
            for _ in 0..50 {
                let mid = 0.5 * (lo + hi);
                if measure_gain(&mut core, mid, 1.0) > 0.7071 {
                    hi = mid
                } else {
                    lo = mid
                }
            }
            0.5 * (lo + hi)
        };
        let high_edge = {
            let (mut lo, mut hi) = (1000.0, 10000.0);
            for _ in 0..50 {
                let mid = 0.5 * (lo + hi);
                if measure_gain(&mut core, mid, 1.0) > 0.7071 {
                    lo = mid
                } else {
                    hi = mid
                }
            }
            0.5 * (lo + hi)
        };
        low_edge = low_edge.max(0.0);
        high_edge - low_edge
    };

    let bw12 = bw_at(12.0);
    let bw24 = bw_at(24.0);
    let bw48 = bw_at(48.0);
    let bw96 = bw_at(96.0);
    println!("-3dB BW: 12={bw12:.1} 24={bw24:.1} 48={bw48:.1} 96={bw96:.1}");
    // C++ 参考: 12=631.8 24=705.0 48=707.1 96=707.1
    assert!((bw96 - bw12).abs() / bw12 < 0.25);
    assert!((bw48 - bw12).abs() / bw12 < 0.25);
    assert!((bw24 - bw12).abs() / bw12 < 0.25);
}

#[test]
fn slope_attenuation_and_passband_flatness() {
    let attn_db = |slope_db: f64, oct_above_cut: f64| {
        let p = Params {
            freq_l: 1000.0,
            bw_l: 1.0,
            slope_db_l: slope_db,
            ..make_params()
        };
        let mut core = fresh_core(&p);
        let g = measure_gain(&mut core, 1414.0 * 2.0f64.powf(oct_above_cut), 1.0);
        20.0 * (g + 1e-12).log10()
    };
    let a12 = attn_db(12.0, 1.0);
    let a24 = attn_db(24.0, 1.0);
    let a48 = attn_db(48.0, 1.0);
    let a96 = attn_db(96.0, 1.0);
    println!("1 oct 外衰减: 12={a12:.1} 24={a24:.1} 48={a48:.1} 96={a96:.1} dB");
    assert!((-13.5..-10.5).contains(&a12));
    assert!((-25.5..-22.5).contains(&a24));
    assert!((-49.5..-46.5).contains(&a48));
    assert!(a96 < -90.0);

    let p = Params {
        freq_l: 1000.0,
        bw_l: 1.0,
        slope_db_l: 96.0,
        ..make_params()
    };
    let mut core = fresh_core(&p);
    let g = measure_gain(&mut core, 1414.0 / 2.0f64.powf(0.25), 1.0);
    let in_db = 20.0 * (g + 1e-12).log10();
    println!("cut 前 0.25 oct: {in_db:.3} dB");
    assert!(in_db > -0.1);
}

#[test]
fn unlinked_slopes_stay_independent_then_link_overrides() {
    let mut p = Params {
        freq_l: 1000.0,
        bw_l: 1.0,
        slope_db_l: 12.0,
        freq_r: 1000.0,
        bw_r: 1.0,
        slope_db_r: 96.0,
        linked: false,
        ..make_params()
    };
    let mut core = fresh_core(&p);
    let n = (FS * 1.0) as usize;
    let mut input = vec![0.0f32; n];
    for (i, s) in input.iter_mut().enumerate() {
        *s = (2.0 * PI * 2828.0 * i as f64 / FS).sin() as f32;
    }
    let mut ol = vec![0.0f32; n];
    let mut or = vec![0.0f32; n];
    let skip = (FS * 0.1) as usize;
    let rms = |v: &[f32]| {
        let s: f64 = v[skip..].iter().map(|x| (*x as f64) * (*x as f64)).sum();
        (s / (n - skip) as f64).sqrt()
    };
    let run_db = |core: &mut BandPassCore, ol: &mut [f32], or: &mut [f32]| {
        let mut pos = 0;
        while pos < n {
            let b = BLOCK.min(n - pos);
            core.update_smoothing(b);
            core.process_stereo(
                &input[pos..pos + b],
                &input[pos..pos + b],
                &mut ol[pos..pos + b],
                &mut or[pos..pos + b],
                None,
                None,
                b,
            );
            pos += b;
        }
        let in_rms = rms(&input);
        (
            20.0 * (rms(ol) / in_rms + 1e-12).log10(),
            20.0 * (rms(or) / in_rms + 1e-12).log10(),
        )
    };

    let (l1, r1) = run_db(&mut core, &mut ol, &mut or);
    println!("unlinked: L={l1:.1} dB  R={r1:.1} dB");
    assert!(l1 > -14.0 && r1 < -90.0, "LINK 关时左右斜率必须独立");

    p.linked = true;
    p.slope_db_r = 96.0;
    core.set_params(&p);
    let (l2, r2) = run_db(&mut core, &mut ol, &mut or);
    println!("linked:   L={l2:.1} dB  R={r2:.1} dB");
    assert!(
        (r2 - l2).abs() < 1.0 && r2 > -14.0,
        "LINK 开时 R 的斜率应跟随 L"
    );
}
