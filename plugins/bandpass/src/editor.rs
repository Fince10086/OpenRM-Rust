//! BandPass 的 egui 界面：916×646 绝对坐标，与 C++ 版逐像素对齐。
//!
//! 布局常量全部取自 `plugins/BandPass/src/BandPass.cpp` 的 `Layout()`，
//! 控件行为取自 `plugins/common/controls/*.h`。任何"看起来差不多"的近似值
//! 都在注释里标了原版常量名，便于日后核对。

use std::sync::Arc;

use egui::{Align2, Color32, Id, Pos2, Rect, Sense, Vec2};
use openrm_ui::{
    fonts,
    native_menu,
    preset::PresetBank,
    shell,
    strings::{self, Key, Lang},
    theme::Theme,
    widgets::{self, BandEdit, BandModel, BarModel, GainModel, Icon, PadModel, PadEdit},
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

const COL1_X: f32 = shell::layout::COL1_X;
const COL2_X: f32 = shell::layout::COL2_X;
const BTN_W: f32 = shell::layout::BTN_W;
const PANEL_R: f32 = shell::layout::PANEL_R;
const GRID_Y: f32 = shell::layout::GRID_Y;
const CELL_W: f32 = shell::layout::CELL_W;
const CELL_H: f32 = shell::layout::CELL_H;
const CELL_STEP_X: f32 = shell::layout::CELL_STEP_X;
const CELL_STEP_Y: f32 = shell::layout::CELL_STEP_Y;

// 底部 quick 槽刻度：原版 mQuickTicks 中心在 39.5..609 之间
const QUICK_X0: f32 = shell::layout::QUICK_X0;
const QUICK_X1: f32 = shell::layout::QUICK_X1;
const QUICK_W: f32 = shell::layout::QUICK_W;
const QUICK_Y: f32 = shell::layout::QUICK_Y;
const QUICK_H: f32 = shell::layout::QUICK_H;

const FADE_L: f32 = shell::layout::FADE_L;
const FADE_R: f32 = shell::layout::FADE_R;
const FADE_Y0: f32 = shell::layout::FADE_Y0;
const FADE_Y1: f32 = shell::layout::FADE_Y1;

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

/// 把交织帧 `[inL, inR, wetL, wetR]` 拆成四路。
fn deinterleave_hop(chunk: &[f32]) -> (Vec<f32>, Vec<f32>, Vec<f32>, Vec<f32>) {
    let mut l = Vec::with_capacity(chunk.len() / 4);
    let mut r = Vec::with_capacity(chunk.len() / 4);
    let mut wl = Vec::with_capacity(chunk.len() / 4);
    let mut wr = Vec::with_capacity(chunk.len() / 4);
    for f in chunk.chunks_exact(4) {
        l.push(f[0]);
        r.push(f[1]);
        wl.push(f[2]);
        wr.push(f[3]);
    }
    (l, r, wl, wr)
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
            let (dry_l, dry_r, wet_l, wet_r) = deinterleave_hop(&chunk);
            self.l.push_hop(&dry_l, &wet_l);
            // 右垫取右声道自己的干信号（修正原版误用 mSpecInL 的 bug）
            self.r.push_hop(&dry_r, &wet_r);
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

// ===== UI 状态 =====

pub struct BandPassUi {
    theme: Theme,
    lang: Lang,
    styled: bool,
    font_stage: u8,
    spectrum: Option<SpectrumUi>,
    bank: PresetBank,
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
    menu_armed: bool,
}

impl Default for BandPassUi {
    fn default() -> Self {
        let (lang, theme) = shell::load_settings("ORMBandPass.settings");
        let bank = PresetBank::from_factory(factory_presets());
        Self {
            theme,
            lang,
            styled: false,
            font_stage: 0,
            spectrum: None,
            bank,
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
            menu_armed: false,
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

    /// 截图与自测用：打开设置面板。
    pub fn open_settings(&mut self) {
        self.settings_open = true;
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
        let Some(target) = self.bank.slot(idx).cloned() else {
            return;
        };
        let from = snapshot(&ctx.params());
        self.push_undo(from.clone());
        self.bank.current = idx;
        self.bank.quick = idx.min(QUICK_COUNT - 1);
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
            "presets": self.bank.slots,
            "currentPreset": self.bank.current,
            "currentValues": snapshot(&ctx.params()),
            "numbers": self.bank.numbers,
            "fadePos": self.bank.quick as f64 / 7.0,
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
        if !slots.is_empty() && slots.len() == SLOT_COUNT {
            self.bank.slots = slots.into_iter().map(Some).collect();
        }
        if let Some(cur) = j.get("currentPreset").and_then(|v| v.as_i64()) {
            self.bank.current = cur.clamp(0, (SLOT_COUNT - 1) as i64) as usize;
        }
        if let Some(nums) = j.get("numbers").and_then(|v| v.as_array()) {
            for (i, n) in nums.iter().take(SLOT_COUNT).enumerate() {
                if let Some(v) = n.as_u64() {
                    self.bank.numbers[i] = v as usize;
                }
            }
        }
        if let Some(pos) = j.get("fadePos").and_then(|v| v.as_f64()) {
            self.bank.quick = ((pos * 7.0).round() as usize).min(QUICK_COUNT - 1);
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
        ui.painter_at(full).rect_filled(full, 0, theme.col_100());

        // 拖拽状态（预设槽交换）
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
        if !ui.input(|i| i.pointer.any_down()) {
            self.last_idle = snapshot(&p);
        }
        // 随机调制在 DSP 线程里持续改变幽灵球的目标，但宿主不会主动刷新 UI。
        // 以前只在参数变化时重绘，球看起来一两秒跳一下；这里在随机开启时
        // 持续请求下一帧，和 C++ 版的 60fps 定时器一致。
        let random_anim = p.random_enable_freq_l.value()
            || p.random_enable_freq_r.value()
            || p.random_enable_bw_l.value()
            || p.random_enable_bw_r.value()
            || p.random_enable_gain_l.value()
            || p.random_enable_gain_r.value()
            || p.random_enable_mix.value();
        if random_anim || self.fade.is_some() {
            ui.ctx().request_repaint();
        }
        // 频谱由音频线程持续写入；egui 只在鼠标事件/参数变化时重绘，会
        // 导致频谱只在鼠标移动时前进。这里按 30fps 持续刷新。
        if self.spectrum.is_some() {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(33));
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
                self.menu_armed = false;
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
                self.menu_armed = false;
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
                    min_db: -96.0,
                    max_db: 12.0,
                    random_on: en_gain,
                    random_color: color_g as usize,
                    show_random: true,
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
                self.menu_armed = false;
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
        shell::preset_heading(ui, &theme, lang);
        let labels = self.bank.grid_labels();
        let responses = shell::preset_grid(
            ui,
            &theme,
            lang,
            &labels,
            if self.drag_from.is_some() { self.drag_hover } else { None },
            "orm_bandpass",
        );
        for (i, resp) in responses.into_iter().enumerate() {
            let abs = i + QUICK_COUNT;
            if resp.hovered {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
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
                let s = snapshot(&ctx.params());
                self.bank.save(abs, s);
            }
            if resp.secondary {
                self.menu_armed = false;
                self.menu = Some((
                    MenuKind::Preset(abs),
                    ui.ctx().pointer_latest_pos().unwrap_or(Pos2::new(COL1_X, GRID_Y)),
                ));
            }
        }

        // MORPH（预设渐变时间）
        let morph = shell::morph_bar(ui, &theme, lang, self.fade_time, "orm_bandpass");
        if let Some(v) = morph.value {
            self.fade_time = shell::morph_seconds(v);
        }
        if morph.edit_value {
            self.text_input = Some(TextInput {
                id: 0xFFFF_0001,
                buf: format!("{:.2}", self.fade_time),
                kind: TextKind::Morph,
                rect: rect(COL1_X, 216.0, COL1_X + 100.0, 244.0),
            });
        }

        // RANDOM 标题 + 选色块：左右键都打开取色菜单
        let swatch = shell::random_header(
            ui,
            &theme,
            lang,
            self.selected_color,
            true,
            "orm_bandpass",
            Some(tr(Key::TipRandomPicker)),
        );
        if swatch.clicked || swatch.secondary {
            self.menu_armed = false;
                self.menu = Some((
                MenuKind::RandomPicker,
                ui.ctx()
                    .pointer_latest_pos()
                    .unwrap_or(Pos2::new(860.0, 260.0)),
            ));
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
        if range.color_picker {
            self.menu_armed = false;
                self.menu = Some((
                MenuKind::RandomPicker,
                ui.ctx().pointer_latest_pos().unwrap_or(Pos2::new(COL1_X + 80.0, 270.0)),
            ));
            self.menu_armed = false;
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
        if speed.color_picker {
            self.menu_armed = false;
                self.menu = Some((
                MenuKind::RandomPicker,
                ui.ctx().pointer_latest_pos().unwrap_or(Pos2::new(COL1_X + 80.0, 315.0)),
            ));
            self.menu_armed = false;
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
            self.menu_armed = false;
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

        // 图标行 + 标题区：与 Fusion 共用同一套外壳组件
        if let Some(icon) = shell::icon_row(ui, &theme, lang, "orm_bandpass") {
            match icon {
                Icon::Undo => self.undo(ctx),
                Icon::Redo => self.redo(ctx),
                Icon::Save => self.save_orm(ctx),
                Icon::Load => self.load_orm(ctx),
            }
        }
        if shell::title_block(
            ui,
            &theme,
            "BandPass",
            env!("CARGO_PKG_VERSION"),
            "orm_bandpass",
        ) {
            self.settings_open = true;
        }

        // ================= 底部：quick 槽 + fade =================
        let quick_labels = self.bank.quick_labels();
        let qresp = shell::quick_row(ui, &theme, lang, &quick_labels, "orm_bandpass");
        for (i, resp) in qresp.into_iter().enumerate() {
            if resp.clicked {
                self.load_slot(ctx, i);
            }
            if resp.save_here {
                let s = snapshot(&ctx.params());
                self.bank.save(i, s);
            }
        }

        // fade 位置：8 档吸附
        let quick_pos = self.bank.quick as f64 / (QUICK_COUNT - 1) as f64;
        let fedit = widgets::fade_slider(
            ui,
            rect(FADE_L, FADE_Y0, FADE_R, FADE_Y1),
            quick_pos,
            QUICK_COUNT,
            &theme,
            Id::new("fade"),
            Some(tr(Key::TipFade)),
        );
        if fedit.began {
            self.push_undo(snapshot(&ctx.params()));
        }
        if let Some(v) = fedit.value {
            let q = (v * (QUICK_COUNT - 1) as f64).round() as usize;
            self.bank.quick = q.min(QUICK_COUNT - 1);
            let a = self.bank.quick;
            let Some(new_slot) = self.bank.slot(a).cloned() else { return; };
            self.start_fade(snapshot(&ctx.params()), new_slot);
            self.bank.current = a;
        }

        // ================= 弹出菜单 =================
        if let Some((kind, pos)) = self.menu {
            // 系统 NSMenu 优先：点击菜单外关闭、键盘导航和外观都交给 AppKit。
            let (labels, checked): (&[&str], Option<usize>) = match kind {
                MenuKind::Slope(is_l) => {
                    let cur = if is_l {
                        p.slope_l.index()
                    } else {
                        p.slope_r.index()
                    };
                    (&["12 dB/oct", "24 dB/oct", "48 dB/oct", "96 dB/oct"], Some(cur as usize))
                }
                MenuKind::Color { target } => {
                    let cur = match target {
                        ColorTarget::Freq(true) => p.random_color_freq_l.index(),
                        ColorTarget::Freq(false) => p.random_color_freq_r.index(),
                        ColorTarget::Bw(true) => p.random_color_bw_l.index(),
                        ColorTarget::Bw(false) => p.random_color_bw_r.index(),
                        ColorTarget::Gain(true) => p.random_color_gain_l.index(),
                        ColorTarget::Gain(false) => p.random_color_gain_r.index(),
                        ColorTarget::Mix => p.random_color_mix.index(),
                    };
                    (
                        &[
                            tr(Key::Red),
                            tr(Key::Yellow),
                            tr(Key::Blue),
                            tr(Key::Green),
                        ],
                        Some(cur as usize),
                    )
                }
                MenuKind::RandomPicker => (
                    &[
                        tr(Key::Red),
                        tr(Key::Yellow),
                        tr(Key::Blue),
                        tr(Key::Green),
                    ],
                    Some(self.selected_color),
                ),
                MenuKind::Preset(_) => (&[tr(Key::SaveHere), tr(Key::RestoreDefault)], None),
            };
            let items: Vec<native_menu::MenuItem> = labels
                .iter()
                .enumerate()
                .map(|(i, label)| native_menu::MenuItem {
                    label: *label,
                    checked: checked == Some(i),
                    enabled: true,
                })
                .collect();
            match native_menu::popup(&items) {
                native_menu::PopupResult::Selected(i) => {
                    self.menu = None;
                    self.menu_armed = false;
                    match kind {
                        MenuKind::Slope(is_l) => {
                            ctx.automate(pid(if is_l { P::SlopeL } else { P::SlopeR }), i as f64 / 3.0);
                        }
                        MenuKind::Color { target } => {
                            set_plain(ctx, target.color_id(), i as f64);
                        }
                        MenuKind::RandomPicker => {
                            self.selected_color = i;
                        }
                        MenuKind::Preset(idx) => {
                            if i == 0 {
                                self.bank.save(idx, snapshot(&ctx.params()));
                            } else if i == 1 {
                                self.bank.slots[idx] = Some(defaults());
                            }
                        }
                    }
                    return;
                }
                native_menu::PopupResult::Dismissed => {
                    self.menu = None;
                    self.menu_armed = false;
                    return;
                }
                native_menu::PopupResult::Unsupported => {}
            }

            let mut keep = true;
            let just_opened = !self.menu_armed;
            self.menu_armed = true;
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
                                        chosen = Some((target.color_id(), c as f64, false));
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
                                    self.bank.save(i, snapshot(&ctx.params()));
                                    keep = false;
                                }
                                if ui.button(tr(Key::RestoreDefault)).clicked() {
                                    self.bank.slots[i] = Some(defaults());
                                    keep = false;
                                }
                            }
                        }
                    });
                });
            if let Some((id, v, normalized)) = chosen {
                if normalized {
                    ctx.automate(id, v);
                } else {
                    set_plain(ctx, id, v);
                }
            }
            if keep && !just_opened && ui.ctx().input(|i| i.pointer.any_click()) {
                keep = false;
            }
            if keep {
                self.menu_armed = false;
                self.menu = Some((kind, pos));
            } else {
                self.menu = None;
                self.menu_armed = false;
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
                    egui::Frame::new()
                        .fill(theme.col_100())
                        .inner_margin(egui::Margin::symmetric(2, 1))
                        .show(ui, |ui| {
                            ui.set_min_size(ti.rect.size());
                            let r = ui.add_sized(
                                ti.rect.size(),
                                egui::TextEdit::singleline(&mut buf)
                                    .font(egui::FontId::proportional(16.0))
                                    .horizontal_align(egui::Align::Center),
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
                            self.bank.quick = (v.round() as usize).min(QUICK_COUNT - 1);
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
            self.draw_settings(ui);
        }
        // 只有文本输入框真正打开时才让插件持有键盘焦点；其余时间把焦点
        // 交给宿主，避免空格/方向键被插件吞掉。
        openrm_ui::focus::set_text_focus(self.text_input.is_some());
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

    fn draw_settings(&mut self, ui: &mut egui::Ui) {
        if !self.settings_open {
            return;
        }
        let res = shell::settings_card(
            ui,
            &mut self.theme,
            &mut self.lang,
            None,
            "orm_bandpass",
        );
        if res.closed {
            self.settings_open = false;
        }
        if res.changed || res.closed {
            shell::save_settings("ORMBandPass.settings", self.lang, &self.theme);
        }
    }
}

pub fn create(params: Arc<BandPassParams>) -> Box<dyn Editor> {
    let ui = BandPassUi::default();
    let background = ui.theme.col_100();
    let editor = truce_egui::EguiEditor::with_ui(
        params,
        (UI_WIDTH as u32, UI_HEIGHT as u32),
        ui,
    )
    .with_font(fonts::REGULAR)
    .resizable(false)
    .into_editor();
    openrm_ui::flash::guard(editor, background)
}

#[cfg(test)]
mod tests {
    use super::deinterleave_hop;

    #[test]
    fn right_channel_dry_is_own_channel() {
        // 每帧 [inL, inR, wetL, wetR]；右垫必须拿 inR，而不是原版误用的 inL。
        let chunk = [1.0, 2.0, 10.0, 20.0, 3.0, 4.0, 30.0, 40.0];
        let (l, r, wl, wr) = deinterleave_hop(&chunk);
        assert_eq!(l, vec![1.0, 3.0]);
        assert_eq!(r, vec![2.0, 4.0]);
        assert_eq!(wl, vec![10.0, 30.0]);
        assert_eq!(wr, vec![20.0, 40.0]);
    }
}
