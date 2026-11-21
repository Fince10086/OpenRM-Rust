//! 拓扑保持变换 (TPT) 状态变量滤波器。
//!
//! 与 C++ `plugins/common/dsp/SvfFilter.h` 逐行对应：状态量 f32、系数 f64、
//! 每样本 6 乘 6 加，断点频率锁在 0.49*SR，Q 锁在 0.1~40。
//!
//! 保留原实现的去常态处理（|state| < 1e-20 归零），否则在 x86 上次正规数
//! 会拖慢运算，也会让离线回归测试在不同 CPU 上出现尾差。

const PI: f64 = std::f64::consts::PI;

#[derive(Clone, Copy, Default)]
pub struct SvfFilter {
    sample_rate: f64,
    ic1eq: f32,
    ic2eq: f32,
    a1: f64,
    a2: f64,
    a3: f64,
    k: f64,
}

/// 一次 process() 的三路输出，语义与 C++ 的引用参数一致。
#[derive(Clone, Copy, Debug, Default)]
pub struct SvfOutputs {
    pub low: f32,
    pub band: f32,
    pub high: f32,
}

impl SvfFilter {
    pub fn prepare(&mut self, sample_rate: f64) {
        self.sample_rate = sample_rate;
        self.reset();
    }

    pub fn reset(&mut self) {
        self.ic1eq = 0.0;
        self.ic2eq = 0.0;
    }

    pub fn set_params(&mut self, freq_hz: f64, q: f64) {
        let f = freq_hz.clamp(20.0, self.sample_rate * 0.49);
        let k = 1.0 / q.clamp(0.1, 40.0);
        let g = (PI * f / self.sample_rate).tan();
        self.a1 = 1.0 / (1.0 + g * (g + k));
        self.a2 = g * self.a1;
        self.a3 = g * g * self.a1;
        self.k = k;
    }

    #[inline]
    pub fn process(&mut self, x: f32) -> SvfOutputs {
        let v3 = x - self.ic2eq;
        let v1 = self.a1 as f32 * self.ic1eq + self.a2 as f32 * v3;
        let v2 = self.ic2eq + self.a2 as f32 * self.ic1eq + self.a3 as f32 * v3;
        self.ic1eq = 2.0 * v1 - self.ic1eq;
        self.ic2eq = 2.0 * v2 - self.ic2eq;

        if self.ic1eq.abs() < 1e-20 {
            self.ic1eq = 0.0;
        }
        if self.ic2eq.abs() < 1e-20 {
            self.ic2eq = 0.0;
        }

        SvfOutputs {
            low: v2,
            band: v1,
            high: x - self.k as f32 * v1 - v2,
        }
    }
}
