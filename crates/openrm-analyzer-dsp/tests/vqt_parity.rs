//! vqt_parity — VQTAnalyzer 离线回归，逐条移植自
//! `OpenRM/plugins/Analyzer/src/dsp/VQTAnalyzer.h`（635 行）。
//!
//! 黄金表由 `tests/data/gen_evidence.cpp` include 原版头文件导出
//! （`tests/data/data_vqt_<sr>_g<gamma>.txt`）。Rust 侧逐项比对：
//! 带数、中心频率、分层/窗长/读出偏移/归一化/复指数核系数；
//! 白噪激励下总能量有限、无 NaN。

use openrm_analyzer_dsp::vqt::VqtAnalyzer;

struct GoldBand {
    fc: f64,
    layer: i32,
    win_len: i32,
    advance: i32,
    read_off: i32,
    wsum_inv: f64,
    kr0: f64,
    ki0: f64,
    kr1: f64,
    ki1: f64,
}

struct Gold {
    bands: usize,
    bands_data: Vec<GoldBand>,
}

fn parse(data: &str) -> Gold {
    let mut bands = 0usize;
    let mut out = Vec::new();
    for line in data.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with("sr ") {
            let toks: Vec<&str> = line.split_whitespace().collect();
            if let Some(p) = toks.iter().position(|&t| t == "bands") {
                bands = toks[p + 1].parse().unwrap();
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("b") {
            let mut g = GoldBand {
                fc: 0.0,
                layer: 0,
                win_len: 0,
                advance: 1,
                read_off: 0,
                wsum_inv: 0.0,
                kr0: 0.0,
                ki0: 0.0,
                kr1: 0.0,
                ki1: 0.0,
            };
            let toks: Vec<&str> = rest.split_whitespace().collect();
            let mut i = 1; // 跳过 band 序号 "0"
            while i + 1 < toks.len() {
                let label = toks[i];
                let val = toks[i + 1];
                match label {
                    "fc" => g.fc = val.parse().unwrap(),
                    "layer" => g.layer = val.parse().unwrap(),
                    "winLen" => g.win_len = val.parse().unwrap(),
                    "advance" => g.advance = val.parse().unwrap(),
                    "readOff" => g.read_off = val.parse().unwrap(),
                    "wsumInv" => g.wsum_inv = val.parse().unwrap(),
                    "kr0" => g.kr0 = val.parse().unwrap(),
                    "ki0" => g.ki0 = val.parse().unwrap(),
                    "kr1" => g.kr1 = val.parse().unwrap(),
                    "ki1" => g.ki1 = val.parse().unwrap(),
                    _ => {}
                }
                i += 2;
            }
            out.push(g);
        }
    }
    Gold { bands, bands_data: out }
}

const CASES: &[(f64, i32, &str)] = &[
    (44_100.0, 20, include_str!("data/data_vqt_44100_g20.txt")),
    (44_100.0, 10, include_str!("data/data_vqt_44100_g10.txt")),
    (44_100.0, 5, include_str!("data/data_vqt_44100_g5.txt")),
    (48_000.0, 20, include_str!("data/data_vqt_48000_g20.txt")),
    (48_000.0, 10, include_str!("data/data_vqt_48000_g10.txt")),
    (48_000.0, 5, include_str!("data/data_vqt_48000_g5.txt")),
    (96_000.0, 20, include_str!("data/data_vqt_96000_g20.txt")),
    (96_000.0, 10, include_str!("data/data_vqt_96000_g10.txt")),
    (96_000.0, 5, include_str!("data/data_vqt_96000_g5.txt")),
];

#[test]
fn band_table_matches_cpp_all_rates() {
    for &(sr, gamma, data) in CASES {
        let gold = parse(data);
        let mut eng = VqtAnalyzer::new();
        eng.set_sample_rate(sr);
        eng.set_gamma_hz(gamma as f64);
        let zero = vec![0.0_f32; 1024];
        eng.process_block(&zero, &zero, &zero); // 触发 check_rebuild

        assert_eq!(
            eng.num_bands(),
            gold.bands_data.len(),
            "sr={sr} gamma={gamma} band count"
        );
        assert_eq!(eng.num_bands(), gold.bands, "sr={sr} gamma={gamma} header bands");

        let freqs = eng.band_freqs();
        for (b, g) in gold.bands_data.iter().enumerate() {
            let f = freqs[b];
            assert!((f - g.fc).abs() < g.fc.abs() * 1e-9 + 1e-6, "sr={sr} g={gamma} b{b} fc {f} vs {}", g.fc);
            let d = eng.band_debug(b);
            assert_eq!(d.layer, g.layer, "sr={sr} g={gamma} b{b} layer");
            assert_eq!(d.win_len, g.win_len, "sr={sr} g={gamma} b{b} winLen");
            assert_eq!(d.advance, g.advance, "sr={sr} g={gamma} b{b} advance");
            assert_eq!(d.read_off, g.read_off, "sr={sr} g={gamma} b{b} readOff");
            assert!(
                (d.wsum_inv as f64 - g.wsum_inv).abs() < 1e-6,
                "sr={sr} g={gamma} b{b} wsumInv {} vs {}",
                d.wsum_inv,
                g.wsum_inv
            );
            assert!((d.kr0 as f64 - g.kr0).abs() < 1e-5, "sr={sr} g={gamma} b{b} kr0");
            assert!((d.ki0 as f64 - g.ki0).abs() < 1e-5, "sr={sr} g={gamma} b{b} ki0");
            assert!((d.kr1 as f64 - g.kr1).abs() < 1e-5, "sr={sr} g={gamma} b{b} kr1");
            assert!((d.ki1 as f64 - g.ki1).abs() < 1e-5, "sr={sr} g={gamma} b{b} ki1");
        }
    }
}

#[test]
fn white_noise_excitation_finite_and_nonzero() {
    let mut eng = VqtAnalyzer::new();
    eng.set_sample_rate(48_000.0);
    eng.set_gamma_hz(5.0);

    // 确定性伪白噪（LCG），3 个 hop。
    let mut state: u64 = 0x12345678;
    let mut noise = Vec::new();
    for _ in 0..3 * 1024 {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let v = ((state >> 33) as f32) / (1u32 << 31) as f32 - 1.0;
        noise.push(v);
    }
    for chunk in noise.chunks(1024) {
        eng.process_block(chunk, chunk, chunk);
        let frame = eng.take_frame().expect("每 hop 一帧");
        assert_eq!(frame.vals.len(), eng.num_bands());
        let mut energy = 0.0_f64;
        for v in &frame.vals {
            for &x in &v[0..2] {
                assert!(x.is_finite(), "出现 NaN/Inf");
                energy += (x as f64) * (x as f64);
            }
        }
        assert!(energy > 1e-9, "白噪激励总能量应 > 0");
    }
}
