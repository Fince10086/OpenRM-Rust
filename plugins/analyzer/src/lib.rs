//! ORM Analyzer — truce 插件外壳（音频线程逻辑）。
//!
//! 处理链逐行对齐原版 `plugins/Analyzer/src/Analyzer.cpp::ProcessBlock`
//! （Analyzer.cpp:729-937）：
//!
//! 1. 直通拷贝输入（单声道 R=L）；
//! 2. 内置测试信号启用时替换输入（左右独立；可选直送监听输出）；
//! 3. 写冻结环形缓冲；活动引擎（STFT / VQT / PBT / RTA，四选一）+ 立体声示波器；
//! 4. 电平表（peak / true peak / RMS / VU / hold / persist / over）→ 响度计；
//! 5. 引擎 / 示波器帧发布到 [`analysis::AnalyzerShared`]，UI 线程只读。
//!
//! 冻结时不再喂新样本，而是把环形历史按 hop 回放给引擎
//! （`kReplayFramesPerTick = 16`，原版 `PumpFreezeReplay` Analyzer.cpp:1465-1506），
//! 保证冻结画面与释放弹道一致。

use std::sync::atomic::Ordering;
use std::time::Instant;

use openrm_analyzer_dsp::frame::EngineFrame;
use openrm_analyzer_dsp::level_meter::LevelMeter;
use openrm_analyzer_dsp::loudness_meter::{LoudnessMeter, LoudnessSnapshot};
use openrm_analyzer_dsp::pbt::PbtAnalyzer;
use openrm_analyzer_dsp::rta::RtaAnalyzer;
use openrm_analyzer_dsp::stereo_scope::StereoScope;
use openrm_analyzer_dsp::stft::StftAnalyzer;
use openrm_analyzer_dsp::test_signal::{
    TestSignalConfig, TestSignalGenerator, TestSignalType,
};
use openrm_analyzer_dsp::vqt::VqtAnalyzer;
use truce::prelude::*;

pub mod analysis;
pub mod editor;
pub mod params;
pub mod ui;

use analysis::{
    AnalyzerShared, FREEZE_RING_LEN, HOP, M_COEFF, REPLAY_FRAMES_PER_TICK,
};
use params::AnalyzerParams;
use ui::data::{ScopeFrame, SpectrumFrame};

/// 单块样本上限（原版 `kMaxBlock = 16384`，Analyzer.cpp:731）。
const K_MAX_BLOCK: usize = 16_384;
const RING_MASK: usize = FREEZE_RING_LEN - 1;

/// 分析引擎档位（原版 `EAnalyzerMode`，Params.h:70-76）；0=STFT 由 `_` 臂兜底。
mod mode {
    pub const VQT: i32 = 1;
    pub const PBT: i32 = 2;
    pub const RTA: i32 = 3;
}

/// truce 插件描述符（无状态；音频运行态在 [`AnalyzerDsp`]）。
pub struct Analyzer;

/// 音频线程运行态：四引擎 + 电平 / 响度 / 示波器 + 测试信号发生器 + 冻结回放状态。
/// 引擎句柄全部 `Box` 持有：STFT 等内部带百 KB 级定长数组，避免运行态落栈。
pub struct AnalyzerDsp {
    stft: Box<StftAnalyzer>,
    vqt: Box<VqtAnalyzer>,
    pbt: Box<PbtAnalyzer>,
    rta: Box<RtaAnalyzer>,
    scope: Box<StereoScope>,
    meter: Box<LevelMeter>,
    loud: Box<LoudnessMeter>,
    gen: Box<TestSignalGenerator>,

    /// 当前块的分析输入（L / R / M），长度在 `reset` 时按 max_block 预置。
    in_l: Vec<f32>,
    in_r: Vec<f32>,
    in_m: Vec<f32>,

    /// 冻结环形缓冲：通道 0=L，1=R，2=M（原版 `mFreezeRing`，Analyzer.h:160）。
    /// 每路一个堆上 Vec，避免 6.3 MB 数组落栈。
    freeze_ring: [Vec<f32>; 3],
    /// 环形写指针（下一个样本槽位）。
    freeze_pos: usize,

    /// 各引擎输入侧 hop 相位（原版 `mEngineHopPhase`，Analyzer.h:166），用于回放对齐。
    ///
    /// 自跟踪口径（`(phase + n) % HOP`）而非读取引擎内部 `buf_count`：
    ///
    /// * 四个引擎的输入计数 `buf_count` 均为私有字段；C++ 经 `HopPhase()` getter
    ///   （VQTAnalyzer.h:377 / PBTAnalyzer.h:116 / RTAAnalyzer.h:120）暴露，Rust 侧
    ///   stft/pbt/rta 属其他任务文件，本轮不跨归属加 getter；
    /// * 等价性证明：冷启动 `buf_count = 0`、外壳 `hop_phase = [0; 4]` 同步归零；
    ///   非冻结分支每块外壳把同一块 `n` 个样本喂给活动引擎，引擎侧
    ///   `buf_count = (buf_count + n) mod hop`，外壳侧 `phase = (phase + n) mod HOP`；
    ///   四引擎 hop 恒为 1024（STFT：`hop = fft_size/overlap`，Res 三档
    ///   2048/4096/8192 对应 overlap 2/4/8，hop 均为 1024；PBT/RTA/VQT `K_HOP=1024`），
    ///   故模数相同、初值相同、每步增量相同 → 两计数逐块相等；
    /// * 冻结回放分支外壳不推进 `hop_phase`，与原版只在非冻结分支写
    ///   `mEngineHopPhase`（Analyzer.cpp:834-843）一致；回放起点由冻结前定格的相位推算。
    hop_phase: [usize; 4],

    /// 冻结回放状态（原版 `mReplayMode / mReplayFrame / mReplayFrames /
    /// mReplayLastStart`，Analyzer.h:168-172）。
    replay_mode: i32,
    replay_frame: usize,
    replay_n_frames: usize,
    replay_last_start: usize,

    /// 已下发到引擎的配置（去重，避免每块触发重建）。
    applied: AppliedConfig,

    /// 上一块是否处于冻结（检测冻结/解冻沿）。
    frozen_prev: bool,
    /// 音频 CPU 平滑值（0..1，原版 `mCpuAudio` 一阶平滑 0.1）。
    cpu_audio: f64,
}

/// 引擎侧关心的配置快照（显示类参数——斜率 / 范围 / 释放时间——不进 DSP）。
#[derive(Clone, Copy, Default, PartialEq)]
struct AppliedConfig {
    sr: f64,
    fft_size: usize,
    fft_window: i32,
    vqt_window: i32,
    gamma_hz: f64,
    pbt_lf_hz: i32,
    rta_mode: i32,
    chan_tri: i32,
    mode: i32,
}

impl AnalyzerDsp {
    fn fresh() -> Self {
        AnalyzerDsp {
            stft: Box::new(StftAnalyzer::new()),
            vqt: Box::new(VqtAnalyzer::new()),
            pbt: Box::new(PbtAnalyzer::new()),
            rta: Box::new(RtaAnalyzer::new()),
            scope: Box::new(StereoScope::new()),
            meter: Box::new(LevelMeter::new()),
            loud: Box::new(LoudnessMeter::new()),
            gen: Box::new(TestSignalGenerator::new()),
            in_l: Vec::new(),
            in_r: Vec::new(),
            in_m: Vec::new(),
            freeze_ring: [
                vec![0.0; FREEZE_RING_LEN],
                vec![0.0; FREEZE_RING_LEN],
                vec![0.0; FREEZE_RING_LEN],
            ],
            freeze_pos: 0,
            hop_phase: [0; 4],
            replay_mode: -1,
            replay_frame: 0,
            replay_n_frames: 0,
            replay_last_start: 0,
            applied: AppliedConfig::default(),
            frozen_prev: false,
            cpu_audio: 0.0,
        }
    }
}

impl Default for AnalyzerDsp {
    fn default() -> Self {
        Self::fresh()
    }
}

impl PluginLogic for Analyzer {
    type Params = AnalyzerParams;
    type DspState = AnalyzerDsp;

    fn init(_params: &AnalyzerParams, _cx: &InitContext) -> AnalyzerDsp {
        AnalyzerDsp::fresh()
    }

    fn reset(dsp: &mut AnalyzerDsp, params: &AnalyzerParams, config: &AudioConfig) {
        let sr = config.sample_rate;
        let max_block = config.max_block_size.max(1);
        for buf in [&mut dsp.in_l, &mut dsp.in_r, &mut dsp.in_m] {
            buf.clear();
            buf.resize(max_block, 0.0);
        }

        apply_config(dsp, params, sr);

        // 清引擎运行态（原版 OnReset Analyzer.cpp:940-957 的引擎 Clear 语义）。
        dsp.stft.reset();
        dsp.vqt.reset();
        dsp.pbt.reset();
        dsp.rta.reset();
        dsp.scope.reset();
        dsp.meter.reset();
        dsp.loud.reset();

        dsp.freeze_pos = 0;
        dsp.hop_phase = [0; 4];
        dsp.replay_mode = -1;
        dsp.replay_frame = 0;
        dsp.replay_n_frames = 0;
        dsp.cpu_audio = 0.0;
        dsp.frozen_prev = false;

        params.analysis.store_sample_rate(sr);
        params
            .analysis
            .shared
            .sample_rate
            .store(sr.round() as u32, Ordering::Relaxed);
    }

    fn process(
        dsp: &mut AnalyzerDsp,
        params: &AnalyzerParams,
        buffer: &mut AudioBuffer<f32>,
        _events: &EventList,
        _context: &mut ProcessContext,
    ) -> ProcessStatus {
        let t0 = Instant::now();
        let sr = params.analysis.sample_rate();
        let n = buffer.num_samples().min(K_MAX_BLOCK);
        let n_in = buffer.num_input_channels();
        let n_out = buffer.num_output_channels();
        let shared = &params.analysis.shared;

        // ── 1. 直通拷贝（原版 737-761）────────────────────────────
        // 先把输入摘到本地，避免 input 不可变借用与 output 可变借用冲突。
        let in0: Vec<f32> = if n_in >= 1 {
            buffer.input(0)[..n].to_vec()
        } else {
            vec![0.0; n]
        };
        let in1: Vec<f32> = if n_in >= 2 {
            buffer.input(1)[..n].to_vec()
        } else {
            in0.clone()
        };
        if n_out >= 1 {
            buffer.output(0)[..n].copy_from_slice(&in0);
        }
        if n_out >= 2 {
            buffer.output(1)[..n].copy_from_slice(&in1);
        }

        let frozen = params.freeze.value();
        let mode = params.mode.value() as i32;

        // ── 2. 构造分析输入（原版 770-821）────────────────────────
        let gen_on = shared.gen_active.load(Ordering::Relaxed);
        if gen_on {
            let type_ = TestSignalType::from_i32(shared.gen_type.load(Ordering::Relaxed));
            let freq = f64::from_bits(shared.gen_freq_hz.load(Ordering::Relaxed));
            let level_db = f32::from_bits(shared.gen_level_db.load(Ordering::Relaxed)) as f64;
            // 重启 / 重播种请求：UI 置位，音频线程取走后清零（原版 Analyzer.cpp:779-782，
            // `exchange(false)` / `exchange(0)` 语义）。仅在发生器开启分支内消费，与原版一致。
            if shared.take_reset(&shared.gen_restart) {
                dsp.gen.restart();
            }
            let seed_req = shared.gen_reseed.swap(0, Ordering::Relaxed);
            if seed_req != 0 {
                dsp.gen.set_seed(seed_req as u32);
            }
            let advance = !(frozen && shared.gen_hold.load(Ordering::Relaxed));
            dsp.gen.set_config(TestSignalConfig {
                type_,
                freq_hz: freq,
                level_db,
                fft_size: dsp.applied.fft_size as i32,
                ..Default::default()
            });
            dsp.gen.fill(&mut dsp.in_l[..n], &mut dsp.in_r[..n], sr, advance);
            for i in 0..n {
                dsp.in_m[i] = (dsp.in_l[i] + dsp.in_r[i]) * M_COEFF;
            }
            // 发生器信号直送监听输出：把发生器 L/R 限幅 -1 dBFS 后写回输出
            // （原版 793-803：outputs[0]=clamp(mSpecInL)，非对直通信号限幅）。
            if shared.gen_to_output.load(Ordering::Relaxed) {
                const CEIL: f32 = 0.891_250_438_133_789_1;
                if n_out >= 1 {
                    for (o, &g) in buffer.output(0)[..n].iter_mut().zip(dsp.in_l[..n].iter()) {
                        *o = g.clamp(-CEIL, CEIL);
                    }
                }
                if n_out >= 2 {
                    for (o, &g) in buffer.output(1)[..n].iter_mut().zip(dsp.in_r[..n].iter()) {
                        *o = g.clamp(-CEIL, CEIL);
                    }
                }
            }
        } else if n_in >= 2 {
            dsp.in_l[..n].copy_from_slice(&in0);
            dsp.in_r[..n].copy_from_slice(&in1);
            for i in 0..n {
                dsp.in_m[i] = (dsp.in_l[i] + dsp.in_r[i]) * M_COEFF;
            }
        } else {
            dsp.in_l[..n].copy_from_slice(&in0);
            dsp.in_r[..n].copy_from_slice(&dsp.in_l[..n]);
            for i in 0..n {
                dsp.in_m[i] = dsp.in_l[i] * M_COEFF;
            }
        }

        // ── 3. 引擎 / 示波器 / 环形缓冲（原版 823-849）────────────
        let config_changed = apply_config(dsp, params, sr);

        if !frozen {
            for i in 0..n {
                let p = dsp.freeze_pos;
                dsp.freeze_ring[0][p] = dsp.in_l[i];
                dsp.freeze_ring[1][p] = dsp.in_r[i];
                dsp.freeze_ring[2][p] = dsp.in_m[i];
                dsp.freeze_pos = (p + 1) & RING_MASK;
            }
            let (il, ir, im) = (&dsp.in_l[..n], &dsp.in_r[..n], &dsp.in_m[..n]);
            match mode {
                mode::VQT => dsp.vqt.process_block(il, ir, im),
                mode::PBT => dsp.pbt.process_block(il, ir, im),
                mode::RTA => dsp.rta.process_block(il, ir, im),
                _ => dsp.stft.process_block(il, ir, im),
            }
            dsp.hop_phase[mode as usize] = (dsp.hop_phase[mode as usize] + n) % HOP;
            dsp.scope.process_block(&dsp.in_l[..n], &dsp.in_r[..n], &dsp.in_m[..n]);
            dsp.replay_mode = -1;
        } else {
            pump_freeze_replay(dsp, params, shared, mode, config_changed);
        }

        // ── 4. 复位请求消费 + 电平 / 响度（原版 863-929）──────────
        consume_resets(dsp, shared);

        if !frozen {
            dsp.meter.process(
                &dsp.in_l[..n],
                &dsp.in_r[..n],
                params.level_mode.value() as usize,
                params.hold_sec(),
            );
            let snap = dsp.meter.snapshot();
            *shared.level.lock().unwrap() = snap;
            dsp.loud.process(&dsp.in_l[..n], &dsp.in_r[..n]);
            dsp.loud.set_true_peaks(snap.true_db_l, snap.true_db_r);
            *shared.loudness.lock().unwrap() = dsp.loud.store();
        }

        // ── 5. 帧发布 ─────────────────────────────────────────────
        let engine_frame = match mode {
            mode::VQT => dsp.vqt.take_frame(),
            mode::PBT => dsp.pbt.take_frame(),
            mode::RTA => dsp.rta.take_frame(),
            _ => dsp.stft.take_frame(),
        };
        if let Some(ef) = engine_frame {
            *shared.spectrum.lock().unwrap() = Some((mode as u8, frame_from_engine(ef)));
            shared.spectrum_seq.fetch_add(1, Ordering::Relaxed);
        }
        if !frozen {
            if let Some(ef) = dsp.scope.take_frame() {
                *shared.scope.lock().unwrap() = Some(scope_from_engine(ef));
            }
        }

        // ── 6. CPU 占用（原版 1290-1294 一阶平滑口径）────────────
        let process_ms = t0.elapsed().as_secs_f64() * 1000.0;
        let block_ms = n as f64 / sr.max(1.0) * 1000.0;
        if block_ms > 0.0 {
            dsp.cpu_audio += (process_ms / block_ms - dsp.cpu_audio) * 0.1;
        }
        shared.store_cpu(dsp.cpu_audio);

        dsp.frozen_prev = frozen;
        ProcessStatus::Normal
    }

    fn editor(params: Arc<AnalyzerParams>) -> Box<dyn Editor> {
        editor::create(params)
    }
}

truce::plugin! {
    logic: Analyzer,
    params: AnalyzerParams,
}

// ── 引擎调度 ────────────────────────────────────────────────────────────────

/// 把参数下发到引擎；返回引擎侧配置是否发生变化（冻结时用于重启回放）。
fn apply_config(dsp: &mut AnalyzerDsp, params: &AnalyzerParams, sr: f64) -> bool {
    let cfg = AppliedConfig {
        sr,
        fft_size: params.fft_size(),
        fft_window: params.fft_window.index() as i32,
        vqt_window: params.window_vqt.index() as i32,
        gamma_hz: params.vqt_gamma_hz(),
        pbt_lf_hz: params.pbt_lf_width_hz() as i32,
        rta_mode: params.rta_octave_index() as i32,
        chan_tri: params.channel_mode.value() as i32,
        mode: params.mode.value() as i32,
    };
    let a = &dsp.applied;
    let mut changed = false;

    if (cfg.sr - a.sr).abs() > 0.01 {
        dsp.stft.set_sample_rate(cfg.sr);
        dsp.vqt.set_sample_rate(cfg.sr);
        dsp.pbt.set_sample_rate(cfg.sr);
        dsp.rta.set_sample_rate(cfg.sr);
        dsp.scope.set_sample_rate(cfg.sr);
        dsp.meter.set_sample_rate(cfg.sr);
        dsp.loud.set_sample_rate(cfg.sr);
        changed = true;
    }
    if cfg.fft_size != a.fft_size {
        dsp.stft.set_fft_size(cfg.fft_size);
        changed = true;
    }
    if cfg.fft_window != a.fft_window {
        dsp.stft.set_window(cfg.fft_window);
        changed = true;
    }
    if cfg.vqt_window != a.vqt_window {
        dsp.vqt.set_window(cfg.vqt_window);
        changed = true;
    }
    if cfg.gamma_hz != a.gamma_hz {
        dsp.vqt.set_gamma_hz(cfg.gamma_hz);
        changed = true;
    }
    if cfg.pbt_lf_hz != a.pbt_lf_hz {
        dsp.pbt.set_lf_width(cfg.pbt_lf_hz);
        changed = true;
    }
    if cfg.rta_mode != a.rta_mode {
        dsp.rta.set_octave_mode(cfg.rta_mode);
        changed = true;
    }
    if cfg.chan_tri != a.chan_tri {
        dsp.stft.set_channel_mode(cfg.chan_tri);
        dsp.vqt.set_channel_mode(cfg.chan_tri);
        dsp.pbt.set_channel_mode(cfg.chan_tri);
        dsp.rta.set_channel_mode(cfg.chan_tri);
        changed = true;
    }

    dsp.applied = cfg;
    changed
}

// ── 冻结回放 ────────────────────────────────────────────────────────────────

/// 进入冻结 / 引擎配置或模式变化时，按环形历史起点初始化回放
/// （原版 `StartFreezeReplay`，Analyzer.cpp:1442-1462）。
fn start_replay(dsp: &mut AnalyzerDsp, mode: i32) {
    let p = dsp.freeze_pos;
    let b = dsp.hop_phase[mode as usize];
    let last_start = p.wrapping_sub(b).wrapping_sub(HOP) & RING_MASK;
    let n_frames =
        (last_start.wrapping_sub(p).wrapping_add(FREEZE_RING_LEN) & RING_MASK) / HOP + 1;

    dsp.replay_last_start = last_start;
    dsp.replay_n_frames = n_frames;
    dsp.replay_frame = 0;
    dsp.replay_mode = mode;

    match mode {
        mode::VQT => dsp.vqt.reset(),
        mode::PBT => dsp.pbt.reset(),
        mode::RTA => dsp.rta.reset(),
        _ => dsp.stft.reset(),
    }
}

/// 冻结态：每块最多泵送 `REPLAY_FRAMES_PER_TICK` 帧历史 hop 包给活动引擎
/// （原版 `PumpFreezeReplay`，Analyzer.cpp:1465-1506）。
fn pump_freeze_replay(
    dsp: &mut AnalyzerDsp,
    _params: &AnalyzerParams,
    shared: &AnalyzerShared,
    mode: i32,
    config_changed: bool,
) {
    if dsp.replay_mode < 0 || dsp.replay_mode != mode || config_changed {
        start_replay(dsp, mode);
    }
    if dsp.replay_mode < 0 || dsp.replay_n_frames == 0 {
        return;
    }

    let end = (dsp.replay_frame + REPLAY_FRAMES_PER_TICK).min(dsp.replay_n_frames);
    let mut packet_l = [0.0; HOP];
    let mut packet_r = [0.0; HOP];
    let mut packet_m = [0.0; HOP];

    while dsp.replay_frame < end {
        let off = (dsp.replay_n_frames - 1 - dsp.replay_frame) * HOP;
        let start = dsp.replay_last_start.wrapping_sub(off) & RING_MASK;
        ring_copy(&dsp.freeze_ring[0], start, &mut packet_l);
        ring_copy(&dsp.freeze_ring[1], start, &mut packet_r);
        ring_copy(&dsp.freeze_ring[2], start, &mut packet_m);

        let (pl, pr, pm) = (&packet_l, &packet_r, &packet_m);
        match mode {
            mode::VQT => dsp.vqt.process_block(pl, pr, pm),
            mode::PBT => dsp.pbt.process_block(pl, pr, pm),
            mode::RTA => dsp.rta.process_block(pl, pr, pm),
            _ => dsp.stft.process_block(pl, pr, pm),
        }
        let engine_frame = match mode {
            mode::VQT => dsp.vqt.take_frame(),
            mode::PBT => dsp.pbt.take_frame(),
            mode::RTA => dsp.rta.take_frame(),
            _ => dsp.stft.take_frame(),
        };
        if let Some(ef) = engine_frame {
            *shared.spectrum.lock().unwrap() =
                Some((mode as u8, frame_from_engine(ef)));
            shared.spectrum_seq.fetch_add(1, Ordering::Relaxed);
        }
        dsp.scope.process_block(&packet_l, &packet_r, &packet_m);
        if let Some(ef) = dsp.scope.take_frame() {
            *shared.scope.lock().unwrap() = Some(scope_from_engine(ef));
        }
        dsp.replay_frame += 1;
    }
}

fn ring_copy(ring: &[f32], start: usize, dst: &mut [f32; HOP]) {
    for i in 0..HOP {
        dst[i] = ring[(start + i) & RING_MASK];
    }
}

// ── 复位请求 ────────────────────────────────────────────────────────────────

/// 取走 UI 置位的复位请求并落到对应表计（原版 863-929 的冻结 / 实时两分支合并）。
fn consume_resets(dsp: &mut AnalyzerDsp, shared: &AnalyzerShared) {
    if shared.take_reset(&shared.reset_hold) {
        dsp.meter.reset_hold();
        republish_level(dsp, shared);
    }
    if shared.take_reset(&shared.reset_level) {
        dsp.meter.reset_hold_over();
        republish_level(dsp, shared);
    }
    if shared.take_reset(&shared.reset_persist) {
        dsp.meter.reset_persist();
        republish_level(dsp, shared);
    }
    if shared.take_reset(&shared.reset_meter_hold) {
        dsp.meter.reset_meter_hold();
        republish_level(dsp, shared);
    }
    if shared.take_reset(&shared.reset_over) {
        dsp.meter.reset_over();
        republish_level(dsp, shared);
    }
    if shared.take_reset(&shared.reset_loud) {
        dsp.loud.reset();
        *shared.loudness.lock().unwrap() = LoudnessSnapshot::default();
    }
}

fn republish_level(dsp: &AnalyzerDsp, shared: &AnalyzerShared) {
    *shared.level.lock().unwrap() = dsp.meter.snapshot();
}

// ── 帧转换 ──────────────────────────────────────────────────────────────────

/// `EngineFrame`（槽位 0=L 1=R 2=M，线性幅度）→ UI 频谱帧。
fn frame_from_engine(ef: EngineFrame) -> SpectrumFrame {
    let EngineFrame { vals, freqs } = ef;
    let n = vals.len();
    let mut l = Vec::with_capacity(n);
    let mut r = Vec::with_capacity(n);
    let mut m = Vec::with_capacity(n);
    for v in vals {
        l.push(v[0]);
        r.push(v[1]);
        m.push(v[2]);
    }
    SpectrumFrame { l, r, m, freqs }
}

/// `EngineFrame` → 示波器帧（只取 L/R）。
fn scope_from_engine(ef: EngineFrame) -> ScopeFrame {
    let EngineFrame { vals, .. } = ef;
    ScopeFrame {
        l: vals.iter().map(|v| v[0]).collect(),
        r: vals.iter().map(|v| v[1]).collect(),
    }
}
