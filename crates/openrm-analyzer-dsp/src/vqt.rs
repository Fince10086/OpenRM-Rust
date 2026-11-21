//! VQTAnalyzer — 多速率变分辨率 Q 变换频谱分析引擎。
//!
//! 逐行移植自 `plugins/Analyzer/src/dsp/VQTAnalyzer.h`（635 行）：
//! 金字塔逐级 2× 最小相位半带抽取（倒谱谱因式分解）、分层 band 表、
//! 层内复指数核点积、跨层群延迟对齐、频率相关 τ 查找表。
//!
//! 输出线性幅度，每 hop（1024 样本）一帧；`freqs` 为各带中心 Hz。

use crate::frame::EngineFrame;
use crate::halfband::radix2_fft;

const K_PI: f64 = 3.14159265358979323846;
const K_FREQ_LO: f64 = 20.0;
const K_FREQ_HI: f64 = 20_000.0;
const K_HOP: usize = 1024;
const K_MAX_LAYERS: usize = 9;
/// band 上边距该层新奈奎斯特的比例（防混叠）。
const K_GUARD: f64 = 0.78;
/// 深层窗长下限：至少覆盖 ~4 个 fc 周期。
const K_CYCLE_FLOOR: f64 = 4.0;

// ---------------------------------------------------------------------------
// 最小相位 2x 半带抗混叠抽取器（VQT 金字塔专用；与 halfband.rs 的分解算法不同，
// 这里保持原版独立实现：固定 M=1024 倒谱法，不复用 HalfbandDec2）。
// ---------------------------------------------------------------------------

struct AntiAliasDec {
    d: i32,
    num_taps: usize,
    tap: [f32; 101],
    tap_rev: [f32; 101],
    state: [f32; 100],
    work: Vec<f32>,
    tau_tab: [f32; 65],
}

impl Default for AntiAliasDec {
    fn default() -> Self {
        Self::new()
    }
}

impl AntiAliasDec {
    fn new() -> Self {
        Self {
            d: 2,
            num_taps: 0,
            tap: [0.0; 101],
            tap_rev: [0.0; 101],
            state: [0.0; 100],
            work: vec![0.0; 100 + 4096],
            tau_tab: [0.0; 65],
        }
    }

    /// 频率相关群延迟（样本@输入采样速率），65 点线性插值。对应原版 `TauAt`。
    fn tau_at(&self, frac: f64) -> f64 {
        let f = frac.clamp(0.0, 1.0);
        let x = f * 64.0;
        let k = x.min(63.0) as usize;
        let t0 = self.tau_tab[k] as f64;
        let t1 = self.tau_tab[k + 1] as f64;
        t0 + (x - k as f64) * (t1 - t0)
    }

    fn build(&mut self) {
        self.build_halfband_2x();
        self.build_tau_tab();
    }

    fn build_halfband_2x(&mut self) {
        const KN: usize = 101;
        self.num_taps = KN;
        self.d = 2;
        let mut taps = [0.0_f64; KN];
        let mut sum = 0.0;
        for i in 0..KN {
            let n = i as isize - (KN - 1) as isize / 2;
            let v: f64;
            if n == 0 {
                v = 0.5;
            } else if n & 1 == 0 {
                v = 0.0; // 偶序（除中心）= 0：半带结构
            } else {
                v = 0.5 * (K_PI * n as f64 / 2.0).sin() / (K_PI * n as f64 / 2.0);
            }
            let theta = 2.0 * K_PI * i as f64 / (KN - 1) as f64;
            let v = v * (0.35875 - 0.48829 * theta.cos()
                + 0.14128 * (2.0 * theta).cos()
                - 0.01168 * (3.0 * theta).cos());
            taps[i] = v;
            sum += v;
        }
        for i in 0..KN {
            self.tap[i] = (taps[i] / sum) as f32;
        }
        Self::min_phase_convert(&mut self.tap, KN);
        for i in 0..self.num_taps {
            self.tap_rev[i] = self.tap[self.num_taps - 1 - i];
        }
    }

    /// 倒谱法最小相位化：保持幅度谱（阻带不变），群延迟降至通带平均 (~N/4)。
    /// 对应原版 `MinPhaseConvert`。
    fn min_phase_convert(taps: &mut [f32; 101], n: usize) {
        const M: usize = 1024;
        let mut re = [0.0_f64; M];
        let mut im = [0.0_f64; M];
        for i in 0..M {
            re[i] = if i < n { taps[i] as f64 } else { 0.0 };
            im[i] = 0.0;
        }
        radix2_fft(&mut re, &mut im, false);
        for i in 0..M {
            let m = (re[i].hypot(im[i])).max(1e-12);
            re[i] = m.ln(); // log|H|（偶对称）
            im[i] = 0.0;
        }
        radix2_fft(&mut re, &mut im, true); // 倒谱 c
        for i in 1..M / 2 {
            re[i] *= 2.0; // 最小相位截断
        }
        for i in M / 2 + 1..M {
            re[i] = 0.0;
        }
        radix2_fft(&mut re, &mut im, false); // FFT(c_min)
        for i in 0..M {
            let e = re[i].exp();
            let r = e * im[i].cos();
            im[i] = e * im[i].sin();
            re[i] = r;
        }
        radix2_fft(&mut re, &mut im, true); // h_min
        let mut dc = 0.0_f64;
        for i in 0..n {
            dc += re[i];
        }
        for i in 0..n {
            taps[i] = (re[i] / dc) as f32; // DC 增益归一
        }
    }

    fn build_tau_tab(&mut self) {
        let taps = self.tap;
        let nt = self.num_taps;
        let phase_at = |w: f64, taps: &[f32]| -> f64 {
            let mut r = 0.0_f64;
            let mut iq = 0.0_f64;
            for k in 0..nt {
                let ph = w * k as f64;
                r += taps[k] as f64 * ph.cos();
                iq -= taps[k] as f64 * ph.sin();
            }
            iq.atan2(r)
        };

        // 逐点差分 + 相位展开。τ[0] 无实际使用，直接置 0 简化。
        self.tau_tab[0] = 0.0;
        let mut prev = phase_at(K_PI / 64.0, &taps);
        for k in 1..=64 {
            let w = K_PI * k as f64 / 64.0;
            let cur = phase_at(w, &taps);
            let mut dp = cur - prev;
            while dp > K_PI {
                dp -= 2.0 * K_PI;
            }
            while dp < -K_PI {
                dp += 2.0 * K_PI;
            }
            // 真最小相位系统群延迟恒非负；截断 FIR 在阻带/过渡带相位无物理意义，
            // 差分会出负 τ，钳 0。
            self.tau_tab[k] = (-dp / (K_PI / 64.0)).max(0.0) as f32;
            prev = cur;
        }

        // 过渡带以下相位差分无物理意义：以 |H| 加权冻结到最后可信值
        // （≥−40dB 全信，≤−70dB 全冻结）；滤波器本身不动（纯对齐元数据）。
        let mag_db_at = |frac: f64, taps: &[f32]| -> f64 {
            let mut r = 0.0_f64;
            let mut iq = 0.0_f64;
            for k in 0..nt {
                let ph = K_PI * frac * k as f64;
                r += taps[k] as f64 * ph.cos();
                iq -= taps[k] as f64 * ph.sin();
            }
            20.0 * (r * r + iq * iq).sqrt().max(1e-9).log10()
        };
        let mut hold = self.tau_tab[1] as f64;
        for k in 1..=64 {
            let w = ((mag_db_at(k as f64 / 64.0, &taps) + 70.0) / 30.0).clamp(0.0, 1.0);
            if w >= 1.0 {
                hold = self.tau_tab[k] as f64;
            } else {
                self.tau_tab[k] = (w * self.tau_tab[k] as f64 + (1.0 - w) * hold) as f32;
            }
        }
    }

    fn reset(&mut self) {
        self.state.fill(0.0);
    }

    /// 通用 FIR + mD 抽取（mD==2 走 8 路分块）。返回输出样本数。
    fn process(&mut self, input: &[f32], output: &mut [f32]) -> usize {
        let nt = self.num_taps;
        let nin = input.len();
        let sz = (nt - 1) + nin;
        for i in 0..nt - 1 {
            self.work[i] = self.state[i];
        }
        for i in 0..nin {
            self.work[nt - 1 + i] = input[i];
        }
        let nout = nin / self.d as usize;
        let t = self.tap_rev;
        let mut j = 0;
        while j + 8 <= nout {
            let x = &self.work[2 * j..];
            let (mut a0, mut a1, mut a2, mut a3) = (0.0_f32, 0.0_f32, 0.0_f32, 0.0_f32);
            let (mut a4, mut a5, mut a6, mut a7) = (0.0_f32, 0.0_f32, 0.0_f32, 0.0_f32);
            for i in 0..nt {
                let tv = t[i];
                a0 += tv * x[i];
                a1 += tv * x[i + 2];
                a2 += tv * x[i + 4];
                a3 += tv * x[i + 6];
                a4 += tv * x[i + 8];
                a5 += tv * x[i + 10];
                a6 += tv * x[i + 12];
                a7 += tv * x[i + 14];
            }
            output[j] = a0;
            output[j + 1] = a1;
            output[j + 2] = a2;
            output[j + 3] = a3;
            output[j + 4] = a4;
            output[j + 5] = a5;
            output[j + 6] = a6;
            output[j + 7] = a7;
            j += 8;
        }
        while j < nout {
            let x = &self.work[2 * j..];
            let mut acc = 0.0_f32;
            for i in 0..nt {
                acc += t[i] * x[i];
            }
            output[j] = acc;
            j += 1;
        }
        for i in 0..nt - 1 {
            self.state[i] = self.work[sz - (nt - 1) + i];
        }
        nout
    }
}

// ---------------------------------------------------------------------------

struct Band {
    layer: i32,
    wsum_inv: f32,
    win_len: i32,
    advance: i32,
    read_off: i32,
    kernel_re: Vec<f32>,
    kernel_im: Vec<f32>,
}

#[derive(Clone, Copy, Default)]
struct RunState {
    phase: i32,
    last: f32,
}

#[derive(Clone, Copy)]
struct DecimStep {
    dst_layer: i32,
    decim_idx: i32,
}

#[derive(Clone, Copy, Debug)]
pub struct BandDebug {
    pub layer: i32,
    pub win_len: i32,
    pub advance: i32,
    pub read_off: i32,
    pub wsum_inv: f32,
    pub kr0: f32,
    pub ki0: f32,
    pub kr1: f32,
    pub ki1: f32,
}

pub struct VqtAnalyzer {
    window_type: i32,
    bpo: i32,
    gamma: i32,
    chan_tri: i32,
    sample_rate: f64,
    need_rebuild: bool,
    bands: Vec<Band>,
    freqs: Vec<f64>,
    max_win_per_layer: [i32; K_MAX_LAYERS],
    path: [DecimStep; K_MAX_LAYERS],
    num_path_steps: i32,
    layers: [[Vec<f32>; K_MAX_LAYERS]; 3],
    decim: [[AntiAliasDec; K_MAX_LAYERS]; 3],
    run: [Vec<RunState>; 3],
    buf_count: usize,
    pending: [Vec<f32>; 3],
    frame: Option<EngineFrame>,
}

impl VqtAnalyzer {
    pub fn new() -> Self {
        let mut me = Self {
            window_type: 1, // Analyzer 运行态默认 Clean/BH5（CurrentVQTWindow）
            bpo: 24,
            gamma: 5,
            chan_tri: 0,
            sample_rate: 48_000.0,
            need_rebuild: false,
            bands: Vec::new(),
            freqs: Vec::new(),
            max_win_per_layer: [0; K_MAX_LAYERS],
            path: [DecimStep { dst_layer: 0, decim_idx: 0 }; K_MAX_LAYERS],
            num_path_steps: 0,
            layers: Default::default(),
            decim: Default::default(),
            run: Default::default(),
            buf_count: 0,
            pending: [vec![0.0; K_HOP], vec![0.0; K_HOP], vec![0.0; K_HOP]],
            frame: None,
        };
        for c in 0..3 {
            me.layers[c] = Default::default();
            for l in 0..K_MAX_LAYERS {
                me.decim[c][l] = AntiAliasDec::new();
            }
        }
        me.rebuild_bands();
        me
    }

    pub fn set_sample_rate(&mut self, sr: f64) {
        if sr != self.sample_rate {
            self.sample_rate = sr;
            self.need_rebuild = true;
        }
    }

    /// 低频带宽下限 γ（Hz）。对应原版 `SetGamma(int)`，钳制 5..20。
    pub fn set_gamma_hz(&mut self, gamma_hz: f64) {
        let g = gamma_hz.round().clamp(5.0, 20.0) as i32;
        if g != self.gamma {
            self.gamma = g;
            self.need_rebuild = true;
        }
    }

    pub fn num_bands(&self) -> usize {
        self.bands.len()
    }

    pub fn band_freqs(&self) -> &[f64] {
        &self.freqs
    }

    /// 测试/比对用：带内参数快照。
    pub fn band_debug(&self, b: usize) -> BandDebug {
        let bd = &self.bands[b];
        BandDebug {
            layer: bd.layer,
            win_len: bd.win_len,
            advance: bd.advance,
            read_off: bd.read_off,
            wsum_inv: bd.wsum_inv,
            kr0: bd.kernel_re.first().copied().unwrap_or(0.0),
            ki0: bd.kernel_im.first().copied().unwrap_or(0.0),
            kr1: bd.kernel_re.get(1).copied().unwrap_or(0.0),
            ki1: bd.kernel_im.get(1).copied().unwrap_or(0.0),
        }
    }

    /// 0=PWR, 1=LR, 2=SUM。
    pub fn set_channel_mode(&mut self, chan_tri: i32) {
        self.chan_tri = chan_tri.clamp(0, 2);
    }

    /// 对应原版 `SetWindowType`（VQTAnalyzer.h:319-327）：
    /// `0` = Hann（锐利档，窄主瓣），`1` = 5 项 Blackman-Harris（纯净档，-125 dB 泄漏）。
    /// 档位变化置重建标记，下一帧 `check_rebuild` 惰性重建 band 表与复指数核；
    /// 返回窗型是否真的发生变化（原版返回值用于冻结态决定是否重置 pad）。
    pub fn set_window(&mut self, window_type: i32) -> bool {
        let w = window_type.clamp(0, 2);
        if w != self.window_type {
            self.window_type = w;
            self.need_rebuild = true;
            return true;
        }
        false
    }

    /// 当前窗型档位（对应原版 `GetWindowType`）。
    pub fn window_type(&self) -> i32 {
        self.window_type
    }

    fn check_rebuild(&mut self) {
        if self.need_rebuild {
            self.need_rebuild = false;
            self.rebuild_bands();
        }
    }

    fn append_layer(&mut self, c: usize, l: usize, src: &[f32], n: usize, max_keep: usize) {
        let buf = &mut self.layers[c][l];
        if n >= max_keep {
            *buf = src[n - max_keep..n].to_vec();
            return;
        }
        let old_keep = max_keep - n;
        if buf.len() > old_keep {
            buf.drain(0..buf.len() - old_keep);
        }
        buf.extend_from_slice(&src[..n]);
    }

    fn assign_layer(&self, band_hi: f64, fs: f64) -> i32 {
        let mut l = 0;
        while l + 1 < K_MAX_LAYERS {
            let new_nyq = fs / (1 << (l + 2)) as f64;
            if band_hi > K_GUARD * new_nyq {
                break;
            }
            l += 1;
        }
        l as i32
    }

    fn gd_total_samples(&self, fc: f64, fs: f64, l: i32) -> f64 {
        let mut g = 0.0_f64;
        let mut src = 0_i32;
        for s in 0..self.num_path_steps as usize {
            let st = self.path[s];
            if st.dst_layer > l {
                break;
            }
            let nyq_in = fs / (1 << src + 1) as f64;
            g += self.decim[0][st.decim_idx as usize].tau_at(fc / nyq_in) * (1 << src) as f64;
            src = st.dst_layer;
        }
        g
    }

    fn rebuild_bands(&mut self) {
        self.bands.clear();
        self.freqs.clear();
        self.max_win_per_layer.fill(0);
        let fs = self.sample_rate.max(1.0);
        let q = 1.0 / (2.0_f64.powf(1.0 / self.bpo as f64) - 1.0);

        // 金字塔路径 / 有效层
        let mut n_steps = 0;
        for l in 0..K_MAX_LAYERS - 1 {
            self.path[n_steps] = DecimStep {
                dst_layer: (l + 1) as i32,
                decim_idx: l as i32,
            };
            n_steps += 1;
        }
        self.num_path_steps = n_steps as i32;

        // 抽取器：全 2x 最小相位半带
        for l in 0..K_MAX_LAYERS {
            for c in 0..3 {
                self.decim[c][l].build();
            }
        }

        // band 表（频率序）
        struct Spec {
            fc: f64,
            bw: f64,
            layer: i32,
        }
        let mut spec: Vec<Spec> = Vec::new();
        let mut layer_count = [0_i32; K_MAX_LAYERS];
        let mut k = 0_usize;
        loop {
            let fc = K_FREQ_LO * 2.0_f64.powf(k as f64 / self.bpo as f64);
            if fc > K_FREQ_HI {
                break;
            }
            let bw = fc / q + self.gamma as f64;
            let l = self.assign_layer(fc + 2.0 * bw, fs);
            spec.push(Spec { fc, bw, layer: l });
            layer_count[l as usize] += 1;
            k += 1;
        }

        let n = spec.len();
        let mut i = 0;
        while i < n {
            let l = spec[i].layer;
            let mut j = i;
            while j < n && spec[j].layer == l {
                j += 1;
            }
            // 下一更深带流层（跳过无流层）
            let mut next_deep = -1_i32;
            for d in (l + 1) as usize..K_MAX_LAYERS {
                if layer_count[d] > 0 {
                    next_deep = d as i32;
                    break;
                }
            }
            for t in i..j {
                let f_seg = if j - i > 1 {
                    (t - i) as f64 / (j - i - 1) as f64
                } else {
                    0.0
                };
                self.add_band(spec[t].fc, spec[t].bw, fs, l, next_deep, f_seg);
            }
            i = j;
        }

        self.reset_runtime_state();
    }

    #[allow(clippy::too_many_arguments)]
    fn add_band(&mut self, fc: f64, bw: f64, fs: f64, l: i32, next_deep: i32, f_seg: f64) {
        let fs_l = fs / (1 << l) as f64;
        let wl = (8.0_f64)
            .max((fs_l / bw).round())
            .max((fs_l / fc * K_CYCLE_FLOOR).round()) as i32;

        let gd_l = self.gd_total_samples(fc, fs, l);
        let gd_d = if next_deep > 0 {
            self.gd_total_samples(fc, fs, next_deep)
        } else {
            gd_l
        };
        let r = gd_d + (gd_l - gd_d) * f_seg;
        // 夹非负：负 readOff 会让取窗起点越过缓冲区末端（读越界）
        let read_off = ((r - gd_l) / fs * fs_l).round().max(0.0) as i32;

        let mut kernel_re = vec![0.0_f32; wl as usize];
        let mut kernel_im = vec![0.0_f32; wl as usize];
        let step = 2.0 * K_PI * fc / fs_l;
        let mut sum = 0.0_f64;
        for n in 0..wl as usize {
            let theta = 2.0 * K_PI * n as f64 / (wl - 1) as f64;
            let w: f64 = if self.window_type == 0 {
                // 0: Hann 窗（锐利档）
                0.5 * (1.0 - theta.cos())
            } else {
                // 1: 5-term Blackman-Harris 窗（纯净档）
                0.3230984033 - 0.4714310503 * theta.cos()
                    + 0.1756446367 * (2.0 * theta).cos()
                    - 0.0285582447 * (3.0 * theta).cos()
                    + 0.001267665 * (4.0 * theta).cos()
            };
            sum += w;
            let ph = step * n as f64;
            kernel_re[n] = (w * ph.cos()) as f32;
            kernel_im[n] = (w * ph.sin()) as f32;
        }
        let wsum_inv = if sum > 1e-12 { (2.0 / sum) as f32 } else { 0.0 };

        self.max_win_per_layer[l as usize] =
            self.max_win_per_layer[l as usize].max(wl + read_off);
        self.bands.push(Band {
            layer: l,
            wsum_inv: wsum_inv,
            win_len: wl,
            advance: 1,
            read_off: read_off,
            kernel_re,
            kernel_im,
        });
        self.freqs.push(fc);
    }

    fn reset_runtime_state(&mut self) {
        let nb = self.bands.len();
        for c in 0..3 {
            self.run[c] = vec![RunState { phase: 0, last: 0.0 }; nb];
            for l in 0..K_MAX_LAYERS {
                self.layers[c][l].clear();
            }
            for l in 0..K_MAX_LAYERS {
                self.decim[c][l].reset();
            }
        }
    }

    pub fn process_block(&mut self, l: &[f32], r: &[f32], m: &[f32]) {
        let n = l.len().min(r.len()).min(m.len());
        for s in 0..n {
            self.pending[0][self.buf_count] = l[s];
            self.pending[1][self.buf_count] = r[s];
            self.pending[2][self.buf_count] = m[s];
            self.buf_count += 1;
            if self.buf_count == K_HOP {
                self.analyze();
                self.buf_count = 0;
            }
        }
    }

    fn analyze(&mut self) {
        self.check_rebuild();
        let nb = self.bands.len();
        if nb == 0 {
            return;
        }
        let need_l = self.chan_tri != 2;
        let need_r = self.chan_tri != 2;
        let need_sum = self.chan_tri == 2;

        let mut vals: Vec<[f32; 3]> = vec![[0.0; 3]; nb];

        for c in 0..3 {
            let active = match c {
                0 => need_l,
                1 => need_r,
                _ => need_sum,
            };
            if !active {
                continue;
            }

            // 金字塔降采样（逐级 2x 最小相位）
            let raw = self.pending[c].clone();
            self.append_layer(c, 0, &raw, K_HOP, self.max_win_per_layer[0] as usize);
            let mut sc0: Vec<f32> = vec![0.0; K_HOP];
            let mut sc1: Vec<f32> = vec![0.0; K_HOP];
            let mut cur: Vec<f32> = raw;
            for s in 0..self.num_path_steps as usize {
                let st = self.path[s];
                let dst: &mut [f32] = if s & 1 == 0 { &mut sc0 } else { &mut sc1 };
                let nout = self.decim[c][st.decim_idx as usize].process(&cur, dst);
                self.append_layer(
                    c,
                    st.dst_layer as usize,
                    dst,
                    nout,
                    self.max_win_per_layer[st.dst_layer as usize] as usize,
                );
                cur = dst[..nout].to_vec();
            }

            for b in 0..nb {
                vals[b][c] = self.band_mag(c, b);
            }
        }

        self.frame = Some(EngineFrame {
            vals,
            freqs: self.freqs.clone(),
        });
    }

    /// 单带点积 + 相位倒计时。对应原版 PrepareDataForUI 内 band 循环体。
    fn band_mag(&mut self, c: usize, b: usize) -> f32 {
        let rs = &mut self.run[c][b];
        rs.phase -= 1;
        if rs.phase > 0 {
            return rs.last;
        }
        let bd = &self.bands[b];
        let buf = &self.layers[c][bd.layer as usize];
        let wl = bd.win_len as usize;
        let read_off = bd.read_off.max(0) as usize;
        let have = (wl).min(buf.len().saturating_sub(read_off)).max(0);
        let skip = wl - have;
        let start = buf.len().saturating_sub(read_off + have);
        let x = &buf[start..];
        let kr = &bd.kernel_re[skip..];
        let ki = &bd.kernel_im[skip..];
        let cnt = wl - skip;

        let (mut re0, mut re1, mut re2, mut re3) = (0.0_f32, 0.0_f32, 0.0_f32, 0.0_f32);
        let (mut im0, mut im1, mut im2, mut im3) = (0.0_f32, 0.0_f32, 0.0_f32, 0.0_f32);
        let mut j = 0;
        while j + 4 <= cnt {
            let v0 = x[j];
            let v1 = x[j + 1];
            let v2 = x[j + 2];
            let v3 = x[j + 3];
            re0 += v0 * kr[j];
            im0 += v0 * ki[j];
            re1 += v1 * kr[j + 1];
            im1 += v1 * ki[j + 1];
            re2 += v2 * kr[j + 2];
            im2 += v2 * ki[j + 2];
            re3 += v3 * kr[j + 3];
            im3 += v3 * ki[j + 3];
            j += 4;
        }
        let mut re = (re0 + re1) + (re2 + re3);
        let mut im = (im0 + im1) + (im2 + im3);
        while j < cnt {
            re += x[j] * kr[j];
            im += x[j] * ki[j];
            j += 1;
        }
        let mag = (re * re + im * im).sqrt() * bd.wsum_inv;
        rs.last = mag;
        rs.phase = bd.advance;
        mag
    }

    pub fn take_frame(&mut self) -> Option<EngineFrame> {
        self.frame.take()
    }

    pub fn reset(&mut self) {
        self.reset_runtime_state();
        self.buf_count = 0;
        self.frame = None;
    }
}

impl Default for VqtAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}
