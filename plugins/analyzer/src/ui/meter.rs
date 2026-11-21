//! 电平表 / 响度计共用件。
//!
//! 逐项移植自原版 `controls/AnalyzerMeterUi.h`（尺寸常量）、`Analyzer/src/Theme.h`
//! （OKLCH→sRGB、`MeterChromaScale`、`GridGray`、`MeterOverLed`）以及
//! `controls/SpectrumPad.h` 里 VU / LUFS / 主电平条的 OKLCH 色标 stops。
//!
//! 数值、顺序、明度/色度/色相全部照抄原版，不做近似。

use egui::{Color32, Rect};
use openrm_ui::theme::{Theme, ThemeMode};

// ===== AnalyzerMeterUi.h:16-22 尺寸常量 =====
pub const K_GAIN_BAR_W: f32 = 14.0;
pub const K_VU_SCALE_W: f32 = 28.0;
pub const K_VU_BAR_W: f32 = 14.0;
pub const K_LUFS_SCALE_W: f32 = 28.0;
pub const K_LOUD_BAR_W: f32 = 14.0;
pub const K_LRA_ZONE_W: f32 = 32.0;
/// `2*kGainBarW + kVuScaleW + 2*kVuBarW + kLufsScaleW + 4*kLoudBarW + kLraZoneW`
pub const K_METER_STRIP_W: f32 = 2.0 * K_GAIN_BAR_W
    + K_VU_SCALE_W
    + 2.0 * K_VU_BAR_W
    + K_LUFS_SCALE_W
    + 4.0 * K_LOUD_BAR_W
    + K_LRA_ZONE_W;

// ===== SpectrumPad.h:1485-1487 VU 量程 =====
pub const K_VU_TOP_DB: f32 = 3.0;
pub const K_VU_BOTTOM_VU: f32 = -20.0;

// 原版固定语义色（Theme.h:52-61），不随主题旋转。
pub const METER_YELLOW: Color32 = Color32::from_rgb(232, 173, 40);
pub const METER_RED: Color32 = Color32::from_rgb(226, 60, 52);
pub const METER_GREEN: Color32 = Color32::from_rgb(96, 186, 96);

// ===== Analyzer Theme.h:40-48 MeterChromaScale =====
// sat 0/15/30/50 → 0/0.55/0.80/1，分段线性。
const CHROMA_X: [u8; 4] = [0, 15, 30, 50];
const CHROMA_Y: [f32; 4] = [0.0, 0.55, 0.80, 1.0];

pub fn meter_chroma_scale(theme: &Theme) -> f32 {
    let sat = theme.sat_max.clamp(0, 50);
    for i in 0..3 {
        if sat <= CHROMA_X[i + 1] {
            let span = (CHROMA_X[i + 1] - CHROMA_X[i]) as f32;
            let f = (sat - CHROMA_X[i]) as f32 / span;
            return CHROMA_Y[i] + (CHROMA_Y[i + 1] - CHROMA_Y[i]) * f;
        }
    }
    1.0
}

// ===== Analyzer Theme.h:10-13 OklchGamma =====
fn oklch_gamma(v: f32) -> f32 {
    let v = v.clamp(0.0, 1.0);
    if v <= 0.0031308 {
        12.92 * v
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

// ===== Analyzer Theme.h:17-36 OklchToIColor =====
/// OKLCH → sRGB。超色域时按 10% 步进收缩 chroma 回落（不截断通道），色相/明度保持。
pub fn oklch_to_color(l: f32, c: f32, h: f32) -> Color32 {
    let hr = h.to_radians();
    let cos_h = hr.cos();
    let sin_h = hr.sin();
    for i in 0..=10 {
        let cc = c * (1.0 - 0.1 * i as f32);
        let oa = cc * cos_h;
        let ob = cc * sin_h;
        let l_ = l + 0.396_337_777_4 * oa + 0.215_803_757_3 * ob;
        let m_ = l - 0.105_561_345_8 * oa - 0.063_854_172_8 * ob;
        let s_ = l - 0.089_484_177_5 * oa - 1.291_485_548_0 * ob;
        let l = l_ * l_ * l_;
        let m = m_ * m_ * m_;
        let s = s_ * s_ * s_;
        let r = 4.076_741_662_1 * l - 3.307_711_591_3 * m + 0.230_969_929_2 * s;
        let g = -1.268_438_004_6 * l + 2.609_757_401_1 * m - 0.341_319_396_5 * s;
        let b = -0.004_196_086_3 * l - 0.703_418_614_7 * m + 1.707_614_701_0 * s;
        if (-0.001..=1.001).contains(&r)
            && (-0.001..=1.001).contains(&g)
            && (-0.001..=1.001).contains(&b)
        {
            return Color32::from_rgb(
                (oklch_gamma(r) * 255.0).round() as u8,
                (oklch_gamma(g) * 255.0).round() as u8,
                (oklch_gamma(b) * 255.0).round() as u8,
            );
        }
    }
    let gray = (oklch_gamma(l) * 255.0).round() as u8;
    Color32::from_rgb(gray, gray, gray)
}

// ===== Analyzer Theme.h:62-69 MeterOverLed =====
pub fn meter_over_led(theme: &Theme) -> Color32 {
    if theme.sat_max == 0 {
        return match theme.mode {
            ThemeMode::Dark => Color32::from_rgb(250, 250, 250),
            ThemeMode::Light => Color32::from_rgb(20, 20, 20),
        };
    }
    let sat_scale = if theme.sat_max <= 30 {
        theme.sat_max as f32 / 30.0
    } else {
        1.0 + (theme.sat_max - 30) as f32 / 55.0
    };
    let s = (0.85 * sat_scale).clamp(0.0, 1.0);
    Theme::hsb(0, s, 0.95)
}

// ===== Analyzer Theme.h:74-87 GridGray =====
/// 频谱背景格子灰。深色：`30+(255-v)*0.40` 且 sat×0.5；浅色：`(255-v)*0.18` 提亮。
pub fn grid_gray(theme: &Theme, v: u8) -> Color32 {
    let mut vv = v as f32;
    let mut s_scale = 1.0;
    match theme.mode {
        ThemeMode::Dark => {
            vv = 30.0 + (255.0 - v as f32) * 0.40;
            s_scale = 0.5;
        }
        ThemeMode::Light => {
            vv += (255.0 - vv) * 0.18;
        }
    }
    let b = vv / 255.0;
    let b_idx = (b * 100.0).round() as u8;
    let s = theme.sat_for_b(b_idx) * s_scale / 100.0;
    Theme::hsb(theme.hue, s, b)
}

// ===== 色标 stops（SpectrumPad.h） =====
/// 一个 OKLCH 色标节点：`t` 是条体纵向分数（0=顶，1=底）。
#[derive(Clone, Copy, Debug)]
pub struct Stop {
    pub t: f32,
    pub l: f32,
    pub c: f32,
    pub h: f32,
}

/// 主电平条 stops。`tOf(db) = clamp((kTopDb-db)/(kTopDb-bottomDb), 0, 1)`，
/// 随当前量程在绘制时解算（原版 DrawMeterBar:1017-1034）。
pub fn meter_stops(dark: bool, top_db: f32, bottom_db: f32) -> [Stop; 7] {
    let t_of = |db: f32| ((top_db - db) / (top_db - bottom_db)).clamp(0.0, 1.0);
    if dark {
        [
            Stop { t: 0.0, l: 0.680, c: 0.205, h: 25.0 },
            Stop { t: t_of(0.0), l: 0.715, c: 0.200, h: 40.0 },
            Stop { t: t_of(-6.0), l: 0.790, c: 0.170, h: 70.0 },
            Stop { t: t_of(-14.0), l: 0.830, c: 0.148, h: 88.0 },
            Stop { t: t_of(-24.0), l: 0.740, c: 0.140, h: 130.0 },
            Stop { t: t_of(-48.0), l: 0.620, c: 0.115, h: 155.0 },
            Stop { t: 1.0, l: 0.510, c: 0.085, h: 165.0 },
        ]
    } else {
        [
            Stop { t: 0.0, l: 0.560, c: 0.200, h: 25.0 },
            Stop { t: t_of(0.0), l: 0.600, c: 0.200, h: 40.0 },
            Stop { t: t_of(-6.0), l: 0.700, c: 0.170, h: 70.0 },
            Stop { t: t_of(-14.0), l: 0.725, c: 0.145, h: 88.0 },
            Stop { t: t_of(-24.0), l: 0.640, c: 0.140, h: 133.0 },
            Stop { t: t_of(-48.0), l: 0.500, c: 0.115, h: 158.0 },
            Stop { t: 1.0, l: 0.410, c: 0.085, h: 168.0 },
        ]
    }
}

/// VU 条 stops。`seam = tOfV(-3)`（绿→黄→红过渡缝，原版 DrawVuBar:733-747）。
pub fn vu_stops(dark: bool, seam: f32) -> [Stop; 5] {
    if dark {
        [
            Stop { t: 0.0, l: 0.720, c: 0.200, h: 25.0 },
            Stop { t: seam * 0.33, l: 0.700, c: 0.180, h: 63.0 },
            Stop { t: seam * 0.67, l: 0.678, c: 0.158, h: 100.0 },
            Stop { t: seam, l: 0.660, c: 0.145, h: 138.0 },
            Stop { t: 1.0, l: 0.510, c: 0.085, h: 165.0 },
        ]
    } else {
        [
            Stop { t: 0.0, l: 0.620, c: 0.205, h: 27.0 },
            Stop { t: seam * 0.33, l: 0.600, c: 0.185, h: 65.0 },
            Stop { t: seam * 0.67, l: 0.578, c: 0.158, h: 102.0 },
            Stop { t: seam, l: 0.560, c: 0.135, h: 140.0 },
            Stop { t: 1.0, l: 0.420, c: 0.095, h: 165.0 },
        ]
    }
}

/// LUFS 条 stops（按窗口分数布点，目标线恒在 2/3；原版 DrawLoudBars:829-838）。
pub fn loud_stops(dark: bool) -> [Stop; 6] {
    if dark {
        [
            Stop { t: 0.00, l: 0.530, c: 0.095, h: 158.0 },
            Stop { t: 0.37, l: 0.585, c: 0.120, h: 142.0 },
            Stop { t: 0.63, l: 0.630, c: 0.135, h: 133.0 },
            Stop { t: 0.70, l: 0.700, c: 0.150, h: 112.0 },
            Stop { t: 0.78, l: 0.760, c: 0.158, h: 98.0 },
            Stop { t: 1.00, l: 0.840, c: 0.165, h: 88.0 },
        ]
    } else {
        [
            Stop { t: 0.00, l: 0.440, c: 0.100, h: 158.0 },
            Stop { t: 0.37, l: 0.520, c: 0.128, h: 142.0 },
            Stop { t: 0.63, l: 0.560, c: 0.140, h: 133.0 },
            Stop { t: 0.70, l: 0.610, c: 0.152, h: 112.0 },
            Stop { t: 0.78, l: 0.645, c: 0.160, h: 98.0 },
            Stop { t: 1.00, l: 0.670, c: 0.165, h: 88.0 },
        ]
    }
}

fn stop_color(st: &Stop, c_scale: f32, alpha: u8) -> Color32 {
    let c = oklch_to_color(st.l, st.c * c_scale, st.h);
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), alpha)
}

/// 在 stops 上按 `t`（0=顶…1=底）做 sRGB 分段插值（原版 colorAtT）。
pub fn stops_color(stops: &[Stop], t: f32, c_scale: f32, alpha: u8) -> Color32 {
    for i in 0..stops.len().saturating_sub(1) {
        if t <= stops[i + 1].t {
            let span = stops[i + 1].t - stops[i].t;
            let f = if span > 0.0 {
                (t - stops[i].t) / span
            } else {
                0.0
            };
            let a = stop_color(&stops[i], c_scale, alpha);
            let b = stop_color(&stops[i + 1], c_scale, alpha);
            return Color32::from_rgba_unmultiplied(
                (a.r() as f32 + (b.r() as f32 - a.r() as f32) * f + 0.5).round() as u8,
                (a.g() as f32 + (b.g() as f32 - a.g() as f32) * f + 0.5).round() as u8,
                (a.b() as f32 + (b.b() as f32 - a.b() as f32) * f + 0.5).round() as u8,
                alpha,
            );
        }
    }
    stop_color(&stops[stops.len() - 1], c_scale, alpha)
}

// ===== 条带分区右缘助手（SpectrumPad.h:633-634） =====
/// 增益条列右缘（= VU 刻度列左缘）。
pub fn lr_zone_r(plot: Rect) -> f32 {
    plot.right() + 2.0 * K_GAIN_BAR_W
}

/// VU 条列右缘（= LUFS 刻度列左缘）。
pub fn vu_zone_r(plot: Rect) -> f32 {
    lr_zone_r(plot) + K_VU_SCALE_W + 2.0 * K_VU_BAR_W
}
