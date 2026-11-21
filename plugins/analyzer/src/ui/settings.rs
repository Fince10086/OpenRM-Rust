//! 设置面板（模态卡片 + 遮罩）。
//!
//! 移植自原版 `plugins/Analyzer/src/controls/SettingsPanelControl.h`（708 行）：
//! 语言切换 / Light-Dark 主题 / 色相 / 饱和度 / 音频驱动与输入输出行 /
//! Developer 内置测试信号区。卡片在窗口内居中（原版 `mCard` 由 `bounds.MW()/MH()`
//! 反推中心，SettingsPanelControl.h:102-103）。
//!
//! 几何常量逐项对应原版 `static constexpr`（SettingsPanelControl.h:637-679），
//! 行为对应 `OnMouseDown / OnMouseDrag / Draw / DrawButton / DrawSlider /
//! DrawDeviceRow / DragTo`。面板只持有状态并返回 [`PanelAction`]，真正写参数、
//! 落盘与菜单宿主路由由集成层（`editor.rs`）完成。

use egui::{Align2, Id, Pos2, Rect, Sense, Vec2};
use openrm_analyzer_dsp::test_signal::TestSignalType;
use openrm_ui::{
    fonts,
    strings::{self, Key, Lang},
    theme::{Theme, ThemeMode},
};

use crate::params::{self, SettingsData};

// ===== SettingsPanelControl.h:637-646 卡片与控件尺寸 =====
const K_CARD_W: f32 = 380.0;
const K_CARD_H: f32 = 270.0;
const K_CARD_H_AUDIO: f32 = 392.0;
const K_PAD: f32 = 20.0;
const K_BTN_H: f32 = 30.0;
const K_BTN_GAP: f32 = 0.0; // 原版 :642；按钮列间距为 0
const K_TITLE_SIZE: f32 = 20.0;
const K_HEADER_FONT_SIZE: f32 = 20.0;
const K_HEADER_H: f32 = 26.0;
const K_TRACK_H: f32 = 26.0;

// ===== SettingsPanelControl.h:647-659 基础区纵向偏移（相对卡片顶） =====
const K_LANG_TITLE_Y: f32 = 14.0;
const K_LANG_BTN_Y: f32 = 38.0;
const K_THEME_TITLE_Y: f32 = 80.0;
const K_THEME_BTN_Y: f32 = 104.0;
const K_HUE_TITLE_Y: f32 = 148.0;
const K_HUE_Y: f32 = 174.0;
const K_SAT_TITLE_Y: f32 = 206.0;
const K_SAT_Y: f32 = 232.0;
const K_AUDIO_TITLE_Y: f32 = 262.0;
const K_DRIVER_ROW_Y: f32 = 288.0;
const K_AUDIO_ROW1_Y: f32 = 320.0;
const K_AUDIO_ROW2_Y: f32 = 352.0;
const K_AUDIO_ROW_H: f32 = 28.0;

// ===== SettingsPanelControl.h:662-671 Developer 区相对基准 Y 的偏移 =====
const K_DEV_BTN_GAP: f32 = 8.0;
const K_DEV_TITLE_OFF: f32 = 12.0;
const K_DEV_SIGNAL_OFF: f32 = 36.0;
const K_DEV_FREQ_HDR_OFF: f32 = 70.0;
const K_DEV_FREQ_OFF: f32 = 96.0;
const K_DEV_LEVEL_HDR_OFF: f32 = 128.0;
const K_DEV_LEVEL_OFF: f32 = 154.0;
const K_DEV_BTN_OFF: f32 = 188.0;
const K_DEV_TOGGLE_OFF: f32 = 224.0;
const K_DEV_H: f32 = 266.0;

// ===== SettingsPanelControl.h:673-679 色相 / 饱和档位 =====
const K_HUE_MIN: i32 = 15;
const K_HUE_MAX: i32 = 360;
const K_HUE_STEP: i32 = 15;
const K_SAT_MIN: i32 = 0;
const K_SAT_MAX: i32 = 50;
const K_NUM_SAT: usize = 4;
const K_SAT_VALS: [u8; K_NUM_SAT] = [0, 15, 30, 50];

/// 色板尺寸与间隙（common/Theme.h `AG_SWATCH / AG_SWATCH_GAP`）。
const SWATCH: f32 = 14.0;
const SWATCH_GAP: f32 = 8.0;

/// 遮罩色：原版 `IColor(70, 26, 25, 22)`（alpha, r, g, b）。
fn mask_color() -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(26, 25, 22, 70)
}

// ---------------------------------------------------------------------------
// 状态
// ---------------------------------------------------------------------------

/// 音频设备绑定（原版 `mHasAudio / mHasDriver` 对应的 hooks 数据）。
///
/// 集成层在宿主可用时填充选项与当前名；列表为空 / 开关为 false 时，面板按原版
/// `mHasAudio = list && current && onDevice` 的判定隐藏对应行。
#[derive(Clone, Debug, Default)]
pub struct AudioState {
    pub driver: String,
    pub input: String,
    pub output: String,
    pub driver_options: Vec<String>,
    pub input_options: Vec<String>,
    pub output_options: Vec<String>,
    pub show_driver: bool,
    pub show_devices: bool,
}

/// 内置测试信号发生器配置（对应原版 `mGen*` 原子量 + `TestSignalGenerator::Config`）。
///
/// 原版面板只暴露 类型 / 频率 / 电平 / RESTART / RESEED / 送到输出 / HOLD；
/// `fft_size / sweep_sec / sweep_f0 / sweep_f1` 由 DSP 配置默认值与集成层接线，
/// 面板不画对应控件（见 SettingsPanelControl.h:205-222）。
#[derive(Clone, Debug)]
pub struct GenState {
    pub type_: i32,
    pub freq: f64,
    pub level: f64,
    pub hold: bool,
    pub to_output: bool,
    pub fft_size: i32,
    pub sweep_sec: f64,
    pub sweep_f0: f64,
    pub sweep_f1: f64,
}

impl Default for GenState {
    fn default() -> Self {
        Self {
            type_: 0,
            freq: 1000.0,
            level: -12.0,
            hold: false,
            to_output: false,
            fft_size: 4096,
            sweep_sec: 4.0,
            sweep_f0: 20.0,
            sweep_f1: 20_000.0,
        }
    }
}

/// 面板返回给集成层的语义动作（对应原版 Hooks 的各 `std::function`）。
#[derive(Clone, Debug, PartialEq)]
pub enum PanelAction {
    SetLang(Lang),
    SetThemeMode(ThemeMode),
    SetHue(u16),
    SetSat(u8),
    GenType(i32),
    GenFreq(f64),
    GenLevel(f64),
    GenHold(bool),
    GenToOutput(bool),
    GenRestart,
    GenReseed,
    SetDriver(String),
    SetAudioDevice { input: bool, name: String },
    /// 遮罩外部点击 / ESC：关闭面板（原版 `SetVisible(false)`）。
    Closed,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum MenuKind {
    GenType,
    Driver,
    AudioIn,
    AudioOut,
}

/// 设置面板 retained 控件。
pub struct SettingsPanel {
    /// 面板是否展开（原版 `mSettingsPanel->IsHidden()` 取反）。
    pub open: bool,
    pub lang: Lang,
    pub theme: Theme,
    /// 电平表保持开关（参数层所有，面板只读透传，落盘时不覆盖）。
    pub hold_on: i32,
    pub gen: GenState,
    pub audio: AudioState,
    /// 是否显示 Developer 测试信号区（原版 `ORM_ENABLE_TEST_GEN` 编译开关）。
    pub show_gen: bool,
    /// 频率 / 电平拖动未稳定落盘标记（原版 `mGenSavePending`，OnIdle 后写盘）。
    pub gen_save_pending: bool,

    menu: Option<MenuKind>,
}

impl Default for SettingsPanel {
    fn default() -> Self {
        Self::load()
    }
}

impl SettingsPanel {
    /// 从设置文件构造（等价原版构造时 `loadSettings()` 回填 ThemeHue/ThemeSatMax/
    /// mThemeMode/`mGen*`；`params.rs::load_settings` 缺失键保留默认值）。
    pub fn load() -> Self {
        let d = params::load_settings();
        let lang = match d.lang {
            1 => Lang::Zh,
            0 => Lang::En,
            _ => strings::detect_lang(),
        };
        let theme = Theme {
            mode: if d.theme_mode == 1 {
                ThemeMode::Dark
            } else {
                ThemeMode::Light
            },
            hue: d.hue.clamp(K_HUE_MIN, K_HUE_MAX) as u16,
            sat_max: d.sat_max.clamp(K_SAT_MIN, K_SAT_MAX) as u8,
        };
        let gen = GenState {
            type_: d.gen_type,
            freq: d.gen_freq,
            level: d.gen_level,
            hold: d.gen_hold != 0,
            to_output: d.gen_to_output != 0,
            ..GenState::default()
        };
        Self {
            open: false,
            lang,
            theme,
            hold_on: d.hold_on,
            gen,
            audio: AudioState::default(),
            show_gen: true,
            gen_save_pending: false,
            menu: None,
        }
    }

    /// 导出当前面板状态为设置文件结构（`hold_on` 属于电平表参数层，由集成层合并）。
    pub fn collect(&self) -> SettingsData {
        SettingsData {
            lang: match self.lang {
                Lang::Zh => 1,
                Lang::En => 0,
            },
            hue: self.theme.hue as i32,
            sat_max: self.theme.sat_max as i32,
            theme_mode: match self.theme.mode {
                ThemeMode::Dark => 1,
                ThemeMode::Light => 0,
            },
            hold_on: self.hold_on,
            gen_type: self.gen.type_,
            gen_freq: self.gen.freq,
            gen_level: self.gen.level,
            gen_hold: self.gen.hold as i32,
            gen_to_output: self.gen.to_output as i32,
        }
    }

    /// 立即写设置文件（任何失败静默，等价原版 `SaveSettingsToDisk` 不抛异常）。
    pub fn save(&self) {
        let _ = params::save_settings(&self.collect());
    }

    pub fn open_panel(&mut self) {
        self.open = true;
        self.menu = None;
    }

    pub fn close_panel(&mut self) {
        self.open = false;
        self.menu = None;
    }

    pub fn toggle(&mut self) {
        if self.open {
            self.close_panel();
        } else {
            self.open_panel();
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    // ------------------------------------------------------------------
    // 几何
    // ------------------------------------------------------------------

    fn has_audio(&self) -> bool {
        self.audio.show_devices || self.audio.show_driver
    }

    /// 卡片高度：原版 `baseH = (mHasAudio||mHasDriver) ? kCardHAudio : kCardH`，
    /// 有发生器再叠 `kDevH`（SettingsPanelControl.h:100-101）。
    fn card_h(&self) -> f32 {
        let base = if self.has_audio() { K_CARD_H_AUDIO } else { K_CARD_H };
        base + if self.show_gen { K_DEV_H } else { 0.0 }
    }

    /// 卡片矩形（窗口内居中；原版 `mCard` 由 `bounds.MW()/MH()` 反推中心）。
    /// 公开给集成层做遮罩命中 / 置顶判断。
    pub fn card_rect(&self, window: Rect) -> Rect {
        Rect::from_center_size(
            window.center(),
            Vec2::new(K_CARD_W, self.card_h()),
        )
    }

    /// 命中卡片本体（原版 `mCard.Contains(mMouse)` 判定）。
    pub fn hit_card(&self, window: Rect, pos: Pos2) -> bool {
        self.card_rect(window).contains(pos)
    }

    // ------------------------------------------------------------------
    // 主绘制
    // ------------------------------------------------------------------

    /// 绘制遮罩 + 卡片，处理点击 / 拖拽 / 菜单 / ESC；返回本帧语义动作。
    pub fn draw(&mut self, ui: &mut egui::Ui, window: Rect) -> Vec<PanelAction> {
        let mut actions = Vec::new();
        if !self.open {
            return actions;
        }

        let card = self.card_rect(window);
        let l = card.left() + K_PAD;
        let r = card.right() - K_PAD;
        let theme = self.theme;

        // 模态遮罩（原版 FillRect(IColor(70,26,25,22), mRECT)，:150）。
        ui.painter_at(window).rect_filled(window, 0.0, mask_color());

        // 卡片底 + 边框（:152-153）。
        let pnt = ui.painter_at(card);
        pnt.rect_filled(card, 0.0, theme.col_100());
        pnt.rect_stroke(
            card,
            0.0,
            egui::Stroke::new(1.0_f32, theme.col_300()),
            egui::StrokeKind::Inside,
        );

        self.draw_language(ui, card, l, r, &mut actions);
        self.draw_theme(ui, card, l, r, &mut actions);
        self.draw_hue(ui, card, l, r, &mut actions);
        self.draw_saturation(ui, card, l, r, &mut actions);
        if self.has_audio() {
            self.draw_audio(ui, card, l, r);
        }
        if self.show_gen {
            self.draw_gen(ui, card, l, r, &mut actions);
        }

        // 菜单（发生器类型 / 驱动 / 输入 / 输出）。
        self.draw_menu(ui, &mut actions);

        // ESC 关闭（原版 C++ 未注册 ESC 快捷键；本实现按任务契约补上）。
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.close_panel();
            actions.push(PanelAction::Closed);
        }

        // 点击遮罩外部关闭（原版 OnMouseDown:227-230）。
        let bg = ui.interact(window, Id::new("settings_mask"), Sense::click());
        if bg.clicked() {
            if let Some(pos) = ui.ctx().pointer_interact_pos() {
                if !card.contains(pos) {
                    self.close_panel();
                    actions.push(PanelAction::Closed);
                }
            }
        }

        actions
    }

    // ----- 语言 -----

    fn draw_language(
        &mut self,
        ui: &mut egui::Ui,
        card: Rect,
        l: f32,
        r: f32,
        actions: &mut Vec<PanelAction>,
    ) {
        let theme = self.theme;
        let title_rect = Rect::from_min_max(
            Pos2::new(l, card.top() + K_LANG_TITLE_Y),
            Pos2::new(r, card.top() + K_LANG_TITLE_Y + K_TITLE_SIZE),
        );
        text_near(
            ui,
            title_rect,
            &strings::analyzer_label("LANGUAGE", self.lang),
            fonts::semibold(K_TITLE_SIZE),
            theme.col_900(),
        );

        let bw = (K_CARD_W - 2.0 * K_PAD - K_BTN_GAP) * 0.5;
        let y0 = card.top() + K_LANG_BTN_Y;
        let zh_rect = Rect::from_min_max(Pos2::new(l, y0), Pos2::new(l + bw, y0 + K_BTN_H));
        let en_rect = Rect::from_min_max(
            Pos2::new(l + bw + K_BTN_GAP, y0),
            Pos2::new(r, y0 + K_BTN_H),
        );

        if button(
            ui,
            zh_rect,
            &strings::analyzer_label("中文", self.lang),
            self.lang == Lang::Zh,
            &theme,
            Id::new("set_zh"),
            true,
        ) {
            self.lang = Lang::Zh;
            actions.push(PanelAction::SetLang(Lang::Zh));
        }
        if button(
            ui,
            en_rect,
            "ENGLISH",
            self.lang == Lang::En,
            &theme,
            Id::new("set_en"),
            true,
        ) {
            self.lang = Lang::En;
            actions.push(PanelAction::SetLang(Lang::En));
        }
    }

    // ----- 主题 Light / Dark -----

    fn draw_theme(
        &mut self,
        ui: &mut egui::Ui,
        card: Rect,
        l: f32,
        r: f32,
        actions: &mut Vec<PanelAction>,
    ) {
        let theme = self.theme;
        let title_rect = Rect::from_min_max(
            Pos2::new(l, card.top() + K_THEME_TITLE_Y),
            Pos2::new(r, card.top() + K_THEME_TITLE_Y + K_TITLE_SIZE),
        );
        text_near(
            ui,
            title_rect,
            &strings::analyzer_label("THEME", self.lang),
            fonts::semibold(K_TITLE_SIZE),
            theme.col_900(),
        );

        let bw = (K_CARD_W - 2.0 * K_PAD - K_BTN_GAP) * 0.5;
        let y0 = card.top() + K_THEME_BTN_Y;
        let dark_rect = Rect::from_min_max(Pos2::new(l, y0), Pos2::new(l + bw, y0 + K_BTN_H));
        let light_rect = Rect::from_min_max(
            Pos2::new(l + bw + K_BTN_GAP, y0),
            Pos2::new(r, y0 + K_BTN_H),
        );

        if button(
            ui,
            dark_rect,
            &strings::analyzer_label("DARK", self.lang),
            self.theme.mode == ThemeMode::Dark,
            &theme,
            Id::new("set_dark"),
            true,
        ) {
            self.theme.mode = ThemeMode::Dark;
            actions.push(PanelAction::SetThemeMode(ThemeMode::Dark));
        }
        if button(
            ui,
            light_rect,
            &strings::analyzer_label("LIGHT", self.lang),
            self.theme.mode == ThemeMode::Light,
            &theme,
            Id::new("set_light"),
            true,
        ) {
            self.theme.mode = ThemeMode::Light;
            actions.push(PanelAction::SetThemeMode(ThemeMode::Light));
        }
    }

    // ----- 色相 -----

    fn draw_hue(
        &mut self,
        ui: &mut egui::Ui,
        card: Rect,
        l: f32,
        r: f32,
        actions: &mut Vec<PanelAction>,
    ) {
        let theme = self.theme;
        let hdr_y = card.top() + K_HUE_TITLE_Y;
        let hdr = Rect::from_min_max(Pos2::new(l, hdr_y), Pos2::new(r, hdr_y + K_HEADER_H));

        let sw = Rect::from_min_max(
            Pos2::new(l, hdr.center().y - SWATCH * 0.5),
            Pos2::new(l + SWATCH, hdr.center().y + SWATCH * 0.5),
        );
        ui.painter_at(hdr)
            .rect_filled(sw, 0.0, Theme::hsb(self.theme.hue, 0.85, 1.0));

        text_near(
            ui,
            Rect::from_min_max(Pos2::new(l + SWATCH + SWATCH_GAP, hdr.top()), Pos2::new(hdr.center().x, hdr.bottom())),
            &strings::analyzer_label("Hue", self.lang),
            fonts::regular(K_HEADER_FONT_SIZE),
            theme.col_900(),
        );
        let hue_label = format!("{}\u{00B0}", self.theme.hue);
        text_far(
            ui,
            hdr,
            &hue_label,
            fonts::regular(K_HEADER_FONT_SIZE),
            theme.col_700(),
        );

        let track = Rect::from_min_max(
            Pos2::new(l, card.top() + K_HUE_Y),
            Pos2::new(r, card.top() + K_HUE_Y + K_TRACK_H),
        );
        let norm = (self.theme.hue as f32 - K_HUE_MIN as f32) / (K_HUE_MAX - K_HUE_MIN) as f32;
        draw_slider(ui, track, norm, &theme);

        let resp = ui.interact(track, Id::new("set_hue"), Sense::click_and_drag());
        if resp.dragged() || resp.clicked() {
            if let Some(pos) = ui.ctx().pointer_interact_pos() {
                let n = ((pos.x - track.left()) / track.width()).clamp(0.0, 1.0);
                let hue = K_HUE_MIN
                    + (n * (K_HUE_MAX - K_HUE_MIN) as f32 / K_HUE_STEP as f32).round() as i32
                        * K_HUE_STEP;
                let hue = hue.clamp(K_HUE_MIN, K_HUE_MAX) as u16;
                if hue != self.theme.hue {
                    self.theme.hue = hue;
                    actions.push(PanelAction::SetHue(hue));
                }
            }
        }
    }

    // ----- 饱和度 -----

    fn draw_saturation(
        &mut self,
        ui: &mut egui::Ui,
        card: Rect,
        l: f32,
        r: f32,
        actions: &mut Vec<PanelAction>,
    ) {
        let theme = self.theme;
        let hdr_y = card.top() + K_SAT_TITLE_Y;
        let hdr = Rect::from_min_max(Pos2::new(l, hdr_y), Pos2::new(r, hdr_y + K_HEADER_H));

        let sw = Rect::from_min_max(
            Pos2::new(l, hdr.center().y - SWATCH * 0.5),
            Pos2::new(l + SWATCH, hdr.center().y + SWATCH * 0.5),
        );
        let seg_h = SWATCH / K_NUM_SAT as f32;
        for i in 0..K_NUM_SAT {
            let b = 1.0 - 0.25 * i as f32;
            let seg = Rect::from_min_max(
                Pos2::new(sw.left(), sw.top() + i as f32 * seg_h),
                Pos2::new(sw.right(), sw.top() + (i + 1) as f32 * seg_h),
            );
            ui.painter_at(hdr).rect_filled(
                seg,
                0.0,
                Theme::hsb(self.theme.hue, self.theme.sat_max as f32 / 100.0, b),
            );
        }

        text_near(
            ui,
            Rect::from_min_max(Pos2::new(l + SWATCH + SWATCH_GAP, hdr.top()), Pos2::new(hdr.center().x, hdr.bottom())),
            &strings::analyzer_label("Saturation", self.lang),
            fonts::regular(K_HEADER_FONT_SIZE),
            theme.col_900(),
        );
        let sat_idx = K_SAT_VALS
            .iter()
            .position(|&v| v == self.theme.sat_max)
            .unwrap_or(K_NUM_SAT - 1);
        let sat_key = match sat_idx {
            0 => Key::SatNone,
            1 => Key::SatLow,
            2 => Key::SatMed,
            _ => Key::SatHigh,
        };
        text_far(
            ui,
            hdr,
            strings::tr(self.lang, sat_key),
            fonts::regular(K_HEADER_FONT_SIZE),
            theme.col_700(),
        );

        let track = Rect::from_min_max(
            Pos2::new(l, card.top() + K_SAT_Y),
            Pos2::new(r, card.top() + K_SAT_Y + K_TRACK_H),
        );
        let norm = sat_idx as f32 / (K_NUM_SAT - 1) as f32;
        draw_slider(ui, track, norm, &theme);

        let resp = ui.interact(track, Id::new("set_sat"), Sense::click_and_drag());
        if resp.dragged() || resp.clicked() {
            if let Some(pos) = ui.ctx().pointer_interact_pos() {
                let n = ((pos.x - track.left()) / track.width()).clamp(0.0, 1.0);
                let idx = (n * (K_NUM_SAT - 1) as f32).round() as usize;
                let sat = K_SAT_VALS[idx.min(K_NUM_SAT - 1)];
                if sat != self.theme.sat_max {
                    self.theme.sat_max = sat;
                    actions.push(PanelAction::SetSat(sat));
                }
            }
        }
    }

    // ----- 音频：驱动 / 输入 / 输出 -----

    fn draw_audio(&mut self, ui: &mut egui::Ui, card: Rect, l: f32, r: f32) {
        let theme = self.theme;
        let title_rect = Rect::from_min_max(
            Pos2::new(l, card.top() + K_AUDIO_TITLE_Y),
            Pos2::new(r, card.top() + K_AUDIO_TITLE_Y + K_TITLE_SIZE),
        );
        text_near(
            ui,
            title_rect,
            &strings::analyzer_label("AUDIO", self.lang),
            fonts::semibold(K_TITLE_SIZE),
            theme.col_900(),
        );

        if self.audio.show_driver {
            let row = Rect::from_min_max(
                Pos2::new(l, card.top() + K_DRIVER_ROW_Y),
                Pos2::new(r, card.top() + K_DRIVER_ROW_Y + K_AUDIO_ROW_H),
            );
            let hover = self.menu == Some(MenuKind::Driver);
            draw_device_row(
                ui,
                row,
                &strings::analyzer_label("Driver", self.lang),
                &self.audio.driver,
                hover,
                &theme,
            );
            let resp = ui.interact(row, Id::new("set_driver_row"), Sense::click());
            if resp.clicked() {
                self.menu = Some(MenuKind::Driver);
            }
        }

        for (i, (label, current, menu_kind)) in [
            (
                "Input",
                self.audio.input.clone(),
                MenuKind::AudioIn,
            ),
            (
                "Output",
                self.audio.output.clone(),
                MenuKind::AudioOut,
            ),
        ]
        .into_iter()
        .enumerate()
        {
            if !self.audio.show_devices {
                continue;
            }
            let y = card.top() + if i == 0 { K_AUDIO_ROW1_Y } else { K_AUDIO_ROW2_Y };
            let row = Rect::from_min_max(Pos2::new(l, y), Pos2::new(r, y + K_AUDIO_ROW_H));
            let hover = self.menu == Some(menu_kind);
            draw_device_row(
                ui,
                row,
                &strings::analyzer_label(label, self.lang),
                &current,
                hover,
                &theme,
            );
            let resp = ui.interact(row, Id::new(("set_audio_row", i)), Sense::click());
            if resp.clicked() {
                self.menu = Some(menu_kind);
            }
        }
    }

    // ----- Developer：测试信号 -----

    fn draw_gen(
        &mut self,
        ui: &mut egui::Ui,
        card: Rect,
        l: f32,
        r: f32,
        actions: &mut Vec<PanelAction>,
    ) {
        let theme = self.theme;
        // 基准 Y：原版 `b = (mHasAudio||mHasDriver) ? kCardHAudio-12 : kCardH-12`（:126）。
        let b = if self.has_audio() {
            K_CARD_H_AUDIO - 12.0
        } else {
            K_CARD_H - 12.0
        };

        let title_rect = Rect::from_min_max(
            Pos2::new(l, card.top() + b + K_DEV_TITLE_OFF),
            Pos2::new(r, card.top() + b + K_DEV_TITLE_OFF + K_TITLE_SIZE),
        );
        text_near(
            ui,
            title_rect,
            &strings::analyzer_label("DEVELOPER", self.lang),
            fonts::semibold(K_TITLE_SIZE),
            theme.col_900(),
        );

        // 信号类型行（点击弹菜单）。
        let signal_row = Rect::from_min_max(
            Pos2::new(l, card.top() + b + K_DEV_SIGNAL_OFF),
            Pos2::new(r, card.top() + b + K_DEV_SIGNAL_OFF + K_AUDIO_ROW_H),
        );
        let signal_name = TestSignalType::from_i32(self.gen.type_).name();
        let hover = self.menu == Some(MenuKind::GenType);
        draw_device_row(
            ui,
            signal_row,
            &strings::analyzer_label("Signal", self.lang),
            signal_name,
            hover,
            &theme,
        );
        let resp = ui.interact(signal_row, Id::new("gen_signal_row"), Sense::click());
        if resp.clicked() {
            self.menu = Some(MenuKind::GenType);
        }

        // 频率滑杆。
        let freq_hdr = Rect::from_min_max(
            Pos2::new(l, card.top() + b + K_DEV_FREQ_HDR_OFF),
            Pos2::new(r, card.top() + b + K_DEV_FREQ_HDR_OFF + K_HEADER_H),
        );
        text_near(
            ui,
            freq_hdr,
            &strings::analyzer_label("Frequency", self.lang),
            fonts::regular(K_HEADER_FONT_SIZE),
            theme.col_900(),
        );
        text_far(
            ui,
            freq_hdr,
            &self.gen_freq_label(),
            fonts::regular(K_HEADER_FONT_SIZE),
            theme.col_700(),
        );
        let freq_track = Rect::from_min_max(
            Pos2::new(l, card.top() + b + K_DEV_FREQ_OFF),
            Pos2::new(r, card.top() + b + K_DEV_FREQ_OFF + K_TRACK_H),
        );
        draw_slider(ui, freq_track, self.gen_freq_norm(), &theme);
        let resp = ui.interact(freq_track, Id::new("gen_freq"), Sense::click_and_drag());
        if resp.dragged() || resp.clicked() {
            if let Some(pos) = ui.ctx().pointer_interact_pos() {
                let n = ((pos.x - freq_track.left()) / freq_track.width()).clamp(0.0, 1.0);
                let f = gen_freq_from_norm(n);
                self.gen.freq = f;
                self.gen_save_pending = true;
                actions.push(PanelAction::GenFreq(f));
            }
        }

        // 电平滑杆。
        let level_hdr = Rect::from_min_max(
            Pos2::new(l, card.top() + b + K_DEV_LEVEL_HDR_OFF),
            Pos2::new(r, card.top() + b + K_DEV_LEVEL_HDR_OFF + K_HEADER_H),
        );
        text_near(
            ui,
            level_hdr,
            &strings::analyzer_label("Level", self.lang),
            fonts::regular(K_HEADER_FONT_SIZE),
            theme.col_900(),
        );
        text_far(
            ui,
            level_hdr,
            &self.gen_level_label(),
            fonts::regular(K_HEADER_FONT_SIZE),
            theme.col_700(),
        );
        let level_track = Rect::from_min_max(
            Pos2::new(l, card.top() + b + K_DEV_LEVEL_OFF),
            Pos2::new(r, card.top() + b + K_DEV_LEVEL_OFF + K_TRACK_H),
        );
        draw_slider(ui, level_track, self.gen_level_norm(), &theme);
        let resp = ui.interact(level_track, Id::new("gen_level"), Sense::click_and_drag());
        if resp.dragged() || resp.clicked() {
            if let Some(pos) = ui.ctx().pointer_interact_pos() {
                let n = ((pos.x - level_track.left()) / level_track.width()).clamp(0.0, 1.0);
                let db = gen_level_from_norm(n);
                self.gen.level = db;
                self.gen_save_pending = true;
                actions.push(PanelAction::GenLevel(db));
            }
        }

        // RESTART / RESEED。
        let dbw = (K_CARD_W - 2.0 * K_PAD - K_DEV_BTN_GAP) * 0.5;
        let by = card.top() + b + K_DEV_BTN_OFF;
        let restart_rect = Rect::from_min_max(Pos2::new(l, by), Pos2::new(l + dbw, by + K_BTN_H));
        let reseed_rect = Rect::from_min_max(
            Pos2::new(l + dbw + K_DEV_BTN_GAP, by),
            Pos2::new(r, by + K_BTN_H),
        );
        if button(
            ui,
            restart_rect,
            &strings::analyzer_label("RESTART", self.lang),
            false,
            &theme,
            Id::new("gen_restart"),
            false,
        ) {
            actions.push(PanelAction::GenRestart);
        }
        if button(
            ui,
            reseed_rect,
            &strings::analyzer_label("RESEED", self.lang),
            false,
            &theme,
            Id::new("gen_reseed"),
            false,
        ) {
            actions.push(PanelAction::GenReseed);
        }

        // TO OUTPUT / HOLD 开关。
        let ty = card.top() + b + K_DEV_TOGGLE_OFF;
        let out_rect = Rect::from_min_max(Pos2::new(l, ty), Pos2::new(l + dbw, ty + K_BTN_H));
        let hold_rect = Rect::from_min_max(
            Pos2::new(l + dbw + K_DEV_BTN_GAP, ty),
            Pos2::new(r, ty + K_BTN_H),
        );
        if button(
            ui,
            out_rect,
            &strings::analyzer_label("TO OUTPUT", self.lang),
            self.gen.to_output,
            &theme,
            Id::new("gen_to_output"),
            false,
        ) {
            self.gen.to_output = !self.gen.to_output;
            actions.push(PanelAction::GenToOutput(self.gen.to_output));
        }
        if button(
            ui,
            hold_rect,
            strings::analyzer_dup_label(strings::AnalyzerDup::GenHold, self.lang),
            self.gen.hold,
            &theme,
            Id::new("gen_hold"),
            false,
        ) {
            self.gen.hold = !self.gen.hold;
            actions.push(PanelAction::GenHold(self.gen.hold));
        }
    }

    // ------------------------------------------------------------------
    // 菜单
    // ------------------------------------------------------------------

    fn draw_menu(&mut self, ui: &mut egui::Ui, actions: &mut Vec<PanelAction>) {
        let Some(kind) = self.menu else { return };
        let theme = self.theme;

        let (items, current, action): (Vec<String>, Option<usize>, MenuBuilder) = match kind {
            MenuKind::GenType => {
                let items = (0..=17)
                    .map(|i| TestSignalType::from_i32(i).name().to_string())
                    .collect();
                let current = (0..=17).contains(&self.gen.type_).then_some(self.gen.type_ as usize);
                (items, current, MenuBuilder::GenType)
            }
            MenuKind::Driver => (
                self.audio.driver_options.clone(),
                self.audio
                    .driver_options
                    .iter()
                    .position(|n| n == &self.audio.driver),
                MenuBuilder::Driver,
            ),
            MenuKind::AudioIn => (
                self.audio.input_options.clone(),
                self.audio
                    .input_options
                    .iter()
                    .position(|n| n == &self.audio.input),
                MenuBuilder::AudioIn,
            ),
            MenuKind::AudioOut => (
                self.audio.output_options.clone(),
                self.audio
                    .output_options
                    .iter()
                    .position(|n| n == &self.audio.output),
                MenuBuilder::AudioOut,
            ),
        };

        let pos = ui
            .ctx()
            .pointer_latest_pos()
            .unwrap_or(Pos2::new(K_PAD, K_PAD));
        let mut keep = true;

        egui::Area::new(Id::new("settings_menu"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style())
                    .fill(theme.col_100())
                    .stroke(egui::Stroke::new(1.0_f32, theme.col_300()))
                    .show(ui, |ui| {
                        ui.set_min_width(160.0);
                        for (i, item) in items.iter().enumerate() {
                            let label = if current == Some(i) {
                                format!("✓ {item}")
                            } else {
                                item.clone()
                            };
                            if ui
                                .add(
                                    egui::Button::new(label)
                                        .min_size(Vec2::new(150.0, 26.0)),
                                )
                                .clicked()
                            {
                                match action {
                                    MenuBuilder::GenType => {
                                        self.gen.type_ = i as i32;
                                        actions.push(PanelAction::GenType(i as i32));
                                    }
                                    MenuBuilder::Driver => {
                                        let name = item.clone();
                                        actions.push(PanelAction::SetDriver(name));
                                    }
                                    MenuBuilder::AudioIn => {
                                        let name = item.clone();
                                        actions.push(PanelAction::SetAudioDevice {
                                            input: true,
                                            name,
                                        });
                                    }
                                    MenuBuilder::AudioOut => {
                                        let name = item.clone();
                                        actions.push(PanelAction::SetAudioDevice {
                                            input: false,
                                            name,
                                        });
                                    }
                                }
                                keep = false;
                            }
                        }
                    });
            });

        if keep && ui.ctx().input(|i| i.pointer.any_click()) {
            keep = false;
        }
        if !keep {
            self.menu = None;
        }
    }

    // ------------------------------------------------------------------
    // 发生器读数与归一化（SettingsPanelControl.h:487-537）
    // ------------------------------------------------------------------

    fn gen_freq_norm(&self) -> f32 {
        let f = self.gen.freq.clamp(1.0, 20_000.0);
        (f.ln() / 20_000.0_f64.ln()) as f32
    }

    fn gen_level_norm(&self) -> f32 {
        let db = self.gen.level.clamp(-120.0, 0.0);
        ((db + 120.0) / 120.0) as f32
    }

    /// 频率读数随信号类型改变语义（原版 `GenFreqLabel`:501-527）。
    fn gen_freq_label(&self) -> String {
        let t = TestSignalType::from_i32(self.gen.type_);
        match t {
            TestSignalType::SweepLog => "20 Hz - 20 kHz".to_string(),
            TestSignalType::ImpulseTrain => format!("{:.1} /s", self.gen.freq),
            TestSignalType::Off
            | TestSignalType::Silence
            | TestSignalType::WhiteNoise
            | TestSignalType::PinkNoise
            | TestSignalType::NoiseC
            | TestSignalType::PinkC => "-".to_string(),
            _ => {
                if self.gen.freq >= 1000.0 {
                    format!("{:.2} kHz", self.gen.freq / 1000.0)
                } else {
                    format!("{:.1} Hz", self.gen.freq)
                }
            }
        }
    }

    /// 电平读数（原版 `GenLevelLabel`:529-537）。
    fn gen_level_label(&self) -> String {
        let t = TestSignalType::from_i32(self.gen.type_);
        match t {
            TestSignalType::Off | TestSignalType::Silence => "-".to_string(),
            _ => format!("{:.0} dB", self.gen.level),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum MenuBuilder {
    GenType,
    Driver,
    AudioIn,
    AudioOut,
}

/// 频率归一化 → Hz（对数映射 1 Hz..20 kHz，原版 `GenFreqFromNorm`:488）。
fn gen_freq_from_norm(n: f32) -> f64 {
    20_000.0_f64.powf(n.clamp(0.0, 1.0) as f64)
}

/// 电平归一化 → dBFS（-120..0 线性，原版 `GenLevelFromNorm`:494）。
fn gen_level_from_norm(n: f32) -> f64 {
    -120.0 + 120.0 * n.clamp(0.0, 1.0) as f64
}

// ---------------------------------------------------------------------------
// 局部绘制助手（对应原版 DrawButton / DrawSlider / DrawDeviceRow）
// ---------------------------------------------------------------------------

fn button(
    ui: &mut egui::Ui,
    rect: Rect,
    label: &str,
    active: bool,
    theme: &Theme,
    id: Id,
    semibold: bool,
) -> bool {
    let p = ui.painter_at(rect);
    let resp = ui.interact(rect, id, Sense::click());
    let fill = if active {
        theme.col_900()
    } else if resp.hovered() {
        theme.col_500()
    } else {
        theme.col_300()
    };
    p.rect_filled(rect, 0.0, fill);
    let fg = if active { theme.col_100() } else { theme.col_900() };
    p.text(
        rect.center(),
        Align2::CENTER_CENTER,
        label,
        if semibold {
            fonts::semibold(K_TITLE_SIZE)
        } else {
            fonts::regular(K_TITLE_SIZE)
        },
        fg,
    );
    resp.clicked()
}

fn draw_slider(ui: &mut egui::Ui, track: Rect, norm: f32, theme: &Theme) {
    let p = ui.painter_at(track);
    let cy = track.center().y;
    let x = track.left() + norm.clamp(0.0, 1.0) * track.width();
    p.rect_filled(
        Rect::from_min_max(Pos2::new(track.left(), cy - 2.0), Pos2::new(track.right(), cy + 2.0)),
        2.0,
        theme.col_300(),
    );
    p.rect_filled(
        Rect::from_min_max(Pos2::new(track.left(), cy - 2.0), Pos2::new(x, cy + 2.0)),
        2.0,
        theme.col_500(),
    );
    // 旋钮：外圈底色 + 内圈主题墨（与 bandpass 设置面板一致；原版 HANDLE_R=7）。
    p.circle_filled(Pos2::new(x, cy), 8.5, theme.col_100());
    p.circle_filled(Pos2::new(x, cy), 7.0, theme.col_900());
}

fn draw_device_row(
    ui: &mut egui::Ui,
    rect: Rect,
    label: &str,
    value: &str,
    hover: bool,
    theme: &Theme,
) {
    let p = ui.painter_at(rect);
    if hover {
        p.rect_filled(rect, 0.0, theme.col_300());
    }
    let label_font = fonts::regular(K_TITLE_SIZE);
    p.text(
        Pos2::new(rect.left(), rect.center().y),
        Align2::LEFT_CENTER,
        label,
        label_font.clone(),
        theme.col_900(),
    );
    // 标签墨迹宽度 + 12 间隙（原版 LABEL_VALUE_GAP / FitText）。
    let label_w = p
        .layout_no_wrap(label.to_owned(), label_font, theme.col_900())
        .size()
        .x;
    let val_rect = Rect::from_min_max(
        Pos2::new(rect.left() + label_w + 12.0, rect.top()),
        Pos2::new(rect.right(), rect.bottom()),
    );
    let fitted = fit_text(&p, value, fonts::regular(K_TITLE_SIZE), theme.col_700(), val_rect.width());
    p.text(
        val_rect.right_center(),
        Align2::RIGHT_CENTER,
        &fitted,
        fonts::regular(K_TITLE_SIZE),
        theme.col_700(),
    );
}

/// 超长文本逐字符截断并补省略号（原版 `FitText`:617-635，省略号 U+2026）。
fn fit_text(
    p: &egui::Painter,
    text: &str,
    font: egui::FontId,
    color: egui::Color32,
    max_w: f32,
) -> String {
    let layout = |s: &str| p.layout_no_wrap(s.to_owned(), font.clone(), color).size().x;
    if layout(text) <= max_w {
        return text.to_string();
    }
    let mut chars: Vec<char> = text.chars().collect();
    while !chars.is_empty() {
        chars.pop();
        let mut candidate: String = chars.iter().collect();
        candidate.push('\u{2026}');
        if layout(&candidate) <= max_w {
            return candidate;
        }
    }
    "\u{2026}".to_string()
}

fn text_near(
    ui: &mut egui::Ui,
    rect: Rect,
    text: &str,
    font: egui::FontId,
    color: egui::Color32,
) {
    ui.painter_at(rect).text(
        Pos2::new(rect.left(), rect.center().y),
        Align2::LEFT_CENTER,
        text,
        font,
        color,
    );
}

fn text_far(
    ui: &mut egui::Ui,
    rect: Rect,
    text: &str,
    font: egui::FontId,
    color: egui::Color32,
) {
    ui.painter_at(rect).text(
        Pos2::new(rect.right(), rect.center().y),
        Align2::RIGHT_CENTER,
        text,
        font,
        color,
    );
}
