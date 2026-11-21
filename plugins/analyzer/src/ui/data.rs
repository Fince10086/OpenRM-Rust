//! 音频线程 → UI 线程的数据契约（原版 `ISender` 消息包在 Rust 侧的等价物）。
//!
//! 原版 SpectrumPad 收到的 `TDataPacket` 是 `std::array<float, 8192>` ×3 路，
//! 值是**线性幅度**（不是 dB）；所有 dB 换算、平滑、峰值保持都在控件里做。
//! 这里保持同一语义：引擎只交线性幅度，UI 负责其余。

pub use openrm_analyzer_dsp::level_meter::LevelSnapshot;
pub use openrm_analyzer_dsp::loudness_meter::LoudnessSnapshot;

/// 一次分析帧（对应原版一条 `kUpdateMessage`）。
///
/// * FFT 模式：`vals` 长度 = `num_bins`（= fft_size / 2），`freqs` 可为空。
/// * VQT / PBT / RTA：`vals` 长度 = 各自带数，`freqs` 为各带中心频率（Hz）。
#[derive(Clone, Debug, Default)]
pub struct SpectrumFrame {
    pub l: Vec<f32>,
    pub r: Vec<f32>,
    pub m: Vec<f32>,
    pub freqs: Vec<f64>,
}

/// 示波器 / 声像图的一个 hop 数据包（原版 `StereoScope` 的 1024 样本对）。
#[derive(Clone, Debug, Default)]
pub struct ScopeFrame {
    pub l: Vec<f32>,
    pub r: Vec<f32>,
}
