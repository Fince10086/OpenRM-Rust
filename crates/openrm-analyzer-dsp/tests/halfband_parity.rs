//! halfband_parity — HalfbandDec2 离线回归，逐条移植自
//! `OpenRM/plugins/Analyzer/src/dsp/HalfbandDec2.h`（303 行）。
//!
//! 黄金表由 `tests/data/gen_evidence.cpp` include 原版头文件导出
//! （`tests/data/data_halfband.txt`）：最小相位/线性相位两组 101 抽头、
//! 每级群延迟。另做冲激响应冒烟检查（输出长度、有限性、线性对称性）。
//!
//! 容差说明：线性相位原型为纯窗函数合成，抽头逐位一致（1e-6）。
//! 最小相位抽头来自倒频谱谱因式分解——阻带 |A|² ~1e-13，log/cepstral
//! 对 FFT 末位舍入敏感；两侧 FFT 均对直接 DFT 误差 ~1e-13（同量级），
//! 分解后抽头绝对偏差 ≤ ~1.2e-3（见报告“原版疑点/已知偏差”）。

use openrm_analyzer_dsp::halfband::{gd_per_stage, HalfbandDec2, K_N};

const DATA: &str = include_str!("data/data_halfband.txt");

/// 最小相位抽头允许的最大绝对偏差（实测 ~1.1e-3）。
const MP_TOL: f64 = 2e-3;
const LP_TOL: f64 = 1e-6;

fn parse() -> (f64, Vec<f64>, Vec<f64>) {
    let mut gd = 0.0_f64;
    let mut minphase: Vec<f64> = Vec::new();
    let mut linear: Vec<f64> = Vec::new();
    let mut section = "";
    for line in DATA.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(v) = line.strip_prefix("gd_per_stage ") {
            gd = v.parse().unwrap();
            continue;
        }
        if line.starts_with("=== MINPHASE") {
            section = "mp";
            continue;
        }
        if line.starts_with("=== LINEAR") {
            section = "lp";
            continue;
        }
        if let Some(rest) = line.strip_prefix("t[") {
            let (idx, val) = rest.split_once("]=").unwrap();
            let idx: usize = idx.parse().unwrap();
            let val: f64 = val.parse().unwrap();
            match section {
                "mp" => {
                    if idx >= minphase.len() {
                        minphase.resize(idx + 1, 0.0);
                    }
                    minphase[idx] = val;
                }
                _ => {
                    if idx >= linear.len() {
                        linear.resize(idx + 1, 0.0);
                    }
                    linear[idx] = val;
                }
            }
        }
    }
    (gd, minphase, linear)
}

#[test]
fn taps_and_group_delay_match_cpp() {
    let (gd_gold, mp_gold, lp_gold) = parse();
    assert_eq!(mp_gold.len(), K_N);
    assert_eq!(lp_gold.len(), K_N);

    let mut eng = HalfbandDec2::new();
    for (i, &g) in lp_gold.iter().enumerate() {
        let got = eng.taps()[i] as f64;
        assert!((got - g).abs() < LP_TOL, "linear t[{i}] = {got} vs {g}");
    }

    eng.set_min_phase(true);
    let mut max_diff = 0.0_f64;
    for (i, &g) in mp_gold.iter().enumerate() {
        let got = eng.taps()[i] as f64;
        max_diff = max_diff.max((got - g).abs());
        assert!((got - g).abs() < MP_TOL, "minphase t[{i}] = {got} vs {g}");
    }
    println!("minphase max abs diff = {max_diff:.3e}");

    let gd = gd_per_stage();
    assert!((gd - gd_gold).abs() < 0.01, "gd {gd} vs {gd_gold}");
}

#[test]
fn impulse_response_finite_and_decimates_2x() {
    let mut eng = HalfbandDec2::new();
    eng.set_min_phase(true);
    let mut input = vec![0.0_f32; 256];
    input[0] = 1.0;
    let mut output = vec![0.0_f32; 128];
    let nout = eng.process(&input, &mut output);
    assert_eq!(nout, 128);
    for &v in &output {
        assert!(v.is_finite());
    }
    // 线性相位原型关于中心对称。
    let eng_lin = HalfbandDec2::new();
    let mid = K_N / 2;
    let sum_left: f64 = eng_lin.taps()[..mid].iter().map(|&v| (v as f64).powi(2)).sum();
    let sum_right: f64 = eng_lin.taps()[mid + 1..].iter().map(|&v| (v as f64).powi(2)).sum();
    assert!((sum_left - sum_right).abs() < 1e-6, "linear phase 应对称");
}
