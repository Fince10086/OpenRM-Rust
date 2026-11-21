//! 基础控件。全部是绝对定位 + `Painter` 自绘，坐标与原 C++ 版 916×646
//! 画布一一对应，方便逐屏对照。

use egui::{Align2, Color32, CursorIcon, Id, Pos2, Rect, Sense, Shape, Stroke, Ui, Vec2};
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
    /// 随机目标幽灵：(频率倍率, 带宽倍率, 调色板索引)
    pub ghost: Option<(f64, f64, usize)>,
    /// 单声道显示态：右声道整体压暗
    pub mono: bool,
    pub tip: Option<&'a str>,
}

#[derive(Default)]
pub struct PadEdit {
    pub freq: Option<f64>,
    pub bw_mult: Option<f64>,
    /// 点击 CENTER / WIDTH 数值 → 打开文本输入
    pub edit_center: bool,
    pub edit_bw: bool,
    pub toggle_pass: bool,
    pub slope: Option<i32>,
    pub color_menu: Option<usize>,
    pub toggle_random: Option<usize>,
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
const PAD_HANDLE_R: f32 = 9.0;

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
            let t = if cells > 1 {
                i as f32 / (cells - 1) as f32
            } else {
                0.0
            };
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
            fonts::bold(40.0),
            theme.col_500(),
        );
        let h = galley.size();
        let mut shape = egui::epaint::TextShape::new(
            Pos2::new(side.center().x - h.y * 0.5, side.center().y + h.x * 0.5),
            galley,
            theme.col_500(),
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
        let vresp = ui.interact(value_rect, id.with("center_value"), Sense::click());
        if vresp.clicked() {
            edit.edit_center = true;
        }
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
        let vresp = ui.interact(value_rect, id.with("bw_value"), Sense::click());
        if vresp.clicked() {
            edit.edit_bw = true;
        }
        (sw, value_rect)
    };

    // 色块交互：左键切换随机、右键选色
    for (i, (sw, _)) in [center_swatch, bw_swatch].iter().enumerate() {
        let resp = ui.interact(*sw, id.with(("swatch", i)), Sense::click());
        if resp.secondary_clicked() {
            edit.color_menu = Some(i);
        } else if resp.clicked() {
            edit.toggle_random = Some(i);
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
            Pos2::new(
                prefix_right - prefix_w - PAD_PASS_GAP,
                rect.top() + PAD_CORNER_H,
            ),
        );
        let resp = ui.interact(btn, id.with("pass"), Sense::click());
        let hover = resp.hovered();
        let fill = if model.pass {
            if hover {
                theme.col_500()
            } else {
                theme.col_300()
            }
        } else {
            theme.col_900()
        };
        p.rect_filled(btn.shrink(1.0), 0, fill);
        p.text(
            btn.center(),
            Align2::CENTER_CENTER,
            model.pass_label,
            font.clone(),
            if model.pass {
                theme.col_900()
            } else {
                theme.col_100()
            },
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
    let hit = Rect::from_center_size(handle, Vec2::splat(PAD_HANDLE_R * 2.0 + 8.0));
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

    if let Some((fm, bm, ci)) = model.ghost {
        let g = Pos2::new(
            freq_to_x(model.freq * fm, plot),
            bw_to_y((model.bw_mult * bm).clamp(BW_MIN, BW_MAX), plot),
        );
        p.circle_filled(g, 4.5, theme.col_100());
        p.circle_filled(g, 3.5, Theme::random_color(ci));
    }
    p.circle_filled(handle, PAD_HANDLE_R, theme.col_900());
    p.circle_filled(handle, PAD_HANDLE_R * 0.25, theme.col_100());

    if model.mono {
        let c = theme.col_100();
        p.rect_filled(
            rect,
            0,
            Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), 170),
        );
    }

    edit
}

// =====================================================================================
// 控件层：几何、字号、颜色全部取自 C++ 版
//   ORMSlider.h / GainSlider.h / BandRangeSlider.h / FlatButton.h / PresetSlotControl.h
//   PresetFadeSlider.h / SectionTitleControl.h
// 统一约定：文字 20px（半粗/常规按原版），旋钮 = 白环 r=8.5 + 深色核心 r=7。
// =====================================================================================

const TEXT_SIZE: f32 = 20.0;
const HANDLE_R: f32 = 7.0;
const HANDLE_RING: f32 = 1.5_f32;
const HEADER_H: f32 = 26.0;

fn knob(p: &egui::Painter, c: Pos2, theme: &Theme) {
    p.circle_filled(c, HANDLE_R + HANDLE_RING, theme.col_100());
    p.circle_filled(c, HANDLE_R, theme.col_900());
}

fn label_value(ui: &Ui, p: &egui::Painter, text: &str, font: egui::FontId, color: Color32) -> f32 {
    ui.painter()
        .layout_no_wrap(text.to_owned(), font, color)
        .size()
        .x
}

/// 水平轨道：COL_300 底 + COL_500 填充，高度 4（原版 mTrackSize）。
fn htrack(p: &egui::Painter, rect: Rect, x0: f32, x1: f32, theme: &Theme) {
    let cy = rect.center().y;
    p.rect_filled(
        Rect::from_min_max(Pos2::new(x0, cy - 2.0), Pos2::new(x1, cy + 2.0)),
        2,
        theme.col_300(),
    );
}

/// 竖直轨道（增益条）。
fn vtrack(p: &egui::Painter, rect: Rect, y0: f32, y1: f32, theme: &Theme) {
    let cx = rect.center().x;
    p.rect_filled(
        Rect::from_min_max(Pos2::new(cx - 2.0, y0), Pos2::new(cx + 2.0, y1)),
        2,
        theme.col_300(),
    );
}

pub struct BandModel<'a> {
    pub freq: f64,
    pub bw_mult: f64,
    pub low_label: &'a str,
    pub high_label: &'a str,
    pub tip: Option<&'a str>,
}

#[derive(Default)]
pub struct BandEdit {
    pub freq: Option<f64>,
    pub bw_mult: Option<f64>,
    pub low_hz: Option<f64>,
    pub high_hz: Option<f64>,
    /// 点击低切 / 高切数值 → 打开文本输入
    pub edit_low: bool,
    pub edit_high: bool,
    pub began: bool,
    pub ended: bool,
}

pub fn band_slider(ui: &mut Ui, rect: Rect, m: &BandModel, theme: &Theme, id: Id) -> BandEdit {
    let mut edit = BandEdit::default();
    let p = ui.painter_at(rect);
    let font = fonts::semibold(TEXT_SIZE);

    let low_hz = (m.freq / m.bw_mult).max(20.0);
    let high_hz = (m.freq * m.bw_mult).min(20500.0);
    let low_text = format!("{:.0}Hz", low_hz);
    let high_text = format!("{:.0}Hz", high_hz);

    let cy = rect.top() + HEADER_H * 0.5;
    let lw = label_value(ui, &p, m.low_label, font.clone(), theme.col_900());
    p.text(
        Pos2::new(rect.left(), cy),
        Align2::LEFT_CENTER,
        m.low_label,
        font.clone(),
        theme.col_900(),
    );
    let low_value_left = rect.left() + lw + LABEL_VALUE_GAP;
    p.text(
        Pos2::new(low_value_left, cy),
        Align2::LEFT_CENTER,
        &low_text,
        font.clone(),
        theme.col_900(),
    );
    let lvw = text_size(ui, &low_text, font.clone());
    let lresp = ui.interact(
        Rect::from_min_size(
            Pos2::new(low_value_left, rect.top()),
            Vec2::new(lvw.x, HEADER_H),
        ),
        id.with("low_value"),
        Sense::click(),
    );
    if lresp.clicked() {
        edit.edit_low = true;
    }
    let hw = label_value(ui, &p, &high_text, font.clone(), theme.col_900());
    let high_value_left = rect.right() - hw;
    p.text(
        Pos2::new(high_value_left, cy),
        Align2::LEFT_CENTER,
        &high_text,
        font.clone(),
        theme.col_900(),
    );
    let hresp = ui.interact(
        Rect::from_min_size(
            Pos2::new(high_value_left, rect.top()),
            Vec2::new(hw, HEADER_H),
        ),
        id.with("high_value"),
        Sense::click(),
    );
    if hresp.clicked() {
        edit.edit_high = true;
    }
    let hp = label_value(ui, &p, m.high_label, font.clone(), theme.col_900());
    p.text(
        Pos2::new(high_value_left - LABEL_VALUE_GAP - hp, cy),
        Align2::LEFT_CENTER,
        m.high_label,
        font.clone(),
        theme.col_900(),
    );

    // 轨道：左右各内缩 handle ring，纵向居中于 header 之后的区间
    let track = Rect::from_min_max(
        Pos2::new(rect.left() + HANDLE_R + HANDLE_RING, rect.top() + HEADER_H),
        Pos2::new(rect.right() - HANDLE_R - HANDLE_RING, rect.bottom()),
    );
    let low_norm = (low_hz.max(20.0) / 20.0).ln() / (20000f64 / 20.0).ln();
    let high_norm = (high_hz.max(20.0) / 20.0).ln() / (20000f64 / 20.0).ln();
    let lx = track.left() + low_norm as f32 * track.width();
    let hx = track.left() + high_norm as f32 * track.width();

    htrack(&p, track, rect.left(), rect.right(), theme);
    p.rect_filled(
        Rect::from_min_max(
            Pos2::new(lx, track.center().y - 2.0),
            Pos2::new(hx, track.center().y + 2.0),
        ),
        0,
        theme.col_500(),
    );
    knob(&p, Pos2::new(lx, track.center().y), theme);
    knob(&p, Pos2::new(hx, track.center().y), theme);

    let resp = ui.interact(rect, id, Sense::click_and_drag());
    let resp = if let Some(t) = m.tip {
        resp.on_hover_text(t)
    } else {
        resp
    };
    if resp.hovered() || resp.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeHorizontal);
    }

    // 命中：中点 ±7 内视为整体平移，其余就近选一个手柄
    let mut active: Option<u8> = None;
    if resp.drag_started() || resp.clicked() {
        if let Some(pos) = ui.ctx().pointer_interact_pos() {
            if pos.x > lx + 7.0 && pos.x < hx - 7.0 {
                active = Some(2);
            } else if (pos.x - lx).abs() <= (pos.x - hx).abs() {
                active = Some(0);
            } else {
                active = Some(1);
            }
        }
        edit.began = true;
    }
    let drag = resp.dragged() || resp.clicked();
    if drag {
        if let Some(pos) = ui.ctx().pointer_interact_pos() {
            let n = ((pos.x - track.left()) / track.width()).clamp(0.0, 1.0) as f64;
            let hz = 20.0 * 1000f64.powf(n);
            // 沿用原版：0=低切 1=高切 2=整体
            let active = active.unwrap_or(1);
            match active {
                0 => {
                    edit.low_hz = Some(hz.min(high_hz - 1.0));
                }
                1 => {
                    edit.high_hz = Some(hz.max(low_hz + 1.0));
                }
                _ => {
                    // 整体拖动需要按下时的起点，这里用当前指针与起点差值在 ui 外部处理：
                    // 用指针相对中心的方式平移，保持带宽不变
                    if let Some(press) = ui.input(|i| i.pointer.press_origin()) {
                        let start_n = ((press.x - track.left()) / track.width()) as f64;
                        let cur = n;
                        let mult = (cur - start_n) as f32;
                        edit.freq = Some(m.freq * 1000f64.powf(mult as f64 * 0.5));
                    }
                }
            }
        }
    }
    if resp.drag_stopped() {
        edit.ended = true;
    }
    edit
}

pub struct GainModel<'a> {
    pub db: f64,
    pub label: &'a str,
    pub random_on: bool,
    pub random_color: usize,
    pub delta_db: f32,
    pub ghost: bool,
    pub tip: Option<&'a str>,
}

#[derive(Default)]
pub struct GainEdit {
    pub db: Option<f64>,
    pub began: bool,
    pub ended: bool,
    pub toggle_random: bool,
    pub color_menu: bool,
    pub edit_value: bool,
}

pub fn gain_slider(ui: &mut Ui, rect: Rect, m: &GainModel, theme: &Theme, id: Id) -> GainEdit {
    let mut edit = GainEdit::default();
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 0, theme.col_100());

    // 文本框：右侧 16 + 6 间隙；增益轨在剩余 36 宽内
    let text_col = Rect::from_min_max(
        Pos2::new(rect.right() - 16.0 - 6.0, rect.top()),
        Pos2::new(rect.right() - 6.0, rect.bottom()),
    );
    let track_col = Rect::from_min_max(
        Pos2::new(rect.left(), rect.top()),
        Pos2::new(rect.right() - 16.0 - 6.0, rect.bottom()),
    );
    // 原版：手柄行程从 T+38 到 B-8；浅色轨道再向两端各扩 8（T+30..B），
    // 深色填充从手柄一直画到轨道底（B）。
    let top = rect.top() + 30.0;
    let track_y0 = top + 8.0;
    let track_y1 = rect.bottom() - 8.0;
    let cx = track_col.center().x;
    let font = fonts::semibold(TEXT_SIZE);
    let value_font = fonts::regular(TEXT_SIZE);

    let norm = ((m.db + 96.0) / 108.0).clamp(0.0, 1.0) as f32;
    let y = track_y1 - norm * (track_y1 - track_y0);

    vtrack(&p, track_col, top, rect.bottom(), theme);
    p.rect_filled(
        Rect::from_min_max(Pos2::new(cx - 2.0, y), Pos2::new(cx + 2.0, rect.bottom())),
        2,
        theme.col_500(),
    );

    // 随机目标幽灵：白环 + 半透明色芯
    let ghost_norm = ((m.db + m.delta_db as f64 + 96.0) / 108.0).clamp(0.0, 1.0) as f32;
    let gy = track_y1 - ghost_norm * (track_y1 - track_y0);
    if m.random_on {
        p.circle_filled(Pos2::new(cx, gy), HANDLE_R + HANDLE_RING, theme.col_100());
        p.circle_filled(
            Pos2::new(cx, gy),
            HANDLE_R,
            Color32::from_rgba_unmultiplied(
                Theme::random_color(m.random_color).r(),
                Theme::random_color(m.random_color).g(),
                Theme::random_color(m.random_color).b(),
                150,
            ),
        );
    }
    knob(&p, Pos2::new(cx, y), theme);

    // 旋转文字：值（+90，顶端）+ 标签（-90，底端）
    let value_text = format!("{:.1}dB", m.db);
    let galley = p.layout_no_wrap(value_text, value_font, theme.col_700());
    let vs = galley.size();
    // +90° 时字形框位于原点左上：x∈[origin.x-h, origin.x]，y∈[origin.y, origin.y+w]
    let mut shape = egui::epaint::TextShape::new(
        Pos2::new(text_col.center().x + vs.y * 0.5, top),
        galley,
        theme.col_700(),
    );
    shape.angle = PI * 0.5;
    p.add(shape);

    let galley = p.layout_no_wrap(m.label.to_owned(), font, theme.col_900());
    let ls = galley.size();
    let mut shape = egui::epaint::TextShape::new(
        Pos2::new(text_col.center().x + ls.y * 0.5, rect.bottom() - ls.x),
        galley,
        theme.col_900(),
    );
    shape.angle = PI * 0.5;
    p.add(shape);

    // 随机色块：位于标签上方
    let title_h = ls.x;
    let swatch_cy = rect.bottom() - title_h - 8.0 - 7.0;
    let sw = Rect::from_center_size(Pos2::new(text_col.center().x, swatch_cy), Vec2::splat(14.0));
    let c = Theme::random_color(m.random_color);
    p.rect_filled(sw, 0, if m.random_on { c } else { Theme::dim_color(c) });
    let sw_resp = ui.interact(sw, id.with("random"), Sense::click());
    if sw_resp.clicked() {
        edit.toggle_random = true;
    }
    if sw_resp.secondary_clicked() {
        edit.color_menu = true;
    }

    // 数值区点击 → 文本输入
    let value_rect = Rect::from_min_max(
        Pos2::new(text_col.left() - 4.0, top),
        Pos2::new(text_col.right(), text_col.center().y),
    );
    let v_resp = ui.interact(value_rect, id.with("value"), Sense::click());
    if v_resp.clicked() {
        edit.edit_value = true;
    }

    let resp = ui.interact(rect, id, Sense::click_and_drag());
    let resp = if let Some(t) = m.tip {
        resp.on_hover_text(t)
    } else {
        resp
    };
    if resp.hovered() || resp.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::ResizeVertical);
    }
    if resp.drag_started() {
        edit.began = true;
    }
    if resp.dragged() || resp.clicked() {
        if let Some(pos) = ui.ctx().pointer_interact_pos() {
            let n = ((track_y1 - pos.y) / (track_y1 - track_y0)).clamp(0.0, 1.0);
            edit.db = Some((n as f64 * 108.0 - 96.0).clamp(-96.0, 12.0));
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
    if m.ghost {
        let base = theme.col_100();
        p.rect_filled(
            rect,
            0,
            Color32::from_rgba_unmultiplied(base.r(), base.g(), base.b(), 150),
        );
    }
    edit
}

pub struct BarModel<'a> {
    pub label: &'a str,
    pub value_text: &'a str,
    pub norm: f64,
    /// 标题左侧色块（RANGE/SPEED 用当前色组）
    pub swatch_left: Option<usize>,
    /// 标题右侧随机色块：(颜色, 开关)
    pub random: Option<(usize, bool)>,
    pub label_regular: bool,
    pub tip: Option<&'a str>,
}

#[derive(Default)]
pub struct BarEdit {
    pub value: Option<f64>,
    pub began: bool,
    pub ended: bool,
    pub toggle_random: bool,
    pub color_menu: bool,
    pub edit_value: bool,
}

pub fn hbar(ui: &mut Ui, rect: Rect, m: &BarModel, theme: &Theme, id: Id) -> BarEdit {
    let mut edit = BarEdit::default();
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 0, theme.col_100());
    let label_font = if m.label_regular {
        fonts::regular(TEXT_SIZE)
    } else {
        fonts::semibold(TEXT_SIZE)
    };
    let value_font = fonts::regular(TEXT_SIZE);
    let cy = rect.top() + HEADER_H * 0.5;

    let mut label_l = rect.left();
    if let Some(c) = m.swatch_left {
        let sw = Rect::from_min_size(Pos2::new(rect.left() + 1.0, cy - 7.0), Vec2::splat(14.0));
        p.rect_filled(sw, 0, Theme::random_color(c));
        label_l = sw.right() + 8.0;
    }
    let mut value_r = rect.right();
    if let Some((c, on)) = m.random {
        let sw = Rect::from_min_max(
            Pos2::new(rect.right() - 1.0 - 14.0, cy - 7.0),
            Pos2::new(rect.right() - 1.0, cy + 7.0),
        );
        let col = Theme::random_color(c);
        p.rect_filled(sw, 0, if on { col } else { Theme::dim_color(col) });
        value_r = sw.left() - 6.0;

        let resp = ui.interact(sw, id.with("random"), Sense::click());
        if resp.clicked() {
            edit.toggle_random = true;
        }
        if resp.secondary_clicked() {
            edit.color_menu = true;
        }
    }
    p.text(
        Pos2::new(label_l, cy),
        Align2::LEFT_CENTER,
        m.label,
        label_font,
        theme.col_900(),
    );
    let vw = label_value(ui, &p, m.value_text, value_font.clone(), theme.col_700());
    p.text(
        Pos2::new(value_r - vw, cy),
        Align2::LEFT_CENTER,
        m.value_text,
        value_font,
        theme.col_700(),
    );

    // 原版 ORMSlider（IVSliderControl，mHandleSize=8）：
    // 手柄行程 = 控件左右各内缩 8；浅色轨道贯穿整个控件宽度；
    // 深色填充从控件左缘画到手柄（0 处仍保留 8px 的填充头）。
    let track = Rect::from_min_max(
        Pos2::new(rect.left() + 8.0, rect.top() + HEADER_H),
        Pos2::new(rect.right() - 8.0, rect.bottom()),
    );
    let norm = m.norm.clamp(0.0, 1.0) as f32;
    let x = track.left() + norm * track.width();
    htrack(&p, track, rect.left(), rect.right(), theme);
    p.rect_filled(
        Rect::from_min_max(
            Pos2::new(rect.left(), track.center().y - 2.0),
            Pos2::new(x, track.center().y + 2.0),
        ),
        2,
        theme.col_500(),
    );
    knob(&p, Pos2::new(x, track.center().y), theme);

    let resp = ui.interact(rect, id, Sense::click_and_drag());
    let resp = if let Some(t) = m.tip {
        resp.on_hover_text(t)
    } else {
        resp
    };
    if resp.hovered() || resp.dragged() {
        ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
    }
    if resp.drag_started() {
        edit.began = true;
    }
    if resp.clicked() || resp.dragged() {
        if let Some(pos) = ui.ctx().pointer_interact_pos() {
            edit.value = Some(((pos.x - track.left()) / track.width()).clamp(0.0, 1.0) as f64);
        }
    }
    if resp.drag_stopped() {
        edit.ended = true;
    }
    // 数值区单击进入文本输入
    let value_rect = Rect::from_min_max(
        Pos2::new(value_r - vw - 4.0, rect.top()),
        Pos2::new(value_r, rect.top() + HEADER_H),
    );
    let v_resp = ui.interact(value_rect, id.with("value"), Sense::click());
    if v_resp.clicked() {
        edit.edit_value = true;
    }
    edit
}

pub fn section_title(ui: &mut Ui, rect: Rect, text: &str, theme: &Theme) {
    let p = ui.painter_at(rect);
    let font = fonts::bold(TEXT_SIZE);
    p.text(
        Pos2::new(rect.left(), rect.center().y),
        Align2::LEFT_CENTER,
        text,
        font,
        theme.col_900(),
    );
}

pub fn flat_button(
    ui: &mut Ui,
    rect: Rect,
    label: &str,
    active: bool,
    theme: &Theme,
    id: Id,
    tip: Option<&str>,
) -> bool {
    let p = ui.painter_at(rect);
    let resp = ui.interact(rect, id, Sense::click());
    let resp = if let Some(t) = tip {
        resp.on_hover_text(t)
    } else {
        resp
    };
    let pressed = resp.is_pointer_button_down_on();
    p.rect_filled(
        rect,
        0,
        if pressed || active {
            theme.col_900()
        } else {
            theme.col_300()
        },
    );
    if !pressed && !active && resp.hovered() {
        p.rect_filled(rect, 0, theme.hover_overlay());
    }
    p.text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        fonts::semibold(TEXT_SIZE),
        if pressed || active {
            theme.col_100()
        } else {
            theme.col_900()
        },
    );
    resp.clicked()
}

pub fn toggle_button(
    ui: &mut Ui,
    rect: Rect,
    label: &str,
    on: bool,
    theme: &Theme,
    id: Id,
) -> bool {
    let p = ui.painter_at(rect);
    let resp = ui.interact(rect, id, Sense::click());
    p.rect_filled(rect, 0, if on { theme.col_900() } else { theme.col_300() });
    if !on && resp.hovered() {
        p.rect_filled(rect, 0, theme.hover_overlay());
    }
    p.text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        fonts::semibold(TEXT_SIZE),
        if on { theme.col_100() } else { theme.col_900() },
    );
    resp.clicked()
}

#[derive(Default)]
pub struct SlotResponse {
    pub clicked: bool,
    pub save_here: bool,
    pub secondary: bool,
    pub drag_started: bool,
    pub dragging: bool,
    pub drag_stopped: bool,
    pub hovered: bool,
}

pub fn preset_slot(
    ui: &mut Ui,
    rect: Rect,
    label: &str,
    drag_target: bool,
    theme: &Theme,
    id: Id,
    tip: Option<&str>,
) -> SlotResponse {
    let p = ui.painter_at(rect);
    let resp = ui.interact(rect, id, Sense::click_and_drag());
    let resp = if let Some(t) = tip {
        resp.on_hover_text(t)
    } else {
        resp
    };
    let pressed = resp.is_pointer_button_down_on();
    let mut out = SlotResponse {
        hovered: resp.hovered(),
        ..Default::default()
    };
    p.rect_filled(
        rect,
        0,
        if drag_target {
            theme.col_500()
        } else if pressed {
            theme.col_900()
        } else if resp.hovered() {
            theme.col_500()
        } else {
            theme.col_300()
        },
    );
    p.text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        fonts::semibold(TEXT_SIZE),
        if pressed {
            theme.col_100()
        } else {
            theme.col_900()
        },
    );
    if resp.drag_started() {
        out.drag_started = true;
    }
    if resp.dragged() {
        out.dragging = true;
    }
    if resp.drag_stopped() {
        out.drag_stopped = true;
    }
    if resp.clicked() {
        if ui.input(|i| i.modifiers.command) {
            out.save_here = true;
        } else {
            out.clicked = true;
        }
    }
    if resp.secondary_clicked() {
        out.secondary = true;
    }
    out
}

#[derive(Clone, Copy)]
pub enum Icon {
    Undo,
    Redo,
    Save,
    Load,
}

// 原版 assets/icons/*.svg 的路径命令（1024×1024 网格，圆弧已转贝塞尔）。
struct SvgCmd(u8, [f32; 6]);

const UNDO_PATH: &[SvgCmd] = &[
    SvgCmd(0, [245.863, 54.752, 0., 0., 0., 0.]),
    SvgCmd(1, [0., 300.615, 0., 0., 0., 0.]),
    SvgCmd(1, [245.863, 546.504, 0., 0., 0., 0.]),
    SvgCmd(1, [321.531, 470.836, 0., 0., 0., 0.]),
    SvgCmd(1, [204.725, 354.056, 0., 0., 0., 0.]),
    SvgCmd(1, [662.909, 354.056, 0., 0., 0., 0.]),
    SvgCmd(2, [803.245, 354.056, 917.010, 467.821, 917.010, 608.157]),
    SvgCmd(2, [917.010, 748.493, 803.245, 862.258, 662.909, 862.258]),
    SvgCmd(1, [328.566, 862.258, 0., 0., 0., 0.]),
    SvgCmd(1, [328.566, 969.248, 0., 0., 0., 0.]),
    SvgCmd(1, [662.909, 969.248, 0., 0., 0., 0.]),
    SvgCmd(2, [862.338, 969.248, 1024.0, 807.586, 1024.0, 608.157]),
    SvgCmd(2, [1024.0, 408.728, 862.338, 247.066, 662.909, 247.066]),
    SvgCmd(1, [204.859, 247.066, 0., 0., 0., 0.]),
    SvgCmd(1, [321.505, 130.394, 0., 0., 0., 0.]),
    SvgCmd(1, [245.863, 54.752, 0., 0., 0., 0.]),
];

const REDO_PATH: &[SvgCmd] = &[
    SvgCmd(0, [778.137, 54.752, 0., 0., 0., 0.]),
    SvgCmd(1, [1024.0, 300.615, 0., 0., 0., 0.]),
    SvgCmd(1, [778.137, 546.504, 0., 0., 0., 0.]),
    SvgCmd(1, [702.469, 470.836, 0., 0., 0., 0.]),
    SvgCmd(1, [819.275, 354.056, 0., 0., 0., 0.]),
    SvgCmd(1, [361.091, 354.056, 0., 0., 0., 0.]),
    SvgCmd(2, [220.755, 354.056, 106.990, 467.821, 106.990, 608.157]),
    SvgCmd(2, [106.990, 748.493, 220.755, 862.258, 361.091, 862.258]),
    SvgCmd(1, [695.434, 862.258, 0., 0., 0., 0.]),
    SvgCmd(1, [695.434, 969.248, 0., 0., 0., 0.]),
    SvgCmd(1, [361.091, 969.248, 0., 0., 0., 0.]),
    SvgCmd(2, [161.662, 969.248, 0., 807.586, 0., 608.157]),
    SvgCmd(2, [0., 408.728, 161.662, 247.066, 361.091, 247.066]),
    SvgCmd(1, [819.141, 247.066, 0., 0., 0., 0.]),
    SvgCmd(1, [702.495, 130.394, 0., 0., 0., 0.]),
    SvgCmd(1, [778.137, 54.752, 0., 0., 0., 0.]),
];

const IMPORT_PATH: &[SvgCmd] = &[
    SvgCmd(0, [849.750, 419.711, 0., 0., 0., 0.]),
    SvgCmd(1, [588.934, 419.711, 0., 0., 0., 0.]),
    SvgCmd(1, [549.228, 419.711, 0., 0., 0., 0.]),
    SvgCmd(1, [549.228, 96.167, 0., 0., 0., 0.]),
    SvgCmd(1, [174.250, 96.167, 0., 0., 0., 0.]),
    SvgCmd(1, [174.250, 907.904, 0., 0., 0., 0.]),
    SvgCmd(1, [463.552, 907.904, 0., 0., 0., 0.]),
    SvgCmd(1, [473.533, 907.904, 0., 0., 0., 0.]),
    SvgCmd(1, [473.533, 994.527, 0., 0., 0., 0.]),
    SvgCmd(1, [473.533, 1004.144, 0., 0., 0., 0.]),
    SvgCmd(1, [114.583, 1004.144, 0., 0., 0., 0.]),
    SvgCmd(1, [74.877, 1004.144, 0., 0., 0., 0.]),
    SvgCmd(1, [74.877, 38.467, 0., 0., 0., 0.]),
    SvgCmd(1, [74.877, 0., 0., 0., 0., 0.]),
    SvgCmd(1, [629.805, 0., 0., 0., 0., 0.]),
    SvgCmd(1, [638.687, 0., 0., 0., 0., 0.]),
    SvgCmd(1, [944.023, 327.623, 0., 0., 0., 0.]),
    SvgCmd(1, [949.123, 333.095, 0., 0., 0., 0.]),
    SvgCmd(1, [949.123, 567.604, 0., 0., 0., 0.]),
    SvgCmd(1, [949.123, 577.293, 0., 0., 0., 0.]),
    SvgCmd(1, [859.731, 577.293, 0., 0., 0., 0.]),
    SvgCmd(1, [849.750, 577.293, 0., 0., 0., 0.]),
    SvgCmd(1, [849.750, 419.711, 0., 0., 0., 0.]),
    SvgCmd(3, [0., 0., 0., 0., 0., 0.]),
    SvgCmd(0, [820.463, 333.160, 0., 0., 0., 0.]),
    SvgCmd(1, [638.693, 138.204, 0., 0., 0., 0.]),
    SvgCmd(1, [638.693, 333.160, 0., 0., 0., 0.]),
    SvgCmd(1, [820.463, 333., 0., 0., 0., 0.]),
    SvgCmd(3, [0., 0., 0., 0., 0., 0.]),
    SvgCmd(0, [675.921, 856.250, 0., 0., 0., 0.]),
    SvgCmd(1, [939.215, 856.250, 0., 0., 0., 0.]),
    SvgCmd(1, [949.123, 856.250, 0., 0., 0., 0.]),
    SvgCmd(1, [949.123, 769.700, 0., 0., 0., 0.]),
    SvgCmd(1, [949.123, 760.083, 0., 0., 0., 0.]),
    SvgCmd(1, [673.735, 760.083, 0., 0., 0., 0.]),
    SvgCmd(1, [771.797, 665.082, 0., 0., 0., 0.]),
    SvgCmd(1, [778.828, 658.271, 0., 0., 0., 0.]),
    SvgCmd(1, [715.626, 597.037, 0., 0., 0., 0.]),
    SvgCmd(1, [708.596, 590.226, 0., 0., 0., 0.]),
    SvgCmd(1, [512.801, 779.900, 0., 0., 0., 0.]),
    SvgCmd(1, [484.712, 807.111, 0., 0., 0., 0.]),
    SvgCmd(1, [701.566, 1017.184, 0., 0., 0., 0.]),
    SvgCmd(1, [708.601, 1024.0, 0., 0., 0., 0.]),
    SvgCmd(1, [771.797, 962.690, 0., 0., 0., 0.]),
    SvgCmd(1, [778.785, 955.910, 0., 0., 0., 0.]),
    SvgCmd(1, [675.921, 856.250, 0., 0., 0., 0.]),
    SvgCmd(3, [0., 0., 0., 0., 0., 0.]),
];

const EXPORT_PATH: &[SvgCmd] = &[
    SvgCmd(0, [111.884, 623.884, 0., 0., 0., 0.]),
    SvgCmd(1, [111.884, 890.580, 0., 0., 0., 0.]),
    SvgCmd(1, [111.884, 917.293, 0., 0., 0., 0.]),
    SvgCmd(1, [885.403, 917.293, 0., 0., 0., 0.]),
    SvgCmd(1, [912.043, 917.293, 0., 0., 0., 0.]),
    SvgCmd(1, [912.043, 623.884, 0., 0., 0., 0.]),
    SvgCmd(1, [805.408, 623.884, 0., 0., 0., 0.]),
    SvgCmd(1, [778.695, 623.884, 0., 0., 0., 0.]),
    SvgCmd(1, [778.695, 543.890, 0., 0., 0., 0.]),
    SvgCmd(1, [778.695, 517.250, 0., 0., 0., 0.]),
    SvgCmd(1, [992.110, 517.250, 0., 0., 0., 0.]),
    SvgCmd(1, [1018.750, 517.250, 0., 0., 0., 0.]),
    SvgCmd(1, [1018.750, 997.287, 0., 0., 0., 0.]),
    SvgCmd(1, [1018.750, 1024.0, 0., 0., 0., 0.]),
    SvgCmd(1, [31.890, 1024.0, 0., 0., 0., 0.]),
    SvgCmd(1, [5.250, 1024.0, 0., 0., 0., 0.]),
    SvgCmd(1, [5.250, 543.890, 0., 0., 0., 0.]),
    SvgCmd(1, [5.250, 517.250, 0., 0., 0., 0.]),
    SvgCmd(1, [218.664, 517.250, 0., 0., 0., 0.]),
    SvgCmd(1, [245.305, 517.250, 0., 0., 0., 0.]),
    SvgCmd(1, [245.305, 597.244, 0., 0., 0., 0.]),
    SvgCmd(1, [245.305, 623.884, 0., 0., 0., 0.]),
    SvgCmd(1, [111.884, 623.884, 0., 0., 0., 0.]),
    SvgCmd(3, [0., 0., 0., 0., 0., 0.]),
    SvgCmd(0, [565.281, 202.703, 0., 0., 0., 0.]),
    SvgCmd(1, [565.281, 656.172, 0., 0., 0., 0.]),
    SvgCmd(1, [565.281, 682.812, 0., 0., 0., 0.]),
    SvgCmd(1, [485.359, 682.812, 0., 0., 0., 0.]),
    SvgCmd(1, [458.646, 682.812, 0., 0., 0., 0.]),
    SvgCmd(1, [458.646, 202.703, 0., 0., 0., 0.]),
    SvgCmd(1, [303.943, 357.406, 0., 0., 0., 0.]),
    SvgCmd(1, [285.320, 376.029, 0., 0., 0., 0.]),
    SvgCmd(1, [229.306, 320.124, 0., 0., 0., 0.]),
    SvgCmd(1, [210.613, 301.467, 0., 0., 0., 0.]),
    SvgCmd(1, [493.323, 18.680, 0., 0., 0., 0.]),
    SvgCmd(1, [511.997, 0., 0., 0., 0., 0.]),
    SvgCmd(1, [794.694, 282.697, 0., 0., 0., 0.]),
    SvgCmd(1, [813.390, 301.392, 0., 0., 0., 0.]),
    SvgCmd(1, [757.340, 357.334, 0., 0., 0., 0.]),
    SvgCmd(1, [738.689, 375.948, 0., 0., 0., 0.]),
    SvgCmd(1, [565.281, 202.703, 0., 0., 0., 0.]),
    SvgCmd(3, [0., 0., 0., 0., 0., 0.]),
];

fn draw_svg_path(p: &egui::Painter, rect: Rect, cmds: &[SvgCmd], color: Color32) {
    let cx = rect.center().x;
    let cy = rect.center().y;
    let s = 18.5 / 1024.0;
    let map = |x: f32, y: f32| Pos2::new(cx + (x - 512.0) * s, cy + (y - 512.0) * s);
    let mut subpaths: Vec<Vec<Pos2>> = Vec::new();
    let mut cur: Vec<Pos2> = Vec::new();
    let mut last = Pos2::ZERO;
    let mut i = 0;
    while i < cmds.len() {
        let e = &cmds[i];
        match e.0 {
            0 => {
                if cur.len() > 1 {
                    subpaths.push(cur.clone());
                }
                cur.clear();
                last = map(e.1[0], e.1[1]);
                cur.push(last);
            }
            1 => {
                last = map(e.1[0], e.1[1]);
                cur.push(last);
            }
            2 => {
                let c1 = map(e.1[0], e.1[1]);
                let c2 = map(e.1[2], e.1[3]);
                let to = map(e.1[4], e.1[5]);
                for k in 1..=14 {
                    let t = k as f32 / 14.0;
                    let inv = 1.0 - t;
                    let w0 = inv * inv * inv;
                    let w1 = 3.0 * inv * inv * t;
                    let w2 = 3.0 * inv * t * t;
                    let w3 = t * t * t;
                    cur.push(Pos2::new(
                        last.x * w0 + c1.x * w1 + c2.x * w2 + to.x * w3,
                        last.y * w0 + c1.y * w1 + c2.y * w2 + to.y * w3,
                    ));
                }
                last = to;
            }
            _ => {
                if cur.len() > 1 {
                    subpaths.push(cur.clone());
                }
                cur.clear();
            }
        }
        i += 1;
    }
    if cur.len() > 1 {
        subpaths.push(cur);
    }
    // 用扫描线做奇偶填充：SVG 图标的轮廓是凹多边形，直接三角扇会破形。
    let mut edges: Vec<(Pos2, Pos2)> = Vec::new();
    for sp in &subpaths {
        for i in 0..sp.len() {
            edges.push((sp[i], sp[(i + 1) % sp.len()]));
        }
    }
    let ymin = subpaths
        .iter()
        .flat_map(|sp| sp.iter().map(|q| q.y))
        .fold(f32::MAX, f32::min);
    let ymax = subpaths
        .iter()
        .flat_map(|sp| sp.iter().map(|q| q.y))
        .fold(f32::MIN, f32::max);
    if ymax <= ymin {
        return;
    }
    // 2x 超采样扫描线：细笔画的箭头只有 1–2px 厚，步长 0.5 会留下毛刺；
    // 这里用 0.2 步长、行高 0.28 的四边形拼一个 Mesh，一次提交。
    let mut mesh = egui::Mesh::default();
    let mut y = ymin;
    while y <= ymax {
        let mut xs: Vec<f32> = Vec::new();
        for (a, b) in &edges {
            if (a.y <= y && b.y > y) || (b.y <= y && a.y > y) {
                let t = (y - a.y) / (b.y - a.y);
                xs.push(a.x + (b.x - a.x) * t);
            }
        }
        xs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let mut i = 0;
        while i + 1 < xs.len() {
            if xs[i + 1] - xs[i] > 0.01 {
                let (x0, x1) = (xs[i], xs[i + 1]);
                let i0 = mesh.vertices.len() as u32;
                for p in [
                    Pos2::new(x0, y),
                    Pos2::new(x1, y),
                    Pos2::new(x1, y + 0.28),
                    Pos2::new(x0, y + 0.28),
                ] {
                    mesh.vertices.push(egui::epaint::Vertex {
                        pos: p,
                        uv: egui::epaint::WHITE_UV,
                        color,
                    });
                }
                mesh.indices
                    .extend_from_slice(&[i0, i0 + 1, i0 + 2, i0, i0 + 2, i0 + 3]);
            }
            i += 2;
        }
        y += 0.2;
    }
    p.add(Shape::mesh(mesh));
}

pub fn icon_button(
    ui: &mut Ui,
    rect: Rect,
    icon: Icon,
    theme: &Theme,
    id: Id,
    tip: Option<&str>,
) -> bool {
    let p = ui.painter_at(rect);
    let resp = ui.interact(rect, id, Sense::click());
    let resp = if let Some(t) = tip {
        resp.on_hover_text(t)
    } else {
        resp
    };
    p.rect_filled(rect, 0, theme.col_300());
    if resp.hovered() {
        p.rect_filled(rect, 0, theme.hover_overlay());
    }
    let path = match icon {
        Icon::Undo => UNDO_PATH,
        Icon::Redo => REDO_PATH,
        Icon::Save => EXPORT_PATH,
        Icon::Load => IMPORT_PATH,
    };
    draw_svg_path(&p, rect, path, theme.col_900());
    resp.clicked()
}

pub fn gear_button(ui: &mut Ui, rect: Rect, theme: &Theme, id: Id, tip: Option<&str>) -> bool {
    let p = ui.painter_at(rect);
    let resp = ui.interact(rect, id, Sense::click());
    let resp = if let Some(t) = tip {
        resp.on_hover_text(t)
    } else {
        resp
    };
    let c = if resp.hovered() {
        theme.col_900()
    } else {
        theme.col_700()
    };
    let cx = rect.center().x;
    let cy = rect.center().y;
    let r = (rect.width() * 0.5 - 2.0) * 0.8;
    p.circle_filled(Pos2::new(cx, cy), r * 0.72, c);
    // 齿：局部矩形 -0.17r..0.17r × -r..-0.70r，按 i*45° 旋转
    for i in 0..8 {
        let a = i as f32 * PI / 4.0;
        let q = [
            Pos2::new(-r * 0.17, -r),
            Pos2::new(r * 0.17, -r),
            Pos2::new(r * 0.17, -r * 0.70),
            Pos2::new(-r * 0.17, -r * 0.70),
        ];
        let rot: Vec<Pos2> = q
            .iter()
            .map(|v| Pos2::new(v.x * a.cos() - v.y * a.sin(), v.x * a.sin() + v.y * a.cos()))
            .map(|v| Pos2::new(v.x + cx, v.y + cy))
            .collect();
        p.add(Shape::convex_polygon(rot, c, Stroke::NONE));
    }
    p.circle_filled(Pos2::new(cx, cy), r * 0.30, theme.col_100());
    resp.clicked()
}

const FADE_TRACK_L: f32 = 31.5;
const FADE_TRACK_R: f32 = 656.5;

pub fn fade_slider(
    ui: &mut Ui,
    rect: Rect,
    norm: f64,
    ticks: usize,
    theme: &Theme,
    id: Id,
    tip: Option<&str>,
) -> BarEdit {
    let mut edit = BarEdit::default();
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 0, theme.col_100());
    // 原版 PresetFadeSlider（IVSliderControl，mHandleSize=8）：
    // 手柄行程与底部快选槽中心同为 39.5..648.5；深色填充从控件左缘开始。
    let track = Rect::from_min_max(
        Pos2::new(rect.left() + 8.0, rect.center().y - 2.0),
        Pos2::new(rect.right() - 8.0, rect.center().y + 2.0),
    );
    p.rect_filled(
        Rect::from_min_max(
            Pos2::new(rect.left(), track.top()),
            Pos2::new(rect.right(), track.bottom()),
        ),
        2,
        theme.col_300(),
    );
    let x = track.left() + norm.clamp(0.0, 1.0) as f32 * track.width();
    p.rect_filled(
        Rect::from_min_max(
            Pos2::new(rect.left(), track.top()),
            Pos2::new(x, track.bottom()),
        ),
        2,
        theme.col_500(),
    );
    // 刻度贯穿手柄行程（原版 mTrackBounds.L..R）
    for i in 0..ticks.max(2) {
        let tx = track.left() + track.width() * i as f32 / (ticks.max(2) - 1) as f32;
        p.rect_filled(
            Rect::from_min_max(
                Pos2::new(tx - 1.0, track.top() - 3.0),
                Pos2::new(tx + 1.0, track.bottom() + 3.0),
            ),
            0,
            theme.col_500(),
        );
    }
    knob(&p, Pos2::new(x, track.center().y), theme);

    let resp = ui.interact(rect, id, Sense::click_and_drag());
    let resp = if let Some(t) = tip {
        resp.on_hover_text(t)
    } else {
        resp
    };
    if resp.drag_started() {
        edit.began = true;
    }
    if resp.clicked() || resp.dragged() {
        if let Some(pos) = ui.ctx().pointer_interact_pos() {
            edit.value = Some(((pos.x - track.left()) / track.width()).clamp(0.0, 1.0) as f64);
        }
    }
    if resp.drag_stopped() {
        edit.ended = true;
    }
    edit
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
