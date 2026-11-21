//! 基础控件。全部是绝对定位 + `Painter` 自绘，坐标与原 C++ 版 916×646
//! 画布一一对应，方便逐屏对照。

use egui::{
    Align2, Color32, CursorIcon, Id, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2,
};
use std::f32::consts::PI;

use crate::{fonts, theme::Theme};

pub const F_MIN: f64 = 20.0;
pub const F_MAX: f64 = 20_000.0;
pub const BW_MIN: f64 = 1.0;
pub const BW_MAX: f64 = 31.0;

pub fn freq_to_x(f: f64, plot: Rect) -> f32 {
    let n = (f.clamp(F_MIN, F_MAX) / F_MIN).ln() / (F_MAX / F_MIN).ln();
    plot.left() + n as f32 * plot.width()
}

pub fn x_to_freq(x: f32, plot: Rect) -> f64 {
    let n = ((x - plot.left()) / plot.width()).clamp(0.0, 1.0) as f64;
    F_MIN * (F_MAX / F_MIN).powf(n)
}

fn bw_to_y(bw: f64, plot: Rect) -> f32 {
    let n = (bw.clamp(BW_MIN, BW_MAX) / BW_MIN).ln() / (BW_MAX / BW_MIN).ln();
    plot.bottom() - n as f32 * plot.height()
}

fn y_to_bw(y: f32, plot: Rect) -> f64 {
    let n = ((plot.bottom() - y) / plot.height()).clamp(0.0, 1.0) as f64;
    BW_MIN * (BW_MAX / BW_MIN).powf(n)
}

pub fn fmt_hz(f: f64) -> String {
    if f >= 10_000.0 {
        format!("{:.1}k", f / 1000.0)
    } else if f >= 1000.0 {
        format!("{:.2}k", f / 1000.0)
    } else if f >= 100.0 {
        format!("{:.0}", f)
    } else {
        format!("{:.1}", f)
    }
}

pub fn fmt_db(db: f32) -> String {
    if db <= -95.9 {
        "-inf".to_string()
    } else {
        format!("{db:+.1}")
    }
}

/// 滤波垫：X = 频率（对数），Y = 带宽倍率（对数）。与 C++ `FilterNodePad` 同构。
pub struct PadModel {
    pub label: &'static str,
    pub freq: f64,
    pub bw_mult: f64,
    pub reject: bool,
}

#[derive(Default)]
pub struct PadEdit {
    pub freq: Option<f64>,
    pub bw_mult: Option<f64>,
    pub toggle_pass: bool,
    pub began: bool,
    pub ended: bool,
}

pub fn filter_pad(ui: &mut Ui, rect: Rect, model: &PadModel, theme: &Theme, id: Id) -> PadEdit {
    let mut edit = PadEdit::default();
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 0, theme.col_100());

    let plot = Rect::from_min_max(
        Pos2::new(rect.left() + 8.0, rect.top() + 22.0),
        Pos2::new(rect.right() - 8.0, rect.bottom() - 22.0),
    );

    // 低端/高端的分段底色（原版 DrawTrack 的 zebra 条）
    for (i, (lo, hi)) in [(20.0, 100.0), (100.0, 1000.0), (1000.0, 10_000.0), (10_000.0, 20_000.0)]
        .iter()
        .enumerate()
    {
        let x0 = freq_to_x(*lo, plot);
        let x1 = freq_to_x(*hi, plot);
        let gray = if i % 2 == 0 { 240 } else { 246 };
        p.rect_filled(
            Rect::from_min_max(Pos2::new(x0, plot.top()), Pos2::new(x1, plot.bottom())),
            0,
            theme.warm_gray(gray),
        );
    }

    // 竖向八度线
    let mut f = 20.0f64;
    while f <= 20_000.0 {
        let x = freq_to_x(f, plot);
        let strong = (f / 1000.0).log10().fract().abs() < 0.001;
        let color = if strong { theme.warm_gray(212) } else { theme.warm_gray(228) };
        p.line_segment(
            [Pos2::new(x, plot.top()), Pos2::new(x, plot.bottom())],
            Stroke::new(1.0_f32, color),
        );
        f *= 2.0;
    }

    // 横向带宽线
    for bw in [2.0f64, 4.0, 8.0, 16.0, 31.0] {
        let y = bw_to_y(bw, plot);
        p.line_segment(
            [Pos2::new(plot.left(), y), Pos2::new(plot.right(), y)],
            Stroke::new(1.0_f32, theme.warm_gray(228)),
        );
    }

    // 轴标签
    for (f, label) in [(100.0, "100"), (1000.0, "1k"), (10_000.0, "10k")] {
        p.text(
            Pos2::new(freq_to_x(f, plot), plot.bottom() + 4.0),
            Align2::CENTER_TOP,
            label,
            fonts::regular(10.0),
            theme.col_700(),
        );
    }
    for (bw, label) in [(2.0, "2"), (8.0, "8"), (31.0, "31")] {
        p.text(
            Pos2::new(plot.right() - 4.0, bw_to_y(bw, plot)),
            Align2::RIGHT_CENTER,
            label,
            fonts::regular(9.0),
            theme.col_700(),
        );
    }

    // 标题与旁标
    p.text(
        Pos2::new(rect.left() + 8.0, rect.top() + 4.0),
        Align2::LEFT_TOP,
        model.label,
        fonts::bold(11.0),
        theme.col_500(),
    );

    // PASS / REJECT 胶囊
    let chip = Rect::from_min_size(
        Pos2::new(rect.right() - 66.0, rect.top() + 2.0),
        Vec2::new(58.0, 18.0),
    );
    let chip_resp = ui.interact(chip, id.with("pass"), Sense::click());
    let chip_bg = if model.reject {
        theme.col_900()
    } else {
        theme.col_300()
    };
    p.rect_filled(chip, 3, chip_bg);
    if chip_resp.hovered() {
        p.rect_filled(chip, 3, theme.hover_overlay());
    }
    p.text(
        chip.center(),
        Align2::CENTER_CENTER,
        if model.reject { "REJECT" } else { "PASS" },
        fonts::semibold(9.0),
        if model.reject { theme.col_100() } else { theme.col_900() },
    );
    if chip_resp.clicked() {
        edit.toggle_pass = true;
    }

    // 手柄（频率 × 带宽）
    let hx = freq_to_x(model.freq, plot);
    let hy = bw_to_y(model.bw_mult, plot);
    let handle = Pos2::new(hx, hy);
    let hit = Rect::from_center_size(handle, Vec2::splat(26.0));
    let resp = ui.interact(hit, id.with("handle"), Sense::click_and_drag());

    if resp.hovered() || resp.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::Grab);
    }
    if resp.drag_started() {
        edit.began = true;
    }
    if resp.dragged() {
        if let Some(pos) = ui.ctx().pointer_interact_pos() {
            edit.freq = Some(x_to_freq(pos.x, plot));
            edit.bw_mult = Some(y_to_bw(pos.y, plot));
        }
    }
    if resp.drag_stopped() {
        edit.ended = true;
    }
    if resp.double_clicked() {
        edit.freq = Some(1095.445);
        edit.bw_mult = Some(3.65);
    }
    if resp.hovered() {
        let scroll = ui.ctx().input(|i| i.smooth_scroll_delta.y);
        if scroll.abs() > 0.0 {
            let factor = (scroll / 300.0).exp2() as f64;
            edit.bw_mult = Some((model.bw_mult * factor).clamp(BW_MIN, BW_MAX));
            edit.began = true;
            edit.ended = true;
        }
    }

    // 随机目标幽灵（仅提示，暂未接线）
    p.circle_filled(handle, 9.0, theme.col_900());
    p.circle_filled(handle, 2.5, theme.col_100());

    edit
}

/// 底部带宽滑杆：两个手柄分别控制低/高截止，内部保持中心×带宽。
pub struct BandEdit {
    pub freq: Option<f64>,
    pub bw_mult: Option<f64>,
    pub began: bool,
    pub ended: bool,
}

pub fn band_slider(
    ui: &mut Ui,
    rect: Rect,
    freq: f64,
    bw_mult: f64,
    theme: &Theme,
    id: Id,
) -> BandEdit {
    let mut edit = BandEdit {
        freq: None,
        bw_mult: None,
        began: false,
        ended: false,
    };
    let p = ui.painter_at(rect);
    let track = Rect::from_min_max(
        Pos2::new(rect.left() + 8.0, rect.center().y - 3.0),
        Pos2::new(rect.right() - 8.0, rect.center().y + 3.0),
    );

    p.rect_filled(track, 0, theme.warm_gray(226));

    let lo = (freq / 2f64.powf(2.0 * bw_mult.log2() / 2.0)).max(F_MIN);
    let hi = (freq * 2f64.powf(2.0 * bw_mult.log2() / 2.0)).min(F_MAX);
    let lo_x = freq_to_x(lo, track);
    let hi_x = freq_to_x(hi, track);
    p.rect_filled(
        Rect::from_min_max(Pos2::new(lo_x, track.top()), Pos2::new(hi_x, track.bottom())),
        0,
        theme.col_500(),
    );

    p.text(
        Pos2::new(rect.left() + 8.0, rect.top() + 2.0),
        Align2::LEFT_TOP,
        format!("LOW CUT  {}", fmt_hz(lo)),
        fonts::regular(10.0),
        theme.col_700(),
    );
    p.text(
        Pos2::new(rect.right() - 8.0, rect.top() + 2.0),
        Align2::RIGHT_TOP,
        format!("HIGH CUT  {}", fmt_hz(hi)),
        fonts::regular(10.0),
        theme.col_700(),
    );

    let lo_handle = Rect::from_center_size(Pos2::new(lo_x, track.center().y), Vec2::splat(22.0));
    let hi_handle = Rect::from_center_size(Pos2::new(hi_x, track.center().y), Vec2::splat(22.0));

    for (which, hr, is_low) in [("lo", lo_handle, true), ("hi", hi_handle, false)] {
        let resp = ui.interact(hr, id.with(which), Sense::click_and_drag());
        if resp.hovered() || resp.dragged() {
            ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
        }
        if resp.drag_started() {
            edit.began = true;
        }
        if resp.dragged() {
            if let Some(pos) = ui.ctx().pointer_interact_pos() {
                let edge = x_to_freq(pos.x, track);
                let (new_lo, new_hi) = if is_low { (edge, hi) } else { (lo, edge) };
                if new_hi > new_lo * 1.02 {
                    edit.freq = Some((new_lo * new_hi).sqrt());
                    edit.bw_mult = Some((2.0 * (new_hi / new_lo).log2()).clamp(0.0, 9.95));
                }
            }
        }
        if resp.drag_stopped() {
            edit.ended = true;
        }
        let color = if resp.hovered() { theme.col_500() } else { theme.col_900() };
        p.circle_filled(Pos2::new(hr.center().x, track.center().y), 6.0, color);
    }
    edit
}

pub struct GainEdit {
    pub db: Option<f32>,
    pub began: bool,
    pub ended: bool,
}

pub fn gain_slider(
    ui: &mut Ui,
    rect: Rect,
    db: f32,
    header: &str,
    theme: &Theme,
    id: Id,
) -> GainEdit {
    let mut edit = GainEdit {
        db: None,
        began: false,
        ended: false,
    };
    let p = ui.painter_at(rect);

    p.text(
        Pos2::new(rect.center().x, rect.top()),
        Align2::CENTER_TOP,
        header,
        fonts::semibold(10.0),
        theme.col_700(),
    );

    let track = Rect::from_min_max(
        Pos2::new(rect.center().x - 5.0, rect.top() + 22.0),
        Pos2::new(rect.center().x + 5.0, rect.bottom() - 22.0),
    );
    p.rect_filled(track, 2, theme.warm_gray(226));

    let norm = ((db + 96.0) / 108.0).clamp(0.0, 1.0);
    let y = track.bottom() - norm * track.height();
    p.rect_filled(
        Rect::from_min_max(Pos2::new(track.left(), y), Pos2::new(track.right(), track.bottom())),
        2,
        theme.col_500(),
    );
    p.rect_filled(
        Rect::from_center_size(Pos2::new(track.center().x, y), Vec2::new(14.0, 5.0)),
        1,
        theme.col_900(),
    );
    p.text(
        Pos2::new(rect.center().x, rect.bottom() - 16.0),
        Align2::CENTER_BOTTOM,
        format!("{} dB", fmt_db(db)),
        fonts::semibold(10.0),
        theme.col_900(),
    );

    let resp = ui.interact(rect, id, Sense::click_and_drag());
    if resp.hovered() || resp.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeVertical);
    }
    if resp.drag_started() {
        edit.began = true;
    }
    if resp.dragged() {
        if let Some(pos) = ui.ctx().pointer_interact_pos() {
            let n = ((track.bottom() - pos.y) / track.height()).clamp(0.0, 1.0);
            edit.db = Some(-96.0 + n * 108.0);
        }
    }
    if resp.drag_stopped() {
        edit.ended = true;
    }
    if resp.double_clicked() {
        edit.db = Some(0.0);
        edit.began = true;
        edit.ended = true;
    }
    edit
}

pub struct BarEdit {
    pub value: Option<f64>,
    pub began: bool,
    pub ended: bool,
}

/// 水平数值条（MIX / MORPH / RANGE / SPEED 共用）。
pub fn hbar(
    ui: &mut Ui,
    rect: Rect,
    label: &str,
    value_text: &str,
    norm: f64,
    theme: &Theme,
    id: Id,
) -> BarEdit {
    let mut edit = BarEdit {
        value: None,
        began: false,
        ended: false,
    };
    let p = ui.painter_at(rect);

    p.text(
        Pos2::new(rect.left(), rect.top()),
        Align2::LEFT_TOP,
        label,
        fonts::semibold(10.0),
        theme.col_700(),
    );
    p.text(
        Pos2::new(rect.right(), rect.top()),
        Align2::RIGHT_TOP,
        value_text,
        fonts::semibold(10.0),
        theme.col_900(),
    );

    let track = Rect::from_min_max(
        Pos2::new(rect.left(), rect.bottom() - 8.0),
        Pos2::new(rect.right(), rect.bottom()),
    );
    p.rect_filled(track, 0, theme.warm_gray(226));
    let w = (norm.clamp(0.0, 1.0) as f32) * track.width();
    p.rect_filled(
        Rect::from_min_max(track.min, Pos2::new(track.left() + w, track.bottom())),
        0,
        theme.col_500(),
    );

    let resp = ui.interact(rect, id, Sense::click_and_drag());
    if resp.hovered() || resp.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    if resp.drag_started() {
        edit.began = true;
    }
    if resp.dragged() || resp.clicked() {
        if let Some(pos) = ui.ctx().pointer_interact_pos() {
            edit.value = Some(((pos.x - track.left()) / track.width()).clamp(0.0, 1.0) as f64);
        }
    }
    if resp.drag_stopped() {
        edit.ended = true;
    }
    edit
}

pub fn section_title(ui: &mut Ui, rect: Rect, text: &str, theme: &Theme) {
    let p = ui.painter_at(rect);
    p.text(
        Pos2::new(rect.left(), rect.center().y),
        Align2::LEFT_CENTER,
        text,
        fonts::bold(11.0),
        theme.col_900(),
    );
    p.line_segment(
        [
            Pos2::new(rect.left() + text.len() as f32 * 6.4 + 10.0, rect.center().y),
            Pos2::new(rect.right(), rect.center().y),
        ],
        theme.hairline(),
    );
}

pub fn flat_button(ui: &mut Ui, rect: Rect, label: &str, active: bool, theme: &Theme, id: Id) -> bool {
    let p = ui.painter_at(rect);
    let resp = ui.interact(rect, id, Sense::click());
    let bg = if active { theme.col_900() } else { theme.col_300() };
    p.rect_filled(rect.shrink(1.0), 2, bg);
    if resp.hovered() {
        p.rect_filled(rect.shrink(1.0), 2, theme.hover_overlay());
    }
    p.text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        fonts::semibold(10.0),
        if active { theme.col_100() } else { theme.col_900() },
    );
    resp.clicked()
}

pub fn preset_slot(
    ui: &mut Ui,
    rect: Rect,
    label: &str,
    active: bool,
    theme: &Theme,
    id: Id,
) -> bool {
    let p = ui.painter_at(rect);
    let resp = ui.interact(rect, id, Sense::click());
    p.rect_filled(rect.shrink(1.0), 1, if active { theme.col_900() } else { theme.col_300() });
    if resp.hovered() {
        p.rect_filled(rect.shrink(1.0), 1, theme.hover_overlay());
    }
    p.text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        fonts::semibold(9.0),
        if active { theme.col_100() } else { theme.col_900() },
    );
    resp.clicked()
}

/// 迷你频谱/电平：给以后接 AudioTap 的占位实现（当前画平线）。
pub fn placeholder_wave(p: &egui::Painter, rect: Rect, color: Color32) {
    let mut pts = Vec::new();
    for i in 0..=64 {
        let t = i as f32 / 64.0;
        let x = rect.left() + t * rect.width();
        let y = rect.center().y + (t * PI * 6.0).sin() * rect.height() * 0.15;
        pts.push(Pos2::new(x, y));
    }
    p.add(Shape::line(pts, Stroke::new(1.0_f32, color)));
}
