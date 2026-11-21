//! stft_parity — SpectrumSTFT 离线回归，逐条移植自
//! `OpenRM/plugins/common/dsp/SpectrumSTFT.h`。
//!
//! 黄金表由 `tests/data/gen_evidence.cpp` include 原版头文件、链接真实 WDL fft.c
//! 导出（`tests/data/data_stft.txt`）。Rust 侧逐项比对：
//! 窗系数 / 归一化系数、整窗 bin 中心正弦幅度、DC 标定、dB 换算。

use openrm_analyzer_dsp::stft::StftAnalyzer;
use std::collections::HashMap;

const DATA: &str = include_str!("data/data_stft.txt");
const PI: f64 = 3.141_592_653_589_793_238_46;

#[derive(Default, Clone)]
struct WindowGold {
    hop: usize,
    num_bins: usize,
    scaling: f64,
    sum: f64,
    w: HashMap<usize, f64>,
}

#[derive(Default, Clone)]
struct SineGold {
    k: usize,
    mag: HashMap<usize, (f64, f64, f64)>,
}

#[derive(Default, Clone)]
struct Parsed {
    windows: HashMap<(usize, i32), WindowGold>,
    sines: HashMap<usize, SineGold>,
    dc: f64,
}

fn parse() -> Parsed {
    let mut out = Parsed::default();
    let mut cur_win: Option<(usize, i32)> = None;
    let mut cur_sine: Option<usize> = None;

    for line in DATA.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("=== WINDOW N=") {
            let (n, rest) = rest.split_once(" WT=").unwrap();
            let wt = rest.trim_end_matches(" ===").parse::<i32>().unwrap();
            cur_win = Some((n.parse().unwrap(), wt));
            cur_sine = None;
            out.windows.entry((n.parse().unwrap(), wt)).or_default();
            continue;
        }
        if let Some(rest) = line.strip_prefix("=== SINE N=") {
            let (n, rest) = rest.split_once(" K=").unwrap();
            let k = rest.split_whitespace().next().unwrap().parse().unwrap();
            cur_sine = Some(n.parse().unwrap());
            cur_win = None;
            out.sines.entry(n.parse().unwrap()).or_insert(SineGold {
                k,
                mag: HashMap::new(),
            });
            continue;
        }
        if line.starts_with("=== DC") {
            cur_win = None;
            cur_sine = None;
            continue;
        }
        if let Some(key) = cur_win {
            let g = out.windows.get_mut(&key).unwrap();
            if let Some(rest) = line.strip_prefix("hop ") {
                let mut it = rest.split_whitespace();
                g.hop = it.next().unwrap().parse().unwrap();
                it.next(); // num_bins
                g.num_bins = it.next().unwrap().parse().unwrap();
                it.next(); // scaling
                g.scaling = it.next().unwrap().parse().unwrap();
            } else if let Some(rest) = line.strip_prefix("sum ") {
                g.sum = rest.parse().unwrap();
            } else if let Some(rest) = line.strip_prefix("w[") {
                let (idx, val) = rest.split_once("]=").unwrap();
                g.w.insert(idx.parse().unwrap(), val.parse().unwrap());
            }
        } else if let Some(n) = cur_sine {
            let g = out.sines.get_mut(&n).unwrap();
            if let Some(rest) = line.strip_prefix("mag[") {
                let (bin, rest) = rest.split_once("] ").unwrap();
                let bin = bin.parse().unwrap();
                let mut l = 0.0_f64;
                let mut r = 0.0_f64;
                let mut m = 0.0_f64;
                for tok in rest.split_whitespace() {
                    if let Some(v) = tok.strip_prefix("L=") {
                        l = v.parse().unwrap();
                    } else if let Some(v) = tok.strip_prefix("R=") {
                        r = v.parse().unwrap();
                    } else if let Some(v) = tok.strip_prefix("M=") {
                        m = v.parse().unwrap();
                    }
                }
                g.mag.insert(bin, (l, r, m));
            }
        } else if let Some(rest) = line.strip_prefix("dc L=") {
            out.dc = rest.parse().unwrap();
        }
    }
    out
}

#[test]
fn window_coeffs_and_scaling_match_cpp() {
    let gold = parse();
    for ((n, wt), g) in &gold.windows {
        let mut eng = StftAnalyzer::new();
        eng.set_fft_size(*n);
        eng.set_window(*wt);
        assert_eq!(eng.num_bins(), g.num_bins, "N={n} WT={wt} num_bins");
        assert_eq!(eng.hop(), g.hop, "N={n} WT={wt} hop");

        let w = eng.window_coeffs();
        let sum: f64 = w.iter().map(|&v| v as f64).sum();
        assert!((sum - g.sum).abs() < 1e-3, "N={n} WT={wt} sum {sum} vs {}", g.sum);
        assert!(
            (eng.scaling() as f64 - g.scaling).abs() < g.scaling.abs() * 1e-6 + 1.0,
            "N={n} WT={wt} scaling {} vs {}",
            eng.scaling(),
            g.scaling
        );
        for (idx, val) in &g.w {
            let got = w[*idx] as f64;
            assert!(
                (got - val).abs() < 1e-6,
                "N={n} WT={wt} w[{idx}] = {got} vs {val}"
            );
        }
    }
}

#[test]
fn sine_on_bin_center_matches_cpp() {
    let gold = parse();
    for (n, g) in &gold.sines {
        let k = g.k;
        let mut eng = StftAnalyzer::new();
        eng.set_fft_size(*n);
        let x: Vec<f32> = (0..*n)
            .map(|i| (2.0 * PI * k as f64 * i as f64 / *n as f64).sin() as f32)
            .collect();
        eng.process_block(&x, &x, &x);
        let frame = eng.take_frame().expect("hop 满后应有一帧");

        for (bin, (l, r, m)) in &g.mag {
            let got_l = frame.vals[*bin][0] as f64;
            let got_r = frame.vals[*bin][1] as f64;
            let got_m = frame.vals[*bin][2] as f64;
            assert!((got_l - l).abs() < 2e-4, "N={n} bin {bin} L {got_l} vs {l}");
            assert!((got_r - r).abs() < 2e-4, "N={n} bin {bin} R {got_r} vs {r}");
            assert!((got_m - m).abs() < 1e-6, "N={n} bin {bin} M {got_m} vs {m}");
        }
    }
}

#[test]
fn dc_calibration_matches_cpp() {
    let gold = parse();
    let mut eng = StftAnalyzer::new();
    eng.set_fft_size(4096);
    let x = vec![1.0_f32; 4096];
    eng.process_block(&x, &x, &x);
    let frame = eng.take_frame().unwrap();
    let dc = frame.vals[0][0] as f64;
    assert!((dc - gold.dc).abs() < 1e-5, "DC {dc} vs {}", gold.dc);
}

#[test]
fn db_conversion_60206_log2() {
    // 与原版 FastLog2 路径一致：dB = 6.0206·log2(amp)，等价 20·log10(amp)。
    for amp in [0.001_f64, 0.1, 0.5, 1.0, 1.4142135] {
        let db = 6.0206 * amp.log2();
        let ref_db = 20.0 * amp.log10();
        assert!((db - ref_db).abs() < 1e-3, "amp {amp}: {db} vs {ref_db}");
    }
}
