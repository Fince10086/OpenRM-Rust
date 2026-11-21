//! ORM Analyzer 参数与状态层（移植自 C++/iPlug2 原版 `Params.h` / `Analyzer.cpp`）。
//!
//! # 权威来源
//!
//! - 原版枚举与档位常量：`plugins/Analyzer/src/Params.h`
//! - 默认值 / 范围 / 宿主参数名：`plugins/Analyzer/src/Analyzer.cpp` 构造函数里
//!   的 `GetParam(kX)->InitInt(...)` / `InitBool(...)`
//! - 档位 → 引擎单位换算：`Analyzer.h` 的 `Current*()` 访问器与 `Analyzer.cpp`
//!   的 `SpeedReleaseSec()` / `EffectiveSlopeDb()`
//! - 状态文件：`StateFileIO.h`（魔数 `analyzer`，JSON，`version=1`，`values` 数组）
//! - 设置文件：`SettingsFileIO.h`（`ORMAnalyzer.settings`，可选键）
//!
//! # 参数数量与 id 顺序
//!
//! 以原版 `EParams` 枚举顺序为准，共 **21** 个参数，id 0..=20（任务背景书写的
//! "22 个参数"与枚举不符；`Params.h` 实际枚举到 `kLoudScale` 共 21 项，以源码为准）：
//!
//! | id | 原版枚举 | 宿主参数名 | 默认 | 档位 |
//! |---|---|---|---|---|
//! | 0 | kSpeed | Speed | 4 (MAX) | MIN/SLOW/MED/FAST/MAX |
//! | 1 | kReleaseMode | ReleaseMode | 0 (LOG) | LOG/LIN |
//! | 2 | kRange | Range | 0 | -80/-100/-120 dB |
//! | 3 | kRes | Res | 1 (M) | L/M/H → 2048/4096/8192 |
//! | 4 | kLfRes | LfRes | 2 (H) | L/M/H → 40/20/10 Hz |
//! | 5 | kMode | Mode | 0 (STFT) | STFT/VQT/PBT/RTA |
//! | 6 | kChannelMode | ChanMode | 0 (PWR) | PWR/L/R/SUM |
//! | 7 | kLevelMode | LevelMode | 0 (dBTP) | dBTP/dBFS |
//! | 8 | kLevelHold | LevelHold | 1 (2s) | 0.5s/2s/∞ |
//! | 9 | kLevelHoldOn | LevelHoldOn | false | bool |
//! | 10 | kFreeze | Freeze | false | bool |
//! | 11 | kSlopeFFT | SlopeFFT | 1 | 0/3/4.5 dB/Oct |
//! | 12 | kSlopeVQT | SlopeVQT | 1 | -3/0/1.5 dB/Oct |
//! | 13 | kSlopePBT | SlopePBT | 1 | -3/0/1.5 dB/Oct |
//! | 14 | kSlopeRTA | SlopeRTA | 1 | -3/0/1.5 dB/Oct |
//! | 15 | kRtaOctave | RtaOctave | 0 (1/6) | L/M/H → 1/6,1/12,1/24 |
//! | 16 | kFFTWindow | WindowFFT | 0 (SHARP) | SHARP/CLEAN |
//! | 17 | kWindowVQT | WindowVQT | 1 (CLEAN) | SHARP/CLEAN |
//! | 18 | kVQTGamma | VQTGamma | 2 (H) | L/M/H → 20/10/5 Hz |
//! | 19 | kLoudPreset | LoudPreset | 1 (-14) | -9/-14/-23/-24 LUFS |
//! | 20 | kLoudScale | LoudScale | 0 (+9) | +9/+18 LU |
//!
//! 所有"档位 → 引擎单位"的换算只在本文件出现一次；DSP 与 UI 都从这里取数，
//! 不许在别处重复硬编码。

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use truce::prelude::*;

// ---------------------------------------------------------------------------
// 档位常量（与 Params.h 的 constexpr 数组逐项一致）
// ---------------------------------------------------------------------------

/// 参数总数（原版 `kNumParams`）。
pub const NUM_PARAMS: usize = 21;

/// Speed 档 × LOG 模式释放时间（秒），MIN..MAX。
pub const SPEED_RELEASE_LOG: [f64; 5] = [4.0, 2.0, 1.0, 0.5, 0.2];
/// Speed 档 × LIN 模式释放时间（秒），MIN..MAX。
pub const SPEED_RELEASE_LIN: [f64; 5] = [9.6, 4.8, 2.4, 1.2, 0.5];

/// Res 档 → FFT 尺寸（样本）。
pub const RES_OPTIONS: [usize; 3] = [2048, 4096, 8192];
/// PBT LfRes 档 → 低频带宽下限（Hz）。
pub const PBT_LF_RES_OPTIONS: [usize; 3] = [40, 20, 10];
/// RTA 档 → 每倍频程频带数（bpo）。
pub const RTA_BPO_OPTIONS: [usize; 3] = [6, 12, 24];
/// RTA 档 → 实际带数（22 Hz..20 kHz，仅作 UI/信息展示，DSP 按 bpo 生成）。
pub const RTA_BAND_COUNTS: [usize; 3] = [59, 118, 236];
/// VQT gamma 档 → 低频带宽下限 γ（Hz）。
pub const VQT_GAMMA_OPTIONS: [f64; 3] = [20.0, 10.0, 5.0];

/// 峰值保持档 → 保持时长（秒）；∞ 档为 `1e9`（原版 `kHoldTimeSecs`）。
pub const HOLD_SECS: [f64; 3] = [0.5, 2.0, 1e9];
/// 响度目标预设（LUFS）。
pub const LOUD_TARGETS_LUFS: [f64; 4] = [-9.0, -14.0, -23.0, -24.0];
/// 响度条刻度窗偏移（LU）。
pub const LOUD_SCALE_LU: [f64; 2] = [9.0, 18.0];

/// FFT 模式斜率档（dB/oct）。
pub const SLOPE_FFT_DB: [f64; 3] = [0.0, 3.0, 4.5];
/// VQT / PBT / RTA 斜率档（dB/oct）。单位保留原版有意设计。
pub const SLOPE_BAND_DB: [f64; 3] = [-3.0, 0.0, 1.5];

/// Range 档 → 频谱动态范围（dB）；画区底部为 `-range`（原版 `kRangeDb`）。
pub const RANGE_DB: [f32; 3] = [80.0, 100.0, 120.0];

/// 频谱画区顶部固定电平（dB，原版 `kTopDb = 9.f`）。
pub const TOP_DB: f32 = 9.0;

/// 频谱起振时间常数（秒），原版固定 `mAttackSec = 0.05f`。
pub const ATTACK_SEC: f64 = 0.05;

/// VQT 每倍频程频带数（原版 `kVQTBpo`）。
pub const VQT_BPO: usize = 24;

/// 状态文件魔数（原版 `kStateMagic`）。
pub const STATE_MAGIC: &str = "analyzer";
/// 设置文件名（原版 `kSettingsFileName`）。
pub const SETTINGS_FILE_NAME: &str = "ORMAnalyzer.settings";

// ---------------------------------------------------------------------------
// 枚举档位（显示文本与原版 FlatCycleButton 标签数组逐条一致）
// ---------------------------------------------------------------------------

/// kReleaseMode：释放回落模式。0 = 对数域单极点，1 = 匀速 dB/s。
#[derive(ParamEnum, Debug)]
pub enum ReleaseMode {
    #[name = "LOG"]
    Log,
    #[name = "LIN"]
    Lin,
}

/// kSpeed：响应速度档，MIN..MAX。
#[derive(ParamEnum, Debug)]
pub enum Speed {
    #[name = "MIN"]
    Min,
    #[name = "SLOW"]
    Slow,
    #[name = "MED"]
    Med,
    #[name = "FAST"]
    Fast,
    #[name = "MAX"]
    Max,
}

/// kRange：频谱显示动态范围档（按钮标签为负号 dB 值）。
#[derive(ParamEnum, Debug)]
pub enum DisplayRange {
    #[name = "-80"]
    Db80,
    #[name = "-100"]
    Db100,
    #[name = "-120"]
    Db120,
}

/// kRes：FFT 分辨率档。
#[derive(ParamEnum, Debug)]
pub enum Res {
    #[name = "L"]
    Low,
    #[name = "M"]
    Mid,
    #[name = "H"]
    High,
}

/// kLfRes：PBT 低频分辨率档。
#[derive(ParamEnum, Debug)]
pub enum PbtLfRes {
    #[name = "L"]
    Low,
    #[name = "M"]
    Mid,
    #[name = "H"]
    High,
}

/// kMode：分析引擎。
#[derive(ParamEnum, Debug)]
pub enum AnalyzerMode {
    #[name = "STFT"]
    Stft,
    #[name = "VQT"]
    Vqt,
    #[name = "PBT"]
    Pbt,
    #[name = "RTA"]
    Rta,
}

/// kChannelMode：声道显示。
#[derive(ParamEnum, Debug)]
pub enum ChannelMode {
    #[name = "PWR"]
    Pwr,
    #[name = "L/R"]
    Lr,
    #[name = "SUM"]
    Sum,
}

/// kLevelMode：电平表模式。
#[derive(ParamEnum, Debug)]
pub enum LevelMode {
    #[name = "dBTP"]
    Dbtp,
    #[name = "dBFS"]
    Dbfs,
}

/// kLevelHold：峰值保持时长档。
#[derive(ParamEnum, Debug)]
pub enum LevelHold {
    #[name = "0.5s"]
    HalfSec,
    #[name = "2s"]
    TwoSec,
    #[name = "∞"]
    Infinite,
}

/// kSlopeFFT：STFT 斜率档。
#[derive(ParamEnum, Debug)]
pub enum SlopeFft {
    #[name = "0 dB/Oct"]
    Zero,
    #[name = "3 dB/Oct"]
    Three,
    #[name = "4.5 dB/Oct"]
    FourPointFive,
}

/// kSlopeVQT / kSlopePBT / kSlopeRTA：逐 band 引擎斜率档。
#[derive(ParamEnum, Debug)]
pub enum SlopeBand {
    #[name = "-3 dB/Oct"]
    MinusThree,
    #[name = "0 dB/Oct"]
    Zero,
    #[name = "1.5 dB/Oct"]
    PlusOnePointFive,
}

/// kRtaOctave：RTA 分数倍频程档。
#[derive(ParamEnum, Debug)]
pub enum RtaOctave {
    #[name = "L"]
    Sixth,
    #[name = "M"]
    Twelfth,
    #[name = "H"]
    TwentyFourth,
}

/// kFFTWindow / kWindowVQT：窗函数。0 = Hann（锐利），1 = 5-term BH（纯净）。
#[derive(ParamEnum, Debug)]
pub enum Window {
    #[name = "SHARP"]
    Sharp,
    #[name = "CLEAN"]
    Clean,
}

/// kVQTGamma：VQT 低频带宽下限 γ 档。
#[derive(ParamEnum, Debug)]
pub enum VqtGamma {
    #[name = "L"]
    Low,
    #[name = "M"]
    Mid,
    #[name = "H"]
    High,
}

/// kLoudPreset：响度目标预设（LUFS）。
#[derive(ParamEnum, Debug)]
pub enum LoudPreset {
    #[name = "-9"]
    Nine,
    #[name = "-14"]
    Fourteen,
    #[name = "-23"]
    TwentyThree,
    #[name = "-24"]
    TwentyFour,
}

/// kLoudScale：响度条刻度窗偏移（LU）。
#[derive(ParamEnum, Debug)]
pub enum LoudScale {
    #[name = "+9"]
    Nine,
    #[name = "+18"]
    Eighteen,
}

// ---------------------------------------------------------------------------
// 档位 → 引擎单位（自由函数，索引口径与原版 `std::clamp` 一致）
// ---------------------------------------------------------------------------

fn clamp_idx(idx: usize, len: usize) -> usize {
    idx.min(len.saturating_sub(1))
}

/// Speed 档 × 释放模式 → 释放时间常数（秒）。
/// 等价原版 `SpeedReleaseSec(speedIdx, releaseMode)`。
pub fn release_sec(speed_idx: usize, release_mode: ReleaseMode) -> f64 {
    let s = clamp_idx(speed_idx, SPEED_RELEASE_LOG.len());
    match release_mode {
        ReleaseMode::Log => SPEED_RELEASE_LOG[s],
        ReleaseMode::Lin => SPEED_RELEASE_LIN[s],
    }
}

/// Res 档 → FFT 尺寸（样本）。
pub fn fft_size(res_idx: usize) -> usize {
    RES_OPTIONS[clamp_idx(res_idx, RES_OPTIONS.len())]
}

/// PBT LfRes 档 → 低频带宽下限（Hz）。
pub fn pbt_lf_width_hz(lf_res_idx: usize) -> f64 {
    PBT_LF_RES_OPTIONS[clamp_idx(lf_res_idx, PBT_LF_RES_OPTIONS.len())] as f64
}

/// VQT gamma 档 → 低频带宽下限 γ（Hz）。
pub fn vqt_gamma_hz(gamma_idx: usize) -> f64 {
    VQT_GAMMA_OPTIONS[clamp_idx(gamma_idx, VQT_GAMMA_OPTIONS.len())]
}

/// RTA 档 → 每倍频程频带数（bpo）。
pub fn rta_bpo(octave_idx: usize) -> usize {
    RTA_BPO_OPTIONS[clamp_idx(octave_idx, RTA_BPO_OPTIONS.len())]
}

/// RTA 档 → 实际带数（信息展示用）。
pub fn rta_band_count(octave_idx: usize) -> usize {
    RTA_BAND_COUNTS[clamp_idx(octave_idx, RTA_BAND_COUNTS.len())]
}

/// Range 档 → 动态范围（dB，正数）；画区底部为 `-range_db`。
pub fn range_db(range_idx: usize) -> f32 {
    RANGE_DB[clamp_idx(range_idx, RANGE_DB.len())]
}

/// 保持时长档 → 保持秒数；开关关闭时返回 0（等价原版 `CurrentHoldSec`）。
pub fn hold_sec(hold_on: bool, hold_idx: usize) -> f64 {
    if !hold_on {
        return 0.0;
    }
    HOLD_SECS[clamp_idx(hold_idx, HOLD_SECS.len())]
}

/// 响度预设档 → 目标响度（LUFS）。
pub fn loud_target_lufs(preset_idx: usize) -> f64 {
    LOUD_TARGETS_LUFS[clamp_idx(preset_idx, LOUD_TARGETS_LUFS.len())]
}

/// 响度刻度档 → 窗口偏移（LU）。
pub fn loud_scale_lu(scale_idx: usize) -> f64 {
    LOUD_SCALE_LU[clamp_idx(scale_idx, LOUD_SCALE_LU.len())]
}

/// FFT 斜率档 → dB/oct。
pub fn slope_fft_db(slope_idx: usize) -> f64 {
    SLOPE_FFT_DB[clamp_idx(slope_idx, SLOPE_FFT_DB.len())]
}

/// 逐 band 引擎斜率档 → dB/oct。
pub fn slope_band_db(slope_idx: usize) -> f64 {
    SLOPE_BAND_DB[clamp_idx(slope_idx, SLOPE_BAND_DB.len())]
}

/// 按分析引擎选斜率表：FFT 用 `kSlopeDbFFT`，其余用 `kSlopeDbLog`。
/// 等价原版 `EffectiveSlopeDb()` 的选型逻辑。
#[allow(clippy::too_many_arguments)]
pub fn slope_db_per_oct(
    mode: AnalyzerMode,
    slope_fft: usize,
    slope_vqt: usize,
    slope_pbt: usize,
    slope_rta: usize,
) -> f64 {
    match mode {
        AnalyzerMode::Stft => slope_fft_db(slope_fft),
        AnalyzerMode::Vqt => slope_band_db(slope_vqt),
        AnalyzerMode::Pbt => slope_band_db(slope_pbt),
        AnalyzerMode::Rta => slope_band_db(slope_rta),
    }
}

/// 画区底部电平（dB，负数）：`-range_db`。
pub fn range_bottom_db(range_idx: usize) -> f32 {
    -range_db(range_idx)
}

/// 一阶平滑系数：`exp(-update_period / tau_sec)`。
/// 等价原版 `mAttackCoeff` / `mReleaseCoeff` 的算式（SpectrumPad.h:100-101）。
pub fn smoothing_coef(tau_sec: f64, update_period: f64) -> f64 {
    let tau = tau_sec.max(1e-3);
    (-update_period / tau).exp()
}

/// LIN 模式匀速回落的每帧 dB 步进（原版 `unifStepDb`，SpectrumPad.h:102）。
pub fn unif_step_db(release_sec: f64, range_db: f32, update_period: f64) -> f64 {
    let tau = release_sec.max(1e-3);
    update_period / (2.0 * tau) * (range_db as f64)
}

// ---------------------------------------------------------------------------
// 音频线程 → UI 线程的共享分析通道（照 bandpass `Analysis` 范式，Arc 跨线程）
// ---------------------------------------------------------------------------

/// 跨线程只读分析状态。参数之外的运行态（采样率、音频 tap）放在这里，
/// 不参与 `.orm` 状态文件序列化。
pub struct AnalysisState {
    pub tap: Arc<AudioTap<f32>>,
    sample_rate: AtomicU32,
    /// 音频→UI 分析帧 / 测量快照通道（本插件外壳写入，界面只读）。
    /// 随外层 `analysis` 字段整体 `#[skip]`，不参与状态文件序列化。
    pub shared: Arc<crate::analysis::AnalyzerShared>,
}

impl Default for AnalysisState {
    fn default() -> Self {
        Self {
            tap: Arc::new(AudioTap::new(32_768, 2)),
            sample_rate: AtomicU32::new(48_000),
            shared: crate::analysis::AnalyzerShared::new(),
        }
    }
}

impl AnalysisState {
    pub fn sample_rate(&self) -> f64 {
        self.sample_rate.load(Ordering::Relaxed) as f64
    }

    pub fn store_sample_rate(&self, sr: f64) {
        self.sample_rate.store(sr.round() as u32, Ordering::Relaxed);
    }
}

// ---------------------------------------------------------------------------
// truce 参数声明
// ---------------------------------------------------------------------------

#[derive(Params)]
pub struct AnalyzerParams {
    #[param(id = 0, name = "Speed", default = 4)]
    pub speed: EnumParam<Speed>,
    #[param(id = 1, name = "ReleaseMode", default = 0)]
    pub release_mode: EnumParam<ReleaseMode>,
    #[param(id = 2, name = "Range", default = 0)]
    pub range: EnumParam<DisplayRange>,
    #[param(id = 3, name = "Res", default = 1)]
    pub res: EnumParam<Res>,
    #[param(id = 4, name = "LfRes", default = 2)]
    pub lf_res: EnumParam<PbtLfRes>,
    #[param(id = 5, name = "Mode", default = 0)]
    pub mode: EnumParam<AnalyzerMode>,
    #[param(id = 6, name = "ChanMode", default = 0)]
    pub channel_mode: EnumParam<ChannelMode>,
    #[param(id = 7, name = "LevelMode", default = 0)]
    pub level_mode: EnumParam<LevelMode>,
    #[param(id = 8, name = "LevelHold", default = 1)]
    pub level_hold: EnumParam<LevelHold>,
    #[param(id = 9, name = "LevelHoldOn", default = false)]
    pub level_hold_on: BoolParam,
    #[param(id = 10, name = "Freeze", default = false)]
    pub freeze: BoolParam,
    #[param(id = 11, name = "SlopeFFT", default = 1)]
    pub slope_fft: EnumParam<SlopeFft>,
    #[param(id = 12, name = "SlopeVQT", default = 1)]
    pub slope_vqt: EnumParam<SlopeBand>,
    #[param(id = 13, name = "SlopePBT", default = 1)]
    pub slope_pbt: EnumParam<SlopeBand>,
    #[param(id = 14, name = "SlopeRTA", default = 1)]
    pub slope_rta: EnumParam<SlopeBand>,
    #[param(id = 15, name = "RtaOctave", default = 0)]
    pub rta_octave: EnumParam<RtaOctave>,
    #[param(id = 16, name = "WindowFFT", default = 0)]
    pub fft_window: EnumParam<Window>,
    #[param(id = 17, name = "WindowVQT", default = 1)]
    pub window_vqt: EnumParam<Window>,
    #[param(id = 18, name = "VQTGamma", default = 2)]
    pub vqt_gamma: EnumParam<VqtGamma>,
    #[param(id = 19, name = "LoudPreset", default = 1)]
    pub loud_preset: EnumParam<LoudPreset>,
    #[param(id = 20, name = "LoudScale", default = 0)]
    pub loud_scale: EnumParam<LoudScale>,

    /// 仅供界面 / DSP 读取的运行态，不参与状态序列化。
    #[skip]
    pub analysis: Arc<AnalysisState>,
}

impl AnalyzerParams {
    // --- 引擎单位换算（方法口径与上面的自由函数一致，读的是当前参数值） ---

    /// 当前 Speed × ReleaseMode 的释放时间常数（秒）。
    pub fn release_sec(&self) -> f64 {
        release_sec(self.speed.index() as usize, self.release_mode.value())
    }

    /// 频谱起振时间常数（秒），固定 0.05 s。
    pub fn attack_sec(&self) -> f64 {
        ATTACK_SEC
    }

    /// 当前 FFT 尺寸（样本）。
    pub fn fft_size(&self) -> usize {
        fft_size(self.res.index() as usize)
    }

    /// 当前 PBT 低频带宽下限（Hz）。
    pub fn pbt_lf_width_hz(&self) -> f64 {
        pbt_lf_width_hz(self.lf_res.index() as usize)
    }

    /// 当前 VQT γ（Hz）。
    pub fn vqt_gamma_hz(&self) -> f64 {
        vqt_gamma_hz(self.vqt_gamma.index() as usize)
    }

    /// 当前 RTA 档位原始索引（0=1/6, 1=1/12, 2=1/24）。
    pub fn rta_octave_index(&self) -> usize {
        self.rta_octave.index() as usize
    }

    /// 当前 RTA bpo。
    pub fn rta_bpo(&self) -> usize {
        rta_bpo(self.rta_octave.index() as usize)
    }

    /// 当前 RTA 带数（信息展示）。
    pub fn rta_band_count(&self) -> usize {
        rta_band_count(self.rta_octave.index() as usize)
    }

    /// 当前动态范围（dB，正数）。
    pub fn range_db(&self) -> f32 {
        range_db(self.range.index() as usize)
    }

    /// 画区电平上下界 `(top_db, bottom_db)`，单位 dB（顶部固定 9 dB）。
    pub fn range_bounds(&self) -> (f32, f32) {
        (TOP_DB, range_bottom_db(self.range.index() as usize))
    }

    /// 当前峰值保持秒数；开关关闭为 0。
    pub fn hold_sec(&self) -> f64 {
        hold_sec(self.level_hold_on.value(), self.level_hold.index() as usize)
    }

    /// 当前响度目标（LUFS）。
    pub fn loud_target_lufs(&self) -> f64 {
        loud_target_lufs(self.loud_preset.index() as usize)
    }

    /// 当前响度刻度偏移（LU）。
    pub fn loud_scale_lu(&self) -> f64 {
        loud_scale_lu(self.loud_scale.index() as usize)
    }

    /// 当前引擎实际生效的频谱斜率（dB/oct）。
    /// 等价原版 `EffectiveSlopeDb()`：FFT 用 `kSlopeDbFFT`，其余用 `kSlopeDbLog`。
    pub fn effective_slope_db(&self) -> f64 {
        slope_db_per_oct(
            self.mode.value(),
            self.slope_fft.index() as usize,
            self.slope_vqt.index() as usize,
            self.slope_pbt.index() as usize,
            self.slope_rta.index() as usize,
        )
    }

    /// 声道拆分标志：L/R 立体声 → true；其余 → false。
    pub fn channel_split(&self) -> bool {
        self.channel_mode.value() == ChannelMode::Lr
    }

    /// 合并算法标志：SUM（信号和）→ true；PWR（能量和）→ false。
    pub fn channel_merge_algo(&self) -> bool {
        self.channel_mode.value() == ChannelMode::Sum
    }

    // --- 快照（.orm 状态文件的参数数组，id 顺序存储） ---

    /// 导出当前全部参数的普通值（索引 / 0|1），顺序与 id 0..=20 一致。
    /// 等价原版 `Snapshot()`。
    pub fn snapshot(&self) -> Vec<f64> {
        (0..NUM_PARAMS)
            .map(|i| self.get_plain(i as u32).unwrap_or(0.0))
            .collect()
    }

    /// 从快照恢复参数。档位数不匹配时报错；等价原版 `ApplySnapshot()` 的
    /// 参数装载部分（撤销入栈 / 界面刷新由调用方处理）。
    pub fn apply_snapshot(&self, values: &[f64]) -> Result<(), String> {
        if values.len() != NUM_PARAMS {
            return Err(format!(
                "Parameter count mismatch (expected {NUM_PARAMS}, got {})",
                values.len()
            ));
        }
        for (i, v) in values.iter().enumerate() {
            self.set_plain(i as u32, *v);
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// 状态文件（.orm，JSON，魔数 "analyzer"）—— 等价 `StateFileIO.h`
// ---------------------------------------------------------------------------

/// 写状态文件：`<magic>\n` + 2 空格缩进的 JSON `{"version":1,"values":[...]}`。
pub fn write_state_file(path: &Path, values: &[f64]) -> Result<(), String> {
    let j = serde_json::json!({
        "version": 1,
        "values": values,
    });
    let body = match serde_json::to_string_pretty(&j) {
        Ok(b) => b,
        Err(e) => return Err(format!("JSON write error: {e}")),
    };
    let mut out = String::from(STATE_MAGIC);
    out.push('\n');
    out.push_str(&body);
    std::fs::write(path, out).map_err(|e| format!("Cannot open file for writing: {path:?}: {e}"))
}

/// 读状态文件：校验魔数与 `version == 1`，返回 `values` 数组。
pub fn read_state_file(path: &Path) -> Result<Vec<f64>, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Cannot open file: {path:?}: {e}"))?;

    if !content.starts_with(STATE_MAGIC) {
        return Err(format!(
            "Not an ORM Analyzer file (missing '{STATE_MAGIC}' header)"
        ));
    }
    let mut pos = STATE_MAGIC.len();
    while pos < content.len() && (content.as_bytes()[pos] == b'\n' || content.as_bytes()[pos] == b'\r') {
        pos += 1;
    }

    let j: serde_json::Value = serde_json::from_str(&content[pos..])
        .map_err(|e| format!("JSON parse error: {e}"))?;

    let version = j
        .get("version")
        .and_then(|v| v.as_i64())
        .ok_or_else(|| "Unsupported state file (missing version)".to_string())?;
    if version != 1 {
        return Err(format!("Unsupported state file (expected version 1, got {version})"));
    }

    let values = j
        .get("values")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "Missing 'values' array".to_string())?;
    values
        .iter()
        .map(|v| v.as_f64().ok_or_else(|| "JSON field error: non-numeric value".to_string()))
        .collect()
}

// ---------------------------------------------------------------------------
// 设置文件（ORMAnalyzer.settings）—— 等价 `SettingsFileIO.h` + common 目录约定
// ---------------------------------------------------------------------------

/// 全局 UI 偏好 / 测试信号发生器字段。缺失键保留默认值；`-1` 表示未保存。
#[derive(Clone, Debug)]
pub struct SettingsData {
    pub lang: i32,
    pub hue: i32,
    pub sat_max: i32,
    pub theme_mode: i32,
    pub hold_on: i32,
    pub gen_type: i32,
    pub gen_freq: f64,
    pub gen_level: f64,
    pub gen_hold: i32,
    pub gen_to_output: i32,
}

impl Default for SettingsData {
    fn default() -> Self {
        Self {
            lang: -1,
            hue: 45,
            sat_max: 15,
            theme_mode: 0,
            hold_on: -1,
            gen_type: 0,
            gen_freq: 1000.0,
            gen_level: -12.0,
            gen_hold: 0,
            gen_to_output: 0,
        }
    }
}

/// 设置目录（原版 `SettingsDir()`）：
/// macOS `~/Library/Application Support/OpenRM`，Windows `%APPDATA%/OpenRM`。
pub fn settings_dir() -> Option<PathBuf> {
    let base = if cfg!(windows) {
        std::env::var("APPDATA").ok()
    } else {
        std::env::var("HOME").ok()
    }?;
    if base.is_empty() {
        return None;
    }
    let dir = if cfg!(windows) {
        PathBuf::from(base).join("OpenRM")
    } else {
        PathBuf::from(base).join("Library/Application Support/OpenRM")
    };
    Some(dir)
}

/// 设置文件完整路径（原版 `SettingsPath(kSettingsFileName)`）。
pub fn settings_path() -> Option<PathBuf> {
    Some(settings_dir()?.join(SETTINGS_FILE_NAME))
}

/// 读取设置；文件不存在 / 解析失败 / 顶层非对象一律返回默认值，不抛异常。
pub fn load_settings() -> SettingsData {
    let mut out = SettingsData::default();
    let Some(path) = settings_path() else {
        return out;
    };
    let Ok(txt) = std::fs::read_to_string(path) else {
        return out;
    };
    let Ok(j) = serde_json::from_str::<serde_json::Value>(&txt) else {
        return out;
    };
    if !j.is_object() {
        return out;
    }
    if let Some(v) = j.get("lang").and_then(|v| v.as_i64()) {
        out.lang = v as i32;
    }
    if let Some(v) = j.get("hue").and_then(|v| v.as_i64()) {
        out.hue = v as i32;
    }
    if let Some(v) = j.get("satMax").and_then(|v| v.as_i64()) {
        out.sat_max = v as i32;
    }
    if let Some(v) = j.get("themeMode").and_then(|v| v.as_i64()) {
        out.theme_mode = v as i32;
    }
    if let Some(v) = j.get("holdOn").and_then(|v| v.as_i64()) {
        out.hold_on = v as i32;
    }
    if let Some(v) = j.get("genType").and_then(|v| v.as_i64()) {
        out.gen_type = v as i32;
    }
    if let Some(v) = j.get("genFreq").and_then(|v| v.as_f64()) {
        out.gen_freq = v;
    }
    if let Some(v) = j.get("genLevel").and_then(|v| v.as_f64()) {
        out.gen_level = v;
    }
    if let Some(v) = j.get("genHold").and_then(|v| v.as_i64()) {
        out.gen_hold = v as i32;
    }
    if let Some(v) = j.get("genToOutput").and_then(|v| v.as_i64()) {
        out.gen_to_output = v as i32;
    }
    out
}

/// 写设置文件；目录不存在则递归创建。任何失败返回 `false`，不抛异常。
pub fn save_settings(d: &SettingsData) -> bool {
    let Some(path) = settings_path() else {
        return false;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let j = serde_json::json!({
        "lang": d.lang,
        "hue": d.hue,
        "satMax": d.sat_max,
        "themeMode": d.theme_mode,
        "holdOn": d.hold_on,
        "genType": d.gen_type,
        "genFreq": d.gen_freq,
        "genLevel": d.gen_level,
        "genHold": d.gen_hold,
        "genToOutput": d.gen_to_output,
    });
    let Ok(txt) = serde_json::to_string_pretty(&j) else {
        return false;
    };
    std::fs::write(path, txt).is_ok()
}
