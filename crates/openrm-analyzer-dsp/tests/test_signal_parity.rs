//! test_signal_parity — TestSignalGenerator 离线回归，移植自
//! `OpenRM/plugins/Analyzer/src/dsp/TestSignalGenerator.h`（336 行）。
//!
//! 黄金表由 `tests/data/gen_evidence.cpp` include 原版头文件导出
//! （`tests/data/data_testgen.txt`）：18 种信号各前 64 个样本（L/R），
//! seed=0x12345678，sr=48000，默认 Config。Rust 侧逐样本比对（1e-6）。

use openrm_analyzer_dsp::test_signal::{TestSignalConfig, TestSignalGenerator, TestSignalType};

const DATA: &str = include_str!("data/data_testgen.txt");
const N: usize = 64;

fn parse() -> Vec<Vec<(f64, f64)>> {
    let mut out: Vec<Vec<(f64, f64)>> = Vec::new();
    let mut cur: Option<Vec<(f64, f64)>> = None;
    for line in DATA.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("=== TYPE ") {
            if let Some(prev) = cur.take() {
                out.push(prev);
            }
            let _ty: i32 = rest.trim_end_matches(" ===").parse().unwrap();
            cur = Some(Vec::new());
            continue;
        }
        if let Some(rest) = line.strip_prefix("s[") {
            let (_, rest) = rest.split_once("] ").unwrap();
            let mut l = 0.0;
            let mut r = 0.0;
            for tok in rest.split_whitespace() {
                if let Some(v) = tok.strip_prefix("L=") {
                    l = v.parse().unwrap();
                } else if let Some(v) = tok.strip_prefix("R=") {
                    r = v.parse().unwrap();
                }
            }
            cur.as_mut().unwrap().push((l, r));
        }
    }
    if let Some(prev) = cur.take() {
        out.push(prev);
    }
    out
}

#[test]
fn all_signal_types_match_cpp_golden() {
    let gold = parse();
    assert_eq!(gold.len(), 18, "应覆盖 18 种信号");

    for (ty, samples) in gold.iter().enumerate() {
        let mut gen = TestSignalGenerator::new();
        let cfg = TestSignalConfig {
            type_: TestSignalType::from_i32(ty as i32),
            ..Default::default()
        };
        gen.set_config(cfg);
        gen.set_seed(0x12345678);
        gen.restart();

        let mut dl = vec![0.0_f32; N];
        let mut dr = vec![0.0_f32; N];
        gen.fill(&mut dl, &mut dr, 48_000.0, true);

        assert_eq!(samples.len(), N, "type {ty} 黄金样本数");
        for (i, &(gl, gr)) in samples.iter().enumerate() {
            let got_l = dl[i] as f64;
            let got_r = dr[i] as f64;
            assert!(
                (got_l - gl).abs() < 1e-6,
                "type {ty} s[{i}] L {got_l} vs {gl}"
            );
            assert!(
                (got_r - gr).abs() < 1e-6,
                "type {ty} s[{i}] R {got_r} vs {gr}"
            );
        }
    }
}

#[test]
fn advance_false_keeps_index() {
    let mut gen = TestSignalGenerator::new();
    let cfg = TestSignalConfig {
        type_: TestSignalType::Sine,
        ..Default::default()
    };
    gen.set_config(cfg);
    gen.set_seed(0x12345678);
    gen.restart();

    let mut a = vec![0.0_f32; 8];
    let mut b = vec![0.0_f32; 8];
    gen.fill(&mut a, &mut b, 48_000.0, false);
    gen.fill(&mut b, &mut a, 48_000.0, false);
    for i in 0..8 {
        assert_eq!(a[i], b[i], "advance=false 应重复同一位置");
    }
}
