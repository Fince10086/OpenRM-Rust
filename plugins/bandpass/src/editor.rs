//! BandPass 的 egui 界面：916×646 绝对坐标，与 C++ 版逐像素对齐。
//!
//! 布局常量全部取自 `plugins/BandPass/src/BandPass.cpp` 的 `Layout()`，
//! 控件行为取自 `plugins/common/controls/*.h`。任何"看起来差不多"的近似值
//! 都在注释里标了原版常量名，便于日后核对。

use std::path::PathBuf;
use std::sync::Arc;

use egui::{Align2, Color32, Id, Pos2, Rect, Sense, Vec2};
use openrm_ui::{
    fonts,
    strings::{self, Key, Lang},
    theme::{Theme, ThemeMode},
    widgets::{self, BandEdit, BandModel, BarModel, GainModel, Icon, PadModel},
    UI_HEIGHT, UI_WIDTH,
};
use truce::prelude::*;
use truce_egui::EditorUi;

use crate::params::{BandPassParams, BandPassParamsParamId as P};
use openrm_dsp::spectrum::{PadSpectrum, SPECTRUM_BANDS};

const SPEC_FFT: usize = 4096;
const SPEC_OVERLAP: usize = 4;
const SPEC_HOP: usize = SPEC_FFT / SPEC_OVERLAP;

const SLOT_COUNT: usize = 24;
const QUICK_COUNT: usize = 8;
const PARAM_COUNT: usize = 34;

// ===== 布局常量（BandPass.cpp） =====
const PAD_L: (f32, f32, f32, f32) = (20.0, 30.0, 668.0, 210.0);
const BAND_L: (f32, f32, f32, f32) = (20.0, 216.0, 668.0, 262.0);
const GAIN_L: (f32, f32, f32, f32) = (672.0, 30.0, 730.0, 210.0);
const PAD_R: (f32, f32, f32, f32) = (20.0, 288.0, 668.0, 468.0);
const BAND_R: (f32, f32, f32, f32) = (20.0, 474.0, 668.0, 520.0);
const GAIN_R: (f32, f32, f32, f32) = (672.0, 288.0, 730.0, 468.0);

const COL1_X: f32 = 740.0;
const COL2_X: f32 = 820.0;
const BTN_W: f32 = 76.0;
const PANEL_R: f32 = 896.0;
const GRID_Y: f32 = 56.0;
const CELL_W: f32 = 36.0;
const CELL_H: f32 = 29.0;
const CELL_STEP_X: f32 = 40.0;
const CELL_STEP_Y: f32 = 33.0;

// 底部 quick 槽刻度：原版 mQuickTicks 中心在 39.5..609 之间
const QUICK_X0: f32 = 39.5;
const QUICK_X1: f32 = 609.0;
const QUICK_W: f32 = 36.0;
const QUICK_Y: f32 = 552.0;
const QUICK_H: f32 = 26.0;

const FADE_L: f32 = 31.5;
const FADE_R: f32 = 656.5;
const FADE_Y0: f32 = 592.0;
const FADE_Y1: f32 = 616.0;

fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
    Rect::from_min_max(Pos2::new(x0, y0), Pos2::new(x1, y1))
}

fn r4(t: (f32, f32, f32, f32)) -> Rect {
    rect(t.0, t.1, t.2, t.3)
}

fn pid(id: P) -> u32 {
    id.into()
}

// ===== 参数快照（原始单位，与 `.orm` 文件里存的数组一致） =====

fn snapshot(p: &BandPassParams) -> Vec<f64> {
    (0..p.count() as u32)
        .map(|i| p.get_plain(i).unwrap_or(0.0))
        .collect()
}

fn set_plain(ctx: &PluginContext<BandPassParams>, id: u32, v: f64) {
    let p = ctx.params();
    p.set_plain(id, v);
    if let Some(n) = p.get_normalized(id) {
        ctx.automate(id, n);
    }
}

fn apply_snapshot(ctx: &PluginContext<BandPassParams>, s: &[f64]) {
    let p = ctx.params();
    for (i, v) in s.iter().enumerate().take(PARAM_COUNT) {
        p.set_plain(i as u32, *v);
    }
    for (i, _) in s.iter().enumerate().take(PARAM_COUNT) {
        if let Some(n) = p.get_normalized(i as u32) {
            ctx.automate(i as u32, n);
        }
    }
}

// ===== 默认值与工厂预设 =====

fn defaults() -> Vec<f64> {
    let mut v = vec![0.0; PARAM_COUNT];
    v[pid(P::FreqL) as usize] = (300.0f64 * 4000.0).sqrt();
    v[pid(P::FreqR) as usize] = (300.0f64 * 4000.0).sqrt();
    v[pid(P::BwL) as usize] = 3.65;
    v[pid(P::BwR) as usize] = 3.65;
    v[pid(P::GainL) as usize] = 0.0;
    v[pid(P::GainR) as usize] = 0.0;
    v[pid(P::Mix) as usize] = 1.0;
    v[pid(P::RandomAmountR) as usize] = 0.5;
    v[pid(P::RandomAmountY) as usize] = 0.3;
    v[pid(P::RandomAmountB) as usize] = 0.6;
    v[pid(P::RandomAmountG) as usize] = 0.4;
    v[pid(P::RandomRateR) as usize] = 0.5;
    v[pid(P::RandomRateY) as usize] = 1.0;
    v[pid(P::RandomRateB) as usize] = 0.75;
    v[pid(P::RandomRateG) as usize] = 2.0;
    v[pid(P::SlopeL) as usize] = 3.0; // 0..3 → 96 dB/oct（索引 3）
    v[pid(P::SlopeR) as usize] = 3.0;
    v[pid(P::PassL) as usize] = 1.0;
    v[pid(P::PassR) as usize] = 1.0;
    v
}

fn factory_presets() -> Vec<Vec<f64>> {
    // 频带参数按下表覆盖；名称只是给未来的 UI 用，原版槽位只显示序号。
    const BANDS: [(f64, f64, f64, f64); 14] = [
        (300.0, 4000.0, 300.0, 4000.0),
        (23.0, 200.0, 23.0, 200.0),
        (23.0, 1000.0, 1000.0, 22050.0),
        (1000.0, 22050.0, 23.0, 1000.0),
        (5000.0, 22050.0, 5000.0, 22050.0),
        (400.0, 4000.0, 400.0, 4000.0),
        (300.0, 300.0, 300.0, 300.0),
        (6000.0, 6000.0, 6000.0, 6000.0),
        (6102.0, 22050.0, 6102.0, 22050.0),
        (467.0, 557.0, 467.0, 557.0),
        (77.0, 5626.0, 77.0, 5626.0),
        (7784.0, 9155.0, 7784.0, 9155.0),
        (4982.0, 12327.0, 4982.0, 12327.0),
        (254.0, 329.0, 254.0, 329.0),
    ];
    let mut list = Vec::with_capacity(SLOT_COUNT);
    for i in 0..SLOT_COUNT {
        let mut v = defaults();
        if i < BANDS.len() {
            let b = BANDS[i];
            v[pid(P::FreqL) as usize] = b.0;
            v[pid(P::FreqR) as usize] = b.1;
            v[pid(P::BwL) as usize] = b.2;
            v[pid(P::BwR) as usize] = b.3;
            v[pid(P::Link) as usize] = 0.0;
        }
        list.push(v);
    }
    list
}

// ===== 频谱管线 =====

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
            // 原版右垫的干信号取左声道（既有行为，保持复刻）
            self.r.push_hop(&dry, &wet_r);
            for ch in 0..2 {
                self.curves_l[ch] = self.l.curve(ch);
                self.curves_r[ch] = self.r.curve(ch);
            }
            let _ = SPECTRUM_BANDS;
        }
    }
}

// ===== 弹出菜单 / 文本输入 =====

#[derive(Clone, Copy)]
enum MenuKind {
    Slope(bool),
    Color { target: ColorTarget },
    RandomPicker,
    Preset(usize),
}

#[derive(Clone, Copy)]
enum ColorTarget {
    Freq(bool),
    Bw(bool),
    Gain(bool),
    Mix,
}

impl ColorTarget {
    fn color_id(self) -> u32 {
        match self {
            ColorTarget::Freq(true) => pid(P::RandomColorFreqL),
            ColorTarget::Freq(false) => pid(P::RandomColorFreqR),
            ColorTarget::Bw(true) => pid(P::RandomColorBwL),
            ColorTarget::Bw(false) => pid(P::RandomColorBwR),
            ColorTarget::Gain(true) => pid(P::RandomColorGainL),
            ColorTarget::Gain(false) => pid(P::RandomColorGainR),
            ColorTarget::Mix => pid(P::RandomColorMix),
        }
    }
    fn enable_id(self) -> u32 {
        match self {
            ColorTarget::Freq(true) => pid(P::RandomEnableFreqL),
            ColorTarget::Freq(false) => pid(P::RandomEnableFreqR),
            ColorTarget::Bw(true) => pid(P::RandomEnableBwL),
            ColorTarget::Bw(false) => pid(P::RandomEnableBwR),
            ColorTarget::Gain(true) => pid(P::RandomEnableGainL),
            ColorTarget::Gain(false) => pid(P::RandomEnableGainR),
            ColorTarget::Mix => pid(P::RandomEnableMix),
        }
    }
}

#[derive(Clone)]
struct TextInput {
    id: u32,
    buf: String,
    /// 提交时的单位解析方式
    kind: TextKind,
    rect: Rect,
}

/// 解析文本输入：支持 `1k`、`1kHz`、`300 Hz` 这类写法（原版 ParseFreq）。
fn parse_number(s: &str) -> Option<f64> {
    let t = s.trim().to_ascii_lowercase();
    let t = t.strip_suffix("hz").unwrap_or(&t).trim();
    let (num, mult) = if let Some(rest) = t.strip_suffix('k') {
        (rest.trim(), 1000.0)
    } else {
        (t, 1.0)
    };
    num.parse::<f64>().ok().map(|v| v * mult)
}

#[derive(Clone, Copy, PartialEq)]
enum TextKind {
    Gain,
    Mix,
    Morph,
    LowCut(bool),
    HighCut(bool),
    Fade,
    /// 中心频率（bool: 是否左声道）
    Freq(bool),
    /// 带宽倍率（bool: 是否左声道）
    Bw(bool),
}

struct Fade {
    from: Vec<f64>,
    to: Vec<f64>,
    start: std::time::Instant,
    dur: f64,
}

// ===== 设置文件 =====

struct Settings {
    lang: Lang,
    hue: u16,
    sat: u8,
    dark: bool,
}

fn settings_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join("Library/Application Support/OpenRM/ORMBandPass.settings")
}

fn load_settings() -> Settings {
    let mut s = Settings {
        lang: strings::detect_lang(),
        hue: 45,
        sat: 15,
        dark: false,
    };
    if let Ok(txt) = std::fs::read_to_string(settings_path()) {
        if let Ok(j) = serde_json::from_str::<serde_json::Value>(&txt) {
            if let Some(v) = j.get("lang").and_then(|v| v.as_i64()) {
                s.lang = if v == 1 { Lang::Zh } else { Lang::En };
            }
            if let Some(v) = j.get("hue").and_then(|v| v.as_i64()) {
                s.hue = v as u16;
            }
            if let Some(v) = j.get("satMax").and_then(|v| v.as_i64()) {
                s.sat = v as u8;
            }
            if let Some(v) = j.get("themeMode").and_then(|v| v.as_i64()) {
                s.dark = v == 1;
            }
        }
    }
    s
}

fn save_settings(s: &Settings) {
    let path = settings_path();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let j = serde_json::json!({
        "lang": if s.lang == Lang::Zh { 1 } else { 0 },
        "hue": s.hue,
        "satMax": s.sat,
        "themeMode": if s.dark { 1 } else { 0 },
    });
    if let Ok(txt) = serde_json::to_string_pretty(&j) {
        let _ = std::fs::write(path, txt);
    }
}

// ===== UI 状态 =====

pub struct BandPassUi {
    theme: Theme,
    lang: Lang,
    styled: bool,
    font_stage: u8,
    spectrum: Option<SpectrumUi>,
    slots: Vec<Vec<f64>>,
    numbers: Vec<usize>,
    current: usize,
    quick: usize,
    fade_time: f64,
    fade: Option<Fade>,
    undo: Vec<Vec<f64>>,
    redo: Vec<Vec<f64>>,
    last_idle: Vec<f64>,
    selected_color: usize,
    menu: Option<(MenuKind, Pos2)>,
    drag_from: Option<usize>,
    drag_hover: Option<usize>,
    settings_open: bool,
    text_input: Option<TextInput>,
}

impl Default for BandPassUi {
    fn default() -> Self {
        let s = load_settings();
        let theme = Theme {
            mode: if s.dark {
                ThemeMode::Dark
            } else {
                ThemeMode::Light
            },
            hue: s.hue,
            sat_max: s.sat,
        };
        let slots = factory_presets();
        let mut numbers = vec![0; SLOT_COUNT];
        for (i, n) in numbers.iter_mut().enumerate() {
            *n = i + 1;
        }
        Self {
            theme,
            lang: s.lang,
            styled: false,
            font_stage: 0,
            spectrum: None,
            slots,
            numbers,
            current: 0,
            quick: 0,
            fade_time: 0.25,
            fade: None,
            undo: Vec::new(),
            redo: Vec::new(),
            last_idle: defaults(),
            selected_color: 0,
            menu: None,
            drag_from: None,
            drag_hover: None,
            settings_open: false,
            text_input: None,
        }
    }
}

impl BandPassUi {
    fn ensure_style(&mut self, ui: &mut egui::Ui) {
        // egui 的字体定义要等到下一帧的 begin_pass 才生效：首帧装字体，
        // 次帧才打开命名族（否则 Name 家族不存在会 panic）。
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
        let mut style = (*ui.ctx().style()).clone();
        style.spacing.item_spacing = Vec2::ZERO;
        style.spacing.button_padding = Vec2::ZERO;
        ui.ctx().set_style(style);
    }

    fn tr(&self, key: Key) -> &'static str {
        strings::tr(self.lang, key)
    }

    fn push_undo(&mut self, snap: Vec<f64>) {
        self.undo.push(snap);
        if self.undo.len() > 100 {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    fn load_slot(&mut self, ctx: &PluginContext<BandPassParams>, idx: usize) {
        if idx >= SLOT_COUNT {
            return;
        }
        let target = self.slots[self.numbers[idx] - 1].clone();
        let from = snapshot(&ctx.params());
        self.push_undo(from.clone());
        self.current = idx;
        self.quick = idx.min(QUICK_COUNT - 1);
        self.start_fade(from, target);
    }

    fn start_fade(&mut self, from: Vec<f64>, to: Vec<f64>) {
        let dur = self.fade_time.max(0.01);
        self.fade = Some(Fade {
            from,
            to,
            start: std::time::Instant::now(),
            dur,
        });
    }

    fn tick_fade(&mut self, ctx: &PluginContext<BandPassParams>) {
        let Some(f) = self.fade.as_ref() else { return };
        let t = (f.start.elapsed().as_secs_f64() / f.dur).clamp(0.0, 1.0);
        if t >= 1.0 {
            let to = f.to.clone();
            apply_snapshot(ctx, &to);
            self.fade = None;
            return;
        }
        // 原版 MixSnapshots 在归一化空间插值（频率/带宽/速度都是 ShapeExp），
        // 直接插 plain 会让 fade 的中间频点偏亮。
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

    fn copy_lr(&mut self, ctx: &PluginContext<BandPassParams>, left_to_right: bool) {
        let before = snapshot(&ctx.params());
        self.push_undo(before);
        let (src, dst) = if left_to_right {
            (P::FreqL, P::FreqR)
        } else {
            (P::FreqR, P::FreqL)
        };
        set_plain(
            ctx,
            pid(dst),
            ctx.params().get_plain(pid(src)).unwrap_or(1000.0),
        );
        let (src, dst) = if left_to_right {
            (P::BwL, P::BwR)
        } else {
            (P::BwR, P::BwL)
        };
        set_plain(
            ctx,
            pid(dst),
            ctx.params().get_plain(pid(src)).unwrap_or(3.65),
        );
        let (src, dst) = if left_to_right {
            (P::GainL, P::GainR)
        } else {
            (P::GainR, P::GainL)
        };
        set_plain(
            ctx,
            pid(dst),
            ctx.params().get_plain(pid(src)).unwrap_or(0.0),
        );
    }

    fn flip(&mut self, ctx: &PluginContext<BandPassParams>) {
        let before = snapshot(&ctx.params());
        self.push_undo(before);
        let p = ctx.params();
        let fl = p.get_plain(pid(P::FreqL)).unwrap_or(1000.0);
        let fr = p.get_plain(pid(P::FreqR)).unwrap_or(1000.0);
        let bl = p.get_plain(pid(P::BwL)).unwrap_or(3.65);
        let br = p.get_plain(pid(P::BwR)).unwrap_or(3.65);
        let gl = p.get_plain(pid(P::GainL)).unwrap_or(0.0);
        let gr = p.get_plain(pid(P::GainR)).unwrap_or(0.0);
        set_plain(ctx, pid(P::FreqL), fr);
        set_plain(ctx, pid(P::FreqR), fl);
        set_plain(ctx, pid(P::BwL), br);
        set_plain(ctx, pid(P::BwR), bl);
        set_plain(ctx, pid(P::GainL), gr);
        set_plain(ctx, pid(P::GainR), gl);
    }

    fn undo(&mut self, ctx: &PluginContext<BandPassParams>) {
        if let Some(prev) = self.undo.pop() {
            let cur = snapshot(&ctx.params());
            self.redo.push(cur);
            apply_snapshot(ctx, &prev);
        }
    }

    fn redo(&mut self, ctx: &PluginContext<BandPassParams>) {
        if let Some(next) = self.redo.pop() {
            let cur = snapshot(&ctx.params());
            self.undo.push(cur);
            apply_snapshot(ctx, &next);
        }
    }

    fn save_orm(&mut self, ctx: &PluginContext<BandPassParams>) {
        let file = rfd::FileDialog::new()
            .add_filter("ORM preset", &["orm"])
            .set_file_name("ORMBandPass.orm")
            .save_file();
        let Some(path) = file else { return };
        let j = serde_json::json!({
            "version": 3,
            "presets": self.slots,
            "currentPreset": self.current,
            "currentValues": snapshot(&ctx.params()),
            "fadePos": self.quick as f64 / 7.0,
        });
        let text = format!(
            "bandpass\n{}",
            serde_json::to_string_pretty(&j).unwrap_or_default()
        );
        let _ = std::fs::write(path, text);
    }

    fn load_orm(&mut self, ctx: &PluginContext<BandPassParams>) {
        let file = rfd::FileDialog::new()
            .add_filter("ORM preset", &["orm"])
            .pick_file();
        let Some(path) = file else { return };
        let Ok(text) = std::fs::read_to_string(path) else {
            return;
        };
        let Some(body) = text.strip_prefix("bandpass") else {
            return;
        };
        let body = body.trim_start_matches(['\r', '\n']);
        let Ok(j) = serde_json::from_str::<serde_json::Value>(body) else {
            return;
        };
        if j.get("version").and_then(|v| v.as_i64()) != Some(3) {
            return;
        }
        let Some(list) = j.get("presets").and_then(|v| v.as_array()) else {
            return;
        };
        let mut slots = Vec::new();
        for item in list {
            let Some(arr) = item.as_array() else { continue };
            if arr.len() != 34 {
                continue;
            }
            slots.push(
                arr.iter()
                    .map(|v| v.as_f64().unwrap_or(0.0))
                    .collect::<Vec<_>>(),
            );
        }
        if slots != vec![Vec::<f64>::new(); 0] && slots.len() == SLOT_COUNT {
            self.slots = slots;
        }
        if let Some(cur) = j.get("currentPreset").and_then(|v| v.as_i64()) {
            self.current = cur.clamp(0, (SLOT_COUNT - 1) as i64) as usize;
        }
        if let Some(vals) = j.get("currentValues").and_then(|v| v.as_array()) {
            if vals.len() == 34 {
                let v: Vec<f64> = vals.iter().map(|v| v.as_f64().unwrap_or(0.0)).collect();
                let before = snapshot(&ctx.params());
                self.push_undo(before);
                apply_snapshot(ctx, &v);
            }
        }
    }
}

impl EditorUi<BandPassParams> for BandPassUi {
    fn ui(&mut self, ui: &mut egui::Ui, ctx: &PluginContext<BandPassParams>) {
        self.ensure_style(ui);

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
        self.tick_fade(ctx);

        let p = ctx.params();
        let theme = self.theme;
        let lang = self.lang;
        let tr = |k: Key| strings::tr(lang, k);

        let (full, _) = ui.allocate_exact_size(Vec2::new(UI_WIDTH, UI_HEIGHT), Sense::hover());
        ui.painter_at(full)
            .rect_filled(full, 0, theme.warm_gray(238));

        // 拖拽状态（预设槽交换）
        if let Some(from) = self.drag_from {
            if !ui.input(|i| i.pointer.any_down()) {
                if let Some(to) = self.drag_hover {
                    if from != to {
                        self.slots.swap(from, to);
                        self.numbers.swap(from, to);
                    }
                }
                self.drag_from = None;
                self.drag_hover = None;
            }
        }
        if !ui.input(|i| i.pointer.any_down()) {
            self.last_idle = snapshot(&p);
        }

        // ================= 左半：两个滤波垫 + 带宽滑杆 + 增益条 =================
        for (is_l, pad_t, band_t, gain_t) in [
            (true, PAD_L, BAND_L, GAIN_L),
            (false, PAD_R, BAND_R, GAIN_R),
        ] {
            let freq = if is_l {
                p.freq_l.raw_target()
            } else {
                p.freq_r.raw_target()
            };
            let bw = if is_l {
                p.bw_l.raw_target()
            } else {
                p.bw_r.raw_target()
            };
            let pass = if is_l {
                p.pass_l.value()
            } else {
                p.pass_r.value()
            };
            let slope_idx = if is_l {
                p.slope_l.index()
            } else {
                p.slope_r.index()
            };
            let slope_db = [12, 24, 48, 96][slope_idx.min(3) as usize];
            let (color_f, color_b) = if is_l {
                (p.random_color_freq_l.index(), p.random_color_bw_l.index())
            } else {
                (p.random_color_freq_r.index(), p.random_color_bw_r.index())
            };
            let en_freq = if is_l {
                p.random_enable_freq_l.value()
            } else {
                p.random_enable_freq_r.value()
            };
            let en_bw = if is_l {
                p.random_enable_bw_l.value()
            } else {
                p.random_enable_bw_r.value()
            };

            let spec = self.spectrum.as_ref();
            let (sin, sout): (&[(f32, f32)], &[(f32, f32)]) = match (spec, is_l) {
                (Some(sp), true) => (&sp.curves_l[0], &sp.curves_l[1]),
                (Some(sp), false) => (&sp.curves_r[0], &sp.curves_r[1]),
                (None, _) => (&[], &[]),
            };
            let deltas = ctx.params().analysis.deltas();
            let ch = if is_l { 0 } else { 1 };
            let ghost = if en_freq || en_bw {
                Some((
                    if en_freq {
                        2f64.powf(deltas.freq_oct[ch] as f64)
                    } else {
                        1.0
                    },
                    if en_bw {
                        2f64.powf(deltas.bw_oct[ch] as f64 * 0.5)
                    } else {
                        1.0
                    },
                    color_f.max(color_b) as usize,
                ))
            } else {
                None
            };
            let model = PadModel {
                center_prefix: tr(Key::Center),
                width_prefix: tr(Key::Bandwidth),
                slope_prefix: tr(Key::Slope),
                pass_label: if pass { tr(Key::Pass) } else { tr(Key::Reject) },
                side_label: if is_l { tr(Key::Left) } else { tr(Key::Right) },
                pass,
                freq,
                bw_mult: bw,
                slope_db,
                swatch_on: [en_freq, en_bw],
                swatch_color: [
                    Theme::random_color(color_f as usize),
                    Theme::random_color(color_b as usize),
                ],
                spec_in: sin,
                spec_out: sout,
                ghost,
                mono: ctx.params().analysis.n_out() < 2 && !is_l,
                tip: Some(tr(Key::TipBand)),
            };
            let edit = widgets::filter_pad(ui, r4(pad_t), &model, &theme, Id::new(("pad", is_l)));

            if edit.began {
                self.push_undo(self.last_idle.clone());
            }
            if let Some(f) = edit.freq {
                set_plain(ctx, pid(if is_l { P::FreqL } else { P::FreqR }), f);
                if p.link.value() {
                    set_plain(ctx, pid(if is_l { P::FreqR } else { P::FreqL }), f);
                }
            }
            if let Some(b) = edit.bw_mult {
                set_plain(ctx, pid(if is_l { P::BwL } else { P::BwR }), b);
                if p.link.value() {
                    set_plain(ctx, pid(if is_l { P::BwR } else { P::BwL }), b);
                }
            }
            if edit.toggle_pass {
                set_plain(
                    ctx,
                    pid(if is_l { P::PassL } else { P::PassR }),
                    if pass { 0.0 } else { 1.0 },
                );
            }
            if let Some(sl) = edit.slope {
                let _ = sl;
                self.menu = Some((
                    MenuKind::Slope(is_l),
                    ui.ctx()
                        .pointer_latest_pos()
                        .unwrap_or(r4(pad_t).left_top() + Vec2::new(180.0, 20.0)),
                ));
            }
            if let Some(i) = edit.toggle_random {
                let id = if i == 0 {
                    ColorTarget::Freq(is_l)
                } else {
                    ColorTarget::Bw(is_l)
                };
                let cur = ctx.params().get_plain(id.enable_id()).unwrap_or(0.0) > 0.5;
                set_plain(ctx, id.enable_id(), if cur { 0.0 } else { 1.0 });
            }
            if let Some(i) = edit.color_menu {
                let target = if i == 0 {
                    ColorTarget::Freq(is_l)
                } else {
                    ColorTarget::Bw(is_l)
                };
                self.menu = Some((
                    MenuKind::Color { target },
                    ui.ctx()
                        .pointer_latest_pos()
                        .unwrap_or(r4(pad_t).left_top() + Vec2::new(120.0, 20.0)),
                ));
            }
            if edit.edit_center {
                self.text_input = Some(TextInput {
                    id: pid(if is_l { P::FreqL } else { P::FreqR }),
                    buf: format!("{:.0}", freq),
                    kind: TextKind::Freq(is_l),
                    rect: Rect::from_center_size(
                        Pos2::new(r4(pad_t).left() + 120.0, r4(pad_t).top() + 11.0),
                        Vec2::new(110.0, 26.0),
                    ),
                });
            }
            if edit.edit_bw {
                self.text_input = Some(TextInput {
                    id: pid(if is_l { P::BwL } else { P::BwR }),
                    buf: format!("{:.2}", bw),
                    kind: TextKind::Bw(is_l),
                    rect: Rect::from_center_size(
                        Pos2::new(r4(pad_t).right() - 150.0, r4(pad_t).top() + 11.0),
                        Vec2::new(110.0, 26.0),
                    ),
                });
            }

            // 带宽滑杆
            let band = widgets::band_slider(
                ui,
                r4(band_t),
                &BandModel {
                    freq,
                    bw_mult: bw,
                    low_label: tr(Key::LowCut),
                    high_label: tr(Key::HighCut),
                    tip: Some(tr(Key::TipBand)),
                },
                &theme,
                Id::new(("band", is_l)),
            );
            let band_edit = band.edit_low;
            let high_edit = band.edit_high;
            self.apply_band(ctx, is_l, band);
            if band_edit {
                self.text_input = Some(TextInput {
                    id: pid(if is_l { P::BwL } else { P::BwR }),
                    buf: format!("{:.0}", (freq / bw.max(1.0)).max(20.0)),
                    kind: TextKind::LowCut(is_l),
                    rect: Rect::from_center_size(
                        Pos2::new(r4(band_t).left() + 90.0, r4(band_t).top() + 13.0),
                        Vec2::new(110.0, 26.0),
                    ),
                });
            }
            if high_edit {
                self.text_input = Some(TextInput {
                    id: pid(if is_l { P::BwL } else { P::BwR }),
                    buf: format!("{:.0}", (freq * bw).min(20500.0)),
                    kind: TextKind::HighCut(is_l),
                    rect: Rect::from_center_size(
                        Pos2::new(r4(band_t).right() - 90.0, r4(band_t).top() + 13.0),
                        Vec2::new(110.0, 26.0),
                    ),
                });
            }

            // 增益条
            let gain_db = if is_l {
                p.gain_l.raw_target()
            } else {
                p.gain_r.raw_target()
            };
            let en_gain = if is_l {
                p.random_enable_gain_l.value()
            } else {
                p.random_enable_gain_r.value()
            };
            let color_g = if is_l {
                p.random_color_gain_l.index()
            } else {
                p.random_color_gain_r.index()
            };
            let gedit = widgets::gain_slider(
                ui,
                r4(gain_t),
                &GainModel {
                    db: gain_db,
                    label: if is_l { tr(Key::GainL) } else { tr(Key::GainR) },
                    random_on: en_gain,
                    random_color: color_g as usize,
                    delta_db: deltas.gain_db[ch],
                    ghost: false,
                    tip: Some(tr(Key::TipGain)),
                },
                &theme,
                Id::new(("gain", is_l)),
            );
            if gedit.began {
                self.push_undo(self.last_idle.clone());
            }
            if let Some(db) = gedit.db {
                set_plain(ctx, pid(if is_l { P::GainL } else { P::GainR }), db);
            }
            if gedit.toggle_random {
                let cur = ctx
                    .params()
                    .get_plain(ColorTarget::Gain(is_l).enable_id())
                    .unwrap_or(0.0)
                    > 0.5;
                set_plain(
                    ctx,
                    ColorTarget::Gain(is_l).enable_id(),
                    if cur { 0.0 } else { 1.0 },
                );
            }
            if gedit.color_menu {
                self.menu = Some((
                    MenuKind::Color {
                        target: ColorTarget::Gain(is_l),
                    },
                    ui.ctx()
                        .pointer_latest_pos()
                        .unwrap_or(Pos2::new(700.0, 200.0)),
                ));
            }
            if gedit.edit_value {
                self.text_input = Some(TextInput {
                    id: pid(if is_l { P::GainL } else { P::GainR }),
                    buf: format!("{:.1}", gain_db),
                    kind: TextKind::Gain,
                    rect: Rect::from_center_size(
                        Pos2::new(r4(gain_t).right() - 18.0, r4(gain_t).top() + 120.0),
                        Vec2::new(80.0, 26.0),
                    ),
                });
            }
        }

        // ================= 右侧面板 =================
        // PRESETS 标题 + 分隔线
        {
            let pnt = ui.painter_at(full);
            let font = fonts::bold(20.0);
            pnt.text(
                Pos2::new(COL1_X, 41.0),
                Align2::LEFT_CENTER,
                tr(Key::Presets),
                font.clone(),
                theme.col_900(),
            );
            let w = pnt
                .layout_no_wrap(tr(Key::Presets).into(), font, theme.col_900())
                .size()
                .x;
            pnt.line_segment(
                [Pos2::new(COL1_X + w + 10.0, 41.0), Pos2::new(PANEL_R, 41.0)],
                theme.hairline(),
            );
        }

        // 24 个预设槽
        for i in 0..SLOT_COUNT - QUICK_COUNT {
            let row = i / 4;
            let col = i % 4;
            let x = COL1_X + col as f32 * CELL_STEP_X;
            let y = GRID_Y + row as f32 * CELL_STEP_Y;
            let r = rect(x, y, x + CELL_W, y + CELL_H);
            let label = format!("{}", self.numbers[i + QUICK_COUNT]);
            let drag_target = self.drag_from.is_some() && self.drag_hover == Some(i);
            let tip = format!(
                "{}\n{}\n{}\n{}",
                tr(Key::TipDrag),
                tr(Key::TipSaveHere),
                tr(Key::TipMenu),
                tr(Key::TipPresets)
            );
            let resp = widgets::preset_slot(
                ui,
                r,
                &label,
                drag_target,
                &theme,
                Id::new(("slot", i)),
                Some(tip.as_str()),
            );
            if resp.hovered {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if resp.drag_started {
                self.drag_from = Some(i);
            }
            if resp.dragging {
                self.drag_hover = Some(i);
            }
            if resp.clicked {
                self.load_slot(ctx, i + QUICK_COUNT);
            }
            if resp.save_here {
                let s = snapshot(&ctx.params());
                self.slots[i + QUICK_COUNT] = s;
                self.numbers[i + QUICK_COUNT] = i + 1 + QUICK_COUNT;
            }
            if resp.secondary {
                self.menu = Some((
                    MenuKind::Preset(i + QUICK_COUNT),
                    ui.ctx().pointer_latest_pos().unwrap_or(r.left_bottom()),
                ));
            }
        }

        // MORPH（预设渐变时间）
        let morph_norm = ((self.fade_time.max(0.01) / 0.01).ln() / 6000f64.ln()).clamp(0.0, 1.0);
        let morph_text = if self.fade_time < 1.0 {
            format!("{:.2}s", self.fade_time)
        } else {
            format!("{:.1}s", self.fade_time)
        };
        let morph = widgets::hbar(
            ui,
            rect(COL1_X, 188.0, PANEL_R, 230.0),
            &BarModel {
                label: tr(Key::Morph),
                value_text: &morph_text,
                norm: morph_norm,
                swatch_left: None,
                random: None,
                label_regular: false,
                tip: Some(tr(Key::TipMorph)),
            },
            &theme,
            Id::new("morph"),
        );
        if let Some(v) = morph.value {
            self.fade_time = 0.01 * 6000f64.powf(v);
        }
        if morph.edit_value {
            self.text_input = Some(TextInput {
                id: 0xFFFF_0001,
                buf: format!("{:.2}", self.fade_time),
                kind: TextKind::Morph,
                rect: rect(COL1_X, 216.0, COL1_X + 100.0, 244.0),
            });
        }

        // RANDOM 标题 + 选色块
        {
            let pnt = ui.painter_at(full);
            let font = fonts::bold(20.0);
            pnt.text(
                Pos2::new(COL1_X, 247.0),
                Align2::LEFT_CENTER,
                tr(Key::Random),
                font,
                theme.col_900(),
            );
            let sw = rect(881.0, 240.0, 895.0, 254.0);
            pnt.rect_filled(sw, 0, Theme::random_color(self.selected_color));
            let resp = ui.interact(sw, Id::new("rand_picker"), Sense::click());
            if resp.clicked() {
                self.menu = Some((
                    MenuKind::RandomPicker,
                    ui.ctx().pointer_latest_pos().unwrap_or(sw.left_bottom()),
                ));
            }
        }

        let ci = self.selected_color;
        let selected_txt = Theme::random_color(ci);
        let _ = selected_txt;

        // RANGE / SPEED（作用于当前选色）
        let (amount, rate, aid, rid) = match ci {
            0 => (
                p.random_amount_r.raw_target(),
                p.random_rate_r.raw_target(),
                P::RandomAmountR,
                P::RandomRateR,
            ),
            1 => (
                p.random_amount_y.raw_target(),
                p.random_rate_y.raw_target(),
                P::RandomAmountY,
                P::RandomRateY,
            ),
            2 => (
                p.random_amount_b.raw_target(),
                p.random_rate_b.raw_target(),
                P::RandomAmountB,
                P::RandomRateB,
            ),
            _ => (
                p.random_amount_g.raw_target(),
                p.random_rate_g.raw_target(),
                P::RandomAmountG,
                P::RandomRateG,
            ),
        };
        let range_text = format!("{:.0}%", amount * 100.0);
        let range = widgets::hbar(
            ui,
            rect(COL1_X, 264.0, PANEL_R, 306.0),
            &BarModel {
                label: tr(Key::Range),
                value_text: &range_text,
                norm: amount,
                swatch_left: Some(ci),
                random: None,
                label_regular: true,
                tip: Some(tr(Key::TipRange)),
            },
            &theme,
            Id::new("range"),
        );
        if range.began {
            self.push_undo(self.last_idle.clone());
        }
        if let Some(v) = range.value {
            set_plain(ctx, pid(aid), v);
        }
        if range.edit_value {
            self.text_input = Some(TextInput {
                id: pid(aid),
                buf: format!("{:.0}", amount * 100.0),
                kind: TextKind::Mix,
                rect: rect(COL1_X, 286.0, COL1_X + 100.0, 312.0),
            });
        }

        let rate_norm = ((rate.max(0.01) / 0.01).ln() / 6000f64.ln()).clamp(0.0, 1.0);
        let rate_text = if rate < 1.0 {
            format!("{:.2}s", rate)
        } else {
            format!("{:.1}s", rate)
        };
        let speed = widgets::hbar(
            ui,
            rect(COL1_X, 310.0, PANEL_R, 352.0),
            &BarModel {
                label: tr(Key::Speed),
                value_text: &rate_text,
                norm: rate_norm,
                swatch_left: Some(ci),
                random: None,
                label_regular: true,
                tip: Some(tr(Key::TipSpeed)),
            },
            &theme,
            Id::new("speed"),
        );
        if speed.began {
            self.push_undo(self.last_idle.clone());
        }
        if let Some(v) = speed.value {
            set_plain(ctx, pid(rid), 0.01 * 6000f64.powf(v));
        }
        if speed.edit_value {
            self.text_input = Some(TextInput {
                id: pid(rid),
                buf: format!("{:.2}", rate),
                kind: TextKind::Morph,
                rect: rect(COL1_X, 332.0, COL1_X + 100.0, 358.0),
            });
        }

        // 四个按钮：左→右 / 右→左 / 同步 / 翻转
        let pressed = widgets::flat_button(
            ui,
            rect(COL1_X, 356.0, COL1_X + BTN_W, 384.0),
            tr(Key::CopyLR),
            false,
            &theme,
            Id::new("copy_lr"),
            None,
        );
        if pressed {
            self.copy_lr(ctx, true);
            self.redo.clear();
        }
        if widgets::flat_button(
            ui,
            rect(COL2_X, 356.0, PANEL_R, 384.0),
            tr(Key::CopyRL),
            false,
            &theme,
            Id::new("copy_rl"),
            None,
        ) {
            self.copy_lr(ctx, false);
            self.redo.clear();
        }
        if widgets::flat_button(
            ui,
            rect(COL1_X, 388.0, COL1_X + BTN_W, 416.0),
            tr(Key::Link),
            p.link.value(),
            &theme,
            Id::new("link"),
            None,
        ) {
            set_plain(ctx, pid(P::Link), if p.link.value() { 0.0 } else { 1.0 });
        }
        if widgets::flat_button(
            ui,
            rect(COL2_X, 388.0, PANEL_R, 416.0),
            tr(Key::Flip),
            false,
            &theme,
            Id::new("flip"),
            None,
        ) {
            self.flip(ctx);
        }

        // MIX
        let mix = p.mix.raw_target();
        let mix_text = format!("{:.0}%", mix * 100.0);
        let en_mix = p.random_enable_mix.value();
        let mix_edit = widgets::hbar(
            ui,
            rect(COL1_X, 420.0, PANEL_R, 462.0),
            &BarModel {
                label: tr(Key::Mix),
                value_text: &mix_text,
                norm: mix,
                swatch_left: None,
                random: Some((p.random_color_mix.index() as usize, en_mix)),
                label_regular: false,
                tip: Some(tr(Key::TipMix)),
            },
            &theme,
            Id::new("mix"),
        );
        if mix_edit.began {
            self.push_undo(self.last_idle.clone());
        }
        if let Some(v) = mix_edit.value {
            set_plain(ctx, pid(P::Mix), v);
        }
        if mix_edit.toggle_random {
            set_plain(ctx, pid(P::RandomEnableMix), if en_mix { 0.0 } else { 1.0 });
        }
        if mix_edit.color_menu {
            self.menu = Some((
                MenuKind::Color {
                    target: ColorTarget::Mix,
                },
                ui.ctx()
                    .pointer_latest_pos()
                    .unwrap_or(Pos2::new(700.0, 440.0)),
            ));
        }
        if mix_edit.edit_value {
            self.text_input = Some(TextInput {
                id: pid(P::Mix),
                buf: format!("{:.0}", mix * 100.0),
                kind: TextKind::Mix,
                rect: rect(COL1_X, 440.0, COL1_X + 100.0, 468.0),
            });
        }

        // 图标行：撤销 / 重做 / 保存 / 读取（36×28）
        let icon_y0 = 482.0;
        let icon_y1 = 510.0;
        for (i, icon) in [Icon::Undo, Icon::Redo, Icon::Save, Icon::Load]
            .iter()
            .enumerate()
        {
            let x = COL1_X + i as f32 * 40.0;
            if widgets::icon_button(
                ui,
                rect(x, icon_y0, x + 36.0, icon_y1),
                *icon,
                &theme,
                Id::new(("icon", i)),
                Some(tr(match icon {
                    Icon::Undo => Key::Undo,
                    Icon::Redo => Key::Redo,
                    Icon::Save => Key::Save,
                    Icon::Load => Key::Load,
                })),
            ) {
                match icon {
                    Icon::Undo => self.undo(ctx),
                    Icon::Redo => self.redo(ctx),
                    Icon::Save => self.save_orm(ctx),
                    Icon::Load => self.load_orm(ctx),
                }
            }
        }

        // 标题区：ORM（32 Bold）+ 齿轮 + 版本号 + BandPass
        {
            let pnt = ui.painter_at(full);
            pnt.text(
                Pos2::new(COL1_X, 545.0),
                Align2::LEFT_TOP,
                "ORM",
                fonts::bold(32.0),
                theme.col_900(),
            );
            let gear = rect(826.0, 547.0, 854.0, 575.0);
            if widgets::gear_button(ui, gear, &theme, Id::new("gear"), None) {
                self.settings_open = true;
            }
            pnt.text(
                Pos2::new(862.0, 575.0),
                Align2::LEFT_BOTTOM,
                "v0.5.1",
                fonts::regular(20.0),
                theme.col_500(),
            );
            pnt.text(
                Pos2::new(COL1_X, 576.0),
                Align2::LEFT_TOP,
                "BandPass",
                fonts::bold(32.0),
                theme.col_900(),
            );
        }

        // ================= 底部：quick 槽 + fade =================
        for i in 0..QUICK_COUNT {
            let cx = QUICK_X0 + (QUICK_X1 - QUICK_X0) * i as f32 / (QUICK_COUNT - 1) as f32;
            let r = rect(
                cx - QUICK_W * 0.5,
                QUICK_Y,
                cx + QUICK_W * 0.5,
                QUICK_Y + QUICK_H,
            );
            let resp = widgets::preset_slot(
                ui,
                r,
                &format!("{}", i + 1),
                false,
                &theme,
                Id::new(("quick", i)),
                Some(tr(Key::TipPresets)),
            );
            if resp.clicked {
                self.load_slot(ctx, i);
            }
            if resp.save_here {
                self.slots[i] = snapshot(&ctx.params());
            }
        }

        // fade 位置：8 档吸附
        let quick_pos = self.quick as f64 / (QUICK_COUNT - 1) as f64;
        let fedit = widgets::fade_slider(
            ui,
            rect(FADE_L, FADE_Y0, FADE_R, FADE_Y1),
            quick_pos,
            QUICK_COUNT,
            &theme,
            Id::new("fade"),
            Some(tr(Key::TipFade)),
        );
        if let Some(v) = fedit.value {
            let q = (v * (QUICK_COUNT - 1) as f64).round() as usize;
            self.quick = q.min(QUICK_COUNT - 1);
            let a = self.quick;
            let b = (a + 1).min(QUICK_COUNT - 1);
            let frac = if b > a { 0.0 } else { 0.0 };
            let _ = frac;
            let new_slot = self.slots[self.numbers[a] - 1].clone();
            let before = snapshot(&ctx.params());
            self.push_undo(before);
            self.start_fade(snapshot(&ctx.params()), new_slot);
            self.current = a;
        }

        // ================= 弹出菜单 =================
        if let Some((kind, pos)) = self.menu {
            let mut keep = true;
            let mut chosen: Option<(u32, f64, bool)> = None;
            egui::Area::new(Id::new("orm_menu"))
                .order(egui::Order::Foreground)
                .fixed_pos(pos)
                .show(ui.ctx(), |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        ui.set_min_width(150.0);
                        match kind {
                            MenuKind::Slope(is_l) => {
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
                                        chosen = Some((
                                            pid(if is_l { P::SlopeL } else { P::SlopeR }),
                                            i as f64 / 3.0,
                                            true,
                                        ));
                                        keep = false;
                                    }
                                }
                            }
                            MenuKind::Color { target } => {
                                for c in 0..4 {
                                    let color = Theme::random_color(c);
                                    if ui
                                        .add(
                                            egui::Button::new("        ")
                                                .fill(color)
                                                .min_size(Vec2::new(140.0, 26.0)),
                                        )
                                        .clicked()
                                    {
                                        chosen = Some((target.color_id(), c as f64 / 3.0, true));
                                        keep = false;
                                    }
                                }
                            }
                            MenuKind::RandomPicker => {
                                for c in 0..4 {
                                    let color = Theme::random_color(c);
                                    if ui
                                        .add(
                                            egui::Button::new("        ")
                                                .fill(color)
                                                .min_size(Vec2::new(140.0, 26.0)),
                                        )
                                        .clicked()
                                    {
                                        self.selected_color = c;
                                        keep = false;
                                    }
                                }
                            }
                            MenuKind::Preset(i) => {
                                if ui.button(tr(Key::SaveHere)).clicked() {
                                    self.slots[i] = snapshot(&ctx.params());
                                    keep = false;
                                }
                                if ui.button(tr(Key::RestoreDefault)).clicked() {
                                    self.slots[i] = defaults();
                                    keep = false;
                                }
                            }
                        }
                    });
                });
            if let Some((id, v, _b)) = chosen {
                ctx.automate(id, v);
            }
            if keep && ui.ctx().input(|i| i.pointer.any_click()) {
                keep = false;
            }
            if keep {
                self.menu = Some((kind, pos));
            } else {
                self.menu = None;
            }
        }

        // ================= 文本输入 =================
        if let Some(ti) = self.text_input.clone() {
            let mut buf = ti.buf.clone();
            let mut commit = false;
            let mut cancel = false;
            egui::Area::new(Id::new("orm_text"))
                .order(egui::Order::Foreground)
                .fixed_pos(ti.rect.left_top())
                .show(ui.ctx(), |ui| {
                    egui::Frame::popup(ui.style()).show(ui, |ui| {
                        let r = ui.add(
                            egui::TextEdit::singleline(&mut buf)
                                .desired_width(ti.rect.width().max(80.0))
                                .font(egui::FontId::proportional(18.0)),
                        );
                        r.request_focus();
                        if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                            commit = true;
                        }
                        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                            cancel = true;
                        }
                    });
                });
            if commit {
                let v: f64 = parse_number(&buf).unwrap_or(f64::NAN);
                if v.is_finite() {
                    match ti.kind {
                        TextKind::Gain => set_plain(ctx, ti.id, v.clamp(-96.0, 12.0)),
                        TextKind::Mix => {
                            if ti.id == pid(P::RandomAmountR)
                                || ti.id == pid(P::RandomAmountY)
                                || ti.id == pid(P::RandomAmountB)
                                || ti.id == pid(P::RandomAmountG)
                            {
                                set_plain(ctx, ti.id, (v / 100.0).clamp(0.0, 1.0));
                            } else {
                                set_plain(ctx, ti.id, (v / 100.0).clamp(0.0, 1.0));
                            }
                        }
                        TextKind::Morph => {
                            if ti.id == 0xFFFF_0001 {
                                self.fade_time = v.clamp(0.01, 60.0);
                            } else {
                                set_plain(ctx, ti.id, v.clamp(0.01, 60.0));
                            }
                        }
                        TextKind::LowCut(is_l) | TextKind::HighCut(is_l) => {
                            let freq = p
                                .get_plain(pid(if is_l { P::FreqL } else { P::FreqR }))
                                .unwrap_or(1000.0);
                            let bw = p
                                .get_plain(pid(if is_l { P::BwL } else { P::BwR }))
                                .unwrap_or(3.65);
                            let m = if matches!(ti.kind, TextKind::LowCut(_)) {
                                (freq / v.max(20.0)).max(1.0)
                            } else {
                                (v.max(20.0) / freq).max(1.0)
                            };
                            set_plain(ctx, pid(if is_l { P::BwL } else { P::BwR }), m);
                            let _ = bw;
                        }
                        TextKind::Fade => {
                            self.quick = (v.round() as usize).min(QUICK_COUNT - 1);
                        }
                        TextKind::Freq(is_l) => {
                            let f = v.clamp(20.0, 20_000.0);
                            set_plain(ctx, pid(if is_l { P::FreqL } else { P::FreqR }), f);
                            if p.link.value() {
                                set_plain(ctx, pid(if is_l { P::FreqR } else { P::FreqL }), f);
                            }
                        }
                        TextKind::Bw(is_l) => {
                            let bwv = v.clamp(1.0, 31.0);
                            set_plain(ctx, pid(if is_l { P::BwL } else { P::BwR }), bwv);
                            if p.link.value() {
                                set_plain(ctx, pid(if is_l { P::BwR } else { P::BwL }), bwv);
                            }
                        }
                    }
                }
                self.text_input = None;
            } else if cancel {
                self.text_input = None;
            } else {
                self.text_input = Some(TextInput { buf, ..ti });
            }
        }

        // ================= 设置面板 =================
        if self.settings_open {
            self.draw_settings(ui, &theme);
        }
    }

    fn state_changed(&mut self, ctx: &PluginContext<BandPassParams>) {
        // 宿主改参（自动化/预设切换）后，把稳定快照对齐到当前值，
        // 避免下一次手势撤销把旧默认值推回栈里。
        self.last_idle = snapshot(&ctx.params());
    }
}

impl BandPassUi {
    fn apply_band(&mut self, ctx: &PluginContext<BandPassParams>, is_l: bool, edit: BandEdit) {
        if edit.began {
            self.push_undo(self.last_idle.clone());
        }
        let p = ctx.params();
        let freq = p
            .get_plain(pid(if is_l { P::FreqL } else { P::FreqR }))
            .unwrap_or(1000.0);
        let bw = p
            .get_plain(pid(if is_l { P::BwL } else { P::BwR }))
            .unwrap_or(3.65);
        if let Some(low) = edit.low_hz {
            let m = (freq / low.max(20.0)).max(1.0);
            set_plain(ctx, pid(if is_l { P::BwL } else { P::BwR }), m);
            if p.link.value() {
                set_plain(ctx, pid(if is_l { P::BwR } else { P::BwL }), m);
            }
        }
        if let Some(high) = edit.high_hz {
            let m = (high.max(20.0) / freq).max(1.0);
            set_plain(ctx, pid(if is_l { P::BwL } else { P::BwR }), m);
            if p.link.value() {
                set_plain(ctx, pid(if is_l { P::BwR } else { P::BwL }), m);
            }
        }
        if let Some(f) = edit.freq {
            set_plain(ctx, pid(if is_l { P::FreqL } else { P::FreqR }), f);
            if p.link.value() {
                set_plain(ctx, pid(if is_l { P::FreqR } else { P::FreqL }), f);
            }
        }
        if let Some(m) = edit.bw_mult {
            set_plain(ctx, pid(if is_l { P::BwL } else { P::BwR }), m);
            if p.link.value() {
                set_plain(ctx, pid(if is_l { P::BwR } else { P::BwL }), m);
            }
        }
        let _ = bw;
    }

    fn draw_settings(&mut self, ui: &mut egui::Ui, theme: &Theme) {
        // 全屏遮罩 IColor(70,26,25,22) + 380×270 卡片（无音频设备版）
        let full = rect(0.0, 0.0, UI_WIDTH, UI_HEIGHT);
        ui.painter_at(full)
            .rect_filled(full, 0, Color32::from_rgba_unmultiplied(26, 25, 22, 70));
        let card = Rect::from_center_size(full.center(), Vec2::new(380.0, 270.0));
        let pnt = ui.painter_at(card);
        pnt.rect_filled(card, 0, theme.col_100());
        pnt.rect_stroke(
            card,
            0,
            egui::Stroke::new(1.0, theme.col_300()),
            egui::StrokeKind::Inside,
        );

        let lang = self.lang;
        let tr = move |k: Key| strings::tr(lang, k);
        let l = card.left() + 20.0;
        let r = card.right() - 20.0;
        let bw = (r - l) / 2.0;

        // 语言：标题 14..34、按钮 38..68
        pnt.text(
            Pos2::new(l, card.top() + 24.0),
            Align2::LEFT_CENTER,
            tr(Key::Language),
            fonts::semibold(20.0),
            theme.col_900(),
        );
        if widgets::flat_button(
            ui,
            rect(l, card.top() + 38.0, l + bw, card.top() + 68.0),
            tr(Key::Chinese),
            self.lang == Lang::Zh,
            theme,
            Id::new("set_zh"),
            None,
        ) {
            self.lang = Lang::Zh;
            save_settings(&Settings {
                lang: self.lang,
                hue: self.theme.hue,
                sat: self.theme.sat_max,
                dark: self.theme.mode == ThemeMode::Dark,
            });
        }
        if widgets::flat_button(
            ui,
            rect(l + bw, card.top() + 38.0, r, card.top() + 68.0),
            "ENGLISH",
            self.lang == Lang::En,
            theme,
            Id::new("set_en"),
            None,
        ) {
            self.lang = Lang::En;
            save_settings(&Settings {
                lang: self.lang,
                hue: self.theme.hue,
                sat: self.theme.sat_max,
                dark: self.theme.mode == ThemeMode::Dark,
            });
        }

        // 主题：标题 80..100、按钮 104..134
        pnt.text(
            Pos2::new(l, card.top() + 90.0),
            Align2::LEFT_CENTER,
            tr(Key::Theme),
            fonts::semibold(20.0),
            theme.col_900(),
        );
        if widgets::flat_button(
            ui,
            rect(l, card.top() + 104.0, l + bw, card.top() + 134.0),
            tr(Key::Dark),
            self.theme.mode == ThemeMode::Dark,
            theme,
            Id::new("set_dark"),
            None,
        ) {
            self.theme.mode = ThemeMode::Dark;
            save_settings(&Settings {
                lang: self.lang,
                hue: self.theme.hue,
                sat: self.theme.sat_max,
                dark: true,
            });
        }
        if widgets::flat_button(
            ui,
            rect(l + bw, card.top() + 104.0, r, card.top() + 134.0),
            tr(Key::Light),
            self.theme.mode == ThemeMode::Light,
            theme,
            Id::new("set_light"),
            None,
        ) {
            self.theme.mode = ThemeMode::Light;
            save_settings(&Settings {
                lang: self.lang,
                hue: self.theme.hue,
                sat: self.theme.sat_max,
                dark: false,
            });
        }

        // 色相：标题 148..174（左色块 14 + 8 间隙），轨道 174..200
        let hue_hdr_y = card.top() + 161.0;
        let sw = rect(l, hue_hdr_y - 7.0, l + 14.0, hue_hdr_y + 7.0);
        pnt.rect_filled(sw, 0, Theme::hsb(self.theme.hue, 0.85, 1.0));
        pnt.text(
            Pos2::new(l + 22.0, hue_hdr_y),
            Align2::LEFT_CENTER,
            tr(Key::Hue),
            fonts::semibold(20.0),
            theme.col_900(),
        );
        let hue_val = format!("{}\u{00b0}", self.theme.hue);
        pnt.text(
            Pos2::new(r, hue_hdr_y),
            Align2::RIGHT_CENTER,
            &hue_val,
            fonts::regular(20.0),
            theme.col_700(),
        );
        let htrack = rect(l, card.top() + 185.0, r, card.top() + 189.0);
        pnt.rect_filled(htrack, 2, theme.col_300());
        let hn = ((self.theme.hue.saturating_sub(15)) as f32 / 345.0).clamp(0.0, 1.0);
        let hx = htrack.left() + hn * htrack.width();
        pnt.rect_filled(
            Rect::from_min_max(htrack.min, Pos2::new(hx, htrack.max.y)),
            2,
            theme.col_500(),
        );
        pnt.circle_filled(Pos2::new(hx, htrack.center().y), 8.5, theme.col_100());
        pnt.circle_filled(Pos2::new(hx, htrack.center().y), 7.0, theme.col_900());
        let hresp = ui.interact(
            rect(l, card.top() + 174.0, r, card.top() + 200.0),
            Id::new("set_hue"),
            Sense::click_and_drag(),
        );
        if hresp.dragged() || hresp.clicked() {
            if let Some(pos) = ui.ctx().pointer_interact_pos() {
                let n = ((pos.x - htrack.left()) / htrack.width()).clamp(0.0, 1.0);
                let h = 15 + (n * 345.0 / 15.0).round() as u16 * 15;
                self.theme.hue = h.min(360);
            }
        }

        // 饱和度：标题 206..232（左色块竖分 4 段），轨道 232..258
        let sat_hdr_y = card.top() + 219.0;
        let sw = rect(l, sat_hdr_y - 7.0, l + 14.0, sat_hdr_y + 7.0);
        let seg_h = 14.0 / 4.0;
        for i in 0..4 {
            let b = 1.0 - 0.25 * i as f32;
            pnt.rect_filled(
                rect(
                    sw.left(),
                    sw.top() + i as f32 * seg_h,
                    sw.right(),
                    sw.top() + (i + 1) as f32 * seg_h,
                ),
                0,
                Theme::hsb(self.theme.hue, self.theme.sat_max as f32 / 100.0, b),
            );
        }
        pnt.text(
            Pos2::new(l + 22.0, sat_hdr_y),
            Align2::LEFT_CENTER,
            tr(Key::Saturation),
            fonts::semibold(20.0),
            theme.col_900(),
        );
        let sat_key = match self.theme.sat_max {
            0 => Key::SatNone,
            15 => Key::SatLow,
            30 => Key::SatMed,
            _ => Key::SatHigh,
        };
        pnt.text(
            Pos2::new(r, sat_hdr_y),
            Align2::RIGHT_CENTER,
            tr(sat_key),
            fonts::regular(20.0),
            theme.col_700(),
        );
        let st = rect(l, card.top() + 243.0, r, card.top() + 247.0);
        pnt.rect_filled(st, 2, theme.col_300());
        let sn = (self.theme.sat_max as f32 / 50.0).clamp(0.0, 1.0);
        let sx = st.left() + sn * st.width();
        pnt.rect_filled(
            Rect::from_min_max(st.min, Pos2::new(sx, st.max.y)),
            2,
            theme.col_500(),
        );
        pnt.circle_filled(Pos2::new(sx, st.center().y), 8.5, theme.col_100());
        pnt.circle_filled(Pos2::new(sx, st.center().y), 7.0, theme.col_900());
        let sresp = ui.interact(
            rect(l, card.top() + 232.0, r, card.top() + 258.0),
            Id::new("set_sat_slider"),
            Sense::click_and_drag(),
        );
        if sresp.dragged() || sresp.clicked() {
            if let Some(pos) = ui.ctx().pointer_interact_pos() {
                let n = ((pos.x - st.left()) / st.width()).clamp(0.0, 1.0);
                let sat = (n * 50.0 / 5.0).round() as u8 * 5;
                self.theme.sat_max = sat;
                save_settings(&Settings {
                    lang: self.lang,
                    hue: self.theme.hue,
                    sat: sat,
                    dark: self.theme.mode == ThemeMode::Dark,
                });
            }
        }

        // 点击卡片外关闭（原版 SetVisible(false)）
        let outside = ui.interact(
            rect(0.0, 0.0, UI_WIDTH, UI_HEIGHT),
            Id::new("settings_bg"),
            Sense::click(),
        );
        if outside.clicked() {
            if let Some(pos) = ui.ctx().pointer_interact_pos() {
                if !card.contains(pos) {
                    self.settings_open = false;
                    save_settings(&Settings {
                        lang: self.lang,
                        hue: self.theme.hue,
                        sat: self.theme.sat_max,
                        dark: self.theme.mode == ThemeMode::Dark,
                    });
                }
            }
        }
    }
}

pub fn create(params: Arc<BandPassParams>) -> Box<dyn Editor> {
    truce_egui::EguiEditor::with_ui(
        params,
        (UI_WIDTH as u32, UI_HEIGHT as u32),
        BandPassUi::default(),
    )
    .with_font(fonts::REGULAR)
    .resizable(false)
    .into_editor()
}
