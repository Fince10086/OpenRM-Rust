//! OpenRM 纯 Rust DSP 核心。
//!
//! 设计约束：这一层不依赖任何插件框架（truce / iPlug2 / clap），只处理样本。
//! 插件层负责参数单位换算、线程边界与 UI；算法层可离线跑回归测试，
//! 也可被将来的 CombFilter / Analyzer 直接复用。
//!
//! 移植基线：C++ 版 `tests/dsp_test.cpp` 的全部断言，见 `tests/bandpass_parity.rs`。

pub mod bandpass;
pub mod response;
pub mod random_walk;
pub mod spectrum;
pub mod svf;

pub use bandpass::{BandPassCore, Params, RandomDeltas};
pub use random_walk::RandomWalk;
pub use svf::{SvfFilter, SvfOutputs};

/// 把 dB 换算成线性幅度。
#[inline]
pub fn db_to_linear(db: f32) -> f32 {
    10.0f32.powf(db / 20.0)
}

/// 把线性幅度换算成 dB（下限 -200 dB，避免 log(0)）。
#[inline]
pub fn linear_to_db(lin: f32) -> f32 {
    if lin <= 1e-10 {
        -200.0
    } else {
        20.0 * lin.log10()
    }
}
