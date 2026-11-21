//! BandPass 的 egui 界面：916×646 绝对坐标，与原 C++ 版布局对齐。
//!
//! 这一版是里程碑 2 的主体骨架：滤波器垫、带宽滑杆、增益条、预设槽、
//! 随机映射区、混音/主题控件全部到位并接参数；频谱显示与预设文件读写
//! 尚未接入（见 `docs/rust-port-plan.md` 的 M2 清单）。

use std::sync::Arc;

use egui::{Id, Pos2, Rect, Sense, Vec2};
use openrm_ui::{
    fonts,
    theme::Theme,
    widgets::{self, BandEdit, GainEdit, PadModel},
    UI_HEIGHT, UI_WIDTH,
};
use truce::prelude::*;
use truce_egui::EditorUi;

use crate::params::{BandPassParams, BandPassParamsParamId as P};
use openrm_dsp::spectrum::{PadSpectrum, SPECTRUM_BANDS};

const SPEC_FFT: usize = 4096;
const SPEC_OVERLAP: usize = 4;
const SPEC_HOP: usize = SPEC_FFT / SPEC_OVERLAP;

/// UI 侧的频谱管线：音频线程把 4 通道帧推进 `AudioTap`，这里按 hop 取走做 STFT。
struct SpectrumUi {
    sr: f64,
    l: PadSpectrum,
    r: PadSpectrum,
    pending: Vec<f32>,
    curves_l: [Vec<(f32, f32)>; 2],
    curves_r: [Vec<(f32, f32)>; 2],
}

impl SpectrumUi {
    fn new(sr: f64) -> Self {
        Self {
            sr,
            l: PadSpectrum::new(sr, SPEC_FFT, SPEC_OVERLAP),
            r: PadSpectrum::new(sr, SPEC_FFT, SPEC_OVERLAP),
            pending: Vec::with_capacity(SPEC_HOP * 8),
            curves_l: [Vec::new(), Vec::new()],
            curves_r: [Vec::new(), Vec::new()],
        }
    }

    fn pump(&mut self, tap: &truce::core::audio_tap::AudioTap<f32>) {
        let pending = &mut self.pending;
        tap.drain_with(|frames| {
            pending.extend_from_slice(frames);
        });
        while self.pending.len() >= SPEC_HOP * 4 {
            let chunk: Vec<f32> = self.pending.drain(..SPEC_HOP * 4).collect();
            let dry: Vec<f32> = chunk.chunks_exact(4).map(|f| f[0]).collect();
            let wet_l: Vec<f32> = chunk.chunks_exact(4).map(|f| f[2]).collect();
            let wet_r: Vec<f32> = chunk.chunks_exact(4).map(|f| f[3]).collect();
            self.l.push_hop(&dry, &wet_l);
            // 原版右垫的干信号取的是左声道（既有行为），此处保持复刻
            self.r.push_hop(&dry, &wet_r);
            for ch in 0..2 {
                self.curves_l[ch] = self.l.curve(ch);
                self.curves_r[ch] = self.r.curve(ch);
            }
            if std::env::var("ORM_SPEC_DEBUG").is_ok() {
                let c = &self.curves_l[0];
                let ymin = c.iter().map(|p| p.1).fold(f32::MAX, f32::min);
                let ymax = c.iter().map(|p| p.1).fold(f32::MIN, f32::max);
                eprintln!("spec hop: {} pts, y {}..{}", c.len(), ymin, ymax);
            }
        }
    }
}

#[derive(Clone, Copy)]
enum MenuKind {
    Slope(bool),
    Color(usize, bool),
}


const COL1_X: f32 = 740.0;
const COL2_X: f32 = 824.0;
const BTN_W: f32 = 72.0;
const PANEL_R: f32 = COL2_X + BTN_W; // 896
const GRID_Y: f32 = 56.0;
const CELL_W: f32 = 36.0;
const CELL_H: f32 = 29.0;
const CELL_STEP_X: f32 = 40.0;
const CELL_STEP_Y: f32 = 33.0;

fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
    Rect::from_min_max(Pos2::new(x0, y0), Pos2::new(x1, y1))
}

fn pid(id: P) -> u32 {
    id.into()
}

fn norm_freq(f: f64) -> f64 {
    (f.max(20.0) / 20.0).ln() / 1000f64.ln()
}

fn norm_bw_mult(m: f64) -> f64 {
    (m.max(1.0) / 1.0).ln() / 31f64.ln()
}

fn norm_gain(db: f64) -> f64 {
    ((db + 96.0) / 108.0).clamp(0.0, 1.0)
}

fn octaves_to_mult(bw_oct: f64) -> f64 {
    2f64.powf(bw_oct * 0.5)
}

struct Gesture {
    ids: Vec<u32>,
    active: bool,
}

impl Gesture {
    fn new() -> Self {
        Self {
            ids: Vec::new(),
            active: false,
        }
    }

    fn begin(&mut self, ctx: &PluginContext<BandPassParams>, id: u32) {
        if self.active {
            self.end(ctx);
        }
        ctx.bridge().begin_edit(id);
        self.ids.push(id);
        self.active = true;
    }

    fn also(&mut self, ctx: &PluginContext<BandPassParams>, id: u32) {
        if !self.ids.contains(&id) {
            ctx.bridge().begin_edit(id);
            self.ids.push(id);
        }
    }

    fn set(&self, ctx: &PluginContext<BandPassParams>, id: u32, v: f64) {
        ctx.bridge().set_param(id, v.clamp(0.0, 1.0));
    }

    fn end(&mut self, ctx: &PluginContext<BandPassParams>) {
        if !self.active {
            return;
        }
        for id in self.ids.drain(..) {
            ctx.bridge().end_edit(id);
        }
        self.active = false;
    }
}

pub struct BandPassUi {
    theme: Theme,
    fonts_ready: bool,
    gesture: Gesture,
    selected_color: usize,
    active_slot: Option<usize>,
    morph_norm: f64,
    spectrum: Option<SpectrumUi>,
    menu: Option<(MenuKind, Pos2)>,
}

impl Default for BandPassUi {
    fn default() -> Self {
        Self {
            theme: Theme::default(),
            fonts_ready: false,
            gesture: Gesture::new(),
            selected_color: 0,
            active_slot: None,
            morph_norm: 0.0,
            spectrum: None,
            menu: None,
        }
    }
}

impl BandPassUi {
    fn ensure_style(&mut self, ui: &mut egui::Ui) {
        if self.fonts_ready {
            return;
        }
        let ctx = ui.ctx();
        fonts::install(ctx);
        let mut visuals = egui::Visuals::light();
        visuals.panel_fill = self.theme.col_100();
        visuals.window_fill = self.theme.col_100();
        ctx.set_visuals(visuals);
        let bound = ctx.fonts(|f| {
            f.families()
                .iter()
                .any(|fam| matches!(fam, egui::FontFamily::Name(n) if n.as_ref() == openrm_ui::fonts::FAMILY_BOLD))
        });
        openrm_ui::fonts::set_named_ready(bound);
        self.fonts_ready = true;
    }

    fn apply_pad(&mut self, ctx: &PluginContext<BandPassParams>, is_l: bool, edit: widgets::PadEdit) {
        let (freq_id, bw_id, pass_id) = if is_l {
            (P::FreqL, P::BwL, P::PassL)
        } else {
            (P::FreqR, P::BwR, P::PassR)
        };
        let freq_id = pid(freq_id);
        let bw_id = pid(bw_id);

        if edit.began || edit.freq.is_some() || edit.bw_mult.is_some() {
            self.gesture.begin(ctx, freq_id);
            self.gesture.also(ctx, bw_id);
        }
        if let Some(f) = edit.freq {
            self.gesture.set(ctx, freq_id, norm_freq(f));
        }
        if let Some(m) = edit.bw_mult {
            self.gesture.set(ctx, bw_id, norm_bw_mult(m));
        }
        if edit.toggle_pass {
            let cur = if is_l {
                ctx.params().pass_l.value()
            } else {
                ctx.params().pass_r.value()
            };
            ctx.bridge()
                .set_param(pid(pass_id), if cur { 0.0 } else { 1.0 });
        }
        if edit.ended {
            self.gesture.end(ctx);
        }
    }

    fn apply_band(&mut self, ctx: &PluginContext<BandPassParams>, is_l: bool, edit: BandEdit) {
        let (freq_id, bw_id) = if is_l {
            (pid(P::FreqL), pid(P::BwL))
        } else {
            (pid(P::FreqR), pid(P::BwR))
        };
        if edit.began || edit.freq.is_some() {
            self.gesture.begin(ctx, freq_id);
            self.gesture.also(ctx, bw_id);
        }
        if let Some(f) = edit.freq {
            self.gesture.set(ctx, freq_id, norm_freq(f));
        }
        if let Some(oct) = edit.bw_mult {
            let m = octaves_to_mult(oct);
            self.gesture.set(ctx, bw_id, norm_bw_mult(m));
        }
        if edit.ended {
            self.gesture.end(ctx);
        }
    }

    fn apply_gain(&mut self, ctx: &PluginContext<BandPassParams>, is_l: bool, edit: GainEdit) {
        let id = pid(if is_l { P::GainL } else { P::GainR });
        if edit.began || edit.db.is_some() {
            self.gesture.begin(ctx, id);
        }
        if let Some(db) = edit.db {
            self.gesture.set(ctx, id, norm_gain(db as f64));
        }
        if edit.ended {
            self.gesture.end(ctx);
        }
    }

    fn swap_channels(ctx: &PluginContext<BandPassParams>, forward: bool) {
        let p = ctx.params();
        let (freq, bw, gain, slope, pass) = if forward {
            (
                p.freq_l.raw_target(),
                p.bw_l.raw_target(),
                p.gain_l.raw_target(),
                p.slope_l.index() as f64,
                p.pass_l.value(),
            )
        } else {
            (
                p.freq_r.raw_target(),
                p.bw_r.raw_target(),
                p.gain_r.raw_target(),
                p.slope_r.index() as f64,
                p.pass_r.value(),
            )
        };
        let to: [(P, f64); 5] = if forward {
            [
                (P::FreqR, norm_freq(freq)),
                (P::BwR, norm_bw_mult(bw)),
                (P::GainR, norm_gain(gain)),
                (P::SlopeR, slope / 3.0),
                (P::PassR, if pass { 1.0 } else { 0.0 }),
            ]
        } else {
            [
                (P::FreqL, norm_freq(freq)),
                (P::BwL, norm_bw_mult(bw)),
                (P::GainL, norm_gain(gain)),
                (P::SlopeL, slope / 3.0),
                (P::PassL, if pass { 1.0 } else { 0.0 }),
            ]
        };
        for (id, v) in to {
            ctx.bridge().set_param(pid(id), v);
        }
    }

    fn flip(ctx: &PluginContext<BandPassParams>) {
        let p = ctx.params();
        let l = [
            (P::FreqL, norm_freq(p.freq_r.raw_target())),
            (P::BwL, norm_bw_mult(p.bw_r.raw_target())),
            (P::GainL, norm_gain(p.gain_r.raw_target())),
            (P::SlopeL, p.slope_r.index() as f64 / 3.0),
            (P::PassL, if p.pass_r.value() { 1.0 } else { 0.0 }),
        ];
        let r = [
            (P::FreqR, norm_freq(p.freq_l.raw_target())),
            (P::BwR, norm_bw_mult(p.bw_l.raw_target())),
            (P::GainR, norm_gain(p.gain_l.raw_target())),
            (P::SlopeR, p.slope_l.index() as f64 / 3.0),
            (P::PassR, if p.pass_l.value() { 1.0 } else { 0.0 }),
        ];
        for (id, v) in l.into_iter().chain(r) {
            ctx.bridge().set_param(pid(id), v);
        }
    }

    fn color_params(color: usize) -> (P, P) {
        match color {
            0 => (P::RandomAmountR, P::RandomRateR),
            1 => (P::RandomAmountY, P::RandomRateY),
            2 => (P::RandomAmountB, P::RandomRateB),
            _ => (P::RandomAmountG, P::RandomRateG),
        }
    }
}

impl EditorUi<BandPassParams> for BandPassUi {
    fn ui(&mut self, ui: &mut egui::Ui, ctx: &PluginContext<BandPassParams>) {
        self.ensure_style(ui);

        // 频谱：采样率变了就重建分析器
        let sr = ctx.params().analysis.sample_rate();
        let need = self
            .spectrum
            .as_ref()
            .map_or(true, |sp| (sp.sr - sr).abs() > 0.5);
        if need {
            self.spectrum = Some(SpectrumUi::new(sr));
        }
        if let Some(sp) = self.spectrum.as_mut() {
            sp.pump(&ctx.params().analysis.tap);
        }

        let p = ctx.params();
        let theme = self.theme;

        let (full, _) = ui.allocate_exact_size(Vec2::new(UI_WIDTH, UI_HEIGHT), Sense::hover());
        let painter = ui.painter_at(full);

        painter.rect_filled(full, 0, theme.warm_gray(238));

        // ---- 左半：两个滤波垫 + 带宽滑杆 ----
        for (is_l, pad_rect, band_rect, gain_rect) in [
            (
                true,
                rect(20.0, 30.0, 668.0, 210.0),
                rect(20.0, 216.0, 668.0, 262.0),
                rect(672.0, 30.0, 730.0, 210.0),
            ),
            (
                false,
                rect(20.0, 288.0, 668.0, 468.0),
                rect(20.0, 474.0, 668.0, 520.0),
                rect(672.0, 288.0, 730.0, 468.0),
            ),
        ] {
            let (freq, bw, reject, pass) = if is_l {
                (
                    p.freq_l.raw_target(),
                    p.bw_l.raw_target(),
                    !p.pass_l.value(),
                    p.pass_l.value(),
                )
            } else {
                (
                    p.freq_r.raw_target(),
                    p.bw_r.raw_target(),
                    !p.pass_r.value(),
                    p.pass_r.value(),
                )
            };

            let slope_db = [12, 24, 48, 96][p.slope_l.index().min(3) as usize];
            let (swatch_a, swatch_b) = if is_l {
                (
                    p.random_color_freq_l.index(),
                    p.random_color_bw_l.index(),
                )
            } else {
                (
                    p.random_color_freq_r.index(),
                    p.random_color_bw_r.index(),
                )
            };
            let spec = self.spectrum.as_ref();
            let (sin, sout): (&[(f32, f32)], &[(f32, f32)]) = match (spec, is_l) {
                (Some(sp), true) => (&sp.curves_l[0], &sp.curves_l[1]),
                (Some(sp), false) => (&sp.curves_r[0], &sp.curves_r[1]),
                (None, _) => (&[], &[]),
            };
            let model = PadModel {
                center_prefix: "中心",
                width_prefix: "宽度",
                slope_prefix: "滚降",
                pass_label: if pass { "带通" } else { "带阻" },
                side_label: if is_l { "LEFT" } else { "RIGHT" },
                pass,
                freq,
                bw_mult: bw,
                slope_db,
                swatch_on: [true, true],
                swatch_color: [
                    openrm_ui::theme::Theme::random_color(swatch_a as usize),
                    openrm_ui::theme::Theme::random_color(swatch_b as usize),
                ],
                spec_in: sin,
                spec_out: sout,
            };
            let edit = widgets::filter_pad(
                ui,
                pad_rect,
                &model,
                &theme,
                Id::new(("pad", is_l)),
            );
            if let Some(_db) = edit.slope {
                self.menu = Some((
                    MenuKind::Slope(is_l),
                    ui.ctx()
                        .pointer_latest_pos()
                        .unwrap_or(pad_rect.left_top() + Vec2::new(200.0, 24.0)),
                ));
            }
            if let Some(i) = edit.color_menu {
                self.menu = Some((
                    MenuKind::Color(i, is_l),
                    ui.ctx()
                        .pointer_latest_pos()
                        .unwrap_or(pad_rect.left_top() + Vec2::new(100.0, 24.0)),
                ));
            }
            self.apply_pad(ctx, is_l, edit);

            let band = widgets::band_slider(
                ui,
                band_rect,
                freq,
                bw,
                &theme,
                Id::new(("band", is_l)),
            );
            self.apply_band(ctx, is_l, band);

            let gain = if is_l {
                p.gain_l.raw_target()
            } else {
                p.gain_r.raw_target()
            };
            let ged = widgets::gain_slider(
                ui,
                gain_rect,
                gain as f32,
                if is_l { "增益 L" } else { "增益 R" },
                &theme,
                Id::new(("gain", is_l)),
            );
            self.apply_gain(ctx, is_l, ged);

        }

        // ---- 右栏：预设 ----
        widgets::section_title(ui, rect(COL1_X, 30.0, PANEL_R, 52.0), "预设", &theme);

        for r in 0..4 {
            for c in 0..4 {
                let slot = 8 + r * 4 + c;
                let x = COL1_X + c as f32 * CELL_STEP_X;
                let y = GRID_Y + r as f32 * CELL_STEP_Y;
                let label = format!("{}", slot + 1);
                if widgets::preset_slot(
                    ui,
                    rect(x, y, x + CELL_W, y + CELL_H),
                    &label,
                    self.active_slot == Some(slot),
                    &theme,
                    Id::new(("slot", slot)),
                ) {
                    self.active_slot = Some(slot);
                }
            }
        }

        // MORPH
        let morph_text = if self.morph_norm <= 0.0 {
            "0.00s".to_string()
        } else {
            format!("{:.2}s", 0.01 * 6000f64.powf(self.morph_norm))
        };
        let morph = widgets::hbar(
            ui,
            rect(COL1_X, 188.0, PANEL_R, 230.0),
            "渐变",
            &morph_text,
            self.morph_norm,
            &theme,
            Id::new("morph"),
        );
        if let Some(v) = morph.value {
            self.morph_norm = v;
        }

        // RANDOM
        widgets::section_title(ui, rect(COL1_X, 234.0, PANEL_R, 260.0), "随机", &theme);

        // 随机标题右侧的当前色块（点击切换要编辑的颜色，原版用弹出菜单）
        {
            let r = rect(PANEL_R - 14.0, 234.0, PANEL_R, 248.0);
            let resp = ui.interact(r, Id::new("random_color"), Sense::click());
            painter.rect_filled(r, 0, Theme::random_color(self.selected_color));
            if resp.clicked() {
                self.selected_color = (self.selected_color + 1) % 4;
            }
        }

        let (amount_id, rate_id) = Self::color_params(self.selected_color);
        let amount = match self.selected_color {
            0 => p.random_amount_r.raw_target(),
            1 => p.random_amount_y.raw_target(),
            2 => p.random_amount_b.raw_target(),
            _ => p.random_amount_g.raw_target(),
        };
        let rate = match self.selected_color {
            0 => p.random_rate_r.raw_target(),
            1 => p.random_rate_y.raw_target(),
            2 => p.random_rate_b.raw_target(),
            _ => p.random_rate_g.raw_target(),
        };

        painter.rect_filled(
            rect(PANEL_R - 14.0, 292.0, PANEL_R, 306.0),
            0,
            Theme::random_color(self.selected_color),
        );
        let range = widgets::hbar(
            ui,
            rect(COL1_X, 292.0, PANEL_R - 18.0, 322.0),
            "范围",
            &format!("{:.2}", amount),
            amount,
            &theme,
            Id::new("random_range"),
        );
        if range.began {
            self.gesture.begin(ctx, pid(amount_id));
        }
        if let Some(v) = range.value {
            self.gesture.set(ctx, pid(amount_id), v);
        }
        if range.ended {
            self.gesture.end(ctx);
        }

        let speed_norm = (rate / 0.01).ln() / 6000f64.ln();
        painter.rect_filled(
            rect(PANEL_R - 14.0, 326.0, PANEL_R, 340.0),
            0,
            Theme::random_color(self.selected_color),
        );
        let speed = widgets::hbar(
            ui,
            rect(COL1_X, 326.0, PANEL_R - 18.0, 356.0),
            "速度",
            &format!("{:.2}s", rate),
            speed_norm,
            &theme,
            Id::new("random_speed"),
        );
        if speed.began {
            self.gesture.begin(ctx, pid(rate_id));
        }
        if let Some(v) = speed.value {
            let secs = 0.01 * 6000f64.powf(v);
            let norm = (secs / 0.01).ln() / 6000f64.ln();
            self.gesture.set(ctx, pid(rate_id), norm);
        }
        if speed.ended {
            self.gesture.end(ctx);
        }

        // ---- 按钮块 ----
        if widgets::flat_button(
            ui,
            rect(COL1_X, 360.0, COL1_X + 76.0, 388.0),
            "左→右",
            false,
            &theme,
            Id::new("copy_lr"),
        ) {
            Self::swap_channels(ctx, true);
        }
        if widgets::flat_button(
            ui,
            rect(COL2_X + 4.0, 360.0, PANEL_R, 388.0),
            "右→左",
            false,
            &theme,
            Id::new("copy_rl"),
        ) {
            Self::swap_channels(ctx, false);
        }

        if widgets::flat_button(
            ui,
            rect(COL1_X, 392.0, COL1_X + 76.0, 420.0),
            "同步",
            p.link.value(),
            &theme,
            Id::new("link"),
        ) {
            ctx.bridge().set_param(pid(P::Link), if p.link.value() { 0.0 } else { 1.0 });
        }
        if widgets::flat_button(
            ui,
            rect(COL2_X + 4.0, 392.0, PANEL_R, 420.0),
            "翻转",
            false,
            &theme,
            Id::new("flip"),
        ) {
            Self::flip(ctx);
        }

        // MIX
        let mix = p.mix.raw_target();
        let mix_edit = widgets::hbar(
            ui,
            rect(COL1_X, 424.0, PANEL_R, 466.0),
            "混合",
            &format!("{:.0}%", mix * 100.0),
            mix,
            &theme,
            Id::new("mix"),
        );
        if mix_edit.began {
            self.gesture.begin(ctx, pid(P::Mix));
        }
        if let Some(v) = mix_edit.value {
            self.gesture.set(ctx, pid(P::Mix), v);
        }
        if mix_edit.ended {
            self.gesture.end(ctx);
        }

        // ---- 底部：快速预设槽 + MORPH 时间条 ----
        for i in 0..8 {
            let w = (648.0 - 7.0 * 4.0) / 8.0;
            let x = 20.0 + i as f32 * (w + 4.0);
            if widgets::preset_slot(
                ui,
                rect(x, 552.0, x + w, 582.0),
                &format!("{}", i + 1),
                self.active_slot == Some(i),
                &theme,
                Id::new(("quick", i)),
            ) {
                self.active_slot = Some(i);
            }
        }

        let fade = widgets::hbar(
            ui,
            rect(20.0, 592.0, 668.0, 616.0),
            "渐变时间",
            &morph_text,
            self.morph_norm,
            &theme,
            Id::new("fade"),
        );
        if let Some(v) = fade.value {
            self.morph_norm = v;
        }

        painter.text(
            Pos2::new(COL1_X, 552.0),
            egui::Align2::LEFT_TOP,
            "ORM",
            fonts::bold(20.0),
            theme.col_900(),
        );
        painter.text(
            Pos2::new(COL1_X, 576.0),
            egui::Align2::LEFT_TOP,
            "RUST / TRUCE",
            fonts::regular(9.0),
            theme.col_700(),
        );

        // 旁标：单声道提示位（与 C++ 的 mMonoDisplay 对应）

        // ---- 弹出菜单（斜率 / 随机色板）----
        if let Some((kind, pos)) = self.menu.take() {
            let mut keep = true;
            egui::Area::new(Id::new("orm_popup"))
                .order(egui::Order::Foreground)
                .fixed_pos(pos)
                .show(ui.ctx(), |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_min_width(132.0);
                        match kind {
                            MenuKind::Slope(is_l) => {
                                let id = pid(if is_l { P::SlopeL } else { P::SlopeR });
                                let cur = if is_l {
                                    p.slope_l.index()
                                } else {
                                    p.slope_r.index()
                                };
                                for (i, db) in [12, 24, 48, 96].iter().enumerate() {
                                    let label = if i as u32 == cur {
                                        format!("✓ {db} dB/oct")
                                    } else {
                                        format!("{db} dB/oct")
                                    };
                                    if ui.button(label).clicked() {
                                        ctx.automate(id, i as f64 / 3.0);
                                        keep = false;
                                    }
                                }
                            }
                            MenuKind::Color(i, is_l) => {
                                let id = pid(match (i, is_l) {
                                    (0, true) => P::RandomColorFreqL,
                                    (1, true) => P::RandomColorBwL,
                                    (0, false) => P::RandomColorFreqR,
                                    _ => P::RandomColorBwR,
                                });
                                for c in 0..4 {
                                    let color = openrm_ui::theme::Theme::random_color(c);
                                    if ui
                                        .add(
                                            egui::Button::new("      ")
                                                .fill(color)
                                                .min_size(Vec2::new(120.0, 24.0)),
                                        )
                                        .clicked()
                                    {
                                        ctx.automate(id, c as f64 / 3.0);
                                        keep = false;
                                    }
                                }
                            }
                        }
                    });
                });
            if ui.ctx().input(|i| i.pointer.any_click()) && keep {
                // 点击别处关闭：仅当点在菜单外
                if let Some(pp) = ui.ctx().pointer_interact_pos() {
                    if !Rect::from_min_size(pos, Vec2::new(140.0, 24.0)).contains(pp) {
                        keep = false;
                    }
                }
            }
            if keep {
                self.menu = Some((kind, pos));
            }
        }

    }

    fn state_changed(&mut self, _ctx: &PluginContext<BandPassParams>) {
        self.gesture = Gesture::new();
    }
}

pub fn create(params: Arc<BandPassParams>) -> Box<dyn Editor> {
    truce_egui::EguiEditor::with_ui(params, (UI_WIDTH as u32, UI_HEIGHT as u32), BandPassUi::default())
        .with_font(fonts::REGULAR)
        .resizable(false)
        .into_editor()
}
