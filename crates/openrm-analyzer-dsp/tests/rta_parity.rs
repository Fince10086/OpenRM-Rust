//! rta_parity — RTAAnalyzer 带表/系数黄金对照 + 单音/白噪响应数值证据。
//!
//! 黄金表由 `tests/data/gen_pbt_rta.cpp` include 原版 `RTAAnalyzer.h` 导出
//! （`tests/data/data_rta.txt`，fs=48000）。Rust 侧逐项比对：
//! 带数、中心频率、边缘低通系数 K0/K1/K2、带中心校准增益、积分时间常数。

use openrm_analyzer_dsp::frame::EngineFrame;
use openrm_analyzer_dsp::rta::RtaAnalyzer;

const DATA: &str = include_str!("data/data_rta.txt");

struct RtaTable {
    mode: i32,
    bands: usize,
    env_alpha: f64,
    freqs: Vec<f64>,
    edges: Vec<f64>,
    k0: Vec<f32>,
    k1: Vec<f32>,
    k2: Vec<f32>,
    gain: Vec<f32>,
}

fn parse_tables() -> Vec<RtaTable> {
    let mut out = Vec::new();
    let mut cur: Option<RtaTable> = None;
    for line in DATA.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut it = line.split_whitespace();
        let label = it.next().unwrap();
        match label {
            "RTA" => {
                if let Some(prev) = cur.take() {
                    out.push(prev);
                }
                let mut mode = 0;
                let mut bands = 0;
                let mut env_alpha = 0.0;
                for tok in it {
                    let (k, v) = tok.split_once('=').unwrap();
                    match k {
                        "mode" => mode = v.parse().unwrap(),
                        "bands" => bands = v.parse().unwrap(),
                        "env_alpha" => env_alpha = v.parse().unwrap(),
                        _ => {}
                    }
                }
                cur = Some(RtaTable {
                    mode,
                    bands,
                    env_alpha,
                    freqs: Vec::new(),
                    edges: Vec::new(),
                    k0: Vec::new(),
                    k1: Vec::new(),
                    k2: Vec::new(),
                    gain: Vec::new(),
                });
            }
            "FREQ" => {
                cur.as_mut().unwrap().freqs = it.map(|v| v.parse::<f64>().unwrap()).collect();
            }
            "EDGES" => {
                cur.as_mut().unwrap().edges = it.map(|v| v.parse::<f64>().unwrap()).collect();
            }
            "K0" => {
                cur.as_mut().unwrap().k0 = it.map(|v| v.parse::<f32>().unwrap()).collect();
            }
            "K1" => {
                cur.as_mut().unwrap().k1 = it.map(|v| v.parse::<f32>().unwrap()).collect();
            }
            "K2" => {
                cur.as_mut().unwrap().k2 = it.map(|v| v.parse::<f32>().unwrap()).collect();
            }
            "GAIN" => {
                cur.as_mut().unwrap().gain = it.map(|v| v.parse::<f32>().unwrap()).collect();
            }
            _ => {}
        }
    }
    if let Some(prev) = cur.take() {
        out.push(prev);
    }
    out
}

#[test]
fn rta_tables_match_golden() {
    let tables = parse_tables();
    assert_eq!(tables.len(), 3, "三档 RTA 表");

    for t in &tables {
        let mut eng = RtaAnalyzer::new();
        eng.set_sample_rate(48_000.0);
        eng.set_octave_mode(t.mode);

        assert_eq!(
            eng.num_bands(),
            t.bands,
            "mode {} 带数不符",
            t.mode
        );
        assert_eq!(eng.band_freqs().len(), t.freqs.len());

        // 中心频率（f64，几何网格）
        for (b, (&got, &exp)) in eng.band_freqs().iter().zip(t.freqs.iter()).enumerate() {
            let rel = (got - exp).abs() / exp.abs().max(1e-9);
            assert!(rel < 1e-9, "mode {} 带 {} 频率 {got} != {exp} (rel {rel})", t.mode, b);
        }

        // 带缘网格（f64）
        let edges = eng.edges_debug();
        assert_eq!(edges.len(), t.edges.len(), "mode {} 缘数不符", t.mode);
        for (j, (&got, &exp)) in edges.iter().zip(t.edges.iter()).enumerate() {
            let abs_d = (got - exp).abs();
            let rel = abs_d / exp.abs().max(1e-9);
            assert!(abs_d < 1e-9 || rel < 1e-10, "mode {} 缘 {j}: {got} != {exp}", t.mode);
        }

        // 系数（f32；低带 K0 ~1e-7，用绝对 + 相对双容差；记录最差偏差作证据）
        let k0 = eng.k0_debug();
        let k1 = eng.k1_debug();
        let k2 = eng.k2_debug();
        let gain = eng.gain_debug();
        let mut worst: (f32, f32, String, usize) = (0.0, 0.0, String::new(), 0);
        let mut check = |g: f32, e: f32, name: &str, idx: usize| {
            let abs_d = (g - e).abs();
            let rel_d = abs_d / e.abs().max(1e-9);
            if abs_d > worst.0 || rel_d > worst.1 {
                worst = (abs_d, rel_d, name.to_string(), idx);
            }
            assert!(
                abs_d < 2e-4 || rel_d < 3e-5,
                "mode {} {name}[{idx}]: {g} != {e}",
                t.mode
            );
        };
        for j in 0..=t.bands {
            check(k0[j], t.k0[j], "K0", j);
            check(k1[j], t.k1[j], "K1", j);
            check(k2[j], t.k2[j], "K2", j);
        }
        for b in 0..t.bands {
            check(gain[b], t.gain[b], "GAIN", b);
        }
        println!(
            "mode {} 系数最差偏差: {}[{}] abs={:.2e} rel={:.2e}",
            t.mode, worst.2, worst.3, worst.0, worst.1
        );

        let alpha = eng.env_alpha_debug() as f64;
        let d = (alpha - t.env_alpha).abs();
        println!(
            "RTA mode {}: bands={}, env_alpha={:.10} (golden {:.10}, Δ={:.2e})",
            t.mode,
            t.bands,
            alpha,
            t.env_alpha,
            d
        );
        assert!(d < 1e-9);
    }
}

// ── 响应数值证据 ──────────────────────────────────────────────────────────

fn run_engine(eng: &mut RtaAnalyzer, l: &[f32], r: &[f32], m: &[f32]) -> Option<EngineFrame> {
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
fn rta_sine_center_gain() {
    // 1/12 档：在若干中心频率放单音，稳态带中心幅度应 ≈ 输入幅度（校准后）。
    let mut eng = RtaAnalyzer::new();
    eng.set_sample_rate(48_000.0);
    eng.set_octave_mode(1);
    let nb = eng.num_bands();
    let freqs = eng.band_freqs().to_vec();

    let sr = 48_000.0;
    let amp = 0.5f32;
    let secs = 1.5;
    let n = (secs * sr) as usize;

    let picks = [nb / 8, nb / 3, nb / 2, (2 * nb) / 3, (7 * nb) / 8];
    let mut worst_db = f32::MAX;
    for &b in &picks {
        let f = freqs[b];
        let x = sine(n, f, amp, sr);
        let m = x.clone();
        let frame = run_engine(&mut eng, &x, &x, &m).expect("应有一帧");
        let out = frame.vals[b][0];
        let err_db = 20.0 * (out / amp).log10();
        worst_db = worst_db.min(err_db);
        // 邻带抑制证据
        let neigh = if b > 0 { frame.vals[b - 1][0] } else { 0.0 };
        println!(
            "单音 {f:8.2} Hz: 中心带={out:.4} (误差 {err_db:+.2} dB), 邻带={neigh:.4}",
        );
        assert!(out > 0.0, "中心带不应为零");
        assert!(err_db > -3.0 && err_db < 3.0, "中心带增益偏离 ±3 dB");
        assert!(neigh < out * 0.9, "邻带应低于中心带");
    }
    println!("最差中心带误差: {worst_db:+.2} dB");
}

#[test]
fn rta_white_noise_response() {
    // 白噪：各带均有可测能量，无 NaN/Inf；高低带电平在同一量级（互补分割）。
    let mut eng = RtaAnalyzer::new();
    eng.set_sample_rate(48_000.0);
    eng.set_octave_mode(0);
    let nb = eng.num_bands();

    let sr = 48_000.0;
    let n = (2.0 * sr) as usize;
    // 确定性伪白噪（同余递推），RMS ≈ 1/√3
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

    let mut bands: Vec<(usize, f32)> = (0..nb).map(|b| (b, frame.vals[b][0])).collect();
    for &(_, v) in &bands {
        assert!(v.is_finite(), "RTA 输出必须有限");
    }
    bands.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    let mid = nb / 2;
    let low = frame.vals[nb / 8][0];
    let midv = frame.vals[mid][0];
    let high = frame.vals[(7 * nb) / 8][0];
    println!(
        "白噪带幅度: 低(#{})={low:.4}, 中(#{})={midv:.4}, 高(#{})={high:.4}, 峰值带={} {:.4}",
        nb / 8,
        mid,
        (7 * nb) / 8,
        bands[0].0,
        bands[0].1
    );
    assert!(low > 0.0 && midv > 0.0 && high > 0.0, "低/中/高带均应有能量");
    // 互补分割下无某一整段塌零；高带不超过中带 10 倍（无异常增益尖峰）。
    assert!(high < midv * 10.0);
}
