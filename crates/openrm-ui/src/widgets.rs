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

pub fn fmt_hz_unit(f: f64) -> String {
    format!("{:.0}Hz", f.max(0.0))
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

/// 滤波垫：X = 频率（对数），Y = 带宽倍率（对数）。几何量逐项取自 C++ `FilterNodePad`。
pub struct PadModel<'a> {
    pub center_prefix: &'a str,
    pub width_prefix: &'a str,
    pub slope_prefix: &'a str,
    pub pass_label: &'a str,
    pub side_label: &'a str,
    pub pass: bool,
    pub freq: f64,
    pub bw_mult: f64,
    pub slope_db: i32,
    /// 随机映射开关（中心/带宽两个色块）
    pub swatch_on: [bool; 2],
    pub swatch_color: [Color32; 2],
    /// 频谱曲线：`(x 0..1, y 0..1)`，已按 3 dB/oct 与 256 段处理
    pub spec_in: &'a [(f32, f32)],
    pub spec_out: &'a [(f32, f32)],
}

#[derive(Default)]
pub struct PadEdit {
    pub freq: Option<f64>,
    pub bw_mult: Option<f64>,
    pub toggle_pass: bool,
    pub slope: Option<i32>,
    pub color_menu: Option<usize>,
    pub began: bool,
    pub ended: bool,
}

const PAD_CORNER_W: f32 = 170.0;
const PAD_CORNER_H: f32 = 22.0;
const PAD_TOP_PAD: f32 = 30.0;
const PAD_SIDE_W: f32 = 24.0;
const PAD_SIDE_X: f32 = 4.0;
const PAD_SWATCH_GAP: f32 = 6.0;
const PAD_SWATCH: f32 = 14.0;
const LABEL_VALUE_GAP: f32 = 12.0;
const PAD_PASS_GAP: f32 = 8.0;
const PAD_PASS_PAD: f32 = 6.0;
const HANDLE_R: f32 = 9.0;

fn text_size(ui: &Ui, text: &str, font: egui::FontId) -> Vec2 {
    ui.painter()
        .layout_no_wrap(text.to_owned(), font, Color32::WHITE)
        .size()
}

fn draw_rotated(
    painter: &egui::Painter,
    center: Pos2,
    text: &str,
    font: egui::FontId,
    color: Color32,
    angle: f32,
) {
    let galley = painter.layout_no_wrap(text.to_owned(), font, color);
    let size = galley.size();
    // epaint 绕 `pos` 旋转；-90° 时文本盒占 [pos.x, pos.x+h] × [pos.y-w, pos.y]
    let pos = Pos2::new(center.x - size.y * 0.5, center.y + size.x * 0.5);
    let mut shape = egui::epaint::TextShape::new(pos, galley, color);
    shape.angle = angle;
    painter.add(shape);
}

/// 谱面渐变填充：逐列四边形 + 顶点色，等价于原版纵向线性渐变。
fn fill_spectrum(
    painter: &egui::Painter,
    plot: Rect,
    pts: &[(f32, f32)],
    color: Color32,
    top_alpha: u8,
) {
    if pts.len() < 2 {
        return;
    }
    let mut mesh = egui::Mesh::default();
    let base = plot.bottom() + 1.0;
    let to_pos = |p: (f32, f32)| {
        Pos2::new(
            plot.left() + p.0.clamp(0.0, 1.0) * plot.width(),
            plot.bottom() - p.1.clamp(0.0, 1.0) * plot.height(),
        )
    };

    // 先做 Catmull-Rom → 三次贝塞尔采样，逼近原版 s=0.6 的曲线
    let mut curve: Vec<Pos2> = Vec::with_capacity(pts.len() * 4);
    if pts.len() > 3 {
        let s = 0.6f32;
        for i in 0..pts.len() - 1 {
            let p0 = to_pos(pts[i.saturating_sub(1)]);
            let p1 = to_pos(pts[i]);
            let p2 = to_pos(pts[i + 1]);
            let p3 = to_pos(pts[(i + 2).min(pts.len() - 1)]);
            let c1 = p1 + (p2 - p0) * (s / 6.0);
            let c2 = p2 - (p3 - p1) * (s / 6.0);
            curve.push(p1);
            for k in 1..4 {
                let t = k as f32 / 4.0;
                let inv = 1.0 - t;
                let w0 = inv * inv * inv;
                let w1 = 3.0 * inv * inv * t;
                let w2 = 3.0 * inv * t * t;
                let w3 = t * t * t;
                let p = Pos2::new(
                    p1.x * w0 + c1.x * w1 + c2.x * w2 + p2.x * w3,
                    p1.y * w0 + c1.y * w1 + c2.y * w2 + p2.y * w3,
                );
                curve.push(p);
            }
        }
    } else {
        curve.extend(pts.iter().map(|&p| to_pos(p)));
    }
    curve.first_mut().map(|p| p.x = plot.left());
    curve.last_mut().map(|p| p.x = plot.right());

    let alpha_at = |y: f32| {
        let t = ((y - plot.top()) / plot.height().max(1.0)).clamp(0.0, 1.0);
        (top_alpha as f32 * (1.0 - t)) as u8
    };
    for w in curve.windows(2) {
        let (a, b) = (w[0], w[1]);
        if (b.x - a.x).abs() < 1e-4 {
            continue;
        }
        let ca = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha_at(a.y));
        let cb = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha_at(b.y));
        let ia = mesh.vertices.len() as u32;
        mesh.vertices.push(egui::epaint::Vertex {
            pos: a,
            uv: egui::epaint::WHITE_UV,
            color: ca,
        });
        mesh.vertices.push(egui::epaint::Vertex {
            pos: Pos2::new(a.x, base),
            uv: egui::epaint::WHITE_UV,
            color: Color32::TRANSPARENT,
        });
        mesh.vertices.push(egui::epaint::Vertex {
            pos: b,
            uv: egui::epaint::WHITE_UV,
            color: cb,
        });
        mesh.vertices.push(egui::epaint::Vertex {
            pos: Pos2::new(b.x, base),
            uv: egui::epaint::WHITE_UV,
            color: Color32::TRANSPARENT,
        });
        mesh.indices
            .extend_from_slice(&[ia, ia + 1, ia + 2, ia + 1, ia + 3, ia + 2]);
    }
    painter.add(Shape::mesh(mesh));
}

pub fn filter_pad(ui: &mut Ui, rect: Rect, model: &PadModel, theme: &Theme, id: Id) -> PadEdit {
    let mut edit = PadEdit::default();
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 0, theme.col_100());

    let plot = Rect::from_min_max(
        Pos2::new(rect.left(), rect.top() + PAD_TOP_PAD),
        Pos2::new(rect.right(), rect.bottom()),
    );

    // zebra 底纹：每个十倍频程内按 1..9 细分，灰阶在同一段内线性过渡
    let bands: [(f64, f64, f32, f32); 4] = [
        (20.0, 100.0, 194.0, 218.0),
        (100.0, 1000.0, 205.0, 229.0),
        (1000.0, 10_000.0, 216.0, 240.0),
        (10_000.0, 20_000.0, 227.0, 227.0),
    ];
    for (lo, hi, v0, v1) in bands {
        let mut edges = vec![lo];
        let mut decade = 1.0f64;
        while decade <= 10_000.0 {
            for m in 1..=9 {
                let f = m as f64 * decade;
                if f > lo && f < hi {
                    edges.push(f);
                }
            }
            decade *= 10.0;
        }
        edges.push(hi);
        let xs: Vec<f32> = edges.iter().map(|&f| freq_to_x(f, plot)).collect();
        let cells = xs.len().saturating_sub(1);
        for i in 0..cells {
            let t = if cells > 1 { i as f32 / (cells - 1) as f32 } else { 0.0 };
            let v = (v0 + (v1 - v0) * t).round() as u8;
            p.rect_filled(
                Rect::from_min_max(
                    Pos2::new(xs[i], plot.top()),
                    Pos2::new(xs[i + 1], plot.bottom()),
                ),
                0,
                theme.warm_gray(v),
            );
        }
    }

    // 频谱（位于网格之下，与原版叠加顺序一致）
    if !model.spec_in.is_empty() {
        fill_spectrum(&p, plot, model.spec_in, theme.col_500(), 110);
    }
    if !model.spec_out.is_empty() {
        fill_spectrum(&p, plot, model.spec_out, theme.col_900(), 170);
    }

    // 网格：8 度线（整 kHz 加深）+ 横向带宽线
    for f in [100.0f64, 1000.0, 10_000.0] {
        let x = freq_to_x(f, plot);
        p.line_segment(
            [Pos2::new(x, plot.top()), Pos2::new(x, plot.bottom())],
            Stroke::new(1.0_f32, theme.warm_gray(215)),
        );
    }
    for bw in [2.0f64, 4.0, 8.0, 16.0, 31.0] {
        let y = bw_to_y(bw, plot);
        p.line_segment(
            [Pos2::new(plot.left(), y), Pos2::new(plot.right(), y)],
            Stroke::new(1.0_f32, theme.warm_gray(228)),
        );
    }

    // 轴标签：只保留原版的三处
    for (f, label) in [(100.0, "100"), (1000.0, "1k"), (10_000.0, "10k")] {
        p.text(
            Pos2::new(freq_to_x(f, plot), plot.bottom() - 2.0),
            Align2::CENTER_BOTTOM,
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

    // 侧标：40px Bold，绕中心旋转 -90°
    if !model.side_label.is_empty() {
        let side = Rect::from_min_max(
            Pos2::new(rect.left() + PAD_SIDE_X, plot.top()),
            Pos2::new(rect.left() + PAD_SIDE_W + PAD_SIDE_X, rect.bottom()),
        );
        let _ = &side;
        let painter_full = ui.painter();
        let galley = painter_full.layout_no_wrap(
            model.side_label.to_owned(),
            fonts::bold(32.0),
            theme.col_700(),
        );
        let h = galley.size();
        let mut shape = egui::epaint::TextShape::new(
            Pos2::new(rect.left() + 3.0, side.center().y + h.x * 0.5),
            galley,
            theme.col_700(),
        );
        shape.angle = -PI * 0.5;
        painter_full.add(shape);
    }

    // 顶部角标：CENTER / WIDTH（20px SemiBold，色块 14×14）
    let cy = rect.top() + PAD_CORNER_H * 0.5;
    let corner_l = Rect::from_min_max(
        Pos2::new(rect.left(), rect.top()),
        Pos2::new(rect.left() + PAD_CORNER_W, rect.top() + PAD_CORNER_H),
    );
    let corner_r = Rect::from_min_max(
        Pos2::new(rect.right() - PAD_CORNER_W, rect.top()),
        Pos2::new(rect.right(), rect.top() + PAD_CORNER_H),
    );
    let font = fonts::semibold(20.0);
    let center_value = format!("{:.0}Hz", model.freq);
    let bw_value = format!("{:.2}", model.bw_mult);

    // 左角：前缀 + 值 + 色块
    let center_swatch = {
        let prefix_w = text_size(ui, model.center_prefix, font.clone()).x;
        let value_w = text_size(ui, &center_value, font.clone()).x;
        let value_rect = Rect::from_min_max(
            Pos2::new(corner_l.left() + prefix_w + LABEL_VALUE_GAP, corner_l.top()),
            Pos2::new(
                corner_l.left() + prefix_w + LABEL_VALUE_GAP + value_w,
                corner_l.top() + PAD_CORNER_H,
            ),
        );
        p.text(
            Pos2::new(corner_l.left(), cy),
            Align2::LEFT_CENTER,
            model.center_prefix,
            font.clone(),
            theme.col_900(),
        );
        p.text(
            Pos2::new(value_rect.left(), cy),
            Align2::LEFT_CENTER,
            &center_value,
            font.clone(),
            theme.col_900(),
        );
        let sw = Rect::from_min_size(
            Pos2::new(value_rect.right() + PAD_SWATCH_GAP, cy - PAD_SWATCH * 0.5),
            Vec2::splat(PAD_SWATCH),
        );
        let c = if model.swatch_on[0] {
            model.swatch_color[0]
        } else {
            Theme::dim_color(model.swatch_color[0])
        };
        p.rect_filled(sw, 0, c);
        (sw, value_rect)
    };

    // 右角：色块右对齐，值左贴色块
    let bw_swatch = {
        let value_w = text_size(ui, &bw_value, font.clone()).x;
        let sw = Rect::from_min_size(
            Pos2::new(corner_r.right() - PAD_SWATCH, cy - PAD_SWATCH * 0.5),
            Vec2::splat(PAD_SWATCH),
        );
        let value_rect = Rect::from_min_max(
            Pos2::new(sw.left() - PAD_SWATCH_GAP - value_w, corner_r.top()),
            Pos2::new(sw.left() - PAD_SWATCH_GAP, corner_r.top() + PAD_CORNER_H),
        );
        let c = if model.swatch_on[1] {
            model.swatch_color[1]
        } else {
            Theme::dim_color(model.swatch_color[1])
        };
        p.rect_filled(sw, 0, c);
        let prefix_right = value_rect.left() - LABEL_VALUE_GAP;
        let prefix_w = text_size(ui, model.width_prefix, font.clone()).x;
        p.text(
            Pos2::new(prefix_right - prefix_w, cy),
            Align2::LEFT_CENTER,
            model.width_prefix,
            font.clone(),
            theme.col_900(),
        );
        p.text(
            Pos2::new(value_rect.left(), cy),
            Align2::LEFT_CENTER,
            &bw_value,
            font.clone(),
            theme.col_900(),
        );
        (sw, value_rect)
    };

    // 色块交互：左键切换随机、右键选色
    for (i, (sw, _)) in [center_swatch, bw_swatch].iter().enumerate() {
        let resp = ui.interact(*sw, id.with(("swatch", i)), Sense::click());
        if resp.clicked() || resp.secondary_clicked() {
            edit.color_menu = Some(i);
        }
    }

    // BP / BR 胶囊：宽度 = 文本宽 + 12，右缘贴 WIDTH 前缀左缘 - 8
    {
        let label_w = text_size(ui, model.pass_label, font.clone()).x;
        let btn_w = label_w + PAD_PASS_PAD * 2.0;
        let prefix_right = bw_swatch.1.left() - LABEL_VALUE_GAP;
        let prefix_w = text_size(ui, model.width_prefix, font.clone()).x;
        let btn = Rect::from_min_max(
            Pos2::new(prefix_right - prefix_w - PAD_PASS_GAP - btn_w, rect.top()),
            Pos2::new(prefix_right - prefix_w - PAD_PASS_GAP, rect.top() + PAD_CORNER_H),
        );
        let resp = ui.interact(btn, id.with("pass"), Sense::click());
        let hover = resp.hovered();
        let fill = if model.pass {
            if hover { theme.col_500() } else { theme.col_300() }
        } else {
            theme.col_900()
        };
        p.rect_filled(btn.shrink(1.0), 0, fill);
        p.text(
            btn.center(),
            Align2::CENTER_CENTER,
            model.pass_label,
            font.clone(),
            if model.pass { theme.col_900() } else { theme.col_100() },
        );
        if resp.clicked() {
            edit.toggle_pass = true;
        }
    }

    // SLOPE：居中，前缀 + 值
    {
        let slope_value = format!("{} dB/oct", model.slope_db);
        let w1 = text_size(ui, model.slope_prefix, font.clone()).x;
        let w2 = text_size(ui, &slope_value, font.clone()).x;
        let total = w1 + LABEL_VALUE_GAP + w2;
        let x0 = rect.center().x - total * 0.5;
        let r = Rect::from_min_max(
            Pos2::new(x0, rect.top()),
            Pos2::new(x0 + total, rect.top() + PAD_CORNER_H),
        );
        p.text(
            Pos2::new(x0, cy),
            Align2::LEFT_CENTER,
            model.slope_prefix,
            font.clone(),
            theme.col_900(),
        );
        p.text(
            Pos2::new(x0 + w1 + LABEL_VALUE_GAP, cy),
            Align2::LEFT_CENTER,
            &slope_value,
            font.clone(),
            theme.col_900(),
        );
        let resp = ui.interact(r, id.with("slope"), Sense::click());
        if resp.clicked() {
            edit.slope = Some(model.slope_db);
        }
    }

    // 手柄
    let handle = Pos2::new(freq_to_x(model.freq, plot), bw_to_y(model.bw_mult, plot));
    let hit = Rect::from_center_size(handle, Vec2::splat(HANDLE_R * 2.0 + 8.0));
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

    p.circle_filled(handle, HANDLE_R, theme.col_900());
    p.circle_filled(handle, HANDLE_R * 0.25, theme.col_100());

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
        format!("低切  {}", fmt_hz_unit(lo)),
        fonts::regular(13.0),
        theme.col_700(),
    );
    p.text(
        Pos2::new(rect.right() - 8.0, rect.top() + 2.0),
        Align2::RIGHT_TOP,
        format!("高切  {}", fmt_hz_unit(hi)),
        fonts::regular(13.0),
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
