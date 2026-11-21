//! 示波器 / 声像相关度 模型层 parity 测试。
//!
//! 权威来源：
//! - `controls/OscilloscopeControl.h`：SYNC 自相关周期检测（:148-167 / :774-877）、
//!   ROLL 右缘映射（:142-147 / :227-245）。
//! - `controls/StereoCorr.h`：64-hop 滑窗相关度 / 平衡（:58-83）。
//!
//! 运行：`cargo test -p orm-analyzer --test scope_model`
//!
//! 断言口径：
//! - 周期 48 样本的正弦 → SYNC 检测 lag ≈ 48（抛物线亚采样插值，容差 ±2）；
//! - ROLL 窗口内逐点等于历史末 nDisp 个样本；
//! - 静音 → 示波器无周期（lag = -1），相关度 `valid = false`；
//! - DC（L = R = 常数）→ 滑窗相关度 = 1.0；
//! - 相关度手算：L = R → +1；R = -L → -1；奇偶交错正交 → 0。

use egui::Rect;

use orm_analyzer::ui::data::ScopeFrame;
use orm_analyzer::ui::field::StereoCorr;
use orm_analyzer::ui::scope::{chan, trig, Oscilloscope};

const SR: f64 = 48_000.0;
const HOP: usize = 1024;
const HOP_SEC: f64 = HOP as f64 / SR;

/// 全局下标连续的正弦帧（跨 hop 不重置相位）。
fn sine_frame(global_start: usize, period: usize) -> (Vec<f32>, Vec<f32>) {
    let l: Vec<f32> = (0..HOP)
        .map(|i| {
            let t = 2.0 * std::f32::consts::PI * (global_start + i) as f32 / period as f32;
            t.sin()
        })
        .collect();
    let r = l.clone();
    (l, r)
}

fn silence_frame() -> ScopeFrame {
    ScopeFrame {
        l: vec![0.0; HOP],
        r: vec![0.0; HOP],
    }
}

// ---------------------------------------------------------------------------
// 示波器：SYNC 周期锁定
// ---------------------------------------------------------------------------

#[test]
fn sync_locks_period_of_48_sample_sine() {
    let mut scope = Oscilloscope::new(Rect::ZERO);
    scope.set_sample_rate(SR);
    scope.set_trig_source(trig::SYNC);
    scope.set_chan_mask(chan::L);

    // 6 个 hop = 6144 样本，覆盖 4096 的自相关窗。
    for hop in 0..6 {
        let (l, r) = sine_frame(hop * HOP, 48);
        scope.update(&ScopeFrame { l, r });
    }

    let lag = scope.detected_lag();
    let period_ms = scope.detected_period_ms();
    let freq = scope.detected_freq();
    println!(
        "[sync] detected_lag={lag}  period_ms={period_ms:.3}  freq={freq:.2} Hz  (真值 48 样本 / 1.0 ms / 1000.0 Hz)"
    );

    assert!(lag > 8, "静音/平坦信号不应报周期；实际 lag={lag}");
    assert!(
        (lag - 48).abs() <= 2,
        "SYNC 周期偏离真值 48 超过容差 ±2：lag={lag}"
    );
    assert!((period_ms - 1.0).abs() < 0.15, "周期 ms 不符：{period_ms}");
    assert!((freq - 1000.0).abs() < 80.0, "频率不符：{freq}");
}

// ---------------------------------------------------------------------------
// 示波器：ROLL 右缘窗口映射
// ---------------------------------------------------------------------------

#[test]
fn roll_window_maps_latest_n_samples() {
    let mut scope = Oscilloscope::new(Rect::ZERO);
    scope.set_sample_rate(SR);
    scope.set_trig_source(trig::ROLL);
    scope.set_window_sec(0.010); // 10 ms → 480 样本
    scope.set_chan_mask(chan::L);

    // 两条 hop 的递增斜坡，样本值 = 下标 × 0.001，便于逐点手算比对。
    let total = 2 * HOP;
    let ramp: Vec<f32> = (0..total).map(|i| i as f32 * 0.001).collect();
    for w in ramp.chunks(HOP) {
        scope.update(&ScopeFrame {
            l: w.to_vec(),
            r: w.to_vec(),
        });
    }

    let disp = scope.disp_l().to_vec();
    let n_disp = disp.len();
    assert_eq!(n_disp, 480, "ROLL 窗长不符：{n_disp}");

    // 原版 ExtractWindow(:234-243)：readStart = rightEdge - nDisp，
    // 映射区间 [head-nDisp, head-1]（rightEdge 本身不计入窗内）。
    let head = total - 1;
    let expected: Vec<f32> = ramp[head - n_disp..head].to_vec();
    for (i, (got, want)) in disp.iter().zip(expected.iter()).enumerate() {
        assert!(
            (got - want).abs() < 1e-6,
            "ROLL 窗口第 {i} 点不符：got={got} want={want}"
        );
    }
    println!(
        "[roll] n_disp={n_disp}  disp[0]={}  disp[last]={}  (期望首点={} 末点={})",
        disp[0],
        disp[n_disp - 1],
        expected[0],
        expected[n_disp - 1]
    );
}

// ---------------------------------------------------------------------------
// 示波器：静音 → 无周期
// ---------------------------------------------------------------------------

#[test]
fn silence_yields_no_trig_period() {
    let mut scope = Oscilloscope::new(Rect::ZERO);
    scope.set_sample_rate(SR);
    scope.set_trig_source(trig::SYNC);
    scope.set_chan_mask(chan::L);

    for _ in 0..3 {
        scope.update(&silence_frame());
    }

    let lag = scope.detected_lag();
    println!("[silence] detected_lag={lag}");
    assert_eq!(lag, -1, "静音自相关能量为 0，不应报周期：lag={lag}");

    let mut corr = StereoCorr::new();
    let zeros = vec![0.0_f32; HOP];
    for _ in 0..2 {
        corr.update(&zeros, &zeros, HOP, HOP_SEC);
    }
    println!("[silence] corr.valid={}  raw_corr={}", corr.valid(), corr.raw_corr());
    assert!(!corr.valid(), "静音滑窗能量为 0，相关度应判无效");
}

// ---------------------------------------------------------------------------
// StereoCorr：DC 相关度 = 1.0
// ---------------------------------------------------------------------------

#[test]
fn dc_input_correlates_one() {
    let mut corr = StereoCorr::new();
    let dc = vec![0.5_f32; HOP];
    corr.update(&dc, &dc, HOP, HOP_SEC);

    println!(
        "[dc] valid={}  raw_corr={:.6}  disp_corr={:.6}  bal={:.3} dB",
        corr.valid(),
        corr.raw_corr(),
        corr.corr(),
        corr.bal()
    );
    assert!(corr.valid(), "DC 信号应判有效");
    assert!(
        (corr.raw_corr() - 1.0).abs() < 1e-6,
        "L=R=常数时相关度应为 1.0：raw={}",
        corr.raw_corr()
    );
    assert!(corr.bal().abs() < 0.01, "等电平 DC 平衡应≈0 dB：bal={}", corr.bal());
}

// ---------------------------------------------------------------------------
// StereoCorr：手算相关度三组对照
// ---------------------------------------------------------------------------

#[test]
fn correlation_matches_hand_computed_values() {
    // 1) L = R → +1
    let mut c = StereoCorr::new();
    let ones = vec![1.0_f32; HOP];
    c.update(&ones, &ones, HOP, HOP_SEC);
    let same = c.raw_corr();
    println!("[corr] L=R → raw={same:.6}（期望 1.0）");
    assert!((same - 1.0).abs() < 1e-6);

    // 2) R = -L → -1
    let mut c = StereoCorr::new();
    let neg: Vec<f32> = ones.iter().map(|v| -v).collect();
    c.update(&ones, &neg, HOP, HOP_SEC);
    let anti = c.raw_corr();
    println!("[corr] R=-L → raw={anti:.6}（期望 -1.0）");
    assert!((anti + 1.0).abs() < 1e-6);

    // 3) 奇偶交错正交：L=[1,0,1,0,…]，R=[0,1,0,1,…] → 0
    let mut c = StereoCorr::new();
    let l_alt: Vec<f32> = (0..HOP).map(|i| if i % 2 == 0 { 1.0 } else { 0.0 }).collect();
    let r_alt: Vec<f32> = (0..HOP).map(|i| if i % 2 == 0 { 0.0 } else { 1.0 }).collect();
    c.update(&l_alt, &r_alt, HOP, HOP_SEC);
    let orth = c.raw_corr();
    println!("[corr] 奇偶正交 → raw={orth:.6}（期望 0.0）");
    assert!(orth.abs() < 1e-6);
}
