//! 插件外壳：BandPass 与 Fusion 共用的标题区、预设槽、quick 槽、
//! 图标行、设置面板与版式常量。
//!
//! 所有函数只负责画和收集意图，具体参数写入仍由插件层完成，
//! 与 `widgets.rs` 的分工一致。

use egui::{Align2, Color32, Id, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};
use std::path::PathBuf;

use crate::fonts;
use crate::strings::{self, Key, Lang};
use crate::theme::{Theme, ThemeMode};
use crate::widgets::{self, BarEdit, BarModel, Icon, SlotResponse};

/// 与原 C++ 版 BandPass 画布一致的右侧栏与底部几何。
pub mod layout {
    pub const COL1_X: f32 = 740.0;
    pub const COL2_X: f32 = 820.0;
    pub const PANEL_R: f32 = 896.0;
    pub const BTN_W: f32 = 76.0;

    pub const PRESET_TITLE_Y: f32 = 41.0;
    pub const GRID_Y: f32 = 56.0;
    pub const CELL_W: f32 = 36.0;
    pub const CELL_H: f32 = 29.0;
    pub const CELL_STEP_X: f32 = 40.0;
    pub const CELL_STEP_Y: f32 = 33.0;
    pub const GRID_COLS: usize = 4;
    pub const GRID_ROWS: usize = 4;

    pub const QUICK_X0: f32 = 39.5;
    pub const QUICK_X1: f32 = 648.5;
    pub const QUICK_W: f32 = 36.0;
    pub const QUICK_Y: f32 = 552.0;
    pub const QUICK_H: f32 = 26.0;

    pub const FADE_L: f32 = 31.5;
    pub const FADE_R: f32 = 656.5;
    pub const FADE_Y0: f32 = 592.0;
    pub const FADE_Y1: f32 = 616.0;

    pub const ICON_Y0: f32 = 482.0;
    pub const ICON_Y1: f32 = 510.0;
    pub const ICON_W: f32 = 36.0;
    pub const ICON_STEP: f32 = 40.0;

    pub const TITLE_Y: f32 = 545.0;
}

pub fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
    Rect::from_min_max(Pos2::new(x0, y0), Pos2::new(x1, y1))
}

/// 预设区标题（PRESETS / 预设）。两个插件都在同一坐标起笔。
pub fn preset_heading(ui: &Ui, theme: &Theme, lang: Lang) {
    ui.painter_at(rect(0.0, 0.0, crate::UI_WIDTH, crate::UI_HEIGHT))
        .text(
            Pos2::new(layout::COL1_X, layout::PRESET_TITLE_Y),
            Align2::LEFT_CENTER,
            strings::tr(lang, Key::Presets),
            fonts::bold(20.0),
            theme.col_900(),
        );
}

/// 4×4 预设槽。`labels` 按显示顺序给出 16 个槽的字面（通常来自 PresetBank）。
pub fn preset_grid(
    ui: &mut Ui,
    theme: &Theme,
    lang: Lang,
    labels: &[String],
    drag_hover: Option<usize>,
    id_salt: &'static str,
) -> Vec<SlotResponse> {
    let tip = format!(
        "{}\n{}\n{}\n{}",
        strings::tr(lang, Key::TipDrag),
        strings::tr(lang, Key::TipSaveHere),
        strings::tr(lang, Key::TipMenu),
        strings::tr(lang, Key::TipPresets)
    );
    let mut out = Vec::with_capacity(layout::GRID_COLS * layout::GRID_ROWS);
    for i in 0..layout::GRID_COLS * layout::GRID_ROWS {
        let row = i / layout::GRID_COLS;
        let col = i % layout::GRID_COLS;
        let x = layout::COL1_X + col as f32 * layout::CELL_STEP_X;
        let y = layout::GRID_Y + row as f32 * layout::CELL_STEP_Y;
        let r = rect(x, y, x + layout::CELL_W, y + layout::CELL_H);
        let label = labels.get(i).cloned().unwrap_or_else(|| format!("{}", i + 9));
        let resp = widgets::preset_slot(
            ui,
            r,
            &label,
            drag_hover == Some(i),
            theme,
            Id::new((id_salt, "slot", i)),
            Some(tip.as_str()),
        );
        if resp.hovered {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        out.push(resp);
    }
    out
}

/// 底部 8 个 quick 槽。与 fade 轨道共用同一组中心点。
pub fn quick_row(ui: &mut Ui, theme: &Theme, lang: Lang, labels: &[String], id_salt: &'static str) -> Vec<SlotResponse> {
    let mut out = Vec::with_capacity(8);
    for i in 0..8 {
        let cx = layout::QUICK_X0
            + (layout::QUICK_X1 - layout::QUICK_X0) * i as f32 / 7.0;
        let r = rect(
            cx - layout::QUICK_W * 0.5,
            layout::QUICK_Y,
            cx + layout::QUICK_W * 0.5,
            layout::QUICK_Y + layout::QUICK_H,
        );
        let label = labels.get(i).cloned().unwrap_or_else(|| format!("{}", i + 1));
        let resp = widgets::preset_slot(
            ui,
            r,
            &label,
            false,
            theme,
            Id::new((id_salt, "quick", i)),
            Some(strings::tr(lang, Key::TipPresets)),
        );
        out.push(resp);
    }
    out
}

/// 撤销 / 重做 / 保存 / 读取 四个图标。
pub fn icon_row(ui: &mut Ui, theme: &Theme, lang: Lang, id_salt: &'static str) -> Option<Icon> {
    let mut clicked = None;
    for (i, icon) in [Icon::Undo, Icon::Redo, Icon::Save, Icon::Load]
        .iter()
        .enumerate()
    {
        let x = layout::COL1_X + i as f32 * layout::ICON_STEP;
        if widgets::icon_button(
            ui,
            rect(x, layout::ICON_Y0, x + layout::ICON_W, layout::ICON_Y1),
            *icon,
            theme,
            Id::new((id_salt, "icon", i)),
            Some(strings::tr(
                lang,
                match icon {
                    Icon::Undo => Key::Undo,
                    Icon::Redo => Key::Redo,
                    Icon::Save => Key::Save,
                    Icon::Load => Key::Load,
                },
            )),
        ) {
            clicked = Some(*icon);
        }
    }
    clicked
}

/// 标题区：ORM（32 Bold）+ 齿轮 + 版本 + 插件名（32 Bold）。
/// 返回齿轮是否被点击。
pub fn title_block(
    ui: &mut Ui,
    theme: &Theme,
    name: &str,
    version: &str,
    id_salt: &'static str,
) -> bool {
    let full = rect(0.0, 0.0, crate::UI_WIDTH, crate::UI_HEIGHT);
    let pnt = ui.painter_at(full);
    let orm_font = fonts::bold(32.0);
    let orm_w = pnt
        .layout_no_wrap("ORM".to_owned(), orm_font.clone(), theme.col_900())
        .size()
        .x;
    pnt.text(
        Pos2::new(layout::COL1_X, layout::TITLE_Y),
        Align2::LEFT_TOP,
        "ORM",
        orm_font,
        theme.col_900(),
    );
    let gear_l = layout::COL1_X + orm_w + 7.0;
    // 齿轮与 ORM 的视觉字高对齐：标题 32px 的首行实际帽高约 17px，
    // 因此以标题顶边 + 3px 为齿轮顶边，避免此前下沉 8px 的错位。
    let gear_size = 17.0;
    let gear = rect(
        gear_l,
        layout::TITLE_Y + 3.0,
        gear_l + gear_size,
        layout::TITLE_Y + 3.0 + gear_size,
    );
    let gear_clicked = widgets::gear_button(ui, gear, theme, Id::new((id_salt, "gear")), None);
    pnt.text(
        Pos2::new(gear.right() + 8.0, layout::TITLE_Y + 30.0),
        Align2::LEFT_BOTTOM,
        &format!("v{version}"),
        fonts::regular(20.0),
        theme.col_500(),
    );
    pnt.text(
        Pos2::new(layout::COL1_X, layout::TITLE_Y + 31.0),
        Align2::LEFT_TOP,
        name,
        fonts::bold(32.0),
        theme.col_900(),
    );
    gear_clicked
}

/// 设置面板额外内容（Fusion 的 FFT 尺寸档位）。
pub struct SettingsExtras<'a> {
    pub title: &'a str,
    pub buttons: &'a [(&'a str, bool)],
    pub columns: usize,
    pub hint: Option<&'a str>,
}

#[derive(Default)]
pub struct SettingsResult {
    pub changed: bool,
    pub closed: bool,
    pub extra: Option<usize>,
}

/// 语言 / 主题 / 色相 / 饱和度四段设置卡片。BandPass 与 Fusion 共用。
/// `extras` 存在时卡片加高，在原四段下方追加一组档位按钮。
pub fn settings_card(
    ui: &mut Ui,
    theme: &mut Theme,
    lang: &mut Lang,
    extras: Option<SettingsExtras<'_>>,
    id_salt: &'static str,
) -> SettingsResult {
    let mut out = SettingsResult::default();
    let full = rect(0.0, 0.0, crate::UI_WIDTH, crate::UI_HEIGHT);
    ui.painter_at(full)
        .rect_filled(full, 0, Color32::from_rgba_unmultiplied(26, 25, 22, 70));
    let card_h = if extras.is_some() { 420.0 } else { 270.0 };
    let card = Rect::from_center_size(full.center(), Vec2::new(380.0, card_h));
    // 点击卡片外的遮罩关闭。必须在按钮之前注册：egui 的点击命中按
    // 注册顺序取最上层，若放在最后，这个全屏区域会把语言/主题按钮全部吞掉。
    let outside = ui.interact(
        full,
        Id::new((id_salt, "settings_bg")),
        Sense::click(),
    );
    if outside.clicked() {
        if let Some(pos) = ui.ctx().pointer_interact_pos() {
            if !card.contains(pos) {
                out.closed = true;
            }
        }
    }

    let pnt = ui.painter_at(card);
    pnt.rect_filled(card, 0, theme.col_100());
    pnt.rect_stroke(
        card,
        0,
        Stroke::new(1.0_f32, theme.col_300()),
        StrokeKind::Inside,
    );

    let lang_now = *lang;
    let tr = move |k: Key| strings::tr(lang_now, k);
    let l = card.left() + 20.0;
    let r = card.right() - 20.0;
    let bw = (r - l) / 2.0;

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
        *lang == Lang::Zh,
        theme,
        Id::new((id_salt, "set_zh")),
        None,
    ) {
        *lang = Lang::Zh;
        out.changed = true;
    }
    if widgets::flat_button(
        ui,
        rect(l + bw, card.top() + 38.0, r, card.top() + 68.0),
        "ENGLISH",
        *lang == Lang::En,
        theme,
        Id::new((id_salt, "set_en")),
        None,
    ) {
        *lang = Lang::En;
        out.changed = true;
    }

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
        theme.mode == ThemeMode::Dark,
        theme,
        Id::new((id_salt, "set_dark")),
        None,
    ) {
        theme.mode = ThemeMode::Dark;
        out.changed = true;
    }
    if widgets::flat_button(
        ui,
        rect(l + bw, card.top() + 104.0, r, card.top() + 134.0),
        tr(Key::Light),
        theme.mode == ThemeMode::Light,
        theme,
        Id::new((id_salt, "set_light")),
        None,
    ) {
        theme.mode = ThemeMode::Light;
        out.changed = true;
    }

    // 色相
    let hue_hdr_y = card.top() + 161.0;
    let sw = rect(l, hue_hdr_y - 7.0, l + 14.0, hue_hdr_y + 7.0);
    pnt.rect_filled(sw, 0, Theme::hsb(theme.hue, 0.85, 1.0));
    pnt.text(
        Pos2::new(l + 22.0, hue_hdr_y),
        Align2::LEFT_CENTER,
        tr(Key::Hue),
        fonts::regular(20.0),
        theme.col_900(),
    );
    pnt.text(
        Pos2::new(r, hue_hdr_y),
        Align2::RIGHT_CENTER,
        &format!("{}\u{00b0}", theme.hue),
        fonts::regular(20.0),
        theme.col_700(),
    );
    let htrack = rect(l, card.top() + 185.0, r, card.top() + 189.0);
    pnt.rect_filled(htrack, 2, theme.col_300());
    let hn = ((theme.hue.saturating_sub(15)) as f32 / 345.0).clamp(0.0, 1.0);
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
        Id::new((id_salt, "set_hue")),
        Sense::click_and_drag(),
    );
    if hresp.dragged() || hresp.clicked() {
        if let Some(pos) = ui.ctx().pointer_interact_pos() {
            let n = ((pos.x - htrack.left()) / htrack.width()).clamp(0.0, 1.0);
            let h = 15 + (n * 345.0 / 15.0).round() as u16 * 15;
            if h.min(360) != theme.hue {
                theme.hue = h.min(360);
                out.changed = true;
            }
        }
    }

    // 饱和度
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
            Theme::hsb(theme.hue, theme.sat_max as f32 / 100.0, b),
        );
    }
    pnt.text(
        Pos2::new(l + 22.0, sat_hdr_y),
        Align2::LEFT_CENTER,
        tr(Key::Saturation),
        fonts::regular(20.0),
        theme.col_900(),
    );
    let sat_key = match theme.sat_max {
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
    const SAT_VALS: [u8; 4] = [0, 15, 30, 50];
    let sat_idx = SAT_VALS
        .iter()
        .position(|&v| v == theme.sat_max)
        .unwrap_or(3) as f32;
    let sx = st.left() + sat_idx / 3.0 * st.width();
    pnt.rect_filled(
        Rect::from_min_max(st.min, Pos2::new(sx, st.max.y)),
        2,
        theme.col_500(),
    );
    pnt.circle_filled(Pos2::new(sx, st.center().y), 8.5, theme.col_100());
    pnt.circle_filled(Pos2::new(sx, st.center().y), 7.0, theme.col_900());
    let sresp = ui.interact(
        rect(l, card.top() + 232.0, r, card.top() + 258.0),
        Id::new((id_salt, "set_sat_slider")),
        Sense::click_and_drag(),
    );
    if sresp.dragged() || sresp.clicked() {
        if let Some(pos) = ui.ctx().pointer_interact_pos() {
            let n = ((pos.x - st.left()) / st.width()).clamp(0.0, 1.0);
            let sat = SAT_VALS[(n * 3.0).round() as usize];
            if sat != theme.sat_max {
                theme.sat_max = sat;
                out.changed = true;
            }
        }
    }

    if let Some(ex) = extras {
        pnt.text(
            Pos2::new(l, card.top() + 278.0),
            Align2::LEFT_CENTER,
            ex.title,
            fonts::semibold(20.0),
            theme.col_900(),
        );
        let cell_w = 48.0;
        let cell_h = 24.0;
        for (i, (label, active)) in ex.buttons.iter().enumerate() {
            let col = i % ex.columns.max(1);
            let row = i / ex.columns.max(1);
            let x = l + col as f32 * cell_w;
            let y = card.top() + 292.0 + row as f32 * 28.0;
            if widgets::flat_button(
                ui,
                rect(x, y, x + 44.0, y + cell_h),
                label,
                *active,
                theme,
                Id::new((id_salt, "extra", i)),
                None,
            ) {
                out.extra = Some(i);
                out.changed = true;
            }
        }
        if let Some(hint) = ex.hint {
            pnt.text(
                Pos2::new(l, card.top() + 362.0),
                Align2::LEFT_CENTER,
                hint,
                fonts::regular(14.0),
                theme.col_700(),
            );
        }
    }

    out
}

/// 设置文件读写（JSON：lang / hue / satMax / themeMode）。
pub fn settings_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(home).join("Library/Application Support/OpenRM")
}

pub fn load_settings(file: &str) -> (Lang, Theme) {
    let mut lang = strings::detect_lang();
    let mut theme = Theme::default();
    let path = settings_dir().join(file);
    if let Ok(txt) = std::fs::read_to_string(path) {
        if let Ok(j) = serde_json::from_str::<serde_json::Value>(&txt) {
            if let Some(v) = j.get("lang").and_then(|v| v.as_i64()) {
                lang = if v == 1 { Lang::Zh } else { Lang::En };
            }
            if let Some(v) = j.get("hue").and_then(|v| v.as_i64()) {
                theme.hue = v as u16;
            }
            if let Some(v) = j.get("satMax").and_then(|v| v.as_i64()) {
                theme.sat_max = v as u8;
            }
            if let Some(v) = j.get("themeMode").and_then(|v| v.as_i64()) {
                theme.mode = if v == 1 {
                    ThemeMode::Dark
                } else {
                    ThemeMode::Light
                };
            }
        }
    }
    (lang, theme)
}

pub fn save_settings(file: &str, lang: Lang, theme: &Theme) {
    let path = settings_dir().join(file);
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let j = serde_json::json!({
        "lang": if lang == Lang::Zh { 1 } else { 0 },
        "hue": theme.hue,
        "satMax": theme.sat_max,
        "themeMode": if theme.mode == ThemeMode::Dark { 1 } else { 0 },
    });
    if let Ok(txt) = serde_json::to_string_pretty(&j) {
        let _ = std::fs::write(path, txt);
    }
}

/// 右侧栏里的紧凑参数条：标题 + 数值 + 4 px 轨道。
/// 比 `hbar` 矮一半，用于 8 个频段参数的 4×2 网格。
pub fn value_row(
    ui: &mut Ui,
    area: Rect,
    label: &str,
    value_text: &str,
    norm: f64,
    theme: &Theme,
    id: Id,
    tip: Option<&str>,
) -> Option<f64> {
    let p = ui.painter().clone();
    p.rect_filled(area, 0, theme.col_100());
    p.text(
        Pos2::new(area.left(), area.top() + 10.0),
        Align2::LEFT_CENTER,
        label,
        fonts::regular(20.0),
        theme.col_900(),
    );
    p.text(
        Pos2::new(area.right(), area.top() + 10.0),
        Align2::RIGHT_CENTER,
        value_text,
        fonts::regular(20.0),
        theme.col_700(),
    );
    let track = rect(
        area.left(),
        area.top() + 23.0,
        area.right(),
        area.top() + 27.0,
    );
    p.rect_filled(track, 2, theme.col_300());
    let x = track.left() + norm.clamp(0.0, 1.0) as f32 * track.width();
    p.rect_filled(Rect::from_min_max(track.min, Pos2::new(x, track.max.y)), 2, theme.col_500());
    p.circle_filled(Pos2::new(x, track.center().y), 7.0, theme.col_100());
    p.circle_filled(Pos2::new(x, track.center().y), 5.5, theme.col_900());

    let resp = ui.interact(area, id, Sense::click_and_drag());
    let resp = if let Some(t) = tip { resp.on_hover_text(t) } else { resp };
    if resp.hovered() || resp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if resp.clicked() || resp.dragged() {
        if let Some(pos) = ui.ctx().pointer_interact_pos() {
            return Some(((pos.x - track.left()) / track.width()).clamp(0.0, 1.0) as f64);
        }
    }
    None
}

/// 单条 Agitation 参数（比 value_row 更紧凑，卡片内用）。
pub fn compact_bar(
    ui: &mut Ui,
    area: Rect,
    label: &str,
    value: f32,
    theme: &Theme,
    id: Id,
    tip: Option<&str>,
) -> Option<f32> {
    let p = ui.painter().clone();
    p.text(
        Pos2::new(area.left(), area.center().y),
        Align2::LEFT_CENTER,
        label,
        fonts::regular(17.0),
        theme.col_700(),
    );
    let track = rect(
        area.left() + 48.0,
        area.center().y - 2.0,
        area.right(),
        area.center().y + 2.0,
    );
    p.rect_filled(track, 2, theme.col_300());
    let x = track.left() + value.clamp(0.0, 1.0) * track.width();
    p.rect_filled(Rect::from_min_max(track.min, Pos2::new(x, track.max.y)), 2, theme.col_500());
    p.circle_filled(Pos2::new(x, track.center().y), 6.5, theme.col_100());
    p.circle_filled(Pos2::new(x, track.center().y), 5.0, theme.col_900());
    let resp = ui.interact(area, id, Sense::click_and_drag());
    let resp = if let Some(t) = tip { resp.on_hover_text(t) } else { resp };
    if resp.clicked() || resp.dragged() {
        if let Some(pos) = ui.ctx().pointer_interact_pos() {
            return Some(((pos.x - track.left()) / track.width()).clamp(0.0, 1.0) as f32);
        }
    }
    None
}

/// 供 Fusion 复用的 morph 滑杆（与 BandPass 的“渐变”完全同一外观）。
pub fn morph_bar(
    ui: &mut Ui,
    theme: &Theme,
    lang: Lang,
    fade_sec: f64,
    id_salt: &'static str,
) -> BarEdit {
    let norm = ((fade_sec.max(0.01) / 0.01).ln() / 6000f64.ln()).clamp(0.0, 1.0);
    let text = if fade_sec < 1.0 {
        format!("{fade_sec:.2}s")
    } else {
        format!("{fade_sec:.1}s")
    };
    widgets::hbar(
        ui,
        rect(layout::COL1_X, 188.0, layout::PANEL_R, 230.0),
        &BarModel {
            label: strings::tr(lang, Key::Morph),
            value_text: &text,
            norm,
            swatch_left: None,
            random: None,
            label_regular: false,
            tip: Some(strings::tr(lang, Key::TipMorph)),
        },
        theme,
        Id::new((id_salt, "morph")),
    )
}

/// 把 morph 滑杆的 0..1 归一化值换回秒（0.01…60）。
pub fn morph_seconds(norm: f64) -> f64 {
    0.01 * 6000f64.powf(norm)
}

/// 颜色小方块（BandPass 的随机色、Fusion 的频段色）。
pub fn color_swatch(p: &egui::Painter, rect: Rect, color: Color32) {
    p.rect_filled(rect, 0, color);
}

// ===== 共用：顶部参数读数 / 随机色块 / 取色菜单 =====

#[derive(Default)]
pub struct ValueEdit {
    pub value: Option<f64>,
    pub began: bool,
    pub clicked: bool,
}

/// 顶部一行「粗标签 + 数值」；水平拖动即可改值。
/// Fusion 的中心 / 延迟 / 长度 / 角度 / 幅度共用这个组件。
pub fn value_field(
    ui: &mut Ui,
    area: Rect,
    label: &str,
    value_text: &str,
    norm: f64,
    theme: &Theme,
    id: Id,
    tip: Option<&str>,
) -> ValueEdit {
    let mut edit = ValueEdit::default();
    let p = ui.painter_at(area);
    p.rect_filled(area, 0, theme.col_100());
    // 上下两行：上标签、下数值。顶部参数区只有 32px 高，单行并排会让
    // “中心 1.10kHz 延迟 2.90s”互相贴在一起。
    p.text(
        Pos2::new(area.left(), area.top() + 3.0),
        Align2::LEFT_TOP,
        label,
        fonts::semibold(13.0),
        theme.col_900(),
    );
    p.text(
        Pos2::new(area.left(), area.top() + 16.0),
        Align2::LEFT_TOP,
        value_text,
        fonts::regular(15.0),
        theme.col_700(),
    );
    // 细竖线分隔相邻读数，避免五个参数连成一条长线。
    p.line_segment(
        [
            Pos2::new(area.right() - 3.0, area.top() + 4.0),
            Pos2::new(area.right() - 3.0, area.bottom() - 4.0),
        ],
        egui::Stroke::new(1.0_f32, theme.col_300()),
    );
    let track = rect(area.left(), area.bottom() - 4.0, area.right(), area.bottom() - 1.0);
    p.rect_filled(track, 0, theme.col_300());
    let x = track.left() + norm.clamp(0.0, 1.0) as f32 * track.width();
    p.rect_filled(
        Rect::from_min_max(track.min, Pos2::new(x, track.max.y)),
        0,
        theme.col_500(),
    );
    p.circle_filled(Pos2::new(x, track.center().y), 3.5, theme.col_900());

    let resp = ui.interact(area, id, Sense::click_and_drag());
    let resp = if let Some(t) = tip { resp.on_hover_text(t) } else { resp };
    if resp.hovered() || resp.dragged() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
    }
    if resp.drag_started() {
        edit.began = true;
    }
    if resp.clicked() || resp.dragged() {
        edit.clicked = resp.clicked();
        if let Some(pos) = ui.ctx().pointer_interact_pos() {
            edit.value = Some(((pos.x - track.left()) / track.width()).clamp(0.0, 1.0) as f64);
        }
    }
    edit
}

#[derive(Default)]
pub struct SwatchEdit {
    pub clicked: bool,
    pub secondary: bool,
}

/// 14×14 随机色块。左键/右键都返回给调用者决定：
/// 参数行用它开关随机，随机标题行用它开取色菜单。
pub fn random_swatch(
    ui: &mut Ui,
    area: Rect,
    color_idx: usize,
    on: bool,
    theme: &Theme,
    id: Id,
    tip: Option<&str>,
) -> SwatchEdit {
    let mut out = SwatchEdit::default();
    let p = ui.painter_at(area);
    let c = Theme::random_color(color_idx);
    p.rect_filled(area, 0, if on { c } else { Theme::dim_color(c) });
    p.rect_stroke(
        area,
        0,
        egui::Stroke::new(1.0_f32, theme.col_300()),
        egui::StrokeKind::Inside,
    );
    // 视觉仍是 14×14，但命中区向外扩 3px，避免色块太小难以点中。
    let resp = ui.interact(area.expand(3.0), id, Sense::click());
    if resp.hovered() {
        p.rect_filled(area, 0, theme.hover_overlay());
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let resp = if let Some(t) = tip { resp.on_hover_text(t) } else { resp };
    out.clicked = resp.clicked();
    out.secondary = resp.secondary_clicked();
    out
}

/// RANDOM 标题 + 右侧色块（两插件同一坐标）。
pub fn random_header(
    ui: &mut Ui,
    theme: &Theme,
    lang: Lang,
    color_idx: usize,
    on: bool,
    id_salt: &'static str,
    tip: Option<&str>,
) -> SwatchEdit {
    let full = rect(0.0, 0.0, crate::UI_WIDTH, crate::UI_HEIGHT);
    ui.painter_at(full).text(
        Pos2::new(layout::COL1_X, 247.0),
        Align2::LEFT_CENTER,
        strings::tr(lang, Key::Random),
        fonts::bold(20.0),
        theme.col_900(),
    );
    random_swatch(
        ui,
        rect(881.0, 240.0, 895.0, 254.0),
        color_idx,
        on,
        theme,
        Id::new((id_salt, "random_swatch")),
        tip,
    )
}

/// 四个随机色的取色菜单；返回被选中的索引。
pub fn color_menu(
    ui: &mut Ui,
    pos: Pos2,
    current: usize,
    id_salt: &'static str,
) -> Option<usize> {
    let mut chosen = None;
    egui::Area::new(Id::new((id_salt, "color_menu")))
        .order(egui::Order::Foreground)
        .fixed_pos(pos)
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_min_width(150.0);
                for c in 0..4 {
                    let color = Theme::random_color(c);
                    let label = if c == current { "✓" } else { "" };
                    if ui
                        .add(
                            egui::Button::new(label)
                                .fill(color)
                                .min_size(Vec2::new(140.0, 26.0)),
                        )
                        .clicked()
                    {
                        chosen = Some(c);
                    }
                }
            });
        });
    chosen
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{Theme, ThemeMode};

    fn run_card(
        ctx: &egui::Context,
        theme: &mut Theme,
        lang: &mut Lang,
        events: Vec<egui::Event>,
        time: f64,
    ) {
        let raw = egui::RawInput {
            screen_rect: Some(rect(0.0, 0.0, crate::UI_WIDTH, crate::UI_HEIGHT)),
            events,
            time: Some(time),
            ..Default::default()
        };
        let _ = ctx.run(raw, |ctx| {
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(ctx, |ui| {
                    let _ = settings_card(ui, theme, lang, None, "test");
                });
        });
    }

    fn click_at(ctx: &egui::Context, theme: &mut Theme, lang: &mut Lang, pos: Pos2) {
        for (n, pressed) in [(0, true), (1, false)] {
            run_card(
                ctx,
                theme,
                lang,
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::default(),
                    },
                ],
                n as f64 * 0.1,
            );
        }
    }

    #[test]
    fn settings_buttons_win_over_fullscreen_scrim() {
        let ctx = egui::Context::default();
        let mut theme = Theme::default();
        let mut lang = Lang::Zh;

        // ENGLISH 按钮位于 468..648 / 226..256
        run_card(&ctx, &mut theme, &mut lang, Vec::new(), 0.0);
        run_card(&ctx, &mut theme, &mut lang, Vec::new(), 0.05);
        click_at(&ctx, &mut theme, &mut lang, Pos2::new(560.0, 240.0));
        assert_eq!(lang, Lang::En, "语言按钮应压过全屏遮罩");
        click_at(&ctx, &mut theme, &mut lang, Pos2::new(380.0, 306.0));
        assert_eq!(theme.mode, ThemeMode::Dark, "主题按钮应压过全屏遮罩");
    }
}
