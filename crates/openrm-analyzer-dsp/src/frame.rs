//! 四个分析引擎共用的输出帧类型。
//!
//! 原版每个引擎都是 `ISender`，每 hop（1024 样本）向 UI 推一包
//! `std::array<float, 8192>` × 3 路；这里用等价的 `EngineFrame`。
//! 值一律是**线性幅度**，dB 换算交给 UI 控件（与原版一致）。

#[derive(Clone, Debug, Default)]
pub struct EngineFrame {
    /// 三路：0=L/左, 1=R/右, 2=M/中（PWR 模式下的合并路），长度 = 带数或 bin 数。
    pub vals: Vec<[f32; 3]>,
    /// 各带中心频率（Hz）。FFT 模式为空（UI 自己按 bin 频率算）。
    pub freqs: Vec<f64>,
}

impl EngineFrame {
    pub fn len(&self) -> usize {
        self.vals.len()
    }
    pub fn is_empty(&self) -> bool {
        self.vals.is_empty()
    }
}
