//! 主频谱控件模型层单测（不依赖 GUI）。
//!
//! 把已知线性幅度数据送进 `SpectrumPad::update`，断言：
//! * attack/release 系数 = `exp(-hop/sr/τ)`；
//! * 首帧平滑 dB 与手算的单极点上升一致；
//! * `StepSmoothed` 非对称 attack/release（LOG 单极点 / LIN 匀速 dB/s）；
//! * 峰值保持：命中即锁存、即时回落帧保持、超龄后按 `20·dt` 老化；
//! * tilt 斜率 = `slopePerOct·log2(f/632.45553)`；
//! * `MergeDb` 功率和 / 信号和两档。
//!
//! 常量与公式逐项对齐原版 `controls/SpectrumPad.h`。

use egui::{Pos2, Rect};
use openrm_analyzer_dsp::fastmath::fast_log2;
use orm_analyzer::ui::data::{LevelSnapshot, LoudnessSnapshot, SpectrumFrame};
use orm_analyzer::ui::spectrum::{mode, MeterClick, SpectrumPad};

const K_HOP: f64 = 1024.0;
const K_SLOPE_REF_HZ: f32 = 632.455_53;

fn level_snap() -> LevelSnapshot {
    LevelSnapshot {
        peak_db_l: -120.0,
        peak_db_r: -120.0,
        true_db_l: -120.0,
        true_db_r: -120.0,
        rms_db_l: -120.0,
        rms_db_r: -120.0,
        vu_db_l: -120.0,
        vu_db_r: -120.0,
        vu_hold_db_l: -120.0,
        vu_hold_db_r: -120.0,
        persist_db_l: -120.0,
        persist_db_r: -120.0,
        hold_db_l: -1000.0,
        hold_db_r: -1000.0,
        hold_sec: 2.0,
        over_l: 0,
        over_r: 0,
    }
}

fn loud_snap() -> LoudnessSnapshot {
    LoudnessSnapshot {
        momentary: -120.0,
        short_term: -120.0,
        integrated: -120.0,
        range: 0.0,
        lra_min: -120.0,
        lra_max: -120.0,
        tp_max: -120.0,
        i_valid: false,
        lra_valid: false,
    }
}

fn test_pad() -> SpectrumPad {
    SpectrumPad::new(Rect::from_min_max(
        Pos2::new(20.0, 58.0),
        Pos2::new(784.0, 328.0),
    ))
}

fn frame3(amp: f32, n: usize) -> SpectrumFrame {
    SpectrumFrame {
        l: vec![amp; n],
        r: vec![amp; n],
        m: vec![amp; n],
        freqs: Vec::new(),
    }
}

#[test]
fn attack_release_coefficients_match_exp_tau() {
    let mut pad = test_pad();
    pad.set_sample_rate(48_000.0);
    pad.set_release(0.2);
    pad.update(&SpectrumFrame::default(), &level_snap(), &loud_snap());

    let period = K_HOP / 48_000.0;
    let expect_a = (-period / 0.05_f64).exp() as f32;
    let expect_r = (-period / 0.2_f64).exp() as f32;
    assert!((pad.attack_coeff() - expect_a).abs() < 1e-6);
    assert!((pad.release_coeff() - expect_r).abs() < 1e-6);
    eprintln!(
        "[coeffs] attack={:.6} (expect {:.6}), release={:.6} (expect {:.6})",
        pad.attack_coeff(),
        expect_a,
        pad.release_coeff(),
        expect_r
    );
}

#[test]
fn first_frame_smoothed_db_matches_attack_pole() {
    let mut pad = test_pad();
    pad.set_sample_rate(48_000.0);
    pad.set_mode(mode::RTA);
    pad.set_rta_bands(&[100.0, 1000.0]);

    let amp = 0.5_f32; // 线性幅度 → -6.02 dB
    let raw_db = 6.020_599_913_28 * fast_log2(amp);
    let frame = frame3(amp, 2);
    pad.update(&frame, &level_snap(), &loud_snap());

    let a = pad.attack_coeff();
    let over = 0.1 * (9.0 - (-100.0));
    let target = raw_db.clamp(-100.0 - over, 9.0);
    let expected = a * (-150.0) + (1.0 - a) * target;
    assert!(
        (pad.spectrum_db(0, 0) - expected).abs() < 1e-3,
        "got {}, expected {}",
        pad.spectrum_db(0, 0),
        expected
    );
    eprintln!(
        "[first-frame dB] smoothed={:.4} dB (target {:.4}, attack pole {:.4})",
        pad.spectrum_db(0, 0),
        target,
        a
    );
}

#[test]
fn step_smoothed_asymmetric_attack_release() {
    // 上升：attack 系数单极点
    let rise = SpectrumPad::step_smoothed_db(-10.0, 0.0, 0.2, 0.9, 5.0, 0);
    assert!((rise - (-2.0)).abs() < 1e-6);

    // 下降（LOG 模式）：release 系数单极点
    let fall_log = SpectrumPad::step_smoothed_db(0.0, -20.0, 0.2, 0.9, 5.0, 0);
    assert!((fall_log - (-2.0)).abs() < 1e-6);

    // 下降（LIN 模式）：匀速 dB/s，单步最多掉 unifStepDb
    let fall_lin = SpectrumPad::step_smoothed_db(0.0, -20.0, 0.2, 0.9, 5.0, 1);
    assert!((fall_lin - (-5.0)).abs() < 1e-6);
    eprintln!(
        "[step smoothed] rise={rise:.2} dB, fall(LOG)={fall_log:.2} dB, fall(LIN)={fall_lin:.2} dB"
    );
}

#[test]
fn peak_hold_locks_retains_then_ages_out() {
    let mut pad = test_pad();
    pad.set_sample_rate(48_000.0);
    pad.set_mode(mode::RTA);
    pad.set_rta_bands(&[100.0]);

    let mut level = level_snap();
    level.hold_sec = 0.5;

    // 1.0 幅度 ≈ 0 dB：多帧喂入让 attack 极点收敛到近 0 dB，命中保持线
    for _ in 0..30 {
        pad.update(&frame3(1.0, 1), &level, &loud_snap());
    }
    assert!(pad.hold_signal());
    let hold0 = pad.hold_db(0, 0);
    assert!(hold0 > -1.0, "hold0={hold0}");

    // 低幅帧即时到来：保持线不被拉低
    pad.update(&frame3(0.01, 1), &level, &loud_snap());
    assert!(pad.hold_db(0, 0) > pad.spectrum_db(0, 0));

    // 超龄 1.0s（> 0.5s 保持档）：按 20 dB/s 回落
    pad.peak_hold_tick(1.0);
    let hold1 = pad.hold_db(0, 0);
    let cur = pad.spectrum_db(0, 0);
    let expected = cur.max(hold0 - 20.0 * 1.0);
    assert!((hold1 - expected).abs() < 1e-3, "hold1={hold1}, expected={expected}");
    eprintln!(
        "[peak hold] hold0={:.4} dB, cur(after low frame)={:.4} dB, hold1(after 1.0s aging)={:.4} dB, expected={:.4}",
        hold0, cur, hold1, expected
    );
}

#[test]
fn tilt_slope_gain_matches_reference_freq() {
    let mut pad = test_pad();
    pad.set_mode(mode::VQT);
    pad.set_vqt_bands(&[1000.0]);
    pad.set_slope(3.0);

    let expected = 3.0 * fast_log2(1000.0 / K_SLOPE_REF_HZ);
    assert!((pad.slope_at(0) - expected).abs() < 1e-4);
    eprintln!("[tilt] 1000 Hz slope={:.4} dB/oct (expect {:.4})", pad.slope_at(0), expected);
}

#[test]
fn merge_db_power_and_sum_modes() {
    // 两路 -6 dB 等幅：功率和 = -6 + 3.0103 ≈ -2.9897 dB；信号和 ≈ 0 dB
    let pwr = SpectrumPad::merge_db(-6.0, -6.0, 0);
    assert!((pwr - (-6.0 + 3.010_299_956_64)).abs() < 1e-3, "pwr={pwr}");
    let sum = SpectrumPad::merge_db(-6.0, -6.0, 1);
    assert!(sum.abs() < 0.05, "sum={sum}");
}

#[test]
fn click_gain_bar_resets_persist_in_dbtp_mode() {
    let mut pad = test_pad();
    // rect: L=20, R=784；strip 宽 200 → 增益条列 x ∈ [584, 612]
    let pos = Pos2::new(590.0, 150.0);
    match pad.click(pos) {
        Some(MeterClick::ResetPersist) => {}
        other => panic!("expected ResetPersist, got {other:?}"),
    }
}
