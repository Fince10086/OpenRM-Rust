//! 音频线程 ↔ UI 线程的共享分析通道。
//!
//! 原版 iPlug2 里这条通道是一组 `std::atomic` 标量 + `ISender` 消息包
//! （`SpectrumPad` / `AnalyzerMeterUi` / `CpuMeterControl` 各自收一帧）。
//! Rust 侧等价物是本模块的 [`AnalyzerShared`]：
//!
//! * 频谱 / 示波器帧、电平 / 响度快照走 `Mutex` 整体替换——临界区内只做
//!   指针拷贝，不做 dB 换算或弹道平滑（那些都在 UI 控件里，与原版一致）；
//! * CPU 占用、采样率、复位请求位、发生器开关走原子量，音频线程写、
//!   UI 线程读（或反向置位请求位，音频线程下一 block 取走后清零）。
//!
//! 字段名是与界面任务冻结的契约，新增字段只能往后加，不得改名。

use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use openrm_analyzer_dsp::level_meter::LevelSnapshot;
use openrm_analyzer_dsp::loudness_meter::LoudnessSnapshot;

use crate::ui::data::{ScopeFrame, SpectrumFrame};

/// 冻结环形缓冲长度（原版 `Analyzer.h:159` `kFreezeRingLen = 1 << 19`，
/// ≈10.92 s @48k，覆盖最长释放弹道收敛与长时基示波）。
pub const FREEZE_RING_LEN: usize = 1 << 19;

/// 冻结回放每 tick 泵送帧数（原版 `Analyzer.h:173`
/// `kReplayFramesPerTick = 16`）。
pub const REPLAY_FRAMES_PER_TICK: usize = 16;

/// 分析 hop（样本），与四个引擎 / 示波器内部 `K_HOP` 一致。
pub const HOP: usize = 1024;

/// M 路归一化系数 `1/√2`（原版 `Analyzer.cpp:792/816/821`
/// `0.7071067811865475`）。
pub const M_COEFF: f32 = 0.707_106_781_186_547_5;

/// 音频线程 ↔ UI 线程共享数据。
///
/// 契约字段（与界面任务共用，字段名不可改）：
///
/// | 字段 | 方向 | 含义 |
/// |---|---|---|
/// | `spectrum` | 音频→UI | `(模式索引 0..3, 最新频谱帧)`，线性幅度 |
/// | `level` | 音频→UI | 电平表快照（peak/true/rms/vu/hold/persist/over） |
/// | `loudness` | 音频→UI | 响度计快照（M/S/I/LRA/TP） |
/// | `scope` | 音频→UI | 最新 hop 的 L/R 样本对 |
/// | `cpu` | 音频→UI | CPU 占用，`f32` bits，0..1 |
/// | `sample_rate` | 音频→UI | 当前采样率（Hz） |
/// | `gen_active` | UI→音频 | 内置测试信号发生器开关 |
/// | `gen_restart` | UI→音频 | 发生器相位/PRNG 重启请求（`exchange(false)` 消费） |
/// | `gen_reseed` | UI→音频 | 发生器重播种请求（非零种子，`exchange(0)` 消费） |
/// | `reset_level` | UI→音频 | 清电平表全量（hold/over/persist/锁存） |
/// | `reset_loud` | UI→音频 | 清响度计 |
/// | `reset_persist` | UI→音频 | 清 persist 余辉 |
/// | `reset_meter_hold` | UI→音频 | 清电平表 hold 线 |
/// | `reset_over` | UI→音频 | 清 over 锁存 |
/// | `reset_hold` | UI→音频 | 清峰值保持（hold + vuHold） |
#[derive(Default)]
pub struct AnalyzerShared {
    /// 最新频谱帧：`(模式索引 0..3, 帧)`；线性幅度，dB 换算在 UI 侧。
    pub spectrum: Mutex<Option<(u8, SpectrumFrame)>>,
    /// 电平表最新快照。
    pub level: Mutex<LevelSnapshot>,
    /// 响度计最新快照。
    pub loudness: Mutex<LoudnessSnapshot>,
    /// 示波器 / 声像图最新 hop 数据包。
    pub scope: Mutex<Option<ScopeFrame>>,
    /// CPU 占用（`f32::to_bits()`，0..1）。
    pub cpu: AtomicU32,
    /// 频谱帧发布序号：每发布一帧自增，测试/UI 据此判断帧是否更新。
    pub spectrum_seq: AtomicU32,
    /// 当前采样率（Hz）。
    pub sample_rate: AtomicU32,
    /// 测试信号发生器是否启用。
    pub gen_active: AtomicBool,

    /// 复位请求：电平表全量（原版 `mLevelResetFlag` → `ResetHoldOver`）。
    pub reset_level: AtomicBool,
    /// 复位请求：响度计（原版 `mLoudResetFlag`）。
    pub reset_loud: AtomicBool,
    /// 复位请求：persist 余辉（原版 `mLevelResetPersistFlag`）。
    pub reset_persist: AtomicBool,
    /// 复位请求：电平表 hold 线（原版 `mLevelResetMeterHoldFlag`）。
    pub reset_meter_hold: AtomicBool,
    /// 复位请求：over 锁存（原版 `mLevelResetOverFlag`）。
    pub reset_over: AtomicBool,
    /// 复位请求：峰值保持（原版 `mLevelResetHoldFlag` → `ResetHold`）。
    pub reset_hold: AtomicBool,

    // ---- 测试信号发生器配置（UI→音频；对齐原版 `mGen*` 原子标量） ----
    /// 信号类型（`TestSignalType` 下标，0=Off）。
    pub gen_type: AtomicI32,
    /// 信号频率（Hz），`f64::to_bits()`。
    pub gen_freq_hz: AtomicU64,
    /// 信号电平（dBFS 峰值），`f32::to_bits()`。
    pub gen_level_db: AtomicU32,
    /// 冻结时是否暂停推进发生器相位（原版 `mGenHold`）。
    pub gen_hold: AtomicBool,
    /// 发生器信号是否直送监听输出（原版 `mGenToOutput`）。
    pub gen_to_output: AtomicBool,
    /// 发生器重启请求（原版 `mGenRestartReq`）：UI 置位，音频线程在发生器分支
    /// `exchange(false)` 取走后调 `TestSignalGenerator::restart()`（相位/PRNG/粉噪清零）。
    pub gen_restart: AtomicBool,
    /// 发生器重播种请求（原版 `mGenSeedReq`）：`0` = 无请求；非零即新种子（正 int，
    /// 对齐原版 `& 0x7FFFFFFF` 掩码）。音频线程 `exchange(0)` 取走后 `set_seed`。
    pub gen_reseed: AtomicI32,
}

impl AnalyzerShared {
    /// 新建一份共享状态，包进 `Arc` 跨线程持有。
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// 读 CPU 占用（0..1）。
    pub fn cpu_f32(&self) -> f32 {
        f32::from_bits(self.cpu.load(Ordering::Relaxed))
    }

    /// 写 CPU 占用（0..1 钳制）。
    pub fn store_cpu(&self, ratio: f64) {
        let v = ratio.clamp(0.0, 1.0) as f32;
        self.cpu.store(v.to_bits(), Ordering::Relaxed);
    }

    /// 读采样率（Hz），缺省 48 kHz。
    pub fn sample_rate_f64(&self) -> f64 {
        self.sample_rate.load(Ordering::Relaxed) as f64
    }

    /// 取走一个复位请求位：置位则返回 `true` 并清零（原版 `exchange(false)` 语义）。
    pub fn take_reset(&self, flag: &AtomicBool) -> bool {
        flag.swap(false, Ordering::Relaxed)
    }
}

