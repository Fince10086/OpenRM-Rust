//! RTAAnalyzer — 模拟硬件风格滤波器组实时频谱分析引擎（Filter-Bank RTA）。
//!
//! 逐行移植自 `plugins/Analyzer/src/dsp/RTAAnalyzer.h`（325 行）。
//!
//! 架构：级联低通差分互补滤波器组。N+1 个 2 阶谐振低通，截止放在带缘
//! （q = 2·bpo，prewrap 预畸变），每带 = 相邻两个低通输出之差 → 各带构成信号
//! 的互补分割（Σ band = 最高低通）；检测为逐样本连续功率积分（无矩形窗）。
//! 带中心增益经解析传输函数校准（z=e^{jω0} 稳态模）。
//!
//! 对外统一 API：每 hop（1024 样本）一帧 `EngineFrame`，线性幅度。

use crate::frame::EngineFrame;
use std::collections::VecDeque;

const K_HOP: usize = 1024;
const K_ENV_BLOCK: usize = 8; // 功率积分块更新长度（样本；帧长为其整数倍）
const K_MAX_BANDS: usize = 256; // 1/24 Oct 22Hz..20kHz ≈ 236 带（上限留余量）
const K_PI: f64 = 3.141_592_653_589_793_238_46;
const K_LN2: f64 = 0.693_147_180_559_945_309_42;
const K_FREQ_HI: f64 = 20_000.0; // 分析上限（与 VQT/PBT 一致）

/// 每倍频程带数与预畸变系数（按档位；与 C++ `kBpo`/`kFerr` 对应）。
const BPO: [usize; 3] = [6, 12, 24];
const FERR: [f64; 3] = [0.68, 0.71, 0.71];

/// 声道模式（与 C++ `mChanTri` 对应；0=PWR, 1=LR, 2=SUM；PWR/LR 在引擎内同路径）。
const MODE_PWR: i32 = 0;
const MODE_SUM: i32 = 2;

struct ChannelState {
    z0: Vec<f32>,
    z1: Vec<f32>,
    env: Vec<f32>,
    acc: Vec<f32>,
    prev_in: f32,
}

impl ChannelState {
    fn new(nb: usize) -> Self {
        Self {
            z0: vec![0.0; nb + 1],
            z1: vec![0.0; nb + 1],
            env: vec![0.0; nb],
            acc: vec![0.0; nb],
            prev_in: 0.0,
        }
    }

    fn reset(&mut self, nb: usize) {
        self.z0 = vec![0.0; nb + 1];
        self.z1 = vec![0.0; nb + 1];
        self.env = vec![0.0; nb];
        self.acc = vec![0.0; nb];
        self.prev_in = 0.0;
    }
}

pub struct RtaAnalyzer {
    sr: f64,
    octave_mode: i32,
    chan_tri: i32,

    freqs: Vec<f64>,
    edges: Vec<f64>,
    k0: Vec<f32>,
    k1: Vec<f32>,
    k2: Vec<f32>,
    inv_gain: Vec<f32>,
    env_alpha_blk: f32,

    chans: [ChannelState; 3],
    pending: [Vec<f32>; 3],
    buf_count: usize,
    queue: VecDeque<EngineFrame>,
}

impl Default for RtaAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl RtaAnalyzer {
    pub fn new() -> Self {
        let mut rta = Self {
            sr: 48_000.0,
            octave_mode: 0,
            chan_tri: MODE_PWR,
            freqs: Vec::new(),
            edges: Vec::new(),
            k0: Vec::new(),
            k1: Vec::new(),
            k2: Vec::new(),
            inv_gain: Vec::new(),
            env_alpha_blk: 0.0,
            chans: [
                ChannelState::new(0),
                ChannelState::new(0),
                ChannelState::new(0),
            ],
            pending: [Vec::new(), Vec::new(), Vec::new()],
            buf_count: 0,
            queue: VecDeque::new(),
        };
        rta.rebuild_bands();
        rta
    }

    pub fn set_sample_rate(&mut self, sr: f64) {
        if (self.sr - sr).abs() > 0.1 {
            self.sr = sr;
            self.rebuild_bands();
        }
    }

    /// 0=1/6, 1=1/12, 2=1/24（59/118/236 带）。返回是否发生档位切换。
    pub fn set_octave_mode(&mut self, mode: i32) -> bool {
        let m = mode.clamp(0, 2);
        if m != self.octave_mode {
            self.octave_mode = m;
            self.rebuild_bands();
            true
        } else {
            false
        }
    }

    pub fn octave_mode(&self) -> i32 {
        self.octave_mode
    }

    pub fn set_channel_mode(&mut self, mode: i32) {
        self.chan_tri = mode.clamp(MODE_PWR, MODE_SUM);
    }

    pub fn num_bands(&self) -> usize {
        self.freqs.len()
    }

    pub fn band_freqs(&self) -> &[f64] {
        &self.freqs
    }

    // ── 测试对照用只读访问器（不对接 UI/集成层）──────────────────────────────
    #[doc(hidden)]
    pub fn k0_debug(&self) -> &[f32] {
        &self.k0
    }
    #[doc(hidden)]
    pub fn k1_debug(&self) -> &[f32] {
        &self.k1
    }
    #[doc(hidden)]
    pub fn k2_debug(&self) -> &[f32] {
        &self.k2
    }
    #[doc(hidden)]
    pub fn gain_debug(&self) -> &[f32] {
        &self.inv_gain
    }
    #[doc(hidden)]
    pub fn env_alpha_debug(&self) -> f32 {
        self.env_alpha_blk
    }
    #[doc(hidden)]
    pub fn edges_debug(&self) -> &[f64] {
        &self.edges
    }

    pub fn reset(&mut self) {
        for ch in &mut self.chans {
            ch.reset(self.freqs.len());
        }
        self.buf_count = 0;
        self.queue.clear();
    }

    /// 音频线程：采集输入采样，每 K_HOP 样本入队一帧原始 hop 数据。
    pub fn process_block(&mut self, l: &[f32], r: &[f32], m: &[f32]) {
        let n = l.len().min(r.len()).min(m.len());
        if n == 0 {
            return;
        }
        for s in 0..n {
            self.pending[0].push(l[s]);
            self.pending[1].push(r[s]);
            self.pending[2].push(m[s]);
            self.buf_count += 1;
            if self.buf_count == K_HOP {
                let frame = self.analyze_hop();
                self.queue.push_back(frame);
                self.pending[0].clear();
                self.pending[1].clear();
                self.pending[2].clear();
                self.buf_count = 0;
            }
        }
    }

    pub fn take_frame(&mut self) -> Option<EngineFrame> {
        self.queue.pop_front()
    }

    // ── 带表与系数重建（对应 C++ `RebuildBands`）─────────────────────────────

    fn rebuild_bands(&mut self) {
        self.freqs.clear();
        self.k0.clear();
        self.k1.clear();
        self.k2.clear();
        self.inv_gain.clear();

        let fs = self.sr.max(1.0);
        let bpo = BPO[self.octave_mode as usize];
        // 预畸变系数 ferr：使相邻带实际交叉频率对齐设计网格带缘。
        let ferr = FERR[self.octave_mode as usize];

        // 带中心/带缘网格：中心 15.625·2^(j/bpo) 自 22Hz 起，带缘取几何中点并加
        // Nyquist 扭曲项；上缘超 fedg = 0.47·fs 或中心超 20kHz 的带不收录。
        let k = (K_LN2 / bpo as f64).exp();
        let mut f = 15.625;
        while f < 22.0 {
            f *= k;
        }
        let fedg = 0.94 * 0.5 * fs;
        // C++: `(int)(0.5 + log(...)/log(k))` — 对 raw 做半值向上取整（正数等价 floor(raw+0.5)）。
        let wi = ((fedg / (1000.0 * k.sqrt())).ln() / k.ln()).round() as i32;
        let fli = 1.0 / (1000.0 * k.powi(wi));
        let wr = (1.0 - (fedg / k.sqrt()) * fli) * fli * fli;

        let mut edges = vec![0.0f64; K_MAX_BANDS + 1];
        let mut centers = [0.0f64; K_MAX_BANDS];
        let mut nb = 0usize;
        let mut ff = (f / k.sqrt()) * (1.0 - wr * f * f); // edges[0]：首中心下缘
        loop {
            let top = f * k.sqrt() * (1.0 - wr * f * f);
            if nb >= K_MAX_BANDS || f > K_FREQ_HI || top > fedg + 1.0 {
                break;
            }
            edges[nb] = ff;
            centers[nb] = f;
            nb += 1;
            ff = top;
            f *= k;
        }
        edges[nb] = ff; // 顶缘（最后计入带的上缘）
        self.edges = edges[..=nb].to_vec();

        // 边缘低通系数（twoPoleLPCoeffs）：q = 2·bpo，prewrap = ferr。
        self.k0.resize(nb + 1, 0.0);
        self.k1.resize(nb + 1, 0.0);
        self.k2.resize(nb + 1, 0.0);
        for j in 0..=nb {
            let w = 2.0 * K_PI * edges[j] / fs;
            let y = w.sin() / ((2.0 * bpo as f64) * (1.0 + (w * ferr).cos()));
            let a = 1.0 / (1.0 + y);
            self.k2[j] = (a * (y - 1.0)) as f32;
            self.k1[j] = (a * 2.0 * w.cos()) as f32;
            self.k0[j] = ((0.5 / (2.0 * bpo as f64)) * (1.0 - self.k1[j] as f64 - self.k2[j] as f64)) as f32;
        }

        // 带中心增益解析校准：稳态增益 = |(1+z⁻¹)·(LP_{j+1}−LP_j)| 在 z=e^{jω0} 的模。
        self.inv_gain.resize(nb, 0.0);
        for j in 0..nb {
            let w0 = 2.0 * K_PI * centers[j] / fs;
            let zr = w0.cos();
            let zi = -w0.sin();
            let z2r = (2.0 * w0).cos();
            let z2i = -(2.0 * w0).sin();
            let k0_lo = self.k0[j] as f64;
            let k1_lo = self.k1[j] as f64;
            let k2_lo = self.k2[j] as f64;
            let k0_hi = self.k0[j + 1] as f64;
            let k1_hi = self.k1[j + 1] as f64;
            let k2_hi = self.k2[j + 1] as f64;
            // A(z) = 1 − k1·z⁻¹ − k2·z⁻²
            let a_lo_re = 1.0 - k1_lo * zr - k2_lo * z2r;
            let a_lo_im = -(k1_lo * zi + k2_lo * z2i);
            let a_hi_re = 1.0 - k1_hi * zr - k2_hi * z2r;
            let a_hi_im = -(k1_hi * zi + k2_hi * z2i);
            // num = k0Hi·A_lo − k0Lo·A_hi，den = A_lo·A_hi
            let num_re = k0_hi * a_lo_re - k0_lo * a_hi_re;
            let num_im = k0_hi * a_lo_im - k0_lo * a_hi_im;
            let den_re = a_lo_re * a_hi_re - a_lo_im * a_hi_im;
            let den_im = a_lo_re * a_hi_im + a_lo_im * a_hi_re;
            let den_sq = den_re * den_re + den_im * den_im;
            let (q_re, q_im) = if den_sq > 0.0 {
                (
                    (num_re * den_re + num_im * den_im) / den_sq,
                    (num_im * den_re - num_re * den_im) / den_sq,
                )
            } else {
                (0.0, 0.0)
            };
            // H = (1 + z⁻¹)·q，1 + z⁻¹ = 1 + cos w0 − j·sin w0
            let one_re = 1.0 + zr;
            let one_im = -w0.sin();
            let h_re = one_re * q_re - one_im * q_im;
            let h_im = one_re * q_im + one_im * q_re;
            let g = (h_re * h_re + h_im * h_im).sqrt();
            self.inv_gain[j] = (1.0 / g.max(1e-9)) as f32;
            self.freqs.push(centers[j]);
        }

        // 连续功率积分时间常数（τ = 50ms）：消矩形窗闪动。
        self.env_alpha_blk = (1.0 - (-(K_ENV_BLOCK as f64) / (fs * 0.05)).exp()) as f32;

        for ch in &mut self.chans {
            ch.reset(nb);
        }
    }

    // ── 帧分析（对应 C++ `PrepareDataForUI`）─────────────────────────────────

    fn analyze_hop(&mut self) -> EngineFrame {
        let nb = self.freqs.len();
        let need_l = self.chan_tri != MODE_SUM;
        let need_r = self.chan_tri != MODE_SUM;
        let need_sum = self.chan_tri == MODE_SUM;

        let n_lp = nb + 1;
        let k0 = &self.k0;
        let k1 = &self.k1;
        let k2 = &self.k2;
        let inv_gain = &self.inv_gain;
        let alpha_blk = self.env_alpha_blk;
        const INV_BLK: f32 = 1.0 / K_ENV_BLOCK as f32;

        let mut vals: Vec<[f32; 3]> = vec![[0.0; 3]; nb];

        for c in 0..3usize {
            let active = match c {
                0 => need_l,
                1 => need_r,
                _ => need_sum,
            };
            if !active {
                continue;
            }

            let raw: Vec<f32> = self.pending[c].clone();
            let ch = &mut self.chans[c];
            let z0 = &mut ch.z0;
            let z1 = &mut ch.z1;
            let env = &mut ch.env;
            let acc = &mut ch.acc;
            let mut prev_in = ch.prev_in;

            for n in 0..K_HOP {
                // 输入预处理（ZeroLP）：x[n] + x[n−1]，Nyquist 陷波
                let xin = raw[n];
                let x = xin + prev_in;
                prev_in = xin;

                let mut y_prev = x * k0[0] + z0[0] * k1[0] + z1[0] * k2[0];
                z1[0] = z0[0];
                z0[0] = y_prev;

                for j in 1..n_lp {
                    let out = x * k0[j] + z0[j] * k1[j] + z1[j] * k2[j];
                    z1[j] = z0[j];
                    z0[j] = out;
                    // band j−1 = LP_j − LP_{j−1}：互补差分（相邻带共享低通输出）
                    let bd = out - y_prev;
                    acc[j - 1] += bd * bd;
                    y_prev = out;
                }

                if (n & (K_ENV_BLOCK - 1)) == (K_ENV_BLOCK - 1) {
                    for b in 0..nb {
                        let e = &mut env[b];
                        *e += (acc[b] * INV_BLK - *e) * alpha_blk;
                        acc[b] = 0.0;
                    }
                }
            }
            ch.prev_in = prev_in;

            // 抗 Denormal
            for j in 0..n_lp {
                if z0[j].abs() < 1e-20 {
                    z0[j] = 0.0;
                }
                if z1[j].abs() < 1e-20 {
                    z1[j] = 0.0;
                }
            }
            // 帧末读数：每带一次 sqrt
            for b in 0..nb {
                vals[b][c] = (2.0 * env[b]).sqrt() * inv_gain[b];
            }
            for j in 0..nb {
                if env[j] < 1e-20 {
                    env[j] = 0.0;
                }
                if acc[j] < 1e-20 {
                    acc[j] = 0.0;
                }
            }
        }

        EngineFrame {
            vals,
            freqs: self.freqs.clone(),
        }
    }
}
