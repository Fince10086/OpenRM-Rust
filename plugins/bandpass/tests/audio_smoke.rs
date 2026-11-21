//! 端到端冒烟测试：宿主参数 → 插件 → DSP → 输出。
//!
//! 目的是验证"接线"（参数 id、单位换算、块边界、单/双声道分支），
//! 算法精度由 `crates/openrm-dsp/tests/bandpass_parity.rs` 负责。

use std::time::Duration;

use orm_bandpass::params::BandPassParamsParamId as P;
use orm_bandpass::Plugin;
use truce_test::{driver, InputSource};

const FS: f64 = 48000.0;

fn rms(v: &[f32]) -> f64 {
    let s: f64 = v.iter().map(|x| (*x as f64) * (*x as f64)).sum();
    (s / v.len().max(1) as f64).sqrt()
}

/// 1 kHz sine，满幅。
fn tone() -> InputSource {
    InputSource::Generator(Box::new(|frame: usize, sr: f64| {
        (2.0 * std::f64::consts::PI * 1000.0 * frame as f64 / sr).sin() as f32
    }))
}

#[test]
fn center_passes_at_unity() {
    let result = driver!(Plugin)
        .sample_rate(FS)
        .duration(Duration::from_secs(1))
        .input(tone())
        .set_param(P::FreqL, 0.5663) // 1000 Hz (log 20..20000)
        .set_param(P::BwL, 0.1009) // 1 octave (multiplier 1..31, log)
        .set_param(P::GainL, 0.8889) // 0 dB (-96..12)
        .set_param(P::Mix, 1.0)
        .set_param(P::PassL, 1.0)
        .run();

    let tail = &result.output[0][result.output[0].len() / 2..];
    let gain = rms(tail) / (1.0 / 2f64.sqrt());
    println!("center gain = {gain:.3}");
    assert!(
        (gain - 1.0).abs() < 0.06,
        "1 kHz 处通带增益应归一，实测 {gain:.3}"
    );
}

#[test]
fn mix_zero_is_passthrough() {
    let result = driver!(Plugin)
        .sample_rate(FS)
        .duration(Duration::from_secs(1))
        .input(tone())
        .set_param(P::FreqL, 0.5663)
        .set_param(P::Mix, 0.0)
        .run();

    let tail = &result.output[0][result.output[0].len() / 2..];
    let gain = rms(tail) / (1.0 / 2f64.sqrt());
    println!("dry gain = {gain:.3}");
    assert!((gain - 1.0).abs() < 0.01, "mix=0 必须全干直通");
}
