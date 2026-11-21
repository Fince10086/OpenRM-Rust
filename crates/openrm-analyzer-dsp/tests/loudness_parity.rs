//! loudness_parity — LoudnessMeter 离线回归，逐条移植自
//! `OpenRM/tests/loudness_test.cpp`（BS.1770-4 / EBU R128 / EBU Tech 3342）。
//!
//! 保留原版期望值与容差；每个用例输出实测值。
//! 覆盖：校准（1kHz 0 dBFS ≈ -3.01 LUFS）、12 LU 线性、低频滚降、
//! 绝对/相对门限、LRA、M/S 时间响应、Reset 与采样率不变性、TP 锁存、不规则块长。

use openrm_analyzer_dsp::loudness_meter::{k_weight_coeffs, LoudnessMeter};

const K_FS: f64 = 48_000.0;
const PI: f64 = 3.141_592_653_589_793_238_46;

/// 滤波器 H(e^{jω}) 幅度（dB）：由 b/a 系数直接求值，独立于引擎时域模拟。
/// 与 C++ `FilterMagnitudeDb` 逐行对应。
fn filter_magnitude_db(b: &[f64; 5], a: &[f64; 5], sr: f64, hz: f64) -> f64 {
    let w = 2.0 * PI * hz / sr;
    let c1 = w.cos();
    let s1 = w.sin();
    let mut zr = 1.0;
    let mut zi = 0.0;
    let mut nr = 0.0;
    let mut ni = 0.0;
    let mut dr = 0.0;
    let mut di = 0.0;
    for k in 0..5 {
        nr += b[k] * zr;
        ni += b[k] * zi;
        dr += a[k] * zr;
        di += a[k] * zi;
        let tzr = zr * c1 + zi * s1;
        zi = zi * c1 - zr * s1;
        zr = tzr;
    }
    10.0 * ((nr * nr + ni * ni) / (dr * dr + di * di)).log10()
}

/// 期望响度（LUFS）：双声道同电平时 z = G²·10^(level/10)，
/// 即 L = -0.691 + |H|²(dB) + levelDb。
fn expected_lufs(sr: f64, hz: f64, level_db: f64) -> f64 {
    let (b, a) = k_weight_coeffs(sr);
    -0.691 + filter_magnitude_db(&b, &a, sr, hz) + level_db
}

/// 生成正弦并喂入引擎（mono=true 时只有 L 有声，R 全零 —— 标准单声道校准点）。
fn process_sine(
    lm: &mut LoudnessMeter,
    seconds: f64,
    level_db: f64,
    freq_hz: f64,
    block: usize,
    mono: bool,
    sr: f64,
) {
    let n = (seconds * sr).round() as usize;
    let amp = 10.0_f64.powf(level_db / 20.0);
    let mut l = Vec::with_capacity(n);
    let mut r = Vec::with_capacity(n);
    for i in 0..n {
        let v = (amp * (2.0 * PI * freq_hz * i as f64 / sr).sin()) as f32;
        l.push(v);
        r.push(if mono { 0.0 } else { v });
    }
    let mut pos = 0;
    while pos < n {
        let b = block.min(n - pos);
        lm.process(&l[pos..pos + b], &r[pos..pos + b]);
        pos += b;
    }
}

#[test]
fn loudness_calibration_and_msi() {
    // ── 1. 校准与 M/S/I 一致性（1kHz 正弦 0 dBFS，10s）──
    let mut lm = LoudnessMeter::new();
    lm.set_sample_rate(K_FS);
    process_sine(&mut lm, 10.0, 0.0, 1000.0, 512, false, K_FS);
    let s = lm.store();
    let exp0 = expected_lufs(K_FS, 1000.0, 0.0);
    println!(
        "1a: I={:.4} LUFS, 期望(H预测)={:.4}, 差={:.4} (容差 0.10)",
        s.integrated,
        exp0,
        (s.integrated as f64 - exp0).abs()
    );
    assert!((s.integrated as f64 - exp0).abs() < 0.10);

    println!(
        "1b: M={:.4}, S={:.4}, I={:.4} (稳态正弦 M/S 应与 I 一致, 容差 0.10)",
        s.momentary, s.short_term, s.integrated
    );
    assert!((s.momentary - s.integrated).abs() < 0.10);
    assert!((s.short_term - s.integrated).abs() < 0.10);

    // 标准校准点：单声道 1kHz 0 dBFS → -3.01 LUFS（ITU-R BS.1770-4 §3）
    let mut lm_mono = LoudnessMeter::new();
    lm_mono.set_sample_rate(K_FS);
    process_sine(&mut lm_mono, 10.0, 0.0, 1000.0, 512, true, K_FS);
    let sm = lm_mono.store();
    println!(
        "1c: 单声道 1kHz 0dBFS I={:.4} LUFS, 期望 -3.01, 差={:.4} (容差 0.15)",
        sm.integrated,
        (sm.integrated + 3.01).abs()
    );
    assert!((sm.integrated + 3.01).abs() < 0.15);

    println!("1d: iValid={} (10s 后应为 true)", s.i_valid);
    assert!(s.i_valid);
}

#[test]
fn loudness_lra_valid_threshold() {
    // ── 1e/1f. LRA 有效位阈值：3s 前无效，3s 后有效（插件惯例，早于 60s 参考）──
    let mut before = LoudnessMeter::new();
    before.set_sample_rate(K_FS);
    process_sine(&mut before, 2.0, -20.0, 1000.0, 512, false, K_FS);
    let sb = before.store();
    println!(
        "1e: 2s: iValid={}, lraValid={} (期望 iValid=true, lraValid=false)",
        sb.i_valid, sb.lra_valid
    );
    assert!(sb.i_valid && !sb.lra_valid);

    let mut after = LoudnessMeter::new();
    after.set_sample_rate(K_FS);
    process_sine(&mut after, 10.0, -20.0, 1000.0, 512, false, K_FS);
    let sa = after.store();
    println!(
        "1f: 10s: iValid={}, lraValid={} (期望均 true)",
        sa.i_valid, sa.lra_valid
    );
    assert!(sa.i_valid && sa.lra_valid);
}

#[test]
fn loudness_level_linearity() {
    // ── 2. 线性：-8 与 -20 dBFS 差 12 LU ──
    let mut lm8 = LoudnessMeter::new();
    lm8.set_sample_rate(K_FS);
    process_sine(&mut lm8, 10.0, -8.0, 1000.0, 512, false, K_FS);
    let mut lm20 = LoudnessMeter::new();
    lm20.set_sample_rate(K_FS);
    process_sine(&mut lm20, 10.0, -20.0, 1000.0, 512, false, K_FS);
    let s8 = lm8.store();
    let s20 = lm20.store();
    let exp20 = expected_lufs(K_FS, 1000.0, -20.0);
    println!(
        "2a: -20dBFS I={:.4}, 期望={:.4}, 差={:.4} (容差 0.10)",
        s20.integrated,
        exp20,
        (s20.integrated as f64 - exp20).abs()
    );
    assert!((s20.integrated as f64 - exp20).abs() < 0.10);

    let step = s8.integrated - s20.integrated;
    println!("2b: 电平差 12 dB → 响度差 {:.4} LU (期望 12.0, 容差 0.05)", step);
    assert!((step - 12.0).abs() < 0.05);
}

#[test]
fn loudness_k_weighting_rolloff() {
    // ── 3. K 加权低频滚降（50Hz / 100Hz vs 1kHz，同电平）──
    let mut lm100 = LoudnessMeter::new();
    lm100.set_sample_rate(K_FS);
    process_sine(&mut lm100, 10.0, 0.0, 100.0, 512, false, K_FS);
    let s100 = lm100.store();
    let exp100 = expected_lufs(K_FS, 100.0, 0.0);
    println!(
        "3a: 100Hz I={:.4}, 期望={:.4}, 差={:.4} (容差 0.10)",
        s100.integrated,
        exp100,
        (s100.integrated as f64 - exp100).abs()
    );
    assert!((s100.integrated as f64 - exp100).abs() < 0.10);

    let mut lm50 = LoudnessMeter::new();
    lm50.set_sample_rate(K_FS);
    process_sine(&mut lm50, 10.0, 0.0, 50.0, 512, false, K_FS);
    let s50 = lm50.store();
    let exp1k = expected_lufs(K_FS, 1000.0, 0.0);
    let exp50 = expected_lufs(K_FS, 50.0, 0.0);
    println!(
        "3b: 50Hz I={:.4}, 期望={:.4}, 差={:.4} (容差 0.10)",
        s50.integrated,
        exp50,
        (s50.integrated as f64 - exp50).abs()
    );
    assert!((s50.integrated as f64 - exp50).abs() < 0.10);

    let drop = exp1k - s50.integrated as f64;
    let drop_pred = exp1k - exp50;
    println!(
        "3c: 50Hz 比 1kHz 低 {:.4} LU (预测 {:.4}, 应 ≥3 LU; 偏差容差 0.20)",
        drop, drop_pred
    );
    assert!(drop >= 3.0);
    assert!((drop - drop_pred).abs() < 0.20);
}

#[test]
fn loudness_absolute_and_relative_gate() {
    // ── 4. 门限：绝对（-90 dBFS 段）与相对（-60 dBFS 段）──
    let mut lma = LoudnessMeter::new();
    lma.set_sample_rate(K_FS);
    process_sine(&mut lma, 5.0, -90.0, 1000.0, 512, false, K_FS); // 低于绝对门限
    process_sine(&mut lma, 10.0, -20.0, 1000.0, 512, false, K_FS);
    let mut lmb = LoudnessMeter::new();
    lmb.set_sample_rate(K_FS);
    process_sine(&mut lmb, 10.0, -20.0, 1000.0, 512, false, K_FS); // 对照：只有 -20 段
    let sa = lma.store();
    let sb = lmb.store();
    println!(
        "4a: 绝对门限剔除 -90dBFS: A.I={:.4}, B.I={:.4}, 差={:.4} (容差 0.15), iValid={}",
        sa.integrated,
        sb.integrated,
        (sa.integrated - sb.integrated).abs(),
        sa.i_valid
    );
    assert!((sa.integrated - sb.integrated).abs() < 0.15 && sa.i_valid);

    let mut lmc = LoudnessMeter::new();
    lmc.set_sample_rate(K_FS);
    process_sine(&mut lmc, 30.0, -10.0, 1000.0, 512, false, K_FS);
    process_sine(&mut lmc, 20.0, -60.0, 1000.0, 512, false, K_FS); // 高于绝对门限, 被相对门限剔除
    let mut lmd = LoudnessMeter::new();
    lmd.set_sample_rate(K_FS);
    process_sine(&mut lmd, 30.0, -10.0, 1000.0, 512, false, K_FS);
    let sc = lmc.store();
    let sd = lmd.store();
    println!(
        "4b: 相对门限剔除 -60dBFS: C.I={:.4}, D.I={:.4}, 差={:.4} (容差 0.15), iValid={}",
        sc.integrated,
        sd.integrated,
        (sc.integrated - sd.integrated).abs(),
        sc.i_valid
    );
    assert!((sc.integrated - sd.integrated).abs() < 0.15 && sc.i_valid);
}

#[test]
fn loudness_lra_two_level_and_flat() {
    // ── 5. LRA：双电平 ≈ 16 LU；恒电平 ≈ 0 ──
    let mut lm = LoudnessMeter::new();
    lm.set_sample_rate(K_FS);
    for _ in 0..4 {
        process_sine(&mut lm, 8.0, -10.0, 1000.0, 512, false, K_FS); // 32s
        process_sine(&mut lm, 8.0, -26.0, 1000.0, 512, false, K_FS); // 32s
    }
    process_sine(&mut lm, 2.0, -20.0, 1000.0, 512, false, K_FS); // 总计 66s ≥ 60s
    let s = lm.store();
    println!(
        "5a: 双电平节目 LRA={:.4} LU (期望 [12,19]), lraValid={}, 端点 {:.4}..{:.4}",
        s.range, s.lra_valid, s.lra_min, s.lra_max
    );
    assert!(s.lra_valid && s.range >= 12.0 && s.range <= 19.0);

    let mut flat = LoudnessMeter::new();
    flat.set_sample_rate(K_FS);
    process_sine(&mut flat, 70.0, -20.0, 1000.0, 512, false, K_FS);
    let sf = flat.store();
    println!(
        "5b: 恒电平节目 LRA={:.4} LU (期望 <0.3), lraValid={}",
        sf.range, sf.lra_valid
    );
    assert!(sf.lra_valid && sf.range < 0.3);
}

#[test]
fn loudness_ms_time_response() {
    // ── 6. M/S 时间响应：阶跃后 400ms / 3s 收敛 ──
    let mut lm = LoudnessMeter::new();
    lm.set_sample_rate(K_FS);
    process_sine(&mut lm, 1.0, -120.0, 1000.0, 512, false, K_FS); // 数字静音
    let mut s = lm.store();
    println!(
        "6a: 静音 M={:.4}, S={:.4} (期望 <-110)",
        s.momentary, s.short_term
    );
    assert!(s.momentary < -110.0 && s.short_term < -110.0);

    let full = expected_lufs(K_FS, 1000.0, -20.0);
    process_sine(&mut lm, 0.3, -20.0, 1000.0, 512, false, K_FS); // 信号 0.3s
    let partial = full + 10.0 * (0.3_f64 / 0.4).log10(); // 能量占比 3/4
    s = lm.store();
    println!(
        "6b: M 部分填充窗 M={:.4}, 期望={:.4}, 差={:.4} (容差 0.30)",
        s.momentary,
        partial,
        (s.momentary as f64 - partial).abs()
    );
    assert!((s.momentary as f64 - partial).abs() < 0.30);

    process_sine(&mut lm, 0.7, -20.0, 1000.0, 512, false, K_FS); // 信号 1.0s: M 窗全填
    s = lm.store();
    println!(
        "6c: 1.0s 后 M={:.4}, 期望={:.4}, 差={:.4} (容差 0.20)",
        s.momentary,
        full,
        (s.momentary as f64 - full).abs()
    );
    assert!((s.momentary as f64 - full).abs() < 0.20);

    process_sine(&mut lm, 2.5, -20.0, 1000.0, 512, false, K_FS); // 信号 3.5s: S 窗全填
    s = lm.store();
    println!(
        "6d: 3.5s 后 S={:.4}, 期望={:.4}, 差={:.4} (容差 0.20)",
        s.short_term,
        full,
        (s.short_term as f64 - full).abs()
    );
    assert!((s.short_term as f64 - full).abs() < 0.20);
}

#[test]
fn loudness_reset_tp_latch_block_sizes() {
    // ── 7. Reset / TP 锁存 / 不规则宿主块长 ──
    let mut lm = LoudnessMeter::new();
    lm.set_sample_rate(K_FS);
    process_sine(&mut lm, 10.0, -20.0, 1000.0, 512, false, K_FS);
    lm.set_true_peaks(-2.0, -3.0);
    lm.set_true_peaks(-0.5, -1.0);
    let mut s = lm.store();
    println!("7a: TP 锁存 tpMax={:.4} (期望 -0.5)", s.tp_max);
    assert!((s.tp_max + 0.5).abs() < 1e-4);

    lm.reset();
    s = lm.store();
    println!(
        "7b: Reset 后 iValid={}, lraValid={}, tpMax={:.4} (期望 false/false/<-119)",
        s.i_valid, s.lra_valid, s.tp_max
    );
    assert!(!s.i_valid && !s.lra_valid && s.tp_max < -119.0);

    process_sine(&mut lm, 5.0, -20.0, 1000.0, 512, false, K_FS);
    s = lm.store();
    let exp20 = expected_lufs(K_FS, 1000.0, -20.0);
    println!(
        "7c: Reset 后恢复 I={:.4}, 期望={:.4}, 差={:.4} (容差 0.20)",
        s.integrated,
        exp20,
        (s.integrated as f64 - exp20).abs()
    );
    assert!((s.integrated as f64 - exp20).abs() < 0.20);

    // 不规则宿主块长（3..17 循环）：引擎内部凑 100ms 子块，结果与定长块一致
    let n = (10.0 * K_FS).round() as usize;
    let amp = 10.0_f64.powf(-20.0 / 20.0);
    let mut l = Vec::with_capacity(n);
    let mut r = Vec::with_capacity(n);
    for i in 0..n {
        let v = (amp * (2.0 * PI * 1000.0 * i as f64 / K_FS).sin()) as f32;
        l.push(v);
        r.push(v);
    }
    let mut lm_odd = LoudnessMeter::new();
    lm_odd.set_sample_rate(K_FS);
    let mut lm_ref = LoudnessMeter::new();
    lm_ref.set_sample_rate(K_FS);
    let mut pos = 0;
    while pos < n {
        let b = 512.min(n - pos);
        lm_ref.process(&l[pos..pos + b], &r[pos..pos + b]);
        pos += b;
    }
    let mut pos = 0;
    let mut blk = 3usize;
    while pos < n {
        let b = blk.min(n - pos);
        lm_odd.process(&l[pos..pos + b], &r[pos..pos + b]);
        pos += b;
        blk = (blk % 17) + 3;
    }
    let s_odd = lm_odd.store();
    let s_ref = lm_ref.store();
    println!(
        "7d: 不规则块长: I 差={:.5}, M 差={:.5}, S 差={:.5} (容差 0.02)",
        (s_odd.integrated - s_ref.integrated).abs(),
        (s_odd.momentary - s_ref.momentary).abs(),
        (s_odd.short_term - s_ref.short_term).abs()
    );
    assert!((s_odd.integrated - s_ref.integrated).abs() < 0.02);
    assert!((s_odd.momentary - s_ref.momentary).abs() < 0.02);
    assert!((s_odd.short_term - s_ref.short_term).abs() < 0.02);
}

#[test]
fn loudness_sample_rate_invariance() {
    // ── 8. 采样率不变性（数字域同电平 → 同 LU）──
    let srs = [44_100.0_f64, 48_000.0, 96_000.0];
    let mut iv = [0.0f64; 3];
    for (i, &sr) in srs.iter().enumerate() {
        let mut lm = LoudnessMeter::new();
        lm.set_sample_rate(sr);
        process_sine(&mut lm, 8.0, -20.0, 1000.0, 512, false, sr);
        let s = lm.store();
        iv[i] = s.integrated as f64;
        let exp = expected_lufs(sr, 1000.0, -20.0);
        println!(
            "8 @{:.0}Hz: I={:.4}, 期望={:.4}, 差={:.4} (容差 0.10)",
            sr,
            iv[i],
            exp,
            (iv[i] - exp).abs()
        );
        assert!((iv[i] - exp).abs() < 0.10);
    }
    println!(
        "8: 采样率间 spread = {:.4}, {:.4} LU (容差 0.10)",
        (iv[0] - iv[1]).abs(),
        (iv[1] - iv[2]).abs()
    );
    assert!((iv[0] - iv[1]).abs() < 0.10);
    assert!((iv[1] - iv[2]).abs() < 0.10);
}
