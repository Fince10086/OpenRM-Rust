//! StereoScope — 双通道示波器：每 hop（1024 样本）攒出一对 L/R 原始样本序列。
//!
//! 逐行移植自 `plugins/Analyzer/src/dsp/StereoScope.h`。
//! 输出帧 `vals[i] = [L[i], R[i], 0.0]`，`freqs` 为空（示波器不做频谱）。

use crate::frame::EngineFrame;

const K_HOP: usize = 1024;

pub struct StereoScope {
    buf_count: usize,
    pending: [[f32; K_HOP]; 2],
    frame: Option<EngineFrame>,
}

impl StereoScope {
    pub fn new() -> Self {
        Self {
            buf_count: 0,
            pending: [[0.0; K_HOP]; 2],
            frame: None,
        }
    }

    pub fn set_sample_rate(&mut self, _sr: f64) {
        // 示波器不做速率相关处理。
    }

    /// 对应原版 `ProcessBlock`：逐样本攒 hop；M 路在示波器中不用。
    pub fn process_block(&mut self, l: &[f32], r: &[f32], _m: &[f32]) {
        let n = l.len().min(r.len());
        for s in 0..n {
            self.pending[0][self.buf_count] = l[s];
            self.pending[1][self.buf_count] = r[s];
            self.buf_count += 1;
            if self.buf_count == K_HOP {
                let vals: Vec<[f32; 3]> = (0..K_HOP)
                    .map(|i| [self.pending[0][i], self.pending[1][i], 0.0])
                    .collect();
                self.frame = Some(EngineFrame {
                    vals,
                    freqs: Vec::new(),
                });
                self.buf_count = 0;
            }
        }
    }

    pub fn take_frame(&mut self) -> Option<EngineFrame> {
        self.frame.take()
    }

    pub fn reset(&mut self) {
        self.buf_count = 0;
        self.frame = None;
    }
}

impl Default for StereoScope {
    fn default() -> Self {
        Self::new()
    }
}
