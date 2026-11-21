//! CPU 占用表 —— 移植自原版 `controls/CpuMeterControl.h`。
//!
//! 原版只读显示：COL_300 底 + `%.2f%%` 文本（20px SemiBold、居中、COL_900），
//! 值由插件经 `kMsgTagCpu` 推送（UI 线程耗时 / 帧预算）。

use egui::Rect;
use openrm_ui::{fonts, Theme};

#[derive(Clone, Debug)]
pub struct CpuMeter {
    pub rect: Rect,
    /// 0..1
    cpu: f64,
}

impl CpuMeter {
    pub fn new(rect: Rect) -> Self {
        Self { rect, cpu: 0.0 }
    }

    /// 等价原版 `OnMsgFromDelegate(kMsgTagCpu)`。
    pub fn set_cpu(&mut self, v: f64) {
        if v != self.cpu {
            self.cpu = v;
        }
    }

    pub fn cpu(&self) -> f64 {
        self.cpu
    }

    pub fn draw(&self, painter: &egui::Painter, theme: &Theme) {
        painter.rect_filled(self.rect, 0.0, theme.col_300());
        painter.text(
            self.rect.center(),
            egui::Align2::CENTER_CENTER,
            format!("{:.2}%", self.cpu * 100.0),
            fonts::semibold(20.0),
            theme.col_900(),
        );
    }
}
