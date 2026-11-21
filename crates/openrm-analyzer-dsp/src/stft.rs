//! SpectrumSTFT — 短时傅里叶变换频谱分析引擎。
//!
//! 逐行移植自 `plugins/common/dsp/SpectrumSTFT.h`（三插件共用，Analyzer 实例化为
//! `SpectrumSTFT<3>`，默认 4096 / 4× 重叠 / Hann / kCalDouble）。
//!
//! 音频线程逐样本攒 hop，攒满一个 hop 即做 FFT 出帧（等价原版 ISender 队列 +
//! UI 线程 PrepareDataForUI 的分工，这里用「最新一帧 + hop 计数」等价替代）。
//! 输出一律线性幅度；dB 换算由 UI 层负责。

use realfft::num_complex::Complex;
use realfft::{RealFftPlanner, RealToComplex};
use std::sync::Arc;

use crate::frame::EngineFrame;

const MAX_FFT_SIZE: usize = 8192;

/// 窗类型（与原版 `EWindowType` 一一对应）。
/// 0=Hann，1=5 项 Blackman-Harris（Clean），2=4 项 Blackman-Harris（BH4）。
pub const WINDOW_HANN: i32 = 0;
pub const WINDOW_CLEAN: i32 = 1;
pub const WINDOW_BH4: i32 = 2;

/// 功率标定（Analyzer 固定 kCalDouble；档位保留以与原版枚举对齐）。
/// kCalHalf：非 DC 0.5 / DC 0.5 → 满幅正弦读 0.707（-3 dB）。
/// kCalDouble：非 DC 2.0 / DC 1.0 → 满幅正弦读 1.414（+3 dB）。
const CAL_HALF: i32 = 0;
const CAL_DOUBLE: i32 = 1;

pub struct StftAnalyzer {
    fft_size: usize,
    overlap: usize,
    hop: usize,
    num_bins: usize,
    buf_count: usize,
    chan_tri: i32,
    window_type: i32,
    calibration: i32,
    cal_dc: f32,
    cal_bins: f32,
    scaling: f32,
    window: Vec<f32>,
    history: [[f32; MAX_FFT_SIZE]; 3],
    pending: [[f32; MAX_FFT_SIZE]; 3],
    windowed: Vec<f32>,
    r2c: Arc<dyn RealToComplex<f32>>,
    complex: Vec<Complex<f32>>,
    scratch: Vec<Complex<f32>>,
    frame: Option<EngineFrame>,
}

impl StftAnalyzer {
    pub fn new() -> Self {
        let mut me = Self {
            fft_size: 4096,
            overlap: 4,
            hop: 1024,
            num_bins: 2048,
            buf_count: 0,
            chan_tri: 0,
            window_type: WINDOW_HANN,
            calibration: CAL_DOUBLE,
            cal_dc: 1.0,
            cal_bins: 2.0,
            scaling: 0.0,
            window: Vec::new(),
            history: [[0.0; MAX_FFT_SIZE]; 3],
            pending: [[0.0; MAX_FFT_SIZE]; 3],
            windowed: Vec::new(),
            r2c: Arc::from(RealFftPlanner::<f32>::new().plan_fft_forward(4096)),
            complex: Vec::new(),
            scratch: Vec::new(),
            frame: None,
        };
        me.set_calibration(CAL_DOUBLE);
        me.set_fft_size_and_overlap(4096, 4, WINDOW_HANN);
        me
    }

    pub fn set_sample_rate(&mut self, _sr: f64) {
        // 原版 SpectrumSTFT 不依赖采样率（bin 频率由 UI 按 sr/N 计算）。
    }

    /// 对应原版 `SetCalibration`。
    fn set_calibration(&mut self, calibration: i32) {
        self.calibration = calibration;
        if calibration == CAL_HALF {
            self.cal_dc = 0.5;
            self.cal_bins = 0.5;
        } else {
            self.cal_dc = 1.0;
            self.cal_bins = 2.0;
        }
    }

    /// 对应原版 `SetFFTSizeAndOverlap`。
    fn set_fft_size_and_overlap(&mut self, fft_size: usize, overlap: usize, window_type: i32) {
        self.fft_size = fft_size.clamp(64, MAX_FFT_SIZE);
        self.overlap = overlap.max(1);
        self.hop = self.fft_size / self.overlap;
        self.num_bins = self.fft_size / 2;
        if window_type >= 0 {
            self.window_type = window_type.clamp(0, 2);
        }
        self.rebuild_window();

        let mut planner = RealFftPlanner::<f32>::new();
        self.r2c = Arc::from(planner.plan_fft_forward(self.fft_size));
        self.complex = self.r2c.make_output_vec();
        self.scratch = self.r2c.make_scratch_vec();
        self.windowed = vec![0.0; self.fft_size];

        for h in self.history.iter_mut() {
            h.fill(0.0);
        }
        self.buf_count = 0;
    }

    /// 对应原版 `SetFFTSize`；重叠度按 Analyzer `FFTOverlapForSize` = max(1, N/1024)。
    pub fn set_fft_size(&mut self, fft_size: usize) {
        let overlap = (fft_size / 1024).max(1);
        self.set_fft_size_and_overlap(fft_size, overlap, -1);
    }

    /// 对应原版 `SetWindowType`。
    pub fn set_window(&mut self, window_type: i32) {
        let new_type = window_type.clamp(0, 2);
        if new_type != self.window_type {
            self.window_type = new_type;
            self.rebuild_window();
        }
    }

    pub fn num_bins(&self) -> usize {
        self.num_bins
    }

    pub fn hop(&self) -> usize {
        self.hop
    }

    /// 测试/比对用：当前窗系数与归一化系数。
    #[allow(dead_code)]
    pub fn window_coeffs(&self) -> &[f32] {
        &self.window
    }

    #[allow(dead_code)]
    pub fn scaling(&self) -> f32 {
        self.scaling
    }

    /// 0=PWR, 1=LR, 2=SUM（与原版 mChanTri 同义；PWR/LR 都出 L/R，SUM 出 M）。
    pub fn set_channel_mode(&mut self, chan_tri: i32) {
        self.chan_tri = chan_tri.clamp(0, 2);
    }

    /// 对应原版 `RebuildWindow`。窗系数与归一化系数逐字照抄。
    fn rebuild_window(&mut self) {
        let n = self.fft_size;
        let m = (n - 1) as f32;
        self.window.resize(n, 0.0);
        let mut sum = 0.0_f64;
        if self.window_type == WINDOW_BH4 {
            const A0: f32 = 0.35875;
            const A1: f32 = 0.48829;
            const A2: f32 = 0.14128;
            const A3: f32 = 0.01168;
            for (i, w) in self.window.iter_mut().enumerate() {
                let theta = 2.0_f32 * std::f32::consts::PI * i as f32 / m;
                *w = A0 - A1 * theta.cos() + A2 * (2.0 * theta).cos() - A3 * (3.0 * theta).cos();
                sum += *w as f64;
            }
        } else if self.window_type == WINDOW_CLEAN {
            const A0: f32 = 0.3230984033;
            const A1: f32 = 0.4714310503;
            const A2: f32 = 0.1756446367;
            const A3: f32 = 0.0285582447;
            const A4: f32 = 0.001267665;
            for (i, w) in self.window.iter_mut().enumerate() {
                let theta = 2.0_f32 * std::f32::consts::PI * i as f32 / m;
                *w = A0 - A1 * theta.cos() + A2 * (2.0 * theta).cos()
                    - A3 * (3.0 * theta).cos() + A4 * (4.0 * theta).cos();
                sum += *w as f64;
            }
        } else {
            for (i, w) in self.window.iter_mut().enumerate() {
                let theta = 2.0_f32 * std::f32::consts::PI * i as f32 / m;
                *w = 0.5 * (1.0 - theta.cos());
                sum += *w as f64;
            }
        }
        self.scaling = (sum * sum) as f32;
    }

    /// 对应原版 `ProcessBlock`：逐样本攒 hop，攒满即出一帧。
    pub fn process_block(&mut self, l: &[f32], r: &[f32], m: &[f32]) {
        let n = l.len().min(r.len()).min(m.len());
        for s in 0..n {
            self.pending[0][self.buf_count] = l[s];
            self.pending[1][self.buf_count] = r[s];
            self.pending[2][self.buf_count] = m[s];
            self.buf_count += 1;
            if self.buf_count == self.hop {
                self.analyze();
                self.buf_count = 0;
            }
        }
    }

    /// 对应原版 `PrepareDataForUI`（ISenderData 帧内三路幅度谱）。
    fn analyze(&mut self) {
        let need_l = self.chan_tri != 2;
        let need_r = self.chan_tri != 2;
        let need_sum = self.chan_tri == 2;
        let hop = self.hop;
        let n = self.num_bins;

        let mut vals: Vec<[f32; 3]> = vec![[0.0; 3]; n];

        for c in 0..3 {
            let active = match c {
                0 => need_l,
                1 => need_r,
                _ => need_sum,
            };
            if !active {
                continue;
            }

            // 历史缓冲左移 hop，尾部追加本 hop 样本（memmove/memcpy 等价）。
            self.history[c].copy_within(hop..self.fft_size, 0);
            let tail = self.fft_size - hop;
            self.history[c][tail..tail + hop].copy_from_slice(&self.pending[c][..hop]);

            for i in 0..self.fft_size {
                self.windowed[i] = self.history[c][i] * self.window[i];
            }
            let _ = self
                .r2c
                .process_with_scratch(&mut self.windowed, &mut self.complex, &mut self.scratch);

            let inv_bins = self.cal_bins / self.scaling;
            let inv_dc = self.cal_dc / self.scaling;

            // WDL 实数 FFT 前向输出 = 标准 DFT 的 2 倍（r2 与 quart bin 各乘 2）；
            // realfft 走标准 DFT，这里补回同一常数因子，幅度标定与原版逐位对齐。
            const WDL_SCALE: f32 = 2.0;

            let dc = self.complex[0].re;
            vals[0][c] = (dc * dc * inv_dc).sqrt() * WDL_SCALE;

            for i in 1..n {
                let cmp = self.complex[i];
                let re = cmp.re;
                let im = cmp.im;
                vals[i][c] = ((re * re + im * im) * inv_bins).sqrt() * WDL_SCALE;
            }
        }

        self.frame = Some(EngineFrame { vals, freqs: Vec::new() });
    }

    pub fn take_frame(&mut self) -> Option<EngineFrame> {
        self.frame.take()
    }

    /// 复位分析侧运行态（窗历史 + 攒包计数），等价冷启动。
    pub fn reset(&mut self) {
        for h in self.history.iter_mut() {
            h.fill(0.0);
        }
        self.buf_count = 0;
        self.frame = None;
    }
}

impl Default for StftAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}
