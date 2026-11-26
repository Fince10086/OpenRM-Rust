//! 宿主参数 → FusionDsp → 输出 的接线冒烟测试。

use std::time::Duration;

use orm_fusion::Plugin;
use truce_test::{driver, InputSource};

const FS: f64 = 48_000.0;

fn rms(v: &[f32]) -> f64 {
    (v.iter().map(|x| (*x as f64) * (*x as f64)).sum::<f64>() / v.len().max(1) as f64).sqrt()
}

fn tone(freq: f64) -> InputSource {
    InputSource::Generator(Box::new(move |frame: usize, sr: f64| {
        (2.0 * std::f64::consts::PI * freq * frame as f64 / sr).sin() as f32
    }))
}

#[test]
fn default_band_passes_its_center() {
    // 默认 bar1：显示频率中心 0.125 → 实际 375 Hz；幅度 0.9 → −4 dB；
    // 合成窗 OLA 的固有增益约 1.495。
    let result = driver!(Plugin)
        .sample_rate(FS)
        .duration(Duration::from_secs(3))
        .input(tone(375.0))
        .run();
    let tail = &result.output[0][result.output[0].len() / 2..];
    let gain = rms(tail) / (1.0 / 2f64.sqrt());
    println!("default band gain = {gain:.3}");
    assert!((gain - 0.57).abs() < 0.15, "实际 {gain:.3}");
}

#[test]
fn legacy_global_mute_is_ignored() {
    let result = driver!(Plugin)
        .sample_rate(FS)
        .duration(Duration::from_secs(2))
        .input(tone(375.0))
        .set_param(89u32, 1.0)
        .run();
    let tail = &result.output[0][result.output[0].len() / 2..];
    println!("legacy mute rms = {}", rms(tail));
    assert!(rms(tail) > 1e-3);
}
