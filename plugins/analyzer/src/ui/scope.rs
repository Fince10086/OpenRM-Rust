//! 示波器控件 —— 逐行移植自原版 `controls/OscilloscopeControl.h`（920 行）。
//!
//! 三种触发：ROLL（右缘为最新样本）/ SYNC（自相关基频周期锁相）/ SWEEP（自由扫描回卷）。
//! 时基 10 ms–2 s、幅度倍率 1×–8×、声道掩码 L/R/M。
//!
//! 原版音频线程给的是 1<<19 的冻结环 + 头位置；Rust 集成层每 hop（1024 对样本）
//! 交一个 [`ScopeFrame`]，控件内部保留窗口所需长度的线性历史，等价原环语义。
//! 常量名 / 数值 / 顺序与原版一一对应，注释标了原版符号。

use std::time::Instant;

use egui::{Align2, Color32, Pos2, Rect, Shape, Stroke};
use openrm_ui::{
    fonts,
    strings::{self, Lang},
    theme::Theme,
};

use super::data::ScopeFrame;
use super::meter;

// ===== OscilloscopeControl.h:36-37 默认值 =====
const K_DEFAULT_WINDOW_SEC: f64 = 0.100;
const K_DEFAULT_ZOOM: f32 = 2.0;

// ===== OscilloscopeControl.h:303-305 连续范围 =====
const K_MIN_WINDOW_SEC: f64 = 0.010;
const K_MAX_WINDOW_SEC: f64 = 2.000;
const K_MAX_ZOOM: f32 = 8.0;

// ===== OscilloscopeControl.h:307-308 贴边标签几何 =====
const K_TICK_RIGHT: f32 = 3.0;
const K_LABEL_H: f32 = 16.0;

// ===== OscilloscopeControl.h:310-311 时间列 1-2-5 阶梯（ms，升序） =====
const K_TIME_CELLS: [f64; 13] = [
    0.1, 0.2, 0.5, 1.0, 2.0, 5.0, 10.0, 20.0, 50.0, 100.0, 200.0, 500.0, 1000.0,
];
// ===== OscilloscopeControl.h:313-314 中间档（主档 ×1.5） =====
const K_TIME_CELLS_MID: [f64; 13] = [
    0.15, 0.3, 0.75, 1.5, 3.0, 7.5, 15.0, 30.0, 75.0, 150.0, 300.0, 750.0, 1500.0,
];
// ===== OscilloscopeControl.h:316 幅度阶梯（±1.0 起逐级减半，升序） =====
const K_AMP_LADDER: [f64; 8] = [
    0.0078125, 0.015625, 0.03125, 0.0625, 0.125, 0.25, 0.5, 1.0,
];
// ===== OscilloscopeControl.h:318-319 幅度中间档（×√½，+3 dB） =====
const K_AMP_LADDER_MID: [f64; 8] = [
    0.00552427, 0.01104854, 0.02209709, 0.04419417, 0.08838835, 0.1767767, 0.35355339,
    0.70710678,
];

// ===== OscilloscopeControl.h:82-83 内嵌滑块尺寸 =====
const K_SLIDER_W: f32 = 20.0;
const K_SLIDER_H: f32 = 20.0;

// ===== OscilloscopeControl.h:819 / 838-839 自相关参数 =====
const K_MIN_LAG_RATE: f64 = 2500.0; // 2500 Hz 上限
const K_ACF_N: usize = 4096;
const K_FFT_N: usize = 8192;
const K_THRESH: f32 = 0.35; // 最低自相关置信门限
const K_MARGIN: f32 = 0.10;

// ===== OscilloscopeControl.h:725 hover 标签间距 =====
const K_GAP: f32 = 5.0;

// ===== 双击判定阈值（对齐 egui InputState 默认：0.3 s / 6 px） =====
const K_DBL_CLICK_DELAY: f32 = 0.3;
const K_DBL_CLICK_DIST: f32 = 6.0;

/// 触发源（原版 ETriggerSource:21-26）。
pub mod trig {
    pub const ROLL: i32 = 0;
    pub const SYNC: i32 = 1;
    pub const SWEEP: i32 = 2;
    pub const NUM: i32 = 3;
}

/// 声道显示掩码（原版 EChanBit:29-33）。
pub mod chan {
    pub const L: i32 = 1;
    pub const R: i32 = 2;
    pub const M: i32 = 4;
}

/// 控件内命中动作（原版 OnMouseDown 为空；双击画区复位缩放）。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScopeAction {
    /// 双击波形区：复位幅度倍率到默认 2×（原版 OnMouseDblClick:250-257）。
    ResetZoom,
}

/// 示波器 retained 控件。
pub struct Oscilloscope {
    pub rect: Rect,

    // ===== 顶栏按钮驱动的纯 UI 状态（OnMsgFromDelegate 等价） =====
    trig_source: i32,
    chan_mask: i32,
    window_sec: f64,
    zoom_factor: f32,
    sample_rate: f64,
    frozen: bool,

    // ===== 内部历史（替代原版冻结环） =====
    hist_l: Vec<f32>,
    hist_r: Vec<f32>,

    // ===== 显示缓冲（mDispL/R/M/S/Sum） =====
    disp_l: Vec<f32>,
    disp_r: Vec<f32>,
    disp_m: Vec<f32>,
    disp_s: Vec<f32>,
    disp_sum: Vec<f32>,

    sweep_pos: usize,
    last_head_pos: i64,
    abs_head: i64,
    prev_head_pos: i64,
    disp_right_abs: i64,
    frozen_anchor: i64,
    extracted_win_sec: f64,

    // ===== 自相关检测结果 =====
    detected_freq: f32,
    detected_period_ms: f32,
    detected_lag: i32,
    trig_state_active: bool,

    // ===== 自相关 scratch（4096 窗 / 8192 FFT） =====
    fft_re: Vec<f32>,
    fft_im: Vec<f32>,
    acf: Vec<f32>,

    // ===== 抽稀包络复用缓冲（方法内局部持有） =====

    // ===== hover =====
    hover_active: bool,
    hover_x: f32,
    hover_y: f32,
    hover_time_buf: String,
    hover_freq_buf: String,
    hover_amp_buf: String,

    // ===== 双击识别（等价原版 OnMouseDblClick:250-257） =====
    last_click_at: Option<Instant>,
    last_click_pos: Option<Pos2>,
}

impl Default for Oscilloscope {
    fn default() -> Self {
        Self::new(Rect::from_min_max(Pos2::new(400.0, 366.0), Pos2::new(696.0, 599.0)))
    }
}

impl Oscilloscope {
    pub fn new(rect: Rect) -> Self {
        Self {
            rect,
            trig_source: trig::SYNC, // 原版默认 SYNC（:323）
            chan_mask: chan::M,      // 原版默认 M（:324）
            window_sec: K_DEFAULT_WINDOW_SEC,
            zoom_factor: K_DEFAULT_ZOOM,
            sample_rate: 48_000.0,
            frozen: false,
            hist_l: Vec::with_capacity(8192),
            hist_r: Vec::with_capacity(8192),
            disp_l: Vec::new(),
            disp_r: Vec::new(),
            disp_m: Vec::new(),
            disp_s: Vec::new(),
            disp_sum: Vec::new(),
            sweep_pos: 0,
            last_head_pos: -1,
            abs_head: 0,
            prev_head_pos: -1,
            disp_right_abs: -1,
            frozen_anchor: -1,
            extracted_win_sec: -1.0,
            detected_freq: 0.0,
            detected_period_ms: 0.0,
            detected_lag: -1,
            trig_state_active: false,
            fft_re: vec![0.0; K_FFT_N],
            fft_im: vec![0.0; K_FFT_N],
            acf: vec![0.0; K_ACF_N + 2],
            hover_active: false,
            hover_x: 0.0,
            hover_y: 0.0,
            hover_time_buf: String::new(),
            hover_freq_buf: String::new(),
            hover_amp_buf: String::new(),
            last_click_at: None,
            last_click_pos: None,
        }
    }

    // ==================== kMsgTag* 对应 setter ====================

    pub fn set_sample_rate(&mut self, sr: f64) {
        self.sample_rate = sr.max(1000.0);
    }

    /// 原版 `SetTrigSource`（:71-75）：切模式后扫描笔从左缘重写。
    pub fn set_trig_source(&mut self, v: i32) {
        self.trig_source = v.clamp(0, trig::NUM - 1);
        self.sweep_pos = 0;
    }

    /// 原版 `SetChanMask`（:76）。
    pub fn set_chan_mask(&mut self, v: i32) {
        self.chan_mask = v.clamp(0, chan::L | chan::R | chan::M);
    }

    /// 原版 `SetWindowSec`（:77）：夹在 [10 ms, 2 s]。
    pub fn set_window_sec(&mut self, s: f64) {
        self.window_sec = s.clamp(K_MIN_WINDOW_SEC, K_MAX_WINDOW_SEC);
    }

    /// 原版 `SetZoomFactor`（:78）：夹在 [1, 8]。
    pub fn set_zoom_factor(&mut self, z: f32) {
        self.zoom_factor = z.clamp(1.0, K_MAX_ZOOM);
    }

    pub fn set_frozen(&mut self, frozen: bool) {
        self.frozen = frozen;
    }

    /// 原版 `Clear`（:47-64）：清空全部迹线与检测状态。
    pub fn clear(&mut self) {
        self.disp_l.clear();
        self.disp_r.clear();
        self.disp_m.clear();
        self.disp_s.clear();
        self.disp_sum.clear();
        self.detected_freq = 0.0;
        self.detected_period_ms = 0.0;
        self.detected_lag = -1;
        self.trig_state_active = false;
        self.sweep_pos = 0;
        self.frozen_anchor = -1;
        self.extracted_win_sec = -1.0;
        self.abs_head = 0;
        self.prev_head_pos = -1;
        self.disp_right_abs = -1;
        self.last_head_pos = -1;
    }

    /// RESET 按钮联动：示波器等价原版 `Clear`。
    pub fn clear_peak_hold(&mut self) {
        self.clear();
    }

    // ==================== 状态读取（集成层 / 测试用） ====================

    pub fn trig_source(&self) -> i32 {
        self.trig_source
    }
    pub fn chan_mask(&self) -> i32 {
        self.chan_mask
    }
    pub fn window_sec(&self) -> f64 {
        self.window_sec
    }
    pub fn zoom_factor(&self) -> f32 {
        self.zoom_factor
    }
    pub fn detected_lag(&self) -> i32 {
        self.detected_lag
    }
    pub fn detected_freq(&self) -> f32 {
        self.detected_freq
    }
    pub fn detected_period_ms(&self) -> f32 {
        self.detected_period_ms
    }
    pub fn disp_l(&self) -> &[f32] {
        &self.disp_l
    }
    pub fn disp_m(&self) -> &[f32] {
        &self.disp_m
    }

    /// 缩放滑块矩形（原版 `GetZoomSliderRect`:84-86）。
    pub fn zoom_slider_rect(&self) -> Rect {
        Rect::from_min_max(
            Pos2::new(self.rect.left(), self.rect.top() + 2.0),
            Pos2::new(self.rect.left() + K_SLIDER_W, self.rect.bottom() - K_SLIDER_H - 6.0),
        )
    }

    /// 时基滑块矩形（原版 `GetTimeSliderRect`:87-89）。
    pub fn time_slider_rect(&self) -> Rect {
        Rect::from_min_max(
            Pos2::new(self.rect.left() + K_SLIDER_W, self.rect.bottom() - K_SLIDER_H - 2.0),
            Pos2::new(self.rect.right() - 2.0, self.rect.bottom() - 2.0),
        )
    }

    /// 波形画区（原版 `GetPlotRect`:91-94）。
    pub fn plot_rect(&self) -> Rect {
        Rect::from_min_max(
            Pos2::new(self.rect.left() + K_SLIDER_W, self.rect.top() + 2.0),
            Pos2::new(self.rect.right() - 2.0, self.rect.bottom() - K_SLIDER_H - 2.0),
        )
    }

    // ==================== 数据管线 ====================

    /// 每 hop 调用一次（等价原版 `UpdateAudio`:108-224）。
    pub fn update(&mut self, frame: &ScopeFrame) {
        // 追加样本到内部历史；只保留最长窗口 + 自相关窗所需长度。
        self.hist_l.extend_from_slice(&frame.l);
        self.hist_r.extend_from_slice(&frame.r);
        let cap = (K_MAX_WINDOW_SEC * self.sample_rate).ceil() as usize + K_ACF_N + 128;
        if self.hist_l.len() > cap {
            let drop = self.hist_l.len() - cap;
            self.hist_l.drain(..drop);
            self.hist_r.drain(..drop);
        }

        let ring_len = self.hist_l.len();
        let n_disp = (self.window_sec * self.sample_rate).round() as usize;
        let n_disp = n_disp.clamp(16, ring_len.saturating_sub(64).max(16));

        if ring_len < n_disp + 128 {
            return;
        }

        if self.frozen {
            // 冻结中拖时间窗：据锚点按新窗长重取迹线（原版 :118-125）。
            if self.frozen_anchor >= 0 && (self.window_sec - self.extracted_win_sec).abs() > 1e-9 {
                self.extract_window(self.frozen_anchor, n_disp);
                self.disp_right_abs = self.abs_head - 1
                    - ((ring_len as i64 - 1 - self.frozen_anchor).rem_euclid(ring_len as i64));
                self.sweep_pos = 0;
                self.extracted_win_sec = self.window_sec;
            }
            return;
        }

        let head = (ring_len - 1) as i64;
        if self.prev_head_pos >= 0 {
            let adv = (head - self.prev_head_pos).rem_euclid(ring_len as i64);
            self.abs_head += adv;
        }
        self.prev_head_pos = head;

        let trig_chan = self.effective_trig_chan();

        let trigger_pos: i64;
        self.trig_state_active = false;

        if self.trig_source == trig::ROLL {
            // ROLL：右缘始终为最新采样点（:142-147）。
            trigger_pos = head;
            self.trig_state_active = true;
        } else if self.trig_source == trig::SYNC {
            // SYNC：自相关基波周期锁定（:148-167）。
            self.compute_autocorr(trig_chan, head);
            if self.detected_lag > 8 {
                self.trig_state_active = true;
                let search_range = (self.detected_lag * 2)
                    .min((ring_len - n_disp).saturating_sub(16) as i32)
                    .max(0);
                let mut best_val = -1e9_f32;
                let mut best_idx = head;
                for k in 0..search_range {
                    let p = (head - k as i64).rem_euclid(ring_len as i64) as usize;
                    let v = self.hist_sample(trig_chan, p);
                    if v > best_val {
                        best_val = v;
                        best_idx = p as i64;
                    }
                }
                trigger_pos = best_idx;
            } else {
                trigger_pos = head;
            }
        } else {
            // SWEEP：自由扫描，逐样本写入固定窗缓冲，写满回卷（:169-213）。
            self.disp_right_abs = -1;
            if self.disp_l.len() != n_disp {
                self.disp_l.resize(n_disp, 0.0);
                self.disp_r.resize(n_disp, 0.0);
                self.disp_m.resize(n_disp, 0.0);
                self.disp_s.resize(n_disp, 0.0);
                self.disp_sum.resize(n_disp, 0.0);
                self.sweep_pos = 0;
            }
            if self.last_head_pos < 0 {
                self.last_head_pos = head;
                return;
            }
            let advance = (head - self.last_head_pos).rem_euclid(ring_len as i64);
            self.last_head_pos = head;
            if advance <= 0 {
                return;
            }
            let n_advance = advance.min(n_disp as i64);
            for k in (1..=n_advance).rev() {
                let p = (head - k).rem_euclid(ring_len as i64) as usize;
                let l = self.hist_l[p];
                let r = self.hist_r[p];
                let pos = self.sweep_pos;
                self.disp_l[pos] = l;
                self.disp_r[pos] = r;
                self.disp_m[pos] = (l + r) * 0.707_106_78;
                self.disp_s[pos] = (l - r) * 0.707_106_78;
                self.disp_sum[pos] = l + r;
                self.sweep_pos += 1;
                if self.sweep_pos >= n_disp {
                    self.sweep_pos = 0;
                }
            }
            self.trig_state_active = true;
            if self.frozen {
                self.frozen_anchor = (head - 1).rem_euclid(ring_len as i64);
                self.extracted_win_sec = self.window_sec;
            }
            return;
        }

        if self.frozen {
            self.frozen_anchor = trigger_pos;
        }

        self.extract_window(trigger_pos, n_disp);
        self.disp_right_abs =
            self.abs_head - 1 - (head - trigger_pos).rem_euclid(ring_len as i64);
        self.extracted_win_sec = self.window_sec;
    }

    fn hist_sample(&self, chan: i32, p: usize) -> f32 {
        match chan {
            0 => self.hist_l[p],
            1 => self.hist_r[p],
            _ => (self.hist_l[p] + self.hist_r[p]) * 0.707_106_78,
        }
    }

    /// 触发参考通道（原版 `EffectiveTrigChan`:97-105）。
    fn effective_trig_chan(&self) -> i32 {
        if self.chan_mask & chan::M != 0 {
            return 2;
        }
        if self.chan_mask & chan::L != 0 && self.chan_mask & chan::R == 0 {
            return 0;
        }
        if self.chan_mask & chan::R != 0 && self.chan_mask & chan::L == 0 {
            return 1;
        }
        2
    }

    /// 以 rightEdge 为右缘提取 nDisp 点前向历史（原版 `ExtractWindow`:227-245）。
    fn extract_window(&mut self, right_edge: i64, n_disp: usize) {
        self.disp_l.resize(n_disp, 0.0);
        self.disp_r.resize(n_disp, 0.0);
        self.disp_m.resize(n_disp, 0.0);
        self.disp_s.resize(n_disp, 0.0);
        self.disp_sum.resize(n_disp, 0.0);

        let ring_len = self.hist_l.len() as i64;
        let read_start = (right_edge - n_disp as i64 + ring_len).rem_euclid(ring_len) as usize;
        for i in 0..n_disp {
            let p = (read_start + i).rem_euclid(ring_len as usize);
            let l = self.hist_l[p];
            let r = self.hist_r[p];
            self.disp_l[i] = l;
            self.disp_r[i] = r;
            self.disp_m[i] = (l + r) * 0.707_106_78;
            self.disp_s[i] = (l - r) * 0.707_106_78;
            self.disp_sum[i] = l + r;
        }
    }

    // ==================== 自相关（原版 :774-877） ====================

    fn compute_autocorr(&mut self, chan: i32, head: i64) {
        let n = K_ACF_N;
        let fft_n = K_FFT_N;

        let ring_len = self.hist_l.len();
        let mut mean = 0.0_f32;
        let start = (head - n as i64 + ring_len as i64).rem_euclid(ring_len as i64) as usize;
        let mut src: Vec<f32> = Vec::with_capacity(n);
        for i in 0..n {
            let p = (start + i) % ring_len;
            src.push(self.hist_sample(chan, p));
            mean += src[i];
        }
        mean /= n as f32;
        let re = &mut self.fft_re;
        let im = &mut self.fft_im;
        re.fill(0.0);
        im.fill(0.0);
        for i in 0..n {
            re[i] = src[i] - mean;
        }

        fft_radix2(re, im);

        // 功率谱
        for i in 0..fft_n {
            let r = re[i];
            let i_ = im[i];
            re[i] = r * r + i_ * i_;
            im[i] = 0.0;
        }
        // 逆 FFT：实偶对称序列的 IFFT 等价再次正向 FFT
        fft_radix2(re, im);

        let energy0 = re[0];
        if energy0 < 1e-8 {
            self.detected_freq = 0.0;
            self.detected_period_ms = 0.0;
            self.detected_lag = -1;
            return;
        }

        let min_lag = 10_i32.max((self.sample_rate / K_MIN_LAG_RATE).round() as i32);
        let max_lag = (n - 4).min((0.85 * n as f32) as usize);

        let inv_e = 1.0 / energy0;
        let acf = &mut self.acf;
        for tau in 0..=max_lag + 1 {
            let bias = n as f32 / (n - tau).max(1) as f32;
            acf[tau] = (re[tau] * inv_e * bias).clamp(-1.0, 1.0);
        }

        // 跨过主瓣下降区
        let mut valley_lag = 1;
        while valley_lag < max_lag && acf[valley_lag] > 0.3 && acf[valley_lag] <= acf[valley_lag - 1] {
            valley_lag += 1;
        }

        let search_from = min_lag.max(valley_lag as i32).max(0) as usize;
        let mut best_lag: i32 = -1;
        let mut best_peak = K_THRESH;
        for lag in search_from..max_lag {
            if acf[lag] > best_peak && acf[lag] > acf[lag - 1] && acf[lag] >= acf[lag + 1] {
                best_peak = acf[lag];
                best_lag = lag as i32;
            }
        }

        if best_lag > 0 {
            // margin 内的最短峰（严格周期信号回退到基波）
            let cut = best_peak - K_MARGIN;
            for lag in search_from..best_lag as usize {
                if acf[lag] >= cut && acf[lag] > acf[lag - 1] && acf[lag] >= acf[lag + 1] {
                    best_lag = lag as i32;
                    break;
                }
            }

            // 二次抛物线亚采样插值
            let y0 = acf[best_lag as usize - 1];
            let y1 = acf[best_lag as usize];
            let y2 = acf[best_lag as usize + 1];
            let denom = 2.0 * (y0 - 2.0 * y1 + y2);
            let delta = if denom.abs() > 1e-6 {
                (y0 - y2) / denom
            } else {
                0.0
            };
            let exact_lag = best_lag as f32 + delta.clamp(-0.5, 0.5);

            self.detected_lag = exact_lag.round() as i32;
            self.detected_period_ms = exact_lag / self.sample_rate as f32 * 1000.0;
            self.detected_freq = self.sample_rate as f32 / exact_lag;
        } else {
            self.detected_lag = -1;
            self.detected_freq = 0.0;
            self.detected_period_ms = 0.0;
        }
    }

    // ==================== 命中 / hover ====================

    pub fn click(&mut self, pos: Pos2) -> Option<ScopeAction> {
        // 原版 OnMouseDown/Up/Drag 均为空（:247-263）；双击由两次快速点击识别，
        // 阈值对齐 egui InputState 默认（0.3 s / 6 px）。
        if !self.plot_rect().contains(pos) {
            self.last_click_at = None;
            self.last_click_pos = None;
            return None;
        }
        let now = Instant::now();
        let within_delay = self
            .last_click_at
            .map_or(false, |t| now.duration_since(t).as_secs_f32() < K_DBL_CLICK_DELAY);
        let within_dist = self
            .last_click_pos
            .map_or(false, |p| p.distance(pos) < K_DBL_CLICK_DIST);
        self.last_click_at = Some(now);
        self.last_click_pos = Some(pos);
        if within_delay && within_dist {
            self.reset_zoom();
            Some(ScopeAction::ResetZoom)
        } else {
            None
        }
    }

    /// 双击画区复位缩放（原版 OnMouseDblClick:250-257）。
    pub fn dbl_click(&mut self, pos: Pos2) -> Option<ScopeAction> {
        if self.plot_rect().contains(pos) {
            self.reset_zoom();
            Some(ScopeAction::ResetZoom)
        } else {
            None
        }
    }

    fn reset_zoom(&mut self) {
        self.zoom_factor = K_DEFAULT_ZOOM;
    }

    /// 记录 hover 位置；外部 tooltip 由集成层按滑杆矩形给（频谱同款分工）。
    pub fn hover(&mut self, pos: Pos2) -> Option<String> {
        let plot = self.plot_rect();
        if !plot.contains(pos) {
            self.hover_active = false;
            return None;
        }
        self.hover_active = true;
        self.hover_x = pos.x;
        self.hover_y = pos.y;
        None
    }

    // ==================== 绘制 ====================

    pub fn draw(&mut self, painter: &egui::Painter, theme: &Theme, _lang: Lang) {
        painter.rect_filled(self.rect, 0.0, theme.col_100());
        let plot = self.plot_rect();

        let hov = self.hover_active && plot.contains(Pos2::new(self.hover_x, self.hover_y));
        let det_r = if self.trig_source == trig::SYNC {
            self.det_rect(plot)
        } else {
            Rect::ZERO
        };
        let (hov_time, hov_amp) = if hov {
            self.compute_hover(painter, theme, plot, det_r)
        } else {
            (Rect::ZERO, Rect::ZERO)
        };

        self.draw_background(painter, plot, theme);
        self.draw_waveforms(painter, plot, theme);
        self.draw_axis(painter, plot, hov_time, hov_amp, det_r, theme);
        self.draw_status_readout(painter, plot, theme);

        if hov {
            self.draw_hover(painter, plot, theme);
        }
    }

    fn current_zoom(&self) -> f32 {
        self.zoom_factor.clamp(1.0, K_MAX_ZOOM)
    }

    fn level_to_y(&self, plot: Rect, lvl: f32) -> f32 {
        let cy = plot.center().y;
        let half_h = plot.height() * 0.48;
        cy - lvl * self.current_zoom() * half_h
    }

    fn y_to_level(&self, plot: Rect, y: f32) -> f32 {
        let cy = plot.center().y;
        let half_h = plot.height() * 0.48;
        let raw = (cy - y) / (half_h * self.current_zoom());
        raw.clamp(-1.0, 1.0)
    }

    /// 色块背景（原版 `DrawPlotBackground`:402-448）。
    fn draw_background(&self, painter: &egui::Painter, plot: Rect, theme: &Theme) {
        let zoom = self.current_zoom();
        let half_h = plot.height() * 0.48;
        let cy = plot.center().y;
        let vis_max: f32 = (plot.height() * 0.5) / (half_h * zoom);
        let win_ms = self.window_sec * 1000.0;

        const K_V_COL: [f32; 5] = [176.0, 190.0, 204.0, 218.0, 232.0];
        const K_H_COL: [f32; 5] = [240.0, 228.0, 216.0, 204.0, 192.0];

        let (amp_b, _amp_maj) = largest_ladder(vis_max as f64, &K_AMP_LADDER, &K_AMP_LADDER_MID, 4);
        let mut bnds = [0.0_f32; 6];
        let mut nb = 0;
        bnds[nb] = vis_max;
        nb += 1;
        for v in amp_b.iter().flatten() {
            if *v > 0.0 {
                bnds[nb] = *v as f32;
                nb += 1;
            }
        }
        bnds[nb] = 0.0;
        nb += 1;

        let (tick_b, _tick_maj) = largest_ladder(win_ms, &K_TIME_CELLS, &K_TIME_CELLS_MID, 4);
        let mut xs = [0.0_f32; 6];
        let mut ncols = 0;
        xs[ncols] = plot.right();
        ncols += 1;
        for i in (0..4).rev() {
            let v = match tick_b[i] {
                Some(v) => v,
                None => continue,
            };
            let x = plot.right() - (v / win_ms) as f32 * plot.width();
            if x - plot.left() < 1.5 {
                continue;
            }
            xs[ncols] = x;
            ncols += 1;
        }
        xs[ncols] = plot.left();
        ncols += 1;

        for r in 0..nb - 1 {
            let y_t0 = cy - bnds[r] * zoom * half_h;
            let y_t1 = cy - bnds[r + 1] * zoom * half_h;
            let y_b0 = cy + bnds[r + 1] * zoom * half_h;
            let y_b1 = cy + bnds[r] * zoom * half_h;
            for c in 0..ncols - 1 {
                let v = (0.5 * (K_H_COL[c] + K_V_COL[r])).round() as u8;
                let cell = meter::grid_gray(theme, v);
                painter.rect_filled(
                    Rect::from_min_max(Pos2::new(xs[c + 1], y_t0), Pos2::new(xs[c], y_t1)),
                    0.0,
                    cell,
                );
                painter.rect_filled(
                    Rect::from_min_max(Pos2::new(xs[c + 1], y_b0), Pos2::new(xs[c], y_b1)),
                    0.0,
                    cell,
                );
            }
        }
    }


    /// 中轴 + 贴边刻度（原版 `DrawAxis`:476-576）。
    fn draw_axis(
        &self,
        painter: &egui::Painter,
        plot: Rect,
        hov_time: Rect,
        hov_amp: Rect,
        det_r: Rect,
        theme: &Theme,
    ) {
        let zoom = self.current_zoom();
        let half_h = plot.height() * 0.48;
        let vis_max: f32 = (plot.height() * 0.5) / (half_h * zoom);
        let win_ms = self.window_sec * 1000.0;

        // 零电平中轴线
        painter.line_segment(
            [
                Pos2::new(plot.left(), plot.center().y),
                Pos2::new(plot.right(), plot.center().y),
            ],
            Stroke::new(1.0_f32, theme.col_500()),
        );

        let amp_font = fonts::regular(14.0);
        let mut amp_rs: Vec<Rect> = Vec::new();

        // 上半幅主档标签
        for i in (0..K_AMP_LADDER.len()).rev() {
            let t = K_AMP_LADDER[i] as f32;
            if t >= vis_max * (1.0_f32 - 1e-4) {
                continue;
            }
            if (t - 0.5 * t) * zoom * half_h < 18.0_f32 {
                break;
            }
            let y = self.level_to_y(plot, t as f32);
            let label_r = if y - K_LABEL_H - 1.0 >= plot.top() {
                Rect::from_min_max(
                    Pos2::new(plot.right() - 52.0, y - K_LABEL_H - 1.0),
                    Pos2::new(plot.right() - K_TICK_RIGHT, y - 1.0),
                )
            } else {
                Rect::from_min_max(
                    Pos2::new(plot.right() - 52.0, y + 1.0),
                    Pos2::new(plot.right() - K_TICK_RIGHT, y + 1.0 + K_LABEL_H),
                )
            };
            let buf = format!("{}", (20.0 * t.log10()).round() as i32);
            let fit = measure_text(painter, &buf, amp_font.clone(), theme.col_700(), label_r);
            let blocked = (!rect_empty(hov_amp) && fit.intersects(hov_amp))
                || (!rect_empty(hov_time) && fit.intersects(hov_time))
                || amp_rs.len() >= 6;
            if blocked {
                continue;
            }
            painter.text(
                Pos2::new(label_r.right() - K_TICK_RIGHT, fit.center().y),
                Align2::RIGHT_CENTER,
                &buf,
                amp_font.clone(),
                theme.col_700(),
            );
            amp_rs.push(fit);
        }

        // 中间档仅在它是量程内最外一颗分界时补标
        let (amp_b, amp_maj) = largest_ladder(vis_max as f64, &K_AMP_LADDER, &K_AMP_LADDER_MID, 4);
        if !amp_maj[0] && amp_b[0].is_some() {
            let mut m1: f64 = 0.0;
            for i in (0..K_AMP_LADDER.len()).rev() {
                if (K_AMP_LADDER[i] as f32) < vis_max * (1.0_f32 - 1e-4) {
                    m1 = K_AMP_LADDER[i] as f64;
                    break;
                }
            }
            let v0 = amp_b[0].unwrap();
            if (v0 - m1) as f32 * zoom * half_h >= 18.0 {
                let y = self.level_to_y(plot, v0 as f32);
                let label_r = if y - K_LABEL_H - 1.0 >= plot.top() {
                    Rect::from_min_max(
                        Pos2::new(plot.right() - 52.0, y - K_LABEL_H - 1.0),
                        Pos2::new(plot.right() - K_TICK_RIGHT, y - 1.0),
                    )
                } else {
                    Rect::from_min_max(
                        Pos2::new(plot.right() - 52.0, y + 1.0),
                        Pos2::new(plot.right() - K_TICK_RIGHT, y + 1.0 + K_LABEL_H),
                    )
                };
                let buf = format!("{}", (20.0 * v0.log10() as f32).round() as i32);
                let fit = measure_text(painter, &buf, amp_font.clone(), theme.col_700(), label_r);
                let mut blocked = (!rect_empty(hov_amp) && fit.intersects(hov_amp))
                    || (!rect_empty(hov_time) && fit.intersects(hov_time));
                for ar in &amp_rs {
                    blocked |= fit.intersects(*ar);
                }
                if !blocked && amp_rs.len() < 6 {
                    painter.text(
                        Pos2::new(label_r.right() - K_TICK_RIGHT, fit.center().y),
                        Align2::RIGHT_CENTER,
                        &buf,
                        amp_font.clone(),
                        theme.col_700(),
                    );
                    amp_rs.push(fit);
                }
            }
        }

        // 时间标签：顶缘右侧，加密阶梯窗内前两颗
        let time_font = fonts::regular(14.0);
        let (tick_b, tick_maj) = largest_ladder(win_ms, &K_TIME_CELLS, &K_TIME_CELLS_MID, 4);
        let mut n_time_drawn = 0;
        for i in 0..4 {
            if n_time_drawn >= 2 {
                break;
            }
            let v = match tick_b[i] {
                Some(v) => v,
                None => break,
            };
            if !tick_maj[i] && i > 0 {
                continue;
            }
            let x = plot.right() - (v / win_ms) as f32 * plot.width();
            let buf = if v < 1000.0 {
                format!("-{}ms", v)
            } else {
                format!("-{:.1}s", v * 0.001)
            };
            let label_r = Rect::from_min_max(
                Pos2::new(x + 5.0, plot.top() + 2.0),
                Pos2::new(plot.right(), plot.top() + 2.0 + K_LABEL_H),
            );
            let fit = measure_text(painter, &buf, time_font.clone(), theme.col_700(), label_r);
            let mut blocked = (!rect_empty(hov_time) && fit.intersects(hov_time))
                || (!rect_empty(det_r) && fit.intersects(det_r));
            for ar in &amp_rs {
                blocked |= fit.intersects(*ar);
            }
            if blocked {
                continue;
            }
            painter.text(
                Pos2::new(fit.left(), plot.top() + 2.0),
                Align2::LEFT_TOP,
                &buf,
                time_font.clone(),
                theme.col_700(),
            );
            n_time_drawn += 1;
        }
    }

    /// SYNC 检测读数矩形（原版 `DetRect`:682-685）。
    fn det_rect(&self, plot: Rect) -> Rect {
        Rect::from_min_max(
            Pos2::new(plot.right() - 190.0, plot.bottom() - 2.0 - K_LABEL_H),
            Pos2::new(plot.right() - K_TICK_RIGHT, plot.bottom() - 2.0),
        )
    }

    /// SYNC 基频检测读数（原版 `DrawStatusReadout`:687-697）。
    fn draw_status_readout(&self, painter: &egui::Painter, plot: Rect, theme: &Theme) {
        if self.trig_source != trig::SYNC {
            return;
        }
        let info = if self.detected_freq > 0.0 {
            format!(
                "Det: {:.1} Hz ({:.2} ms)",
                self.detected_freq, self.detected_period_ms
            )
        } else {
            "Det: —".to_string()
        };
        let r = self.det_rect(plot);
        painter.text(
            Pos2::new(r.right() - K_TICK_RIGHT, r.center().y),
            Align2::RIGHT_CENTER,
            &info,
            fonts::regular(14.0),
            theme.col_700(),
        );
    }

    /// 波形绘制分发（原版 `DrawWaveforms`:662-679）。
    fn draw_waveforms(&mut self, painter: &egui::Painter, plot: Rect, theme: &Theme) {
        let (c_l, c_r, c_m) = theme.channel_colors();
        let mask = self.chan_mask.clamp(0, chan::L | chan::R | chan::M);
        let n_sel = ((mask & chan::L != 0) as usize)
            + ((mask & chan::R != 0) as usize)
            + ((mask & chan::M != 0) as usize);
        if n_sel == 0 {
            return;
        }
        let alpha = if n_sel > 1 { 210 } else { 255 };
        if mask & chan::R != 0 {
            self.draw_waveform_line(painter, plot, &self.disp_r, with_alpha(c_r, alpha));
        }
        if mask & chan::L != 0 {
            self.draw_waveform_line(painter, plot, &self.disp_l, with_alpha(c_l, alpha));
        }
        if mask & chan::M != 0 {
            self.draw_waveform_line(painter, plot, &self.disp_m, with_alpha(c_m, alpha));
        }
    }

    /// 单路波形：超像素列抽稀 min/max 填充带，否则折线 + 采样点。
    /// 原版 `DrawWaveformLine`:578-660。
    fn draw_waveform_line(
        &self,
        painter: &egui::Painter,
        plot: Rect,
        wave: &[f32],
        col: Color32,
    ) {
        let n = wave.len();
        if n < 2 {
            return;
        }
        let zoom = self.current_zoom();
        let cy = plot.center().y;
        let half_h = plot.height() * 0.48;
        let w = plot.width();

        if n > w as usize {
            let cols = (2.0_f32).max(w) as usize;
            let step_d = n as f64 / cols as f64;
            let mut phase = 0.0_f64;
            if self.disp_right_abs >= 0 {
                phase = ((n - 1) as f64 - self.disp_right_abs as f64) % step_d;
                if phase < 0.0 {
                    phase += step_d;
                }
            }
            let mut env_top: Vec<f32> = Vec::new();
            let mut env_bot: Vec<f32> = Vec::new();
            let mut env_x: Vec<f32> = Vec::new();
            let mut b_prev = 0.0_f64;
            let mut b = if phase > 0.0 { phase } else { step_d };
            while b_prev < n as f64 - 0.5 {
                let i0 = (b_prev.ceil() as usize).max(0);
                let i1 = (b.ceil() as usize).min(n);
                if i1 > i0 {
                    let mut min_v = wave[i0];
                    let mut max_v = wave[i0];
                    for i in i0 + 1..i1 {
                        min_v = min_v.min(wave[i]);
                        max_v = max_v.max(wave[i]);
                    }
                    env_top.push(cy - max_v.clamp(-1.15, 1.15) * zoom * half_h);
                    env_bot.push(cy - min_v.clamp(-1.15, 1.15) * zoom * half_h);
                    env_x.push(plot.left() + ((b_prev + b) * 0.5 / n as f64) as f32 * w);
                }
                b_prev = b;
                b += step_d;
            }
            let nb = env_x.len();
            if nb >= 2 {
                let mut pts: Vec<Pos2> = Vec::with_capacity(nb * 2);
                for px in 0..nb {
                    pts.push(Pos2::new(env_x[px], env_top[px]));
                }
                for px in (0..nb).rev() {
                    pts.push(Pos2::new(env_x[px], env_bot[px]));
                }
                painter.add(Shape::convex_polygon(pts, col, Stroke::NONE));
            }
        } else {
            let mut pts: Vec<Pos2> = Vec::with_capacity(n);
            for i in 0..n {
                let frac = i as f32 / (n - 1) as f32;
                let x = plot.left() + frac * w;
                let y = cy - wave[i].clamp(-1.15, 1.15) * zoom * half_h;
                pts.push(Pos2::new(x, y));
            }
            painter.add(Shape::line(pts.clone(), Stroke::new(1.5_f32, col)));
            if n <= 80 {
                for p in pts {
                    painter.circle_filled(p, 1.5, col);
                }
            }
        }
    }

    // ==================== hover（原版 :700-767） ====================

    fn compute_hover(
        &mut self,
        painter: &egui::Painter,
        theme: &Theme,
        plot: Rect,
        det_r: Rect,
    ) -> (Rect, Rect) {
        let win_ms = self.window_sec * 1000.0;
        let dt_ms = ((self.hover_x - plot.right()) / plot.width() * win_ms as f32) as f32;
        if dt_ms.abs() >= 1000.0 {
            self.hover_time_buf = format!("{:+.2} s", dt_ms * 0.001);
        } else {
            self.hover_time_buf = format!("{:+.1} ms", dt_ms);
        }

        self.hover_freq_buf.clear();
        if dt_ms.abs() > 0.01 {
            let f_eq = 1000.0 / dt_ms.abs() as f64;
            if (20.0..=20_000.0).contains(&f_eq) {
                let note = freq_to_note_name(f_eq);
                self.hover_freq_buf = format!("{:.0} Hz ({})", f_eq, note);
            }
        }
        self.hover_amp_buf = format!("{:+.2}", self.y_to_level(plot, self.hover_y));

        let t_font = fonts::regular(14.0);
        let r_t0 = Rect::from_min_max(
            Pos2::new(self.hover_x, plot.top() + 2.0),
            Pos2::new(self.hover_x, plot.top() + 2.0 + K_LABEL_H),
        );
        let w_t = measure_text(painter, &self.hover_time_buf, t_font.clone(), Color32::BLACK, r_t0).width();
        let w_f = if self.hover_freq_buf.is_empty() {
            0.0
        } else {
            measure_text(painter, &self.hover_freq_buf, t_font.clone(), Color32::BLACK, r_t0).width()
        };

        let t_fits_l = self.hover_x - K_GAP - w_t >= plot.left();
        let f_fits_r = w_f > 0.0 && self.hover_x + K_GAP + w_f <= plot.right();
        let (mut r_t, mut r_f) = if t_fits_l && f_fits_r {
            (
                Rect::from_min_max(
                    Pos2::new(self.hover_x - K_GAP - w_t, plot.top() + 2.0),
                    Pos2::new(self.hover_x - K_GAP, plot.top() + 2.0 + K_LABEL_H),
                ),
                Rect::from_min_max(
                    Pos2::new(self.hover_x + K_GAP, plot.top() + 2.0),
                    Pos2::new(self.hover_x + K_GAP + w_f, plot.top() + 2.0 + K_LABEL_H),
                ),
            )
        } else if !t_fits_l {
            let rt = Rect::from_min_max(
                Pos2::new(self.hover_x + K_GAP, plot.top() + 2.0),
                Pos2::new(self.hover_x + K_GAP + w_t, plot.top() + 2.0 + K_LABEL_H),
            );
            let rf = Rect::from_min_max(
                Pos2::new(rt.right() + 2.0, plot.top() + 2.0),
                Pos2::new(rt.right() + 2.0 + w_f, plot.top() + 2.0 + K_LABEL_H),
            );
            (rt, rf)
        } else {
            let rt = Rect::from_min_max(
                Pos2::new(self.hover_x - K_GAP - w_t, plot.top() + 2.0),
                Pos2::new(self.hover_x - K_GAP, plot.top() + 2.0 + K_LABEL_H),
            );
            let rf = Rect::from_min_max(
                Pos2::new(rt.left() - 2.0 - w_f, plot.top() + 2.0),
                Pos2::new(rt.left() - 2.0, plot.top() + 2.0 + K_LABEL_H),
            );
            (rt, rf)
        };

        // 撞上 SYNC 检测读数占位时整体下移
        if !rect_empty(det_r) && (r_t.intersects(det_r) || r_f.intersects(det_r)) {
            let dy = det_r.height() + 2.0;
            r_t = Rect::from_min_max(
                Pos2::new(r_t.left(), r_t.top() + dy),
                Pos2::new(r_t.right(), r_t.bottom() + dy),
            );
            r_f = Rect::from_min_max(
                Pos2::new(r_f.left(), r_f.top() + dy),
                Pos2::new(r_f.right(), r_f.bottom() + dy),
            );
        }
        let _ = r_f;

        let mut r_amp = if self.hover_y - K_LABEL_H - 1.0 >= plot.top() {
            Rect::from_min_max(
                Pos2::new(plot.right() - 52.0, self.hover_y - K_LABEL_H - 1.0),
                Pos2::new(plot.right() - K_TICK_RIGHT, self.hover_y - 1.0),
            )
        } else {
            Rect::from_min_max(
                Pos2::new(plot.right() - 52.0, self.hover_y + 1.0),
                Pos2::new(plot.right() - K_TICK_RIGHT, self.hover_y + 1.0 + K_LABEL_H),
            )
        };
        if !rect_empty(det_r) && r_amp.intersects(det_r) {
            r_amp = Rect::from_min_max(
                Pos2::new(plot.right() - 52.0, self.hover_y + 1.0),
                Pos2::new(plot.right() - K_TICK_RIGHT, self.hover_y + 1.0 + K_LABEL_H),
            );
        }
        let amp_font = fonts::regular(14.0);
        r_amp = measure_text(painter, &self.hover_amp_buf, amp_font, theme.col_700(), r_amp);

        (r_t, r_amp)
    }

    fn draw_hover(&self, painter: &egui::Painter, plot: Rect, theme: &Theme) {
        painter.line_segment(
            [
                Pos2::new(self.hover_x, plot.top()),
                Pos2::new(self.hover_x, plot.bottom()),
            ],
            Stroke::new(1.0_f32, theme.col_700()),
        );
        painter.line_segment(
            [
                Pos2::new(plot.left(), self.hover_y),
                Pos2::new(plot.right(), self.hover_y),
            ],
            Stroke::new(1.0_f32, theme.col_700()),
        );
        painter.text(
            Pos2::new(self.hover_x, plot.top() + 2.0),
            Align2::LEFT_TOP,
            &self.hover_time_buf,
            fonts::regular(14.0),
            theme.col_700(),
        );
        if !self.hover_freq_buf.is_empty() {
            painter.text(
                Pos2::new(self.hover_x, plot.top() + 2.0),
                Align2::LEFT_TOP,
                &self.hover_freq_buf,
                fonts::regular(14.0),
                theme.col_700(),
            );
        }
        painter.text(
            Pos2::new(plot.right() - K_TICK_RIGHT, self.hover_y - 1.0),
            Align2::RIGHT_BOTTOM,
            &self.hover_amp_buf,
            fonts::regular(14.0),
            theme.col_700(),
        );
    }
}

// ==================== 模块级工具 ====================

fn with_alpha(c: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), alpha)
}

/// 等价原版 IRECT::Empty()：零面积矩形视为空。
fn rect_empty(r: Rect) -> bool {
    r == Rect::ZERO
}

/// 合并主档/中间档阶梯，取量程内最大 n 档（降序；不足填 None）。
/// 原版 `LargestLadder`:453-473。
fn largest_ladder(
    scale: f64,
    major: &[f64],
    mid: &[f64],
    n: usize,
) -> ([Option<f64>; 4], [bool; 4]) {
    let mut i_m = major.len() as i32 - 1;
    let mut i_i = mid.len() as i32 - 1;
    let mut out_v = [None; 4];
    let mut out_maj = [false; 4];
    let mut found = 0;
    while found < n && (i_m >= 0 || i_i >= 0) {
        let take_major = i_m >= 0 && (i_i < 0 || major[i_m as usize] > mid[i_i as usize]);
        let v = if take_major {
            major[i_m as usize]
        } else {
            mid[i_i as usize]
        };
        if take_major {
            i_m -= 1;
        } else {
            i_i -= 1;
        }
        if v < scale * (1.0 - 1e-4) {
            out_v[found] = Some(v);
            out_maj[found] = take_major;
            found += 1;
        }
    }
    (out_v, out_maj)
}

/// 实测文本矩形（等价 iPlug2 `MeasureText`）。
fn measure_text(
    painter: &egui::Painter,
    text: &str,
    font: egui::FontId,
    color: Color32,
    label_r: Rect,
) -> Rect {
    let galley = painter.layout_no_wrap(text.to_owned(), font, color);
    let size = galley.size();
    Rect::from_min_max(
        Pos2::new(label_r.left(), label_r.top()),
        Pos2::new(label_r.left() + size.x, label_r.top() + size.y),
    )
}

/// 音名换算（原版 `FreqToNoteName`:912-916）。
fn freq_to_note_name(hz: f64) -> String {
    const NAMES: [&str; 12] = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    let nn = (12.0 * (hz.max(1.0) / 440.0).log2()).round() as i32 + 69;
    format!("{}{}", NAMES[(((nn % 12) + 12) % 12) as usize], nn / 12 - 1)
}

/// 基-2 FFT（原版 `FftRadix2`:879-910，正向，角度 -2π/len）。
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

// 引用保留：strings 供集成层 tooltip 查表（本控件内部读数用原版字面量）。
#[allow(unused_imports)]
use strings as _strings;
