//! 平滑随机游走：每 period 秒抽一个新的目标值 in [-1, 1]，段内用升余弦插值。
//!
//! 对应 C++ `plugins/common/dsp/RandomWalk.h`。xorshift32 + 分段升余弦，
//! 位运算必须与 C++ 完全一致，否则同一 seed 下两侧调制轨迹不同，A/B 对比会失真。

const PI: f64 = std::f64::consts::PI;

#[derive(Clone, Copy)]
pub struct RandomWalk {
    from: f64,
    to: f64,
    phase: f64,
    inc: f64,
    state: u32,
}

impl Default for RandomWalk {
    fn default() -> Self {
        Self {
            from: 0.0,
            to: 0.0,
            phase: 0.0,
            inc: 0.0,
            state: 1,
        }
    }
}

impl RandomWalk {
    pub fn seed(&mut self, s: u32) {
        self.state = if s != 0 { s } else { 1 };
        self.from = 0.0;
        self.to = 0.0;
        self.phase = 0.0;
        self.inc = 0.0;
    }

    pub fn set_period(&mut self, seconds: f64, sample_rate: f64) {
        self.inc = 1.0 / (seconds * sample_rate).max(1.0);
    }

    /// 让调制立刻起作用：先抽一个目标，第一个周期从 0 升到该目标。
    pub fn start_now(&mut self) {
        self.to = self.next_target();
    }

    #[inline]
    pub fn tick(&mut self) -> f32 {
        self.phase += self.inc;
        if self.phase >= 1.0 {
            self.phase -= self.phase.floor();
            self.from = self.to;
            self.to = self.next_target();
        }
        let t = 0.5 - 0.5 * (PI * self.phase).cos();
        (self.from + (self.to - self.from) * t) as f32
    }

    fn next_target(&mut self) -> f64 {
        let mut x = self.state;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.state = x;
        (x >> 8) as f64 / 8_388_607.5 - 1.0
    }
}
