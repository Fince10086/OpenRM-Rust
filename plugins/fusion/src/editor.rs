//! ORM Fusion 的 egui 界面。
//!
//! 版式沿用 BandPass：同一张 916×646 画布、右侧 4×4 预设槽 + morph +
//! 随机 + 混合 + 图标行 + 标题区，底部 8 个 quick 槽与 fade 轨道。
//! 中间是 Fusion 自己的“时间 × 频率”平面和 8 个频段的参数。

use std::sync::Arc;

use egui::{Align2, CursorIcon, Id, Pos2, Rect, Sense, Stroke, Ui, Vec2};
use openrm_ui::{
    fonts,
    preset::{Fade, History, PresetBank, QUICK_COUNT, SLOT_COUNT},
    shell,
    strings::{self, Key, Lang},
    theme::{rgba, Theme},
    widgets::{self, BarModel, GainModel, Icon},
};
use serde_json::Value;
use truce::prelude::*;
use truce_egui::EditorUi;

use crate::params::{FusionParams, PARAM_COUNT};

pub const W: f32 = openrm_ui::UI_WIDTH;
pub const H: f32 = openrm_ui::UI_HEIGHT;

/// 画布左侧：频段区 + 平面；增益条紧接着平面右缘。
const PAD: (f32, f32, f32, f32) = (20.0, 64.0, 668.0, 372.0);
const GAIN: (f32, f32, f32, f32) = (672.0, 64.0, 730.0, 372.0);
const PLOT_TOP: f32 = 112.0;
const PARAM_TOP: f32 = 380.0;
const ROW_H: f32 = 34.0;

fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
    Rect::from_min_max(Pos2::new(x0, y0), Pos2::new(x1, y1))
}

fn r4(t: (f32, f32, f32, f32)) -> Rect {
    rect(t.0, t.1, t.2, t.3)
}

// 参数 id（params.rs 的 94 个线性参数；90..93 是随机控制）
const P_BANDS: u32 = 4;
const P_GAIN: u32 = 5;
const P_MIX: u32 = 6;
const AMP0: u32 = 7;
const FB0: u32 = 15;
const DELAY0: u32 = 24;
const FREQ0: u32 = 32;
const LEN0: u32 = 40;
const ANG0: u32 = 48;
const PAN0: u32 = 56;
const WID0: u32 = 64;
const ACT0: u32 = 72;
const MUTE0: u32 = 80;
const P_RANDOM_ON: u32 = 90;
const P_RANDOM_RANGE: u32 = 91;
const P_RANDOM_SPEED: u32 = 92;
const P_RANDOM_COLOR: u32 = 93;

#[derive(Clone, Copy, Default)]
struct BarView {
    amp: f32,
    fb: f32,
    delay: f32,
    freq: f32,
    length: f32,
    angle: f32,
    pan: f32,
    width: f32,
    active: bool,
    mute: bool,
}

#[derive(Clone, Copy)]
struct RandomBase {
    values: [f64; 8],
    bar: usize,
}

#[derive(Clone, Copy)]
enum MenuKind {
    Preset(usize),
    RandomColor,
}

pub struct FusionUi {
    theme: Theme,
    lang: Lang,
    bank: PresetBank,
    history: History,
    last_idle: Vec<f64>,
    selected: usize,
    fade_time: f64,
    fade: Option<Fade>,
    random_base: Option<RandomBase>,
    random_phase: f32,
    last_time: f64,
    font_stage: u8,
    styled: bool,
    settings_open: bool,
    drag_from: Option<usize>,
    drag_hover: Option<usize>,
    menu: Option<(MenuKind, Pos2)>,
    menu_armed: bool,
    pad_drag: Option<usize>,
    editing: Vec<u32>,
}

impl Default for FusionUi {
    fn default() -> Self {
        let (lang, theme) = shell::load_settings("ORMFusion.settings");
        let bank = PresetBank::from_factory(factory_presets());
        let last_idle = defaults();
        Self {
            theme,
            lang,
            bank,
            history: History::default(),
            last_idle,
            selected: 0,
            fade_time: 0.25,
            fade: None,
            random_base: None,
            random_phase: 0.0,
            last_time: 0.0,
            font_stage: 0,
            styled: false,
            settings_open: false,
            drag_from: None,
            drag_hover: None,
            menu: None,
            menu_armed: false,
            pad_drag: None,
            editing: Vec::new(),
        }
    }
}

// ===== 参数快照 / 写入 =====

fn defaults() -> Vec<f64> {
    let p = FusionParams::new();
    (0..PARAM_COUNT as u32)
        .map(|id| p.get_plain(id).unwrap_or(0.0))
        .collect()
}

fn snapshot(p: &FusionParams) -> Vec<f64> {
    (0..PARAM_COUNT as u32)
        .map(|id| p.get_plain(id).unwrap_or(0.0))
        .collect()
}

/// 拖动中的参数：本地 + 宿主 `set_param`（配合 begin_edit / end_edit）。
fn write_param(ctx: &PluginContext<FusionParams>, id: u32, v: f64) {
    let p = ctx.params();
    p.set_plain(id, v);
    if let Some(n) = p.get_normalized(id) {
        ctx.set_param(id, n);
    }
}

/// 单次点击 / 开关：本地 + 一次性 automate。
fn click_param(ctx: &PluginContext<FusionParams>, id: u32, v: f64) {
    let p = ctx.params();
    p.set_plain(id, v);
    if let Some(n) = p.get_normalized(id) {
        ctx.automate(id, n);
    }
}

fn apply_snapshot(ctx: &PluginContext<FusionParams>, s: &[f64]) {
    let p = ctx.params();
    for (i, v) in s.iter().enumerate().take(PARAM_COUNT) {
        p.set_plain(i as u32, *v);
    }
    for i in 0..PARAM_COUNT {
        if let Some(n) = p.get_normalized(i as u32) {
            ctx.automate(i as u32, n);
        }
    }
}

fn bar_ids(i: usize) -> (u32, u32, u32, u32, u32, u32, u32, u32, u32, u32) {
    let b = i as u32;
    (
        AMP0 + b,
        FB0 + b,
        DELAY0 + b,
        FREQ0 + b,
        LEN0 + b,
        ANG0 + b,
        PAN0 + b,
        WID0 + b,
        ACT0 + b,
        MUTE0 + b,
    )
}

fn band_param_ids(i: usize) -> [u32; 8] {
    let b = i as u32;
    [
        AMP0 + b,
        FB0 + b,
        DELAY0 + b,
        FREQ0 + b,
        LEN0 + b,
        ANG0 + b,
        PAN0 + b,
        WID0 + b,
    ]
}

fn amplitude_from_slider(v: f32) -> f32 {
    10f32.powf(2.0 * (v - 1.0))
}

fn delay_sec(v: f32) -> f32 {
    0.001 + 9.999 * v.clamp(0.0, 1.0)
}

fn center_hz(v: f32, sr: f32) -> f64 {
    (v.clamp(0.0, 1.0) as f64) * (v.clamp(0.0, 1.0) as f64) * sr as f64 * 0.5
}

fn display_freq(v: f32, sr: f32) -> String {
    let hz = center_hz(v, sr);
    if hz >= 10_000.0 {
        format!("{:.1}k", hz / 1000.0)
    } else if hz >= 1000.0 {
        format!("{:.2}k", hz / 1000.0)
    } else {
        format!("{:.0}", hz)
    }
}

/// 频段线段端点：中心 + 长度 / 角度。
/// 当前平面 X = 时间（延迟），Y = 频率；角度 0° 沿时间轴向右。
fn bar_endpoints(b: &BarView) -> (f32, f32, f32, f32) {
    let half = b.length.clamp(0.0, 0.5);
    let theta = b.angle * std::f32::consts::PI;
    // 与 DSP 的 set_angle_param 一致：角度 0° 沿频率轴（Y），
    // 90° 才转到时间轴（X）。这样截图与真实滤波器形状一致。
    let df = half * theta.cos();
    let da = half * theta.sin();
    let center_a = b.delay.clamp(0.0, 1.0);
    (
        (b.freq - df).clamp(0.0, 1.0),
        (b.freq + df).clamp(0.0, 1.0),
        (center_a - da).clamp(0.0, 1.0),
        (center_a + da).clamp(0.0, 1.0),
    )
}

/// X 轴：1 ms 在左，10 s 在右（对数）；Y 轴：0 Hz 在底，Nyquist 在顶。
fn pad_point(pad: Rect, f: f32, a: f32) -> Pos2 {
    let ms = 1.0 + a.clamp(0.0, 1.0) * 9_999.0;
    let lx = (ms.ln() / 10_000f32.ln()).clamp(0.0, 1.0);
    // 频率参数到 Hz 是平方关系（f²·Nyquist）。Y 轴用 20 Hz…24 kHz 的对数
    // 刻度：否则 375 Hz 的频段会挤在底边，整块平面几乎空着。
    let hz = f.clamp(0.0, 1.0).powi(2) * 24_000.0;
    let y = (hz.max(20.0) / 20.0).ln() / (24_000f32 / 20.0).ln();
    Pos2::new(
        pad.left() + lx * pad.width(),
        pad.bottom() - y.clamp(0.0, 1.0) * pad.height(),
    )
}

fn distance_to_segment(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let len2 = ab.length_sq();
    if len2 <= 1e-9 {
        return (p - a).length();
    }
    let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    (p - (a + ab * t)).length()
}

/// 预置 24 个工厂预设：8 个单段、8 个双段、8 个全段。
fn factory_presets() -> Vec<Vec<f64>> {
    let mut list = Vec::with_capacity(SLOT_COUNT);
    let base = defaults();

    for solo in 0..8usize {
        let mut v = base.clone();
        for b in 0..8 {
            v[(ACT0 + b as u32) as usize] = if b == solo { 1.0 } else { 0.0 };
            v[(MUTE0 + b as u32) as usize] = 0.0;
        }
        v[(FREQ0 as usize) + solo] = 0.10 + solo as f64 * 0.05;
        v[(DELAY0 as usize) + solo] = 0.04 + solo as f64 * 0.06;
        v[(LEN0 as usize) + solo] = 0.06;
        v[(AMP0 as usize) + solo] = 0.92;
        list.push(v);
    }

    for pair in 0..8usize {
        let mut v = base.clone();
        for b in 0..8 {
            let on = b == pair || b == (pair + 1) % 8;
            v[(ACT0 + b as u32) as usize] = if on { 1.0 } else { 0.0 };
            v[(MUTE0 + b as u32) as usize] = 0.0;
        }
        for b in [pair, (pair + 1) % 8] {
            v[(FREQ0 as usize) + b] = 0.08 + b as f64 * 0.055;
            v[(DELAY0 as usize) + b] = 0.08 + b as f64 * 0.07;
            v[(LEN0 as usize) + b] = 0.10;
            v[(ANG0 as usize) + b] = if b % 2 == 0 { 0.0 } else { 0.15 };
            v[(PAN0 as usize) + b] = if b % 2 == 0 { 0.35 } else { 0.65 };
            v[(WID0 as usize) + b] = 0.8;
            v[(AMP0 as usize) + b] = 0.9;
        }
        list.push(v);
    }

    for all in 0..8usize {
        let mut v = base.clone();
        for b in 0..8usize {
            v[(ACT0 + b as u32) as usize] = 1.0;
            v[(MUTE0 + b as u32) as usize] = 0.0;
            v[(FREQ0 as usize) + b] = 0.06 + b as f64 * 0.055;
            v[(DELAY0 as usize) + b] = 0.05 + ((b + all) % 8) as f64 * 0.08;
            v[(LEN0 as usize) + b] = 0.07;
            v[(ANG0 as usize) + b] = if all % 2 == 0 { 0.0 } else { 0.12 };
            v[(PAN0 as usize) + b] = 0.15 + b as f64 / 7.0 * 0.7;
            v[(WID0 as usize) + b] = 0.95;
            v[(AMP0 as usize) + b] = 0.88;
        }
        list.push(v);
    }
    list
}

fn smooth_noise(phase: f32, i: usize) -> f32 {
    let p = phase + i as f32 * 1.71;
    (p.sin() * 0.58 + (p * 2.13 + i as f32 * 0.9).sin() * 0.28
        + (p * 4.37 + i as f32 * 2.2).sin() * 0.14)
        .clamp(-1.0, 1.0)
}

impl FusionUi {
    /// 截图与自测用：直接打开设置面板。
    pub fn open_settings(&mut self) {
        self.settings_open = true;
    }

    /// 截图与自测用：深色主题。
    pub fn set_dark(&mut self) {
        self.theme.mode = openrm_ui::theme::ThemeMode::Dark;
    }

    /// 截图与自测用：切换语言。
    pub fn set_language(&mut self, lang: Lang) {
        self.lang = lang;
    }

    /// 截图与自测用：立即套用某个预设（不渐变）。
    pub fn preview_preset(&mut self, ctx: &PluginContext<FusionParams>, idx: usize) {
        if let Some(t) = self.bank.slot(idx).cloned() {
            apply_snapshot(ctx, &t);
            self.bank.current = idx;
            self.bank.quick = idx.min(QUICK_COUNT - 1);
        }
    }

    /// 截图与自测用：选中某个 band。
    pub fn select_band(&mut self, i: usize) {
        self.selected = i.min(7);
    }

    fn tr(&self, key: Key) -> &'static str {
        strings::tr(self.lang, key)
    }

    fn ensure_style(&mut self, ui: &mut Ui) {
        match self.font_stage {
            0 => {
                openrm_ui::fonts::install(ui.ctx());
                self.font_stage = 1;
                ui.ctx().request_repaint();
            }
            1 => {
                openrm_ui::fonts::set_named_ready(true);
                self.font_stage = 2;
            }
            _ => {}
        }
        if self.styled {
            return;
        }
        self.styled = true;
        let mut style = (*ui.ctx().global_style()).clone();
        style.spacing.item_spacing = Vec2::ZERO;
        style.spacing.button_padding = Vec2::ZERO;
        ui.ctx().set_global_style(style);
    }

    fn bar_view(&self, ctx: &PluginContext<FusionParams>, i: usize) -> BarView {
        let (amp, fb, delay, freq, length, angle, pan, width, active, mute) = bar_ids(i);
        let p = ctx.params();
        BarView {
            amp: p.get_plain(amp).unwrap_or(0.0) as f32,
            fb: p.get_plain(fb).unwrap_or(0.0) as f32,
            delay: p.get_plain(delay).unwrap_or(0.0) as f32,
            freq: p.get_plain(freq).unwrap_or(0.0) as f32,
            length: p.get_plain(length).unwrap_or(0.0) as f32,
            angle: p.get_plain(angle).unwrap_or(0.0) as f32,
            pan: p.get_plain(pan).unwrap_or(0.0) as f32,
            width: p.get_plain(width).unwrap_or(0.0) as f32,
            active: p.get_plain(active).unwrap_or(0.0) >= 0.5,
            mute: p.get_plain(mute).unwrap_or(0.0) >= 0.5,
        }
    }

    fn begin_param(&mut self, ctx: &PluginContext<FusionParams>, id: u32) {
        if !self.editing.contains(&id) {
            if self.editing.is_empty() {
                self.history.push(self.last_idle.clone());
            }
            self.editing.push(id);
            ctx.begin_edit(id);
        }
    }

    fn finish_edits(&mut self, ui: &Ui, ctx: &PluginContext<FusionParams>) {
        if ui.input(|i| i.pointer.any_released()) && !self.editing.is_empty() {
            for id in self.editing.drain(..) {
                ctx.end_edit(id);
            }
        }
    }

    fn random_color(&self, ctx: &PluginContext<FusionParams>) -> usize {
        (ctx.params().get_plain(P_RANDOM_COLOR).unwrap_or(0.0).round() as usize).min(3)
    }

    fn random_on(&self, ctx: &PluginContext<FusionParams>) -> bool {
        ctx.params().get_plain(P_RANDOM_ON).unwrap_or(0.0) >= 0.5
    }

    fn stop_random(&mut self, ctx: &PluginContext<FusionParams>) {
        if let Some(base) = self.random_base.take() {
            for (id, v) in band_param_ids(base.bar).iter().zip(base.values.iter()) {
                click_param(ctx, *id, *v);
            }
            click_param(ctx, P_RANDOM_ON, 0.0);
        }
        self.random_phase = 0.0;
    }

    fn toggle_random(&mut self, ctx: &PluginContext<FusionParams>) {
        if self.random_on(ctx) {
            self.stop_random(ctx);
            return;
        }
        let ids = band_param_ids(self.selected);
        let mut values = [0.0f64; 8];
        for (i, id) in ids.iter().enumerate() {
            values[i] = ctx.params().get_plain(*id).unwrap_or(0.0);
        }
        self.random_base = Some(RandomBase {
            values,
            bar: self.selected,
        });
        self.random_phase = 0.0;
        click_param(ctx, P_RANDOM_ON, 1.0);
    }

    fn random_values(&self, ctx: &PluginContext<FusionParams>) -> Option<[f64; 8]> {
        let base = self.random_base?;
        if base.bar != self.selected || !self.random_on(ctx) {
            return None;
        }
        let range = ctx.params().get_plain(P_RANDOM_RANGE).unwrap_or(0.35) as f32;
        let spans = [0.30f32, 0.24, 0.32, 0.24, 0.18, 0.50, 0.32, 0.24];
        let mut out = [0.0f64; 8];
        for i in 0..8 {
            let v = base.values[i] as f32 + range * spans[i] * smooth_noise(self.random_phase, i);
            out[i] = match i {
                0 | 1 | 6 | 7 => v.clamp(0.0, 1.0) as f64,
                2 | 3 => v.clamp(0.0, 1.0) as f64,
                4 => v.clamp(0.01, 0.48) as f64,
                _ => v.clamp(0.0, 1.0) as f64,
            };
        }
        Some(out)
    }

    fn animate_random(&mut self, ctx: &PluginContext<FusionParams>, dt: f64) {
        if !self.random_on(ctx) {
            self.random_base = None;
            return;
        }
        if self.random_base.is_none() {
            let ids = band_param_ids(self.selected);
            let mut values = [0.0f64; 8];
            for (i, id) in ids.iter().enumerate() {
                values[i] = ctx.params().get_plain(*id).unwrap_or(0.0);
            }
            self.random_base = Some(RandomBase {
                values,
                bar: self.selected,
            });
        }
        let speed = ctx.params().get_plain(P_RANDOM_SPEED).unwrap_or(0.4) as f32;
        // Speed 0..1 → 周期 50 ms … 20 s
        let period = 0.05 + 19.95 * speed.clamp(0.0, 1.0);
        if dt > 0.0 {
            self.random_phase += (dt / period as f64 * std::f64::consts::TAU) as f32;
            if self.random_phase > std::f32::consts::TAU * 1024.0 {
                self.random_phase %= std::f32::consts::TAU * 1024.0;
            }
        }
        if let Some(vals) = self.random_values(ctx) {
            for (id, v) in band_param_ids(self.selected).iter().zip(vals.iter()) {
                write_param(ctx, *id, *v);
            }
        }
    }

    fn set_selected(&mut self, ctx: &PluginContext<FusionParams>, i: usize) {
        if i == self.selected {
            return;
        }
        self.stop_random(ctx);
        self.selected = i.min(7);
    }

    fn load_slot(&mut self, ctx: &PluginContext<FusionParams>, idx: usize) {
        self.stop_random(ctx);
        let Some(target) = self.bank.slot(idx).cloned() else {
            return;
        };
        let from = snapshot(&ctx.params());
        self.history.push(from.clone());
        self.bank.current = idx;
        self.bank.quick = idx.min(QUICK_COUNT - 1);
        self.fade = Some(Fade::new(from, target, self.fade_time));
    }

    fn tick_fade(&mut self, ctx: &PluginContext<FusionParams>) {
        let Some(f) = self.fade.as_ref() else { return };
        let t = f.progress();
        if t >= 1.0 {
            let to = f.to.clone();
            apply_snapshot(ctx, &to);
            self.fade = None;
            return;
        }
        let infos = ctx.params().param_infos();
        let mut mix = Vec::with_capacity(PARAM_COUNT);
        for i in 0..PARAM_COUNT {
            let r = infos.get(i).map(|info| info.range);
            let v = match r {
                Some(range) => {
                    let a = range.normalize(f.from[i]);
                    let b = range.normalize(f.to[i]);
                    range.denormalize(a + (b - a) * t)
                }
                None => f.from[i] + (f.to[i] - f.from[i]) * t,
            };
            mix.push(v);
        }
        apply_snapshot(ctx, &mix);
    }

    fn undo(&mut self, ctx: &PluginContext<FusionParams>) {
        self.stop_random(ctx);
        let cur = snapshot(&ctx.params());
        if let Some(prev) = self.history.undo(cur) {
            apply_snapshot(ctx, &prev);
        }
    }

    fn redo(&mut self, ctx: &PluginContext<FusionParams>) {
        self.stop_random(ctx);
        let cur = snapshot(&ctx.params());
        if let Some(next) = self.history.redo(cur) {
            apply_snapshot(ctx, &next);
        }
    }

    fn save_orm(&mut self, ctx: &PluginContext<FusionParams>) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("ORM preset", &["orm"])
            .set_file_name("ORMFusion.orm")
            .save_file()
        else {
            return;
        };
        let mut j = self.bank.to_json();
        j["currentPreset"] = serde_json::json!(self.bank.current);
        j["currentValues"] = serde_json::json!(snapshot(&ctx.params()));
        j["fadePos"] = serde_json::json!(self.bank.quick as f64 / 7.0);
        j["selected"] = serde_json::json!(self.selected);
        let text = format!(
            "fusion\n{}",
            serde_json::to_string_pretty(&j).unwrap_or_default()
        );
        let _ = std::fs::write(path, text);
    }

    fn load_orm(&mut self, ctx: &PluginContext<FusionParams>) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("ORM preset", &["orm"])
            .pick_file()
        else {
            return;
        };
        let Ok(text) = std::fs::read_to_string(path) else {
            return;
        };
        let Some(body) = text.strip_prefix("fusion") else {
            return;
        };
        let Ok(j) = serde_json::from_str::<Value>(body.trim_start_matches(['\r', '\n'])) else {
            return;
        };
        // 旧版插件的 90 参数存档：把每个槽补到 94，避免整库被当成无效。
        let mut jn = j.clone();
        let base_defaults = defaults();
        if let Some(slots) = jn.get_mut("slots").and_then(|v| v.as_array_mut()) {
            for slot in slots.iter_mut() {
                if let Some(vals) = slot.as_array_mut() {
                    if vals.len() < PARAM_COUNT {
                        let old_len = vals.len();
                        for i in old_len..PARAM_COUNT {
                            vals.push(serde_json::json!(base_defaults[i]));
                        }
                    }
                }
            }
        }
        let Some(bank) = PresetBank::from_json(&jn, PARAM_COUNT) else {
            return;
        };
        self.stop_random(ctx);
        self.bank = bank;
        if let Some(cur) = j.get("currentPreset").and_then(Value::as_i64) {
            self.bank.current = cur.clamp(0, (SLOT_COUNT - 1) as i64) as usize;
        }
        if let Some(sel) = j.get("selected").and_then(Value::as_i64) {
            self.selected = sel.clamp(0, 7) as usize;
        }
        if let Some(vals) = j.get("currentValues").and_then(Value::as_array) {
            // 旧版 90 参数的 .orm 仍可读取，随机控制参数补默认值。
            if vals.len() == PARAM_COUNT || vals.len() == 90 {
                let mut v: Vec<f64> = vals.iter().map(|v| v.as_f64().unwrap_or(0.0)).collect();
                v.resize(PARAM_COUNT, 0.0);
                if vals.len() == 90 {
                    for i in 90..PARAM_COUNT {
                        v[i] = base_defaults[i];
                    }
                }
                self.history.push(snapshot(&ctx.params()));
                apply_snapshot(ctx, &v);
            }
        }
    }

    // ===== 顶部 band 开关 =====

    fn header(&mut self, ctx: &PluginContext<FusionParams>, ui: &mut Ui) {
        let theme = self.theme;
        let y0 = 30.0;
        let y1 = 56.0;

        for i in 0..8 {
            let r = rect(20.0 + i as f32 * 34.0, y0, 20.0 + i as f32 * 34.0 + 30.0, y1);
            let bv = self.bar_view(ctx, i);
            let resp = ui.interact(r, Id::new(("bar_tab", i)), Sense::click());
            let resp = resp.on_hover_text(self.tr(Key::TipBandTab));
            let selected = i == self.selected;
            let pnt = ui.painter_at(r);
            pnt.rect_filled(
                r,
                0,
                if resp.hovered() {
                    theme.col_300()
                } else {
                    theme.col_100()
                },
            );
            pnt.rect_filled(
                rect(r.left(), r.bottom() - 3.0, r.right(), r.bottom()),
                0,
                if bv.active && !bv.mute {
                    Theme::random_color(i % 4)
                } else {
                    Theme::random_color_dim(i % 4)
                },
            );
            pnt.text(
                r.center(),
                Align2::CENTER_CENTER,
                format!("{}", i + 1),
                fonts::semibold(20.0),
                theme.col_900(),
            );
            if selected {
                pnt.rect_stroke(
                    r.shrink(1.0),
                    0,
                    Stroke::new(2.0_f32, theme.col_900()),
                    egui::StrokeKind::Inside,
                );
            }
            // 1–8 只作为启用开关，不再承担选择职责；选择请点击平面里的频段线。
            if resp.clicked() {
                self.history.push(snapshot(&ctx.params()));
                click_param(ctx, ACT0 + i as u32, if bv.active { 0.0 } else { 1.0 });
                if !bv.active && bv.mute {
                    click_param(ctx, MUTE0 + i as u32, 0.0);
                }
            }
        }

        if widgets::flat_button(
            ui,
            rect(300.0, y0, 350.0, y1),
            self.tr(Key::Reset),
            false,
            &theme,
            Id::new("fusion_reset"),
            Some(self.tr(Key::TipReset)),
        ) {
            ctx.params().host.request_reset();
        }
        if widgets::flat_button(
            ui,
            rect(358.0, y0, 408.0, y1),
            self.tr(Key::Mute),
            false,
            &theme,
            Id::new("fusion_mute_all"),
            Some(self.tr(Key::TipMute)),
        ) {
            let all_muted = (0..8).all(|i| {
                let bv = self.bar_view(ctx, i);
                !bv.active || bv.mute
            });
            let target = if all_muted { 0.0 } else { 1.0 };
            for i in 0..8 {
                click_param(ctx, MUTE0 + i as u32, target);
            }
        }

        let pnt = ui.painter_at(rect(0.0, 0.0, W, H));
        pnt.text(
            Pos2::new(430.0, (y0 + y1) * 0.5),
            Align2::CENTER_CENTER,
            format!("{}", self.selected + 1),
            fonts::bold(34.0),
            theme.col_900(),
        );
        let b = self.bar_view(ctx, self.selected);
        if widgets::flat_button(
            ui,
            rect(458.0, y0, 514.0, y1),
            if b.active { self.tr(Key::On) } else { self.tr(Key::Off) },
            b.active,
            &theme,
            Id::new("fusion_active"),
            Some(self.tr(Key::TipActivate)),
        ) {
            self.history.push(snapshot(&ctx.params()));
            click_param(ctx, ACT0 + self.selected as u32, if b.active { 0.0 } else { 1.0 });
        }
    }

    // ===== 中央 时间 × 频率 平面 =====

    fn display(&mut self, ctx: &PluginContext<FusionParams>, ui: &mut Ui) {
        let theme = self.theme;
        let area = r4(PAD);
        let p = ui.painter_at(area);
        p.rect_filled(area, 0, theme.col_100());

        // 顶部参数条：中心 / 延迟 / 长度 / 角度 / 幅度
        let b = self.bar_view(ctx, self.selected);
        let sr = ctx.params().host.sample_rate();
        let sw = rect(20.0, 70.0, 34.0, 84.0);
        p.rect_filled(sw, 0, Theme::random_color(self.selected % 4));
        p.text(
            Pos2::new(41.0, 77.0),
            Align2::LEFT_CENTER,
            format!("{} {}", self.tr(Key::Band), self.selected + 1),
            fonts::bold(18.0),
            theme.col_900(),
        );

        let fields: [(Rect, Key, u32, f64, String); 5] = [
            (
                rect(108.0, 66.0, 226.0, 98.0),
                Key::Center,
                FREQ0,
                b.freq as f64,
                format!("{} Hz", display_freq(b.freq, sr)),
            ),
            (
                rect(232.0, 66.0, 338.0, 98.0),
                Key::Delay,
                DELAY0,
                b.delay as f64,
                format!("{:.2}s", delay_sec(b.delay)),
            ),
            (
                rect(344.0, 66.0, 428.0, 98.0),
                Key::Length,
                LEN0,
                (b.length * 2.0) as f64,
                format!("{:.0}%", b.length * 200.0),
            ),
            (
                rect(434.0, 66.0, 506.0, 98.0),
                Key::Angle,
                ANG0,
                b.angle as f64,
                format!("{:.0}°", b.angle * 180.0),
            ),
            (
                rect(512.0, 66.0, 652.0, 98.0),
                Key::Amplitude,
                AMP0,
                b.amp as f64,
                format!("{:.0} dB", 20.0 * amplitude_from_slider(b.amp).log10()),
            ),
        ];
        for (i, (r, key, base, norm, text)) in fields.iter().enumerate() {
            let label = self.tr(*key);
            let edit = shell::value_field(
                ui,
                *r,
                label,
                text,
                *norm,
                &theme,
                Id::new(("fusion_top", i)),
                None,
            );
            if edit.began {
                if self.random_on(ctx) {
                    self.stop_random(ctx);
                }
                self.begin_param(ctx, *base + self.selected as u32);
            }
            if let Some(v) = edit.value {
                let id = *base + self.selected as u32;
                let value = match i {
                    2 => (v * 0.5).clamp(0.0, 0.5),
                    _ => v,
                };
                write_param(ctx, id, value);
            }
        }

        let plot = rect(area.left(), PLOT_TOP, area.right(), area.bottom());
        // X 轴时间：按十倍程画竖纹
        let cells = 16;
        for i in 0..cells {
            let x0 = plot.left() + plot.width() * i as f32 / cells as f32;
            let x1 = plot.left() + plot.width() * (i + 1) as f32 / cells as f32;
            let v = if i % 2 == 0 { 236.0 } else { 228.0 } + if i % 4 == 0 { 4.0 } else { 0.0 };
            p.rect_filled(rect(x0, plot.top(), x1, plot.bottom()), 0, theme.warm_gray(v as u8));
        }
        // 频率横线 100 / 1k / 6k / 12k / 24k（对数）
        for hz in [100.0f32, 1000.0, 6000.0, 12000.0, 24_000.0] {
            let n = (hz / 20.0).ln() / (24_000f32 / 20.0).ln();
            let y = plot.bottom() - n * plot.height();
            p.line_segment(
                [Pos2::new(plot.left(), y), Pos2::new(plot.right(), y)],
                Stroke::new(1.0_f32, theme.col_300()),
            );
        }
        // 时间竖线 1ms / 10ms / 100ms / 1s / 10s
        for i in 0..=4 {
            let x = plot.left() + plot.width() * i as f32 / 4.0;
            p.line_segment(
                [Pos2::new(x, plot.top()), Pos2::new(x, plot.bottom())],
                Stroke::new(1.0_f32, theme.col_300()),
            );
        }

        let nyq = sr * 0.5;
        let _ = nyq;
        let views: Vec<BarView> = (0..8).map(|i| self.bar_view(ctx, i)).collect();
        for (i, b) in views.iter().enumerate() {
            // 关闭的频段也画成很淡的线，保证仍可在平面里点选；顶栏 1–8 只负责开关。
            let (f1, f2, a1, a2) = bar_endpoints(b);
            let p1 = pad_point(plot, f1, a1);
            let p2 = pad_point(plot, f2, a2);
            let col = Theme::random_color(i % 4);
            let selected = i == self.selected;
            let alpha = if !b.active || b.mute {
                40
            } else if selected {
                255
            } else {
                150
            };
            p.line_segment(
                [p1, p2],
                Stroke::new(if selected { 3.0_f32 } else { 1.4_f32 }, rgba(col, alpha)),
            );
            for h in [p1, p2] {
                p.circle_filled(h, if selected { 7.0 } else { 4.0 }, rgba(col, alpha));
            }
            if selected {
                for h in [p1, p2] {
                    p.circle_stroke(h, 10.0, Stroke::new(1.0_f32, theme.col_900()));
                }
            }
            if b.active && !b.mute {
                p.text(
                    Pos2::new(p2.x + if selected { 12.0 } else { 8.0 }, p2.y - 11.0),
                    Align2::LEFT_CENTER,
                    format!("{}", i + 1),
                    fonts::semibold(if selected { 16.0 } else { 13.0 }),
                    rgba(col, if selected { 255 } else { 170 }),
                );
            }
        }

        // 随机目标：与 BandPass 一样用半透明色线/色点显示
        if let Some(vals) = self.random_values(ctx) {
            let ghost = BarView {
                amp: vals[0] as f32,
                fb: vals[1] as f32,
                delay: vals[2] as f32,
                freq: vals[3] as f32,
                length: vals[4] as f32,
                angle: vals[5] as f32,
                pan: vals[6] as f32,
                width: vals[7] as f32,
                active: true,
                mute: false,
            };
            let (f1, f2, a1, a2) = bar_endpoints(&ghost);
            let g1 = pad_point(plot, f1, a1);
            let g2 = pad_point(plot, f2, a2);
            let col = Theme::random_color(self.random_color(ctx));
            p.line_segment([g1, g2], Stroke::new(1.6_f32, rgba(col, 120)));
            for g in [g1, g2] {
                p.circle_filled(g, 6.0, rgba(col, 150));
                p.circle_filled(g, 2.5, theme.col_100());
            }
        }

        // 轴标只留刻度：1ms…10s 与 100…24k。原先左上角的长句会和
        // 12k/6k 刻度叠在一起，悬停提示里已经写明 X=时间 / Y=频率。
        for (i, label) in ["1ms", "10ms", "100ms", "1s", "10s"].iter().enumerate() {
            let x = plot.left() + plot.width() * i as f32 / 4.0;
            p.text(
                Pos2::new(x + if i == 0 { 2.0 } else { 0.0 }, plot.bottom() - 4.0),
                if i == 0 {
                    Align2::LEFT_BOTTOM
                } else if i == 4 {
                    Align2::RIGHT_BOTTOM
                } else {
                    Align2::CENTER_BOTTOM
                },
                *label,
                fonts::regular(10.0),
                theme.col_500(),
            );
        }
        for (hz, label) in [
            (100.0f32, "100"),
            (1000.0, "1k"),
            (6000.0, "6k"),
            (12000.0, "12k"),
            (24_000.0, "24k"),
        ] {
            let n = (hz / 20.0).ln() / (24_000f32 / 20.0).ln();
            let y = plot.bottom() - n * plot.height();
            p.text(
                Pos2::new(plot.left() + 3.0, y - 2.0),
                Align2::LEFT_BOTTOM,
                label,
                fonts::regular(10.0),
                theme.col_500(),
            );
        }

        // 增益：与 BandPass 一样竖直贴在平面右缘
        let gain = ctx.params().get_plain(P_GAIN).unwrap_or(0.8);
        let db = -4.8 + 6.0 * gain;
        let gedit = widgets::gain_slider(
            ui,
            r4(GAIN),
            &GainModel {
                db,
                label: self.tr(Key::Gain),
                min_db: -4.8,
                max_db: 1.2,
                random_on: false,
                random_color: 0,
                show_random: false,
                delta_db: 0.0,
                ghost: false,
                tip: Some(self.tr(Key::TipGain)),
            },
            &theme,
            Id::new("fusion_gain"),
        );
        if gedit.began {
            self.history.push(self.last_idle.clone());
        }
        if let Some(v) = gedit.db {
            write_param(ctx, P_GAIN, ((v + 4.8) / 6.0).clamp(0.0, 1.0));
        }

        // 点击平面上的频段线选择；拖动其中心改时间/频率
        let resp = ui
            .interact(plot, Id::new("fusion_pad"), Sense::click_and_drag())
            .on_hover_text(self.tr(Key::TipFusionPad));
        if resp.hovered() || resp.dragged() {
            ui.ctx().set_cursor_icon(CursorIcon::Grab);
        }
        if resp.clicked() {
            if let Some(pos) = ui.ctx().pointer_interact_pos() {
                if let Some(i) = nearest_band(&views, plot, pos) {
                    self.set_selected(ctx, i);
                }
            }
        }
        if resp.drag_started() {
            if let Some(pos) = ui.ctx().pointer_interact_pos() {
                let hit = nearest_band(&views, plot, pos);
                if let Some(i) = hit {
                    self.set_selected(ctx, i);
                    self.pad_drag = Some(i);
                    self.history.push(snapshot(&ctx.params()));
                    self.begin_param(ctx, FREQ0 + i as u32);
                    self.begin_param(ctx, DELAY0 + i as u32);
                }
            }
        }
        if resp.dragged() {
            if let (Some(i), Some(pos)) = (self.pad_drag, ui.ctx().pointer_interact_pos()) {
                let fx = ((pos.y - plot.top()) / plot.height()).clamp(0.0, 1.0);
                let tx = ((pos.x - plot.left()) / plot.width()).clamp(0.0, 1.0);
                // X 轴是对数时间：指针位置要按 10^4 反算回 ms。
                let ms = 10f32.powf(4.0 * tx);
                let a = ((ms - 1.0) / 9_999.0).clamp(0.0, 1.0);
                // Y 轴是 20 Hz…24 kHz 对数刻度；反算回参数 sqrt(Hz/24k)。
                let hz = 20.0 * (24_000f32 / 20.0).powf(1.0 - fx);
                let f = (hz / 24_000.0).sqrt();
                write_param(ctx, FREQ0 + i as u32, f as f64);
                write_param(ctx, DELAY0 + i as u32, a as f64);
            }
        }
        if resp.drag_stopped() {
            self.pad_drag = None;
        }
    }

    // ===== 下方 4 个频段参数 =====

    fn params(&mut self, ctx: &PluginContext<FusionParams>, ui: &mut Ui) {
        let theme = self.theme;
        let b = self.bar_view(ctx, self.selected);
        let rows: [(Key, u32, f64, String); 4] = [
            (Key::Amplitude, AMP0, b.amp as f64, format!("{:.0} dB", 20.0 * amplitude_from_slider(b.amp).log10())),
            (Key::Feedback, FB0, b.fb as f64, format!("{:.0}%", b.fb * 100.0)),
            (Key::Pan, PAN0, b.pan as f64, format!("{:+.2}", b.pan - 0.5)),
            (Key::Width, WID0, b.width as f64, format!("{:.0}%", b.width * 100.0)),
        ];
        for (n, (key, base, norm, text)) in rows.iter().enumerate() {
            let y = PARAM_TOP + n as f32 * ROW_H;
            let id = base + self.selected as u32;
            let r = rect(20.0, y, 688.0, y + 30.0);
            let label = self.tr(*key);
            if let Some(v) = shell::value_row(
                ui,
                r,
                label,
                text,
                *norm,
                &theme,
                Id::new(("fusion_param", n)),
                None,
            ) {
                if self.random_on(ctx) {
                    self.stop_random(ctx);
                }
                self.begin_param(ctx, id);
                write_param(ctx, id, v);
            }
        }
    }

    // ===== 右侧栏 =====

    fn right_rail(&mut self, ctx: &PluginContext<FusionParams>, ui: &mut Ui) {
        let theme = self.theme;
        let lang = self.lang;

        shell::preset_heading(ui, &theme, lang);
        let labels = self.bank.grid_labels();
        let responses = shell::preset_grid(
            ui,
            &theme,
            lang,
            &labels,
            if self.drag_from.is_some() { self.drag_hover } else { None },
            "orm_fusion",
        );
        for (i, resp) in responses.into_iter().enumerate() {
            let abs = i + QUICK_COUNT;
            if resp.hovered {
                ui.ctx().set_cursor_icon(CursorIcon::PointingHand);
            }
            if resp.drag_started {
                self.drag_from = Some(abs);
            }
            if resp.dragging {
                self.drag_hover = Some(abs);
            }
            if resp.clicked {
                self.load_slot(ctx, abs);
            }
            if resp.save_here {
                self.bank.save(abs, snapshot(&ctx.params()));
            }
            if resp.secondary {
                self.menu = Some((
                    MenuKind::Preset(abs),
                    ui.ctx().pointer_latest_pos().unwrap_or(Pos2::new(740.0, 56.0)),
                ));
                self.menu_armed = false;
            }
        }

        let morph = shell::morph_bar(ui, &theme, lang, self.fade_time, "orm_fusion");
        if let Some(v) = morph.value {
            self.fade_time = shell::morph_seconds(v);
        }

        // RANDOM：与 BandPass 完全共用的标题、色块、范围、速度
        let color = self.random_color(ctx);
        let on = self.random_on(ctx);
        let swatch = shell::random_header(
            ui,
            &theme,
            lang,
            color,
            on,
            "orm_fusion",
            Some(self.tr(Key::TipRandomSwatch)),
        );
        if swatch.clicked {
            self.toggle_random(ctx);
        }
        if swatch.secondary {
            self.menu = Some((
                MenuKind::RandomColor,
                ui.ctx().pointer_latest_pos().unwrap_or(Pos2::new(870.0, 260.0)),
            ));
            self.menu_armed = false;
        }

        let x0 = shell::layout::COL1_X;
        let x1 = shell::layout::PANEL_R;
        let amount = ctx.params().get_plain(P_RANDOM_RANGE).unwrap_or(0.35);
        let range_text = format!("{:.0}%", amount * 100.0);
        let range = widgets::hbar(
            ui,
            rect(x0, 262.0, x1, 302.0),
            &BarModel {
                label: self.tr(Key::Range),
                value_text: &range_text,
                norm: amount,
                swatch_left: Some(color),
                random: None,
                label_regular: true,
                tip: Some(self.tr(Key::TipRange)),
            },
            &theme,
            Id::new("fusion_random_range"),
        );
        if range.began {
            self.history.push(self.last_idle.clone());
        }
        if range.color_picker {
            self.menu = Some((
                MenuKind::RandomColor,
                ui.ctx().pointer_latest_pos().unwrap_or(Pos2::new(870.0, 270.0)),
            ));
            self.menu_armed = false;
        }
        if let Some(v) = range.value {
            write_param(ctx, P_RANDOM_RANGE, v);
        }

        let speed = ctx.params().get_plain(P_RANDOM_SPEED).unwrap_or(0.4);
        let period = 0.05 + 19.95 * speed;
        let speed_text = format!("{period:.2}s");
        let sp = widgets::hbar(
            ui,
            rect(x0, 306.0, x1, 346.0),
            &BarModel {
                label: self.tr(Key::Speed),
                value_text: &speed_text,
                norm: speed,
                swatch_left: Some(color),
                random: None,
                label_regular: true,
                tip: Some(self.tr(Key::TipSpeed)),
            },
            &theme,
            Id::new("fusion_random_speed"),
        );
        if sp.began {
            self.history.push(self.last_idle.clone());
        }
        if sp.color_picker {
            self.menu = Some((
                MenuKind::RandomColor,
                ui.ctx().pointer_latest_pos().unwrap_or(Pos2::new(870.0, 315.0)),
            ));
            self.menu_armed = false;
        }
        if let Some(v) = sp.value {
            write_param(ctx, P_RANDOM_SPEED, v);
        }

        // MIX
        let mix = ctx.params().get_plain(P_MIX).unwrap_or(1.0);
        let mix_text = format!("{:.0}%", mix * 100.0);
        let m = widgets::hbar(
            ui,
            rect(x0, 354.0, x1, 394.0),
            &BarModel {
                label: self.tr(Key::Mix),
                value_text: &mix_text,
                norm: mix,
                swatch_left: None,
                random: None,
                label_regular: false,
                tip: Some(self.tr(Key::TipMix)),
            },
            &theme,
            Id::new("fusion_mix"),
        );
        if m.began {
            self.history.push(self.last_idle.clone());
        }
        if let Some(v) = m.value {
            write_param(ctx, P_MIX, v);
        }

        if let Some(icon) = shell::icon_row(ui, &theme, lang, "orm_fusion") {
            match icon {
                Icon::Undo => self.undo(ctx),
                Icon::Redo => self.redo(ctx),
                Icon::Save => self.save_orm(ctx),
                Icon::Load => self.load_orm(ctx),
            }
        }
        if shell::title_block(ui, &theme, "Fusion", env!("CARGO_PKG_VERSION"), "orm_fusion") {
            self.settings_open = true;
        }
    }

    // ===== quick 槽 + fade =====

    fn quick(&mut self, ctx: &PluginContext<FusionParams>, ui: &mut Ui) {
        let labels = self.bank.quick_labels();
        let responses = shell::quick_row(ui, &self.theme, self.lang, &labels, "orm_fusion");
        for (i, resp) in responses.into_iter().enumerate() {
            if resp.clicked {
                self.load_slot(ctx, i);
            }
            if resp.save_here {
                self.bank.save(i, snapshot(&ctx.params()));
            }
        }

        let pos = self.bank.quick as f64 / (QUICK_COUNT - 1) as f64;
        let fedit = widgets::fade_slider(
            ui,
            rect(
                shell::layout::FADE_L,
                shell::layout::FADE_Y0,
                shell::layout::FADE_R,
                shell::layout::FADE_Y1,
            ),
            pos,
            QUICK_COUNT,
            &self.theme,
            Id::new("fusion_fade"),
            Some(self.tr(Key::TipFade)),
        );
        if fedit.began {
            self.history.push(snapshot(&ctx.params()));
        }
        if let Some(v) = fedit.value {
            self.stop_random(ctx);
            let q = (v * (QUICK_COUNT - 1) as f64).round() as usize;
            self.bank.quick = q.min(QUICK_COUNT - 1);
            let a = self.bank.quick;
            let Some(target) = self.bank.slot(a).cloned() else {
                return;
            };
            let before = snapshot(&ctx.params());
            self.fade = Some(Fade::new(before, target, self.fade_time));
            self.bank.current = a;
        }
    }

    // ===== 弹出菜单 =====

    fn menu(&mut self, ctx: &PluginContext<FusionParams>, ui: &mut Ui) {
        let Some((kind, pos)) = self.menu else { return };
        let mut keep = true;
        let just_opened = !self.menu_armed;
        self.menu_armed = true;
        let mut save_here = false;
        let mut restore = false;
        match kind {
            MenuKind::Preset(idx) => {
                egui::Area::new(Id::new("orm_fusion_menu"))
                    .order(egui::Order::Foreground)
                    .fixed_pos(pos)
                    .show(ui.ctx(), |ui| {
                        egui::Frame::popup(ui.style()).show(ui, |ui| {
                            ui.set_min_width(150.0);
                            if ui.button(self.tr(Key::SaveHere)).clicked() {
                                save_here = true;
                                keep = false;
                            }
                            if ui.button(self.tr(Key::RestoreDefault)).clicked() {
                                restore = true;
                                keep = false;
                            }
                        });
                    });
                if save_here {
                    self.bank.save(idx, snapshot(&ctx.params()));
                }
                if restore {
                    let defaults = defaults();
                    self.bank.slots[self.bank.numbers[idx] - 1] = Some(defaults);
                }
            }
            MenuKind::RandomColor => {
                let current = self.random_color(ctx);
                if let Some(c) = shell::color_menu(ui, pos, current, "orm_fusion") {
                    write_param(ctx, P_RANDOM_COLOR, c as f64);
                    keep = false;
                }
            }
        }
        if keep && !just_opened && ui.ctx().input(|i| i.pointer.any_click()) {
            keep = false;
        }
        self.menu = if keep { Some((kind, pos)) } else { None };
        if !keep {
            self.menu_armed = false;
        }
    }

    // ===== 设置面板 =====

    fn settings(&mut self, ctx: &PluginContext<FusionParams>, ui: &mut Ui) {
        if !self.settings_open {
            return;
        }
        let current = ctx.params().get_plain(P_BANDS).unwrap_or(0.0).round() as usize;
        let labels = ["256", "512", "1k", "2k", "4k", "8k", "16k", "32k", "64k", "131k"];
        let buttons: Vec<(&str, bool)> = labels
            .iter()
            .enumerate()
            .map(|(i, l)| (*l, i == current))
            .collect();
        let hint = if self.lang == Lang::Zh {
            "切换会重置频谱历史与重叠尾音"
        } else {
            "Switching resets the spectral history and OLA tail"
        };
        let fft_title = strings::tr(self.lang, Key::FftSize);
        let res = shell::settings_card(
            ui,
            &mut self.theme,
            &mut self.lang,
            Some(shell::SettingsExtras {
                title: fft_title,
                buttons: &buttons,
                columns: 5,
                hint: Some(hint),
            }),
            "orm_fusion",
        );
        if let Some(i) = res.extra {
            self.history.push(snapshot(&ctx.params()));
            click_param(ctx, P_BANDS, i as f64);
        }
        if res.changed || res.closed {
            shell::save_settings("ORMFusion.settings", self.lang, &self.theme);
        }
        if res.closed {
            self.settings_open = false;
        }
    }
}

fn nearest_band(views: &[BarView], plot: Rect, pos: Pos2) -> Option<usize> {
    let mut best = None;
    let mut best_d = 16.0f32;
    for (i, b) in views.iter().enumerate() {
        let (f1, f2, a1, a2) = bar_endpoints(b);
        let d = distance_to_segment(pos, pad_point(plot, f1, a1), pad_point(plot, f2, a2));
        if d < best_d {
            best_d = d;
            best = Some(i);
        }
    }
    best
}

impl EditorUi<FusionParams> for FusionUi {
    fn ui(&mut self, ui: &mut Ui, ctx: &PluginContext<FusionParams>) {
        let now = ui.input(|i| i.time);
        let dt = if self.last_time > 0.0 {
            (now - self.last_time).min(0.1)
        } else {
            0.0
        };
        self.last_time = now;

        self.ensure_style(ui);
        ui.set_min_size(Vec2::new(W, H));
        let full = rect(0.0, 0.0, W, H);
        ui.allocate_rect(full, Sense::hover());
        ui.painter_at(full).rect_filled(full, 0, self.theme.col_100());

        self.tick_fade(ctx);
        self.animate_random(ctx, dt);

        if let Some(from) = self.drag_from {
            if !ui.input(|i| i.pointer.any_down()) {
                if let Some(to) = self.drag_hover {
                    if from != to {
                        self.bank.swap(from, to);
                    }
                }
                self.drag_from = None;
                self.drag_hover = None;
            }
        }
        let rnd = self.random_on(ctx);
        if !ui.input(|i| i.pointer.any_down()) && !rnd {
            self.last_idle = snapshot(&ctx.params());
        }
        if rnd || self.fade.is_some() {
            // 随机目标球/预设渐变是连续动画，必须持续请求下一帧。
            ui.ctx().request_repaint();
        }

        self.header(ctx, ui);
        self.display(ctx, ui);
        self.params(ctx, ui);
        self.right_rail(ctx, ui);
        self.quick(ctx, ui);
        self.menu(ctx, ui);
        self.settings(ctx, ui);
        self.finish_edits(ui, ctx);
    }

    fn state_changed(&mut self, ctx: &PluginContext<FusionParams>) {
        if !self.random_on(ctx) {
            self.last_idle = snapshot(&ctx.params());
        }
    }
}

pub fn create(params: Arc<FusionParams>) -> Box<dyn Editor> {
    let ui = FusionUi::default();
    let background = ui.theme.col_100();
    let editor = truce_egui::EguiEditor::with_ui(params, (W as u32, H as u32), ui)
        .with_font(fonts::REGULAR)
        .resizable(false)
        .into_editor();
    openrm_ui::flash::guard(editor, background)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn factory_bank_is_full_and_well_formed() {
        let bank = factory_presets();
        assert_eq!(bank.len(), SLOT_COUNT);
        for patch in &bank {
            assert_eq!(patch.len(), PARAM_COUNT);
        }
    }

    #[test]
    fn preset_bank_roundtrips_24_slots() {
        let bank = PresetBank::from_json(&factory_presets()
            .into_iter()
            .map(Some)
            .fold(
                serde_json::json!({"numbers": (1..=24).collect::<Vec<_>>(), "slots": []}),
                |mut j, s| {
                    j["slots"].as_array_mut().unwrap().push(serde_json::json!(s.unwrap()));
                    j
                },
            ), PARAM_COUNT)
            .expect("24 slots");
        assert_eq!(bank.slots.len(), SLOT_COUNT);
        assert_eq!(bank.numbers[23], 24);
        assert!(bank.slot(9).is_some());
    }

    #[test]
    fn endpoints_stay_in_display_domain() {
        let b = BarView {
            freq: 0.9,
            length: 0.2,
            angle: 0.5,
            delay: 0.95,
            ..Default::default()
        };
        let (f1, f2, a1, a2) = bar_endpoints(&b);
        assert!((0.0..=1.0).contains(&f1));
        assert!((0.0..=1.0).contains(&f2));
        assert!((0.0..=1.0).contains(&a1));
        assert!((0.0..=1.0).contains(&a2));
    }
}
