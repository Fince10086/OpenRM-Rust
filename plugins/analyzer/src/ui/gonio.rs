//! 时域矢量示波器（45° 正方形李萨如散点云）。
//!
//! 移植自原版 `controls/GoniometerControl.h`（368 行）：1024 点/hop 累进密度缓冲、
//! 6 档分色、persist 余辉（挂 hop）、1×/2×/4× 缩放、L1 越界缩回、底部复合条。
//! 相关度/平衡与复合条复用 `field.rs` 的 `StereoCorr` / `draw_stereo_corr_bar`。

use egui::{Align2, Color32, Pos2, Rect, Shape, Stroke};
use openrm_ui::{fonts, strings::Lang, theme::Theme};

use super::data::ScopeFrame;
use super::field::{self, StereoCorr, STEREO_BELOW_H};
use super::meter;

// ===== GoniometerControl.h:98-104 密度缓冲常量 =====
const K_LEVELS: usize = 6;
const K_DENSITY_SAT: u16 = 4096;
const K_HIT_GAIN: u16 = 512;
const K_HOP: usize = 1024;
const K_DOT_FLOOR_AMP: f32 = 1e-4;

// ===== GoniometerControl.h:241 虚线勾档 =====
const K_DASH_ON: f32 = 5.0;
const K_DASH_OFF: f32 = 5.0;

/// 余辉档位（原版 EPersist:38）。
pub mod persist {
    pub const OFF: i32 = 0;
    pub const SHORT: i32 = 1;
    pub const LONG: i32 = 2;
    pub const NUM: i32 = 3;
}

/// 缩放档（原版 EZoom:39）。
pub mod zoom {
    pub const Z1: i32 = 0;
    pub const Z2: i32 = 1;
    pub const Z4: i32 = 2;
    pub const NUM: i32 = 3;
}

/// 控件内命中动作（双击复位缩放与持久度，原版 OnMouseDblClick:79-83）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GonioAction {
    ResetView,
}

/// 矢量示波 retained 控件。
pub struct Goniometer {
    pub rect: Rect,

    sample_rate: f64,
    persist: i32,
    zoom_idx: i32,

    corr: StereoCorr,
    density: Vec<u16>,
    buf_n: usize,
    buckets: [Vec<usize>; K_LEVELS],
}

impl Default for Goniometer {
    fn default() -> Self {
        Self::new(Rect::from_min_max(
            Pos2::new(20.0, 366.0),
            Pos2::new(394.0, 599.0),
        ))
    }
}

impl Goniometer {
    pub fn new(rect: Rect) -> Self {
        let mut me = Self {
            rect,
            sample_rate: 48_000.0,
            persist: persist::SHORT, // 原版默认 SHORT（:351）
            zoom_idx: zoom::Z1,      // 原版默认 1×（:352）
            corr: StereoCorr::new(),
            density: Vec::new(),
            buf_n: 0,
            buckets: [Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new()],
        };
        me.rebuild_buffer();
        me
    }

    // ==================== setter（原版 kMsgTag* / Set*） ====================

    pub fn set_sample_rate(&mut self, sr: f64) {
        self.sample_rate = sr.max(1000.0);
    }

    /// 原版 `SetPersist`（:48-51）。
    pub fn set_persist(&mut self, v: i32) {
        self.persist = v.clamp(0, persist::NUM - 1);
    }

    /// 原版 `SetZoomIdx`（:53-56）。
    pub fn set_zoom_idx(&mut self, v: i32) {
        self.zoom_idx = v.clamp(0, zoom::NUM - 1);
        self.rebuild_buffer();
    }

    /// 原版 `ClearPeakHold` / kMsgTagReset → `ResetDensity`（:44, :126-130）。
    pub fn clear_peak_hold(&mut self) {
        self.density.fill(0);
        self.corr.reset();
    }

    /// 原版 `ZoomFactor`（:57-59）。
    pub fn zoom_factor(&self) -> f32 {
        match self.zoom_idx {
            zoom::Z2 => 2.0,
            zoom::Z4 => 4.0,
            _ => 1.0,
        }
    }

    pub fn persist(&self) -> i32 {
        self.persist
    }

    pub fn corr(&self) -> &StereoCorr {
        &self.corr
    }

    // ==================== 几何（原版 PlotGeom:108-116） ====================

    fn bar_half_width(&self) -> f32 {
        (self.rect.width() * 0.5 - 2.0).max(12.0)
    }

    fn plot_geom(&self) -> (f32, f32, f32) {
        let half = self.bar_half_width();
        let bar_top = self.rect.top()
            + (self.rect.bottom() - STEREO_BELOW_H - self.rect.top()).min(half);
        let cx = self.rect.center().x;
        let cy = self.rect.top() + (bar_top - self.rect.top()) * 0.5;
        let r = ((bar_top - self.rect.top()) * 0.5 - 4.0).min(half).max(12.0);
        (cx, cy, r)
    }

    /// 原版 `InitBuffer`（:119-124）：1 格 = 1px，覆盖菱形外接方框。
    fn rebuild_buffer(&mut self) {
        let (_, _, r) = self.plot_geom();
        self.buf_n = (2.0 * r).ceil() as usize + 1;
        self.density = vec![0; self.buf_n * self.buf_n];
    }

    // ==================== 数据管线（原版 ProcessPacket:132-174） ====================

    pub fn update(&mut self, frame: &ScopeFrame) {
        let hop_sec = K_HOP as f64 / self.sample_rate.max(1.0);
        self.corr.update(&frame.l, &frame.r, K_HOP.min(frame.l.len()), hop_sec);

        // 余辉挂在 hop 上（不挂帧）：SHORT 右移 2，LONG 右移 4，OFF 整窗清空。
        let shift = match self.persist {
            persist::LONG => 4,
            persist::SHORT => 2,
            _ => -1,
        };
        if shift < 0 {
            self.density.fill(0);
        } else {
            for v in self.density.iter_mut() {
                *v = v.saturating_sub(*v >> shift);
            }
        }

        if self.buf_n < 2 {
            return;
        }

        let z = self.zoom_factor();
        let nm1 = (self.buf_n - 1) as f32;
        let n = K_HOP.min(frame.l.len()).min(frame.r.len());
        for i in 0..n {
            let l = frame.l[i];
            let r = frame.r[i];
            if l.abs().max(r.abs()) < K_DOT_FLOOR_AMP {
                continue;
            }
            let mut nx = 0.5 * (r - l) * z;
            let mut ny = 0.5 * (l + r) * z;
            if !nx.is_finite() || !ny.is_finite() {
                continue;
            }
            // 放大后越界：沿原点方向按 L1 距离等比缩回菱形边界（逐轴 clamp 会拉歪方向）。
            let l1 = nx.abs() + ny.abs();
            if l1 > 1.0 {
                let s = 1.0 / l1;
                nx *= s;
                ny *= s;
            }
            let col = ((nx * 0.5 + 0.5) * nm1).round() as usize;
            let row = ((0.5 - ny * 0.5) * nm1).round() as usize;
            let col = col.min(self.buf_n - 1);
            let row = row.min(self.buf_n - 1);
            let idx = row * self.buf_n + col;
            let next = self.density[idx].saturating_add(K_HIT_GAIN);
            self.density[idx] = next.min(K_DENSITY_SAT);
        }
    }

    // ==================== 命中 ====================

    pub fn click(&mut self, _pos: Pos2) -> Option<GonioAction> {
        None
    }

    /// 双击复位缩放与持久度（原版 OnMouseDblClick:79-83）。
    pub fn dbl_click(&mut self, _pos: Pos2) -> Option<GonioAction> {
        self.zoom_idx = zoom::Z1;
        self.persist = persist::SHORT;
        self.rebuild_buffer();
        Some(GonioAction::ResetView)
    }

    pub fn hover(&mut self, _pos: Pos2) -> Option<String> {
        None
    }

    // ==================== 绘制 ====================

    pub fn draw(&mut self, painter: &egui::Painter, theme: &Theme, lang: Lang) {
        painter.rect_filled(self.rect, 0.0, theme.col_100());
        let (cx, cy, r) = self.plot_geom();

        self.draw_grid_content(painter, theme, cx, cy, r);
        self.draw_dots(painter, theme, cx, cy, r);

        let half = self.bar_half_width();
        let bar_top = self.rect.top()
            + (self.rect.bottom() - STEREO_BELOW_H - self.rect.top()).min(half);
        field::draw_stereo_corr_bar(painter, theme, cx, half, bar_top, &self.corr, lang);
    }

    /// 网格内容（原版 DrawGridContent:192-237）。
    fn draw_grid_content(&self, painter: &egui::Painter, theme: &Theme, cx: f32, cy: f32, r: f32) {
        let h = r * 0.5;

        // 菱形内部浅块（单通道峰值满幅边界）
        let diamond = [
            Pos2::new(cx, cy - r),
            Pos2::new(cx + r, cy),
            Pos2::new(cx, cy + r),
            Pos2::new(cx - r, cy),
        ];
        painter.add(Shape::convex_polygon(diamond.to_vec(), meter::grid_gray(theme, 206), Stroke::NONE));

        // 十字轴：水平 = 反相轴（略强），垂直 = 中置轴
        painter.line_segment(
            [Pos2::new(cx - r, cy), Pos2::new(cx + r, cy)],
            Stroke::new(1.0_f32, theme.col_500()),
        );
        painter.line_segment(
            [Pos2::new(cx, cy - r), Pos2::new(cx, cy + r)],
            Stroke::new(1.0_f32, theme.col_300()),
        );

        // 内虚线菱形（半满幅 ≈ −6 dBFS）
        draw_dashed_diamond(painter, cx, cy, h, theme.col_300());

        // 两条过中心 45° 线 = L=0 / R=0 纯单边信号线
        painter.line_segment(
            [Pos2::new(cx - h, cy - h), Pos2::new(cx + h, cy + h)],
            Stroke::new(1.0_f32, theme.col_300()),
        );
        painter.line_segment(
            [Pos2::new(cx + h, cy - h), Pos2::new(cx - h, cy + h)],
            Stroke::new(1.0_f32, theme.col_300()),
        );

        // 外菱形描边
        painter.add(Shape::closed_line(diamond.to_vec(), Stroke::new(1.0_f32, theme.col_500())));

        self.draw_scale_content(painter, theme, cx, cy, r);
    }

    /// 右侧刻度 + L/R 硬声像点标注（原版 DrawScaleContent:274-297）。
    fn draw_scale_content(&self, painter: &egui::Painter, theme: &Theme, cx: f32, cy: f32, r: f32) {
        let edge = 1.0 / self.zoom_factor();
        let vals = [edge, edge * 0.5, 0.0, -edge * 0.5, -edge];
        let font = fonts::regular(13.0);
        let x_l = cx + r + 6.0;
        for v in vals {
            let buf = fmt_scale(v);
            let y = cy - v * r;
            painter.text(
                Pos2::new(x_l, y),
                Align2::LEFT_CENTER,
                buf,
                font.clone(),
                theme.col_700(),
            );
        }

        let h = r * 0.5;
        let off = 12.0;
        let (c_l, c_r, _) = theme.channel_colors();
        let font_lr = fonts::semibold(15.0);
        painter.text(
            Pos2::new(cx - h - off, cy - h - off),
            Align2::CENTER_CENTER,
            "L",
            font_lr.clone(),
            c_l,
        );
        painter.text(
            Pos2::new(cx + h + off, cy - h - off),
            Align2::CENTER_CENTER,
            "R",
            font_lr,
            c_r,
        );
    }

    /// 散点云：密度缓冲分档批量填充（原版 DrawDots:309-348）。
    fn draw_dots(&mut self, painter: &egui::Painter, theme: &Theme, cx: f32, cy: f32, r: f32) {
        if self.buf_n < 2 || self.density.len() < self.buf_n * self.buf_n {
            return;
        }
        for b in self.buckets.iter_mut() {
            b.clear();
        }

        let cells = self.buf_n * self.buf_n;
        let mut any = false;
        for (i, &d) in self.density.iter().enumerate().take(cells) {
            if d == 0 {
                continue;
            }
            let mut lv = (d as usize * K_LEVELS) / K_DENSITY_SAT as usize;
            if lv >= K_LEVELS {
                lv = K_LEVELS - 1;
            }
            self.buckets[lv].push(i);
            any = true;
        }
        if !any {
            return;
        }

        let (_, _, base0) = theme.channel_colors();
        let base = field::stereo_sem_color(theme, base0);

        let left = cx - r;
        let top = cy - r;
        let pitch = 2.0 * r / (self.buf_n - 1) as f32;
        let s = pitch.max(1.0) * 0.5;

        for (lv, bucket) in self.buckets.iter().enumerate() {
            if bucket.is_empty() {
                continue;
            }
            let col = dot_level_color(theme, base, lv);
            for &idx in bucket {
                let px = left + (idx % self.buf_n) as f32 * pitch;
                let py = top + (idx / self.buf_n) as f32 * pitch;
                painter.rect_filled(
                    Rect::from_min_max(Pos2::new(px - s, py - s), Pos2::new(px + s, py + s)),
                    0.0,
                    col,
                );
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 模块级工具
// ---------------------------------------------------------------------------

/// 原版 `DrawDashedDiamond`（:240-261）：手工按 5px 通 / 5px 断打断四条边。
fn draw_dashed_diamond(painter: &egui::Painter, cx: f32, cy: f32, r: f32, c: Color32) {
    if r <= 2.0 {
        return;
    }
    let xs = [cx, cx + r, cx, cx - r];
    let ys = [cy - r, cy, cy + r, cy];
    let stroke = Stroke::new(1.0_f32, c);
    for e in 0..4 {
        let n = (e + 1) & 3;
        let dx = xs[n] - xs[e];
        let dy = ys[n] - ys[e];
        let len = (dx * dx + dy * dy).sqrt();
        if len <= 1e-3 {
            continue;
        }
        let ux = dx / len;
        let uy = dy / len;
        let mut t = 0.0_f32;
        while t < len {
            let t1 = (t + K_DASH_ON).min(len);
            painter.line_segment(
                [
                    Pos2::new(xs[e] + ux * t, ys[e] + uy * t),
                    Pos2::new(xs[e] + ux * t1, ys[e] + uy * t1),
                ],
                stroke,
            );
            t += K_DASH_ON + K_DASH_OFF;
        }
    }
}

/// 原版 `DotLevelColor`（:300-307）：深色向白提亮，浅色向黑压深。
fn dot_level_color(theme: &Theme, base: Color32, lv: usize) -> Color32 {
    let t = (lv + 1) as f32 / K_LEVELS as f32;
    let target = if matches!(theme.mode, openrm_ui::theme::ThemeMode::Dark) {
        255
    } else {
        20
    };
    let mix = |ch: u8| -> u8 {
        (ch as f32 + (target as f32 - ch as f32) * 0.55 * t).round() as u8
    };
    let a = (50.0 + 205.0 * t).round() as u8;
    Color32::from_rgba_unmultiplied(mix(base.r()), mix(base.g()), mix(base.b()), a)
}

/// 原版 `FmtScale`（:265-272）。
fn fmt_scale(v: f32) -> String {
    if v.abs() < 1e-4 {
        "0".to_string()
    } else if (v - v.round()).abs() < 5e-3 {
        format!("{:+}", v.round() as i32)
    } else {
        format!("{:+}", v)
    }
}
