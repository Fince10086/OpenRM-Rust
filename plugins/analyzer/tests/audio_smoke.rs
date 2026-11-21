//! audio_smoke — 插件外壳音频路径冒烟测试（48 kHz / 512 块）。
//!
//! 覆盖任务验收点：
//! * `take_frame` 节奏：hop 1024、块 512 → 每 2 块出一帧；
//! * M 路合成：`M = (L+R)/√2`（SUM 档 m 带相对 PWR 档 L 带幅度比 ≈ √2）；
//! * 电平 peak ≈ -6.02 dBFS（0.5 幅度正弦）；
//! * 响度 I ≈ -3.01 LUFS（0 dBFS 1 kHz 正弦，BS.1770 校准点）；
//! * freeze 后帧定格不变；
//! * reset 请求后快照回默认。
//!
//! 每个断言都打印实测值；测试直接走 `PluginLogic` 外壳，不经宿主驱动。

use std::sync::atomic::Ordering;

use openrm_analyzer_dsp::loudness_meter::LoudnessMeter;
use openrm_analyzer_dsp::test_signal::{
    TestSignalConfig, TestSignalGenerator, TestSignalType,
};
use openrm_analyzer_dsp::vqt::VqtAnalyzer;
use truce::prelude::*;

use orm_analyzer::analysis::M_COEFF;
use orm_analyzer::params::AnalyzerParams;
use orm_analyzer::{Analyzer, AnalyzerDsp};

const SR: f64 = 48_000.0;
const BLOCK: usize = 512;
const FREQ_HZ: f64 = 1000.0;
const TWO_PI: f64 = 6.283_185_307_179_586;

/// 连续相位正弦发生器（块间相位连续）。
struct Sine {
    phase: f64,
    amp: f32,
}

impl Sine {
    fn new(amp: f32) -> Self {
        Self { phase: 0.0, amp }
    }

    fn fill(&mut self, dst: &mut [f32]) {
        for s in dst.iter_mut() {
            let v = self.amp as f64 * (TWO_PI * FREQ_HZ * self.phase).sin();
            *s = v as f32;
            self.phase = (self.phase + 1.0 / SR) % 1.0;
        }
    }
}

/// 确定性 LCG 伪白噪（跨块可复现），用于窗型 / 种子对比的同一激励。
fn fill_lcg(state: &mut u64, dst: &mut [f32]) {
    for s in dst.iter_mut() {
        *state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        *s = ((*state >> 33) as f32) / (1u32 << 31) as f32 - 1.0;
    }
}

fn make_state() -> (AnalyzerParams, AnalyzerDsp) {
    let params = AnalyzerParams::default();
    let mut state = Analyzer::init(&params, &InitContext::new(None));
    Analyzer::reset(&mut state, &params, &AudioConfig::new(SR, BLOCK));
    (params, state)
}

fn run_block(state: &mut AnalyzerDsp, params: &AnalyzerParams, l: &[f32], r: &[f32]) {
    let mut out_l = vec![0.0; BLOCK];
    let mut out_r = vec![0.0; BLOCK];
    let mut outs: Vec<&mut [f32]> = vec![out_l.as_mut_slice(), out_r.as_mut_slice()];
    let ins: [&[f32]; 2] = [l, r];
    let mut buffer = AudioBuffer::from_slices_checked(&ins, &mut outs, BLOCK);

    let events = EventList::with_capacity(8);
    let transport = TransportInfo::default();
    let mut out_events = EventList::with_capacity(8);
    let mut ctx = ProcessContext::new(&transport, SR, BLOCK, &mut out_events);
    Analyzer::process(state, params, &mut buffer, &events, &mut ctx);
}

fn latest_frame(
    params: &AnalyzerParams,
) -> Option<(u8, orm_analyzer::ui::data::SpectrumFrame)> {
    params.analysis.shared.spectrum.lock().unwrap().clone()
}

/// PWR 档：等幅双声道 → L/R 两路一致，M 槽位由引擎置零（引擎通道契约）。
#[test]
fn frame_cadence_and_pwr_lanes() {
    let (params, mut state) = make_state();
    let mut sine = Sine::new(0.5);
    let mut l = vec![0.0; BLOCK];
    let mut r = vec![0.0; BLOCK];

    let seq0 = params.analysis.shared.spectrum_seq.load(Ordering::Relaxed);
    let mut last: Option<orm_analyzer::ui::data::SpectrumFrame> = None;
    for _ in 0..20 {
        sine.fill(&mut l);
        r.copy_from_slice(&l);
        run_block(&mut state, &params, &l, &r);
        if let Some((_, f)) = latest_frame(&params) {
            last = Some(f);
        }
    }
    let frames = params.analysis.shared.spectrum_seq.load(Ordering::Relaxed) - seq0;

    println!(
        "1: 20 块（512）内出帧 {frames} 次；hop=1024 期望每 2 块一帧 → 10 次"
    );
    assert_eq!(frames, 10, "帧节奏不对");

    let frame = last.expect("PWR 档应有频谱帧");
    assert!(!frame.l.is_empty() && !frame.r.is_empty());

    let mut lane_diff = 0.0f32;
    let mut m_nonzero = 0usize;
    for i in 0..frame.l.len() {
        lane_diff = lane_diff.max((frame.l[i] - frame.r[i]).abs());
        if frame.m[i].abs() > 1e-6 {
            m_nonzero += 1;
        }
    }
    println!("1: L/R 最大差 {lane_diff:.2e}；M 非零 bin 数 {m_nonzero}（PWR 档期望 0）");
    assert!(lane_diff < 1e-5, "等幅双声道 L/R 不一致");
    assert_eq!(m_nonzero, 0, "PWR 档 M 槽位应为 0");

    let level = params.analysis.shared.level.lock().unwrap().clone();
    println!(
        "1: peak L={:.3} dBFS, R={:.3} dBFS（0.5 幅度期望 ≈ -6.021）",
        level.peak_db_l, level.peak_db_r
    );
    assert!(
        (level.peak_db_l + 6.0206).abs() < 0.15,
        "peak L={}",
        level.peak_db_l
    );
    assert!(
        (level.peak_db_r + 6.0206).abs() < 0.15,
        "peak R={}",
        level.peak_db_r
    );
}

/// M 路合成一致性：等幅 L=R=A 时 M=(L+R)/√2=√2·A；
/// 同一引擎同一窗函数，FFT 标度抵消，SUM 档 m 带峰值 / PWR 档 l 带峰值 ≈ √2。
#[test]
fn m_lane_matches_sum_coeff() {
    // PWR 参考：L 带峰值
    let (params_pwr, mut state_pwr) = make_state();
    let mut sine = Sine::new(0.5);
    let mut l = vec![0.0; BLOCK];
    let mut r = vec![0.0; BLOCK];
    for _ in 0..20 {
        sine.fill(&mut l);
        r.copy_from_slice(&l);
        run_block(&mut state_pwr, &params_pwr, &l, &r);
    }
    let pwr = latest_frame(&params_pwr).expect("PWR 帧");
    let l_peak = pwr.1.l.iter().copied().fold(0.0f32, f32::max);

    // SUM 档：M 带峰值
    let (params_sum, mut state_sum) = make_state();
    params_sum.channel_mode.set_index(2);
    let mut sine = Sine::new(0.5);
    for _ in 0..20 {
        sine.fill(&mut l);
        r.copy_from_slice(&l);
        run_block(&mut state_sum, &params_sum, &l, &r);
    }
    let sum = latest_frame(&params_sum).expect("SUM 帧");
    let m_peak = sum.1.m.iter().copied().fold(0.0f32, f32::max);

    let ratio = m_peak / l_peak.max(1e-9);
    println!(
        "2: L 带峰值={l_peak:.5}, M 带峰值={m_peak:.5}, 比={ratio:.4}（期望 ≈ √2={:.4}）",
        2.0_f32.sqrt()
    );
    println!("2: M 系数 M_COEFF={M_COEFF}（=1/√2）");
    assert!(ratio > 1.25 && ratio < 1.55, "M/L 幅度比 {ratio} 偏离 √2");
    assert!(m_peak > 0.0, "SUM 档 M 带应为非零");
}

/// 响度：外壳立体声等幅 0 dBFS（G_L=G_R=1，I ≈ 0.0 LUFS）；
/// BS.1770 单声道校准点 R=0、0 dBFS 1 kHz → I ≈ -3.01 LUFS
/// （与 `loudness_parity` 1c 同一口径，DSP 级校验）。
#[test]
fn loudness_shell_stereo_and_mono_calibration() {
    // ── 外壳路径：立体声等幅满幅正弦 10s ──
    let (params, mut state) = make_state();
    let mut sine = Sine::new(1.0);
    let mut l = vec![0.0; BLOCK];
    let mut r = vec![0.0; BLOCK];

    let blocks = (SR * 10.0 / BLOCK as f64).round() as usize;
    for _ in 0..blocks {
        sine.fill(&mut l);
        r.copy_from_slice(&l);
        run_block(&mut state, &params, &l, &r);
    }

    let snap = params.analysis.shared.loudness.lock().unwrap().clone();
    println!(
        "3a: 外壳立体声 I={:.4} LUFS, M={:.4}, S={:.4}, iValid={}（等幅双声道 G=1，期望 ≈ 0.00）",
        snap.integrated, snap.momentary, snap.short_term, snap.i_valid
    );
    assert!(snap.i_valid, "10s 后 iValid 应为 true");
    assert!(
        snap.integrated.abs() < 0.25,
        "外壳立体声 I={} 偏离 0 LUFS",
        snap.integrated
    );

    // ── DSP 级单声道校准：L=满幅正弦、R=0，10s ──
    let mut lm = LoudnessMeter::new();
    lm.set_sample_rate(SR);
    let mut phase = 0.0f64;
    for _ in 0..blocks {
        let mut mono_l = Vec::with_capacity(BLOCK);
        let zero = vec![0.0; BLOCK];
        for _ in 0..BLOCK {
            let v = (TWO_PI * FREQ_HZ * phase).sin() as f32;
            mono_l.push(v);
            phase = (phase + 1.0 / SR) % 1.0;
        }
        lm.process(&mono_l, &zero);
    }
    let sm = lm.store();
    println!(
        "3b: 单声道校准 I={:.4} LUFS（期望 -3.01，容差 0.15）, iValid={}",
        sm.integrated, sm.i_valid
    );
    assert!(sm.i_valid);
    assert!(
        (sm.integrated + 3.01).abs() < 0.15,
        "单声道 I={} 偏离校准点",
        sm.integrated
    );
}

/// 冻结：定格后喂静音，频谱帧内容不变（回放重建同一历史）。
#[test]
fn freeze_holds_last_frame() {
    let (params, mut state) = make_state();
    let mut sine = Sine::new(0.5);
    let mut l = vec![0.0; BLOCK];
    let mut r = vec![0.0; BLOCK];

    for _ in 0..12 {
        sine.fill(&mut l);
        r.copy_from_slice(&l);
        run_block(&mut state, &params, &l, &r);
    }
    let before = latest_frame(&params).expect("冻结前应有帧").1;

    params.freeze.set_value(true);
    let silence = vec![0.0; BLOCK];
    // 回放 16 帧/块；环形历史约 512 帧，需 ≥32 块才播到最新帧。
    for _ in 0..40 {
        run_block(&mut state, &params, &silence, &silence);
    }
    let after = latest_frame(&params).expect("冻结后应保留帧").1;

    let max_diff = before
        .l
        .iter()
        .zip(&after.l)
        .map(|(a, b)| (a - b).abs())
        .chain(before.r.iter().zip(&after.r).map(|(a, b)| (a - b).abs()))
        .chain(before.m.iter().zip(&after.m).map(|(a, b)| (a - b).abs()))
        .fold(0.0f32, f32::max);

    println!("4: freeze 后帧三路最大偏差 {max_diff:.6e}（期望 ≈ 0）");
    assert!(max_diff < 1e-4, "冻结帧被改动: {max_diff}");
}

/// 复位请求：电平全量 + 响度清零，下一 block 消费后快照回默认。
#[test]
fn reset_requests_return_to_default() {
    let (params, mut state) = make_state();
    let mut sine = Sine::new(1.0);
    let mut l = vec![0.0; BLOCK];
    let mut r = vec![0.0; BLOCK];

    for _ in 0..12 {
        sine.fill(&mut l);
        r.copy_from_slice(&l);
        run_block(&mut state, &params, &l, &r);
    }

    let warm_level = params.analysis.shared.level.lock().unwrap().clone();
    let warm_loud = params.analysis.shared.loudness.lock().unwrap().clone();
    println!(
        "5: 复位前 peak L={:.2}, persist L={:.2}, I={:.2}, iValid={}",
        warm_level.peak_db_l, warm_level.persist_db_l, warm_loud.integrated, warm_loud.i_valid
    );

    params
        .analysis
        .shared
        .reset_level
        .store(true, Ordering::Relaxed);
    params
        .analysis
        .shared
        .reset_loud
        .store(true, Ordering::Relaxed);

    let silence = vec![0.0; BLOCK];
    run_block(&mut state, &params, &silence, &silence);

    let level = params.analysis.shared.level.lock().unwrap().clone();
    let loud = params.analysis.shared.loudness.lock().unwrap().clone();
    println!(
        "5: 复位后 peak L={:.2}, persist L={:.2}, hold L={:.2}, over L={}, I={:.2}, iValid={}",
        level.peak_db_l,
        level.persist_db_l,
        level.hold_db_l,
        level.over_l,
        loud.integrated,
        loud.i_valid
    );

    assert!(level.persist_db_l < -119.0, "persist 未清零");
    assert!(level.hold_db_l < -119.0, "hold 未清零");
    assert_eq!(level.over_l, 0, "over 未清零");
    assert!(loud.integrated < -119.0, "响度 I 未清零");
    assert!(!loud.i_valid, "响度 iValid 未复位");

    // 请求位应被音频线程取走清零
    assert!(!params.analysis.shared.reset_level.load(Ordering::Relaxed));
    assert!(!params.analysis.shared.reset_loud.load(Ordering::Relaxed));
}

/// VQT 窗型（DSP 级）：SHARP(0, Hann) / CLEAN(1, 5 项 BH) 两档核系数与输出可区分；
/// `set_window` 返回值语义对齐原版 `SetWindowType`（跨档 true，同档 false）。
#[test]
fn vqt_window_tiers_are_distinguishable() {
    let mut sharp = VqtAnalyzer::new();
    sharp.set_sample_rate(SR);
    sharp.set_gamma_hz(5.0);
    sharp.set_window(0);
    let mut clean = VqtAnalyzer::new();
    clean.set_sample_rate(SR);
    clean.set_gamma_hz(5.0);
    clean.set_window(1);

    // 喂一 hop 静音触发惰性重建（check_rebuild）。
    let zero = vec![0.0_f32; 1024];
    sharp.process_block(&zero, &zero, &zero);
    clean.process_block(&zero, &zero, &zero);

    assert_eq!(sharp.window_type(), 0);
    assert_eq!(clean.window_type(), 1);

    // 返回值语义（原版 `SetWindowType`：跨档 true，同档 false）在独立 scratch 上验证，
    // 避免改动上面两个激励实例的窗型。
    let mut scratch = VqtAnalyzer::new(); // 默认 CLEAN(1)
    assert!(!scratch.set_window(1), "同档重复设置应返回 false");
    assert!(scratch.set_window(0), "跨档应返回 true");
    assert_eq!(scratch.window_type(), 0);

    // 核系数：Hann 与 BH5 窗权重 / 归一化和不同，首核与 wsum_inv 必然不同。
    let b0s = sharp.band_debug(0);
    let b0c = clean.band_debug(0);
    println!(
        "6a: band0 wsum_inv sharp={:.6} clean={:.6}; kr0 sharp={:.6} clean={:.6}",
        b0s.wsum_inv, b0c.wsum_inv, b0s.kr0, b0c.kr0
    );
    assert!(
        (b0s.wsum_inv - b0c.wsum_inv).abs() > 1e-5,
        "两档 wsum_inv 未区分"
    );
    assert!((b0s.kr0 - b0c.kr0).abs() > 1e-6, "两档首核未区分");

    // 同一确定性伪白噪激励 3 hop，两档 L 带幅度谱存在可测量差异。
    let mut rng: u64 = 0x12345678;
    let mut noise = Vec::with_capacity(3 * 1024);
    for _ in 0..3 * 1024 {
        rng = rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        noise.push(((rng >> 33) as f32) / (1u32 << 31) as f32 - 1.0);
    }
    let mut peak_diff = 0.0_f32;
    let mut n_diff = 0usize;
    for chunk in noise.chunks(1024) {
        sharp.process_block(chunk, chunk, chunk);
        clean.process_block(chunk, chunk, chunk);
        let fs = sharp.take_frame().expect("sharp 帧");
        let fc = clean.take_frame().expect("clean 帧");
        for (vs, vc) in fs.vals.iter().zip(&fc.vals) {
            let d = (vs[0] - vc[0]).abs();
            if d > 1e-4 {
                n_diff += 1;
            }
            peak_diff = peak_diff.max(d);
        }
    }
    println!(
        "6b: 3 hop 后两档 L 带最大幅度差 {peak_diff:.6e}，差异带数 {n_diff}/{}",
        clean.num_bands()
    );
    assert!(peak_diff > 1e-3, "Hann/BH5 输出无可区分差异: {peak_diff}");
}

/// VQT 窗型（外壳级）：VQT 模式下切换 `WindowVQT` 参数，配置真正下发到引擎，
/// 同一激励两档帧内容可区分（覆盖 `apply_config` → `set_window` 下发路径）。
#[test]
fn vqt_window_param_flows_to_engine() {
    // CLEAN（参数默认档 1）参考壳。
    let (params_c, mut state_c) = make_state();
    params_c.mode.set_index(1);
    let mut l = vec![0.0; BLOCK];
    let mut r = vec![0.0; BLOCK];
    let mut rng_c: u64 = 0x12345678;
    for _ in 0..20 {
        fill_lcg(&mut rng_c, &mut l);
        r.copy_from_slice(&l);
        run_block(&mut state_c, &params_c, &l, &r);
    }
    let clean = latest_frame(&params_c).expect("CLEAN 帧").1;

    // SHARP 壳：建态后切到 0，首个 process 的 apply_config 下发重建。
    let (params_s, mut state_s) = make_state();
    params_s.mode.set_index(1);
    params_s.window_vqt.set_index(0);
    let mut rng_s: u64 = 0x12345678;
    for _ in 0..20 {
        fill_lcg(&mut rng_s, &mut l);
        r.copy_from_slice(&l);
        run_block(&mut state_s, &params_s, &l, &r);
    }
    let sharp = latest_frame(&params_s).expect("SHARP 帧").1;

    let max_diff = clean
        .l
        .iter()
        .zip(&sharp.l)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0_f32, f32::max);
    println!(
        "7: 外壳 CLEAN/SHARP L 带最大幅度差 {max_diff:.6e}（带数 {}）",
        clean.l.len()
    );
    assert!(max_diff > 1e-3, "外壳两档帧无差异，窗型未下发: {max_diff}");
}

/// 发生器 restart / reseed 请求位（外壳级）：UI 置位后音频线程下一 block 取走清零；
/// 相位 / 种子重置的确定性在 `generator_restart_and_reseed_determinism` DSP 用例验证。
#[test]
fn gen_restart_reseed_bits_consumed() {
    let (params, mut state) = make_state();
    let shared = &params.analysis.shared;
    shared.gen_active.store(true, Ordering::Relaxed);
    shared.gen_type.store(TestSignalType::WhiteNoise as i32, Ordering::Relaxed);
    shared
        .gen_freq_hz
        .store(1000.0_f64.to_bits(), Ordering::Relaxed);
    shared
        .gen_level_db
        .store((-12.0_f32).to_bits(), Ordering::Relaxed);

    let silence = vec![0.0_f32; BLOCK];
    for _ in 0..4 {
        run_block(&mut state, &params, &silence, &silence);
    }

    shared.gen_restart.store(true, Ordering::Relaxed);
    shared.gen_reseed.store(123_456_789, Ordering::Relaxed);
    run_block(&mut state, &params, &silence, &silence);

    println!(
        "8: 消费后 gen_restart={}, gen_reseed={}（期望 false / 0）",
        shared.gen_restart.load(Ordering::Relaxed),
        shared.gen_reseed.load(Ordering::Relaxed)
    );
    assert!(
        !shared.gen_restart.load(Ordering::Relaxed),
        "restart 位未被音频线程取走清零"
    );
    assert_eq!(
        shared.gen_reseed.load(Ordering::Relaxed),
        0,
        "reseed 位未被音频线程取走清零"
    );
}

/// 发生器重启 / 重播种（DSP 级）：restart 后信号流回到 t=0 逐样本一致；
/// 同种子可复现，异种子流不同。
#[test]
fn generator_restart_and_reseed_determinism() {
    let cfg = TestSignalConfig {
        type_: TestSignalType::WhiteNoise,
        freq_hz: 1000.0,
        level_db: 0.0,
        ..Default::default()
    };
    let mut g = TestSignalGenerator::new();
    g.set_config(cfg);

    let mut first_l = vec![0.0_f32; 1024];
    let mut first_r = vec![0.0_f32; 1024];
    g.fill(&mut first_l, &mut first_r, SR, true);

    // 推进若干 block 后重启，首块应与冷启动逐样本一致。
    let mut skip_l = vec![0.0_f32; 1024];
    let mut skip_r = vec![0.0_f32; 1024];
    for _ in 0..4 {
        g.fill(&mut skip_l, &mut skip_r, SR, true);
    }
    g.restart();
    let mut back_l = vec![0.0_f32; 1024];
    let mut back_r = vec![0.0_f32; 1024];
    g.fill(&mut back_l, &mut back_r, SR, true);
    assert_eq!(first_l, back_l, "restart 后 L 路未回到 t=0 样本流");
    assert_eq!(first_r, back_r, "restart 后 R 路未回到 t=0 样本流");

    // 种子：A 记录 → B 不同 → 回 A 复现。
    g.set_seed(0x1111_1111);
    let mut xa = vec![0.0_f32; 1024];
    let mut xar = vec![0.0_f32; 1024];
    g.fill(&mut xa, &mut xar, SR, true);
    g.set_seed(0x2222_2222);
    let mut xb = vec![0.0_f32; 1024];
    let mut xbr = vec![0.0_f32; 1024];
    g.fill(&mut xb, &mut xbr, SR, true);
    let diff_ab = xa
        .iter()
        .zip(&xb)
        .map(|(x, y)| (x - y).abs())
        .fold(0.0_f32, f32::max);
    println!("9: 种子 A/B 首块 L 最大差 {diff_ab:.6e}（期望 > 0）");
    assert!(diff_ab > 1e-6, "异种子噪声流相同");

    g.set_seed(0x1111_1111);
    let mut xc = vec![0.0_f32; 1024];
    let mut xcr = vec![0.0_f32; 1024];
    g.fill(&mut xc, &mut xcr, SR, true);
    assert_eq!(xa, xc, "回种 A 后噪声流未复现");
}
