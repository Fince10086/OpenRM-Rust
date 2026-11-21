//! LoudnessMeter — BS.1770-4 / EBU R128 / EBU Tech 3342 响度测量引擎。
//!
//! 逐行移植自 `plugins/Analyzer/src/dsp/LoudnessMeter.h`（318 行）。
//!
//! 测量量：
//!   * M  Momentary  400ms 滑动窗（100ms 步进，无门限），LUFS
//!   * S  Short-Term 3000ms 滑动窗（100ms 步进，无门限），LUFS
//!   * I  Integrated  400ms 窗双门限（绝对 -70 LUFS + 相对 均值-10 LU），LUFS
//!   * LRA            EBU 3342：3s 块分布 10%..95% 最近秩百分位差，LU
//!   * TP Max         真峰值锁存 dBTP（由调用方喂入，本引擎不做过采样）
//!
//! `LoudnessSnapshot` 的字段名/默认值与 C++ `LoudnessMeter::Snapshot` 一一对应，
//! UI 侧（电平/响度表头、bracket）依赖它，改动前先看 `plugins/analyzer/PORT-SPEC.md`。

#[derive(Clone, Copy, Debug)]
pub struct LoudnessSnapshot {
    pub momentary: f32,
    pub short_term: f32,
    pub integrated: f32,
    pub range: f32,
    pub lra_min: f32,
    pub lra_max: f32,
    pub tp_max: f32,
    pub i_valid: bool,
    pub lra_valid: bool,
}

impl Default for LoudnessSnapshot {
    fn default() -> Self {
        Self {
            momentary: -120.0,
            short_term: -120.0,
            integrated: -120.0,
            range: 0.0,
            lra_min: -120.0,
            lra_max: -120.0,
            tp_max: -120.0,
            i_valid: false,
            lra_valid: false,
        }
    }
}

const K_MOMENTARY_BLOCKS: usize = 4; // 400ms
const K_SHORT_BLOCKS: usize = 30; // 3000ms
const K_RING_SIZE: usize = K_SHORT_BLOCKS;
const K_LRA_MIN_HOPS: usize = 30; // 3s：首个 3s 块后即开始显示
const K_LRA_BINS: usize = 1200; // LUFS -120..0，0.1 LU 步进
/// 10^((-70+0.691)/10)，绝对门限能量
const K_ABS_GATE_Z: f64 = 1.172_443_196_018_975e-7;
/// I 相对门限：均值 -10 LU
const K_REL_GATE_FACTOR: f64 = 0.1;
/// LRA 相对门限：均值 -20 LU（EBU 3342）
const K_LRA_GATE_FACTOR: f64 = 0.01;

const PI: f64 = 3.141_592_653_589_793_238_46;

/// 四阶直 II 型滤波器（与 libebur128 同构；双二阶级联展开成单段，状态 4 个）。
#[derive(Clone, Copy)]
struct KFilter {
    b: [f64; 5],
    a: [f64; 5],
    v: [f64; 4],
}

impl KFilter {
    fn new() -> Self {
        Self {
            b: [1.0, 0.0, 0.0, 0.0, 0.0],
            a: [1.0, 0.0, 0.0, 0.0, 0.0],
            v: [0.0; 4],
        }
    }

    fn process(&mut self, x: f64) -> f64 {
        let w = x - self.a[1] * self.v[0] - self.a[2] * self.v[1]
            - self.a[3] * self.v[2]
            - self.a[4] * self.v[3];
        let y = self.b[0] * w
            + self.b[1] * self.v[0]
            + self.b[2] * self.v[1]
            + self.b[3] * self.v[2]
            + self.b[4] * self.v[3];
        self.v[3] = self.v[2];
        self.v[2] = self.v[1];
        self.v[1] = self.v[0];
        self.v[0] = w;
        y
    }

    fn reset(&mut self) {
        self.v = [0.0; 4];
    }
}

/// K 加权四阶 IIR 系数（BS.1770-4 附录 2；与 libebur128 `ebur128_init_filter` 相同）。
///
/// 级联 [1681.974Hz 架形双二阶（含 +4dB @HF）] × [(1-z⁻¹)² / 38.135Hz 二阶高通]。
/// 第二级是有名数 0.5003270373238773 的纯高通（分子 (1-z⁻¹)² 并入 b 多项式）。
pub fn k_weight_coeffs(sr: f64) -> ([f64; 5], [f64; 5]) {
    // 级 1 双二阶
    let f0 = 1681.974_450_955_533;
    let g = 3.999_843_853_973_347;
    let q = 0.707_175_236_955_419_6;
    let k = (PI * f0 / sr).tan();
    let vh = 10.0_f64.powf(g / 20.0);
    let vb = vh.powf(0.499_666_774_154_541_6);
    let a0 = 1.0 + k / q + k * k;
    let pb0 = (vh + vb * k / q + k * k) / a0;
    let pb1 = 2.0 * (k * k - vh) / a0;
    let pb2 = (vh - vb * k / q + k * k) / a0;
    let pa1 = 2.0 * (k * k - 1.0) / a0;
    let pa2 = (1.0 - k / q + k * k) / a0;

    // 级 2（仅分母；分子 (1-z⁻¹)² 见下方 rb）
    let f1 = 38.135_470_876_024_44;
    let q1 = 0.500_327_037_323_877_3;
    let k1 = (PI * f1 / sr).tan();
    let q0 = 1.0 + k1 / q1 + k1 * k1;
    let ra1 = 2.0 * (k1 * k1 - 1.0) / q0;
    let ra2 = (1.0 - k1 / q1 + k1 * k1) / q0;

    // 多项式卷积：b = pb ⊗ [1,-2,1]；a = pa ⊗ [1,ra1,ra2]
    let b = [
        pb0,
        -2.0 * pb0 + pb1,
        pb0 - 2.0 * pb1 + pb2,
        pb1 - 2.0 * pb2,
        pb2,
    ];
    let a = [
        1.0,
        ra1 + pa1,
        ra2 + pa1 * ra1 + pa2,
        pa1 * ra2 + pa2 * ra1,
        pa2 * ra2,
    ];
    (b, a)
}

fn loudness_from_energy(z: f64) -> f32 {
    if z <= 1e-12 {
        -120.0
    } else {
        (-0.691 + 10.0 * z.log10()).max(-120.0) as f32
    }
}

fn bin_of(lufs: f32) -> usize {
    (((lufs + 120.0) * 10.0).round() as i32).clamp(0, K_LRA_BINS as i32 - 1) as usize
}

#[derive(Clone, Copy, Default)]
struct LraResult {
    range: f32,
    lra_min: f32,
    lra_max: f32,
}

pub struct LoudnessMeter {
    sr: f64,
    kf_l: KFilter,
    kf_r: KFilter,
    acc_l: f64,
    acc_r: f64,
    sub_len: usize,
    sub_pos: usize,
    hops: u64,
    ring: [f32; K_RING_SIZE],
    head: usize,
    filled: usize,
    sum_s: f64,
    gate_blocks: Vec<f64>, // 通过绝对门限的 400ms 窗能量（会话内累计，RESET 清空）
    gate_sum: f64,
    hist: [u32; K_LRA_BINS],
    cur_m: f32,
    cur_s: f32,
    cur_i: f32,
    cur_lra: f32,
    cur_lra_min: f32,
    cur_lra_max: f32,
    tp_max: f32,
    i_valid: bool,
    lra_valid: bool,
}

impl Default for LoudnessMeter {
    fn default() -> Self {
        Self::new()
    }
}

impl LoudnessMeter {
    pub fn new() -> Self {
        let mut lm = Self {
            sr: 48_000.0,
            kf_l: KFilter::new(),
            kf_r: KFilter::new(),
            acc_l: 0.0,
            acc_r: 0.0,
            sub_len: 4800,
            sub_pos: 0,
            hops: 0,
            ring: [0.0; K_RING_SIZE],
            head: 0,
            filled: 0,
            sum_s: 0.0,
            gate_blocks: Vec::new(),
            gate_sum: 0.0,
            hist: [0; K_LRA_BINS],
            cur_m: -120.0,
            cur_s: -120.0,
            cur_i: -120.0,
            cur_lra: 0.0,
            cur_lra_min: -120.0,
            cur_lra_max: -120.0,
            tp_max: -120.0,
            i_valid: false,
            lra_valid: false,
        };
        lm.set_sample_rate(48_000.0);
        lm
    }

    pub fn set_sample_rate(&mut self, sr: f64) {
        self.sr = sr;
        let (b, a) = k_weight_coeffs(sr);
        self.kf_l.b = b;
        self.kf_l.a = a;
        self.kf_r = self.kf_l;
        self.sub_len = ((sr * 0.1).round() as usize).max(1);
        self.reset();
    }

    pub fn reset(&mut self) {
        self.kf_l.reset();
        self.kf_r.reset();
        self.acc_l = 0.0;
        self.acc_r = 0.0;
        self.sub_pos = 0;
        self.hops = 0;
        self.filled = 0;
        self.head = 0;
        self.ring = [0.0; K_RING_SIZE];
        self.sum_s = 0.0;
        self.gate_blocks.clear();
        self.gate_sum = 0.0;
        self.hist = [0; K_LRA_BINS];
        self.cur_m = -120.0;
        self.cur_s = -120.0;
        self.cur_i = -120.0;
        self.cur_lra = 0.0;
        self.cur_lra_min = -120.0;
        self.cur_lra_max = -120.0;
        self.tp_max = -120.0;
        self.i_valid = false;
        self.lra_valid = false;
    }

    /// 音频线程逐块喂入（L/R 为宿主原始输入，立体声权重各 1.0）。
    pub fn process(&mut self, l: &[f32], r: &[f32]) {
        let n = l.len().min(r.len());
        for i in 0..n {
            let lf = self.kf_l.process(l[i] as f64);
            let rf = self.kf_r.process(r[i] as f64);
            self.acc_l += lf * lf;
            self.acc_r += rf * rf;
            self.sub_pos += 1;
            if self.sub_pos >= self.sub_len {
                self.sub_pos = 0;
                self.flush_sub();
                // 子块能量已在 flush_sub 消费，清零开始下一子块
                self.acc_l = 0.0;
                self.acc_r = 0.0;
            }
        }
    }

    /// 喂入当前真峰值（L/R，dBTP）：锁存历史最大（复用 LevelMeter 的 4x 过采样结果）。
    pub fn set_true_peaks(&mut self, tp_l: f32, tp_r: f32) {
        self.tp_max = self.tp_max.max(tp_l.max(tp_r));
    }

    pub fn store(&self) -> LoudnessSnapshot {
        LoudnessSnapshot {
            momentary: self.cur_m,
            short_term: self.cur_s,
            integrated: self.cur_i,
            range: self.cur_lra,
            lra_min: self.cur_lra_min,
            lra_max: self.cur_lra_max,
            tp_max: self.tp_max,
            i_valid: self.i_valid,
            lra_valid: self.lra_valid,
        }
    }

    /// 每 100ms 子块边界：更新 M/S、I 门限块、LRA 直方图。
    fn flush_sub(&mut self) {
        let z_sub = (self.acc_l + self.acc_r) / self.sub_len as f64; // 100ms 双声道加权能量 (G=1)
        let old = self.ring[self.head]; // 即将被覆盖的旧值（前 30 块为 0）
        self.ring[self.head] = z_sub as f32;
        self.head = (self.head + 1) % K_RING_SIZE;
        if self.filled < K_RING_SIZE {
            self.filled += 1;
        }
        self.sum_s += z_sub - old as f64; // 运行和：最近 30 子块
        self.hops += 1;

        // 最近 400ms 窗能量（M 与 I 门限块同源）
        let mut sum_m = 0.0;
        for i in 0..K_MOMENTARY_BLOCKS {
            let idx = (self.head + K_RING_SIZE - 1 - i) % K_RING_SIZE;
            sum_m += self.ring[idx] as f64;
        }

        // M（最近 400ms）/ S（最近 3000ms）
        if self.filled >= K_MOMENTARY_BLOCKS {
            self.cur_m = loudness_from_energy(sum_m / K_MOMENTARY_BLOCKS as f64);
        }
        if self.filled >= K_SHORT_BLOCKS {
            self.cur_s = loudness_from_energy(self.sum_s / K_SHORT_BLOCKS as f64);
        }

        // I：400ms 窗双门限（窗与 M 同源：100ms 步进，75% 重叠）
        if self.filled >= K_MOMENTARY_BLOCKS {
            let z400 = sum_m / K_MOMENTARY_BLOCKS as f64; // 400ms 窗平均能量
            if z400 >= K_ABS_GATE_Z {
                // 绝对门限 -70 LUFS
                self.gate_blocks.push(z400);
                self.gate_sum += z400;
            }
            if !self.gate_blocks.is_empty() {
                // 相对门限重扫（精确）：阈值 = 绝对门限通过块均值 - 10 LU
                let rel_thr = K_REL_GATE_FACTOR * (self.gate_sum / self.gate_blocks.len() as f64);
                let mut sum_keep = 0.0;
                let mut n_keep: u64 = 0;
                for &z in &self.gate_blocks {
                    if z >= rel_thr {
                        sum_keep += z;
                        n_keep += 1;
                    }
                }
                if n_keep > 0 {
                    self.cur_i = loudness_from_energy(sum_keep / n_keep as f64);
                    self.i_valid = true;
                } else {
                    // 全部块被相对门限剔除（病态节目），与参考实现一致
                    self.cur_i = -120.0;
                    self.i_valid = false;
                }
            }
        }

        // LRA：3s 块直方图（仅收录 ≥ 绝对门限的块）
        if self.filled >= K_SHORT_BLOCKS {
            let z_s = self.sum_s / K_SHORT_BLOCKS as f64;
            if z_s >= K_ABS_GATE_Z {
                let bin = bin_of(loudness_from_energy(z_s));
                self.hist[bin] += 1;
            }
        }
        if self.hops >= K_LRA_MIN_HOPS as u64 {
            let r = self.lra_from_hist();
            self.cur_lra = r.range;
            self.cur_lra_min = r.lra_min;
            self.cur_lra_max = r.lra_max;
            self.lra_valid = true;
        }
    }

    /// EBU 3342：剔除低于（均值-20 LU）的 3s 块后，取 10%..95% 最近秩百分位差。
    /// 同时暴露端点 lraMin/lraMax（10%/95% 百分位的 LUFS 值）供 UI 用作 bracket 上下臂。
    fn lra_from_hist(&self) -> LraResult {
        // kEnergy[i] = 10^((-120 + 0.1*i + 0.691)/10)
        let mut mean_p = 0.0;
        let mut n: u64 = 0;
        for i in 0..K_LRA_BINS {
            let energy = 10.0_f64.powf((-120.0 + 0.1 * i as f64 + 0.691) / 10.0);
            mean_p += self.hist[i] as f64 * energy;
            n += self.hist[i] as u64;
        }
        if n == 0 {
            return LraResult {
                range: 0.0,
                lra_min: -120.0,
                lra_max: -120.0,
            };
        }
        mean_p /= n as f64;
        let rel_thr = K_LRA_GATE_FACTOR * mean_p;
        let mut kept: u64 = 0;
        for i in 0..K_LRA_BINS {
            let energy = 10.0_f64.powf((-120.0 + 0.1 * i as f64 + 0.691) / 10.0);
            if energy >= rel_thr {
                kept += self.hist[i] as u64;
            }
        }
        if kept <= 1 {
            return LraResult {
                range: 0.0,
                lra_min: -120.0,
                lra_max: -120.0,
            };
        }
        let p10 = (kept - 1) as f64 * 0.10 + 0.5;
        let p95 = (kept - 1) as f64 * 0.95 + 0.5;
        let mut cum: u64 = 0;
        let mut l_bin: i32 = -1;
        let mut h_bin: i32 = -1;
        for i in 0..K_LRA_BINS {
            let energy = 10.0_f64.powf((-120.0 + 0.1 * i as f64 + 0.691) / 10.0);
            if energy < rel_thr {
                continue;
            }
            cum += self.hist[i] as u64;
            if l_bin < 0 && cum as f64 > p10 {
                l_bin = i as i32;
            }
            if h_bin < 0 && cum as f64 > p95 {
                h_bin = i as i32;
                break;
            }
        }
        if l_bin < 0 || h_bin < 0 {
            return LraResult {
                range: 0.0,
                lra_min: -120.0,
                lra_max: -120.0,
            };
        }
        LraResult {
            range: (h_bin - l_bin) as f32 * 0.1,
            lra_min: l_bin as f32 * 0.1 - 120.0,
            lra_max: h_bin as f32 * 0.1 - 120.0,
        }
    }
}
