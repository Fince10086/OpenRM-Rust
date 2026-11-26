//! ORM Fusion 的 DSP 实现（沿用 GRM Fusion 的公开 DSP 规范）。
//!
//! 只以 `fusion/DSP.md` 为规范：N 点 Hann 分析/合成、N/4 跳距、
//! 频谱环形历史里的整帧搬运延迟、频带内延迟插值、边缘升余弦锥化、
//! 可选逐 bin 反馈，以及频谱域干/湿混合。

use std::collections::VecDeque;
use std::sync::Arc;

use realfft::num_complex::Complex32;
use realfft::{ComplexToReal, RealFftPlanner, RealToComplex};

pub const BAND_SIZES: [usize; 10] = [
    256, 512, 1024, 2048, 4096, 8192, 16384, 32768, 65536, 131072,
];
pub const NUM_BARS: usize = 8;
pub const HISTORY_SAMPLES: f64 = 1_764_000.0;
const DRY_TRIM: f32 = 0.707_000_017_166;
const FB_GATE: f32 = 0.01;
const FB_ENERGY_MAX: f32 = 0.8;

#[derive(Clone, Debug)]
pub struct Bar {
    pub f1: f64,
    pub f2: f64,
    pub a1: f64,
    pub a2: f64,
    pub amp_slider: f32,
    pub bar_amp: f32,
    pub feedback: f32,
    pub pan: f32,
    pub width: f32,
    pub smooth: f32,
    pub active: bool,
    pub mute: bool,
}

impl Default for Bar {
    fn default() -> Self {
        Self {
            f1: 0.075,
            f2: 0.175,
            a1: 0.10,
            a2: 0.10,
            amp_slider: 0.9,
            bar_amp: amplitude_from_slider(0.9),
            feedback: 0.0,
            pan: 0.75,
            width: 0.95,
            smooth: 0.5,
            active: false,
            mute: false,
        }
    }
}

/// `FloatToAmplitude`：滑块 0..1 → 线性幅度。默认 0.9 对应约 −4 dB。
#[inline]
pub fn amplitude_from_slider(v: f32) -> f32 {
    10f32.powf(2.0 * (v - 1.0))
}

/// `FloatToDelay`：参数 0..1 → 秒，再除以 10 s 历史深度得到归一化 a。
#[inline]
pub fn delay_norm_from_param(v: f32) -> f64 {
    (0.001 + 9.999 * v.clamp(0.0, 1.0) as f64) / 10.0
}

#[inline]
pub fn delay_sec_from_norm(a: f64) -> f64 {
    (a * 10.0).max(0.001)
}

impl Bar {
    pub fn set_amplitude(&mut self, v: f32) {
        self.amp_slider = v.clamp(0.0, 1.0);
        self.bar_amp = amplitude_from_slider(self.amp_slider);
    }

    pub fn set_delay_param(&mut self, v: f32) {
        let target = delay_norm_from_param(v);
        let center = (self.a1 + self.a2) * 0.5;
        let d = target - center;
        self.a1 += d;
        self.a2 += d;
        self.check_values();
    }

    pub fn set_freq_param(&mut self, v: f32) {
        let target = v.clamp(0.0, 1.0) as f64;
        let center = (self.f1 + self.f2) * 0.5;
        let d = target - center;
        self.f1 += d;
        self.f2 += d;
        self.check_values();
    }

    pub fn set_length_param(&mut self, v: f32) {
        let target = v.clamp(0.0, 0.5) as f64;
        let half = self.length();
        if half <= 1e-12 {
            return;
        }
        let center_f = (self.f1 + self.f2) * 0.5;
        let center_a = (self.a1 + self.a2) * 0.5;
        let scale = target / half;
        self.f1 = center_f + (self.f1 - center_f) * scale;
        self.f2 = center_f + (self.f2 - center_f) * scale;
        self.a1 = center_a + (self.a1 - center_a) * scale;
        self.a2 = center_a + (self.a2 - center_a) * scale;
        self.check_values();
    }

    pub fn set_angle_param(&mut self, v: f32) {
        let target = (v.clamp(0.0, 1.0) as f64) * std::f64::consts::PI;
        let half = self.length();
        if half <= 1e-12 {
            return;
        }
        let center_f = (self.f1 + self.f2) * 0.5;
        let center_a = (self.a1 + self.a2) * 0.5;
        let df = half * target.cos();
        let da = half * target.sin();
        self.f1 = center_f - df;
        self.f2 = center_f + df;
        self.a1 = center_a - da;
        self.a2 = center_a + da;
        self.check_values();
    }

    pub fn length(&self) -> f64 {
        ((self.f2 - self.f1).powi(2) + (self.a2 - self.a1).powi(2)).sqrt() * 0.5
    }

    pub fn angle(&self) -> f64 {
        let df = self.f2 - self.f1;
        let da = self.a2 - self.a1;
        let a = da.atan2(df);
        if a < 0.0 {
            a + std::f64::consts::PI
        } else {
            a
        }
    }

    fn check_values(&mut self) {
        if self.f1 > self.f2 {
            std::mem::swap(&mut self.f1, &mut self.f2);
            std::mem::swap(&mut self.a1, &mut self.a2);
        }
        self.f1 = self.f1.clamp(0.0, 1.0);
        self.f2 = self.f2.clamp(0.0, 1.0);
        self.a1 = self.a1.clamp(0.0, 1.0);
        self.a2 = self.a2.clamp(0.0, 1.0);
    }

    #[inline]
    pub fn delay_at(&self, x: f32) -> f64 {
        if self.f1 == self.f2 {
            return self.a1.min(self.a2);
        }
        let r = (x.clamp(0.0, 1.0) as f64).sqrt(); // pow(x, 1/2)
        if self.f1 > r {
            self.a1
        } else if r > self.f2 {
            self.a2
        } else {
            self.a1 + (self.a2 - self.a1) * (r - self.f1) / (self.f2 - self.f1)
        }
    }

    #[inline]
    pub fn edge_taper(&self, x: f32) -> f32 {
        let f1c = self.f1.clamp(0.0, 1.0);
        let f2c = self.f2.clamp(0.0, 1.0);
        if f1c == f2c {
            return 0.0;
        }
        let mut g = x as f64 * (f2c - f1c) / (self.f2 - self.f1);
        if self.f1 < 0.0 {
            g -= self.f1 / (self.f2 - self.f1);
        }
        let s = self.smooth as f64;
        if s <= 0.0 {
            return 1.0;
        }
        if g < 0.5 * s {
            (std::f64::consts::PI * g / s).sin() as f32
        } else if g > 1.0 - 0.5 * s {
            (std::f64::consts::PI * (1.0 - g) / s).sin() as f32
        } else {
            1.0
        }
    }
}

#[inline]
fn pan_weights(theta: f32) -> (f32, f32) {
    let theta = theta.clamp(-1.0, 1.0);
    ((1.0 - theta) * 0.5, (1.0 + theta) * 0.5)
}

#[inline]
fn dry_mix(wet: f32, dry: f32, mix: f32) -> f32 {
    wet * mix + dry * (1.0 - mix) * DRY_TRIM
}

struct Fft {
    fwd: Arc<dyn RealToComplex<f32>>,
    inv: Arc<dyn ComplexToReal<f32>>,
    scratch_fwd: Vec<Complex32>,
}

impl Fft {
    fn new(n: usize) -> Self {
        let mut planner = RealFftPlanner::<f32>::new();
        let fwd = planner.plan_fft_forward(n);
        let inv = planner.plan_fft_inverse(n);
        let scratch_fwd = fwd.make_scratch_vec();
        Self {
            fwd,
            inv,
            scratch_fwd,
        }
    }
}

pub struct FusionCore {
    pub bars: Vec<Bar>,
    pub gain_v: f32,
    pub mix: f32,
    pub band_index: usize,
    pub n: usize,
    hop: usize,
    half: usize,
    window: Vec<f32>,
    frames: usize,
    hist_l: Vec<Complex32>,
    hist_r: Vec<Complex32>,
    w_hist: usize,
    fft: Fft,
    pending_l: VecDeque<f32>,
    pending_r: VecDeque<f32>,
    ola_l: Vec<f32>,
    ola_r: Vec<f32>,
    ola_pos: usize,
    out_l: VecDeque<f32>,
    out_r: VecDeque<f32>,
    fft_in: Vec<f32>,
    spec_in_l: Vec<Complex32>,
    spec_in_r: Vec<Complex32>,
    spec_out_l: Vec<Complex32>,
    spec_out_r: Vec<Complex32>,
    scratch_inv: Vec<Complex32>,
    fft_out_l: Vec<f32>,
    fft_out_r: Vec<f32>,
    fft_tmp: Vec<Complex32>,
    packed_tmp: Vec<Complex32>,
    ifft_spec: Vec<Complex32>,
    frame_l: Vec<f32>,
    frame_r: Vec<f32>,
    stereo_in: bool,
}

impl Default for FusionCore {
    fn default() -> Self {
        Self::new()
    }
}

impl FusionCore {
    pub fn new() -> Self {
        let mut bars = Vec::with_capacity(NUM_BARS);
        for i in 0..NUM_BARS {
            let mut bar = Bar::default();
            bar.f1 = 0.075 + i as f64 * 0.05;
            bar.f2 = bar.f1 + 0.10;
            bar.active = i == 0;
            bars.push(bar);
        }
        let n = BAND_SIZES[0];
        let hop = n / 4;
        let half = n / 2;
        let frames = (HISTORY_SAMPLES / n as f64) as usize;
        let fft = Fft::new(n);
        let mut core = Self {
            bars,
            gain_v: 0.8,
            mix: 1.0,
            band_index: 0,
            n,
            hop,
            half,
            window: Vec::new(),
            frames,
            hist_l: vec![Complex32::default(); frames * half],
            hist_r: vec![Complex32::default(); frames * half],
            w_hist: 0,
            fft,
            pending_l: VecDeque::new(),
            pending_r: VecDeque::new(),
            ola_l: vec![0.0; n],
            ola_r: vec![0.0; n],
            ola_pos: 0,
            out_l: VecDeque::new(),
            out_r: VecDeque::new(),
            fft_in: vec![0.0; n],
            spec_in_l: vec![Complex32::default(); half],
            spec_in_r: vec![Complex32::default(); half],
            spec_out_l: vec![Complex32::default(); half],
            spec_out_r: vec![Complex32::default(); half],
            scratch_inv: vec![Complex32::default(); n],
            fft_out_l: vec![0.0; n],
            fft_out_r: vec![0.0; n],
            fft_tmp: vec![Complex32::default(); half + 1],
            packed_tmp: vec![Complex32::default(); half],
            ifft_spec: vec![Complex32::default(); half + 1],
            frame_l: vec![0.0; n],
            frame_r: vec![0.0; n],
            stereo_in: true,
        };
        core.rebuild_window();
        core
    }

    pub fn set_bands_index(&mut self, index: usize) {
        let index = index.min(BAND_SIZES.len() - 1);
        if index == self.band_index && !self.window.is_empty() {
            return;
        }
        self.band_index = index;
        self.n = BAND_SIZES[index];
        self.hop = self.n / 4;
        self.half = self.n / 2;
        self.frames = ((HISTORY_SAMPLES / self.n as f64) as usize).max(1);
        self.fft = Fft::new(self.n);
        self.hist_l = vec![Complex32::default(); self.frames * self.half];
        self.hist_r = vec![Complex32::default(); self.frames * self.half];
        self.w_hist = 0;
        self.pending_l.clear();
        self.pending_r.clear();
        self.ola_l = vec![0.0; self.n];
        self.ola_r = vec![0.0; self.n];
        self.ola_pos = 0;
        self.out_l.clear();
        self.out_r.clear();
        self.fft_in = vec![0.0; self.n];
        self.spec_in_l = vec![Complex32::default(); self.half];
        self.spec_in_r = vec![Complex32::default(); self.half];
        self.spec_out_l = vec![Complex32::default(); self.half];
        self.spec_out_r = vec![Complex32::default(); self.half];
        self.scratch_inv = vec![Complex32::default(); self.n];
        self.fft_out_l = vec![0.0; self.n];
        self.fft_out_r = vec![0.0; self.n];
        self.fft_tmp = vec![Complex32::default(); self.half + 1];
        self.packed_tmp = vec![Complex32::default(); self.half];
        self.ifft_spec = vec![Complex32::default(); self.half + 1];
        self.frame_l = vec![0.0; self.n];
        self.frame_r = vec![0.0; self.n];
        self.rebuild_window();
    }

    fn rebuild_window(&mut self) {
        let n = self.n;
        self.window = (0..n)
            .map(|k| {
                (0.5 * (1.0
                    - (2.0 * std::f64::consts::PI * k as f64 / (n - 1).max(1) as f64).cos()))
                    as f32
            })
            .collect();
    }

    pub fn reset(&mut self) {
        self.hist_l.fill(Complex32::default());
        self.hist_r.fill(Complex32::default());
        self.w_hist = 0;
        self.pending_l.clear();
        self.pending_r.clear();
        self.ola_l.fill(0.0);
        self.ola_r.fill(0.0);
        self.ola_pos = 0;
        self.out_l.clear();
        self.out_r.clear();
    }

    pub fn set_gain(&mut self, v: f32) {
        self.gain_v = v.clamp(0.0, 1.0);
    }

    pub fn gain_linear(&self) -> f32 {
        10f32.powf(-4.8 + 6.0 * self.gain_v)
    }

    pub fn set_mix(&mut self, v: f32) {
        self.mix = v.clamp(0.0, 1.0);
    }

    pub fn set_smoothness(&mut self, v: f32) {
        for bar in &mut self.bars {
            bar.smooth = v.clamp(0.0, 1.0);
        }
    }

    pub fn process(&mut self, in_l: &[f32], in_r: &[f32], out_l: &mut [f32], out_r: &mut [f32]) {
        let n = in_l.len().min(in_r.len()).min(out_l.len()).min(out_r.len());
        if self.mix <= 1e-6 {
            out_l[..n].copy_from_slice(&in_l[..n]);
            out_r[..n].copy_from_slice(&in_r[..n]);
            return;
        }
        self.stereo_in = true;
        self.pending_l.extend(in_l[..n].iter().copied());
        self.pending_r.extend(in_r[..n].iter().copied());

        while self.pending_l.len() >= self.n {
            let mut frame_l = std::mem::take(&mut self.frame_l);
            let mut frame_r = std::mem::take(&mut self.frame_r);
            for k in 0..self.n {
                frame_l[k] = self.pending_l[k];
                frame_r[k] = self.pending_r[k];
            }
            self.one_frame(&frame_l, &frame_r);
            self.frame_l = frame_l;
            self.frame_r = frame_r;
            // 输入侧净前进 H，留下 3N/4 与下一帧重叠。
            self.pending_l.drain(..self.hop);
            self.pending_r.drain(..self.hop);
            self.emit_hop();
        }

        for i in 0..n {
            out_l[i] = self.out_l.pop_front().unwrap_or(0.0);
            out_r[i] = self.out_r.pop_front().unwrap_or(0.0);
        }
    }

    fn emit_hop(&mut self) {
        let n = self.n;
        let mut pos = self.ola_pos;
        for _ in 0..self.hop {
            self.out_l.push_back(self.ola_l[pos]);
            self.out_r.push_back(self.ola_r[pos]);
            self.ola_l[pos] = 0.0;
            self.ola_r[pos] = 0.0;
            pos += 1;
            if pos == n {
                pos = 0;
            }
        }
        self.ola_pos = pos;
    }

    fn one_frame(&mut self, seg_l: &[f32], seg_r: &[f32]) {
        let n = self.n;
        let half = self.half;

        // 分析：×Hann → rFFT → 打包成 N/2 个复 bin（bin0 = DC + i·Nyquist）。
        for k in 0..n {
            self.fft_in[k] = seg_l[k] * self.window[k];
        }
        self.forward_into(false);
        for k in 0..n {
            self.fft_in[k] = seg_r[k] * self.window[k];
        }
        self.forward_into(true);

        // 先存本帧，再按帧号取过去。
        let w = self.w_hist;
        let base = w * half;
        self.hist_l[base..base + half].copy_from_slice(&self.spec_in_l);
        self.hist_r[base..base + half].copy_from_slice(&self.spec_in_r);
        self.w_hist = (w + 1) % self.frames;
        let current = w;

        self.spec_out_l.fill(Complex32::default());
        self.spec_out_r.fill(Complex32::default());

        let gain = self.gain_linear();

        for bar in &self.bars {
            if !bar.active || bar.mute {
                continue;
            }
            let theta_a = (2.0 * (bar.pan - 0.5) - bar.width).clamp(-1.0, 1.0);
            let theta_b = (2.0 * (bar.pan - 0.5) + bar.width).clamp(-1.0, 1.0);
            let (a0, a1) = pan_weights(theta_a);
            let (b0, b1) = pan_weights(theta_b);

            let mut b1_idx = (bar.f1.clamp(0.0, 1.0).powi(2) * half as f64) as usize;
            let mut b2_idx = (bar.f2.clamp(0.0, 1.0).powi(2) * half as f64) as usize;
            b1_idx = b1_idx.min(half - 1);
            b2_idx = b2_idx.min(half - 1);
            if b1_idx == b2_idx {
                if b2_idx + 1 < half {
                    b2_idx += 1;
                } else if b1_idx > 0 {
                    b1_idx -= 1;
                } else {
                    continue;
                }
            }
            if b1_idx > b2_idx {
                std::mem::swap(&mut b1_idx, &mut b2_idx);
            }
            let n_bins = b2_idx - b1_idx;
            if n_bins == 0 {
                continue;
            }

            let g = bar.bar_amp * gain;
            let fb = bar.feedback;
            let frames = self.frames;
            let prev_idx = (current + frames - 1) % frames;

            for j in 0..n_bins {
                let k = b1_idx + j;
                let frm = (bar.delay_at(k as f32 / half as f32) * frames as f64) as usize;
                let amp = bar.edge_taper(j as f32 / n_bins as f32);
                let src = ((current + frames - frm.min(frames - 1)) % frames) * half + k;
                let l = self.hist_l[src] * amp;
                let r = self.hist_r[src] * amp;

                if fb > FB_GATE {
                    let fb_gain = bar.bar_amp * gain * fb;
                    let pi = prev_idx * half + k;
                    if self.hist_l[pi].norm_sqr() < FB_ENERGY_MAX {
                        self.hist_l[pi] += l * fb_gain;
                    }
                    if self.hist_r[pi].norm_sqr() < FB_ENERGY_MAX {
                        self.hist_r[pi] += r * fb_gain;
                    }
                }

                self.spec_out_l[k] +=
                    Complex32::new(g * (a0 * l.re + b0 * r.re), g * (a0 * l.im + b0 * r.im));
                self.spec_out_r[k] +=
                    Complex32::new(g * (a1 * l.re + b1 * r.re), g * (a1 * l.im + b1 * r.im));
            }
        }

        // 频谱域干/湿。
        for k in 0..half {
            self.spec_out_l[k] = Complex32::new(
                dry_mix(self.spec_out_l[k].re, self.spec_in_l[k].re, self.mix),
                dry_mix(self.spec_out_l[k].im, self.spec_in_l[k].im, self.mix),
            );
            self.spec_out_r[k] = Complex32::new(
                dry_mix(self.spec_out_r[k].re, self.spec_in_r[k].re, self.mix),
                dry_mix(self.spec_out_r[k].im, self.spec_in_r[k].im, self.mix),
            );
        }

        let mut out_l = std::mem::take(&mut self.fft_out_l);
        let mut out_r = std::mem::take(&mut self.fft_out_r);
        let spec_l = std::mem::take(&mut self.spec_out_l);
        let spec_r = std::mem::take(&mut self.spec_out_r);
        self.inverse_into(&spec_l, &mut out_l);
        self.inverse_into(&spec_r, &mut out_r);
        self.fft_out_l = out_l;
        self.fft_out_r = out_r;
        self.spec_out_l = spec_l;
        self.spec_out_r = spec_r;
        let mut pos = self.ola_pos;
        for k in 0..n {
            self.ola_l[pos] += self.fft_out_l[k];
            self.ola_r[pos] += self.fft_out_r[k];
            pos += 1;
            if pos == n {
                pos = 0;
            }
        }
    }

    fn forward_into(&mut self, right: bool) {
        let half = self.half;
        let mut input = std::mem::take(&mut self.fft_in);
        let mut out = std::mem::take(&mut self.fft_tmp);
        self.fft
            .fwd
            .process_with_scratch(&mut input, &mut out, &mut self.fft.scratch_fwd)
            .expect("real FFT forward");
        self.fft_in = input;
        let mut packed = std::mem::take(&mut self.packed_tmp);
        packed[0] = Complex32::new(out[0].re, out[half].re);
        packed[1..].copy_from_slice(&out[1..half]);
        self.fft_tmp = out;
        if right {
            self.spec_in_r.copy_from_slice(&packed);
        } else {
            self.spec_in_l.copy_from_slice(&packed);
        }
        self.packed_tmp = packed;
    }

    fn inverse_into(&mut self, packed: &[Complex32], out: &mut [f32]) {
        let n = self.n;
        let half = self.half;
        let mut spec = std::mem::take(&mut self.ifft_spec);
        spec[0] = Complex32::new(packed[0].re, 0.0);
        spec[half] = Complex32::new(packed[0].im, 0.0);
        spec[1..half].copy_from_slice(&packed[1..half]);
        self.fft
            .inv
            .process_with_scratch(&mut spec, out, &mut self.scratch_inv)
            .expect("real FFT inverse");
        self.ifft_spec = spec;
        for k in 0..n {
            out[k] = out[k] * (1.0 / n as f32) * self.window[k];
        }
    }
}

/// 只用于离线验证：把整段信号跑过核心，返回同长度输出（含算法启动延迟）。
pub fn render_offline(
    core: &mut FusionCore,
    input_l: &[f32],
    input_r: &[f32],
) -> (Vec<f32>, Vec<f32>) {
    let mut out_l = vec![0.0; input_l.len()];
    let mut out_r = vec![0.0; input_r.len()];
    core.process(input_l, input_r, &mut out_l, &mut out_r);
    (out_l, out_r)
}
