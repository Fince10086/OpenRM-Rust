//! ORM Fusion 的宿主参数。id 0..89 与 DSP 文档 §3 一一对应；
//! 90..93 是界面侧的 RANDOM 开关/范围/速度/颜色，DSP 不消费。
//!
//! 所有参数都在 truce 侧声明 `smooth = "none"`：原版参数是下一帧立即生效，
//! 平滑由 UI 的 Super Slider / RANDOM 自己完成。

use truce::prelude::*;

#[derive(ParamEnum, Debug)]
pub enum Bands {
    #[name = "256"]
    N256,
    #[name = "512"]
    N512,
    #[name = "1024"]
    N1024,
    #[name = "2048"]
    N2048,
    #[name = "4096"]
    N4096,
    #[name = "8192"]
    N8192,
    #[name = "16384"]
    N16384,
    #[name = "32768"]
    N32768,
    #[name = "65536"]
    N65536,
    #[name = "131072"]
    N131072,
}

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

#[derive(Default)]
pub struct HostInfo {
    sample_rate: AtomicU32,
    reset_generation: AtomicU32,
    /// 音频线程写入、UI 线程读取的干信号环形队列（显示输入频谱）。
    pub tap: Arc<AudioTap<f32>>,
}

impl HostInfo {
    pub fn set_sample_rate(&self, sr: f64) {
        self.sample_rate.store(sr.round() as u32, Ordering::Relaxed);
    }
    pub fn sample_rate(&self) -> f32 {
        match self.sample_rate.load(Ordering::Relaxed) {
            0 => 48_000.0,
            v => v as f32,
        }
    }
    pub fn request_reset(&self) {
        self.reset_generation.fetch_add(1, Ordering::Relaxed);
    }
    pub fn reset_generation(&self) -> u32 {
        self.reset_generation.load(Ordering::Relaxed)
    }
}

#[derive(Params)]
pub struct FusionParams {
    #[param(
        id = 0,
        name = "Preset",
        range = "linear(1, 16)",
        default = 1.0,
        smooth = "none"
    )]
    pub preset: FloatParam,
    #[param(
        id = 1,
        name = "Super Slider",
        range = "linear(0, 1)",
        default = 0.0,
        smooth = "none"
    )]
    pub super_slider: FloatParam,
    #[param(
        id = 2,
        name = "Inter Speed",
        range = "linear(0, 1)",
        default = 0.0666667,
        smooth = "none"
    )]
    pub inter_speed: FloatParam,
    #[param(id = 3, name = "Load Speed")]
    pub load_speed: BoolParam,
    #[param(id = 4, name = "Bands", default = 0)]
    pub bands: EnumParam<Bands>,
    #[param(
        id = 5,
        name = "Gain",
        range = "linear(0, 1)",
        default = 0.8,
        smooth = "none"
    )]
    pub gain: FloatParam,
    #[param(
        id = 6,
        name = "Mix",
        range = "linear(0, 1)",
        default = 1.0,
        smooth = "none"
    )]
    pub mix: FloatParam,
    #[param(
        id = 23,
        name = "Smoothness",
        range = "linear(0, 1)",
        default = 0.5,
        smooth = "none"
    )]
    pub smoothness: FloatParam,
    #[param(id = 88, name = "Global Reset", default = false)]
    pub global_reset: BoolParam,
    #[param(id = 89, name = "Global Mute", default = false)]
    pub global_mute: BoolParam,
    #[param(
        id = 7,
        name = "Amplitude 1",
        range = "linear(0, 1)",
        default = 0.9,
        smooth = "none"
    )]
    pub amp1: FloatParam,
    #[param(
        id = 8,
        name = "Amplitude 2",
        range = "linear(0, 1)",
        default = 0.9,
        smooth = "none"
    )]
    pub amp2: FloatParam,
    #[param(
        id = 9,
        name = "Amplitude 3",
        range = "linear(0, 1)",
        default = 0.9,
        smooth = "none"
    )]
    pub amp3: FloatParam,
    #[param(
        id = 10,
        name = "Amplitude 4",
        range = "linear(0, 1)",
        default = 0.9,
        smooth = "none"
    )]
    pub amp4: FloatParam,
    #[param(
        id = 11,
        name = "Amplitude 5",
        range = "linear(0, 1)",
        default = 0.9,
        smooth = "none"
    )]
    pub amp5: FloatParam,
    #[param(
        id = 12,
        name = "Amplitude 6",
        range = "linear(0, 1)",
        default = 0.9,
        smooth = "none"
    )]
    pub amp6: FloatParam,
    #[param(
        id = 13,
        name = "Amplitude 7",
        range = "linear(0, 1)",
        default = 0.9,
        smooth = "none"
    )]
    pub amp7: FloatParam,
    #[param(
        id = 14,
        name = "Amplitude 8",
        range = "linear(0, 1)",
        default = 0.9,
        smooth = "none"
    )]
    pub amp8: FloatParam,
    #[param(
        id = 15,
        name = "Feedback 1",
        range = "linear(0, 1)",
        default = 0.0,
        smooth = "none"
    )]
    pub fb1: FloatParam,
    #[param(
        id = 16,
        name = "Feedback 2",
        range = "linear(0, 1)",
        default = 0.0,
        smooth = "none"
    )]
    pub fb2: FloatParam,
    #[param(
        id = 17,
        name = "Feedback 3",
        range = "linear(0, 1)",
        default = 0.0,
        smooth = "none"
    )]
    pub fb3: FloatParam,
    #[param(
        id = 18,
        name = "Feedback 4",
        range = "linear(0, 1)",
        default = 0.0,
        smooth = "none"
    )]
    pub fb4: FloatParam,
    #[param(
        id = 19,
        name = "Feedback 5",
        range = "linear(0, 1)",
        default = 0.0,
        smooth = "none"
    )]
    pub fb5: FloatParam,
    #[param(
        id = 20,
        name = "Feedback 6",
        range = "linear(0, 1)",
        default = 0.0,
        smooth = "none"
    )]
    pub fb6: FloatParam,
    #[param(
        id = 21,
        name = "Feedback 7",
        range = "linear(0, 1)",
        default = 0.0,
        smooth = "none"
    )]
    pub fb7: FloatParam,
    #[param(
        id = 22,
        name = "Feedback 8",
        range = "linear(0, 1)",
        default = 0.0,
        smooth = "none"
    )]
    pub fb8: FloatParam,
    #[param(
        id = 24,
        name = "Delay 1",
        range = "linear(0, 1)",
        default = 0.1,
        smooth = "none"
    )]
    pub delay1: FloatParam,
    #[param(
        id = 25,
        name = "Delay 2",
        range = "linear(0, 1)",
        default = 0.1,
        smooth = "none"
    )]
    pub delay2: FloatParam,
    #[param(
        id = 26,
        name = "Delay 3",
        range = "linear(0, 1)",
        default = 0.1,
        smooth = "none"
    )]
    pub delay3: FloatParam,
    #[param(
        id = 27,
        name = "Delay 4",
        range = "linear(0, 1)",
        default = 0.1,
        smooth = "none"
    )]
    pub delay4: FloatParam,
    #[param(
        id = 28,
        name = "Delay 5",
        range = "linear(0, 1)",
        default = 0.1,
        smooth = "none"
    )]
    pub delay5: FloatParam,
    #[param(
        id = 29,
        name = "Delay 6",
        range = "linear(0, 1)",
        default = 0.1,
        smooth = "none"
    )]
    pub delay6: FloatParam,
    #[param(
        id = 30,
        name = "Delay 7",
        range = "linear(0, 1)",
        default = 0.1,
        smooth = "none"
    )]
    pub delay7: FloatParam,
    #[param(
        id = 31,
        name = "Delay 8",
        range = "linear(0, 1)",
        default = 0.1,
        smooth = "none"
    )]
    pub delay8: FloatParam,
    #[param(
        id = 32,
        name = "Frequency 1",
        range = "linear(0, 1)",
        default = 0.125,
        smooth = "none"
    )]
    pub freq1: FloatParam,
    #[param(
        id = 33,
        name = "Frequency 2",
        range = "linear(0, 1)",
        default = 0.175,
        smooth = "none"
    )]
    pub freq2: FloatParam,
    #[param(
        id = 34,
        name = "Frequency 3",
        range = "linear(0, 1)",
        default = 0.225,
        smooth = "none"
    )]
    pub freq3: FloatParam,
    #[param(
        id = 35,
        name = "Frequency 4",
        range = "linear(0, 1)",
        default = 0.275,
        smooth = "none"
    )]
    pub freq4: FloatParam,
    #[param(
        id = 36,
        name = "Frequency 5",
        range = "linear(0, 1)",
        default = 0.325,
        smooth = "none"
    )]
    pub freq5: FloatParam,
    #[param(
        id = 37,
        name = "Frequency 6",
        range = "linear(0, 1)",
        default = 0.375,
        smooth = "none"
    )]
    pub freq6: FloatParam,
    #[param(
        id = 38,
        name = "Frequency 7",
        range = "linear(0, 1)",
        default = 0.425,
        smooth = "none"
    )]
    pub freq7: FloatParam,
    #[param(
        id = 39,
        name = "Frequency 8",
        range = "linear(0, 1)",
        default = 0.475,
        smooth = "none"
    )]
    pub freq8: FloatParam,
    #[param(
        id = 40,
        name = "Length 1",
        range = "linear(0, 0.5)",
        default = 0.05,
        smooth = "none"
    )]
    pub length1: FloatParam,
    #[param(
        id = 41,
        name = "Length 2",
        range = "linear(0, 0.5)",
        default = 0.05,
        smooth = "none"
    )]
    pub length2: FloatParam,
    #[param(
        id = 42,
        name = "Length 3",
        range = "linear(0, 0.5)",
        default = 0.05,
        smooth = "none"
    )]
    pub length3: FloatParam,
    #[param(
        id = 43,
        name = "Length 4",
        range = "linear(0, 0.5)",
        default = 0.05,
        smooth = "none"
    )]
    pub length4: FloatParam,
    #[param(
        id = 44,
        name = "Length 5",
        range = "linear(0, 0.5)",
        default = 0.05,
        smooth = "none"
    )]
    pub length5: FloatParam,
    #[param(
        id = 45,
        name = "Length 6",
        range = "linear(0, 0.5)",
        default = 0.05,
        smooth = "none"
    )]
    pub length6: FloatParam,
    #[param(
        id = 46,
        name = "Length 7",
        range = "linear(0, 0.5)",
        default = 0.05,
        smooth = "none"
    )]
    pub length7: FloatParam,
    #[param(
        id = 47,
        name = "Length 8",
        range = "linear(0, 0.5)",
        default = 0.05,
        smooth = "none"
    )]
    pub length8: FloatParam,
    #[param(
        id = 48,
        name = "Angle 1",
        range = "linear(0, 1)",
        default = 0.0,
        smooth = "none"
    )]
    pub angle1: FloatParam,
    #[param(
        id = 49,
        name = "Angle 2",
        range = "linear(0, 1)",
        default = 0.0,
        smooth = "none"
    )]
    pub angle2: FloatParam,
    #[param(
        id = 50,
        name = "Angle 3",
        range = "linear(0, 1)",
        default = 0.0,
        smooth = "none"
    )]
    pub angle3: FloatParam,
    #[param(
        id = 51,
        name = "Angle 4",
        range = "linear(0, 1)",
        default = 0.0,
        smooth = "none"
    )]
    pub angle4: FloatParam,
    #[param(
        id = 52,
        name = "Angle 5",
        range = "linear(0, 1)",
        default = 0.0,
        smooth = "none"
    )]
    pub angle5: FloatParam,
    #[param(
        id = 53,
        name = "Angle 6",
        range = "linear(0, 1)",
        default = 0.0,
        smooth = "none"
    )]
    pub angle6: FloatParam,
    #[param(
        id = 54,
        name = "Angle 7",
        range = "linear(0, 1)",
        default = 0.0,
        smooth = "none"
    )]
    pub angle7: FloatParam,
    #[param(
        id = 55,
        name = "Angle 8",
        range = "linear(0, 1)",
        default = 0.0,
        smooth = "none"
    )]
    pub angle8: FloatParam,
    #[param(
        id = 56,
        name = "Pan 1",
        range = "linear(0, 1)",
        default = 0.75,
        smooth = "none"
    )]
    pub pan1: FloatParam,
    #[param(
        id = 57,
        name = "Pan 2",
        range = "linear(0, 1)",
        default = 0.75,
        smooth = "none"
    )]
    pub pan2: FloatParam,
    #[param(
        id = 58,
        name = "Pan 3",
        range = "linear(0, 1)",
        default = 0.75,
        smooth = "none"
    )]
    pub pan3: FloatParam,
    #[param(
        id = 59,
        name = "Pan 4",
        range = "linear(0, 1)",
        default = 0.75,
        smooth = "none"
    )]
    pub pan4: FloatParam,
    #[param(
        id = 60,
        name = "Pan 5",
        range = "linear(0, 1)",
        default = 0.75,
        smooth = "none"
    )]
    pub pan5: FloatParam,
    #[param(
        id = 61,
        name = "Pan 6",
        range = "linear(0, 1)",
        default = 0.75,
        smooth = "none"
    )]
    pub pan6: FloatParam,
    #[param(
        id = 62,
        name = "Pan 7",
        range = "linear(0, 1)",
        default = 0.75,
        smooth = "none"
    )]
    pub pan7: FloatParam,
    #[param(
        id = 63,
        name = "Pan 8",
        range = "linear(0, 1)",
        default = 0.75,
        smooth = "none"
    )]
    pub pan8: FloatParam,
    #[param(
        id = 64,
        name = "Width 1",
        range = "linear(0, 1)",
        default = 0.95,
        smooth = "none"
    )]
    pub width1: FloatParam,
    #[param(
        id = 65,
        name = "Width 2",
        range = "linear(0, 1)",
        default = 0.95,
        smooth = "none"
    )]
    pub width2: FloatParam,
    #[param(
        id = 66,
        name = "Width 3",
        range = "linear(0, 1)",
        default = 0.95,
        smooth = "none"
    )]
    pub width3: FloatParam,
    #[param(
        id = 67,
        name = "Width 4",
        range = "linear(0, 1)",
        default = 0.95,
        smooth = "none"
    )]
    pub width4: FloatParam,
    #[param(
        id = 68,
        name = "Width 5",
        range = "linear(0, 1)",
        default = 0.95,
        smooth = "none"
    )]
    pub width5: FloatParam,
    #[param(
        id = 69,
        name = "Width 6",
        range = "linear(0, 1)",
        default = 0.95,
        smooth = "none"
    )]
    pub width6: FloatParam,
    #[param(
        id = 70,
        name = "Width 7",
        range = "linear(0, 1)",
        default = 0.95,
        smooth = "none"
    )]
    pub width7: FloatParam,
    #[param(
        id = 71,
        name = "Width 8",
        range = "linear(0, 1)",
        default = 0.95,
        smooth = "none"
    )]
    pub width8: FloatParam,
    #[param(id = 72, name = "Active 1", default = true)]
    pub active1: BoolParam,
    #[param(id = 73, name = "Active 2", default = false)]
    pub active2: BoolParam,
    #[param(id = 74, name = "Active 3", default = false)]
    pub active3: BoolParam,
    #[param(id = 75, name = "Active 4", default = false)]
    pub active4: BoolParam,
    #[param(id = 76, name = "Active 5", default = false)]
    pub active5: BoolParam,
    #[param(id = 77, name = "Active 6", default = false)]
    pub active6: BoolParam,
    #[param(id = 78, name = "Active 7", default = false)]
    pub active7: BoolParam,
    #[param(id = 79, name = "Active 8", default = false)]
    pub active8: BoolParam,
    #[param(id = 80, name = "Mute 1", default = false)]
    pub mute1: BoolParam,
    #[param(id = 81, name = "Mute 2", default = false)]
    pub mute2: BoolParam,
    #[param(id = 82, name = "Mute 3", default = false)]
    pub mute3: BoolParam,
    #[param(id = 83, name = "Mute 4", default = false)]
    pub mute4: BoolParam,
    #[param(id = 84, name = "Mute 5", default = false)]
    pub mute5: BoolParam,
    #[param(id = 85, name = "Mute 6", default = false)]
    pub mute6: BoolParam,
    #[param(id = 86, name = "Mute 7", default = false)]
    pub mute7: BoolParam,
    #[param(id = 87, name = "Mute 8", default = false)]
    pub mute8: BoolParam,
    #[param(id = 90, name = "Random", default = false)]
    pub random_on: BoolParam,
    #[param(
        id = 91,
        name = "Random Range",
        range = "linear(0, 1)",
        default = 0.35,
        smooth = "none"
    )]
    pub random_range: FloatParam,
    #[param(
        id = 92,
        name = "Random Speed",
        range = "linear(0, 1)",
        default = 0.4,
        smooth = "none"
    )]
    pub random_speed: FloatParam,
    #[param(
        id = 93,
        name = "Random Color",
        range = "linear(0, 3)",
        default = 0.0,
        smooth = "none"
    )]
    pub random_color: FloatParam,
    #[skip]
    pub host: Arc<HostInfo>,
}

pub const PARAM_COUNT: usize = 94;

impl FusionParams {
    pub fn value(&self, id: u32) -> f64 {
        self.get_plain(id).unwrap_or(0.0)
    }
}
