//! 频域声像（PAZ 扇形）+ 共用时域相关度/平衡层。
//!
//! 移植自原版：
//! - `controls/StereoFieldControl.h`（541 行）：16 对数频带 FFT 方位直方图、弹道、
//!   扇形网格/刻度/矢量、复合平衡条。
//! - `controls/StereoCorr.h`（166 行）：64-hop 滑窗相关度/平衡、状态色、底部复合条。
//!
//! 常量名/数值/顺序与原版一一对应。`StereoCorr` 同时供 `gonio.rs` 引用。

use egui::{Align2, Color32, Mesh, Pos2, Rect, Shape, Stroke};
use openrm_analyzer_dsp::fastmath::fast_pwr_to_db;
use openrm_ui::{
    fonts,
    strings::{self, Lang},
    theme::Theme,
};

use super::data::ScopeFrame;
use super::meter;

// ===== StereoCorr.h:30-33 复合条几何/量程 =====
pub const CORR_BAR_H: f32 = 14.0;
pub const CORR_READOUT_H: f32 = 22.0;
pub const STEREO_BELOW_H: f32 = CORR_BAR_H + 4.0 + CORR_READOUT_H + 2.0;
pub const BAL_RANGE_DB: f32 = 24.0;

// ===== StereoFieldControl.h:83-96 常量 =====
const K_FFT_N: usize = 1024;
const K_SCOPE_BANDS: usize = 16;
const K_FFT_ATTACK_SEC: f32 = 0.003;
const K_ANTI_WIN_FRAMES: f32 = 4.0;
const K_IMG_BINS: usize = 64;
const K_IMG_STEP: f32 = std::f32::consts::PI / K_IMG_BINS as f32;
const K_IMG_KERNEL_BINS: f32 = 1.0;
const K_IMG_DECAY_SEC: f32 = 0.7;
const K_IMG_DOME_FRAC: f32 = 0.10;
const K_IMG_ANTI_POW: f32 = 2.0;
const K_IMG_WIN_FRAMES: f32 = 8.0;
const K_FFT_CAL: f64 = 0.375 * K_FFT_N as f64 * K_FFT_N as f64;
const K_CX_N: f32 = 0.28;

// ===== StereoFieldControl.h:102-104 16 带分 bin 表 =====
const K_BAND_BINS: [[usize; 2]; K_SCOPE_BANDS] = [
    [1, 1],
    [2, 2],
    [3, 3],
    [4, 4],
    [5, 7],
    [8, 10],
    [11, 15],
    [16, 22],
    [23, 33],
    [34, 49],
    [50, 72],
    [73, 107],
    [108, 158],
    [159, 234],
    [235, 345],
    [346, 510],
];

// ---------------------------------------------------------------------------
// StereoCorr（原版 StereoCorr.h:36-84）
// ---------------------------------------------------------------------------

pub mod corr {
    use super::*;

    const K_WIN: usize = 64;

    #[derive(Clone, Copy, Default)]
    struct HopSum {
        lr: f64,
        l2: f64,
        r2: f64,
    }

    /// 64-hop 滑窗相关度/平衡 + 150 ms 显示平滑。
    #[derive(Clone)]
    pub struct StereoCorr {
        ring: [HopSum; K_WIN],
        head: usize,
        sum_lr: f64,
        sum_l2: f64,
        sum_r2: f64,
        valid: bool,
        corr_disp: f32,
        bal_disp: f32,
    }

    impl Default for StereoCorr {
        fn default() -> Self {
            Self::new()
        }
    }

    impl StereoCorr {
        pub fn new() -> Self {
            Self {
                ring: [HopSum::default(); K_WIN],
                head: 0,
                sum_lr: 0.0,
                sum_l2: 0.0,
                sum_r2: 0.0,
                valid: false,
                corr_disp: 0.0,
                bal_disp: 0.0,
            }
        }

        /// 原版 `Reset`（:49-55）。
        pub fn reset(&mut self) {
            self.ring = [HopSum::default(); K_WIN];
            self.head = 0;
            self.sum_lr = 0.0;
            self.sum_l2 = 0.0;
            self.sum_r2 = 0.0;
            self.valid = false;
            self.corr_disp = 0.0;
            self.bal_disp = 0.0;
        }

        /// 原版 `Update`（:58-83）：L/R 原始样本，n = hop 长度，hopSec = hop 时长。
        pub fn update(&mut self, l: &[f32], r: &[f32], n: usize, hop_sec: f64) {
            let n = n.min(l.len()).min(r.len());
            let mut lr = 0.0_f64;
            let mut l2 = 0.0_f64;
            let mut r2 = 0.0_f64;
            for i in 0..n {
                let lv = l[i] as f64;
                let rv = r[i] as f64;
                lr += lv * rv;
                l2 += lv * lv;
                r2 += rv * rv;
            }
            let old = self.ring[self.head];
            self.sum_lr += lr - old.lr;
            self.sum_l2 += l2 - old.l2;
            self.sum_r2 += r2 - old.r2;
            self.ring[self.head] = HopSum { lr, l2, r2 };
            self.head = (self.head + 1) & (K_WIN - 1);

            if self.sum_l2 + self.sum_r2 > 1e-10 {
                let corr = (self.sum_lr / (self.sum_l2 * self.sum_r2).max(1e-24).sqrt())
                    .clamp(-1.0, 1.0) as f32;
                let bal_db = fast_pwr_to_db(
                    (self.sum_r2 / self.sum_l2.max(1e-12)) as f32,
                    -120.0,
                );
                let sm = (1.0 - (-hop_sec / 0.15).exp()) as f32;
                self.corr_disp += sm * (corr - self.corr_disp);
                self.bal_disp += sm * (bal_db - self.bal_disp);
                self.valid = true;
            } else {
                self.valid = false;
            }
        }

        pub fn valid(&self) -> bool {
            self.valid
        }
        pub fn corr(&self) -> f32 {
            self.corr_disp
        }
        pub fn bal(&self) -> f32 {
            self.bal_disp
        }

        /// 未平滑的滑窗原始相关度（测试用，等价 Update 内未走显示平滑的 corr）。
        pub fn raw_corr(&self) -> f32 {
            if self.sum_l2 + self.sum_r2 > 1e-10 {
                (self.sum_lr / (self.sum_l2 * self.sum_r2).max(1e-24).sqrt())
                    .clamp(-1.0, 1.0) as f32
            } else {
                0.0
            }
        }
    }
}

pub use corr::StereoCorr;

// ---------------------------------------------------------------------------
// 状态色（原版 StereoCorr.h:87-105）
// ---------------------------------------------------------------------------

/// 原版 `StereoMeterSatScale`：0 饱和主题下退化为灰度。
fn stereo_meter_sat_scale(theme: &Theme) -> f32 {
    let sat = theme.sat_max as i32;
    if sat <= 30 {
        sat as f32 / 30.0
    } else {
        1.0 + (sat - 30) as f32 / 55.0
    }
}

/// 原版 `StereoSemColor`。
pub fn stereo_sem_color(theme: &Theme, c: Color32) -> Color32 {
    let m = stereo_meter_sat_scale(theme).clamp(0.0, 1.0);
    let lum = 0.299 * c.r() as f32 + 0.587 * c.g() as f32 + 0.114 * c.b() as f32;
    Color32::from_rgb(
        (c.r() as f32 * m + lum * (1.0 - m)).round() as u8,
        (c.g() as f32 * m + lum * (1.0 - m)).round() as u8,
        (c.b() as f32 * m + lum * (1.0 - m)).round() as u8,
    )
}

/// 原版 `StereoCorrColor`：≥+0.3 绿 / 0~+0.3 黄 / <0 红。
pub fn stereo_corr_color(theme: &Theme, c: f32) -> Color32 {
    let base = if c >= 0.3 {
        meter::METER_GREEN
    } else if c >= 0.0 {
        meter::METER_YELLOW
    } else {
        meter::METER_RED
    };
    stereo_sem_color(theme, base)
}

// ---------------------------------------------------------------------------
// 频域声像控件
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Default)]
struct Band {
    db: f32,
    ang: f32,
}

/// 频域 PAZ 扇形 retained 控件。
pub struct FieldControl {
    pub rect: Rect,

    band: [Band; K_SCOPE_BANDS],
    img: [f32; K_IMG_BINS],

    corr: StereoCorr,
    sample_rate: f64,
    release_sec: f32,
    release_mode: i32,
    floor_db: f32,

    rc_win: [f64; K_SCOPE_BANDS],
    e_win: [f64; K_SCOPE_BANDS],
    dl_in_win: [f64; K_SCOPE_BANDS],
    rc_in_win: [f64; K_SCOPE_BANDS],

    // FFT scratch
    re_l: Vec<f32>,
    im_l: Vec<f32>,
    re_r: Vec<f32>,
    im_r: Vec<f32>,

    hover_active: bool,
    hover_x: f32,
    hover_y: f32,
    hover_th: f32,
    hover_buf: String,
    hover_box: Rect,
}

impl Default for FieldControl {
    fn default() -> Self {
        Self::new(Rect::from_min_max(
            Pos2::new(20.0, 366.0),
            Pos2::new(394.0, 599.0),
        ))
    }
}

impl FieldControl {
    pub fn new(rect: Rect) -> Self {
        Self {
            rect,
            band: [Band::default(); K_SCOPE_BANDS],
            img: [0.0; K_IMG_BINS],
            corr: StereoCorr::new(),
            sample_rate: 48_000.0,
            release_sec: 0.2,
            release_mode: 0,
            floor_db: -80.0,
            rc_win: [0.0; K_SCOPE_BANDS],
            e_win: [0.0; K_SCOPE_BANDS],
            dl_in_win: [0.0; K_SCOPE_BANDS],
            rc_in_win: [0.0; K_SCOPE_BANDS],
            re_l: vec![0.0; K_FFT_N],
            im_l: vec![0.0; K_FFT_N],
            re_r: vec![0.0; K_FFT_N],
            im_r: vec![0.0; K_FFT_N],
            hover_active: false,
            hover_x: 0.0,
            hover_y: 0.0,
            hover_th: 0.0,
            hover_buf: String::new(),
            hover_box: Rect::ZERO,
        }
    }

    // ==================== kMsgTag* 对应 setter ====================

    pub fn set_sample_rate(&mut self, sr: f64) {
        self.sample_rate = sr.max(1000.0);
    }

    /// 原版 kMsgTagRelease：释放弹道秒数（:109 用到）。
    pub fn set_release_sec(&mut self, sec: f32) {
        self.release_sec = sec;
    }

    /// 原版 kMsgTagReleaseMode：0=LOG，1=LIN 匀速（:209）。
    pub fn set_release_mode(&mut self, mode: i32) {
        self.release_mode = mode;
    }

    /// 原版 kMsgTagReset / `ResetDisplay`（:214-223）。
    pub fn clear_peak_hold(&mut self) {
        self.band = [Band::default(); K_SCOPE_BANDS];
        self.img = [0.0; K_IMG_BINS];
        self.rc_win = [0.0; K_SCOPE_BANDS];
        self.e_win = [0.0; K_SCOPE_BANDS];
        self.dl_in_win = [0.0; K_SCOPE_BANDS];
        self.rc_in_win = [0.0; K_SCOPE_BANDS];
        self.corr.reset();
    }

    pub fn corr(&self) -> &StereoCorr {
        &self.corr
    }

    // ==================== 数据管线（原版 ProcessPacket:106-204） ====================

    pub fn update(&mut self, frame: &ScopeFrame) {
        let hop_sec = 1024.0 / self.sample_rate.max(1.0);
        let a_coef = (-hop_sec / K_FFT_ATTACK_SEC as f64).exp() as f32;
        let r_coef = (-hop_sec / self.release_sec.max(1e-3) as f64).exp() as f32;
        let unif_step =
            (hop_sec / (2.0 * self.release_sec.max(1e-3) as f64) * -self.floor_db as f64) as f32;

        // L/R 1024 点 FFT（Hann 窗）
        let n = K_FFT_N.min(frame.l.len()).min(frame.r.len());
        for i in 0..K_FFT_N {
            let w = if i < n {
                0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / K_FFT_N as f32).cos()
            } else {
                0.0
            };
            self.re_l[i] = frame.l.get(i).copied().unwrap_or(0.0) * w;
            self.im_l[i] = 0.0;
            self.re_r[i] = frame.r.get(i).copied().unwrap_or(0.0) * w;
            self.im_r[i] = 0.0;
        }
        fft_radix2(&mut self.re_l, &mut self.im_l);
        fft_radix2(&mut self.re_r, &mut self.im_r);

        let mut e_in = [0.0_f64; K_SCOPE_BANDS];
        let mut dl_in = [0.0_f64; K_SCOPE_BANDS];
        let mut rc_in = [0.0_f64; K_SCOPE_BANDS];
        let mut e_anti = [0.0_f64; K_SCOPE_BANDS];
        let mut dl_anti = [0.0_f64; K_SCOPE_BANDS];
        let mut rc_anti = [0.0_f64; K_SCOPE_BANDS];

        for b in 0..K_SCOPE_BANDS {
            for k in K_BAND_BINS[b][0]..=K_BAND_BINS[b][1] {
                let l2 = self.re_l[k] as f64 * self.re_l[k] as f64
                    + self.im_l[k] as f64 * self.im_l[k] as f64;
                let r2 = self.re_r[k] as f64 * self.re_r[k] as f64
                    + self.im_r[k] as f64 * self.im_r[k] as f64;
                let rck = self.re_l[k] as f64 * self.re_r[k] as f64
                    + self.im_l[k] as f64 * self.im_r[k] as f64;
                if rck >= 0.0 {
                    e_in[b] += l2 + r2;
                    dl_in[b] += r2 - l2;
                    rc_in[b] += rck;
                } else {
                    e_anti[b] += l2 + r2;
                    dl_anti[b] += r2 - l2;
                    rc_anti[b] += rck;
                }
            }
        }

        let e_b: [f64; K_SCOPE_BANDS] = std::array::from_fn(|b| e_in[b] + e_anti[b]);
        let dl_b: [f64; K_SCOPE_BANDS] = std::array::from_fn(|b| dl_in[b] + dl_anti[b]);
        let rc_b: [f64; K_SCOPE_BANDS] = std::array::from_fn(|b| rc_in[b] + rc_anti[b]);

        // 时域相关度/平衡（共用 StereoCorr）
        self.corr.update(&frame.l, &frame.r, n, hop_sec);

        for b in 0..K_SCOPE_BANDS {
            let e = e_b[b];
            let raw_db = if e > 1e-12 {
                fast_pwr_to_db((e / K_FFT_CAL) as f32, -120.0)
            } else {
                -120.0
            };
            let target = raw_db.clamp(self.floor_db - 0.1 * -self.floor_db, 0.0);
            self.band[b].db = step_smoothed(
                self.band[b].db,
                target,
                a_coef,
                r_coef,
                unif_step,
                self.release_mode,
            );
            self.band[b].ang = if e > 1e-12 {
                0.5 * (dl_b[b] as f32).atan2(2.0 * rc_b[b] as f32)
            } else {
                0.0
            };

            if e > 1e-12 {
                self.rc_win[b] += (rc_b[b] - self.rc_win[b]) / K_ANTI_WIN_FRAMES as f64;
                self.e_win[b] += (e - self.e_win[b]) / K_ANTI_WIN_FRAMES as f64;
            }
            if e_in[b] > 1e-12 {
                self.dl_in_win[b] += (dl_in[b] - self.dl_in_win[b]) / K_IMG_WIN_FRAMES as f64;
                self.rc_in_win[b] += (rc_in[b] - self.rc_in_win[b]) / K_IMG_WIN_FRAMES as f64;
            }
        }

        // 方位直方图：每 hop 衰减，同相组直接注入中心锥，反相组按相关负度加权
        let img_coef = (-hop_sec / K_IMG_DECAY_SEC as f64).exp() as f32;
        for i in 0..K_IMG_BINS {
            self.img[i] *= img_coef;
        }

        let inject = |img: &mut [f32; K_IMG_BINS], th: f32, db: f32, floor_db: f32, r_max: f32| {
            if db <= floor_db + 0.5 {
                return;
            }
            let r_norm = radius_for(db, r_max, floor_db);
            let fc = (th + 0.5 * std::f32::consts::PI) / K_IMG_STEP - 0.5;
            let i0 = (0.0_f32).max(fc - K_IMG_KERNEL_BINS).ceil() as usize;
            let i1 = ((K_IMG_BINS - 1) as f32).min(fc + K_IMG_KERNEL_BINS).floor() as usize;
            for i in i0..=i1 {
                let k = 0.5 * (1.0 + (std::f32::consts::PI * (i as f32 - fc).abs() / K_IMG_KERNEL_BINS).cos());
                img[i] = img[i].max(r_norm * k);
            }
        };

        let r_max = 1.0; // 注入用归一化半径（RadiusFor 内乘 rMax，这里 rMax=1）
        let group_th = |dl: f32, rc: f32| {
            (0.5 * dl.atan2(2.0 * rc))
                .clamp(-0.5 * std::f32::consts::PI, 0.5 * std::f32::consts::PI)
        };

        for b in 0..K_SCOPE_BANDS {
            if e_in[b] > 1e-12 {
                let db = fast_pwr_to_db((e_in[b] / K_FFT_CAL) as f32, -120.0);
                inject(
                    &mut self.img,
                    group_th(self.dl_in_win[b] as f32, self.rc_in_win[b] as f32),
                    db,
                    self.floor_db,
                    r_max,
                );
            }
            if e_b[b] > 1e-12 && self.e_win[b] > 1e-12 {
                let w = (-self.rc_win[b] / (0.5 * self.e_win[b]))
                    .clamp(0.0, 1.0)
                    .powf(K_IMG_ANTI_POW as f64) as f32;
                if w > 1e-4 {
                    let db = fast_pwr_to_db((e_anti[b] / K_FFT_CAL) as f32, -120.0)
                        + 20.0 * w.log10();
                    inject(
                        &mut self.img,
                        group_th(dl_anti[b] as f32, rc_anti[b] as f32),
                        db,
                        self.floor_db,
                        r_max,
                    );
                }
            }
        }
    }

    // ==================== 几何（原版 FanGeom:226-232） ====================

    fn fan_geom(&self) -> (f32, f32, f32) {
        let cv = self.rect;
        let cx = if cv.width() <= 500.0 {
            cv.center().x
        } else {
            cv.left() + cv.width() * K_CX_N
        };
        let r_horiz = (cv.width() * 0.5 - 2.0).max(12.0);
        let r_vert = (cv.bottom() - STEREO_BELOW_H - cv.top()).max(12.0);
        let r_max = if cv.width() <= 500.0 {
            r_vert.min(r_horiz)
        } else {
            r_vert
        };
        let cy = cv.top() + r_max;
        (cx, cy, r_max)
    }

    // ==================== 命中 / hover ====================

    pub fn click(&mut self, _pos: Pos2) -> Option<()> {
        // 原版 OnMouseDown/Up/Drag 为空。
        None
    }

    pub fn hover(&mut self, pos: Pos2) -> Option<String> {
        let cv = self.rect;
        if !cv.contains(pos) {
            self.hover_active = false;
            return None;
        }
        self.hover_active = true;
        self.hover_x = pos.x;
        self.hover_y = pos.y;
        None
    }

    // ==================== 绘制 ====================

    pub fn draw(&mut self, painter: &egui::Painter, theme: &Theme, lang: Lang) {
        painter.rect_filled(self.rect, 0.0, theme.col_100());
        let cv = self.rect;
        let (cx, cy, r_max) = self.fan_geom();

        // 原版 Draw:73-77：先 ComputeHover 拿到 labelBox，作为刻度 skipRect 传入 DrawTicks。
        if self.hover_active {
            self.compute_hover(painter, theme, cx, cy, r_max);
        }
        let skip = if self.hover_active && !rect_empty(self.hover_box) {
            self.hover_box
        } else {
            Rect::ZERO
        };

        self.draw_grid_content(painter, theme, cx, cy, r_max);
        self.draw_ticks(painter, theme, cx, cy, r_max, skip);
        self.draw_vectors(painter, theme, cx, cy, r_max);
        draw_stereo_corr_bar(painter, theme, cx, r_max.min((cx - cv.left() - 2.0).min(cv.right() - cx - 2.0)), cy, &self.corr, lang);

        if self.hover_active && !rect_empty(self.hover_box) {
            self.draw_hover(painter, theme, cx, cy, r_max);
        }
    }

    /// 扇形色块网格（原版 DrawGridContent:256-279）。
    fn draw_grid_content(&self, painter: &egui::Painter, theme: &Theme, cx: f32, cy: f32, r_max: f32) {
        let floor_int = self.floor_db as i32;
        let k_deg = std::f32::consts::PI / 180.0;
        let mut j = 0;
        loop {
            let db_hi = -20 * j;
            if db_hi <= floor_int {
                break;
            }
            let db_lo = (db_hi - 20).max(floor_int);
            let r_hi = radius_for(db_hi as f32, r_max, self.floor_db);
            let r_lo = radius_for(db_lo as f32, r_max, self.floor_db);
            let t_row = (0.0 - (db_hi + db_lo) as f32 * 0.5) / (0.0 - floor_int as f32);
            let v_db = 245.0 + (172.0 - 245.0) * t_row;
            for i in 0..12 {
                let a0 = (-90.0 + 15.0 * i as f32) * k_deg;
                let a1 = a0 + 15.0 * k_deg;
                let aa = (a0 + 7.5 * k_deg).abs();
                let v_ang = if aa <= 15.0 * k_deg {
                    230.0
                } else if aa <= 45.0 * k_deg {
                    215.0
                } else {
                    195.0
                };
                let v = (0.5 * (v_ang + v_db)).round() as u8;
                fill_sector(painter, cx, cy, r_lo, r_hi, a0, a1, meter::grid_gray(theme, v));
            }
            j += 1;
        }
    }

    /// 场内标注（原版 DrawTicks:313-361）。
    fn draw_ticks(&self, painter: &egui::Painter, theme: &Theme, cx: f32, cy: f32, r_max: f32, skip: Rect) {
        // L/R 方向字：原版沿半径旋转 ±30°（IText 角度，顺时针为正）。
        // epaint 0.34 `TextShape::angle` 支持绕锚点旋转，这里以字心为锚点复刻。
        const K_LR_DEG: f32 = 30.0;
        const K_LR_PAD: f32 = 26.0;
        let th_rad = K_LR_DEG * std::f32::consts::PI / 180.0;
        let r_lbl = r_max - K_LR_PAD;
        let font_lr = fonts::bold(40.0); // 原版 kLrSize=40, kFontBold（×0.8 字号系数）
        for side in 0..2 {
            let is_l = side == 0;
            let th = (if is_l { -1.0 } else { 1.0 }) * th_rad;
            let px = cx + th.sin() * r_lbl;
            let py = cy - th.cos() * r_lbl;
            let txt = if is_l { "L" } else { "R" };
            let galley = painter.layout_no_wrap(txt.to_owned(), font_lr.clone(), theme.col_500());
            let size = galley.size();
            let box_r = Rect::from_min_max(
                Pos2::new(px - size.x * 0.5, py - size.y * 0.5),
                Pos2::new(px + size.x * 0.5, py + size.y * 0.5),
            );
            if !rect_empty(skip) && box_r.intersects(skip) {
                continue;
            }
            let shape = egui::epaint::TextShape::new(box_r.min, galley, theme.col_500())
                .with_angle_and_anchor(th, Align2::CENTER_CENTER);
            painter.add(Shape::Text(shape));
        }

        let ap_font = fonts::regular(14.0);
        let ax = r_max - 50.0;
        let ap_box_l = Rect::from_min_max(
            Pos2::new(cx - ax - 36.0, cy - 19.0),
            Pos2::new(cx - ax + 36.0, cy - 5.0),
        );
        let ap_box_r = Rect::from_min_max(
            Pos2::new(cx + ax - 36.0, cy - 19.0),
            Pos2::new(cx + ax + 36.0, cy - 5.0),
        );
        if rect_empty(skip) || !ap_box_l.intersects(skip) {
            painter.text(ap_box_l.center(), Align2::CENTER_CENTER, "Anti Phase", ap_font.clone(), theme.col_700());
        }
        if rect_empty(skip) || !ap_box_r.intersects(skip) {
            painter.text(ap_box_r.center(), Align2::CENTER_CENTER, "Anti Phase", ap_font, theme.col_700());
        }

        let db_font = fonts::regular(14.0);
        let floor_int = self.floor_db as i32;
        let mut db = 0;
        while db >= floor_int {
            let r = radius_for(db as f32, r_max, self.floor_db);
            if r > 20.0 {
                let y = cy - r;
                let box_r = Rect::from_min_max(
                    Pos2::new(cx + 5.0, y + 1.0),
                    Pos2::new(cx + 48.0, y + 17.0),
                );
                if rect_empty(skip) || !box_r.intersects(skip) {
                    painter.text(box_r.min, Align2::LEFT_TOP, format!("{db}"), db_font.clone(), theme.col_700());
                }
            }
            db -= 20;
        }
    }

    /// 方位直方图矢量（原版 DrawVectors:364-407）。
    fn draw_vectors(&self, painter: &egui::Painter, theme: &Theme, cx: f32, cy: f32, r_max: f32) {
        let (_, _, c_m) = theme.channel_colors();

        let mut max_img = 0.0_f32;
        for v in self.img.iter() {
            max_img = max_img.max(*v);
        }
        if max_img <= 1e-3 {
            return;
        }

        let r_dome = 3.0_f32.max(r_max * K_IMG_DOME_FRAC);
        let r_peak = (max_img * r_max).max(r_dome + 1.0);

        // 径向渐变 stops：alpha = 15 + 240·exp(-3.5(1-s))（原版 :382-389）
        const K_N: usize = 8;
        const K_MIN_A: f32 = 15.0;
        let mut stops: [(f32, f32); K_N] = [(0.0, 0.0); K_N];
        for i in 0..K_N {
            let s = i as f32 / (K_N - 1) as f32;
            let w = (-3.5 * (1.0 - s)).exp();
            let a = (K_MIN_A + (255.0 - K_MIN_A) * w).round();
            let pos = (r_dome + (r_peak - r_dome) * s) / r_peak;
            stops[i] = (pos, a);
        }
        let alpha_at = |r: f32| -> u8 {
            let pos = r / r_peak;
            let mut a = K_MIN_A;
            for win in stops.windows(2) {
                let (p0, a0) = win[0];
                let (p1, a1) = win[1];
                if pos >= p0 && pos <= p1 {
                    let t = if p1 > p0 { (pos - p0) / (p1 - p0) } else { 0.0 };
                    a = a0 + (a1 - a0) * t;
                    break;
                }
            }
            a.round().clamp(0.0, 255.0) as u8
        };

        // epaint 0.34 无径向渐变图元：按原版 8-stop 表取 alpha，用「外轮廓环 +
        // 中环 + 中心顶点」三层扇形网格逼近；中环半径取穹顶到外轮廓的中点，
        // 把 barycentric 线性插值的误差压在半径步长内。
        let mut mesh = Mesh::default();
        let mut outer: Vec<Pos2> = Vec::with_capacity(K_IMG_BINS);
        let mut outer_alpha: Vec<u8> = Vec::with_capacity(K_IMG_BINS);
        let mut mid: Vec<Pos2> = Vec::with_capacity(K_IMG_BINS);
        let mut mid_alpha: Vec<u8> = Vec::with_capacity(K_IMG_BINS);
        for i in 0..K_IMG_BINS {
            let th = -0.5 * std::f32::consts::PI + (i as f32 + 0.5) * K_IMG_STEP;
            let r = self.img[i] * r_max;
            let r = r.max(r_dome);
            let r_mid = r_dome + 0.5 * (r - r_dome);
            outer.push(Pos2::new(cx + th.sin() * r, cy - th.cos() * r));
            outer_alpha.push(alpha_at(r));
            mid.push(Pos2::new(cx + th.sin() * r_mid, cy - th.cos() * r_mid));
            mid_alpha.push(alpha_at(r_mid));
        }

        let col = |alpha: u8| Color32::from_rgba_unmultiplied(c_m.r(), c_m.g(), c_m.b(), alpha);
        let ia = mesh.vertices.len() as u32;
        for i in 0..K_IMG_BINS {
            mesh.vertices.push(egui::epaint::Vertex {
                pos: outer[i],
                uv: egui::epaint::WHITE_UV,
                color: col(outer_alpha[i]),
            });
        }
        let im = mesh.vertices.len() as u32;
        for i in 0..K_IMG_BINS {
            mesh.vertices.push(egui::epaint::Vertex {
                pos: mid[i],
                uv: egui::epaint::WHITE_UV,
                color: col(mid_alpha[i]),
            });
        }
        let ic = mesh.vertices.len() as u32;
        mesh.vertices.push(egui::epaint::Vertex {
            pos: Pos2::new(cx, cy),
            uv: egui::epaint::WHITE_UV,
            color: col(K_MIN_A as u8),
        });

        for i in 0..K_IMG_BINS {
            let j = (i + 1) % K_IMG_BINS;
            let ou = ia + i as u32;
            let oj = ia + j as u32;
            let mu = im + i as u32;
            let mj = im + j as u32;
            // 外环 ↔ 中环四边形
            mesh.indices.extend_from_slice(&[ou, oj, mj, ou, mj, mu]);
            // 中环 ↔ 中心三角形
            mesh.indices.extend_from_slice(&[mu, mj, ic]);
        }
        painter.add(Shape::mesh(mesh));
    }

    // ==================== hover（原版 ComputeHover:418-467 / DrawHover:469-475） ====================

    fn compute_hover(&mut self, painter: &egui::Painter, theme: &Theme, cx: f32, cy: f32, r_max: f32) {
        self.hover_box = Rect::ZERO;
        let dx = self.hover_x - cx;
        let dy_up = cy - self.hover_y;
        if self.hover_y > cy || dx * dx + dy_up * dy_up > r_max * r_max + 4.0 {
            self.hover_active = false;
            return;
        }
        self.hover_th = dx.atan2(dy_up.max(0.001));

        let mut max_db = self.floor_db;
        for b in self.band.iter() {
            max_db = max_db.max(b.db);
        }
        let thr = (max_db - 35.0).max(self.floor_db + 0.5);
        let mut best = 1e9_f32;
        let mut bi = -1_i32;
        for (b, band) in self.band.iter().enumerate() {
            if band.db < thr {
                continue;
            }
            let d = (band.ang - self.hover_th).abs();
            if d < best {
                best = d;
                bi = b as i32;
            }
        }
        if bi >= 0 && best < 0.14 {
            let b = bi as usize;
            let hz = 0.5 * (K_BAND_BINS[b][0] + K_BAND_BINS[b][1]) as f32 * self.sample_rate as f32 / K_FFT_N as f32;
            let fb = if hz < 1000.0 {
                format!("{:.0} Hz", hz)
            } else {
                format!("{:.2} kHz", hz / 1000.0)
            };
            self.hover_buf = format!("{:.1}°  {}  {:.1} dB", self.hover_th.to_degrees(), fb, self.band[b].db);
        } else {
            self.hover_buf = format!("{:.1}°", self.hover_th.to_degrees());
        }

        let font = fonts::regular(14.0);
        let galley = painter.layout_no_wrap(self.hover_buf.clone(), font, theme.col_700());
        let w = galley.size().x;
        let px = cx + (r_max + 18.0) * self.hover_th.sin();
        let py = (cy - (r_max + 18.0) * self.hover_th.cos()).max(self.rect.top() + 17.0);
        let cx_l = (px - w * 0.5).clamp(self.rect.left() + 2.0, self.rect.right() - 2.0 - w);
        self.hover_box = Rect::from_min_max(
            Pos2::new(cx_l, py - 17.0),
            Pos2::new(cx_l + w, py - 1.0),
        );
    }

    fn draw_hover(&self, painter: &egui::Painter, theme: &Theme, cx: f32, cy: f32, r_max: f32) {
        painter.line_segment(
            [
                Pos2::new(cx, cy),
                Pos2::new(cx + self.hover_th.sin() * r_max, cy - self.hover_th.cos() * r_max),
            ],
            Stroke::new(1.0_f32, theme.col_700()),
        );
        painter.text(
            self.hover_box.min,
            Align2::LEFT_BOTTOM,
            &self.hover_buf,
            fonts::regular(14.0),
            theme.col_700(),
        );
    }
}

// ---------------------------------------------------------------------------
// 模块级工具
// ---------------------------------------------------------------------------

/// 等价原版 IRECT::Empty()：零面积矩形视为空。
fn rect_empty(r: Rect) -> bool {
    r == Rect::ZERO
}

/// 原版 `StepSmoothed`（:206-212）。
fn step_smoothed(prev_db: f32, target_db: f32, a_coef: f32, r_coef: f32, unif_step: f32, release_mode: i32) -> f32 {
    if target_db > prev_db {
        a_coef * prev_db + (1.0 - a_coef) * target_db
    } else if release_mode == 1 {
        target_db.max(prev_db - unif_step)
    } else {
        r_coef * prev_db + (1.0 - r_coef) * target_db
    }
}

/// 原版 `RadiusFor`（:234-236）。
fn radius_for(db: f32, r_max: f32, floor_db: f32) -> f32 {
    r_max * ((db - floor_db) / (0.0 - floor_db)).clamp(0.0, 1.0)
}

/// 原版 `FillSector`（:282-310）：扇形楔形填充。
fn fill_sector(
    painter: &egui::Painter,
    cx: f32,
    cy: f32,
    r_lo: f32,
    r_hi: f32,
    a0: f32,
    a1: f32,
    c: Color32,
) {
    if r_hi - r_lo <= 0.5 {
        return;
    }
    const K_PAD: f32 = 0.003;
    const K_SEGS: usize = 4;
    let r_lo = (r_lo - 0.75).max(0.0);
    let a0 = a0 - K_PAD;
    let a1 = a1 + K_PAD;

    let mut pts: Vec<Pos2> = Vec::with_capacity((K_SEGS + 1) * 2 + 1);
    if r_lo <= 1.0 {
        pts.push(Pos2::new(cx, cy));
    } else {
        for k in 0..=K_SEGS {
            let a = a0 + (a1 - a0) * k as f32 / K_SEGS as f32;
            pts.push(Pos2::new(cx + a.sin() * r_lo, cy - a.cos() * r_lo));
        }
    }
    for k in (0..=K_SEGS).rev() {
        let a = a0 + (a1 - a0) * k as f32 / K_SEGS as f32;
        pts.push(Pos2::new(cx + a.sin() * r_hi, cy - a.cos() * r_hi));
    }
    painter.add(Shape::convex_polygon(pts, c, Stroke::NONE));
}

/// 基-2 FFT（原版 `FftRadix2`:477-508，正向）。
fn fft_radix2(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -2.0 * std::f32::consts::PI / len as f32;
        let w_re = ang.cos();
        let w_im = ang.sin();
        let half = len >> 1;
        for i in (0..n).step_by(len) {
            let mut c_re = 1.0_f32;
            let mut c_im = 0.0_f32;
            for k in 0..half {
                let u_re = re[i + k];
                let u_im = im[i + k];
                let v_re = re[i + k + half] * c_re - im[i + k + half] * c_im;
                let v_im = re[i + k + half] * c_im + im[i + k + half] * c_re;
                re[i + k] = u_re + v_re;
                im[i + k] = u_im + v_im;
                re[i + k + half] = u_re - v_re;
                im[i + k + half] = u_im - v_im;
                let n_re = c_re * w_re - c_im * w_im;
                c_im = c_re * w_im + c_im * w_re;
                c_re = n_re;
            }
        }
        len <<= 1;
    }
}

// ---------------------------------------------------------------------------
// 底部复合条（原版 DrawStereoCorrBar:109-163）
// ---------------------------------------------------------------------------

/// 相关性单声道窗口 + ±24 dB 平衡指针 + 两角读数。
pub fn draw_stereo_corr_bar(
    painter: &egui::Painter,
    theme: &Theme,
    cx: f32,
    half: f32,
    track_t: f32,
    corr: &StereoCorr,
    lang: Lang,
) {
    let x0 = cx - half;
    let x1 = cx + half;
    let track_b = track_t + CORR_BAR_H;
    let ro_t = track_b + 4.0;
    let _ro_b = ro_t + CORR_READOUT_H;
    let (c_l, c_r, _) = theme.channel_colors();

    painter.rect_filled(Rect::from_min_max(Pos2::new(x0, track_t), Pos2::new(x1, track_b)), 0.0, theme.col_300());

    if corr.valid() {
        let corr_v = corr.corr().clamp(-1.0, 1.0);
        let cc = stereo_corr_color(theme, corr_v);
        let half_w = 0.5 * corr_v.abs() * (x1 - x0);
        painter.rect_filled(
            Rect::from_min_max(Pos2::new(cx - half_w, track_t), Pos2::new(cx + half_w, track_b)),
            0.0,
            Color32::from_rgba_unmultiplied(cc.r(), cc.g(), cc.b(), 150),
        );
        let bal = corr.bal().clamp(-BAL_RANGE_DB, BAL_RANGE_DB);
        let nx = cx + bal / BAL_RANGE_DB * half;
        painter.rect_filled(
            Rect::from_min_max(Pos2::new(nx - 1.5, track_t), Pos2::new(nx + 1.5, track_b)),
            0.0,
            theme.col_700(),
        );
    }

    let font_lr = fonts::semibold(12.0);
    painter.text(Pos2::new(x0 + 5.0, track_t), Align2::LEFT_CENTER, "L", font_lr.clone(), c_l);
    painter.text(Pos2::new(x1 - 5.0, track_t), Align2::RIGHT_CENTER, "R", font_lr, c_r);

    let corr_lbl = strings::analyzer_label("CORRELATION", lang);
    let bal_lbl = strings::analyzer_label("BALANCE", lang);
    let font_ro = fonts::semibold(16.0);

    if corr.valid() {
        let corr_txt = format!("{}  {:+.2}", corr_lbl, corr.corr());
        painter.text(
            Pos2::new(x0, ro_t),
            Align2::LEFT_CENTER,
            corr_txt,
            font_ro.clone(),
            stereo_corr_color(theme, corr.corr().clamp(-1.0, 1.0)),
        );
        let bal_txt = if corr.bal().abs() < 0.1 {
            format!("{}  C", bal_lbl)
        } else {
            let side = if corr.bal() > 0.0 { "R" } else { "L" };
            format!("{}  {} {:.1} dB", bal_lbl, side, corr.bal().abs())
        };
        painter.text(Pos2::new(x1, ro_t), Align2::RIGHT_CENTER, bal_txt, font_ro, theme.col_900());
    } else {
        painter.text(
            Pos2::new(x0, ro_t),
            Align2::LEFT_CENTER,
            format!("{}  —", corr_lbl),
            font_ro.clone(),
            theme.col_500(),
        );
        painter.text(Pos2::new(x1, ro_t), Align2::RIGHT_CENTER, format!("{}  —", bal_lbl), font_ro, theme.col_500());
    }
}
