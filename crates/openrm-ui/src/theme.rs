//! 主题：把 C++ `plugins/common/Theme.h` 的 HSB 色阶机制原样搬到 Rust。
//!
//! 关键点是不能"凭感觉调色"。原版的 `COL_*` / `WarmGray` / 随机色板都是
//! 由 (hue, satMax, mode) 三个旋钮实时算出来的，`SettingsPanel` 里的主题色相
//! 滑杆直接改这三个值。平移时保持同一套 HSB→RGB 公式，切换主题才和 C++ 版一致。

use egui::{Color32, Stroke};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ThemeMode {
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub mode: ThemeMode,
    /// 0..360
    pub hue: u16,
    /// 0..100，对应 C++ 的 ThemeSatMax()
    pub sat_max: u8,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            mode: ThemeMode::Light,
            hue: 45,
            sat_max: 15,
        }
    }
}

const LIGHT_B: [u8; 5] = [10, 40, 60, 90, 95];
const DARK_B: [u8; 5] = [90, 60, 40, 20, 10];

impl Theme {
    fn brightness(&self, idx: usize) -> u8 {
        match self.mode {
            ThemeMode::Light => LIGHT_B[idx],
            ThemeMode::Dark => DARK_B[idx],
        }
    }

    pub fn sat_for_b(&self, b: u8) -> f32 {
        self.sat_max as f32 * ((100.0 - b as f32) / 100.0)
    }

    /// HSB → Color32，公式与 C++ `HSBToIColor` 逐行一致。
    pub fn hsb(h: u16, s: f32, b: f32) -> Color32 {
        let c = b * s;
        let hh = (h % 360) as f32 / 60.0;
        let x = c * (1.0 - ((hh % 2.0) - 1.0).abs());
        let (r, g, bl) = if hh < 1.0 {
            (c, x, 0.0)
        } else if hh < 2.0 {
            (x, c, 0.0)
        } else if hh < 3.0 {
            (0.0, c, x)
        } else if hh < 4.0 {
            (0.0, x, c)
        } else if hh < 5.0 {
            (x, 0.0, c)
        } else {
            (c, 0.0, x)
        };
        let m = b - c;
        Color32::from_rgb(
            ((r + m) * 255.0).round() as u8,
            ((g + m) * 255.0).round() as u8,
            ((bl + m) * 255.0).round() as u8,
        )
    }

    fn col(&self, idx: usize) -> Color32 {
        let b = self.brightness(idx);
        Self::hsb(self.hue, self.sat_for_b(b) / 100.0, b as f32 / 100.0)
    }

    pub fn col_900(&self) -> Color32 {
        self.col(0)
    }
    pub fn col_700(&self) -> Color32 {
        self.col(1)
    }
    pub fn col_500(&self) -> Color32 {
        self.col(2)
    }
    pub fn col_300(&self) -> Color32 {
        self.col(3)
    }
    pub fn col_100(&self) -> Color32 {
        self.col(4)
    }

    /// 面板/控件底色灰（BandPass 的 pad 背景、分区底色）。
    pub fn warm_gray(&self, v: u8) -> Color32 {
        let vv = match self.mode {
            ThemeMode::Light => v,
            ThemeMode::Dark => 255u16.saturating_sub(v as u16).saturating_add(16).min(255) as u8,
        };
        let b = vv as f32 / 255.0;
        let b100 = (b * 100.0).round();
        let s = self.sat_for_b(b100 as u8) / 100.0;
        Self::hsb(self.hue, s, b)
    }

    pub fn hover_overlay(&self) -> Color32 {
        match self.mode {
            ThemeMode::Dark => Color32::from_rgba_unmultiplied(255, 255, 255, 48),
            ThemeMode::Light => Color32::from_rgba_unmultiplied(0, 0, 0, 48),
        }
    }

    /// 四组随机映射色，与主题色相解耦（红色/黄色/蓝色/绿色）。
    pub fn random_color(idx: usize) -> Color32 {
        const COLORS: [Color32; 4] = [
            Color32::from_rgb(224, 66, 61),
            Color32::from_rgb(240, 184, 40),
            Color32::from_rgb(72, 138, 255),
            Color32::from_rgb(80, 190, 96),
        ];
        COLORS[idx.min(3)]
    }

    pub fn dim_color(c: Color32) -> Color32 {
        Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), 90)
    }

    pub fn random_color_dim(idx: usize) -> Color32 {
        let c = Self::random_color(idx);
        Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), 90)
    }

    pub fn random_color_ghost(idx: usize) -> Color32 {
        let c = Self::random_color(idx);
        Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), 150)
    }

    /// 频谱三通道：M 用主题色相，L/R 各偏移 ±30°。
    pub fn channel_colors(&self) -> (Color32, Color32, Color32) {
        let wrap = |h: i32| ((h % 360 + 360) % 360) as u16;
        let (b, s_m, s_lr) = match self.mode {
            ThemeMode::Dark => {
                let b = 0.70;
                let s = (self.sat_max as f32 * 2.5).min(45.0) / 100.0;
                (b, s, s)
            }
            ThemeMode::Light => {
                let b = LIGHT_B[2] as f32 / 100.0;
                let s_m = self.sat_max as f32 / 100.0;
                let s_lr = (self.sat_max as f32).max(15.0) / 100.0;
                (b, s_m, s_lr)
            }
        };
        let s_mix = (s_lr * 1.155).min(1.0);
        (
            Self::hsb(wrap(self.hue as i32 - 30), s_mix, b),
            Self::hsb(wrap(self.hue as i32 + 30), s_mix, b),
            Self::hsb(self.hue, s_m, b),
        )
    }

    pub fn hairline(&self) -> Stroke {
        Stroke::new(1.0_f32, self.warm_gray(214))
    }
}

pub fn rgba(c: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), alpha)
}
