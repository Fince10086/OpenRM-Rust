//! PBTAnalyzer — 心理声学临界频带频谱分析引擎（多速率解调核算法）。
//!
//! 逐行移植自 `plugins/Analyzer/src/dsp/PBTAnalyzer.h`（437 行）与配套的
//! `dsp/HalfbandDec2.h`（半带 ×2 抽取器 + 倒频谱最小相位谱分解）。
//!
//! 每个频带一个复数解调 FIR 核：kernel[n] = proto[n]·e^{-j2π·fc·n/fsL}。
//! 核形状：Kaiser 等波纹原型低通，核长系数 K=4.6。
//! 多速率：核跑在 2× 半带抽取金字塔上（层 L 速率 fs/2^L，跨帧保持滤波状态）。
//! 最小相位化：带核原型与半带抽取链都做同幅频谱分解（倒频谱法）。
//!
//! 对外统一 API：每 hop（1024 样本）一帧 `EngineFrame`，线性幅度。

use crate::frame::EngineFrame;
use std::collections::VecDeque;
use std::sync::OnceLock;

const K_HOP: usize = 1024;
const K_MAX_LAYERS: usize = 10;
/// 带上面距该层新奈奎斯特的比例（防抽取混叠，VQT 同款）。
const K_GUARD: f64 = 0.78;
/// 分析上限（与 VQT/RTA 一致；高采样率下带表不再上扩）。
const K_FREQ_HI: f64 = 20_000.0;
const K_PI: f64 = 3.141_592_653_589_793_238_46;
/// 核长系数 k（Kaiser 墙位 Esb=0.13·k·bw），原版拟合定值。
const K_KERNEL_LEN: f64 = 4.6;

const MODE_PWR: i32 = 0;
const MODE_SUM: i32 = 2;

// ── 原版临界频带表（kPbtFreqs40/20/10）─────────────────────────────────────

const FREQS_40: [f64; 52] = [
    23.0, 70.0, 117.0, 164.0, 211.0, 258.0, 305.0, 352.0, 422.0, 516.0,
    609.0, 703.0, 797.0, 891.0, 984.0, 1078.0, 1219.0, 1406.0, 1594.0, 1781.0,
    1969.0, 2156.0, 2344.0, 2531.0, 2719.0, 2906.0, 3188.0, 3563.0, 3937.0, 4312.0,
    4687.0, 5063.0, 5437.0, 5813.0, 6187.0, 6563.0, 6938.0, 7313.0, 7875.0, 8625.0,
    9375.0, 10125.0, 10875.0, 11625.0, 12750.0, 14250.0, 15750.0, 17250.0, 18750.0, 20250.0,
    21750.0, 23250.0,
];

const FREQS_20: [f64; 60] = [
    12.0, 35.0, 59.0, 82.0, 105.0, 129.0, 152.0, 176.0, 211.0, 258.0,
    305.0, 352.0, 398.0, 445.0, 492.0, 539.0, 609.0, 703.0, 797.0, 891.0,
    984.0, 1078.0, 1172.0, 1266.0, 1359.0, 1453.0, 1594.0, 1781.0, 1969.0, 2156.0,
    2344.0, 2531.0, 2719.0, 2906.0, 3094.0, 3281.0, 3469.0, 3656.0, 3937.0, 4312.0,
    4687.0, 5063.0, 5437.0, 5813.0, 6375.0, 7125.0, 7875.0, 8625.0, 9375.0, 10125.0,
    10875.0, 11625.0, 12750.0, 14250.0, 15750.0, 17250.0, 18750.0, 20250.0, 21750.0, 23250.0,
];

const FREQS_10: [f64; 68] = [
    6.0, 18.0, 29.0, 41.0, 53.0, 64.0, 76.0, 88.0, 105.0, 129.0,
    152.0, 176.0, 199.0, 223.0, 246.0, 270.0, 305.0, 352.0, 398.0, 445.0,
    492.0, 539.0, 586.0, 633.0, 680.0, 727.0, 797.0, 891.0, 984.0, 1078.0,
    1172.0, 1266.0, 1359.0, 1453.0, 1547.0, 1641.0, 1734.0, 1828.0, 1969.0, 2156.0,
    2344.0, 2531.0, 2719.0, 2906.0, 3188.0, 3563.0, 3937.0, 4312.0, 4687.0, 5063.0,
    5437.0, 5813.0, 6375.0, 7125.0, 7875.0, 8625.0, 9375.0, 10125.0, 10875.0, 11625.0,
    12750.0, 14250.0, 15750.0, 17250.0, 18750.0, 20250.0, 21750.0, 23250.0,
];

// ── 基-2 迭代 FFT（对应 HalfbandDec2.h `Fft`）──────────────────────────────

fn fft(re: &mut [f64], im: &mut [f64], n: usize, inverse: bool) {
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
        let ang = 2.0 * K_PI / len as f64 * if inverse { 1.0 } else { -1.0 };
        let wr = ang.cos();
        let wi = ang.sin();
        for i in (0..n).step_by(len) {
            let mut cr = 1.0;
            let mut ci = 0.0;
            for j in 0..len / 2 {
                let ur = re[i + j];
                let ui = im[i + j];
                let xr = re[i + j + len / 2];
                let xi = im[i + j + len / 2];
                let vr = xr * cr - xi * ci;
                let vi = xr * ci + xi * cr;
                re[i + j] = ur + vr;
                im[i + j] = ui + vi;
                re[i + j + len / 2] = ur - vr;
                im[i + j + len / 2] = ui - vi;
                let ncr = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = ncr;
            }
        }
        len <<= 1;
    }
    if inverse {
        let inv = 1.0 / n as f64;
        for i in 0..n {
            re[i] *= inv;
            im[i] *= inv;
        }
    }
}

const MIN_PHASE_DECAY_TARGET: f64 = 5e-4;

/// 指定 FFT 长度与抬底量的倒频谱谱分解（`MinPhaseFactorWithLen`）。
fn min_phase_factor_with_len(proto: &[f64], lfft: usize, lift: f64) -> Vec<f64> {
    let n = proto.len();
    let mr = n - 1; // 自相关最高滞后 2M (= N−1)
    let mut re = vec![0.0; lfft];
    let mut im = vec![0.0; lfft];
    // 自相关 r[n] = Σ_k proto[k]·proto[k−n]，零相位装载（bin n 与 bin Lfft−n）
    for k in 0..=mr {
        let mut acc = 0.0;
        for kk in k..n {
            acc += proto[kk] * proto[kk - k];
        }
        re[k] = acc;
        if k > 0 {
            re[lfft - k] = acc;
        }
    }
    re[0] *= 1.0 + lift; // 抬底座：圆上零点 → 圆内
    fft(&mut re, &mut im, lfft, false); // R = A²（实，≥0）
    // log R（floor 兜底）→ IFFT 得实倒频谱
    for k in 0..lfft {
        re[k] = re[k].max(1e-24).ln();
        im[k] = 0.0;
    }
    fft(&mut re, &mut im, lfft, true);
    // 复倒频谱因果化（c[0] 减半；后半置零，不可再加倍）
    re[0] *= 0.5;
    for k in lfft / 2..lfft {
        re[k] = 0.0;
        im[k] = 0.0;
    }
    fft(&mut re, &mut im, lfft, false);
    // exp → 最小相位频谱 → IFFT 得因果因子 g（支撑 0..2M，能量前置）
    for k in 0..lfft {
        let er = re[k].exp();
        re[k] = er * im[k].cos();
        im[k] = er * im[k].sin();
    }
    fft(&mut re, &mut im, lfft, true);
    re.truncate(mr + 1);
    re
}

/// 实对称 FIR 原型 → 同幅频最小相位谱因子（自相关 + 倒频谱分解），长度 N 不变。
fn min_phase_factor(proto: &[f64], stop_ripple: f64) -> Vec<f64> {
    let n = proto.len();
    let mut lfft = 1024;
    while lfft < 8 * n {
        lfft <<= 1;
    }
    if lfft < 16_384 {
        lfft = 16_384;
    }
    // 自适应抬底：Δt = sqrt(ε·Σp²)/δNπ → ε = (Δt·δ·N/π)²/Σp²
    let mut r0 = 0.0;
    for v in proto {
        r0 += v * v;
    }
    let s_est = stop_ripple * n as f64 / K_PI;
    let mut lift = (MIN_PHASE_DECAY_TARGET * s_est).powi(2) / r0.max(1e-30);
    lift = lift.clamp(1e-15, 1e-3);

    loop {
        let g = min_phase_factor_with_len(proto, lfft, lift);
        // 校验 |A0| 与 |G| 在 64 个频点上的幅谱一致性（零陷点跳过）
        let mut worst: f64 = 0.0;
        for i in 1..=64 {
            let w = K_PI * i as f64 / 64.0;
            let cw0 = w.cos();
            let sw0 = w.sin();
            let (mut ar, mut ai, mut gr, mut gi) = (0.0, 0.0, 0.0, 0.0);
            let (mut cw, mut sw) = (1.0, 0.0);
            for nidx in 0..n {
                ar += proto[nidx] * cw;
                ai -= proto[nidx] * sw;
                gr += g[nidx] * cw;
                gi -= g[nidx] * sw;
                let nc = cw * cw0 - sw * sw0;
                sw = cw * sw0 + sw * cw0;
                cw = nc;
            }
            let db_a = 20.0 * (ar * ar + ai * ai).sqrt().max(1e-15).log10();
            if db_a < -60.0 {
                continue; // 零陷点：抬底钳制区内，不参与校验
            }
            let db_g = 20.0 * (gr * gr + gi * gi).sqrt().max(1e-15).log10();
            worst = worst.max((db_a - db_g).abs());
        }
        if worst < 1.0 || lfft >= (1 << 19) {
            return g;
        }
        lfft <<= 1; // 正常一步收敛，此阶梯仅为正确性兜底
    }
}

/// 第一类修正贝塞尔 I0（Kaiser 窗用；级数展开，x ≤ ~10 收敛快）。
fn bessel_i0(x: f64) -> f64 {
    let mut sum = 1.0;
    let mut term = 1.0;
    for k in 1..32 {
        term *= (x / 2.0) / k as f64 * (x / 2.0) / k as f64;
        sum += term;
        if term < 1e-12 * sum {
            break;
        }
    }
    sum
}

// ── 半带 ×2 抽取器（对应 HalfbandDec2.h `HalfbandDec2`）─────────────────────

const HB_KN: usize = 101;
const HB_KQ: usize = (HB_KN - 1) / 2;

/// 对称半带原型（sinc × BH4 窗；偶序抽头除中心外严格为零）。
fn build_proto_taps() -> [f64; HB_KN] {
    let mut taps = [0.0; HB_KN];
    for i in 0..HB_KN {
        let n = i as i32 - HB_KQ as i32;
        let v: f64 = if n == 0 {
            0.5
        } else if n & 1 == 0 {
            0.0
        } else {
            0.5 * (K_PI * n as f64 / 2.0).sin() / (K_PI * n as f64 / 2.0)
        };
        let theta = 2.0 * K_PI * i as f64 / (HB_KN - 1) as f64;
        taps[i] = v
            * (0.35875 - 0.48829 * theta.cos()
                + 0.14128 * (2.0 * theta).cos()
                - 0.01168 * (3.0 * theta).cos());
    }
    taps
}

/// MINPH 形态系数（分解只算一次，全体抽取器共享；Σg=Σp 由分解保证，归一化兜底）。
fn minphase_taps() -> &'static [f32; HB_KN] {
    static TAPS: OnceLock<[f32; HB_KN]> = OnceLock::new();
    TAPS.get_or_init(|| {
        let proto: Vec<f64> = build_proto_taps().to_vec();
        let g = min_phase_factor(&proto, 10f64.powf(-92.0 / 20.0));
        let sum: f64 = g.iter().sum();
        let mut out = [0.0f32; HB_KN];
        for i in 0..HB_KN {
            out[i] = (g[i] / sum) as f32;
        }
        out
    })
}

/// 每级有效群延迟（本级样本，最小相位形态）：分解后冲激的能量重心。
fn gd_per_stage() -> f64 {
    static CENT: OnceLock<f64> = OnceLock::new();
    *CENT.get_or_init(|| {
        let t = minphase_taps();
        let mut e = 0.0;
        let mut c = 0.0;
        for n in 0..HB_KN {
            let v = t[n] as f64;
            e += v * v;
            c += n as f64 * v * v;
        }
        c / e
    })
}

struct HalfbandDec2 {
    tap: [f32; HB_KN],
    tap_rev: [f32; HB_KN],
    state: [f32; HB_KN - 1],
    work: Vec<f32>,
    min_phase: bool,
}

impl HalfbandDec2 {
    fn new() -> Self {
        let mut hb = Self {
            tap: [0.0; HB_KN],
            tap_rev: [0.0; HB_KN],
            state: [0.0; HB_KN - 1],
            work: vec![0.0; HB_KN - 1 + 2048],
            min_phase: false,
        };
        hb.rebuild_taps();
        hb
    }

    fn set_min_phase(&mut self, mp: bool) {
        if mp == self.min_phase {
            return;
        }
        self.min_phase = mp;
        self.rebuild_taps();
    }

    fn rebuild_taps(&mut self) {
        if self.min_phase {
            self.tap = *minphase_taps();
        } else {
            let proto = build_proto_taps();
            let sum: f64 = proto.iter().sum();
            for i in 0..HB_KN {
                self.tap[i] = (proto[i] / sum) as f32;
            }
        }
        for i in 0..HB_KN {
            self.tap_rev[i] = self.tap[HB_KN - 1 - i];
        }
    }

    fn reset(&mut self) {
        self.state = [0.0; HB_KN - 1];
    }

    /// 半带 ×2 抽取：返回输出样本数（nin/2），状态跨帧保持。
    fn process(&mut self, input: &[f32], nin: usize, out: &mut [f32]) -> usize {
        let sz = HB_KN - 1 + nin;
        for i in 0..HB_KN - 1 {
            self.work[i] = self.state[i];
        }
        for i in 0..nin {
            self.work[HB_KN - 1 + i] = input[i];
        }
        let nout = nin / 2;
        let t = &self.tap_rev;
        for j in 0..nout {
            let x = 2 * j;
            let mut acc = 0.0f32;
            for i in 0..HB_KN {
                acc += t[i] * self.work[x + i];
            }
            out[j] = acc;
        }
        for i in 0..HB_KN - 1 {
            self.state[i] = self.work[sz - (HB_KN - 1) + i];
        }
        nout
    }
}

// ── 带定义与引擎 ───────────────────────────────────────────────────────────

struct Band {
    layer: usize,
    win_len: usize,
    read_off: usize,
    wsum_inv: f32,
    kernel_re: Vec<f32>,
    kernel_im: Vec<f32>,
}

pub struct PbtAnalyzer {
    sr: f64,
    lf_mode: i32, // 0=40Hz, 1=20Hz, 2=10Hz
    chan_tri: i32,

    bands: Vec<Band>,
    freqs: Vec<f64>,
    max_win_per_layer: [usize; K_MAX_LAYERS],

    decim: [[HalfbandDec2; K_MAX_LAYERS - 1]; 3],
    layers: [[Vec<f32>; K_MAX_LAYERS]; 3],
    scratch: [[f32; K_HOP / 2]; 2],

    pending: [Vec<f32>; 3],
    buf_count: usize,
    queue: VecDeque<EngineFrame>,
}

impl Default for PbtAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

impl PbtAnalyzer {
    pub fn new() -> Self {
        let mut pbt = Self {
            sr: 48_000.0,
            lf_mode: 0,
            chan_tri: MODE_PWR,
            bands: Vec::new(),
            freqs: Vec::new(),
            max_win_per_layer: [0; K_MAX_LAYERS],
            decim: [
                [(); K_MAX_LAYERS - 1].map(|_| HalfbandDec2::new()),
                [(); K_MAX_LAYERS - 1].map(|_| HalfbandDec2::new()),
                [(); K_MAX_LAYERS - 1].map(|_| HalfbandDec2::new()),
            ],
            layers: [const { [const { Vec::new() }; K_MAX_LAYERS] }; 3],
            scratch: [[0.0; K_HOP / 2]; 2],
            pending: [Vec::new(), Vec::new(), Vec::new()],
            buf_count: 0,
            queue: VecDeque::new(),
        };
        pbt.rebuild_bands();
        pbt
    }

    pub fn set_sample_rate(&mut self, sr: f64) {
        if (self.sr - sr).abs() > 0.1 {
            self.sr = sr;
            self.rebuild_bands();
        }
    }

    /// 低频分辨率档位（Hz）：40 / 20 / 10。返回是否发生档位切换。
    pub fn set_lf_width(&mut self, width_hz: i32) -> bool {
        let mode = if width_hz <= 10 { 2 } else if width_hz <= 20 { 1 } else { 0 };
        if mode != self.lf_mode {
            self.lf_mode = mode;
            self.rebuild_bands();
            true
        } else {
            false
        }
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
    pub fn band_debug(
        &self,
        b: usize,
    ) -> (usize, usize, usize, f32, &[f32], &[f32]) {
        let bd = &self.bands[b];
        (
            bd.layer,
            bd.win_len,
            bd.read_off,
            bd.wsum_inv,
            &bd.kernel_re,
            &bd.kernel_im,
        )
    }

    pub fn reset(&mut self) {
        for c in 0..3 {
            for l in 0..K_MAX_LAYERS {
                self.layers[c][l].clear();
            }
            for l in 0..K_MAX_LAYERS - 1 {
                self.decim[c][l].reset();
            }
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

    // ── 带表重建（对应 C++ `RebuildBands`）───────────────────────────────────

    /// 频带依频率升序 → 层号非增；层 L 速率 = fs/2^L。
    fn assign_layer(band_hi: f64, fs: f64) -> usize {
        let mut l = 0usize;
        while l + 1 < K_MAX_LAYERS {
            let new_nyq = fs / (1 << (l + 2)) as f64;
            if band_hi > K_GUARD * new_nyq {
                break;
            }
            l += 1;
        }
        l
    }

    /// 半带链总群延迟（输入样本）：每级 GdPerStage()，到层 L 共 Σ gd·2^k。
    fn chain_gd(l: usize) -> f64 {
        gd_per_stage() * ((1 << l) - 1) as f64
    }

    fn rebuild_bands(&mut self) {
        // 半带抽取链固定最小相位形态
        for c in 0..3 {
            for l in 0..K_MAX_LAYERS - 1 {
                self.decim[c][l].set_min_phase(true);
            }
        }
        self.bands.clear();
        self.freqs.clear();
        self.max_win_per_layer = [0; K_MAX_LAYERS];

        let fs = self.sr.max(1.0);
        let scale = fs / 48_000.0;
        let nyq_limit = 0.485 * fs;

        let (src_freqs, num_src): (&[f64], usize) = match self.lf_mode {
            2 => (&FREQS_10, 68),
            1 => (&FREQS_20, 60),
            _ => (&FREQS_40, 52),
        };

        struct Spec {
            fc: f64,
            bw: f64,
            layer: usize,
        }
        let mut spec: Vec<Spec> = Vec::new();
        for i in 0..num_src {
            let fc = src_freqs[i] * scale;
            if fc > K_FREQ_HI || fc >= nyq_limit {
                continue;
            }
            // 带宽取频带表相邻间距（首末带单边），与显示网格一致
            let bw = if i == 0 {
                (src_freqs[1] - src_freqs[0]) * scale
            } else if i == num_src - 1 {
                (src_freqs[num_src - 1] - src_freqs[num_src - 2]) * scale
            } else {
                0.5 * (src_freqs[i + 1] - src_freqs[i - 1]) * scale
            };
            let layer = Self::assign_layer(fc + bw / 2.0, fs);
            spec.push(Spec { fc, bw, layer });
        }

        let mut layer_count = [0usize; K_MAX_LAYERS];
        for sp in &spec {
            layer_count[sp.layer] += 1;
        }

        let mut i = 0usize;
        let n = spec.len();
        while i < n {
            let l = spec[i].layer;
            let mut j = i;
            while j < n && spec[j].layer == l {
                j += 1; // 本层 band 段 [i, j)
            }
            // 下一更深带流层（段顶 band 对齐到该层基准时刻）
            let mut next_deep: i32 = -1;
            for d in (l + 1)..K_MAX_LAYERS {
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
    fn add_band(
        &mut self,
        fc: f64,
        bw: f64,
        fs: f64,
        l: usize,
        next_deep: i32,
        f_seg: f64,
    ) {
        let fs_l = fs / (1 << l) as f64;
        // Kaiser 等波纹原型低通：平顶到 0.37bw，阻带墙 Esb=0.13·K·bw 起 −68dB。
        let epb = 0.37 * bw;
        let esb = (0.13 * K_KERNEL_LEN * bw).max(epb + 0.10 * bw); // 过渡带不退化
        let df = esb - epb;
        // 过渡带越窄核越长：T = (A−8)/(2.285·2π·(Esb−Epb))。
        let mut wl = ((68.0 - 8.0) / (2.285 * 2.0 * K_PI * df / fs_l)).ceil() as usize;
        wl = wl.max(9);
        if wl & 1 == 0 {
            wl += 1; // 奇长 → 实对称（最小相位谱分解要求奇长实对称原型）
        }

        // 1) 实对称原型（中心 mid）：Kaiser 窗化 sinc 低通（DC 增益 1）
        let mid = (wl - 1) / 2;
        let mut proto: Vec<f64> = Vec::with_capacity(wl);
        let beta = 0.1102 * (68.0 - 8.7);
        let fc_lp = 0.5 * (epb + esb);
        let wc = 2.0 * K_PI * fc_lp / fs_l; // 原型截止（rad/样本）
        for n in 0..wl {
            let d = n as i32 - mid as i32;
            let h = if d == 0 {
                wc / K_PI
            } else {
                (wc * d as f64).sin() / (K_PI * d as f64)
            };
            let t = 2.0 * n as f64 / (wl - 1) as f64 - 1.0; // −1..1
            let arg = beta * (1.0 - t * t).max(0.0).sqrt();
            let wk = bessel_i0(arg) / bessel_i0(beta);
            proto.push(h * wk);
        }

        // 2) 最小相位谱分解（同幅谱分解，能量前置；核长不变）
        proto = min_phase_factor(&proto, 10f64.powf(-68.0 / 20.0));
        // 时间反转：点积按 "kernel[0] 配最旧样本" 取向；不反转则最小相位核等效最大相位。
        proto.reverse();

        let gd_l = Self::chain_gd(l);
        let gd_d = if next_deep > 0 {
            Self::chain_gd(next_deep as usize)
        } else {
            gd_l
        };
        let r = gd_d + (gd_l - gd_d) * f_seg;
        // 夹非负：负 readOff 会让取窗起点越过缓冲区末端（读越界）
        let read_off = ((r - gd_l) / (1 << l) as f64).round().max(0.0) as usize;

        // 3) 载波调制 + 归一（谱分解保持 DC 增益：按分解结果实测求和）
        let step = 2.0 * K_PI * fc / fs_l;
        let mut ksum = 0.0;
        let mut kernel_re = Vec::with_capacity(wl);
        let mut kernel_im = Vec::with_capacity(wl);
        for n in 0..wl {
            let ph = step * n as f64;
            kernel_re.push((proto[n] * ph.cos()) as f32);
            kernel_im.push((proto[n] * ph.sin()) as f32);
            ksum += proto[n];
        }
        let wsum_inv = if ksum > 1e-12 { (2.0 / ksum) as f32 } else { 0.0 };

        self.bands.push(Band {
            layer: l,
            win_len: wl,
            read_off,
            wsum_inv,
            kernel_re,
            kernel_im,
        });
        self.freqs.push(fc);
        self.max_win_per_layer[l] = self.max_win_per_layer[l].max(wl + read_off);
    }

    fn reset_runtime_state(&mut self) {
        for c in 0..3 {
            for l in 0..K_MAX_LAYERS {
                self.layers[c][l].clear();
            }
            for l in 0..K_MAX_LAYERS - 1 {
                self.decim[c][l].reset();
            }
        }
    }

    /// 层缓冲追加（保留窗口覆盖所需尾部）。
    fn append_layer(&mut self, c: usize, l: usize, src: &[f32]) {
        let max_keep = self.max_win_per_layer[l];
        let buf = &mut self.layers[c][l];
        if src.len() >= max_keep {
            *buf = src[src.len() - max_keep..].to_vec();
            return;
        }
        let old_keep = max_keep - src.len();
        if buf.len() > old_keep {
            buf.drain(..buf.len() - old_keep);
        }
        buf.extend_from_slice(src);
    }

    // ── 帧分析（对应 C++ `PrepareDataForUI`）─────────────────────────────────

    fn analyze_hop(&mut self) -> EngineFrame {
        let nb = self.bands.len();
        if nb == 0 {
            return EngineFrame {
                vals: Vec::new(),
                freqs: self.freqs.clone(),
            };
        }
        let need_l = self.chan_tri != MODE_SUM;
        let need_r = self.chan_tri != MODE_SUM;
        let need_sum = self.chan_tri == MODE_SUM;

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

            // 构建多速率降采样金字塔
            let first = self.pending[c].clone();
            self.append_layer(c, 0, &first);
            let mut cur: Vec<f32> = first;
            let mut nin = cur.len();
            for l in 0..K_MAX_LAYERS - 1 {
                let nout = {
                    let dst = &mut self.scratch[l & 1];
                    self.decim[c][l].process(&cur, nin, dst)
                };
                let produced: Vec<f32> = self.scratch[l & 1][..nout].to_vec();
                self.append_layer(c, l + 1, &produced);
                cur = produced;
                nin = nout;
            }

            // band 点积（复数解调核相关，幅度 = |Σ x·w·e^{jωn}|·2/Σw）
            for b in 0..nb {
                let bd = &self.bands[b];
                let buf = &self.layers[c][bd.layer];
                let wl = bd.win_len;
                // readOff 夹非负 + 冷启动短缓冲部分窗（零窗长跳过）：取窗不越界
                let have = (wl).min(buf.len().saturating_sub(bd.read_off)).max(0);
                let skip = wl - have;
                let start = buf.len().saturating_sub(bd.read_off).saturating_sub(have);
                let x = &buf[start..start + have];
                let kr = &bd.kernel_re[skip..];
                let ki = &bd.kernel_im[skip..];
                let cnt = wl - skip;
                let mut re = 0.0f32;
                let mut im = 0.0f32;
                for j in 0..cnt {
                    re += x[j] * kr[j];
                    im += x[j] * ki[j];
                }
                vals[b][c] = (re * re + im * im).sqrt() * bd.wsum_inv;
            }
        }

        EngineFrame {
            vals,
            freqs: self.freqs.clone(),
        }
    }
}
