//! pbt_parity — PBTAnalyzer 带表/层号/核长/对齐偏移/复解调核黄金对照 + 响应证据。
//!
//! 黄金表由 `tests/data/gen_pbt_rta.cpp` include 原版 `PBTAnalyzer.h` 导出
//! （`tests/data/data_pbt.txt`，fs=48000）。Rust 侧逐项比对。

use openrm_analyzer_dsp::frame::EngineFrame;
use openrm_analyzer_dsp::pbt::PbtAnalyzer;

const DATA: &str = include_str!("data/data_pbt.txt");

struct PbtBand {
    layer: usize,
    win_len: usize,
    read_off: usize,
    wsum_inv: f64,
    kr: Vec<f32>,
    ki: Vec<f32>,
}

struct PbtTable {
    lf: i32,
    bands: usize,
    freqs: Vec<f64>,
    pb: Vec<PbtBand>,
}

fn parse_tables() -> Vec<PbtTable> {
    let mut out = Vec::new();
    let mut cur: Option<PbtTable> = None;
    let mut band: Option<PbtBand> = None;
    for line in DATA.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut it = line.split_whitespace();
        let label = it.next().unwrap();
        match label {
            "PBT" => {
                if let Some(b) = band.take() {
                    cur.as_mut().unwrap().pb.push(b);
                }
                if let Some(prev) = cur.take() {
                    out.push(prev);
                }
                let mut lf = 0;
                let mut bands = 0;
                for tok in it {
                    let (k, v) = tok.split_once('=').unwrap();
                    match k {
                        "lf" => lf = v.parse().unwrap(),
                        "bands" => bands = v.parse().unwrap(),
                        _ => {}
                    }
                }
                cur = Some(PbtTable {
                    lf,
                    bands,
                    freqs: Vec::new(),
                    pb: Vec::new(),
                });
            }
            "FREQ" => {
                cur.as_mut().unwrap().freqs = it.map(|v| v.parse::<f64>().unwrap()).collect();
            }
            "BAND" => {
                if let Some(b) = band.take() {
                    cur.as_mut().unwrap().pb.push(b);
                }
                let idx: usize = it.next().unwrap().parse().unwrap();
                let mut layer = 0;
                let mut win_len = 0;
                let mut read_off = 0;
                let mut wsum_inv = 0.0;
                while let Some(tok) = it.next() {
                    match tok {
                        "layer" => layer = it.next().unwrap().parse().unwrap(),
                        "winLen" => win_len = it.next().unwrap().parse().unwrap(),
                        "readOff" => read_off = it.next().unwrap().parse().unwrap(),
                        "wsumInv" => wsum_inv = it.next().unwrap().parse().unwrap(),
                        "klen" => {
                            it.next();
                        }
                        _ => {}
                    }
                }
                assert_eq!(idx, cur.as_ref().unwrap().pb.len());
                band = Some(PbtBand {
                    layer,
                    win_len,
                    read_off,
                    wsum_inv,
                    kr: Vec::new(),
                    ki: Vec::new(),
                });
            }
            "KR" => {
                band.as_mut().unwrap().kr = it.map(|v| v.parse::<f32>().unwrap()).collect();
            }
            "KI" => {
                band.as_mut().unwrap().ki = it.map(|v| v.parse::<f32>().unwrap()).collect();
            }
            _ => {}
        }
    }
    if let Some(b) = band.take() {
        cur.as_mut().unwrap().pb.push(b);
    }
    if let Some(prev) = cur.take() {
        out.push(prev);
    }
    out
}

#[test]
fn pbt_tables_match_golden() {
    let tables = parse_tables();
    assert_eq!(tables.len(), 3);

    for t in &tables {
        let mut eng = PbtAnalyzer::new();
        eng.set_sample_rate(48_000.0);
        eng.set_lf_width(t.lf);

        assert_eq!(eng.num_bands(), t.bands, "lf={} 带数不符", t.lf);
        assert_eq!(eng.band_freqs().len(), t.freqs.len());

        for (b, (&got, &exp)) in eng.band_freqs().iter().zip(t.freqs.iter()).enumerate() {
            let rel = (got - exp).abs() / exp.abs().max(1e-9);
            assert!(rel < 1e-9, "lf {} 带 {b} 频率 {got} != {exp}", t.lf);
        }

        let mut worst_kernel: f32 = 0.0;
        let mut worst_kernel_desc = String::new();
        for b in 0..t.bands {
            let (layer, win_len, read_off, wsum_inv, kr, ki) = eng.band_debug(b);
            let gb = &t.pb[b];
            assert_eq!(layer, gb.layer, "lf {} 带 {b} 层号", t.lf);
            assert_eq!(win_len, gb.win_len, "lf {} 带 {b} 核长", t.lf);
            assert_eq!(read_off, gb.read_off, "lf {} 带 {b} 对齐偏移", t.lf);
            let wd = (wsum_inv as f64 - gb.wsum_inv).abs();
            assert!(wd < 1e-5, "lf {} 带 {b} 归一增益 {wsum_inv} != {}", t.lf, gb.wsum_inv);
            assert_eq!(kr.len(), gb.kr.len(), "lf {} 带 {b} Re 核长", t.lf);
            assert_eq!(ki.len(), gb.ki.len(), "lf {} 带 {b} Im 核长", t.lf);
            for n in 0..kr.len() {
                for (g, e, name) in [(kr[n], gb.kr[n], "Re"), (ki[n], gb.ki[n], "Im")] {
                    let d = (g - e).abs();
                    let rel = d / e.abs().max(1e-9);
                    if d > worst_kernel {
                        worst_kernel = d;
                        worst_kernel_desc = format!("lf {} 带 {b} {name}[{n}]", t.lf);
                    }
                    assert!(
                        d < 1e-3 || rel < 1e-2,
                        "{} 核 {name}[{n}]: {g} != {e}",
                        worst_kernel_desc
                    );
                }
            }
        }
        println!(
            "PBT lf={}: bands={}, 核最大绝对偏差={:.2e} ({})",
            t.lf,
            t.bands,
            worst_kernel,
            worst_kernel_desc
        );
    }
}

// ── 响应数值证据 ──────────────────────────────────────────────────────────

fn run_engine(eng: &mut PbtAnalyzer, l: &[f32], r: &[f32], m: &[f32]) -> Option<EngineFrame> {
    let mut last: Option<EngineFrame> = None;
    let mut pos = 0;
    while pos < l.len() {
        let b = 1024.min(l.len() - pos);
        eng.process_block(&l[pos..pos + b], &r[pos..pos + b], &m[pos..pos + b]);
        while let Some(f) = eng.take_frame() {
            last = Some(f);
        }
        pos += b;
    }
    last
}

fn sine(n: usize, freq: f64, amp: f32, sr: f64) -> Vec<f32> {
    (0..n)
        .map(|i| (amp as f64 * (2.0 * std::f64::consts::PI * freq * i as f64 / sr).sin()) as f32)
        .collect()
}

#[test]
fn pbt_sine_center_gain() {
    let mut eng = PbtAnalyzer::new();
    eng.set_sample_rate(48_000.0);
    eng.set_lf_width(20);
    let nb = eng.num_bands();
    let freqs = eng.band_freqs().to_vec();
    let sr = 48_000.0;
    let amp = 0.5f32;
    let n = (2.0 * sr) as usize;

    let picks = [nb / 6, nb / 3, nb / 2, (2 * nb) / 3, (5 * nb) / 6];
    let mut worst_db = f32::MAX;
    for &b in &picks {
        let f = freqs[b];
        let x = sine(n, f, amp, sr);
        let m = x.clone();
        let frame = run_engine(&mut eng, &x, &x, &m).expect("应有一帧");
        let out = frame.vals[b][0];
        let err_db = 20.0 * (out / amp).log10();
        worst_db = worst_db.min(err_db);
        let neigh = if b > 0 { frame.vals[b - 1][0] } else { 0.0 };
        println!(
            "单音 {f:8.2} Hz: 中心带={out:.4} (误差 {err_db:+.2} dB), 邻带={neigh:.4}"
        );
        assert!(out > 0.0, "中心带不应为零");
        assert!(err_db > -4.0 && err_db < 4.0, "中心带增益偏离 ±4 dB");
        assert!(neigh < out * 0.95, "邻带应低于中心带");
    }
    println!("最差中心带误差: {worst_db:+.2} dB");
}

#[test]
fn pbt_white_noise_response() {
    let mut eng = PbtAnalyzer::new();
    eng.set_sample_rate(48_000.0);
    eng.set_lf_width(40);
    let nb = eng.num_bands();
    let sr = 48_000.0;
    let n = (2.0 * sr) as usize;
    let mut state: u64 = 0x1234_5678_9ABC_DEF0;
    let noise: Vec<f32> = (0..n)
        .map(|_| {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let u = (state >> 33) as f32 / (1u32 << 31) as f32;
            (u * 2.0 - 1.0) * 0.5
        })
        .collect();
    let m = noise.clone();
    let frame = run_engine(&mut eng, &noise, &noise, &m).expect("应有一帧");
    for v in frame.vals.iter() {
        assert!(v[0].is_finite(), "PBT 输出必须有限");
    }
    let low = frame.vals[nb / 8][0];
    let mid = frame.vals[nb / 2][0];
    let high = frame.vals[(7 * nb) / 8][0];
    println!(
        "白噪带幅度: 低(#{})={low:.4}, 中(#{})={mid:.4}, 高(#{})={high:.4}",
        nb / 8,
        nb / 2,
        (7 * nb) / 8
    );
    assert!(low > 0.0 && mid > 0.0 && high > 0.0);
}
