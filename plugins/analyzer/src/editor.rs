//! Analyzer egui 编辑器 —— 把原版 C++/iPlug2 界面逐矩形装配到 truce-egui。
//!
//! 布局矩形全部来自 [`crate::ui::layout::Layout`]（960×635，绝对坐标）；
//! 控件 retained 状态（频谱 / 声像 / 矢量 / 示波器 / CPU / 设置面板）挂在本结构上，
//! 每帧从 [`crate::analysis::AnalyzerShared`] 读最新帧，把参数配置下发给控件。
//!
//! 参数写回路径：
//! * 单次离散切换（cycle 按钮 / toggle / 图标）——先压一次撤销快照，再
//!   `begin_edit → set_param(normalized) → end_edit` 写参；
//! * 撤销 / 重做 —— `apply_snapshot` 还原纯值后逐个 `automate` 同步宿主；
//! * `.orm` 预设 —— `write_state_file` / `read_state_file` + `rfd` 对话框。

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use egui::{Align2, Id, Pos2, Sense, Vec2};
use openrm_ui::{
    fonts,
    strings::{self},
    widgets::{self, Icon},
};
use truce::prelude::*;
use truce_egui::EditorUi;

use crate::params::AnalyzerParams;
use crate::ui::cpu::CpuMeter;
use crate::ui::field::FieldControl;
use crate::ui::gonio::Goniometer;
use crate::ui::layout::{Layout, UI_H, UI_W};
use crate::ui::scope::{self, Oscilloscope};
use crate::ui::settings::{PanelAction, SettingsPanel};
use crate::ui::spectrum::SpectrumPad;
use crate::ui::widgets::{
    action_button, toggle_button, CycleButton, OrmSlider, SliderDir,
};

// ==================== 参数 ID（与 params.rs 声明顺序一致） ====================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum P {
    Speed = 0,
    ReleaseMode = 1,
    Range = 2,
    Res = 3,
    LfRes = 4,
    Mode = 5,
    ChannelMode = 6,
    LevelMode = 7,
    LevelHold = 8,
    LevelHoldOn = 9,
    Freeze = 10,
    SlopeFft = 11,
    SlopeVqt = 12,
    SlopePbt = 13,
    SlopeRta = 14,
    RtaOctave = 15,
    FftWindow = 16,
    WindowVqt = 17,
    VqtGamma = 18,
    LoudPreset = 19,
    LoudScale = 20,
}

impl P {
    fn id(self) -> u32 {
        self as u32
    }
}

/// 频谱模式（与 `AnalyzerMode` 枚举下标一致）。
mod mode {
    pub const FFT: i32 = 0;
    pub const VQT: i32 = 1;
    pub const PBT: i32 = 2;
    pub const RTA: i32 = 3;
}

/// 撤销栈上限（原版 `kUndoLimit = 100`，Analyzer.cpp:33）。
const UNDO_LIMIT: usize = 100;

// ---------------------------------------------------------------------------
// 编辑器结构
// ---------------------------------------------------------------------------

/// 装配后的 Analyzer 界面。`layout` 对外暴露（截图 / 测试任务按矩形核对位置）。
pub struct AnalyzerUi {
    pub layout: Layout,

    // ===== retained 控件 =====
    panel: SettingsPanel,
    pad: SpectrumPad,
    field: FieldControl,
    gonio: Goniometer,
    scope: Oscilloscope,
    cpu_meter: CpuMeter,
    zoom_slider: OrmSlider,
    time_slider: OrmSlider,

    // ===== 撤销 / 重做 =====
    undo: Vec<Vec<f64>>,
    redo: Vec<Vec<f64>>,
    /// 手势间隙内作为「撤销前稳定快照」的上次空闲参数快照。
    last_idle: Vec<f64>,

    // ===== 纯 UI 状态（不进参数；控件内的等价 retained 字段由 setter 同步） =====
    /// 下半面板：0 = FIELD，1 = VECTOR。
    scope_mode: i32,
    /// 示波器声道：0 M / 1 L / 2 R。
    scope_chan: i32,
    /// 触发源：0 ROLL / 1 SYNC / 2 SWEEP。
    scope_trig: i32,
    /// 矢量余辉：0 OFF / 1 SHORT / 2 LONG。
    gonio_persist: i32,
    /// 矢量倍率：0 x1 / 1 x2 / 2 x4。
    gonio_zoom: i32,

    // ===== 字体两阶段安装 =====
    styled: bool,
    font_stage: u8,
}

impl Default for AnalyzerUi {
    fn default() -> Self {
        let layout = Layout::new();
        let mut zoom_slider = OrmSlider::new(layout.oscilloscope, SliderDir::Vertical);
        zoom_slider.track_far = true;
        zoom_slider.header_visible = false;
        let mut time_slider = OrmSlider::new(layout.oscilloscope, SliderDir::Horizontal);
        time_slider.track_far = true;
        time_slider.header_visible = false;
        Self {
            layout,
            panel: SettingsPanel::load(),
            pad: SpectrumPad::new(layout.plot),
            field: FieldControl::new(layout.scope_panel),
            gonio: Goniometer::new(layout.scope_panel),
            scope: Oscilloscope::new(layout.oscilloscope),
            cpu_meter: CpuMeter::new(layout.cpu),
            zoom_slider,
            time_slider,
            undo: Vec::new(),
            redo: Vec::new(),
            last_idle: Vec::new(),
            scope_mode: 0,
            scope_chan: 0,
            scope_trig: scope::trig::SYNC,
            gonio_persist: 1,
            gonio_zoom: 0,
            styled: false,
            font_stage: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// 参数写回 / 撤销重做
// ---------------------------------------------------------------------------

fn snapshot(p: &AnalyzerParams) -> Vec<f64> {
    p.snapshot()
}

/// 写一个参数：先落纯值，再用 `begin_edit/set_param/end_edit` 包裹通知宿主。
fn write_param(ctx: &PluginContext<AnalyzerParams>, id: u32, plain: f64) {
    let p = ctx.params();
    p.set_plain(id, plain);
    if let Some(n) = p.get_normalized(id) {
        ctx.begin_edit(id);
        ctx.set_param(id, n);
        ctx.end_edit(id);
    }
}

/// 还原整组纯值后逐个 `automate` 同步宿主（撤销 / 重做 / 读预设路径）。
fn apply_snapshot(ctx: &PluginContext<AnalyzerParams>, values: &[f64]) {
    let _ = ctx.params().apply_snapshot(values);
    let n = ctx.params().count();
    for i in 0..n {
        if let Some(nv) = ctx.params().get_normalized(i as u32) {
            ctx.automate(i as u32, nv);
        }
    }
}

impl AnalyzerUi {
    fn ensure_style(&mut self, ui: &mut egui::Ui) {
        // egui 字体定义要等到下一帧 begin_pass 才生效：首帧装字体，次帧打开命名族。
        match self.font_stage {
            0 => {
                fonts::install(ui.ctx());
                self.font_stage = 1;
                ui.ctx().request_repaint();
            }
            1 => {
                fonts::set_named_ready(true);
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

    fn push_undo(&mut self, stable: Vec<f64>) {
        if self.undo.last() == Some(&stable) {
            return;
        }
        self.undo.push(stable);
        if self.undo.len() > UNDO_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    /// 手势开始时的稳定快照：优先用上次空闲快照，首帧兜底取当前值。
    fn stable_snapshot(&self, p: &AnalyzerParams) -> Vec<f64> {
        if self.last_idle.is_empty() {
            snapshot(p)
        } else {
            self.last_idle.clone()
        }
    }

    /// 循环档位：先压稳定快照，再写下一挡。
    fn cycle(&mut self, ctx: &PluginContext<AnalyzerParams>, id: u32, num: usize) {
        let cur = ctx
            .params()
            .get_plain(id)
            .unwrap_or(0.0)
            .round() as usize;
        let next = (cur + 1) % num.max(1);
        self.push_undo(self.stable_snapshot(ctx.params()));
        write_param(ctx, id, next as f64);
    }

    fn undo(&mut self, ctx: &PluginContext<AnalyzerParams>) {
        if let Some(prev) = self.undo.pop() {
            self.redo.push(snapshot(ctx.params()));
            apply_snapshot(ctx, &prev);
        }
    }

    fn redo(&mut self, ctx: &PluginContext<AnalyzerParams>) {
        if let Some(next) = self.redo.pop() {
            self.undo.push(snapshot(ctx.params()));
            apply_snapshot(ctx, &next);
        }
    }

    fn save_preset(&self, ctx: &PluginContext<AnalyzerParams>) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("ORM preset", &["orm"])
            .set_file_name("ORMAnalyzer.orm")
            .save_file()
        else {
            return;
        };
        let _ = crate::params::write_state_file(&path, &snapshot(ctx.params()));
    }

    fn load_preset(&mut self, ctx: &PluginContext<AnalyzerParams>) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("ORM preset", &["orm"])
            .pick_file()
        else {
            return;
        };
        let Ok(values) = crate::params::read_state_file(&path) else {
            return;
        };
        self.push_undo(snapshot(ctx.params()));
        apply_snapshot(ctx, &values);
    }

    // ------------------------------------------------------------------
    // 帧数据泵送
    // ------------------------------------------------------------------

    /// 把共享通道里的最新帧喂给 retained 控件；配置类 setters 每帧从参数重推。
    fn pump_shared(&mut self, ctx: &PluginContext<AnalyzerParams>) {
        let shared = &ctx.params().analysis.shared;
        let sr = shared.sample_rate_f64();
        let cpu = shared.cpu_f32() as f64;

        let level = shared.level.lock().map(|g| g.clone()).unwrap_or_default();
        let loud = shared.loudness.lock().map(|g| g.clone()).unwrap_or_default();
        let scope_frame = shared.scope.lock().map(|g| g.clone()).unwrap_or_default();
        let spectrum_opt = shared.spectrum.lock().map(|g| g.clone()).unwrap_or_default();

        let p = ctx.params();
        let mode_idx = p.mode.index() as i32;
        let release_mode = p.release_mode.index() as i32;

        // 频谱控件配置
        self.pad.set_sample_rate(sr);
        self.pad.set_fft_size(p.fft_size() as i32);
        self.pad.set_release(p.release_sec() as f32);
        self.pad.set_release_mode(release_mode);
        self.pad.set_range(p.range_db());
        self.pad.set_slope(p.effective_slope_db() as f32);
        self.pad.set_mode(mode_idx);
        self.pad.set_chan_mode(if p.channel_split() { 0 } else { 1 });
        self.pad.set_merge_algo(if p.channel_merge_algo() { 1 } else { 0 });
        self.pad.set_meter_mode(p.level_mode.index() as i32);
        self.pad.set_loud_target(p.loud_target_lufs() as f32);
        self.pad.set_loud_scale_off(p.loud_scale_lu() as f32);

        // 下半区控件配置
        self.field.set_sample_rate(sr);
        self.field.set_release_sec(p.release_sec() as f32);
        self.field.set_release_mode(release_mode);
        self.gonio.set_sample_rate(sr);
        self.scope.set_sample_rate(sr);

        // 帧摄入：模式不匹配的频谱帧直接丢弃（原版 OnMsgFromDelegate 同样按模式校验）。
        if let Some((tag, frame)) = spectrum_opt.as_ref() {
            if *tag == mode_idx as u8 {
                self.pad.update(frame, &level, &loud);
            }
        }
        if let Some(frame) = scope_frame.as_ref() {
            self.field.update(frame);
            self.gonio.update(frame);
            self.scope.update(frame);
        }
        self.cpu_meter.set_cpu(cpu);
    }

    // ------------------------------------------------------------------
    // 设置面板动作路由
    // ------------------------------------------------------------------

    fn route_panel_action(&mut self, ctx: &PluginContext<AnalyzerParams>, action: PanelAction) {
        let shared = &ctx.params().analysis.shared;
        match action {
            PanelAction::SetLang(_) => self.panel.save(),
            PanelAction::SetThemeMode(_) => self.panel.save(),
            PanelAction::SetHue(_) => self.panel.save(),
            PanelAction::SetSat(_) => self.panel.save(),
            PanelAction::GenType(t) => {
                shared.gen_type.store(t, Ordering::Relaxed);
                shared.gen_active.store(t != 0, Ordering::Relaxed);
                self.panel.save();
            }
            PanelAction::GenFreq(f) => {
                shared.gen_freq_hz.store(f.to_bits(), Ordering::Relaxed);
                self.panel.save();
            }
            PanelAction::GenLevel(db) => {
                shared.gen_level_db.store((db as f32).to_bits(), Ordering::Relaxed);
                self.panel.save();
            }
            PanelAction::GenHold(b) => {
                shared.gen_hold.store(b, Ordering::Relaxed);
                self.panel.save();
            }
            PanelAction::GenToOutput(b) => {
                shared.gen_to_output.store(b, Ordering::Relaxed);
                self.panel.save();
            }
            // 发生器重启：置请求位，音频线程在发生器分支 exchange(false) 取走后
            // 调 restart()（相位/PRNG/粉噪清零）；原版 Analyzer.cpp:650 不落盘。
            PanelAction::GenRestart => {
                shared.gen_restart.store(true, Ordering::Relaxed);
            }
            // 发生器重播种：原版 Analyzer.cpp:651-653 用 steady_clock tick 取掩码种子
            // （|1 保证非零奇数，& 0x7FFFFFFF 清符号位，避免塞进负值）；音频线程
            // exchange(0) 取走后 set_seed；同样不落盘。
            PanelAction::GenReseed => {
                let ticks = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_nanos() as u64)
                    .unwrap_or(0);
                let seed = ((ticks | 1) & 0x7FFF_FFFF) as i32;
                shared.gen_reseed.store(seed, Ordering::Relaxed);
            }
            // 音频设备 / 驱动选择属于宿主（APP_API）路由：面板内部状态已自更新，
            // 设备枚举与实际打开待宿主集成阶段接入，此处不伪造效果（宿主侧待办）。
            PanelAction::SetDriver(_) | PanelAction::SetAudioDevice { .. } => {}
            PanelAction::Closed => self.panel.save(),
        }
    }
}

// ---------------------------------------------------------------------------
// 入口
// ---------------------------------------------------------------------------

/// 冻结入口：truce 插件壳通过本函数构造编辑器。
pub fn create(params: Arc<AnalyzerParams>) -> Box<dyn Editor> {
    truce_egui::EguiEditor::with_ui(params, (UI_W as u32, UI_H as u32), AnalyzerUi::default())
        .with_font(fonts::REGULAR)
        .resizable(false)
        .into_editor()
}

// ---------------------------------------------------------------------------
// EditorUi 实现
// ---------------------------------------------------------------------------

impl EditorUi<AnalyzerParams> for AnalyzerUi {
    fn opened(&mut self, _ctx: &PluginContext<AnalyzerParams>) {
        self.font_stage = 0;
        self.styled = false;
    }

    fn ui(&mut self, ui: &mut egui::Ui, ctx: &PluginContext<AnalyzerParams>) {
        self.ensure_style(ui);

        let p = ctx.params();
        self.pump_shared(ctx);

        let theme = self.panel.theme;
        let lang = self.panel.lang;

        // 背景：col_100 铺满 960×635。
        let (full, _) = ui.allocate_exact_size(Vec2::new(UI_W, UI_H), Sense::hover());
        ui.painter_at(full).rect_filled(full, 0.0, theme.col_100());

        let clicked = ui.input(|i| i.pointer.any_click());
        let pointer = ui.ctx().pointer_interact_pos();

        // 空闲时刷新稳定快照（手势间隙撤销基准）。
        if !ui.input(|i| i.pointer.any_down()) {
            self.last_idle = snapshot(p);
        }

        let tr = |lit: &str| strings::analyzer_label(lit, lang);

        // ==================== 顶行 7 按钮 ====================

        // 1) 通道模式 PWR / L/R / SUM —— splitChannels 样式。
        {
            let labels: Vec<String> = ["PWR", "L/R", "SUM"]
                .iter()
                .map(|s| tr(s))
                .collect();
            let resp = CycleButton::new(
                self.layout.chan_mode,
                &labels,
                p.channel_mode.index() as usize,
            )
            .split_channels_at(1)
            .draw(ui, &theme, Id::new("analyzer_chan_mode"));
            if resp.clicked() {
                self.cycle(ctx, P::ChannelMode.id(), 3);
            }
        }

        // 2) 引擎模式 STFT / VQT / PBT / RTA。
        {
            let labels: Vec<String> = ["STFT", "VQT", "PBT", "RTA"]
                .iter()
                .map(|s| s.to_string())
                .collect();
            let resp = CycleButton::new(
                self.layout.mode,
                &labels,
                p.mode.index() as usize,
            )
            .draw(ui, &theme, Id::new("analyzer_mode"));
            if resp.clicked() {
                self.cycle(ctx, P::Mode.id(), 4);
            }
        }

        let mode_idx = p.mode.index() as i32;

        // 3) 分辨率槽：按引擎换绑 Res / VqtGamma / LfRes / RtaOctave（互斥可见）。
        {
            let (res_id, res_idx) = match mode_idx {
                mode::VQT => (P::VqtGamma.id(), p.vqt_gamma.index() as usize),
                mode::PBT => (P::LfRes.id(), p.lf_res.index() as usize),
                mode::RTA => (P::RtaOctave.id(), p.rta_octave.index() as usize),
                _ => (P::Res.id(), p.res.index() as usize),
            };
            let labels: Vec<String> = ["L", "M", "H"].iter().map(|s| tr(s)).collect();
            let resp = CycleButton::new(self.layout.res, &labels, res_idx)
                .draw(ui, &theme, Id::new("analyzer_res"));
            if resp.clicked() {
                self.cycle(ctx, res_id, 3);
            }
        }

        // 4) 窗型 SHARP / CLEAN：仅 STFT / VQT 显示。
        if mode_idx == mode::FFT || mode_idx == mode::VQT {
            let (win_id, win_idx) = if mode_idx == mode::FFT {
                (P::FftWindow.id(), p.fft_window.index() as usize)
            } else {
                (P::WindowVqt.id(), p.window_vqt.index() as usize)
            };
            let labels: Vec<String> = ["SHARP", "CLEAN"]
                .iter()
                .map(|s| tr(s))
                .collect();
            let resp = CycleButton::new(self.layout.window_btn, &labels, win_idx)
                .draw(ui, &theme, Id::new("analyzer_window"));
            if resp.clicked() {
                self.cycle(ctx, win_id, 2);
            }
        }

        // 5) 释放模式 LOG / LIN。
        {
            let labels: Vec<String> = ["LOG", "LIN"]
                .iter()
                .map(|s| tr(s))
                .collect();
            let resp = CycleButton::new(
                self.layout.release_mode,
                &labels,
                p.release_mode.index() as usize,
            )
            .draw(ui, &theme, Id::new("analyzer_release"));
            if resp.clicked() {
                self.cycle(ctx, P::ReleaseMode.id(), 2);
            }
        }

        // 6) 斜率：FFT 用 dB/Oct 档；VQT/PBT/RTA 用带斜率档。
        {
            let (slope_id, slope_idx, labels): (u32, usize, Vec<String>) = match mode_idx {
                mode::FFT => (
                    P::SlopeFft.id(),
                    p.slope_fft.index() as usize,
                    ["0 dB/Oct", "3 dB/Oct", "4.5 dB/Oct"]
                        .iter()
                        .map(|s| s.to_string())
                        .collect(),
                ),
                mode::VQT => (
                    P::SlopeVqt.id(),
                    p.slope_vqt.index() as usize,
                    band_slope_labels(),
                ),
                mode::PBT => (
                    P::SlopePbt.id(),
                    p.slope_pbt.index() as usize,
                    band_slope_labels(),
                ),
                _ => (
                    P::SlopeRta.id(),
                    p.slope_rta.index() as usize,
                    band_slope_labels(),
                ),
            };
            let resp = CycleButton::new(self.layout.slope, &labels, slope_idx)
                .draw(ui, &theme, Id::new("analyzer_slope"));
            if resp.clicked() {
                self.cycle(ctx, slope_id, 3);
            }
        }

        // 7) 速度 MIN / SLOW / MED / FAST / MAX。
        {
            let labels: Vec<String> = ["MIN", "SLOW", "MED", "FAST", "MAX"]
                .iter()
                .map(|s| tr(s))
                .collect();
            let resp = CycleButton::new(
                self.layout.speed,
                &labels,
                p.speed.index() as usize,
            )
            .draw(ui, &theme, Id::new("analyzer_speed"));
            if resp.clicked() {
                self.cycle(ctx, P::Speed.id(), 5);
            }
        }

        // ==================== 主频谱 ====================

        self.pad
            .draw(&ui.painter_at(self.layout.plot), &theme, lang);

        // 频谱命中：电平表复位 / 响度刻度循环。
        if let Some(pos) = pointer {
            if self.layout.plot.contains(pos) {
                if let Some(tip) = self.pad.hover(pos) {
                    let _ = tip;
                }
                if clicked {
                    if let Some(click) = self.pad.click(pos) {
                        use crate::ui::spectrum::MeterClick::*;
                        match click {
                            ResetPersist => shared_reset(
                                ctx,
                                |s| s.reset_persist.store(true, Ordering::Relaxed),
                            ),
                            ResetMeterHold => shared_reset(
                                ctx,
                                |s| s.reset_meter_hold.store(true, Ordering::Relaxed),
                            ),
                            ResetOver => shared_reset(
                                ctx,
                                |s| s.reset_over.store(true, Ordering::Relaxed),
                            ),
                            LoudScale => self.cycle(ctx, P::LoudScale.id(), 2),
                            LoudPreset => self.cycle(ctx, P::LoudPreset.id(), 4),
                        }
                    }
                }
            }
        }

        // RANGE 刻度按钮（scale_style）+ 电平模式幽灵按钮（dBTP / dBFS）。
        {
            let labels: Vec<String> = ["-80", "-100", "-120"]
                .iter()
                .map(|s| s.to_string())
                .collect();
            let resp = CycleButton::new(
                self.layout.range_btn,
                &labels,
                p.range.index() as usize,
            )
            .scale_style()
            .draw(ui, &theme, Id::new("analyzer_range"));
            if resp.clicked() {
                self.cycle(ctx, P::Range.id(), 3);
            }
        }
        {
            let labels: Vec<String> = ["dBTP", "dBFS"]
                .iter()
                .map(|s| s.to_string())
                .collect();
            // 幽灵信号态：电平表有未到底的读数（原版 mGhostSignalActive）。
            let signal_active = ctx
                .params()
                .analysis
                .shared
                .level
                .lock()
                .map(|g| {
                    g.peak_db_l > -119.0
                        || g.peak_db_r > -119.0
                        || g.true_db_l > -119.0
                        || g.true_db_r > -119.0
                })
                .unwrap_or(false);
            let resp = CycleButton::new(
                self.layout.level_mode,
                &labels,
                p.level_mode.index() as usize,
            )
            .ghost(signal_active)
            .text_px(11.0)
            .draw(ui, &theme, Id::new("analyzer_level_mode"));
            if resp.clicked() {
                self.cycle(ctx, P::LevelMode.id(), 2);
            }
        }

        // ==================== 下半区按钮行 ====================

        // FIELD / VECTOR 互斥。
        {
            let labels: Vec<String> = ["FIELD", "VECTOR"]
                .iter()
                .map(|s| s.to_string())
                .collect();
            let resp = CycleButton::new(self.layout.scope_mode, &labels, self.scope_mode as usize)
                .draw(ui, &theme, Id::new("analyzer_scope_mode"));
            if resp.clicked() {
                self.scope_mode = 1 - self.scope_mode;
                if self.scope_mode == 1 {
                    self.gonio.clear_peak_hold();
                }
            }
        }

        let vector = self.scope_mode == 1;

        // 矢量余辉 / 倍率：仅 VECTOR 模式显示。
        if vector {
            let pers_labels: Vec<String> = ["PERS OFF", "PERS S", "PERS L"]
                .iter()
                .map(|s| s.to_string())
                .collect();
            let resp = CycleButton::new(
                self.layout.gonio_persist,
                &pers_labels,
                self.gonio_persist as usize,
            )
            .text_px(11.0)
            .draw(ui, &theme, Id::new("analyzer_gonio_persist"));
            if resp.clicked() {
                self.gonio_persist = (self.gonio_persist + 1) % 3;
                self.gonio.set_persist(self.gonio_persist);
            }

            let zoom_labels: Vec<String> = ["x1", "x2", "x4"]
                .iter()
                .map(|s| s.to_string())
                .collect();
            let resp = CycleButton::new(
                self.layout.gonio_zoom,
                &zoom_labels,
                self.gonio_zoom as usize,
            )
            .draw(ui, &theme, Id::new("analyzer_gonio_zoom"));
            if resp.clicked() {
                self.gonio_zoom = (self.gonio_zoom + 1) % 3;
                self.gonio.set_zoom_idx(self.gonio_zoom);
            }
        }

        // 示波器声道 M / L / R（分色档）。
        {
            let (cl, cr, cm) = theme.channel_colors();
            let labels: Vec<String> = ["M", "L", "R"]
                .iter()
                .map(|s| s.to_string())
                .collect();
            let mut btn = CycleButton::new(
                self.layout.scope_chan,
                &labels,
                self.scope_chan as usize,
            );
            btn.index_colors = Some(vec![cm, cl, cr]);
            let resp = btn.draw(ui, &theme, Id::new("analyzer_scope_chan"));
            if resp.clicked() {
                self.scope_chan = (self.scope_chan + 1) % 3;
                let bit = [scope::chan::M, scope::chan::L, scope::chan::R][self.scope_chan as usize];
                self.scope.set_chan_mask(bit);
            }
        }

        // 触发源 ROLL / SYNC / SWEEP。
        {
            let labels: Vec<String> = ["ROLL", "SYNC", "SWEEP"]
                .iter()
                .map(|s| tr(s))
                .collect();
            let resp = CycleButton::new(
                self.layout.scope_trig,
                &labels,
                self.scope_trig as usize,
            )
            .draw(ui, &theme, Id::new("analyzer_scope_trig"));
            if resp.clicked() {
                self.scope_trig = (self.scope_trig + 1) % 3;
                self.scope.set_trig_source(self.scope_trig);
            }
        }

        // ==================== 下半区画区 ====================

        if vector {
            self.gonio
                .draw(&ui.painter_at(self.layout.scope_panel), &theme, lang);
        } else {
            self.field
                .draw(&ui.painter_at(self.layout.scope_panel), &theme, lang);
        }

        self.scope
            .draw(&ui.painter_at(self.layout.oscilloscope), &theme, lang);

        // 示波器双击复位幅度倍率。
        if let Some(pos) = pointer {
            if self.layout.oscilloscope.contains(pos) {
                let _ = self.scope.hover(pos);
                if clicked {
                    if let Some(scope::ScopeAction::ResetZoom) = self.scope.click(pos) {
                        self.scope.set_zoom_factor(2.0);
                    }
                }
            }
        }

        // 示波器内嵌滑条：幅度倍率（纵向）/ 时基（横向）。
        self.zoom_slider.rect = self.scope.zoom_slider_rect();
        self.zoom_slider.value =
            (self.scope.zoom_factor().log2() / 3.0).clamp(0.0, 1.0);
        let zoom_v = self.zoom_slider.ui(ui, &theme, Id::new("analyzer_scope_zoom_slider"));
        self.scope.set_zoom_factor(2.0f32.powf(3.0 * zoom_v));

        self.time_slider.rect = self.scope.time_slider_rect();
        self.time_slider.value =
            (((self.scope.window_sec() / 0.010).ln() / 200.0_f64.ln()) as f32).clamp(0.0, 1.0);
        let time_v = self.time_slider.ui(ui, &theme, Id::new("analyzer_scope_time_slider"));
        self.scope
            .set_window_sec(0.010 * 200.0f64.powf(time_v as f64));

        // ==================== 右栏 ====================

        self.cpu_meter.draw(&ui.painter_at(self.layout.cpu), &theme);

        // 图标行：undo / redo / save / load（共享 SVG 路径渲染器）。
        if widgets::icon_button(
            ui,
            self.layout.icon_undo,
            Icon::Undo,
            &theme,
            Id::new("analyzer_undo"),
            None,
        ) {
            self.undo(ctx);
        }
        if widgets::icon_button(
            ui,
            self.layout.icon_redo,
            Icon::Redo,
            &theme,
            Id::new("analyzer_redo"),
            None,
        ) {
            self.redo(ctx);
        }
        if widgets::icon_button(
            ui,
            self.layout.icon_save,
            Icon::Save,
            &theme,
            Id::new("analyzer_save"),
            None,
        ) {
            self.save_preset(ctx);
        }
        if widgets::icon_button(
            ui,
            self.layout.icon_load,
            Icon::Load,
            &theme,
            Id::new("analyzer_load"),
            None,
        ) {
            self.load_preset(ctx);
        }

        // FREEZE 开关 + RESET 动作。
        let frozen = p.freeze.value();
        if toggle_button(ui, &theme, self.layout.freeze, &tr("FREEZE"), frozen, Id::new("analyzer_freeze"))
            .clicked()
        {
            let next = !frozen;
            self.push_undo(self.stable_snapshot(ctx.params()));
            write_param(ctx, P::Freeze.id(), next as i32 as f64);
            self.scope.set_frozen(next);
        }
        if action_button(ui, &theme, self.layout.reset, &tr("RESET"), Id::new("analyzer_reset"))
            .clicked()
        {
            let shared = &ctx.params().analysis.shared;
            shared.reset_level.store(true, Ordering::Relaxed);
            shared.reset_loud.store(true, Ordering::Relaxed);
            shared.reset_hold.store(true, Ordering::Relaxed);
            self.pad.clear_peak_hold();
            self.field.clear_peak_hold();
            self.gonio.clear_peak_hold();
            self.scope.clear();
        }

        // HOLD 开关 + 保持时长 0.5s / 2s / ∞。
        let hold_on = p.level_hold_on.value();
        if toggle_button(
            ui,
            &theme,
            self.layout.hold_toggle,
            &tr("HOLD"),
            hold_on,
            Id::new("analyzer_hold"),
        )
        .clicked()
        {
            self.push_undo(self.stable_snapshot(ctx.params()));
            write_param(ctx, P::LevelHoldOn.id(), (!hold_on) as i32 as f64);
        }
        {
            let labels: Vec<String> = ["0.5s", "2s", "∞"]
                .iter()
                .map(|s| s.to_string())
                .collect();
            let resp = CycleButton::new(
                self.layout.hold_time,
                &labels,
                p.level_hold.index() as usize,
            )
            .draw(ui, &theme, Id::new("analyzer_hold_time"));
            if resp.clicked() {
                self.cycle(ctx, P::LevelHold.id(), 3);
            }
        }

        // ==================== 标题区 ====================

        let pnt = ui.painter_at(full);
        let orm_font = fonts::bold(32.0);
        let orm_ink_w = pnt
            .layout_no_wrap("ORM".to_owned(), orm_font.clone(), theme.col_900())
            .size()
            .x;
        pnt.text(
            self.layout.title_orm.left_center(),
            Align2::LEFT_CENTER,
            "ORM",
            orm_font,
            theme.col_900(),
        );

        let gear = self.layout.gear_rect(orm_ink_w);
        if widgets::gear_button(ui, gear, &theme, Id::new("analyzer_gear"), None) {
            self.panel.toggle();
        }

        let version_rect = self.layout.version_rect(gear);
        pnt.text(
            Pos2::new(version_rect.min.x, version_rect.max.y),
            Align2::LEFT_BOTTOM,
            "v0.1.0",
            fonts::regular(20.0),
            theme.col_500(),
        );
        pnt.text(
            self.layout.title_analyzer.left_center(),
            Align2::LEFT_CENTER,
            "Analyzer",
            fonts::bold(32.0),
            theme.col_900(),
        );

        // ==================== 设置面板（模态） ====================

        self.panel.hold_on = hold_on as i32;
        let actions = self.panel.draw(ui, self.layout.window);
        for action in actions {
            self.route_panel_action(ctx, action);
        }
    }

    fn state_changed(&mut self, ctx: &PluginContext<AnalyzerParams>) {
        self.last_idle = snapshot(ctx.params());
    }
}

// ---------------------------------------------------------------------------
// 小工具
// ---------------------------------------------------------------------------

/// 带斜率档标签（VQT / PBT / RTA 共用）。
fn band_slope_labels() -> Vec<String> {
    ["-3 dB/Oct", "0 dB/Oct", "1.5 dB/Oct"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

/// 置位一条共享复位请求位。
fn shared_reset<F: FnOnce(&crate::analysis::AnalyzerShared)>(
    ctx: &PluginContext<AnalyzerParams>,
    f: F,
) {
    f(&ctx.params().analysis.shared);
}
