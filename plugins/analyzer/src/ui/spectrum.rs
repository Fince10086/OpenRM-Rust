//! 主频谱控件 —— 逐行移植自原版 `controls/SpectrumPad.h`（1611 行）。
//!
//! 职责：四模式（STFT/VQT/PBT/RTA）频谱聚合与非对称平滑、峰值保持、
//! 频谱曲线/填充绘制、右缘 200px 电平/响度条、hover 十字线与点击命中。
//! 数据一律线性幅度，dB 在控件内换算（与原版一致）。
//!
//! 常量名/数值/顺序与原版一一对应，注释里标了原版符号。

use std::time::Instant;

use egui::{Align2, Color32, Pos2, Rect, Shape, Stroke};
use openrm_analyzer_dsp::fastmath::fast_log2;
use openrm_ui::{
    fonts,
    strings::{self, Lang},
    theme::{Theme, ThemeMode},
};

use super::data::{LevelSnapshot, LoudnessSnapshot, SpectrumFrame};
use super::meter::{
    self, K_GAIN_BAR_W, K_LRA_ZONE_W, K_LUFS_SCALE_W, K_LOUD_BAR_W, K_METER_STRIP_W,
    K_VU_BAR_W, K_VU_BOTTOM_VU, K_VU_SCALE_W, K_VU_TOP_DB,
};

// ===== SpectrumPad.h:1462-1488 绘制/量程常量 =====
const K_GRADIENT_MIN_ALPHA: i32 = 15;
const K_LAYER_TOP_ALPHA: i32 = 200;
const K_HOLD_LINE_ALPHA: i32 = 75;
const K_GRADIENT_STOPS: usize = 12;
const K_GRADIENT_DECAY: f32 = 3.5;
const K_SPECTRUM_BANDS: usize = 256;
const K_SPEC_FREQ_LO: f32 = 20.0;
const K_SPEC_FREQ_HI: f32 = 20_000.0;
const K_SPEC_ANCHOR_LO: f32 = 10.0;
const K_SPEC_ANCHOR_HZ: f32 = 14.142_14; // sqrt(10·20)
const K_SUB_BAND: i32 = -2;
const K_TOP_DB: f32 = 9.0;
const K_TICK_RIGHT: f32 = 3.0;
const K_LABEL_H: f32 = 16.0;

// SpectrumPad.h:1545
const K_SLOPE_REF_HZ: f32 = 632.455_53; // sqrt(20·20000)
// SpectrumPad.h:1600
const K_HUD_N: usize = 120;

// AnalyzerMeterUi / SpectrumPad 绘制杂项常量
const K_MARK_H: f32 = 19.0;
const K_GAP_BARS: f32 = 4.0;
const K_SPINE_W: f32 = 1.5;
const K_CAP_H: f32 = 1.5;
const K_TEXT_PAD: f32 = 2.0;
const K_PERSIST_LBL_H: f32 = 17.0;
const K_HZ_KHZ_GAP: f32 = 6.0;
const K_HOP: f64 = 1024.0;

/// 引擎模式（原版 EAnalyzerMode / mMode）。
pub mod mode {
    pub const STFT: i32 = 0;
    pub const VQT: i32 = 1;
    pub const PBT: i32 = 2;
    pub const RTA: i32 = 3;
}

/// 电平条点击语义（原版 EMeterClick）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeterClick {
    /// dBTP 模式：清除持久真峰值锁存
    ResetPersist,
    /// dBFS 模式：清除电平表峰值保持线
    ResetMeterHold,
    /// dBFS 模式：清除过载 LED
    ResetOver,
    /// 响度刻度窗偏移档位循环（+9 / +18 LU）
    LoudScale,
    /// 响度目标预设循环（-9 / -14 / -23 / -24）
    LoudPreset,
}

pub struct SpectrumPad {
    pub rect: Rect,

    // ===== 消息参数（OnMsgFromDelegate 等价字段） =====
    sample_rate: f64,
    num_bins: i32,
    release_sec: f32,
    release_mode: i32, // 0 = LOG 单极点, 1 = LIN dB/s
    bottom_db: f32,
    slope_db_per_oct: f32,
    mode: i32,
    chan_mode: i32, // 0 = PWR/LR 双路, 1 = 合并
    merge_algo: i32, // 0 = PWR 能量和, 1 = SUM 信号和
    meter_mode: i32, // 0 = dBTP, 1 = dBFS+RMS
    attack_sec: f32,
    attack_coeff: f32,
    release_coeff: f32,

    vqt_freqs: Vec<f32>,
    vqt_norm: Vec<f32>,
    pbt_freqs: Vec<f32>,
    pbt_norm: Vec<f32>,
    rta_freqs: Vec<f32>,
    rta_norm: Vec<f32>,
    slope_db: Vec<f32>,

    bin_to_band: Vec<i32>,
    band_used: [bool; K_SPECTRUM_BANDS],
    band_norm_x: [f32; K_SPECTRUM_BANDS],

    spectrum: [Vec<f32>; 3],

    // ===== 电平表快照（kMsgTagLevelMeter 等价） =====
    peak_l: f32,
    peak_r: f32,
    true_l: f32,
    true_r: f32,
    rms_l: f32,
    rms_r: f32,
    vu_l: f32,
    vu_r: f32,
    vu_hold_l: f32,
    vu_hold_r: f32,
    persist_l: f32,
    persist_r: f32,
    hold_l: f32,
    hold_r: f32,
    hold_sec: f32,
    over_l: bool,
    over_r: bool,

    // ===== 响度计快照（kMsgTagLoudness 等价） =====
    momentary: f32,
    short_term: f32,
    integrated: f32,
    range: f32,
    lra_min: f32,
    lra_max: f32,
    lra_valid: bool,
    i_valid: bool,
    target: f32,
    scale_off: f32,

    // ===== 峰值保持 =====
    hold_spec: [Vec<f32>; 3],
    hold_age: [Vec<f32>; 3],
    hold_signal: bool,
    hold_tp_valid: bool,
    hold_was_active: bool,
    last_hold_tp: Option<Instant>,

    // ===== hover =====
    hover_active: bool,
    hover_x: f32,
    hover_y: f32,
    lang: Lang,

    // ===== HUD（开发者性能浮层） =====
    hud_on: bool,
    hud_ms: [f32; K_HUD_N],
    hud_is_data: [u8; K_HUD_N],
    hud_t: [f64; K_HUD_N],
    hud_pos: usize,
    hud_fill: usize,
    data_pkts: u32,
}

impl Default for SpectrumPad {
    fn default() -> Self {
        Self::new(Rect::from_min_max(Pos2::new(20.0, 58.0), Pos2::new(784.0, 328.0)))
    }
}

impl SpectrumPad {
    pub fn new(rect: Rect) -> Self {
        let mut pad = Self {
            rect,
            sample_rate: 48_000.0,
            num_bins: 2048,
            release_sec: 0.2,
            release_mode: 0,
            bottom_db: -100.0,
            slope_db_per_oct: 0.0,
            mode: mode::STFT,
            chan_mode: 0,
            merge_algo: 0,
            meter_mode: 0,
            attack_sec: 0.05,
            attack_coeff: 0.2,
            release_coeff: 0.9,
            vqt_freqs: Vec::new(),
            vqt_norm: Vec::new(),
            pbt_freqs: Vec::new(),
            pbt_norm: Vec::new(),
            rta_freqs: Vec::new(),
            rta_norm: Vec::new(),
            slope_db: Vec::new(),
            bin_to_band: Vec::new(),
            band_used: [false; K_SPECTRUM_BANDS],
            band_norm_x: [0.0; K_SPECTRUM_BANDS],
            spectrum: [Vec::new(), Vec::new(), Vec::new()],
            peak_l: -120.0,
            peak_r: -120.0,
            true_l: -120.0,
            true_r: -120.0,
            rms_l: -120.0,
            rms_r: -120.0,
            vu_l: -120.0,
            vu_r: -120.0,
            vu_hold_l: -120.0,
            vu_hold_r: -120.0,
            persist_l: -120.0,
            persist_r: -120.0,
            hold_l: -1000.0,
            hold_r: -1000.0,
            hold_sec: 2.0,
            over_l: false,
            over_r: false,
            momentary: -120.0,
            short_term: -120.0,
            integrated: -120.0,
            range: 0.0,
            lra_min: -120.0,
            lra_max: -120.0,
            lra_valid: false,
            i_valid: false,
            target: -14.0,
            scale_off: 9.0,
            hold_spec: [Vec::new(), Vec::new(), Vec::new()],
            hold_age: [Vec::new(), Vec::new(), Vec::new()],
            hold_signal: false,
            hold_tp_valid: false,
            hold_was_active: false,
            last_hold_tp: None,
            hover_active: false,
            hover_x: 0.0,
            hover_y: 0.0,
            lang: Lang::En,
            hud_on: false,
            hud_ms: [0.0; K_HUD_N],
            hud_is_data: [0; K_HUD_N],
            hud_t: [0.0; K_HUD_N],
            hud_pos: 0,
            hud_fill: 0,
            data_pkts: 0,
        };
        pad.rebuild_bin_to_band();
        let log_lo = (K_SPEC_FREQ_LO as f64).log2();
        let log_hi = (K_SPEC_FREQ_HI as f64).log2();
        let log_band = (log_hi - log_lo) / K_SPECTRUM_BANDS as f64;
        for b in 0..K_SPECTRUM_BANDS {
            let f_center = K_SPEC_FREQ_LO as f64 * (log_band * (b as f64 + 0.5)).exp2();
            pad.band_norm_x[b] = freq_norm(f_center);
        }
        pad
    }

    // ==================== kMsgTag* 对应 setter ====================

    pub fn set_sample_rate(&mut self, sr: f64) {
        self.sample_rate = sr;
        self.rebuild_bin_to_band();
    }

    pub fn set_fft_size(&mut self, fft_size: i32) {
        self.num_bins = (fft_size / 2).max(1);
        self.rebuild_bin_to_band();
    }

    pub fn set_release(&mut self, release_sec: f32) {
        self.release_sec = release_sec.clamp(0.01, 10.0);
    }

    pub fn set_release_mode(&mut self, mode: i32) {
        self.release_mode = mode.clamp(0, 1);
    }

    pub fn set_range(&mut self, range_db: f32) {
        self.bottom_db = -range_db.clamp(80.0, 120.0);
    }

    pub fn set_slope(&mut self, slope_db: f32) {
        self.slope_db_per_oct = slope_db;
        self.rebuild_slope_gain();
    }

    pub fn set_mode(&mut self, mode: i32) {
        self.mode = mode.clamp(0, 3);
        self.rebuild_slope_gain();
    }

    pub fn set_chan_mode(&mut self, chan_mode: i32) {
        self.chan_mode = chan_mode.clamp(0, 1);
    }

    pub fn set_merge_algo(&mut self, merge_algo: i32) {
        self.merge_algo = merge_algo.clamp(0, 1);
    }

    pub fn set_vqt_bands(&mut self, freqs: &[f64]) {
        let changed = freqs.len() != self.vqt_freqs.len();
        self.vqt_freqs.clear();
        self.vqt_freqs.extend(freqs.iter().map(|&f| f as f32));
        self.vqt_norm = self.vqt_freqs.iter().map(|&f| freq_norm(f as f64)).collect();
        if changed {
            for s in self.spectrum.iter_mut() {
                s.clear();
            }
        }
        self.rebuild_slope_gain();
    }

    pub fn set_pbt_bands(&mut self, freqs: &[f64]) {
        let changed = freqs.len() != self.pbt_freqs.len();
        self.pbt_freqs.clear();
        self.pbt_freqs.extend(freqs.iter().map(|&f| f as f32));
        self.pbt_norm = self.pbt_freqs.iter().map(|&f| freq_norm(f as f64)).collect();
        if changed {
            for s in self.spectrum.iter_mut() {
                s.clear();
            }
        }
        self.rebuild_slope_gain();
    }

    pub fn set_rta_bands(&mut self, freqs: &[f64]) {
        let changed = freqs.len() != self.rta_freqs.len();
        self.rta_freqs.clear();
        self.rta_freqs.extend(freqs.iter().map(|&f| f as f32));
        self.rta_norm = self.rta_freqs.iter().map(|&f| freq_norm(f as f64)).collect();
        if changed {
            for s in self.spectrum.iter_mut() {
                s.clear();
            }
        }
        self.rebuild_slope_gain();
    }

    pub fn set_reset(&mut self) {
        for s in self.spectrum.iter_mut() {
            s.fill(-150.0);
        }
        self.clear_peak_hold();
    }

    /// 电平表模式（0 dBTP / 1 dBFS+RMS），随 `kLevelMode` 参数下发。
    pub fn set_meter_mode(&mut self, mode: i32) {
        self.meter_mode = mode.clamp(0, 1);
    }

    /// 响度目标 LUFS（随 `kLoudPreset` 档位下发；原版走 LoudnessUiData.target）。
    pub fn set_loud_target(&mut self, target: f32) {
        self.target = target;
    }

    /// 响度刻度窗偏移 LU（随 `kLoudScale` 档位下发）。
    pub fn set_loud_scale_off(&mut self, off: f32) {
        self.scale_off = off;
    }

    pub fn set_level_meter(&mut self, level: &LevelSnapshot) {
        self.peak_l = level.peak_db_l;
        self.peak_r = level.peak_db_r;
        self.true_l = level.true_db_l;
        self.true_r = level.true_db_r;
        self.rms_l = level.rms_db_l;
        self.rms_r = level.rms_db_r;
        self.vu_l = level.vu_db_l;
        self.vu_r = level.vu_db_r;
        self.vu_hold_l = level.vu_hold_db_l;
        self.vu_hold_r = level.vu_hold_db_r;
        self.persist_l = level.persist_db_l;
        self.persist_r = level.persist_db_r;
        self.hold_l = level.hold_db_l;
        self.hold_r = level.hold_db_r;
        self.hold_sec = level.hold_sec;
        self.over_l = level.over_l != 0;
        self.over_r = level.over_r != 0;
    }

    pub fn set_loudness(&mut self, loud: &LoudnessSnapshot) {
        self.momentary = loud.momentary;
        self.short_term = loud.short_term;
        self.integrated = loud.integrated;
        self.range = loud.range;
        self.lra_min = loud.lra_min;
        self.lra_max = loud.lra_max;
        self.lra_valid = loud.lra_valid;
        self.i_valid = loud.i_valid;
    }

    pub fn clear_peak_hold(&mut self) {
        for c in 0..3 {
            self.hold_spec[c].fill(-1000.0);
            self.hold_age[c].fill(0.0);
        }
        self.hold_signal = false;
    }

    pub fn toggle_hud(&mut self) {
        self.hud_on = !self.hud_on;
    }

    // ==================== 帧更新 ====================

    /// 等价原版 `OnMsgFromDelegate(kUpdateMessage)`：摄入一帧线性幅度数据，
    /// 做 dB 换算、非对称平滑与峰值保持。
    pub fn update(
        &mut self,
        frame: &SpectrumFrame,
        level: &LevelSnapshot,
        loud: &LoudnessSnapshot,
    ) {
        self.data_pkts += 1;
        self.set_level_meter(level);
        self.set_loudness(loud);

        // 非 FFT 模式：帧自带带中心频率时补齐带表（原版频率由独立 band 消息下发）。
        if self.mode != mode::STFT && !frame.freqs.is_empty() {
            match self.mode {
                mode::VQT if self.vqt_freqs.is_empty() => self.set_vqt_bands(&frame.freqs),
                mode::PBT if self.pbt_freqs.is_empty() => self.set_pbt_bands(&frame.freqs),
                mode::RTA if self.rta_freqs.is_empty() => self.set_rta_bands(&frame.freqs),
                _ => {}
            }
        }

        let n_vals = match self.mode {
            mode::STFT => self.num_bins.max(0) as usize,
            mode::VQT => self.vqt_freqs.len(),
            mode::PBT => self.pbt_freqs.len(),
            _ => self.rta_freqs.len(),
        };
        if n_vals == 0 {
            return;
        }

        let update_period = K_HOP / self.sample_rate.max(1.0);
        self.attack_coeff = (-update_period / self.attack_sec as f64).exp() as f32;
        self.release_coeff = (-update_period / self.release_sec as f64).exp() as f32;
        let unif_step_db = (update_period
            / (2.0 * self.release_sec.max(1e-3) as f64)
            * (K_TOP_DB - self.bottom_db) as f64) as f32;

        let a = self.attack_coeff;
        let r = self.release_coeff;

        if self.mode == mode::STFT {
            self.process_fft(frame, a, r, unif_step_db);
        } else {
            let tilt = if self.slope_db.is_empty() {
                None
            } else {
                Some(&self.slope_db[..])
            };
            for c in 0..3 {
                if self.spectrum[c].len() != n_vals {
                    self.spectrum[c].resize(n_vals, -150.0);
                }
                let vals = match c {
                    0 => &frame.l,
                    1 => &frame.r,
                    _ => &frame.m,
                };
                for i in 0..n_vals {
                    let raw = vals.get(i).copied().unwrap_or(0.0);
                    let raw_db = if raw > 1e-30 {
                        6.020_599_913_28 * fast_log2(raw)
                    } else {
                        -150.0
                    };
                    let tilt_db = tilt.map(|t| t.get(i).copied().unwrap_or(0.0)).unwrap_or(0.0);
                    let target = self.smooth_target(raw_db, tilt_db);
                    let mut a_coef = a;
                    // PBT 起振 τ = 1/(π·bw)：窄带低频自然蓄力爬坡（原版:117-125）
                    if self.mode == mode::PBT && self.pbt_freqs.len() == n_vals {
                        let fc = self.pbt_freqs[i];
                        let bw = if fc < 250.0 {
                            if i > 0 && self.pbt_freqs[i] < 250.0 {
                                fc - self.pbt_freqs[i - 1]
                            } else {
                                40.0
                            }
                        } else {
                            fc * 0.1
                        };
                        let tau_band = self.attack_sec.max(1.0 / (std::f32::consts::PI * bw.max(5.0)));
                        a_coef = (-update_period / tau_band as f64).exp() as f32;
                    }
                    self.spectrum[c][i] =
                        self.step_smoothed(self.spectrum[c][i], target, a_coef, r, unif_step_db);
                }
            }
        }

        self.update_peak_hold();
    }

    // ==================== 平滑 / 合并模型 ====================

    /// SmoothTarget（原版:1178-1181）：顶部留 10% 超调余量，夹到 [bottom-over, top]。
    fn smooth_target(&self, raw_db: f32, tilt_db: f32) -> f32 {
        let over = 0.1 * (K_TOP_DB - self.bottom_db);
        (raw_db + tilt_db).clamp(self.bottom_db - over, K_TOP_DB)
    }

    /// StepSmoothed（原版:1183-1189）：上升走 attack，下降按 release 模式。
    fn step_smoothed(
        &self,
        prev_db: f32,
        target_db: f32,
        a_coef: f32,
        r_coef: f32,
        unif_step_db: f32,
    ) -> f32 {
        Self::step_smoothed_db(prev_db, target_db, a_coef, r_coef, unif_step_db, self.release_mode)
    }

    pub fn step_smoothed_db(
        prev_db: f32,
        target_db: f32,
        a_coef: f32,
        r_coef: f32,
        unif_step_db: f32,
        release_mode: i32,
    ) -> f32 {
        if target_db > prev_db {
            a_coef * prev_db + (1.0 - a_coef) * target_db
        } else if release_mode == 1 {
            target_db.max(prev_db - unif_step_db)
        } else {
            r_coef * prev_db + (1.0 - r_coef) * target_db
        }
    }

    /// MergeDb（原版:1191-1198）：algo 0 = 总功率和，1 = 信号和。
    pub fn merge_db(d_l: f32, d_r: f32, algo: i32) -> f32 {
        if algo == 0 {
            let p = (d_l * 0.332_192_809_49).exp2() + (d_r * 0.332_192_809_49).exp2();
            3.010_299_956_64 * fast_log2(p)
        } else {
            let a = (d_l * 0.166_096_404_74).exp2() + (d_r * 0.166_096_404_74).exp2();
            6.020_599_913_28 * fast_log2(a)
        }
    }

    // ==================== STFT bin → band 聚合 ====================

    fn rebuild_bin_to_band(&mut self) {
        let nb = self.num_bins as usize;
        self.bin_to_band.clear();
        self.bin_to_band.resize(nb, -1);
        if nb == 0 {
            return;
        }
        let bin_hz = self.sample_rate / (nb as f64 * 2.0).max(1.0);
        let log_lo = (K_SPEC_FREQ_LO as f64).log2();
        let log_hi = (K_SPEC_FREQ_HI as f64).log2();
        let log_band = (log_hi - log_lo) / K_SPECTRUM_BANDS as f64;
        for i in 0..nb {
            let f = i as f64 * bin_hz;
            if f > K_SPEC_FREQ_HI as f64 {
                continue;
            }
            if f < K_SPEC_FREQ_LO as f64 {
                if f >= K_SPEC_ANCHOR_LO as f64 {
                    self.bin_to_band[i] = K_SUB_BAND;
                }
                continue;
            }
            let b = ((f.log2() - log_lo) / log_band) as i32;
            if b >= 0 && (b as usize) < K_SPECTRUM_BANDS {
                self.bin_to_band[i] = b;
            }
        }
    }

    fn rebuild_slope_gain(&mut self) {
        let n = if self.mode == mode::STFT {
            K_SPECTRUM_BANDS
        } else {
            match self.mode {
                mode::VQT => self.vqt_freqs.len(),
                mode::PBT => self.pbt_freqs.len(),
                _ => self.rta_freqs.len(),
            }
        };
        self.slope_db.clear();
        self.slope_db.resize(n, 0.0);
        if self.slope_db_per_oct == 0.0 || n == 0 {
            return;
        }
        let log_lo = (K_SPEC_FREQ_LO as f64).log2();
        let log_band = ((K_SPEC_FREQ_HI as f64).log2() - log_lo) / K_SPECTRUM_BANDS as f64;
        for b in 0..n {
            let f = if self.mode == mode::STFT {
                (K_SPEC_FREQ_LO as f64 * (log_band * (b as f64 + 0.5)).exp2()) as f32
            } else {
                match self.mode {
                    mode::VQT => self.vqt_freqs[b],
                    mode::PBT => self.pbt_freqs[b],
                    _ => self.rta_freqs[b],
                }
            };
            self.slope_db[b] = self.slope_db_per_oct * fast_log2(f / K_SLOPE_REF_HZ);
        }
    }

    fn process_fft(&mut self, frame: &SpectrumFrame, a: f32, r: f32, unif_step_db: f32) {
        if self.bin_to_band.len() != self.num_bins as usize {
            self.rebuild_bin_to_band();
        }
        let nb = self
            .num_bins
            .min(frame.l.len() as i32)
            .max(0) as usize;
        if self.spectrum[0].len() != K_SPECTRUM_BANDS {
            for s in self.spectrum.iter_mut() {
                s.resize(K_SPECTRUM_BANDS, -150.0);
            }
        }

        let mut band_max = [[0.0f32; K_SPECTRUM_BANDS]; 3];
        let mut anchor = [0.0f32; 3];
        self.band_used.fill(false);
        let mut anchor_used = false;

        let vals: [&[f32]; 3] = [&frame.l, &frame.r, &frame.m];
        for i in 0..nb {
            let b = self.bin_to_band.get(i).copied().unwrap_or(-1);
            if b == K_SUB_BAND {
                for c in 0..3 {
                    if vals[c].get(i).copied().unwrap_or(0.0) > anchor[c] {
                        anchor[c] = vals[c][i];
                    }
                }
                anchor_used = true;
                continue;
            }
            if b < 0 {
                continue;
            }
            let b = b as usize;
            self.band_used[b] = true;
            for c in 0..3 {
                let v = vals[c].get(i).copied().unwrap_or(0.0);
                if v > band_max[c][b] {
                    band_max[c][b] = v;
                }
            }
        }

        let log_lo = (K_SPEC_FREQ_LO as f64).log2();
        let log_band = ((K_SPEC_FREQ_HI as f64).log2() - log_lo) / K_SPECTRUM_BANDS as f64;
        let mut first = 0;
        while first < K_SPECTRUM_BANDS && !self.band_used[first] {
            first += 1;
        }
        if first < K_SPECTRUM_BANDS {
            let f_first = K_SPEC_FREQ_LO as f64 * (log_band * (first as f64 + 0.5)).exp2();
            for c in 0..3 {
                let a_first = band_max[c][first];
                let a_a = anchor[c];
                let have_anchor = anchor_used && a_a > 1e-9 && a_first > 1e-9;
                for b in 0..first {
                    let mut ext = a_first;
                    if have_anchor {
                        let f_b = K_SPEC_FREQ_LO as f64 * (log_band * (b as f64 + 0.5)).exp2();
                        let t = (f_b / K_SPEC_ANCHOR_HZ as f64).log2()
                            / (f_first / K_SPEC_ANCHOR_HZ as f64).log2();
                        ext = a_a * (a_first / a_a).powf(t as f32);
                    }
                    band_max[c][b] = ext;
                }
            }
            self.band_used[0] = true;
        }

        let has_tilt = !self.slope_db.is_empty();
        for c in 0..3 {
            for b in 0..K_SPECTRUM_BANDS {
                let raw_db = if band_max[c][b] > 1e-30 {
                    6.020_599_913_28 * fast_log2(band_max[c][b])
                } else {
                    -150.0
                };
                let tilt = if has_tilt { self.slope_db[b] } else { 0.0 };
                let target = self.smooth_target(raw_db, tilt);
                self.spectrum[c][b] = self.step_smoothed(self.spectrum[c][b], target, a, r, unif_step_db);
            }
        }
    }

    // ==================== 峰值保持 ====================

    fn update_peak_hold(&mut self) {
        let now = Instant::now();
        let mut dt = 0.0;
        if self.hold_tp_valid {
            if let Some(last) = self.last_hold_tp {
                dt = (now - last).as_secs_f32().clamp(0.0, 0.25);
            }
        }
        self.last_hold_tp = Some(now);
        self.hold_tp_valid = true;
        self.apply_peak_hold(dt);
    }

    /// 测试/外部可注入经过秒数的峰值保持步进（等价原版老化 + 回落公式）。
    /// 注入值不做真机帧间隔 0.25s 钳制（原版钳制只作用于实测墙钟 dt）。
    pub fn peak_hold_tick(&mut self, dt: f32) {
        self.apply_peak_hold(dt);
    }

    fn apply_peak_hold(&mut self, dt: f32) {
        let n_vals = self.spectrum[0].len();
        if self.hold_sec <= 0.0 {
            if self.hold_was_active {
                self.clear_peak_hold();
                self.hold_was_active = false;
            }
            self.hold_tp_valid = false;
            return;
        }
        self.hold_was_active = true;

        // ∞ 档（1e9）恒不回落（原版:1287）
        let fall_db = if self.hold_sec < 1e8 && dt > 0.0 {
            20.0 * dt
        } else {
            0.0
        };

        for c in 0..3 {
            if self.hold_spec[c].len() != n_vals {
                self.hold_spec[c].resize(n_vals, -1000.0);
                self.hold_age[c].resize(n_vals, 0.0);
                self.hold_signal = false;
            }
            for i in 0..n_vals {
                let cur = self.spectrum[c][i];
                let hold = &mut self.hold_spec[c][i];
                if cur > *hold {
                    *hold = cur;
                    self.hold_age[c][i] = 0.0;
                    if cur > self.bottom_db + 0.5 {
                        self.hold_signal = true;
                    }
                } else if fall_db > 0.0 {
                    self.hold_age[c][i] += dt;
                    if self.hold_age[c][i] > self.hold_sec {
                        *hold = cur.max(*hold - fall_db);
                    }
                }
            }
        }
    }

    fn hold_val_at(&self, b: usize) -> f32 {
        let h_l = self.hold_spec[0].get(b).copied().unwrap_or(-1000.0);
        let h_r = self.hold_spec[1].get(b).copied().unwrap_or(-1000.0);
        if self.chan_mode == 0 {
            h_l.max(h_r)
        } else {
            let h_sum = self.hold_spec[2].get(b).copied().unwrap_or(-1000.0);
            if self.merge_algo == 0 {
                Self::merge_db(h_l, h_r, 0)
            } else {
                h_sum
            }
        }
    }

    // ===== 测试/校验用只读访问器 =====
    pub fn attack_coeff(&self) -> f32 {
        self.attack_coeff
    }
    pub fn release_coeff(&self) -> f32 {
        self.release_coeff
    }
    pub fn spectrum_db(&self, ch: usize, i: usize) -> f32 {
        self.spectrum[ch].get(i).copied().unwrap_or(f32::NAN)
    }
    pub fn hold_db(&self, ch: usize, i: usize) -> f32 {
        self.hold_spec[ch].get(i).copied().unwrap_or(f32::NAN)
    }
    pub fn slope_at(&self, i: usize) -> f32 {
        self.slope_db.get(i).copied().unwrap_or(0.0)
    }
    pub fn hold_signal(&self) -> bool {
        self.hold_signal
    }
    pub fn release_mode(&self) -> i32 {
        self.release_mode
    }

    // ==================== 几何助手 ====================

    fn plot_rect(&self) -> Rect {
        Rect::from_min_max(
            Pos2::new(self.rect.left(), self.rect.top()),
            Pos2::new(self.rect.right() - K_METER_STRIP_W, self.rect.bottom()),
        )
    }

    fn y_of(&self, plot: Rect, db: f32) -> f32 {
        plot.bottom() - (db - self.bottom_db) / (K_TOP_DB - self.bottom_db) * plot.height()
    }

    fn x_of(&self, plot: Rect, f: f64) -> f32 {
        plot.left() + freq_norm(f) * plot.width()
    }

    fn loud_window(&self) -> (f32, f32) {
        if self.target > -100.0 {
            (self.target + self.scale_off, self.target - 2.0 * self.scale_off)
        } else {
            (0.0, -60.0)
        }
    }

    fn loud_y_of(&self, plot: Rect, lufs: f32) -> f32 {
        let (top_l, bot_l) = self.loud_window();
        let f = ((lufs - bot_l) / (top_l - bot_l)).clamp(0.0, 1.0);
        plot.bottom() - f * plot.height()
    }

    fn vu_tick_rect(&self, plot: Rect, vu: f32) -> Rect {
        let scale_l = meter::lr_zone_r(plot);
        if vu >= K_VU_TOP_DB {
            Rect::from_min_max(
                Pos2::new(scale_l, plot.top() + 1.0),
                Pos2::new(scale_l + K_VU_SCALE_W - K_TICK_RIGHT, plot.top() + 1.0 + K_LABEL_H),
            )
        } else {
            let y = plot.bottom() - (vu - K_VU_BOTTOM_VU) / (K_VU_TOP_DB - K_VU_BOTTOM_VU) * plot.height();
            let t = y.min(plot.bottom() - K_LABEL_H);
            Rect::from_min_max(
                Pos2::new(scale_l, t),
                Pos2::new(scale_l + K_VU_SCALE_W - K_TICK_RIGHT, t + K_LABEL_H),
            )
        }
    }

    fn lufs_tick_rect(&self, plot: Rect, lufs: f32) -> Rect {
        let scale_l = meter::vu_zone_r(plot);
        let (top_l, _bot_l) = self.loud_window();
        if lufs >= top_l {
            Rect::from_min_max(
                Pos2::new(scale_l, plot.top() + 1.0),
                Pos2::new(scale_l + K_LUFS_SCALE_W - K_TICK_RIGHT, plot.top() + 1.0 + K_LABEL_H),
            )
        } else {
            let y = self.loud_y_of(plot, lufs);
            let t = y.min(plot.bottom() - K_LABEL_H);
            Rect::from_min_max(
                Pos2::new(scale_l, t),
                Pos2::new(scale_l + K_LUFS_SCALE_W - K_TICK_RIGHT, t + K_LABEL_H),
            )
        }
    }

    fn loud_tick_btn_rect(label: Rect) -> Rect {
        Rect::from_min_max(
            Pos2::new(label.left(), label.top() - 1.0),
            Pos2::new(label.right() + 1.0, label.bottom() + 1.0),
        )
    }

    fn loud_scale_tick_btn_rect(&self, plot: Rect) -> Rect {
        let (top_l, _) = self.loud_window();
        Self::loud_tick_btn_rect(self.lufs_tick_rect(plot, top_l))
    }

    fn loud_preset_tick_btn_rect(&self, plot: Rect) -> Rect {
        Self::loud_tick_btn_rect(self.lufs_tick_rect(plot, self.target))
    }

    // ==================== 命中测试 ====================

    pub fn click(&mut self, pos: Pos2) -> Option<MeterClick> {
        let approx_plot = self.plot_rect();

        let scale_btn = self.loud_scale_tick_btn_rect(approx_plot);
        if scale_btn.contains(pos) {
            return Some(MeterClick::LoudScale);
        }
        if self.target > -100.0 {
            let preset_btn = self.loud_preset_tick_btn_rect(approx_plot);
            if preset_btn.contains(pos) {
                return Some(MeterClick::LoudPreset);
            }
        }

        let bar_l0 = self.rect.right() - K_METER_STRIP_W;
        let bar_r = bar_l0 + 2.0 * K_GAIN_BAR_W;
        if pos.x < bar_l0
            || pos.x > bar_r
            || pos.y < self.rect.top()
            || pos.y > self.rect.bottom()
        {
            return None;
        }
        if self.meter_mode == 0 {
            Some(MeterClick::ResetPersist)
        } else {
            let y0 = self.y_of(approx_plot, 0.0);
            if pos.y < y0 - 3.0 {
                Some(MeterClick::ResetOver)
            } else {
                Some(MeterClick::ResetMeterHold)
            }
        }
    }

    /// 记录 hover 位置并返回悬停提示文案（交互区）；十字线/读数由 `draw` 绘制。
    ///
    /// 原版 `SpectrumPad` 自身不挂任何 tooltip 绑定（`Analyzer.cpp` 的 `bindTip`
    /// 只绑右栏面板按钮）；本控件仅两条响度刻度按钮在 `Strings.h` 中有对应常量
    /// （`kTxtTipLoudScale` / `kTxtTipLoudPreset`），其余命中区原版均无外部提示：
    /// 三条电平表复位（持久真峰值 / 过载锁存 / 电平保持）在原版字符串表中不存在，
    /// 不提供自造提示；VU / LUFS 读数由控件内联 chip 绘制，不走 tooltip。
    pub fn hover(&mut self, pos: Pos2) -> Option<String> {
        let plot = self.plot_rect();
        let in_strip = pos.x >= plot.left()
            && pos.x <= self.rect.right()
            && pos.y >= plot.top()
            && pos.y <= plot.bottom();
        if !in_strip {
            self.hover_active = false;
            return None;
        }
        self.hover_active = true;
        self.hover_x = pos.x;
        self.hover_y = pos.y;

        // kTxtTipLoudScale：响度刻度窗偏移档位循环（+9 / +18 LU）
        if self.loud_scale_tick_btn_rect(plot).contains(pos) {
            return Some(strings::analyzer_label(
                "Loudness bar scale window offset (top = target + offset, 1/3 height = target, bottom = target - 2x offset): +9 / +18 LU",
                self.lang,
            ));
        }
        // kTxtTipLoudPreset：响度目标预设循环（-9 / -14 / -23 / -24）
        if self.target > -100.0 && self.loud_preset_tick_btn_rect(plot).contains(pos) {
            return Some(strings::analyzer_label(
                "Loudness target preset: -9 / -14 (streaming music) / -23 (EBU R128 broadcast) / -24",
                self.lang,
            ));
        }

        // 增益条 / VU 条 / LUFS 条命中区：原版无 tooltip（复位动作由点击 handler 完成）。
        None
    }

    // ==================== 绘制 ====================

    pub fn draw(&mut self, painter: &egui::Painter, theme: &Theme, lang: Lang) {
        self.lang = lang;
        let t0 = Instant::now();
        self.draw_content(painter, theme, lang);
        let ms = t0.elapsed().as_secs_f32() * 1000.0;
        self.perf_sample(ms);
        if self.hud_on {
            self.draw_hud(painter, theme);
        }
    }

    fn perf_sample(&mut self, ms: f32) {
        self.hud_ms[self.hud_pos] = ms;
        self.hud_is_data[self.hud_pos] = (self.data_pkts > 0) as u8;
        self.data_pkts = 0;
        self.hud_t[self.hud_pos] = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        self.hud_pos = (self.hud_pos + 1) % K_HUD_N;
        if self.hud_fill < K_HUD_N {
            self.hud_fill += 1;
        }
    }

    fn draw_content(&mut self, painter: &egui::Painter, theme: &Theme, _lang: Lang) {
        painter.rect_filled(self.rect, 0.0, theme.col_100());
        let plot = self.plot_rect();

        // hover 分区标志（与原版 DrawContent:335-343 一致）
        let zone_lr = meter::lr_zone_r(plot);
        let zone_vu = meter::vu_zone_r(plot);
        let zone_lra = plot.right() + K_METER_STRIP_W - K_LRA_ZONE_W;
        let led_area = self.meter_mode == 1
            && self.hover_x > plot.right()
            && self.hover_x <= zone_lr
            && self.hover_y < self.y_of(plot, 0.0) - 3.0;
        let in_strip = self.hover_active
            && !led_area
            && self.hover_x >= plot.left()
            && self.hover_x <= self.rect.right()
            && self.hover_y >= plot.top()
            && self.hover_y <= self.rect.bottom();
        let in_spectrum = in_strip && self.hover_x <= plot.right();
        let in_zone0 = in_strip && self.hover_x <= zone_lr;
        let in_zone1 = in_strip && self.hover_x > zone_lr + K_VU_SCALE_W && self.hover_x <= zone_vu;
        let in_zone2 = in_strip && self.hover_x > zone_vu + K_LUFS_SCALE_W && self.hover_x <= zone_lra;

        // ---- 先算 chip 避让矩形（原版 DrawContent:353-433），静态刻度据此跳过绘制 ----
        let mut hz_skip = Rect::ZERO;
        let mut db_skip = Rect::ZERO;
        let mut vu_skip = Rect::ZERO;
        let mut lufs_skip = Rect::ZERO;

        let mut note = String::new();
        let mut hz_txt = String::new();
        let mut pitch_r = Rect::ZERO;
        let mut freq_r = Rect::ZERO;
        if in_zone0 && in_spectrum {
            let hz: f64 = 20.0 * 1000.0_f64.powf(((self.hover_x - plot.left()) / plot.width()) as f64);
            note = freq_to_note_name(hz);
            hz_txt = if hz < 1000.0 {
                format!("{:.0} Hz", hz)
            } else {
                format!("{:.2} kHz", hz / 1000.0)
            };
            let (pr, fr) = Self::layout_hover_freq_labels(
                painter, plot, self.hover_x, &note, &hz_txt, theme,
            );
            pitch_r = pr;
            freq_r = fr;
            hz_skip = pr.union(fr);
        }
        let mut db_t = self.hover_y + 1.0;
        if in_zone0 {
            // 原版 :428-430：贴底时收敛进画区
            if db_t + K_LABEL_H > plot.bottom() {
                db_t = plot.bottom() - K_LABEL_H;
            }
            if db_t < plot.top() {
                db_t = plot.top();
            }
            db_skip = Rect::from_min_max(
                Pos2::new(plot.right() - 52.0, db_t),
                Pos2::new(plot.right() - K_TICK_RIGHT, db_t + K_LABEL_H),
            );
        }
        let mut vu_box = Rect::ZERO;
        let mut lufs_box = Rect::ZERO;
        if in_zone1 {
            let mut vu_t = self.hover_y - K_LABEL_H * 0.5;
            if vu_t + K_LABEL_H > plot.bottom() {
                vu_t = plot.bottom() - K_LABEL_H;
            }
            if vu_t < plot.top() {
                vu_t = plot.top();
            }
            vu_box = Rect::from_min_max(
                Pos2::new(zone_lr, vu_t),
                Pos2::new(zone_lr + K_VU_SCALE_W - K_TICK_RIGHT, vu_t + K_LABEL_H),
            );
            vu_skip = vu_box.expand(2.0);
        } else if in_zone2 {
            let mut lufs_t = self.hover_y - K_LABEL_H * 0.5;
            if lufs_t + K_LABEL_H > plot.bottom() {
                lufs_t = plot.bottom() - K_LABEL_H;
            }
            if lufs_t < plot.top() {
                lufs_t = plot.top();
            }
            lufs_box = Rect::from_min_max(
                Pos2::new(zone_vu, lufs_t),
                Pos2::new(zone_vu + K_LUFS_SCALE_W - K_TICK_RIGHT, lufs_t + K_LABEL_H),
            );
            lufs_skip = lufs_box.expand(2.0);
        }

        self.draw_background(painter, plot, theme);
        self.draw_db_grid(painter, plot, theme, db_skip);
        self.draw_freq_grid(painter, plot, theme, hz_skip);
        self.draw_spectrum(painter, plot, theme);
        self.draw_level_meter(painter, plot, theme, vu_skip, lufs_skip);
        self.draw_lra_bracket(painter, plot, theme);

        // ---- 十字线 + chip 读数（盖在静态刻度之上，原版 :435-500） ----
        if in_zone0 && in_spectrum {
            painter.line_segment(
                [Pos2::new(self.hover_x, plot.top()), Pos2::new(self.hover_x, plot.bottom())],
                Stroke::new(1.0_f32, theme.col_700()),
            );
            painter.text(
                pitch_r.left_center(),
                Align2::LEFT_CENTER,
                &note,
                fonts::regular(14.0),
                theme.col_700(),
            );
            painter.text(
                freq_r.left_center(),
                Align2::LEFT_CENTER,
                &hz_txt,
                fonts::regular(14.0),
                theme.col_700(),
            );
        }
        if in_zone0 {
            painter.line_segment(
                [Pos2::new(plot.left(), self.hover_y), Pos2::new(zone_lr, self.hover_y)],
                Stroke::new(1.0_f32, theme.col_700()),
            );
            let db = self.bottom_db
                + (K_TOP_DB - self.bottom_db) * (plot.bottom() - self.hover_y) / plot.height();
            painter.text(
                Pos2::new(plot.right() - K_TICK_RIGHT, db_t),
                Align2::RIGHT_TOP,
                format!("{:.1} dB", db),
                fonts::regular(14.0),
                theme.col_700(),
            );
        } else if in_zone1 {
            painter.line_segment(
                [Pos2::new(zone_lr + K_VU_SCALE_W, self.hover_y), Pos2::new(zone_vu, self.hover_y)],
                Stroke::new(1.0_f32, theme.col_700()),
            );
            let vu = K_VU_BOTTOM_VU
                + (plot.bottom() - self.hover_y) / plot.height() * (K_VU_TOP_DB - K_VU_BOTTOM_VU);
            painter.text(
                vu_box.right_center(),
                Align2::RIGHT_CENTER,
                format!("{:+.1}", vu),
                fonts::regular(14.0),
                theme.col_700(),
            );
        } else if in_zone2 {
            painter.line_segment(
                [Pos2::new(zone_vu + K_LUFS_SCALE_W, self.hover_y), Pos2::new(zone_lra, self.hover_y)],
                Stroke::new(1.0_f32, theme.col_700()),
            );
            let (top_l, bot_l) = self.loud_window();
            let lufs = bot_l + (plot.bottom() - self.hover_y) / plot.height() * (top_l - bot_l);
            painter.text(
                lufs_box.right_center(),
                Align2::RIGHT_CENTER,
                format!("{:+.1}", lufs),
                fonts::regular(14.0),
                theme.col_700(),
            );
        }
    }

    /// 布局音名 / 频率 chip 矩形（不绘制），返回 (pitchR, freqR)；原版 :401-421。
    fn layout_hover_freq_labels(
        painter: &egui::Painter,
        plot: Rect,
        x_line: f32,
        note: &str,
        hz: &str,
        theme: &Theme,
    ) -> (Rect, Rect) {
        let font = fonts::regular(14.0);
        let w_p = painter.layout_no_wrap(note.to_owned(), font.clone(), theme.col_700()).size().x;
        let w_f = painter.layout_no_wrap(hz.to_owned(), font.clone(), theme.col_700()).size().x;
        let pitch_fits_left = x_line - 5.0 - w_p >= plot.left();
        let freq_fits_right = x_line + 5.0 + w_f <= plot.right();
        if pitch_fits_left && freq_fits_right {
            (
                Rect::from_min_max(
                    Pos2::new(x_line - 5.0 - w_p, plot.top() + 2.0),
                    Pos2::new(x_line - 5.0, plot.top() + 2.0 + K_LABEL_H),
                ),
                Rect::from_min_max(
                    Pos2::new(x_line + 5.0, plot.top() + 2.0),
                    Pos2::new(x_line + 5.0 + w_f, plot.top() + 2.0 + K_LABEL_H),
                ),
            )
        } else if !pitch_fits_left {
            let freq = Rect::from_min_max(
                Pos2::new(x_line + 5.0, plot.top() + 2.0),
                Pos2::new(x_line + 5.0 + w_f, plot.top() + 2.0 + K_LABEL_H),
            );
            let pitch = Rect::from_min_max(
                Pos2::new(freq.right() + K_HZ_KHZ_GAP, plot.top() + 2.0),
                Pos2::new(freq.right() + K_HZ_KHZ_GAP + w_p, plot.top() + 2.0 + K_LABEL_H),
            );
            (pitch, freq)
        } else {
            let pitch = Rect::from_min_max(
                Pos2::new(x_line - 5.0 - w_f - K_HZ_KHZ_GAP - w_p, plot.top() + 2.0),
                Pos2::new(x_line - 5.0 - w_f - K_HZ_KHZ_GAP, plot.top() + 2.0 + K_LABEL_H),
            );
            let freq = Rect::from_min_max(
                Pos2::new(x_line - 5.0 - w_f, plot.top() + 2.0),
                Pos2::new(x_line - 5.0, plot.top() + 2.0 + K_LABEL_H),
            );
            (pitch, freq)
        }
    }

    // ----- 背景网格 -----

    fn draw_background(&self, painter: &egui::Painter, plot: Rect, theme: &Theme) {
        if self.bottom_db >= 0.0 {
            return;
        }
        let edge = plot.shrink(1.0);
        let bands: [(f64, f64, f32, f32); 4] = [
            (20.0, 100.0, 194.0, 218.0),
            (100.0, 1000.0, 205.0, 229.0),
            (1000.0, 10_000.0, 216.0, 240.0),
            (10_000.0, 20_000.0, 227.0, 227.0),
        ];
        struct FreqCell {
            x_l: f32,
            x_r: f32,
            v_freq: f32,
        }
        let mut cells: Vec<FreqCell> = Vec::with_capacity(32);
        for &(lo, hi, v0, v1) in &bands {
            let mut edges = vec![lo];
            let mut decade = 1.0_f64;
            while decade <= 10_000.0 {
                for m in 1..=9 {
                    let f = m as f64 * decade;
                    if f > lo && f < hi {
                        edges.push(f);
                    }
                }
                decade *= 10.0;
            }
            edges.push(hi);
            let xs: Vec<f32> = edges.iter().map(|&f| self.x_of(plot, f)).collect();
            let n = xs.len().saturating_sub(1);
            for i in 0..n {
                let t = if n > 1 { i as f32 / (n - 1) as f32 } else { 0.0 };
                cells.push(FreqCell {
                    x_l: xs[i],
                    x_r: xs[i + 1],
                    v_freq: v0 + (v1 - v0) * t,
                });
            }
        }

        let mut db_bounds = vec![K_TOP_DB];
        let mut db = 0.0_f32;
        while db > self.bottom_db {
            db_bounds.push(db);
            db -= 20.0;
        }
        db_bounds.push(self.bottom_db);

        let db_to_y = |db: f32| -> f32 {
            plot.bottom() - (db - self.bottom_db) / (K_TOP_DB - self.bottom_db) * plot.height()
        };
        const K_V_DB_TOP: f32 = 245.0;
        const K_V_DB_BOTTOM: f32 = 172.0;
        let v_db_at = |db: f32| -> f32 {
            let t = (K_TOP_DB - db) / (K_TOP_DB - self.bottom_db);
            K_V_DB_TOP + (K_V_DB_BOTTOM - K_V_DB_TOP) * t
        };

        let n_rows = db_bounds.len();
        for r in 0..n_rows.saturating_sub(1) {
            let y_high = db_to_y(db_bounds[r]);
            let y_low = if r + 2 < n_rows {
                db_to_y(db_bounds[r + 1]) + 1.0
            } else {
                edge.bottom()
            };
            let v_db = v_db_at((db_bounds[r] + db_bounds[r + 1]) * 0.5);
            for (i, fc) in cells.iter().enumerate() {
                let x_r = if i + 1 < cells.len() { fc.x_r + 1.0 } else { edge.right() };
                let v = ((fc.v_freq + v_db) * 0.5).round() as u8;
                painter.rect_filled(
                    Rect::from_min_max(Pos2::new(fc.x_l, y_high), Pos2::new(x_r, y_low)),
                    0.0,
                    meter::grid_gray(theme, v),
                );
            }
        }
    }

    fn draw_db_grid(&self, painter: &egui::Painter, plot: Rect, theme: &Theme, skip: Rect) {
        if self.bottom_db >= 0.0 {
            return;
        }
        let bottom = self.bottom_db as i32;
        let font = fonts::regular(14.0);
        let col = theme.col_700();
        let mut db = 0_i32;
        while db >= bottom {
            if db != bottom {
                let y = plot.bottom()
                    - (db - bottom) as f32 / (K_TOP_DB - self.bottom_db) * plot.height();
                let label = Rect::from_min_max(
                    Pos2::new(plot.right() - 52.0, y + 1.0),
                    Pos2::new(plot.right() - K_TICK_RIGHT, y + 1.0 + K_LABEL_H),
                );
                if skip.width() <= 0.0 && skip.height() <= 0.0 || !label.intersects(skip) {
                    painter.text(
                        label.right_center(),
                        Align2::RIGHT_CENTER,
                        format!("{db}"),
                        font.clone(),
                        col,
                    );
                }
            }
            db -= 20;
        }
    }

    fn draw_freq_grid(&self, painter: &egui::Painter, plot: Rect, theme: &Theme, skip: Rect) {
        let labels: [(f64, &str); 4] = [
            (20.0, "20Hz"),
            (100.0, "100Hz"),
            (1000.0, "1kHz"),
            (10_000.0, "10kHz"),
        ];
        let font = fonts::regular(14.0);
        let col = theme.col_700();
        for &(hz, txt) in &labels {
            let x = self.x_of(plot, hz);
            let label = Rect::from_min_max(
                Pos2::new(x + 5.0, plot.top() + 2.0),
                Pos2::new(plot.right(), plot.top() + 2.0 + K_LABEL_H),
            );
            if !rect_empty(skip) && label.intersects(skip) {
                continue;
            }
            painter.text(label.left_center(), Align2::LEFT_CENTER, txt, font.clone(), col);
        }
    }

    // ----- 频谱曲线 -----

    fn draw_spectrum(&mut self, painter: &egui::Painter, plot: Rect, theme: &Theme) {
        if self.spectrum[0].is_empty() || self.spectrum[1].is_empty() {
            return;
        }
        if plot.width() <= 0.0 || plot.height() <= 0.0 {
            return;
        }
        let (c_l, c_r, c_m) = theme.channel_colors();

        let nb = match self.mode {
            mode::VQT => self.vqt_freqs.len(),
            mode::PBT => self.pbt_freqs.len(),
            mode::RTA => self.rta_freqs.len(),
            _ => K_SPECTRUM_BANDS,
        };
        let smooth = self.mode != mode::PBT;

        let mut pts_l: Vec<Pos2> = Vec::new();
        let mut pts_r: Vec<Pos2> = Vec::new();
        let mut pts_m: Vec<Pos2> = Vec::new();
        let mut pts_h: Vec<Pos2> = Vec::new();

        let have = nb.min(self.spectrum[0].len());
        let mut edge_padded = false;
        let db_to_y = |db: f32| -> f32 {
            plot.bottom() - (db - self.bottom_db) / (K_TOP_DB - self.bottom_db) * plot.height()
        };

        for b in 0..have {
            if self.mode == mode::STFT && !self.band_used[b] {
                continue;
            }
            let x = match self.mode {
                mode::VQT => plot.left() + self.vqt_norm[b] * plot.width(),
                mode::PBT => plot.left() + self.pbt_norm[b] * plot.width(),
                mode::RTA => plot.left() + self.rta_norm[b] * plot.width(),
                _ => plot.left() + self.band_norm_x[b] * plot.width(),
            };
            let d_l = self.spectrum[0][b];
            let d_r = self.spectrum[1][b];
            let d_sum = self.spectrum[2].get(b).copied().unwrap_or(self.bottom_db);
            let d_m = if self.merge_algo == 0 {
                Self::merge_db(d_l, d_r, 0)
            } else {
                d_sum
            };
            let y_l = db_to_y(d_l);
            let y_r = db_to_y(d_r);
            let y_m = db_to_y(d_m);
            let y_h = db_to_y(self.hold_val_at(b));

            if !edge_padded && self.mode != mode::STFT && x > plot.left() + 0.5 {
                edge_padded = true;
                pts_l.push(Pos2::new(plot.left(), y_l));
                pts_r.push(Pos2::new(plot.left(), y_r));
                pts_m.push(Pos2::new(plot.left(), y_m));
                pts_h.push(Pos2::new(plot.left(), y_h));
            }
            pts_l.push(Pos2::new(x, y_l));
            pts_r.push(Pos2::new(x, y_r));
            pts_m.push(Pos2::new(x, y_m));
            pts_h.push(Pos2::new(x, y_h));
        }

        let all_below = |pts: &[Pos2]| pts.iter().all(|p| p.y >= plot.bottom());

        if self.chan_mode == 0 {
            if !all_below(&pts_l) {
                self.fill_curve(painter, plot, &pts_l, c_l, K_GRADIENT_MIN_ALPHA, K_LAYER_TOP_ALPHA, smooth, false);
            }
            if !all_below(&pts_r) {
                self.fill_curve(painter, plot, &pts_r, c_r, K_GRADIENT_MIN_ALPHA, K_LAYER_TOP_ALPHA, smooth, true);
            }
        } else if !all_below(&pts_m) {
            self.fill_curve(painter, plot, &pts_m, c_m, K_GRADIENT_MIN_ALPHA, 255, smooth, false);
        }

        if !all_below(&pts_h) {
            self.draw_hold_curve(painter, plot, &pts_h, smooth, theme);
        }
    }

    fn sample_catmull(pts: &[Pos2], smooth: bool) -> Vec<Pos2> {
        if pts.len() < 2 {
            return pts.to_vec();
        }
        let mut out = Vec::with_capacity(pts.len() * 4);
        if !smooth || pts.len() <= 3 {
            return pts.to_vec();
        }
        let s = 0.6_f32;
        let n = pts.len();
        for i in 0..n - 1 {
            let p0 = pts[i.saturating_sub(1)];
            let p1 = pts[i];
            let p2 = pts[i + 1];
            let p3 = pts[(i + 2).min(n - 1)];
            let c1 = p1 + (p2 - p0) * (s / 6.0);
            let c2 = p2 - (p3 - p1) * (s / 6.0);
            out.push(p1);
            for k in 1..4 {
                let t = k as f32 / 4.0;
                let inv = 1.0 - t;
                let w0 = inv * inv * inv;
                let w1 = 3.0 * inv * inv * t;
                let w2 = 3.0 * inv * t * t;
                let w3 = t * t * t;
                out.push(Pos2::new(
                    p1.x * w0 + c1.x * w1 + c2.x * w2 + p2.x * w3,
                    p1.y * w0 + c1.y * w1 + c2.y * w2 + p2.y * w3,
                ));
            }
        }
        out.push(pts[n - 1]);
        out
    }

    fn fill_curve(
        &self,
        painter: &egui::Painter,
        plot: Rect,
        pts: &[Pos2],
        color: Color32,
        min_alpha: i32,
        top_alpha: i32,
        smooth: bool,
        complementary: bool,
    ) {
        if pts.len() < 2 {
            return;
        }
        let mut curve = Self::sample_catmull(pts, smooth);
        if let Some(last) = curve.last_mut() {
            if last.x >= plot.right() - plot.width() * 0.02 {
                last.x = plot.right();
            }
        }

        let top_y = curve.iter().map(|p| p.y).fold(plot.bottom(), f32::min);
        let alpha_at = |y: f32| -> u8 {
            let t = ((y - top_y) / (plot.bottom() - top_y).max(1.0)).clamp(0.0, 1.0);
            Self::gradient_alpha(t, min_alpha, top_alpha, complementary)
        };
        // 底缘取 t=1 档的表值（原版渐变在 plot.B 处仍有 exp(-3.5) 的残留 alpha，
        // 不是纯色 minAlpha）。
        let base_alpha = Self::gradient_alpha(1.0, min_alpha, top_alpha, complementary);

        let mut mesh = egui::Mesh::default();
        for w in curve.windows(2) {
            let (a, b) = (w[0], w[1]);
            if (b.x - a.x).abs() < 1e-4 {
                continue;
            }
            let ca = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha_at(a.y));
            let cb = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha_at(b.y));
            let ba = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), base_alpha);
            let bb = ba;
            let ia = mesh.vertices.len() as u32;
            mesh.vertices.push(egui::epaint::Vertex { pos: a, uv: egui::epaint::WHITE_UV, color: ca });
            mesh.vertices.push(egui::epaint::Vertex { pos: Pos2::new(a.x, plot.bottom()), uv: egui::epaint::WHITE_UV, color: ba });
            mesh.vertices.push(egui::epaint::Vertex { pos: b, uv: egui::epaint::WHITE_UV, color: cb });
            mesh.vertices.push(egui::epaint::Vertex { pos: Pos2::new(b.x, plot.bottom()), uv: egui::epaint::WHITE_UV, color: bb });
            mesh.indices.extend_from_slice(&[ia, ia + 1, ia + 2, ia + 1, ia + 3, ia + 2]);
        }
        painter.add(Shape::mesh(mesh));
    }

    /// 12-stop 纵向渐变 alpha 表（原版 `GradW()`:1467-1477 + `DrawFill`:1444-1449）。
    ///
    /// stop i 的权重 `w[i] = exp(-3.5 · i/11)`；相邻 stop 之间线性插值
    /// （等价 NanoVG `AddStop` 的段间线性过渡），再做 min→top 拉伸与互补曲线。
    fn gradient_alpha(t: f32, min_alpha: i32, top_alpha: i32, complementary: bool) -> u8 {
        let t = t.clamp(0.0, 1.0);
        let n = K_GRADIENT_STOPS;
        let x = t * (n - 1) as f32;
        let i = (x as usize).min(n - 2);
        let f = x - i as f32;
        let w0 = (-K_GRADIENT_DECAY * i as f32 / (n - 1) as f32).exp();
        let w1 = (-K_GRADIENT_DECAY * (i + 1) as f32 / (n - 1) as f32).exp();
        let w = w0 + (w1 - w0) * f;
        let mut a1 = min_alpha as f32 + (top_alpha - min_alpha) as f32 * w;
        a1 = a1.clamp(0.0, 255.0);
        let a = if complementary { 255.0 * a1 / (255.0 + a1) } else { a1 };
        a.round() as u8
    }

    fn draw_hold_curve(
        &self,
        painter: &egui::Painter,
        _plot: Rect,
        pts: &[Pos2],
        smooth: bool,
        theme: &Theme,
    ) {
        if self.hold_sec <= 0.0 || !self.hold_signal || pts.len() < 2 {
            return;
        }
        let curve = Self::sample_catmull(pts, smooth);
        let col = Color32::from_rgba_unmultiplied(
            theme.col_900().r(),
            theme.col_900().g(),
            theme.col_900().b(),
            K_HOLD_LINE_ALPHA as u8,
        );
        painter.line(curve, Stroke::new(1.0_f32, col));
    }

    // ----- 电平 / 响度条 -----

    #[allow(clippy::too_many_arguments)]
    fn draw_level_meter(
        &self,
        painter: &egui::Painter,
        plot: Rect,
        theme: &Theme,
        vu_skip: Rect,
        lufs_skip: Rect,
    ) {
        let (c_l, c_r, _) = theme.channel_colors();
        let bar_l = Rect::from_min_max(
            Pos2::new(plot.right(), plot.top()),
            Pos2::new(plot.right() + K_GAIN_BAR_W, plot.bottom()),
        );
        let bar_r = Rect::from_min_max(
            Pos2::new(bar_l.right(), plot.top()),
            Pos2::new(bar_l.right() + K_GAIN_BAR_W, plot.bottom()),
        );
        self.draw_meter_bar(painter, plot, bar_l, c_l, 0, theme);
        self.draw_meter_bar(painter, plot, bar_r, c_r, 1, theme);
        self.draw_vu_scale(painter, plot, theme, vu_skip);
        self.draw_vu_bar(painter, plot, theme);
        self.draw_loud_bars(painter, plot, theme, lufs_skip);
    }

    fn meter_top_db(&self) -> f32 {
        if self.meter_mode == 1 {
            0.0
        } else {
            K_TOP_DB
        }
    }

    fn draw_meter_bar(
        &self,
        painter: &egui::Painter,
        plot: Rect,
        bar: Rect,
        _chan: Color32,
        ch: usize,
        theme: &Theme,
    ) {
        let top = self.meter_top_db();
        let (val, rms_val, hold, over) = if self.meter_mode == 0 {
            let v = if ch == 0 { self.true_l } else { self.true_r };
            let h = if ch == 0 { self.hold_l } else { self.hold_r };
            let o = if ch == 0 { self.over_l } else { self.over_r };
            (v, -999.0, h, o)
        } else {
            let v = if ch == 0 { self.peak_l } else { self.peak_r };
            let r = if ch == 0 { self.rms_l } else { self.rms_r };
            let h = if ch == 0 { self.hold_l } else { self.hold_r };
            let o = if ch == 0 { self.over_l } else { self.over_r };
            (v, r, h, o)
        };

        painter.rect_filled(bar, 0.0, theme.col_300());

        let dark = theme.mode == ThemeMode::Dark;
        let stops = meter::meter_stops(dark, K_TOP_DB, self.bottom_db);
        let c_scale = meter::meter_chroma_scale(theme);
        let t_of = |db: f32| ((K_TOP_DB - db) / (K_TOP_DB - self.bottom_db)).clamp(0.0, 1.0);

        let fill_grad = |painter: &egui::Painter, y_top: f32, alpha: u8| {
            if y_top >= bar.bottom() {
                return;
            }
            for i in 0..stops.len() - 1 {
                let y_a = plot.top() + stops[i].t * plot.height();
                let y_b = plot.top() + stops[i + 1].t * plot.height();
                if y_b <= y_top {
                    continue;
                }
                let r_t = (y_a - 1.0).max(y_top);
                let r_b = (y_b + 1.0).min(bar.bottom());
                if r_b - r_t <= 0.0 {
                    continue;
                }
                let mid_t = (stops[i].t + stops[i + 1].t) * 0.5;
                let col = meter::stops_color(&stops, mid_t, c_scale, alpha);
                painter.rect_filled(
                    Rect::from_min_max(Pos2::new(bar.left(), r_t), Pos2::new(bar.right(), r_b)),
                    0.0,
                    col,
                );
            }
        };

        if self.meter_mode == 1 {
            let y_peak = self.y_of(plot, val);
            let y_rms = self.y_of(plot, rms_val.clamp(self.bottom_db, K_TOP_DB));
            fill_grad(painter, y_peak, 90);
            fill_grad(painter, y_rms, 245);
        } else {
            fill_grad(painter, self.y_of(plot, val), 255);
        }

        if self.hold_sec > 0.0 && hold > self.bottom_db {
            let y_h = self.y_of(plot, hold.clamp(self.bottom_db, top));
            let mut hc = meter::stops_color(&stops, t_of(hold), c_scale, 255);
            if self.meter_mode == 1 {
                let bg = theme.col_300();
                let a = 90.0 / 255.0;
                hc = Color32::from_rgb(
                    (hc.r() as f32 * a + bg.r() as f32 * (1.0 - a) + 0.5).round() as u8,
                    (hc.g() as f32 * a + bg.g() as f32 * (1.0 - a) + 0.5).round() as u8,
                    (hc.b() as f32 * a + bg.b() as f32 * (1.0 - a) + 0.5).round() as u8,
                );
            }
            painter.rect_filled(
                Rect::from_min_max(Pos2::new(bar.left(), y_h - 1.0), Pos2::new(bar.right(), y_h + 1.0)),
                0.0,
                hc,
            );
        }

        // dBTP 持久锁存只在 L 条绘制一次（原版:1107-1121）
        if ch == 0 && self.meter_mode == 0 && self.hold_sec > 0.0 {
            let persist_max = self.persist_l.max(self.persist_r);
            if persist_max > 0.0 && persist_max > self.bottom_db {
                let y_p = self.y_of(plot, persist_max.clamp(self.bottom_db, top));
                let pbuf = if persist_max.abs() >= 10.0 {
                    format!("{}", persist_max.round() as i32)
                } else {
                    format!("{:.1}", persist_max)
                };
                let pt_col = meter::stops_color(&stops, t_of(K_TOP_DB), c_scale, 255);
                let pr_t = (y_p - K_PERSIST_LBL_H * 0.5).max(plot.top());
                let pr = Rect::from_min_max(
                    Pos2::new(bar.left(), pr_t),
                    Pos2::new(bar.right() + K_GAIN_BAR_W, pr_t + K_PERSIST_LBL_H),
                );
                painter.text(
                    pr.center(),
                    Align2::CENTER_CENTER,
                    pbuf,
                    fonts::semibold(14.0),
                    pt_col,
                );
            }
        }

        if self.meter_mode == 1 {
            let y0 = self.y_of(plot, 0.0);
            const K_GAP: f32 = 3.0;
            let led_r = Rect::from_min_max(
                Pos2::new(bar.left(), plot.top()),
                Pos2::new(bar.right(), y0 - K_GAP),
            );
            painter.rect_filled(
                Rect::from_min_max(Pos2::new(bar.left(), plot.top()), Pos2::new(bar.right(), y0)),
                0.0,
                theme.col_100(),
            );
            painter.rect_filled(led_r, 0.0, theme.col_300());
            if over && y0 - K_GAP > plot.top() + 1.0 {
                painter.rect_filled(led_r, 0.0, meter::meter_over_led(theme));
            }
        }
    }

    fn draw_vu_scale(&self, painter: &egui::Painter, plot: Rect, theme: &Theme, skip: Rect) {
        let ticks: [(f32, &str); 4] = [(-20.0, "-20"), (-10.0, "-10"), (0.0, "0"), (3.0, "+3")];
        let font = fonts::regular(14.0);
        for &(vu, txt) in &ticks {
            let label = self.vu_tick_rect(plot, vu);
            if !rect_empty(skip) && label.intersects(skip) {
                continue;
            }
            painter.text(label.right_center(), Align2::RIGHT_CENTER, txt, font.clone(), theme.col_700());
        }
    }

    fn draw_vu_bar(&self, painter: &egui::Painter, plot: Rect, theme: &Theme) {
        let scale_l = plot.right() + 2.0 * K_GAIN_BAR_W;
        let bar_l = Rect::from_min_max(
            Pos2::new(scale_l + K_VU_SCALE_W, plot.top()),
            Pos2::new(scale_l + K_VU_SCALE_W + K_VU_BAR_W, plot.bottom()),
        );
        let bar_r = Rect::from_min_max(
            Pos2::new(bar_l.right(), plot.top()),
            Pos2::new(bar_l.right() + K_VU_BAR_W, plot.bottom()),
        );

        let t_of_v = |vu: f32| ((K_VU_TOP_DB - vu) / (K_VU_TOP_DB - K_VU_BOTTOM_VU)).clamp(0.0, 1.0);
        let seam = t_of_v(-3.0);
        let dark = theme.mode == ThemeMode::Dark;
        let stops = meter::vu_stops(dark, seam);
        let c_scale = meter::meter_chroma_scale(theme);

        let y_of_v = |vu: f32, bar: Rect| -> f32 {
            bar.bottom() - (vu - K_VU_BOTTOM_VU) / (K_VU_TOP_DB - K_VU_BOTTOM_VU) * bar.height()
        };

        let draw_bar = |painter: &egui::Painter, bar: Rect, vu_db: f32, vu_hold: f32| {
            painter.rect_filled(bar, 0.0, theme.col_300());
            let vu = vu_db + 18.0;
            let y_top = y_of_v(vu.clamp(K_VU_BOTTOM_VU, K_VU_TOP_DB), bar);
            if y_top < bar.bottom() {
                for i in 0..stops.len() - 1 {
                    let y_a = plot.top() + stops[i].t * plot.height();
                    let y_b = plot.top() + stops[i + 1].t * plot.height();
                    if y_b <= y_top {
                        continue;
                    }
                    let r_t = (y_a - 1.0).max(y_top);
                    let r_b = (y_b + 1.0).min(bar.bottom());
                    if r_b - r_t <= 0.0 {
                        continue;
                    }
                    let mid_t = (stops[i].t + stops[i + 1].t) * 0.5;
                    painter.rect_filled(
                        Rect::from_min_max(Pos2::new(bar.left(), r_t), Pos2::new(bar.right(), r_b)),
                        0.0,
                        meter::stops_color(&stops, mid_t, c_scale, 255),
                    );
                }
            }
            if self.hold_sec > 0.0 && vu_hold > -900.0 {
                let vh = vu_hold + 18.0;
                if vh > K_VU_BOTTOM_VU {
                    let y_h = y_of_v(vh.clamp(K_VU_BOTTOM_VU, K_VU_TOP_DB), bar);
                    painter.rect_filled(
                        Rect::from_min_max(Pos2::new(bar.left(), y_h - 1.0), Pos2::new(bar.right(), y_h + 1.0)),
                        0.0,
                        meter::stops_color(&stops, t_of_v(vh), c_scale, 255),
                    );
                }
            }
        };

        draw_bar(painter, bar_l, self.vu_l, self.vu_hold_l);
        draw_bar(painter, bar_r, self.vu_r, self.vu_hold_r);

        let vu_max = self.vu_l.max(self.vu_r);
        let lbl_col = if vu_max + 18.0 > K_VU_BOTTOM_VU {
            theme.col_300()
        } else {
            theme.col_700()
        };
        let vu_label = Rect::from_min_max(
            Pos2::new(bar_l.left(), plot.bottom() - 19.0),
            Pos2::new(bar_r.right(), plot.bottom()),
        );
        painter.text(
            vu_label.center(),
            Align2::CENTER_CENTER,
            "VU",
            fonts::semibold(11.0),
            lbl_col,
        );
    }

    fn draw_loud_bars(&self, painter: &egui::Painter, plot: Rect, theme: &Theme, skip: Rect) {
        let scale_l = meter::vu_zone_r(plot);
        let bar0_l = scale_l + K_LUFS_SCALE_W;
        let bar_i = Rect::from_min_max(
            Pos2::new(bar0_l, plot.top()),
            Pos2::new(bar0_l + 2.0 * K_LOUD_BAR_W, plot.bottom()),
        );
        let bar_s = Rect::from_min_max(
            Pos2::new(bar_i.right(), plot.top()),
            Pos2::new(bar_i.right() + K_LOUD_BAR_W, plot.bottom()),
        );
        let bar_m = Rect::from_min_max(
            Pos2::new(bar_s.right(), plot.top()),
            Pos2::new(bar_s.right() + K_LOUD_BAR_W, plot.bottom()),
        );

        let (top_l, bot_l) = self.loud_window();
        let tgt_valid = self.target > -100.0;
        let dark = theme.mode == ThemeMode::Dark;
        let stops = meter::loud_stops(dark);
        let c_scale = meter::meter_chroma_scale(theme);

        let draw_tick_text = |painter: &egui::Painter, v: f32, label: Rect| {
            painter.text(
                label.right_center(),
                Align2::RIGHT_CENTER,
                format!("{}", v.round() as i32),
                fonts::regular(14.0),
                theme.col_700(),
            );
        };
        let draw_tick_btn = |painter: &egui::Painter, v: f32, label: Rect| {
            let btn = Self::loud_tick_btn_rect(label);
            if self.hover_active && btn.contains(Pos2::new(self.hover_x, self.hover_y)) {
                painter.rect_filled(btn, 0.0, theme.hover_overlay());
            }
            draw_tick_text(painter, v, label);
        };

        let hidden_by_chip = |label: Rect| !rect_empty(skip) && label.intersects(skip);
        let top_label = self.lufs_tick_rect(plot, top_l);
        if !hidden_by_chip(top_label) {
            draw_tick_btn(painter, top_l, top_label);
        }
        if tgt_valid {
            let mid_label = self.lufs_tick_rect(plot, self.target);
            if !hidden_by_chip(mid_label) {
                draw_tick_btn(painter, self.target, mid_label);
            }
        }
        let bot_label = self.lufs_tick_rect(plot, bot_l);
        if !hidden_by_chip(bot_label) {
            draw_tick_text(painter, bot_l, bot_label);
        }

        if tgt_valid {
            let y_t = self.loud_y_of(plot, self.target);
            painter.rect_filled(
                Rect::from_min_max(Pos2::new(bar_i.left(), y_t - 1.0), Pos2::new(bar_m.right(), y_t + 1.0)),
                0.0,
                theme.col_900(),
            );
        }

        let y_of_f = |f: f32| plot.bottom() - f.clamp(0.0, 1.0) * plot.height();
        let draw_bar = |painter: &egui::Painter, bar: Rect, lufs: f32, label: &str| {
            painter.rect_filled(bar, 0.0, theme.col_300());
            if lufs > -99.0 {
                let y_top = self.loud_y_of(plot, lufs);
                for i in 0..stops.len() - 1 {
                    let y_a = y_of_f(stops[i].t);
                    let y_b = y_of_f(stops[i + 1].t);
                    if y_a <= y_top {
                        continue;
                    }
                    let r_t = (y_b - 1.0).max(y_top);
                    let r_b = (y_a + 1.0).min(bar.bottom());
                    if r_b - r_t <= 0.0 {
                        continue;
                    }
                    let mid_f = (stops[i].t + stops[i + 1].t) * 0.5;
                    painter.rect_filled(
                        Rect::from_min_max(Pos2::new(bar.left(), r_t), Pos2::new(bar.right(), r_b)),
                        0.0,
                        meter::stops_color(&stops, mid_f, c_scale, 255),
                    );
                }
            }
            painter.text(
                Pos2::new(bar.center().x, plot.top()),
                Align2::CENTER_TOP,
                label,
                fonts::regular(10.0),
                theme.col_500(),
            );
        };

        draw_bar(painter, bar_i, self.integrated, "I");
        draw_bar(painter, bar_s, self.short_term, "S");
        draw_bar(painter, bar_m, self.momentary, "M");

        let i_act = self.integrated > -99.0 && self.integrated > bot_l;
        let s_act = self.short_term > -99.0 && self.short_term > bot_l;
        let m_act = self.momentary > -99.0 && self.momentary > bot_l;
        let i_buf = if self.integrated > -99.0 {
            format!("{:.1}", self.integrated)
        } else {
            "—".to_string()
        };

        let draw_mark = |painter: &egui::Painter, bar: Rect, txt: &str, active: bool| {
            let box_r = Rect::from_min_max(
                Pos2::new(bar.left(), plot.bottom() - K_MARK_H),
                Pos2::new(bar.right(), plot.bottom()),
            );
            let col = if active { theme.col_300() } else { theme.col_700() };
            let mut size = 11.0_f32;
            loop {
                let font = fonts::semibold(size);
                let m = painter.layout_no_wrap(txt.to_owned(), font, col);
                if (m.rect.min.x >= box_r.left() - 0.5 && m.rect.max.x <= box_r.right() + 0.5) || size <= 8.0 {
                    break;
                }
                size -= 1.0;
            }
            painter.text(box_r.center(), Align2::CENTER_CENTER, txt, fonts::semibold(size), col);
        };

        draw_mark(painter, bar_i, &i_buf, i_act);
        draw_mark(painter, bar_s, "LU", s_act);
        draw_mark(painter, bar_m, "FS", m_act);
    }

    fn draw_lra_bracket(&self, painter: &egui::Painter, plot: Rect, theme: &Theme) {
        if !self.lra_valid {
            return;
        }
        let (top_l, bot_l) = self.loud_window();
        let y_top = self.loud_y_of(plot, self.lra_max.clamp(bot_l, top_l));
        let y_bot = self.loud_y_of(plot, self.lra_min.clamp(bot_l, top_l));
        if y_bot - y_top < 1.0 {
            return;
        }
        let dark = theme.mode == ThemeMode::Dark;
        let stops = meter::loud_stops(dark);
        let c_scale = meter::meter_chroma_scale(theme);
        let col = meter::stops_color(&stops, 1.0, c_scale, 255);

        let bars_r = meter::vu_zone_r(plot) + K_LUFS_SCALE_W + 4.0 * K_LOUD_BAR_W;
        let span = y_bot - y_top;
        let mid_y = (y_top + y_bot) * 0.5;
        let val_buf = format!("LRA {:.1}", self.range);

        let galley = painter.layout_no_wrap(val_buf.clone(), fonts::semibold(14.0), col);
        let mr = galley.size();
        let text_l = bars_r + K_GAP_BARS;
        let text_t = (mid_y - mr.y * 0.5).clamp(plot.top(), plot.bottom() - mr.y);

        let mut shape = egui::epaint::TextShape::new(
            Pos2::new(text_l + mr.y * 0.5, text_t + mr.x * 0.5),
            galley,
            col,
        );
        shape.angle = -std::f32::consts::PI * 0.5;
        painter.add(shape);

        if mr.y > span {
            return;
        }
        let spine_l = text_l + mr.y + K_TEXT_PAD;
        painter.rect_filled(
            Rect::from_min_max(Pos2::new(spine_l, y_top), Pos2::new(spine_l + K_SPINE_W, y_bot)),
            0.0,
            col,
        );
        painter.rect_filled(
            Rect::from_min_max(
                Pos2::new(text_l, y_top - K_CAP_H * 0.5),
                Pos2::new(spine_l + K_SPINE_W, y_top + K_CAP_H * 0.5),
            ),
            0.0,
            col,
        );
        painter.rect_filled(
            Rect::from_min_max(
                Pos2::new(text_l, y_bot - K_CAP_H * 0.5),
                Pos2::new(spine_l + K_SPINE_W, y_bot + K_CAP_H * 0.5),
            ),
            0.0,
            col,
        );
    }

    // ----- HUD -----

    fn draw_hud(&self, painter: &egui::Painter, theme: &Theme) {
        if self.hud_fill == 0 {
            return;
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        let (mut sum, mut sum_d, mut sum_h, mut mx) = (0.0_f32, 0.0_f32, 0.0_f32, 0.0_f32);
        let (mut n_d, mut n_h) = (0, 0);
        let (mut n_d1, mut n_h1, mut n_all1) = (0, 0, 0);
        for i in 0..self.hud_fill {
            let v = self.hud_ms[i];
            sum += v;
            mx = mx.max(v);
            if self.hud_is_data[i] != 0 {
                sum_d += v;
                n_d += 1;
            } else {
                sum_h += v;
                n_h += 1;
            }
            if now - self.hud_t[i] <= 1.0 {
                n_all1 += 1;
                if self.hud_is_data[i] != 0 {
                    n_d1 += 1;
                } else {
                    n_h1 += 1;
                }
            }
        }
        let avg = sum / self.hud_fill as f32;
        let avg_d = if n_d > 0 { sum_d / n_d as f32 } else { 0.0 };
        let avg_h = if n_h > 0 { sum_h / n_h as f32 } else { 0.0 };

        let l1 = format!("pad {avg:.2} ms avg / {mx:.2} max / {n_all1} draw/s");
        let l2 = format!("data {avg_d:.2} ms x{n_d1}/s | hover {avg_h:.2} ms x{n_h1}/s");

        let plot = self.plot_rect();
        let w_box = painter
            .layout_no_wrap(l1.clone(), fonts::regular(11.0), theme.col_700())
            .size()
            .x
            .max(
                painter
                    .layout_no_wrap(l2.clone(), fonts::regular(11.0), theme.col_700())
                    .size()
                    .x,
            )
            + 8.0;
        let box_r = Rect::from_min_max(
            Pos2::new(plot.left() + 4.0, plot.bottom() - 38.0),
            Pos2::new(plot.left() + 4.0 + w_box, plot.bottom() - 4.0),
        );
        let bg = Color32::from_rgba_unmultiplied(
            theme.col_100().r(),
            theme.col_100().g(),
            theme.col_100().b(),
            190,
        );
        painter.rect_filled(box_r, 0.0, bg);
        painter.text(
            Pos2::new(box_r.left() + 4.0, box_r.top() + 2.0),
            Align2::LEFT_TOP,
            l1,
            fonts::regular(11.0),
            theme.col_700(),
        );
        painter.text(
            Pos2::new(box_r.left() + 4.0, box_r.top() + 19.0),
            Align2::LEFT_TOP,
            l2,
            fonts::regular(11.0),
            theme.col_700(),
        );
    }
}

/// 等价原版 IRECT::Empty()：零面积矩形视为空。
fn rect_empty(r: Rect) -> bool {
    r.width() <= 0.0 || r.height() <= 0.0
}

fn freq_norm(hz: f64) -> f32 {
    let hz = hz.clamp(K_SPEC_FREQ_LO as f64, K_SPEC_FREQ_HI as f64);
    ((hz / K_SPEC_FREQ_LO as f64).ln() / (K_SPEC_FREQ_HI as f64 / K_SPEC_FREQ_LO as f64).ln()) as f32
}

fn freq_to_note_name(hz: f64) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    let nn = (12.0 * (hz / 440.0).log2()).round() as i32 + 69;
    let name = NAMES[(((nn % 12) + 12) % 12) as usize];
    format!("{}{}", name, nn / 12 - 1)
}
