//! ORM Analyzer 的分析引擎（移植自 C++ `plugins/Analyzer/src/dsp` 与
//! `plugins/common/dsp/SpectrumSTFT.h`）。
//!
//! 与 BandPass 一样，这里只保留算法；采样率、块长、UI 快照都由插件层驱动。
//! 四个引擎对外的统一出口是 `frame::EngineFrame`（每 hop 一帧、线性幅度）。

pub mod fastmath;
pub mod frame;
pub mod halfband;
pub mod level_meter;
pub mod loudness_meter;
pub mod pbt;
pub mod rta;
pub mod stereo_scope;
pub mod stft;
pub mod test_signal;
pub mod vqt;
