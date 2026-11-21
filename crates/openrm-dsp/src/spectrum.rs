//! BandPass 谱垫的频谱引擎（对应 C++ `SpectrumSTFT` + `FilterNodePad` 内部聚合）。
//!
//! 关键常量必须与原版一致，否则频谱高度/斜率整体跑偏：
//!   - 4096 点、4 倍重叠（hop 1024）、4 阶 Blackman-Harris 窗；
//!   - 标定 `kCalHalf`：满幅正弦读 0.707（-3 dB）；
//!   - 256 个对数频段，20 Hz–20 kHz；
//!   - 显示域 3 dB/oct 补偿，支点 sqrt(20*20000)=632.45553 Hz；
//!   - [10,20) Hz 作为锚点桶，用于低频空桶外推。
//!
//! 注意：C++ 版的 FFT 在 UI 线程做（音频线程只按 hop 推原始样本），这里保持同样分工。

use realfft::RealFftPlanner;
use realfft::RealToComplex;

pub const SPECTRUM_BANDS: usize = 256;
pub const SPEC_LO: f32 = 20.0;
pub const SPEC_HI: f32 = 20_000.0;
pub const SPECTRUM_BOTTOM_DB: f32 = -85.0;
const ANCHOR_LO: f32 = 10.0;
const ANCHOR_HZ: f32 = 14.142_136; // sqrt(10*20)
const SUB_BAND: i32 = -2;
const SLOPE_REF_HZ: f32 = 632.455_53; // sqrt(20*20000)
const CAL_DC: f32 = 0.707_106_8;
const CAL_BIN: f32 = 1.414_213_6;

/// 单个通道的 STFT：环形历史 + 窗 + 实数 FFT + 幅度标定。
pub struct Stft {
    fft_size: usize,
    hop: usize,
    num_bins: usize,
    window: Vec<f32>,
    /// sum(window)；C++ 里 `mScaling = sum^2` 与开方抵消，等价于除以 sum
    window_sum: f32,
    history: Vec<f32>,
    windowed: Vec<f32>,
    r2c: std::sync::Arc<dyn RealToComplex<f32>>,
    complex: Vec<realfft::num_complex::Complex<f32>>,
    scratch: Vec<realfft::num_complex::Complex<f32>>,
}

impl Stft {
    pub fn new(fft_size: usize, overlap: usize) -> Self {
        let hop = (fft_size / overlap.max(1)).max(1);
        let mut planner = RealFftPlanner::<f32>::new();
        let r2c = planner.plan_fft_forward(fft_size);
        let complex = r2c.make_output_vec();
        let scratch = r2c.make_scratch_vec();

        // 4-term Blackman-Harris，系数与原版逐字一致
        let m = (fft_size - 1) as f32;
        let (a0, a1, a2, a3) = (0.358_75_f32, 0.488_29, 0.141_28, 0.011_68);
        let mut window = vec![0.0; fft_size];
        let mut sum = 0.0f64;
        for (i, w) in window.iter_mut().enumerate() {
            let theta = 2.0 * std::f32::consts::PI * i as f32 / m;
            *w = a0 - a1 * theta.cos() + a2 * (2.0 * theta).cos() - a3 * (3.0 * theta).cos();
            sum += *w as f64;
        }

        Self {
            fft_size,
            hop,
            num_bins: fft_size / 2,
            window,
            window_sum: sum as f32,
            history: vec![0.0; fft_size],
            windowed: vec![0.0; fft_size],
            r2c,
            complex,
            scratch,
        }
    }

    pub fn hop(&self) -> usize {
        self.hop
    }

    pub fn num_bins(&self) -> usize {
        self.num_bins
    }

    /// 推入一个 hop 的原始样本，输出该帧的幅度谱（长度为 `num_bins`，不含 Nyquist）。
    pub fn push_hop(&mut self, hop_samples: &[f32], out: &mut [f32]) {
        let fft_size = self.fft_size;
        let hop = self.hop;
        self.history.copy_within(hop.., 0);
        let tail = fft_size - hop;
        self.history[tail..].copy_from_slice(&hop_samples[..hop.min(hop_samples.len())]);

        for i in 0..fft_size {
            self.windowed[i] = self.history[i] * self.window[i];
        }

        let _ = self
            .r2c
            .process_with_scratch(&mut self.windowed, &mut self.complex, &mut self.scratch);

        out[0] = self.complex[0].re.abs() * CAL_DC / self.window_sum;
        for i in 1..self.num_bins.min(out.len()) {
            let c = self.complex[i];
            out[i] = (c.re * c.re + c.im * c.im).sqrt() * CAL_BIN / self.window_sum;
        }
    }
}

/// 一个谱垫的完整显示链路：干/湿两条 STFT、平滑、256 段聚合、低频外推、斜率补偿。
pub struct PadSpectrum {
    sample_rate: f64,
    stft_in: Stft,
    stft_out: Stft,
    spec_in: Vec<f32>,
    spec_out: Vec<f32>,
    pub band_max: [[f32; SPECTRUM_BANDS]; 2],
    pub band_used: [[bool; SPECTRUM_BANDS]; 2],
    slope_gain: [f32; SPECTRUM_BANDS],
    bin_to_band: Vec<i32>,
    anchor_max: [f32; 2],
    anchor_used: [bool; 2],
    attack: f32,
    release: f32,
}

impl PadSpectrum {
    pub fn new(sample_rate: f64, fft_size: usize, overlap: usize) -> Self {
        let stft = Stft::new(fft_size, overlap);
        let num_bins = stft.num_bins();
        let mut slope_gain = [1.0f32; SPECTRUM_BANDS];

        // 3 dB/oct，支点 sqrt(20*20000)，与 C++ `RebuildSlopeGain` 同式
        let k = 3.0f32 * 0.166_096_4f32;
        let log_lo = SPEC_LO.log2();
        let log_band = (SPEC_HI.log2() - log_lo) / SPECTRUM_BANDS as f32;
        for (b, gain) in slope_gain.iter_mut().enumerate() {
            let f = SPEC_LO * 2f32.powf(log_band * (b as f32 + 0.5));
            *gain = 2f32.powf(k * (f / SLOPE_REF_HZ).log2());
        }

        // 初始平滑系数：hop 时长 1024/48000 ≈ 21 ms
        let update = 1024.0 / sample_rate.max(1.0);
        let attack = (-update / 0.003f64).exp() as f32;
        let release = (-update / 0.15f64).exp() as f32;

        let mut me = Self {
            sample_rate,
            spec_in: vec![0.0; num_bins],
            spec_out: vec![0.0; num_bins],
            band_max: [[0.0; SPECTRUM_BANDS]; 2],
            band_used: [[false; SPECTRUM_BANDS]; 2],
            slope_gain,
            bin_to_band: vec![-1; num_bins],
            anchor_max: [0.0; 2],
            anchor_used: [false; 2],
            attack,
            release,
            stft_out: Stft::new(fft_size, overlap),
            stft_in: stft,
        };
        me.rebuild_bin_to_band();
        me
    }

    pub fn rebuild_bin_to_band(&mut self) {
        let nb = self.spec_in.len();
        self.bin_to_band = vec![-1; nb];
        if nb == 0 {
            return;
        }
        let bin_hz = self.sample_rate / (nb as f64 * 2.0);
        let log_lo = SPEC_LO.log2();
        let log_band = (SPEC_HI.log2() - log_lo) / SPECTRUM_BANDS as f32;
        for i in 0..nb {
            let f = i as f64 * bin_hz;
            if f > SPEC_HI as f64 {
                continue;
            }
            if f < SPEC_LO as f64 {
                if f >= ANCHOR_LO as f64 {
                    self.bin_to_band[i] = SUB_BAND;
                }
                continue;
            }
            let b = (((f as f32).log2() - log_lo) / log_band) as i32;
            if (0..SPECTRUM_BANDS as i32).contains(&b) {
                self.bin_to_band[i] = b;
            }
        }
    }

    pub fn set_sample_rate(&mut self, sample_rate: f64) {
        if (sample_rate - self.sample_rate).abs() < 1e-9 {
            return;
        }
        self.sample_rate = sample_rate;
        let update = self.stft_in.hop() as f64 / sample_rate.max(1.0);
        self.attack = (-update / 0.003f64).exp() as f32;
        self.release = (-update / 0.15f64).exp() as f32;
        self.rebuild_bin_to_band();
    }

    /// 推入一个 hop 的干/湿样本，并按原版攻击/释放系数更新平滑谱。
    pub fn push_hop(&mut self, dry: &[f32], wet: &[f32]) {
        let nb = self.spec_in.len();
        let mut tmp = vec![0.0f32; nb];

        self.stft_in.push_hop(dry, &mut tmp);
        let (a, r) = (self.attack, self.release);
        for i in 0..nb {
            let raw = tmp[i];
            let prev = self.spec_in[i];
            let coef = if raw > prev { a } else { r };
            self.spec_in[i] = coef * prev + (1.0 - coef) * raw;
        }

        self.stft_out.push_hop(wet, &mut tmp);
        for i in 0..nb {
            let raw = tmp[i];
            let prev = self.spec_out[i];
            let coef = if raw > prev { a } else { r };
            self.spec_out[i] = coef * prev + (1.0 - coef) * raw;
        }

        self.aggregate();
    }

    fn aggregate(&mut self) {
        for ch in 0..2 {
            self.band_max[ch].fill(0.0);
            self.band_used[ch].fill(false);
            self.anchor_max[ch] = 0.0;
            self.anchor_used[ch] = false;
        }
        let nb = self.spec_in.len().min(self.spec_out.len());
        for i in 0..nb {
            let b = self.bin_to_band[i];
            if b == SUB_BAND {
                for ch in 0..2 {
                    let amp = if ch == 1 { self.spec_out[i] } else { self.spec_in[i] };
                    self.anchor_max[ch] = self.anchor_max[ch].max(amp);
                    self.anchor_used[ch] = true;
                }
                continue;
            }
            if b < 0 {
                continue;
            }
            let band = b as usize;
            for ch in 0..2 {
                let amp = if ch == 1 { self.spec_out[i] } else { self.spec_in[i] };
                self.band_max[ch][band] = self.band_max[ch][band].max(amp);
                self.band_used[ch][band] = true;
            }
        }
        self.extrapolate_low_bands();
    }

    /// 低频空桶外推：锚点存在时按 dB-对数频率线性内插，否则首带常值延伸。
    fn extrapolate_low_bands(&mut self) {
        let log_lo = SPEC_LO.log2();
        let log_band = (SPEC_HI.log2() - log_lo) / SPECTRUM_BANDS as f32;
        for ch in 0..2 {
            let first = (0..SPECTRUM_BANDS).find(|&b| self.band_used[ch][b]);
            let Some(first) = first else { continue };
            let f_first = SPEC_LO * 2f32.powf(log_band * (first as f32 + 0.5));
            let a_first = self.band_max[ch][first];
            let a_anchor = self.anchor_max[ch];
            let have_anchor = self.anchor_used[ch] && a_anchor > 1e-9 && a_first > 1e-9;
            for b in 0..first {
                let ext = if have_anchor {
                    let fb = SPEC_LO * 2f32.powf(log_band * (b as f32 + 0.5));
                    let t = (fb / ANCHOR_HZ).log2() / (f_first / ANCHOR_HZ).log2();
                    a_anchor * (a_first / a_anchor).powf(t)
                } else {
                    a_first
                };
                self.band_max[ch][b] = ext;
                self.band_used[ch][b] = true;
            }
        }
    }

    /// 取某个通道的显示点：`(x 归一化 0..1, y 归一化 0..1，底=0)`。
    pub fn curve(&self, ch: usize) -> Vec<(f32, f32)> {
        let mut pts = Vec::with_capacity(SPECTRUM_BANDS);
        for b in 0..SPECTRUM_BANDS {
            if !self.band_used[ch][b] {
                continue;
            }
            let amp = self.band_max[ch][b] * self.slope_gain[b];
            let db = if amp > 1e-6 {
                (20.0 * amp.log10()).clamp(SPECTRUM_BOTTOM_DB, 0.0)
            } else {
                SPECTRUM_BOTTOM_DB
            };
            let y = (db - SPECTRUM_BOTTOM_DB) / (0.0 - SPECTRUM_BOTTOM_DB);
            let x = (b as f32 + 0.5) / SPECTRUM_BANDS as f32;
            pts.push((x, y));
        }
        pts
    }
}
