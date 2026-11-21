//! ORM BandPass 的 DSP 核心（Rust 移植）。
//!
//! 与 C++ `plugins/BandPass/src/dsp/BandPassCore.h` 一一对应，保留全部算法细节：
//! 链路顺序（先 HP 级联再 LP 级联）、Butterworth Q 分配、15 ms 参数平滑、
//! 7 路随机游走的取值顺序与种子、混音等功率交叉淡化、reject（带阻）分支。
//!
//! 差异只有一处：C++ 用 `std::atomic` + 外部 mailbox 传参，这里由调用方在
//! 每块开始时 `set_params()`，参数传递方式不属于 DSP 核心。

use crate::random_walk::RandomWalk;
use crate::svf::SvfFilter;

const PI: f64 = std::f64::consts::PI;

pub const K_MAX_SECTIONS: usize = 8;
pub const K_NUM_RANDOM_STREAMS: usize = 7;

/// 随机调制流的下标。顺序必须与 C++ 的 enum 一致（set_params 里逐条取值）。
pub const K_RANDOM_FREQ_L: usize = 0;
pub const K_RANDOM_BW_L: usize = 1;
pub const K_RANDOM_GAIN_L: usize = 2;
pub const K_RANDOM_FREQ_R: usize = 3;
pub const K_RANDOM_BW_R: usize = 4;
pub const K_RANDOM_GAIN_R: usize = 5;
pub const K_RANDOM_MIX: usize = 6;

// 随机调制幅度上限（满量程时的最大偏移）
const K_RANDOM_FREQ_MOD_OCT: f64 = 4.906_890_595_608_52; // log2(30)
const K_RANDOM_BW_MOD_OCT: f64 = 4.643_856_189_774_72; // log2(25)
const K_RANDOM_GAIN_MOD_DB: f32 = 48.0;
const K_RANDOM_MIX_MOD: f32 = 0.5;
const K_DB_TO_LIN: f32 = 0.115_129_254_649_702; // ln(10)/20

/// 传给 DSP 核心的参数快照（已换算成核心单位：Hz / octave / 线性增益）。
#[derive(Clone, Copy, Debug)]
pub struct Params {
    pub freq_l: f64,
    pub bw_l: f64,
    pub gain_l: f32,

    pub freq_r: f64,
    pub bw_r: f64,
    pub gain_r: f32,

    pub linked: bool,
    pub reject_l: bool,
    pub reject_r: bool,
    pub mix: f32,
    pub random_amount: [f32; 4],
    pub random_period_sec: [f64; 4],
    pub random_enable_freq_l: bool,
    pub random_enable_bw_l: bool,
    pub random_enable_gain_l: bool,
    pub random_enable_freq_r: bool,
    pub random_enable_bw_r: bool,
    pub random_enable_gain_r: bool,
    pub random_enable_mix: bool,
    pub random_color_freq_l: u8,
    pub random_color_bw_l: u8,
    pub random_color_gain_l: u8,
    pub random_color_freq_r: u8,
    pub random_color_bw_r: u8,
    pub random_color_gain_r: u8,
    pub random_color_mix: u8,
    pub slope_db_l: f64,
    pub slope_db_r: f64,
}

impl Default for Params {
    fn default() -> Self {
        Self {
            freq_l: 1000.0,
            bw_l: 1.0,
            gain_l: 1.0,
            freq_r: 1000.0,
            bw_r: 1.0,
            gain_r: 1.0,
            linked: false,
            reject_l: false,
            reject_r: false,
            mix: 1.0,
            random_amount: [0.1; 4],
            random_period_sec: [1.0; 4],
            random_enable_freq_l: false,
            random_enable_bw_l: false,
            random_enable_gain_l: false,
            random_enable_freq_r: false,
            random_enable_bw_r: false,
            random_enable_gain_r: false,
            random_enable_mix: false,
            random_color_freq_l: 0,
            random_color_bw_l: 0,
            random_color_gain_l: 0,
            random_color_freq_r: 0,
            random_color_bw_r: 0,
            random_color_gain_r: 0,
            random_color_mix: 0,
            slope_db_l: 96.0,
            slope_db_r: 96.0,
        }
    }
}

/// 每样本的随机调制量，UI 用来画"幽灵手柄"。
#[derive(Clone, Copy, Debug, Default)]
pub struct RandomDeltas {
    pub freq_oct: [f32; 2],
    pub bw_oct: [f32; 2],
    pub gain_db: [f32; 2],
    pub mix: f32,
}

#[derive(Clone)]
struct ChannelChain {
    sections: usize,
    q: [f64; K_MAX_SECTIONS],
    hp: [SvfFilter; K_MAX_SECTIONS],
    lp: [SvfFilter; K_MAX_SECTIONS],
}

impl Default for ChannelChain {
    fn default() -> Self {
        Self {
            sections: 0,
            q: [0.0; K_MAX_SECTIONS],
            hp: [SvfFilter::default(); K_MAX_SECTIONS],
            lp: [SvfFilter::default(); K_MAX_SECTIONS],
        }
    }
}

impl ChannelChain {
    fn prepare(&mut self, sample_rate: f64) {
        for f in &mut self.hp {
            f.prepare(sample_rate);
        }
        for f in &mut self.lp {
            f.prepare(sample_rate);
        }
    }

    fn reset(&mut self) {
        for f in &mut self.hp {
            f.reset();
        }
        for f in &mut self.lp {
            f.reset();
        }
    }

    fn update(&mut self, low_hz: f64, high_hz: f64, slope_db: f64, reject: bool) {
        let sections = ((slope_db / 12.0).round() as i64).clamp(1, K_MAX_SECTIONS as i64) as usize;
        if sections != self.sections {
            self.sections = sections;
            let poles = (2 * sections) as f64;
            for k in 0..sections {
                self.q[k] = 1.0 / (2.0 * ((2.0 * k as f64 + 1.0) * PI / (2.0 * poles)).cos());
                self.hp[k].reset();
                self.lp[k].reset();
            }
        }
        let hp_freq = if reject { high_hz } else { low_hz };
        let lp_freq = if reject { low_hz } else { high_hz };
        for k in 0..sections {
            self.hp[k].set_params(hp_freq, self.q[k]);
            self.lp[k].set_params(lp_freq, self.q[k]);
        }
    }

    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let mut s = x;
        for k in 0..self.sections {
            s = self.hp[k].process(s).high;
        }
        for k in 0..self.sections {
            s = self.lp[k].process(s).low;
        }
        s
    }

    /// 带阻：LP 级联与 HP 级联并联相加（不是串联）。
    #[inline]
    fn process_reject(&mut self, x: f32) -> f32 {
        let mut lp_acc = x;
        for k in 0..self.sections {
            lp_acc = self.lp[k].process(lp_acc).low;
        }
        let mut hp_acc = x;
        for k in 0..self.sections {
            hp_acc = self.hp[k].process(hp_acc).high;
        }
        lp_acc + hp_acc
    }
}

#[derive(Clone, Default)]
struct Channel {
    target_freq: f64,
    target_bw: f64,
    sm_freq: f64,
    sm_bw: f64,
    half_bw: f64,
    slope_db: f64,
    reject: bool,
    chain: ChannelChain,
}

impl Channel {
    fn prepare(&mut self, sample_rate: f64) {
        self.chain.prepare(sample_rate);
        self.sm_freq = self.target_freq;
        self.sm_bw = self.target_bw;
        self.half_bw = 2.0f64.powf(self.sm_bw * 0.5);
    }

    fn reset(&mut self) {
        self.chain.reset();
    }

    fn set_target(&mut self, freq: f64, bw: f64, slope: f64) {
        self.target_freq = freq;
        self.target_bw = bw;
        self.slope_db = slope;
    }

    fn smooth(&mut self, coef: f64) {
        self.sm_freq += (self.target_freq - self.sm_freq) * coef;
        self.sm_bw += (self.target_bw - self.sm_bw) * coef;
        self.half_bw = 2.0f64.powf(self.sm_bw * 0.5);
        self.chain.update(
            self.sm_freq / self.half_bw,
            self.sm_freq * self.half_bw,
            self.slope_db,
            self.reject,
        );
    }

    #[inline]
    fn process(&mut self, x: f32, mod_freq_oct: f64, mod_bw_oct: f64, sample_rate: f64) -> f32 {
        if mod_freq_oct != 0.0 || mod_bw_oct != 0.0 {
            let f = (self.sm_freq * mod_freq_oct.exp2()).clamp(20.0, sample_rate * 0.49);
            let hb = self.half_bw * (mod_bw_oct * 0.5).exp2();
            self.chain
                .update(f / hb, f * hb, self.slope_db, self.reject);
        }
        if self.reject {
            self.chain.process_reject(x)
        } else {
            self.chain.process(x)
        }
    }
}

#[derive(Clone, Default)]
struct RandomStream {
    walk: RandomWalk,
    active: bool,
    amount: f32,
    period_sec: f64,
}

pub struct BandPassCore {
    sample_rate: f64,
    params: Params,
    ch: [Channel; 2],
    random_stream: [RandomStream; K_NUM_RANDOM_STREAMS],
    random_deltas: RandomDeltas,
}

impl Default for BandPassCore {
    fn default() -> Self {
        Self::new()
    }
}

impl BandPassCore {
    pub fn new() -> Self {
        Self {
            sample_rate: 44100.0,
            params: Params::default(),
            ch: [Channel::default(), Channel::default()],
            random_stream: std::array::from_fn(|_| RandomStream::default()),
            random_deltas: RandomDeltas::default(),
        }
    }

    pub fn random_deltas(&self) -> RandomDeltas {
        self.random_deltas
    }

    pub fn prepare(&mut self, sample_rate: f64) {
        self.sample_rate = sample_rate;
        for ch in &mut self.ch {
            ch.prepare(sample_rate);
        }
        for (i, rs) in self.random_stream.iter_mut().enumerate() {
            rs.walk.seed(
                0x9E37_79B9u32
                    .wrapping_mul(i as u32 + 1)
                    .wrapping_add(0x5BD1_E995),
            );
            rs.walk.set_period(rs.period_sec, sample_rate);
        }
        self.reset();
    }

    pub fn reset(&mut self) {
        for ch in &mut self.ch {
            ch.reset();
        }
    }

    pub fn set_params(&mut self, p: &Params) {
        self.params = *p;
        self.params.gain_l = p.gain_l.clamp(0.0, 2.0);
        self.params.gain_r = p.gain_r.clamp(0.0, 2.0);
        self.params.mix = p.mix.clamp(0.0, 1.0);
        for c in 0..4 {
            self.params.random_amount[c] = p.random_amount[c].clamp(0.0, 1.0);
            self.params.random_period_sec[c] = p.random_period_sec[c].max(0.001);
        }
        if p.linked {
            self.params.freq_r = self.params.freq_l;
            self.params.bw_r = self.params.bw_l;
            self.params.slope_db_r = self.params.slope_db_l;
            self.params.gain_r = self.params.gain_l;
            self.params.reject_r = self.params.reject_l;
            self.params.random_enable_freq_r = self.params.random_enable_freq_l;
            self.params.random_color_freq_r = self.params.random_color_freq_l;
            self.params.random_enable_bw_r = self.params.random_enable_bw_l;
            self.params.random_color_bw_r = self.params.random_color_bw_l;
            self.params.random_enable_gain_r = self.params.random_enable_gain_l;
            self.params.random_color_gain_r = self.params.random_color_gain_l;
        }
        // 必须从镜像后的 self.params 读取（而非原始 p）—— linked 时 R 流的
        // enable/color 已被强制同步为 L 值，读 p 会形成两条不一致的数据来源。
        let maps: [(bool, u8); K_NUM_RANDOM_STREAMS] = [
            (
                self.params.random_enable_freq_l,
                self.params.random_color_freq_l,
            ),
            (
                self.params.random_enable_bw_l,
                self.params.random_color_bw_l,
            ),
            (
                self.params.random_enable_gain_l,
                self.params.random_color_gain_l,
            ),
            (
                self.params.random_enable_freq_r,
                self.params.random_color_freq_r,
            ),
            (
                self.params.random_enable_bw_r,
                self.params.random_color_bw_r,
            ),
            (
                self.params.random_enable_gain_r,
                self.params.random_color_gain_r,
            ),
            (self.params.random_enable_mix, self.params.random_color_mix),
        ];
        for i in 0..K_NUM_RANDOM_STREAMS {
            let col = (maps[i].1 as usize).min(3);
            let amt = self.params.random_amount[col];
            let rs = &mut self.random_stream[i];
            rs.active = maps[i].0 && amt > 0.0;
            rs.amount = amt;
            rs.period_sec = self.params.random_period_sec[col];
            rs.walk.set_period(rs.period_sec, self.sample_rate);
        }
        self.ch[0].set_target(self.params.freq_l, self.params.bw_l, self.params.slope_db_l);
        self.ch[1].set_target(self.params.freq_r, self.params.bw_r, self.params.slope_db_r);
        self.ch[0].reject = self.params.reject_l;
        self.ch[1].reject = self.params.reject_r;
    }

    /// 每块开始时调用一次，推进 15 ms 一阶平滑。
    pub fn update_smoothing(&mut self, block_size: usize) {
        let coef = 1.0 - (-(block_size as f64) / (0.015 * self.sample_rate)).exp();
        for ch in &mut self.ch {
            ch.smooth(coef);
        }
    }

    /// 立体声处理；`wet_l` / `wet_r` 用于频谱叠加显示（干/湿两条曲线）。
    pub fn process_stereo(
        &mut self,
        in_l: &[f32],
        in_r: &[f32],
        out_l: &mut [f32],
        out_r: &mut [f32],
        wet_l: Option<&mut [f32]>,
        wet_r: Option<&mut [f32]>,
        n: usize,
    ) {
        self.run([in_l, in_r], [out_l, out_r], [wet_l, wet_r], 2, n);
    }

    pub fn process_mono(
        &mut self,
        input: &[f32],
        output: &mut [f32],
        wet: Option<&mut [f32]>,
        n: usize,
    ) {
        self.run([input, &[]], [output, &mut []], [wet, None], 1, n);
    }

    fn run(
        &mut self,
        ins: [&[f32]; 2],
        outs: [&mut [f32]; 2],
        wets: [Option<&mut [f32]>; 2],
        num_ch: usize,
        n: usize,
    ) {
        let Self {
            sample_rate,
            params,
            ch,
            random_stream,
            random_deltas,
        } = self;
        let sample_rate = *sample_rate;
        let outs = outs;
        let mut wets = wets;

        let gains = [params.gain_l, params.gain_r];
        let mix_mod = random_stream[K_RANDOM_MIX].active;
        let base_mix = params.mix;

        let (mut dry_gain, mut wet_gain) = if !mix_mod {
            let mix_angle = base_mix * 0.5 * PI as f32;
            (mix_angle.cos(), mix_angle.sin())
        } else {
            (0.0, 0.0)
        };

        for i in 0..n {
            let mut freq_oct = [0.0f32; 2];
            let mut bw_oct = [0.0f32; 2];
            let mut gain_db = [0.0f32; 2];
            let mut mix_delta = 0.0f32;
            let mut gain_lin = [1.0f32, 1.0f32];

            if random_stream[K_RANDOM_FREQ_L].active {
                freq_oct[0] = random_stream[K_RANDOM_FREQ_L].walk.tick()
                    * random_stream[K_RANDOM_FREQ_L].amount
                    * K_RANDOM_FREQ_MOD_OCT as f32;
            }
            if random_stream[K_RANDOM_BW_L].active {
                bw_oct[0] = random_stream[K_RANDOM_BW_L].walk.tick()
                    * random_stream[K_RANDOM_BW_L].amount
                    * K_RANDOM_BW_MOD_OCT as f32;
            }
            if random_stream[K_RANDOM_GAIN_L].active {
                gain_db[0] = random_stream[K_RANDOM_GAIN_L].walk.tick()
                    * random_stream[K_RANDOM_GAIN_L].amount
                    * K_RANDOM_GAIN_MOD_DB;
                gain_lin[0] = (gain_db[0] * K_DB_TO_LIN).exp();
            }
            if num_ch > 1 {
                if params.linked {
                    freq_oct[1] = freq_oct[0];
                    bw_oct[1] = bw_oct[0];
                    gain_db[1] = gain_db[0];
                    gain_lin[1] = gain_lin[0];
                } else {
                    if random_stream[K_RANDOM_FREQ_R].active {
                        freq_oct[1] = random_stream[K_RANDOM_FREQ_R].walk.tick()
                            * random_stream[K_RANDOM_FREQ_R].amount
                            * K_RANDOM_FREQ_MOD_OCT as f32;
                    }
                    if random_stream[K_RANDOM_BW_R].active {
                        bw_oct[1] = random_stream[K_RANDOM_BW_R].walk.tick()
                            * random_stream[K_RANDOM_BW_R].amount
                            * K_RANDOM_BW_MOD_OCT as f32;
                    }
                    if random_stream[K_RANDOM_GAIN_R].active {
                        gain_db[1] = random_stream[K_RANDOM_GAIN_R].walk.tick()
                            * random_stream[K_RANDOM_GAIN_R].amount
                            * K_RANDOM_GAIN_MOD_DB;
                        gain_lin[1] = (gain_db[1] * K_DB_TO_LIN).exp();
                    }
                }
            }
            if mix_mod {
                mix_delta = random_stream[K_RANDOM_MIX].walk.tick()
                    * random_stream[K_RANDOM_MIX].amount
                    * K_RANDOM_MIX_MOD;
                let m = (base_mix + mix_delta).clamp(0.0, 1.0);
                let mix_angle = m * 0.5 * PI as f32;
                dry_gain = mix_angle.cos();
                wet_gain = mix_angle.sin();
            }

            for c in 0..num_ch {
                let x = ins[c][i];
                let wet_s = ch[c].process(x, freq_oct[c] as f64, bw_oct[c] as f64, sample_rate)
                    * gains[c]
                    * gain_lin[c];
                if let Some(w) = wets[c].as_deref_mut() {
                    w[i] = wet_s;
                }
                outs[c][i] = x * dry_gain + wet_s * wet_gain;
            }

            random_deltas.freq_oct[0] = freq_oct[0];
            random_deltas.freq_oct[1] = freq_oct[1];
            random_deltas.bw_oct[0] = bw_oct[0];
            random_deltas.bw_oct[1] = bw_oct[1];
            random_deltas.gain_db[0] = gain_db[0];
            random_deltas.gain_db[1] = gain_db[1];
            random_deltas.mix = mix_delta;
        }
    }
}
