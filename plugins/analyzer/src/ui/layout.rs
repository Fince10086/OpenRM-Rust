//! 全局布局 —— 从原版 `Analyzer.cpp` 的 `AttachControl(...)` 与常量段逐行抄来。
//!
//! 960×635 逻辑像素，全部矩形都是绝对坐标（与原版 IRECT(左, 上, 右, 下) 一致）。
//! 任何控件都不许自己写死这些数字，一律从这里取，保证改一处全窗同步。

use egui::{pos2, vec2, Rect};

pub const UI_W: f32 = 960.0;
pub const UI_H: f32 = 635.0;

// 右栏（Analyzer.cpp 第 230–236 行）
pub const COL1_X: f32 = 784.0;
pub const BTN_W: f32 = 76.0;
pub const BTN_GAP: f32 = 4.0;
pub const COL2_X: f32 = COL1_X + BTN_W + BTN_GAP; // 864
pub const PANEL_R: f32 = COL2_X + BTN_W; // 940

// 顶行按钮（第 238–241 行）
pub const TOP_BTN_W: f32 = 62.0;
pub const TOP_BTN_H: f32 = 26.0;
pub const TOP_BTN_Y: f32 = 26.0;
pub const TOP_BTN_GAP: f32 = 6.0;

// 主频谱（第 318 行）
pub const PLOT: Rect = Rect {
    min: pos2(20.0, 58.0),
    max: pos2(784.0, 328.0),
};

// 下半区（第 356–362 行）
pub const BOT_BTN_Y: f32 = 334.0;
pub const BOT_BTN_H: f32 = 26.0;
pub const BOT_BTN_GAP: f32 = 6.0;
pub const BOTTOM_TOP: f32 = BOT_BTN_Y + BOT_BTN_H + 6.0; // 366
pub const BOTTOM_MID_X: f32 = 398.0;
pub const SCOPE_H: f32 = 233.0;
pub const SCOPE_B: f32 = BOTTOM_TOP + SCOPE_H; // 599

// 电平条宽度（AnalyzerMeterUi.h 第 16–22 行）
pub const GAIN_BAR_W: f32 = 14.0;
pub const VU_SCALE_W: f32 = 28.0;
pub const VU_BAR_W: f32 = 14.0;
pub const LUFS_SCALE_W: f32 = 28.0;
pub const LOUD_BAR_W: f32 = 14.0;
pub const LRA_ZONE_W: f32 = 32.0;
pub const METER_STRIP_W: f32 =
    2.0 * GAIN_BAR_W + VU_SCALE_W + 2.0 * VU_BAR_W + LUFS_SCALE_W + 4.0 * LOUD_BAR_W + LRA_ZONE_W; // 200

// 示波器右缘 = LUFS Integrated 条左缘 = 784 - 200 + 28 + 28 + 28 + 28 = 696
pub const SCOPE_R: f32 = 784.0 - METER_STRIP_W + 2.0 * GAIN_BAR_W + VU_SCALE_W + 2.0 * VU_BAR_W + LUFS_SCALE_W;

// 右栏控件簇（第 486–492 行）
pub const ROW_GAP: f32 = BTN_GAP;
pub const ROW_H: f32 = 26.0;
pub const ICON_ROW_Y: f32 = SCOPE_B - 74.0 - ROW_H; // 499
pub const HOLD_ROW_Y: f32 = ICON_ROW_Y - ROW_GAP - ROW_H; // 469
pub const FREEZE_ROW_Y: f32 = HOLD_ROW_Y - ROW_GAP - ROW_H; // 439
pub const CPU_Y: f32 = FREEZE_ROW_Y - ROW_GAP - ROW_H; // 409
pub const ICON_BTN_W: f32 = (BTN_W - BTN_GAP) / 2.0; // 36
pub const ICON_STEP: f32 = ICON_BTN_W + BTN_GAP; // 40

// 声像面板按钮行（第 402–436 行）
pub const PANEL_BTN_W_FIELD: f32 = 76.0;
pub const PANEL_BTN_W_PERS: f32 = 86.0;
pub const PANEL_BTN_W_ZOOM: f32 = 52.0;

// 频谱底部按钮（第 475–478 行）
pub const RANGE_BTN_W: f32 = 38.0;
pub const RANGE_BTN_H: f32 = 19.0;

/// 示波器内嵌滑杆尺寸与下缘按钮起点（OscilloscopeControl::kSliderW）。
pub const SCOPE_SLIDER_W: f32 = 20.0;

const fn rect_c(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect {
        min: pos2(x, y),
        max: pos2(x + w, y + h),
    }
}

/// 全窗矩形集合。字段名与原版控件一一对应。
#[derive(Clone, Copy, Debug)]
pub struct Layout {
    pub window: Rect,

    // 顶行 7 个按钮
    pub chan_mode: Rect,
    pub mode: Rect,
    pub res: Rect,
    pub window_btn: Rect,
    pub release_mode: Rect,
    pub slope: Rect,
    pub speed: Rect,

    pub plot: Rect,
    pub meter_strip: Rect, // plot 右缘的 200px 电平条

    // 下半区按钮行
    pub scope_mode: Rect,
    pub gonio_persist: Rect,
    pub gonio_zoom: Rect,
    pub scope_chan: Rect,
    pub scope_trig: Rect,

    pub scope_panel: Rect, // 声像 / 矢量示波（同一矩形，互斥显示）
    pub oscilloscope: Rect,

    // 右栏
    pub cpu: Rect,
    pub icon_undo: Rect,
    pub icon_redo: Rect,
    pub icon_save: Rect,
    pub icon_load: Rect,
    pub level_mode: Rect, // 幽灵按钮，压在电平条上
    pub freeze: Rect,
    pub reset: Rect,
    pub hold_toggle: Rect,
    pub hold_time: Rect,

    pub range_btn: Rect,

    // 标题区
    pub title_orm: Rect,
    pub title_analyzer: Rect,
    pub version: Rect,
}

impl Default for Layout {
    fn default() -> Self {
        Self::new()
    }
}

impl Layout {
    pub fn new() -> Self {
        // 顶行 x 递推（第 244–313 行）
        let chan_mode = rect_c(20.0, TOP_BTN_Y, TOP_BTN_W, TOP_BTN_H);
        let mode_x = 20.0 + TOP_BTN_W + TOP_BTN_GAP; // 88
        let mode = rect_c(mode_x, TOP_BTN_Y, TOP_BTN_W, TOP_BTN_H);
        let res_x = mode_x + TOP_BTN_W; // 150
        let res_w = 31.0;
        let res = rect_c(res_x, TOP_BTN_Y, res_w, TOP_BTN_H);
        let win_x = res_x + res_w + TOP_BTN_GAP; // 187
        let window_btn = rect_c(win_x, TOP_BTN_Y, TOP_BTN_W, TOP_BTN_H);
        let log_x = win_x + TOP_BTN_W + TOP_BTN_GAP; // 255
        let release_mode = rect_c(log_x, TOP_BTN_Y, TOP_BTN_W, TOP_BTN_H);
        let slope_x = log_x + TOP_BTN_W + TOP_BTN_GAP; // 323
        let slope_w = 116.0;
        let slope = rect_c(slope_x, TOP_BTN_Y, slope_w, TOP_BTN_H);
        let speed_x = slope_x + slope_w + TOP_BTN_GAP; // 445
        let speed = rect_c(speed_x, TOP_BTN_Y, TOP_BTN_W, TOP_BTN_H);

        let plot = PLOT;
        let meter_strip = Rect::from_min_max(pos2(PLOT.max.x - METER_STRIP_W, PLOT.min.y), PLOT.max);

        // 声像面板按钮（第 402 行起）
        let scope_mode = rect_c(20.0, BOT_BTN_Y, PANEL_BTN_W_FIELD, BOT_BTN_H);
        let pers_x = 20.0 + PANEL_BTN_W_FIELD + BOT_BTN_GAP; // 102
        let gonio_persist = rect_c(pers_x, BOT_BTN_Y, PANEL_BTN_W_PERS, BOT_BTN_H);
        let zoom_x = pers_x + PANEL_BTN_W_PERS + BOT_BTN_GAP; // 194
        let gonio_zoom = rect_c(zoom_x, BOT_BTN_Y, PANEL_BTN_W_ZOOM, BOT_BTN_H);

        // 示波器按钮行（第 363–389 行）
        let mut scope_btn_x = BOTTOM_MID_X + 2.0 + SCOPE_SLIDER_W; // 420
        let scope_chan = rect_c(scope_btn_x, BOT_BTN_Y, 34.0, BOT_BTN_H);
        scope_btn_x += 34.0 + BOT_BTN_GAP; // 460
        let scope_trig = rect_c(scope_btn_x, BOT_BTN_Y, TOP_BTN_W, BOT_BTN_H);

        let scope_panel = Rect::from_min_max(pos2(20.0, BOTTOM_TOP), pos2(BOTTOM_MID_X - 4.0, SCOPE_B));
        let oscilloscope = Rect::from_min_max(pos2(BOTTOM_MID_X + 2.0, BOTTOM_TOP), pos2(SCOPE_R, SCOPE_B));

        // 右栏
        let cpu = rect_c(COL1_X, CPU_Y, PANEL_R - COL1_X, ROW_H);
        let icon_undo = rect_c(COL1_X, ICON_ROW_Y, ICON_BTN_W, ROW_H);
        let icon_redo = rect_c(COL1_X + ICON_STEP, ICON_ROW_Y, ICON_BTN_W, ROW_H);
        let icon_save = rect_c(COL1_X + 2.0 * ICON_STEP, ICON_ROW_Y, ICON_BTN_W, ROW_H);
        let icon_load = rect_c(COL1_X + 3.0 * ICON_STEP, ICON_ROW_Y, PANEL_R - (COL1_X + 3.0 * ICON_STEP), ROW_H);
        let level_mode = rect_c(PLOT.max.x - METER_STRIP_W, PLOT.max.y - RANGE_BTN_H, 2.0 * GAIN_BAR_W, RANGE_BTN_H);
        let freeze = rect_c(COL1_X, FREEZE_ROW_Y, BTN_W, ROW_H);
        let reset = rect_c(COL2_X, FREEZE_ROW_Y, PANEL_R - COL2_X, ROW_H);
        let hold_toggle = rect_c(COL1_X, HOLD_ROW_Y, BTN_W, ROW_H);
        let hold_time = rect_c(COL2_X, HOLD_ROW_Y, PANEL_R - COL2_X, ROW_H);

        // RANGE：频谱底部右缘，画区右缘（不含电平条）
        let plot_r = PLOT.max.x - METER_STRIP_W; // 584
        let range_btn = rect_c(plot_r - RANGE_BTN_W, PLOT.max.y - RANGE_BTN_H, RANGE_BTN_W, RANGE_BTN_H);

        // 标题区（第 577–590 行）
        let title_orm = Rect::from_min_max(pos2(COL1_X, SCOPE_B - 66.0), pos2(COL1_X + 120.0, SCOPE_B - 32.0));
        let title_analyzer = Rect::from_min_max(pos2(COL1_X, SCOPE_B - 34.0), pos2(PANEL_R, SCOPE_B));
        let version = Rect::from_min_max(pos2(COL1_X, SCOPE_B - 69.0), pos2(PANEL_R, SCOPE_B - 35.0));

        Self {
            window: Rect::from_min_size(pos2(0.0, 0.0), vec2(UI_W, UI_H)),
            chan_mode,
            mode,
            res,
            window_btn,
            release_mode,
            slope,
            speed,
            plot,
            meter_strip,
            scope_mode,
            gonio_persist,
            gonio_zoom,
            scope_chan,
            scope_trig,
            scope_panel,
            oscilloscope,
            cpu,
            icon_undo,
            icon_redo,
            icon_save,
            icon_load,
            level_mode,
            freeze,
            reset,
            hold_toggle,
            hold_time,
            range_btn,
            title_orm,
            title_analyzer,
            version,
        }
    }

    /// 齿轮按钮：原版先 `MeasureText` 量 "ORM" 的 ink 宽度，再 `gearL = ink.R + 8`、
    /// `gearR = gearL + ink 高度`。egui 侧把量出来的墨水宽度传进来即可。
    pub fn gear_rect(&self, orm_ink_w: f32) -> Rect {
        let ink_h = self.title_orm.height();
        let gear_l = self.title_orm.min.x + orm_ink_w + 8.0;
        Rect::from_min_size(pos2(gear_l, self.title_orm.min.y), vec2(ink_h, ink_h))
    }

    /// 版本号矩形：左缘 = `gearR + 8`。
    pub fn version_rect(&self, gear: Rect) -> Rect {
        Rect::from_min_max(pos2(gear.max.x + 8.0, self.version.min.y), pos2(PANEL_R, self.version.max.y))
    }
}

/// 色板/盒子绘制时统一使用的面板内边距：原版卡片 `kPad=20`。
pub const CARD_PAD: f32 = 20.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_reference_constants() {
        assert_eq!(COL2_X, 864.0);
        assert_eq!(PANEL_R, 940.0);
        assert_eq!(BOTTOM_TOP, 366.0);
        assert_eq!(SCOPE_B, 599.0);
        assert_eq!(METER_STRIP_W, 200.0);
        assert_eq!(SCOPE_R, 696.0);
        assert_eq!(ICON_ROW_Y, 499.0);
        assert_eq!(HOLD_ROW_Y, 469.0);
        assert_eq!(FREEZE_ROW_Y, 439.0);
        assert_eq!(CPU_Y, 409.0);
        assert_eq!(ICON_BTN_W, 36.0);

        let l = Layout::new();
        assert_eq!(l.chan_mode, rect_c(20.0, 26.0, 62.0, 26.0));
        assert_eq!(l.mode, rect_c(88.0, 26.0, 62.0, 26.0));
        assert_eq!(l.res, rect_c(150.0, 26.0, 31.0, 26.0));
        assert_eq!(l.window_btn, rect_c(187.0, 26.0, 62.0, 26.0));
        assert_eq!(l.release_mode, rect_c(255.0, 26.0, 62.0, 26.0));
        assert_eq!(l.slope, rect_c(323.0, 26.0, 116.0, 26.0));
        assert_eq!(l.speed, rect_c(445.0, 26.0, 62.0, 26.0));
        assert_eq!(l.scope_chan, rect_c(420.0, 334.0, 34.0, 26.0));
        assert_eq!(l.scope_trig, rect_c(460.0, 334.0, 62.0, 26.0));
        assert_eq!(l.oscilloscope, Rect::from_min_max(pos2(400.0, 366.0), pos2(696.0, 599.0)));
        assert_eq!(l.range_btn, rect_c(546.0, 309.0, 38.0, 19.0));
        assert_eq!(l.level_mode, rect_c(584.0, 309.0, 28.0, 19.0));
        assert_eq!(l.icon_load, rect_c(904.0, 499.0, 36.0, 26.0));
        assert_eq!(l.hold_time, rect_c(864.0, 469.0, 76.0, 26.0));
    }
}
