//! 扁平控件族 —— 移植自原版 `controls/FlatButton.h` + `common/controls/FlatCycleButton.h`
//! + `controls/ORMSlider.h`。
//!
//! 全部是「给定绝对矩形 + 立即模式响应」：原版的 retained IControl 在这里等价为
//! 一个画函数 + 一个 `egui::Response`，调用方用返回值决定参数写回。
//! 颜色一律走 `openrm_ui::Theme`，不写死 RGB。

use egui::{Color32, FontId, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2};
use openrm_ui::{fonts, Theme};

/// 原版 `MakeButtonStyle()` 的 valueText：20px SemiBold、居中、COL_900。
pub const BTN_TEXT_PX: f32 = 20.0;
/// 窄按钮（PERS/dBTP）用 11px；Range 刻度用 12px；幽灵按钮 12px。
pub const BTN_TEXT_SMALL_PX: f32 = 11.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BtnState {
    Normal,
    Hover,
    Pressed,
    Active,
    Disabled,
}

/// 画一个扁平按钮（原版 `FlatActionButton` / `FlatToggleControl` / `FlatCycleButton` 的公共外观）。
///
/// * `active` —— 当前是否为激活/按下态（填充 COL_900、文字 COL_100）。
/// * `pressed` —— 本帧鼠标按下（未松开）的瞬时反馈。
pub fn draw_flat(
    painter: &egui::Painter,
    theme: &Theme,
    rect: Rect,
    label: &str,
    text_px: Option<f32>,
    active: bool,
    hover: bool,
    pressed: bool,
) {
    let (bg, fg) = if active || pressed {
        (theme.col_900(), theme.col_100())
    } else {
        (theme.col_300(), theme.col_900())
    };
    painter.rect_filled(rect, 0.0, bg);
    // 原版：未按下且悬停时叠一层 HoverOverlay()
    if hover && !active && !pressed {
        painter.rect_filled(rect, 0.0, theme.hover_overlay());
    }
    if !label.is_empty() {
        let px = text_px.unwrap_or(BTN_TEXT_PX);
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            label,
            fonts::semibold(px),
            fg,
        );
    }
}

/// 瞬时按钮（原版 `MakeMomentary` / `FlatActionButton`）：双击也算一次按下。
pub fn action_button(ui: &mut Ui, theme: &Theme, rect: Rect, label: &str, id: egui::Id) -> Response {
    let resp = ui.interact(rect, id, Sense::click());
    draw_flat(
        ui.painter(),
        theme,
        rect,
        label,
        None,
        false,
        resp.hovered(),
        resp.is_pointer_button_down_on(),
    );
    resp
}

/// 反色开关（原版 `FlatToggleControl` / `InvertToggleControl`）：`on` 时填充深色、文字浅色。
pub fn toggle_button(
    ui: &mut Ui,
    theme: &Theme,
    rect: Rect,
    label: &str,
    on: bool,
    id: egui::Id,
) -> Response {
    let resp = ui.interact(rect, id, Sense::click());
    draw_flat(
        ui.painter(),
        theme,
        rect,
        label,
        None,
        on,
        resp.hovered(),
        false,
    );
    resp
}

/// 循环按钮（原版 `FlatCycleButton`）：单击在档位间循环，返回是否被点击。
///
/// `index_colors` 用于示波器声道按钮（每档底色不同，文字用与背景同灰度的反白色）。
pub struct CycleButton<'a> {
    pub rect: Rect,
    pub labels: &'a [String],
    pub index: usize,
    pub text_px: Option<f32>,
    pub index_colors: Option<Vec<Color32>>,
    pub ghost: bool,
    pub ghost_signal: bool,
    pub scale_style: bool,
    /// 原版 `FlatCycleButton(..., splitChannels=true)`：L/R 档左半 L 色、右半 R 色，
    /// 其余档整块 M 色。通道模式按钮（PWR/L-R/SUM）用这个。
    pub split_channels: bool,
    /// 分色档的显式档位 index（等价原版构造期 `mSplitIdx`：23-27 行 strcmp 捕获一次后
    /// 每帧按 index 比较）。`None` 时回退到文案字面量匹配（兼容既有调用方）；
    /// 中文语言环境文案变为「左右」，字面量匹配会失配，必须走显式 index。
    pub split_idx: Option<usize>,
}

impl<'a> CycleButton<'a> {
    pub fn new(rect: Rect, labels: &'a [String], index: usize) -> Self {
        Self {
            rect,
            labels,
            index,
            text_px: None,
            index_colors: None,
            ghost: false,
            ghost_signal: false,
            scale_style: false,
            split_channels: false,
            split_idx: None,
        }
    }

    /// 原版 `splitChannels=true`（通道模式按钮）；分色档按文案字面量回退匹配。
    /// 语言环境会改文案时请用 [`Self::split_channels_at`] 显式指定档位 index。
    pub fn split_channels(mut self) -> Self {
        self.split_channels = true;
        self.split_idx = None;
        self
    }

    /// 显式指定分色档位 index（等价原版构造期捕获 `mSplitIdx`，此后按 index 比较，
    /// 不随语言文案变化失配）。
    pub fn split_channels_at(mut self, idx: usize) -> Self {
        self.split_channels = true;
        self.split_idx = Some(idx);
        self
    }

    pub fn text_px(mut self, px: f32) -> Self {
        self.text_px = Some(px);
        self
    }

    pub fn ghost(mut self, signal_active: bool) -> Self {
        self.ghost = true;
        self.ghost_signal = signal_active;
        self
    }

    pub fn scale_style(mut self) -> Self {
        self.scale_style = true;
        self
    }

    pub fn draw(&self, ui: &mut Ui, theme: &Theme, id: egui::Id) -> Response {
        let resp = ui.interact(self.rect, id, Sense::click());
        let idx = self.index.min(self.labels.len().saturating_sub(1));
        let p = ui.painter();

        if self.split_channels {
            // 原版 FlatCycleButton::Draw 的 splitChannels 分支：L/R 档左右分色，
            // 其余档整块 M 色；文字统一用与背景同灰度的反白色。
            let (cl, cr, cm) = theme.channel_colors();
            let is_split = self.split_idx.map_or_else(
                || self.labels.get(idx).map(|s| s == "L/R").unwrap_or(false),
                |si| si == idx,
            );
            let wt = fonts::semibold(self.text_px.unwrap_or(BTN_TEXT_PX));
            let fg = split_text_color(theme);
            if is_split {
                let mid = self.rect.center().x;
                p.rect_filled(
                    Rect::from_min_max(self.rect.min, Pos2::new(mid, self.rect.max.y)),
                    0.0,
                    cl,
                );
                p.rect_filled(
                    Rect::from_min_max(Pos2::new(mid, self.rect.min.y), self.rect.max),
                    0.0,
                    cr,
                );
                p.text(
                    Pos2::new((self.rect.min.x + mid) / 2.0, self.rect.center().y),
                    egui::Align2::CENTER_CENTER,
                    "L",
                    wt.clone(),
                    fg,
                );
                p.text(
                    Pos2::new((mid + self.rect.max.x) / 2.0, self.rect.center().y),
                    egui::Align2::CENTER_CENTER,
                    "R",
                    wt,
                    fg,
                );
            } else {
                p.rect_filled(self.rect, 0.0, cm);
                if let Some(l) = self.labels.get(idx) {
                    p.text(
                        self.rect.center(),
                        egui::Align2::CENTER_CENTER,
                        l,
                        wt,
                        fg,
                    );
                }
            }
            if resp.hovered() {
                p.rect_filled(self.rect, 0.0, theme.hover_overlay());
            }
            return resp;
        }

        if self.ghost {
            // 幽灵样式：无底色，只有文字；有信号 → COL_300，空轨 → COL_700
            let color = if self.ghost_signal {
                theme.col_300()
            } else {
                theme.col_700()
            };
            if let Some(l) = self.labels.get(idx) {
                p.text(
                    self.rect.center(),
                    egui::Align2::CENTER_CENTER,
                    l,
                    fonts::semibold(self.text_px.unwrap_or(12.0)),
                    color,
                );
            }
            if resp.hovered() {
                p.rect_filled(self.rect, 0.0, theme.hover_overlay());
            }
            return resp;
        }

        if self.scale_style {
            // 刻度样式：半透明加色底 + 右对齐底部的刻度文字
            let fill = if resp.hovered() {
                Color32::from_rgba_unmultiplied(255, 255, 255, 72)
            } else {
                Color32::from_rgba_unmultiplied(255, 255, 255, 40)
            };
            p.rect_filled(self.rect, 0.0, fill);
            if let Some(l) = self.labels.get(idx) {
                p.text(
                    self.rect.max - Vec2::new(3.0, 1.0),
                    egui::Align2::RIGHT_BOTTOM,
                    l,
                    fonts::regular(14.0),
                    theme.col_700(),
                );
            }
            return resp;
        }

        let bg = match &self.index_colors {
            Some(cols) if idx < cols.len() => cols[idx],
            _ => theme.col_300(),
        };
        p.rect_filled(self.rect, 0.0, bg);
        if resp.hovered() {
            p.rect_filled(self.rect, 0.0, theme.hover_overlay());
        }
        if let Some(l) = self.labels.get(idx) {
            // 分色档：文字用与背景同灰度的反白色（原版 SplitBtnTextColor）
            let fg = if self.index_colors.is_some() {
                split_text_color(theme)
            } else {
                theme.col_900()
            };
            p.text(
                self.rect.center(),
                egui::Align2::CENTER_CENTER,
                l,
                fonts::semibold(self.text_px.unwrap_or(BTN_TEXT_PX)),
                fg,
            );
        }
        resp
    }
}

/// 原版 `FlatCycleButton::SplitBtnTextColor()`：取界面底色三通道最大值做灰。
pub fn split_text_color(theme: &Theme) -> Color32 {
    let bg = theme.col_100();
    let g = bg.r().max(bg.g()).max(bg.b());
    Color32::from_rgb(g, g, g)
}

// ---------------------------------------------------------------------------
// 图标按钮（原版 IconActionButton，SVG 1024×1024 网格缩放到 16px 居中）
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IconAction {
    Undo,
    Redo,
    Save,
    Load,
}

#[derive(Clone, Copy)]
struct SwCmd(u8, [f32; 6]);

const ICON_UNDO: &[SwCmd] = &[
    SwCmd(0, [245.863, 54.752, 0., 0., 0., 0.]),
    SwCmd(1, [0., 300.615, 0., 0., 0., 0.]),
    SwCmd(1, [245.863, 546.504, 0., 0., 0., 0.]),
    SwCmd(1, [321.531, 470.836, 0., 0., 0., 0.]),
    SwCmd(1, [204.725, 354.056, 0., 0., 0., 0.]),
    SwCmd(1, [662.909, 354.056, 0., 0., 0., 0.]),
    SwCmd(2, [803.245, 354.056, 917.010, 467.821, 917.010, 608.157]),
    SwCmd(2, [917.010, 748.493, 803.245, 862.258, 662.909, 862.258]),
    SwCmd(1, [328.566, 862.258, 0., 0., 0., 0.]),
    SwCmd(1, [328.566, 969.248, 0., 0., 0., 0.]),
    SwCmd(1, [662.909, 969.248, 0., 0., 0., 0.]),
    SwCmd(2, [862.338, 969.248, 1024., 807.586, 1024., 608.157]),
    SwCmd(2, [1024., 408.728, 862.338, 247.066, 662.909, 247.066]),
    SwCmd(1, [204.859, 247.066, 0., 0., 0., 0.]),
    SwCmd(1, [321.505, 130.394, 0., 0., 0., 0.]),
    SwCmd(1, [245.863, 54.752, 0., 0., 0., 0.]),
    SwCmd(3, [0.; 6]),
];

const ICON_REDO: &[SwCmd] = &[
    SwCmd(0, [778.137, 54.752, 0., 0., 0., 0.]),
    SwCmd(1, [1024., 300.615, 0., 0., 0., 0.]),
    SwCmd(1, [778.137, 546.504, 0., 0., 0., 0.]),
    SwCmd(1, [702.469, 470.836, 0., 0., 0., 0.]),
    SwCmd(1, [819.275, 354.056, 0., 0., 0., 0.]),
    SwCmd(1, [361.091, 354.056, 0., 0., 0., 0.]),
    SwCmd(2, [220.755, 354.056, 106.990, 467.821, 106.990, 608.157]),
    SwCmd(2, [106.990, 748.493, 220.755, 862.258, 361.091, 862.258]),
    SwCmd(1, [695.434, 862.258, 0., 0., 0., 0.]),
    SwCmd(1, [695.434, 969.248, 0., 0., 0., 0.]),
    SwCmd(1, [361.091, 969.248, 0., 0., 0., 0.]),
    SwCmd(2, [161.662, 969.248, 0., 807.586, 0., 608.157]),
    SwCmd(2, [0., 408.728, 161.662, 247.066, 361.091, 247.066]),
    SwCmd(1, [819.141, 247.066, 0., 0., 0., 0.]),
    SwCmd(1, [702.495, 130.394, 0., 0., 0., 0.]),
    SwCmd(1, [778.137, 54.752, 0., 0., 0., 0.]),
    SwCmd(3, [0.; 6]),
];

const ICON_SAVE: &[SwCmd] = &[
    SwCmd(0, [111.884, 623.884, 0., 0., 0., 0.]),
    SwCmd(1, [111.884, 890.580, 0., 0., 0., 0.]),
    SwCmd(1, [111.884, 917.293, 0., 0., 0., 0.]),
    SwCmd(1, [885.403, 917.293, 0., 0., 0., 0.]),
    SwCmd(1, [912.043, 917.293, 0., 0., 0., 0.]),
    SwCmd(1, [912.043, 623.884, 0., 0., 0., 0.]),
    SwCmd(1, [805.408, 623.884, 0., 0., 0., 0.]),
    SwCmd(1, [778.695, 623.884, 0., 0., 0., 0.]),
    SwCmd(1, [778.695, 543.890, 0., 0., 0., 0.]),
    SwCmd(1, [778.695, 517.250, 0., 0., 0., 0.]),
    SwCmd(1, [992.110, 517.250, 0., 0., 0., 0.]),
    SwCmd(1, [1018.750, 517.250, 0., 0., 0., 0.]),
    SwCmd(1, [1018.750, 997.287, 0., 0., 0., 0.]),
    SwCmd(1, [1018.750, 1024., 0., 0., 0., 0.]),
    SwCmd(1, [31.890, 1024., 0., 0., 0., 0.]),
    SwCmd(1, [5.250, 1024., 0., 0., 0., 0.]),
    SwCmd(1, [5.250, 543.890, 0., 0., 0., 0.]),
    SwCmd(1, [5.250, 517.250, 0., 0., 0., 0.]),
    SwCmd(1, [218.664, 517.250, 0., 0., 0., 0.]),
    SwCmd(1, [245.305, 517.250, 0., 0., 0., 0.]),
    SwCmd(1, [245.305, 597.244, 0., 0., 0., 0.]),
    SwCmd(1, [245.305, 623.884, 0., 0., 0., 0.]),
    SwCmd(1, [111.884, 623.884, 0., 0., 0., 0.]),
    SwCmd(3, [0.; 6]),
    SwCmd(0, [565.281, 202.703, 0., 0., 0., 0.]),
    SwCmd(1, [565.281, 656.172, 0., 0., 0., 0.]),
    SwCmd(1, [565.281, 682.812, 0., 0., 0., 0.]),
    SwCmd(1, [485.359, 682.812, 0., 0., 0., 0.]),
    SwCmd(1, [458.646, 682.812, 0., 0., 0., 0.]),
    SwCmd(1, [458.646, 202.703, 0., 0., 0., 0.]),
    SwCmd(1, [303.943, 357.406, 0., 0., 0., 0.]),
    SwCmd(1, [285.320, 376.029, 0., 0., 0., 0.]),
    SwCmd(1, [229.306, 320.124, 0., 0., 0., 0.]),
    SwCmd(1, [210.613, 301.467, 0., 0., 0., 0.]),
    SwCmd(1, [493.323, 18.680, 0., 0., 0., 0.]),
    SwCmd(1, [511.997, 0., 0., 0., 0., 0.]),
    SwCmd(1, [794.694, 282.697, 0., 0., 0., 0.]),
    SwCmd(1, [813.390, 301.392, 0., 0., 0., 0.]),
    SwCmd(1, [757.340, 357.334, 0., 0., 0., 0.]),
    SwCmd(1, [738.689, 375.948, 0., 0., 0., 0.]),
    SwCmd(1, [565.281, 202.703, 0., 0., 0., 0.]),
    SwCmd(3, [0.; 6]),
];

const ICON_LOAD: &[SwCmd] = &[
    SwCmd(0, [849.750, 419.711, 0., 0., 0., 0.]),
    SwCmd(1, [588.934, 419.711, 0., 0., 0., 0.]),
    SwCmd(1, [549.228, 419.711, 0., 0., 0., 0.]),
    SwCmd(1, [549.228, 96.167, 0., 0., 0., 0.]),
    SwCmd(1, [174.250, 96.167, 0., 0., 0., 0.]),
    SwCmd(1, [174.250, 907.904, 0., 0., 0., 0.]),
    SwCmd(1, [463.552, 907.904, 0., 0., 0., 0.]),
    SwCmd(1, [473.533, 907.904, 0., 0., 0., 0.]),
    SwCmd(1, [473.533, 994.527, 0., 0., 0., 0.]),
    SwCmd(1, [473.533, 1004.144, 0., 0., 0., 0.]),
    SwCmd(1, [114.583, 1004.144, 0., 0., 0., 0.]),
    SwCmd(1, [74.877, 1004.144, 0., 0., 0., 0.]),
    SwCmd(1, [74.877, 38.467, 0., 0., 0., 0.]),
    SwCmd(1, [74.877, 0., 0., 0., 0., 0.]),
    SwCmd(1, [629.805, 0., 0., 0., 0., 0.]),
    SwCmd(1, [638.687, 0., 0., 0., 0., 0.]),
    SwCmd(1, [944.023, 327.623, 0., 0., 0., 0.]),
    SwCmd(1, [949.123, 333.095, 0., 0., 0., 0.]),
    SwCmd(1, [949.123, 567.604, 0., 0., 0., 0.]),
    SwCmd(1, [949.123, 577.293, 0., 0., 0., 0.]),
    SwCmd(1, [859.731, 577.293, 0., 0., 0., 0.]),
    SwCmd(1, [849.750, 577.293, 0., 0., 0., 0.]),
    SwCmd(1, [849.750, 419.711, 0., 0., 0., 0.]),
    SwCmd(3, [0.; 6]),
    SwCmd(0, [820.463, 333.160, 0., 0., 0., 0.]),
    SwCmd(1, [638.693, 138.204, 0., 0., 0., 0.]),
    SwCmd(1, [638.693, 333.160, 0., 0., 0., 0.]),
    SwCmd(1, [820.463, 333.160, 0., 0., 0., 0.]),
    SwCmd(3, [0.; 6]),
    SwCmd(0, [675.921, 856.250, 0., 0., 0., 0.]),
    SwCmd(1, [939.215, 856.250, 0., 0., 0., 0.]),
    SwCmd(1, [949.123, 856.250, 0., 0., 0., 0.]),
    SwCmd(1, [949.123, 769.700, 0., 0., 0., 0.]),
    SwCmd(1, [949.123, 760.083, 0., 0., 0., 0.]),
    SwCmd(1, [673.735, 760.083, 0., 0., 0., 0.]),
    SwCmd(1, [771.797, 665.082, 0., 0., 0., 0.]),
    SwCmd(1, [778.828, 658.271, 0., 0., 0., 0.]),
    SwCmd(1, [715.626, 597.037, 0., 0., 0., 0.]),
    SwCmd(1, [708.596, 590.226, 0., 0., 0., 0.]),
    SwCmd(1, [512.801, 779.900, 0., 0., 0., 0.]),
    SwCmd(1, [484.712, 807.111, 0., 0., 0., 0.]),
    SwCmd(1, [701.566, 1017.184, 0., 0., 0., 0.]),
    SwCmd(1, [708.601, 1024., 0., 0., 0., 0.]),
    SwCmd(1, [771.797, 962.690, 0., 0., 0., 0.]),
    SwCmd(1, [778.785, 955.910, 0., 0., 0., 0.]),
    SwCmd(1, [675.921, 856.250, 0., 0., 0., 0.]),
    SwCmd(3, [0.; 6]),
];

fn icon_path(icon: IconAction) -> &'static [SwCmd] {
    match icon {
        IconAction::Undo => ICON_UNDO,
        IconAction::Redo => ICON_REDO,
        IconAction::Save => ICON_SAVE,
        IconAction::Load => ICON_LOAD,
    }
}

/// 图标按钮（原版 `IconActionButton`）：COL_300 底 + 悬停叠层 + 16px 居中图标。
pub fn icon_button(ui: &mut Ui, theme: &Theme, rect: Rect, icon: IconAction, id: egui::Id) -> Response {
    let resp = ui.interact(rect, id, Sense::click());
    let p = ui.painter();
    p.rect_filled(rect, 0.0, theme.col_300());
    if resp.hovered() {
        p.rect_filled(rect, 0.0, theme.hover_overlay());
    }

    let c = rect.center();
    let s = 16.0 / 1024.0;
    let color = theme.col_900();
    let mut pts: Vec<Pos2> = Vec::new();
    for cmd in icon_path(icon) {
        match cmd.0 {
            0 => {
                if pts.len() > 2 {
                    p.add(egui::Shape::Path(egui::epaint::PathShape::convex_polygon(
                        std::mem::take(&mut pts),
                        color,
                        Stroke::NONE,
                    )));
                } else {
                    pts.clear();
                }
                pts.push(Pos2::new(c.x + (cmd.1[0] - 512.0) * s, c.y + (cmd.1[1] - 512.0) * s));
            }
            1 => pts.push(Pos2::new(c.x + (cmd.1[0] - 512.0) * s, c.y + (cmd.1[1] - 512.0) * s)),
            2 => {
                // 三次贝塞尔按 8 段折线铺开（图标只有 16px，折线误差 < 0.2px）
                let p0 = *pts.last().unwrap_or(&c);
                let p1 = Pos2::new(c.x + (cmd.1[0] - 512.0) * s, c.y + (cmd.1[1] - 512.0) * s);
                let p2 = Pos2::new(c.x + (cmd.1[2] - 512.0) * s, c.y + (cmd.1[3] - 512.0) * s);
                let p3 = Pos2::new(c.x + (cmd.1[4] - 512.0) * s, c.y + (cmd.1[5] - 512.0) * s);
                for i in 1..=8 {
                    let t = i as f32 / 8.0;
                    let u = 1.0 - t;
                    let b0 = u * u * u;
                    let b1 = 3.0 * u * u * t;
                    let b2 = 3.0 * u * t * t;
                    let b3 = t * t * t;
                    pts.push(Pos2::new(
                        p0.x * b0 + p1.x * b1 + p2.x * b2 + p3.x * b3,
                        p0.y * b0 + p1.y * b1 + p2.y * b2 + p3.y * b3,
                    ));
                }
            }
            _ => {
                if pts.len() > 2 {
                    p.add(egui::Shape::Path(egui::epaint::PathShape::convex_polygon(
                        std::mem::take(&mut pts),
                        color,
                        Stroke::NONE,
                    )));
                } else {
                    pts.clear();
                }
            }
        }
    }
    if pts.len() > 2 {
        p.add(egui::Shape::Path(egui::epaint::PathShape::convex_polygon(
            pts,
            color,
            Stroke::NONE,
        )));
    }
    resp
}

// ---------------------------------------------------------------------------
// ORMSlider
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SliderDir {
    Horizontal,
    Vertical,
}

/// 原版 `ORMSlider`：把手半径 8、轨道厚 2（`IVSliderControl` 默认值），
/// 轨道 COL_300、已填充段 COL_500、把手 COL_100 圆 + COL_900 细边。
///
/// `track_far` 等价原版 `SetTrackFar(true)`：垂直贴右缘 / 水平贴顶缘。
#[derive(Clone, Debug)]
pub struct OrmSlider {
    pub rect: Rect,
    pub dir: SliderDir,
    /// 0..1
    pub value: f32,
    pub track_far: bool,
    pub header_visible: bool,
    pub header_label: String,
    pub header_value: String,
}

impl OrmSlider {
    /// 原版 `common/Theme.h:107-108`：实心核心半径 7，白色晕圈宽 1.5。
    pub const HANDLE_R: f32 = 7.0;
    pub const HANDLE_RING: f32 = 1.5;
    pub const TRACK_W: f32 = 2.0;

    pub fn new(rect: Rect, dir: SliderDir) -> Self {
        Self {
            rect,
            dir,
            value: 0.0,
            track_far: false,
            header_visible: false,
            header_label: String::new(),
            header_value: String::new(),
        }
    }

    /// 滑条本体（未含把手悬出区）。
    pub fn strip(&self) -> Rect {
        if self.header_visible {
            match self.dir {
                SliderDir::Horizontal => Rect::from_min_max(
                    Pos2::new(self.rect.min.x, self.rect.min.y + 26.0),
                    self.rect.max,
                ),
                SliderDir::Vertical => Rect::from_min_max(
                    Pos2::new(self.rect.min.x + 26.0, self.rect.min.y),
                    self.rect.max,
                ),
            }
        } else {
            self.rect
        }
    }

    pub fn track_rect(&self) -> Rect {
        let strip = self.strip();
        let travel = strip.shrink(Self::HANDLE_R + Self::HANDLE_RING);
        match self.dir {
            SliderDir::Horizontal => {
                if self.track_far {
                    Rect::from_min_max(
                        Pos2::new(travel.min.x, strip.min.y),
                        Pos2::new(travel.max.x, strip.min.y + Self::TRACK_W),
                    )
                } else {
                    let cy = strip.center().y;
                    Rect::from_min_max(
                        Pos2::new(travel.min.x, cy - Self::TRACK_W / 2.0),
                        Pos2::new(travel.max.x, cy + Self::TRACK_W / 2.0),
                    )
                }
            }
            SliderDir::Vertical => {
                if self.track_far {
                    Rect::from_min_max(
                        Pos2::new(strip.max.x - Self::TRACK_W, travel.min.y),
                        Pos2::new(strip.max.x, travel.max.y),
                    )
                } else {
                    let cx = strip.center().x;
                    Rect::from_min_max(
                        Pos2::new(cx - Self::TRACK_W / 2.0, travel.min.y),
                        Pos2::new(cx + Self::TRACK_W / 2.0, travel.max.y),
                    )
                }
            }
        }
    }

    fn handle_center(&self) -> Pos2 {
        let track = self.track_rect();
        let v = self.value.clamp(0.0, 1.0);
        match self.dir {
            SliderDir::Horizontal => Pos2::new(
                track.min.x + track.width() * v,
                track.center().y,
            ),
            SliderDir::Vertical => Pos2::new(
                track.center().x,
                track.max.y - track.height() * v,
            ),
        }
    }

    /// 画控件并处理拖动；返回新的 0..1 值（未拖动则原值）。
    pub fn ui(&mut self, ui: &mut Ui, theme: &Theme, id: egui::Id) -> f32 {
        let strip = self.strip();
        let resp = ui.interact(strip, id, Sense::click_and_drag());
        if resp.dragged() || resp.clicked() {
            if let Some(pos) = ui.ctx().pointer_interact_pos() {
                let track = self.track_rect();
                let v = match self.dir {
                    SliderDir::Horizontal => {
                        ((pos.x - track.min.x) / track.width().max(1.0)).clamp(0.0, 1.0)
                    }
                    SliderDir::Vertical => {
                        ((track.max.y - pos.y) / track.height().max(1.0)).clamp(0.0, 1.0)
                    }
                };
                self.value = v;
            }
        }

        let p = ui.painter();
        p.rect_filled(strip, 0.0, theme.col_100());

        let track = self.track_rect();
        let r = track.height().min(track.width()) / 2.0;
        p.rect_filled(track, r, theme.col_300());

        let hc = self.handle_center();
        let fill = match self.dir {
            SliderDir::Horizontal => Rect::from_min_max(
                track.min,
                Pos2::new(hc.x.max(track.min.x), track.max.y),
            ),
            SliderDir::Vertical => Rect::from_min_max(
                Pos2::new(track.min.x, hc.y),
                Pos2::new(track.max.x, track.max.y),
            ),
        };
        p.rect_filled(fill, r, theme.col_500());

        // 原版 `UiUtils.h:18-22 DrawKnob`：先白晕圈（外缘半径），再实心深色核心。
        p.circle_filled(hc, Self::HANDLE_R + Self::HANDLE_RING, theme.col_100());
        p.circle_filled(hc, Self::HANDLE_R, theme.col_900());

        if self.header_visible {
            let hdr = match self.dir {
                SliderDir::Horizontal => Rect::from_min_max(
                    self.rect.min,
                    Pos2::new(self.rect.max.x, self.rect.min.y + 26.0),
                ),
                SliderDir::Vertical => Rect::from_min_max(
                    self.rect.min,
                    Pos2::new(self.rect.min.x + 26.0, self.rect.max.y),
                ),
            };
            let font: FontId = fonts::semibold(20.0);
            p.text(
                Pos2::new(hdr.min.x, hdr.center().y),
                egui::Align2::LEFT_CENTER,
                &self.header_label,
                font.clone(),
                theme.col_900(),
            );
            p.text(
                Pos2::new(hdr.max.x, hdr.center().y),
                egui::Align2::RIGHT_CENTER,
                &self.header_value,
                fonts::regular(20.0),
                theme.col_700(),
            );
        }

        self.value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slider_track_and_handle_geometry() {
        let mut s = OrmSlider::new(
            Rect::from_min_size(Pos2::new(400.0, 366.0), Vec2::new(20.0, 233.0)),
            SliderDir::Vertical,
        );
        s.track_far = true;
        s.value = 0.5;
        let t = s.track_rect();
        assert_eq!(t.min.x, 420.0 - 2.0);
        assert_eq!(t.max.x, 420.0);
        assert_eq!(t.min.y, 374.5); // 366 + 8.5（HANDLE_R + HANDLE_RING）
        assert_eq!(t.max.y, 590.5); // 599 - 8.5
        let hc = s.handle_center();
        assert!((hc.y - (t.max.y - t.height() * 0.5)).abs() < 1e-3);
    }
}
