//! 35 个宿主参数，id 与原 iPlug2 版 `EParams` 一一对应（0..=34）。
//!
//! 固定 id 的理由：老版本的 VST3/AU 会话、以及 `.orm` 预设文件都按"参数序号"
//! 存值。新版本沿用同一序号，迁移脚本只需处理单位换算，不需要重排映射表。

use truce::prelude::*;

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

/// 音频线程 → UI 线程的共享分析通道。
/// 4 通道：干 L、干 R、湿 L、湿 R（与原版两路 STFT 的输入一致）。
pub struct Analysis {
    pub tap: Arc<AudioTap<f32>>,
    pub sample_rate: AtomicU32,
}

impl Default for Analysis {
    fn default() -> Self {
        Self {
            tap: Arc::new(AudioTap::new(32_768, 4)),
            sample_rate: AtomicU32::new(48_000),
        }
    }
}

impl Analysis {
    pub fn sample_rate(&self) -> f64 {
        self.sample_rate.load(Ordering::Relaxed) as f64
    }
}

/// 斜率档：与原版 `{"12 dB/oct", "24 dB/oct", "48 dB/oct", "96 dB/oct"}` 一致。
#[derive(ParamEnum, Debug)]
pub enum Slope {
    #[name = "12 dB/oct"]
    Db12,
    #[name = "24 dB/oct"]
    Db24,
    #[name = "48 dB/oct"]
    Db48,
    #[name = "96 dB/oct"]
    Db96,
}

impl Slope {
    /// 落到 DSP 核心的 dB/oct 数值。
    pub fn db_per_oct(self) -> f64 {
        match self {
            Slope::Db12 => 12.0,
            Slope::Db24 => 24.0,
            Slope::Db48 => 48.0,
            Slope::Db96 => 96.0,
        }
    }
}

/// 随机映射的颜色槽：红/黄/蓝/绿，每槽一组独立的 amount / speed。
#[derive(ParamEnum, Debug)]
pub enum RandomColor {
    Red,
    Yellow,
    Blue,
    Green,
}

#[derive(Params)]
pub struct BandPassParams {
    #[param(
        id = 0,
        name = "Freq L",
        range = "log(20, 20000)",
        unit = "Hz",
        default = 1095.445,
        smooth = "none"
    )]
    pub freq_l: FloatParam,
    #[param(
        id = 1,
        name = "BW L",
        range = "log(1, 31)",
        default = 3.65,
        smooth = "none"
    )]
    pub bw_l: FloatParam,
    #[param(
        id = 2,
        name = "Gain L",
        range = "linear(-96, 12)",
        unit = "dB",
        default = 0.0,
        smooth = "none"
    )]
    pub gain_l: FloatParam,

    #[param(
        id = 3,
        name = "Freq R",
        range = "log(20, 20000)",
        unit = "Hz",
        default = 1095.445,
        smooth = "none"
    )]
    pub freq_r: FloatParam,
    #[param(
        id = 4,
        name = "BW R",
        range = "log(1, 31)",
        default = 3.65,
        smooth = "none"
    )]
    pub bw_r: FloatParam,
    #[param(
        id = 5,
        name = "Gain R",
        range = "linear(-96, 12)",
        unit = "dB",
        default = 0.0,
        smooth = "none"
    )]
    pub gain_r: FloatParam,

    #[param(id = 6, name = "Link")]
    pub link: BoolParam,
    #[param(
        id = 7,
        name = "Mix",
        range = "linear(0, 1)",
        default = 1.0,
        smooth = "none"
    )]
    pub mix: FloatParam,

    #[param(id = 8, name = "Random Amount", range = "linear(0, 1)", default = 0.5)]
    pub random_amount_r: FloatParam,
    #[param(id = 9, name = "Random Speed", range = "log(0.01, 60)", default = 0.5)]
    pub random_rate_r: FloatParam,

    #[param(id = 10, name = "Slope L", default = 3)]
    pub slope_l: EnumParam<Slope>,
    #[param(id = 11, name = "Slope R", default = 3)]
    pub slope_r: EnumParam<Slope>,

    #[param(id = 12, name = "Random Color Freq L")]
    pub random_color_freq_l: EnumParam<RandomColor>,
    #[param(id = 13, name = "Random Color BW L")]
    pub random_color_bw_l: EnumParam<RandomColor>,
    #[param(id = 14, name = "Random Color Gain L")]
    pub random_color_gain_l: EnumParam<RandomColor>,
    #[param(id = 15, name = "Random Color Freq R")]
    pub random_color_freq_r: EnumParam<RandomColor>,
    #[param(id = 16, name = "Random Color BW R")]
    pub random_color_bw_r: EnumParam<RandomColor>,
    #[param(id = 17, name = "Random Color Gain R")]
    pub random_color_gain_r: EnumParam<RandomColor>,
    #[param(id = 18, name = "Random Color Mix")]
    pub random_color_mix: EnumParam<RandomColor>,

    #[param(id = 19, name = "Random Freq L")]
    pub random_enable_freq_l: BoolParam,
    #[param(id = 20, name = "Random BW L")]
    pub random_enable_bw_l: BoolParam,
    #[param(id = 21, name = "Random Gain L")]
    pub random_enable_gain_l: BoolParam,
    #[param(id = 22, name = "Random Freq R")]
    pub random_enable_freq_r: BoolParam,
    #[param(id = 23, name = "Random BW R")]
    pub random_enable_bw_r: BoolParam,
    #[param(id = 24, name = "Random Gain R")]
    pub random_enable_gain_r: BoolParam,
    #[param(id = 25, name = "Random Mix")]
    pub random_enable_mix: BoolParam,

    #[param(
        id = 26,
        name = "Random Amount Yellow",
        range = "linear(0, 1)",
        default = 0.3
    )]
    pub random_amount_y: FloatParam,
    #[param(
        id = 27,
        name = "Random Speed Yellow",
        range = "log(0.01, 60)",
        default = 1.0
    )]
    pub random_rate_y: FloatParam,
    #[param(
        id = 28,
        name = "Random Amount Blue",
        range = "linear(0, 1)",
        default = 0.6
    )]
    pub random_amount_b: FloatParam,
    #[param(
        id = 29,
        name = "Random Speed Blue",
        range = "log(0.01, 60)",
        default = 0.75
    )]
    pub random_rate_b: FloatParam,
    #[param(
        id = 30,
        name = "Random Amount Green",
        range = "linear(0, 1)",
        default = 0.4
    )]
    pub random_amount_g: FloatParam,
    #[param(
        id = 31,
        name = "Random Speed Green",
        range = "log(0.01, 60)",
        default = 2.0
    )]
    pub random_rate_g: FloatParam,

    #[param(id = 32, name = "Pass L", default = 1)]
    pub pass_l: BoolParam,
    #[param(id = 33, name = "Pass R", default = 1)]
    pub pass_r: BoolParam,
    /// 仅供界面读取的频谱源与采样率，不参与状态序列化。
    #[skip]
    pub analysis: Arc<Analysis>,
}

impl BandPassParams {
    /// 宿主参数 → DSP 核心参数。
    ///
    /// 单位换算与原版 `CollectParams()` 完全一致：BW 倍率取 `2*log2(m)`，
    /// 增益用 `10^(dB/20)`；颜色槽把 Red/Yellow/Blue/Green 映射到
    /// amount/period 数组的 0..3。
    pub fn to_core(&self) -> openrm_dsp::bandpass::Params {
        let color_idx = |c: &EnumParam<RandomColor>| c.index() as u8;
        openrm_dsp::bandpass::Params {
            freq_l: self.freq_l.raw_target(),
            bw_l: 2.0 * self.bw_l.raw_target().log2(),
            gain_l: 10f64.powf(self.gain_l.raw_target() / 20.0) as f32,

            freq_r: self.freq_r.raw_target(),
            bw_r: 2.0 * self.bw_r.raw_target().log2(),
            gain_r: 10f64.powf(self.gain_r.raw_target() / 20.0) as f32,

            linked: self.link.value(),
            reject_l: !self.pass_l.value(),
            reject_r: !self.pass_r.value(),
            mix: self.mix.raw_target() as f32,

            random_amount: [
                self.random_amount_r.raw_target() as f32,
                self.random_amount_y.raw_target() as f32,
                self.random_amount_b.raw_target() as f32,
                self.random_amount_g.raw_target() as f32,
            ],
            random_period_sec: [
                self.random_rate_r.raw_target(),
                self.random_rate_y.raw_target(),
                self.random_rate_b.raw_target(),
                self.random_rate_g.raw_target(),
            ],

            random_enable_freq_l: self.random_enable_freq_l.value(),
            random_enable_bw_l: self.random_enable_bw_l.value(),
            random_enable_gain_l: self.random_enable_gain_l.value(),
            random_enable_freq_r: self.random_enable_freq_r.value(),
            random_enable_bw_r: self.random_enable_bw_r.value(),
            random_enable_gain_r: self.random_enable_gain_r.value(),
            random_enable_mix: self.random_enable_mix.value(),

            random_color_freq_l: color_idx(&self.random_color_freq_l),
            random_color_bw_l: color_idx(&self.random_color_bw_l),
            random_color_gain_l: color_idx(&self.random_color_gain_l),
            random_color_freq_r: color_idx(&self.random_color_freq_r),
            random_color_bw_r: color_idx(&self.random_color_bw_r),
            random_color_gain_r: color_idx(&self.random_color_gain_r),
            random_color_mix: color_idx(&self.random_color_mix),

            slope_db_l: self.slope_l.value().db_per_oct(),
            slope_db_r: self.slope_r.value().db_per_oct(),
        }
    }
}
