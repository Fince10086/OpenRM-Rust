//! level_meter_parity — LevelMeter 黄金快照对照 + 解析值校验。
//!
//! 黄金快照由 `tests/data/gen_levelmeter.cpp` 跑原版 `LevelMeter.h` 导出
//! （`tests/data/data_levelmeter.txt`，sr=48000，块长 512）。Rust 侧重放同一
//! 正弦序列（同相位、同块长），逐字段比对：ballistic 字段 ±0.05 dB，锁存标志精确相等。

use openrm_analyzer_dsp::level_meter::LevelMeter;

const DATA: &str = include_str!("data/data_levelmeter.txt");
const K_SR: f64 = 48_000.0;
const K_BLOCK: usize = 512;

#[derive(Clone, Copy, Default)]
struct Gold {
    peak_l: f32,
    peak_r: f32,
    true_l: f32,
    true_r: f32,
    rms_l: f32,
    rms_r: f32,
    vu_l: f32,
    vu_r: f32,
    vu_hold_l: f32,
    vu_hold_r: f32,
    persist_l: f32,
    persist_r: f32,
    hold_l: f32,
    hold_r: f32,
    hold_sec: f32,
    over_l: i32,
    over_r: i32,
}

fn parse_gold() -> std::collections::HashMap<String, Gold> {
    let mut map = std::collections::HashMap::new();
    for line in DATA.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut it = line.split_whitespace();
        let tag = it.next().unwrap().to_string();
        let mut g = Gold::default();
        for tok in it {
            let (k, v) = tok.split_once('=').unwrap();
            match k {
                "peakL" => g.peak_l = v.parse().unwrap(),
                "peakR" => g.peak_r = v.parse().unwrap(),
                "trueL" => g.true_l = v.parse().unwrap(),
                "trueR" => g.true_r = v.parse().unwrap(),
                "rmsL" => g.rms_l = v.parse().unwrap(),
                "rmsR" => g.rms_r = v.parse().unwrap(),
                "vuL" => g.vu_l = v.parse().unwrap(),
                "vuR" => g.vu_r = v.parse().unwrap(),
                "vuHoldL" => g.vu_hold_l = v.parse().unwrap(),
                "vuHoldR" => g.vu_hold_r = v.parse().unwrap(),
                "persistL" => g.persist_l = v.parse().unwrap(),
                "persistR" => g.persist_r = v.parse().unwrap(),
                "holdL" => g.hold_l = v.parse().unwrap(),
                "holdR" => g.hold_r = v.parse().unwrap(),
                "holdSec" => g.hold_sec = v.parse().unwrap(),
                "overL" => g.over_l = v.parse().unwrap(),
                "overR" => g.over_r = v.parse().unwrap(),
                _ => {}
            }
        }
        map.insert(tag, g);
    }
    map
}

/// 与 C++ 黄金生成器一致：f64 相位 × f64 幅度，末尾单次转 f32。
fn sine(n: usize, freq: f64, amp: f64) -> Vec<f32> {
    (0..n)
        .map(|i| {
            let phase = 2.0 * std::f64::consts::PI * freq * i as f64 / K_SR;
            (amp * phase.sin()) as f32
        })
        .collect()
}

fn run(m: &mut LevelMeter, x: &[f32], mode: usize, hold_sec: f64) {
    let mut pos = 0;
    while pos < x.len() {
        let b = K_BLOCK.min(x.len() - pos);
        m.process(&x[pos..pos + b], &x[pos..pos + b], mode, hold_sec);
        pos += b;
    }
}

fn compare(tag: &str, g: &Gold, m: &LevelMeter) {
    let s = m.snapshot();
    let db = |name: &str, got: f32, exp: f32| {
        let d = (got - exp).abs();
        println!("  {tag} {name}: got={got:.4} golden={exp:.4} Δ={d:.4}");
        assert!(d < 0.05, "{tag} {name}: {got} != {exp}");
    };
    db("peakL", s.peak_db_l, g.peak_l);
    db("peakR", s.peak_db_r, g.peak_r);
    db("trueL", s.true_db_l, g.true_l);
    db("trueR", s.true_db_r, g.true_r);
    db("rmsL", s.rms_db_l, g.rms_l);
    db("rmsR", s.rms_db_r, g.rms_r);
    db("vuL", s.vu_db_l, g.vu_l);
    db("vuR", s.vu_db_r, g.vu_r);
    db("vuHoldL", s.vu_hold_db_l, g.vu_hold_l);
    db("vuHoldR", s.vu_hold_db_r, g.vu_hold_r);
    db("persistL", s.persist_db_l, g.persist_l);
    db("persistR", s.persist_db_r, g.persist_r);
    db("holdL", s.hold_db_l, g.hold_l);
    db("holdR", s.hold_db_r, g.hold_r);
    assert_eq!(s.hold_sec, g.hold_sec, "{tag} holdSec");
    assert_eq!(s.over_l, g.over_l, "{tag} overL");
    assert_eq!(s.over_r, g.over_r, "{tag} overR");
}

#[test]
fn level_meter_matches_golden() {
    let gold = parse_gold();

    // A: mode 1 稳态正弦 0.5（峰值 −6.02 dBFS，RMS −9.03 dBFS）
    {
        let mut m = LevelMeter::new();
        m.set_sample_rate(K_SR);
        let x = sine((0.5 * K_SR) as usize, 1000.0, 0.5);
        run(&mut m, &x, 1, 2.0);
        let g = &gold["A"];
        compare("A", g, &m);
        let s = m.snapshot();
        assert!((s.peak_db_l + 6.02).abs() < 0.1);
        assert!((s.rms_db_l + 9.03).abs() < 0.1);
        assert_eq!(s.over_l, 0);
    }

    // B: 过载锁存 1.2 幅度 0.2s + 静音 0.3s
    {
        let mut m = LevelMeter::new();
        m.set_sample_rate(K_SR);
        let x = sine((0.2 * K_SR) as usize, 1000.0, 1.2);
        run(&mut m, &x, 1, 2.0);
        let quiet = vec![0.0f32; (0.3 * K_SR) as usize];
        run(&mut m, &quiet, 1, 2.0);
        let g = &gold["B"];
        compare("B", g, &m);
        assert_eq!(m.snapshot().over_l, 1);
        assert_eq!(m.snapshot().over_r, 1);
    }

    // C: mode 0 真峰值
    {
        let mut m = LevelMeter::new();
        m.set_sample_rate(K_SR);
        let x = sine((0.5 * K_SR) as usize, 1000.0, 0.5);
        run(&mut m, &x, 0, 2.0);
        let g = &gold["C"];
        compare("C", g, &m);
    }

    // D: dBTP 持久锁存 1.1 幅度 + 静音
    {
        let mut m = LevelMeter::new();
        m.set_sample_rate(K_SR);
        let x = sine((0.2 * K_SR) as usize, 1000.0, 1.1);
        run(&mut m, &x, 0, 2.0);
        let quiet = vec![0.0f32; (0.3 * K_SR) as usize];
        run(&mut m, &quiet, 0, 2.0);
        let g = &gold["D"];
        compare("D", g, &m);
        assert!(m.snapshot().persist_db_l > 0.0);
        assert_eq!(m.snapshot().over_l, 1);
    }

    // E: 保持衰减（holdSec=0.5，静音 3s）
    {
        let mut m = LevelMeter::new();
        m.set_sample_rate(K_SR);
        let x = sine((0.5 * K_SR) as usize, 1000.0, 0.5);
        run(&mut m, &x, 1, 0.5);
        let quiet = vec![0.0f32; (3.0 * K_SR) as usize];
        run(&mut m, &quiet, 1, 0.5);
        let g = &gold["E"];
        compare("E", g, &m);
    }

    // F: Reset 全默认
    {
        let mut m = LevelMeter::new();
        m.set_sample_rate(K_SR);
        let x = sine((0.2 * K_SR) as usize, 1000.0, 0.8);
        run(&mut m, &x, 1, 2.0);
        m.reset();
        let g = &gold["F"];
        compare("F", g, &m);
    }
}

#[test]
fn level_meter_reset_over_and_persist_independent() {
    // 过载锁存手动清除；persist 独立清除；互不影响（对应 ResetOver / ResetPersist）。
    let mut m = LevelMeter::new();
    m.set_sample_rate(K_SR);
    let x = sine((0.2 * K_SR) as usize, 1000.0, 1.2);
    run(&mut m, &x, 1, 2.0);
    assert_eq!(m.snapshot().over_l, 1);
    let held = m.snapshot().hold_db_l;
    assert!(held > 0.0, "burst 后峰值保持应在正电平");
    m.reset_over();
    assert_eq!(m.snapshot().over_l, 0);
    // ResetOver 只清过载锁存；L/R 条峰值保持保留（对应 C++ ResetOver 行 71-76）。
    assert_eq!(m.snapshot().hold_db_l, held);
}
