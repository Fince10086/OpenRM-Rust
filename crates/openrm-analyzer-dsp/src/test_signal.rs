//! TestSignalGenerator — 分析仪内置测试信号源（18 种）。
//!
//! 逐行移植自 `plugins/Analyzer/src/dsp/TestSignalGenerator.h`（336 行）：
//! splitmix32 播种 / xorshift32 白噪、Kellet 7 极点粉噪、带限方波（谐波递推）、
//! 对数扫频、脉冲串、双音、双通道相干性变体等。默认 `ORM_ENABLE_TEST_GEN=1`。

const K_PI: f64 = 3.14159265358979323846;
const K_TWO_PI: f64 = 6.283185307179586476925286766559;
const K_FOUR_OVER_PI: f64 = 1.2732395447351626861510701069801;

/// 信号类型（与原版 `ETestSignal` 枚举值一一对应，下标不可改动）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
#[repr(i32)]
pub enum TestSignalType {
    #[default]
    Off = 0,
    Sine = 1,
    SineL = 2,
    SineR = 3,
    Sine90 = 4,
    SinePanSweep = 5,
    SinePhaseSweep = 6,
    DualAzimuth = 7,
    HarmonicAnti = 8,
    SweepLog = 9,
    WhiteNoise = 10,
    PinkNoise = 11,
    NoiseC = 12,
    PinkC = 13,
    ImpulseTrain = 14,
    TwoTone = 15,
    Square = 16,
    Silence = 17,
}

impl TestSignalType {
    pub fn from_i32(v: i32) -> Self {
        match v {
            1 => Self::Sine,
            2 => Self::SineL,
            3 => Self::SineR,
            4 => Self::Sine90,
            5 => Self::SinePanSweep,
            6 => Self::SinePhaseSweep,
            7 => Self::DualAzimuth,
            8 => Self::HarmonicAnti,
            9 => Self::SweepLog,
            10 => Self::WhiteNoise,
            11 => Self::PinkNoise,
            12 => Self::NoiseC,
            13 => Self::PinkC,
            14 => Self::ImpulseTrain,
            15 => Self::TwoTone,
            16 => Self::Square,
            17 => Self::Silence,
            _ => Self::Off,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Off => "OFF",
            Self::Sine => "SINE",
            Self::SineL => "SINE L",
            Self::SineR => "SINE R",
            Self::Sine90 => "SINE 90°",
            Self::SinePanSweep => "PAN SWEEP",
            Self::SinePhaseSweep => "PHASE SWEEP",
            Self::DualAzimuth => "DUAL AZ",
            Self::HarmonicAnti => "HARM ANTI",
            Self::SweepLog => "LOG SWEEP",
            Self::WhiteNoise => "WHITE",
            Self::PinkNoise => "PINK",
            Self::NoiseC => "NOISE C",
            Self::PinkC => "PINK C",
            Self::ImpulseTrain => "IMPULSE",
            Self::TwoTone => "TWO TONE",
            Self::Square => "SQUARE",
            Self::Silence => "SILENCE",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct TestSignalConfig {
    pub type_: TestSignalType,
    /// 正弦/方波/双音基频，或脉冲串速率 (Hz)。
    pub freq_hz: f64,
    /// 输出电平 (dBFS 峰值)。
    pub level_db: f64,
    /// bin 吸附用 FFT 网格尺寸（当前信号生成未直接使用，保留对齐原版）。
    pub fft_size: i32,
    /// 对数扫频周期 / 摆动信号往返周期 (s)。
    pub sweep_sec: f64,
    pub sweep_f0: f64,
    pub sweep_f1: f64,
}

impl Default for TestSignalConfig {
    fn default() -> Self {
        Self {
            type_: TestSignalType::Off,
            freq_hz: 1000.0,
            level_db: -12.0,
            fft_size: 4096,
            sweep_sec: 4.0,
            sweep_f0: 20.0,
            sweep_f1: 20_000.0,
        }
    }
}

pub struct TestSignalGenerator {
    cfg: TestSignalConfig,
    seed: u32,
    rng_l: u32,
    rng_r: u32,
    pink_l: [f64; 7],
    pink_r: [f64; 7],
    index: u64,
    impulse_idx: u64,
}

impl TestSignalGenerator {
    pub fn new() -> Self {
        let mut me = Self {
            cfg: TestSignalConfig::default(),
            seed: 0x12345678,
            rng_l: 0x1234567,
            rng_r: 0x89abcde,
            pink_l: [0.0; 7],
            pink_r: [0.0; 7],
            index: 0,
            impulse_idx: u64::MAX,
        };
        me.reseed();
        me
    }

    pub fn active(&self) -> bool {
        self.cfg.type_ != TestSignalType::Off
    }

    pub fn type_(&self) -> TestSignalType {
        self.cfg.type_
    }

    pub fn config(&self) -> &TestSignalConfig {
        &self.cfg
    }

    /// 仅在配置真的变化时做重活；信号类型切换时自动 Restart（从头开始，可复现）。
    pub fn set_config(&mut self, c: TestSignalConfig) {
        if c.type_ == self.cfg.type_
            && c.freq_hz == self.cfg.freq_hz
            && c.level_db == self.cfg.level_db
            && c.fft_size == self.cfg.fft_size
            && c.sweep_sec == self.cfg.sweep_sec
            && c.sweep_f0 == self.cfg.sweep_f0
            && c.sweep_f1 == self.cfg.sweep_f1
        {
            return;
        }
        let type_changed = c.type_ != self.cfg.type_;
        self.cfg = c;
        if type_changed {
            self.restart();
        }
    }

    pub fn set_seed(&mut self, seed: u32) {
        self.seed = seed;
        self.reseed();
    }

    pub fn seed(&self) -> u32 {
        self.seed
    }

    /// 信号归零：样本索引归 0，相位/滤波器/PRNG 全部复位。
    pub fn restart(&mut self) {
        self.index = 0;
        self.impulse_idx = u64::MAX;
        self.pink_l = [0.0; 7];
        self.pink_r = [0.0; 7];
        self.reseed();
    }

    /// 当前信号实际频率（UI 显示用；噪声类不适用）。
    pub fn effective_freq_hz(&self) -> f64 {
        self.cfg.freq_hz
    }

    fn reseed(&mut self) {
        self.rng_l = splitmix32(self.seed);
        self.rng_r = splitmix32(self.seed ^ 0x9e3779b9);
        if self.rng_l == 0 {
            self.rng_l = 0x1234567; // xorshift 的零状态是吸收态
        }
        if self.rng_r == 0 {
            self.rng_r = 0x89abcde;
        }
    }

    /// xorshift32，均匀映射到 [-1, 1)。
    fn white(s: &mut u32) -> f64 {
        *s ^= *s << 13;
        *s ^= *s >> 17;
        *s ^= *s << 5;
        (*s as i32 as f64) * (1.0 / 2_147_483_648.0)
    }

    /// Paul Kellet 7 极点 -3 dB/oct 近似粉噪。
    fn pink(b: &mut [f64; 7], w: f64) -> f64 {
        b[0] = 0.99886 * b[0] + w * 0.0555179;
        b[1] = 0.99332 * b[1] + w * 0.0750759;
        b[2] = 0.96900 * b[2] + w * 0.1538520;
        b[3] = 0.86650 * b[3] + w * 0.3104856;
        b[4] = 0.55000 * b[4] + w * 0.5329522;
        b[5] = -0.7616 * b[5] - w * 0.0168980;
        let out = b[0] + b[1] + b[2] + b[3] + b[4] + b[5] + b[6] + w * 0.5362;
        b[6] = w * 0.115926;
        out * 0.11
    }

    /// 带限方波：(4/π)·Σ_{k odd} sin(k·x)/k，谐波截断在 0.9·Nyquist 以下。
    /// 谐波正弦走二项递推 sin((k+2)x) = 2cos(2x)sin(kx) − sin((k−2)x)。
    fn band_limited_square(x: f64, f: f64, nyq: f64) -> f64 {
        if f <= 0.0 {
            return 0.0;
        }
        let max_k = ((0.9 * nyq / f).floor() as i32).min(63);
        let mut s_km2 = -x.sin(); // sin((k−2)x), k=1 时为 sin(−x)
        let mut s_k = x.sin();
        let mut sum = s_k;
        if max_k < 3 {
            return K_FOUR_OVER_PI * sum;
        }
        let c2 = (2.0 * x).cos();
        let mut k = 3;
        while k <= max_k {
            let s_next = 2.0 * c2 * s_k - s_km2;
            s_km2 = s_k;
            s_k = s_next;
            sum += s_next / k as f64;
            k += 2;
        }
        K_FOUR_OVER_PI * sum
    }

    /// 填充 n 个样本；`advance=false` 时不推进样本索引（冻结取段用）。
    pub fn fill(&mut self, dst_l: &mut [f32], dst_r: &mut [f32], sr: f64, advance: bool) {
        let n = dst_l.len().min(dst_r.len());
        if !self.active() || n == 0 || sr <= 0.0 {
            return;
        }

        let amp = 10.0_f64.powf(self.cfg.level_db / 20.0);
        let inv_sr = 1.0 / sr;
        let nyq = 0.5 * sr;

        let f = self.cfg.freq_hz.clamp(0.0, nyq * 0.95);
        let w = K_TWO_PI * f;

        for i in 0..n {
            let t = self.index as f64 * inv_sr; // 绝对时间，跨 block 连续
            let l: f64;
            let r: f64;

            match self.cfg.type_ {
                TestSignalType::Sine => {
                    let v = amp * (w * t).sin();
                    l = v;
                    r = v;
                }
                TestSignalType::SineL => {
                    l = amp * (w * t).sin();
                    r = 0.0;
                }
                TestSignalType::SineR => {
                    l = 0.0;
                    r = amp * (w * t).sin();
                }
                TestSignalType::Sine90 => {
                    // 右路超前 90°: 相关性 = 0。
                    l = amp * (w * t).sin();
                    r = amp * (w * t).cos();
                }
                TestSignalType::SinePanSweep => {
                    // 同相恒功率摆: α = (π/4)(1−cos 2πt/T) ∈ [0, π/2] 往返。
                    let period = self.cfg.sweep_sec.max(0.05);
                    let alpha = 0.25 * K_PI
                        * (1.0 - (K_TWO_PI * (t % period) / period).cos());
                    let s = (w * t).sin();
                    l = amp * alpha.cos() * s;
                    r = amp * alpha.sin() * s;
                }
                TestSignalType::SinePhaseSweep => {
                    // 右路相对相位 φ = (1−cos 2πt/T)·π − π ∈ [−π, π] 往返。
                    let period = self.cfg.sweep_sec.max(0.05);
                    let phi = (1.0 - (K_TWO_PI * (t % period) / period).cos()) * K_PI - K_PI;
                    l = amp * (w * t).sin();
                    r = amp * (w * t + phi).sin();
                }
                TestSignalType::DualAzimuth => {
                    // 双频双方位: f 与 3.5f 两个同相正弦, 幅度权重 0.9/0.4。
                    let f2 = (f * 3.5).min(nyq * 0.95);
                    let w2 = K_TWO_PI * f2;
                    const A1: f64 = 0.9 / 1.3;
                    const A2: f64 = 0.4 / 1.3;
                    let s1 = (w * t).sin();
                    let s2 = (w2 * t).sin();
                    l = amp * (A1 * s1 + A2 * s2);
                    r = amp * (A2 * s1 + A1 * s2);
                }
                TestSignalType::HarmonicAnti => {
                    // 基波同相同幅 + 二次谐波反相 (0.588/0.412 加权)。
                    let w2 = 2.0 * w;
                    const A1: f64 = 1.0 / 1.7;
                    const A2: f64 = 0.7 / 1.7;
                    let s1 = (w * t).sin();
                    let s2 = (w2 * t).sin();
                    l = amp * (A1 * s1 + A2 * s2);
                    r = amp * (A1 * s1 - A2 * s2);
                }
                TestSignalType::SweepLog => {
                    // 相位积分: φ(t) = 2π·f0·T/ln(ratio)·(ratio^(t/T) − 1)。
                    let period = self.cfg.sweep_sec.max(0.05);
                    let tt = t % period;
                    let ratio = self.cfg.sweep_f1.max(1e-6) / self.cfg.sweep_f0.max(1e-6);
                    let kk = ratio.ln();
                    let ph = K_TWO_PI * self.cfg.sweep_f0 * period / kk
                        * ((tt * kk / period).exp() - 1.0);
                    let v = amp * ph.sin();
                    l = v;
                    r = v;
                }
                TestSignalType::WhiteNoise => {
                    l = amp * Self::white(&mut self.rng_l);
                    r = amp * Self::white(&mut self.rng_r);
                }
                TestSignalType::PinkNoise => {
                    l = amp * Self::pink(&mut self.pink_l, Self::white(&mut self.rng_l));
                    r = amp * Self::pink(&mut self.pink_r, Self::white(&mut self.rng_r));
                }
                TestSignalType::NoiseC => {
                    // 同源白噪: L = R 完全相关。
                    let n = Self::white(&mut self.rng_l);
                    let v = amp * n;
                    l = v;
                    r = v;
                }
                TestSignalType::PinkC => {
                    let n = Self::pink(&mut self.pink_l, Self::white(&mut self.rng_l));
                    let v = amp * n;
                    l = v;
                    r = v;
                }
                TestSignalType::ImpulseTrain => {
                    let rate = self.cfg.freq_hz.clamp(0.5, 50.0);
                    let idx = (t * rate) as u64;
                    let fire = idx != self.impulse_idx;
                    if fire {
                        self.impulse_idx = idx;
                    }
                    let v = if fire { amp } else { 0.0 };
                    l = v;
                    r = v;
                }
                TestSignalType::TwoTone => {
                    // 两个等幅音, 相距 1/3 八度; 各占 amp/2。
                    let f2 = (f * 1.2599210498948732).min(nyq * 0.95); // 2^(1/3)
                    let a = amp * 0.5;
                    let v = a * (w * t).sin() + a * (K_TWO_PI * f2 * t).sin();
                    l = v;
                    r = v;
                }
                TestSignalType::Square => {
                    let v = amp * Self::band_limited_square(w * t, f, nyq);
                    l = v;
                    r = v;
                }
                TestSignalType::Silence => {
                    l = 0.0;
                    r = 0.0;
                }
                TestSignalType::Off => {
                    l = 0.0;
                    r = 0.0;
                }
            }

            dst_l[i] = l as f32;
            dst_r[i] = r as f32;
            if advance {
                self.index += 1;
            }
        }
    }
}

impl Default for TestSignalGenerator {
    fn default() -> Self {
        Self::new()
    }
}

fn splitmix32(x: u32) -> u32 {
    let mut x = x.wrapping_add(0x9e3779b9);
    x = (x ^ (x >> 16)).wrapping_mul(0x85ebca6b);
    x = (x ^ (x >> 13)).wrapping_mul(0xc2b2ae35);
    x ^ (x >> 16)
}
