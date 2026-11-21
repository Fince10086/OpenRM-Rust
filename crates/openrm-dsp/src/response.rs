//! 滤波幅频响应（给界面画曲线用，不参与音频处理）。
//!
//! C++ 版在 `FilterNodePad::DrawResponse()` 里按级联 SVF 的解析式画曲线。
//! 这里用同一套 TPT/双线性映射：数字角频率先预畸变成 Ω = tan(πf/fs) /
//! tan(πf_c/fs)，再按每节的二阶传递函数连乘。
//!
//! 只用于 UI，所以用 f64、允许比音频路径慢。

use crate::bandpass::K_MAX_SECTIONS;

#[derive(Clone, Copy, Default)]
struct Complex {
    re: f64,
    im: f64,
}

impl Complex {
    fn mul(self, o: Self) -> Self {
        Self {
            re: self.re * o.re - self.im * o.im,
            im: self.re * o.im + self.im * o.re,
        }
    }

    fn add(self, o: Self) -> Self {
        Self {
            re: self.re + o.re,
            im: self.im + o.im,
        }
    }

    fn norm(self) -> f64 {
        (self.re * self.re + self.im * self.im).sqrt()
    }

    fn inv(self) -> Self {
        let d = self.re * self.re + self.im * self.im;
        Self {
            re: self.re / d,
            im: -self.im / d,
        }
    }
}

/// 单边（左或右）的带通/带阻幅频响应，线性幅度。
///
/// `freq_hz` 中心频率、`bw_oct` 带宽（octave）、`slope_db` 滚降、`reject` 为带阻。
pub fn magnitude(
    center_hz: f64,
    bw_oct: f64,
    slope_db: f64,
    reject: bool,
    freq_hz: f64,
    sample_rate: f64,
) -> f64 {
    let center = center_hz.clamp(20.0, sample_rate * 0.49);
    let half = 2f64.powf(bw_oct * 0.5);
    let low = (center / half).max(20.0);
    let high = (center * half).min(sample_rate * 0.49);

    let sections = ((slope_db / 12.0).round() as usize).clamp(1, K_MAX_SECTIONS);
    let poles = (2 * sections) as f64;

    let omega = (std::f64::consts::PI * freq_hz / sample_rate).tan();
    let omega_low = (std::f64::consts::PI * low / sample_rate).tan();
    let omega_high = (std::f64::consts::PI * high / sample_rate).tan();

    // 带通：HP 在低截止、LP 在高截止；带阻反过来（LP 在低截止、HP 在高截止），
    // 与 BandPassCore::process_reject 的接线一致。
    let (omega_lp, omega_hp) = if reject {
        (omega_low, omega_high)
    } else {
        (omega_high, omega_low)
    };

    let mut lp_all = Complex {
        re: 1.0,
        im: 0.0,
    };
    let mut hp_all = Complex {
        re: 1.0,
        im: 0.0,
    };

    for k in 0..sections {
        let q = 1.0 / (2.0 * (((2.0 * k as f64 + 1.0) * std::f64::consts::PI) / (2.0 * poles)).cos());
        let kk = 1.0 / q;

        let ratio_lp = omega / omega_lp;
        let denom_lp = Complex {
            re: 1.0 - ratio_lp * ratio_lp,
            im: kk * ratio_lp,
        };
        lp_all = lp_all.mul(denom_lp.inv());

        let ratio_hp = omega / omega_hp;
        let denom_hp = Complex {
            re: 1.0 - ratio_hp * ratio_hp,
            im: kk * ratio_hp,
        };
        let num_hp = Complex {
            re: -ratio_hp * ratio_hp,
            im: 0.0,
        };
        hp_all = hp_all.mul(num_hp.mul(denom_hp.inv()));
    }

    if reject {
        lp_all.add(hp_all).norm()
    } else {
        lp_all.mul(hp_all).norm()
    }
}

/// 以 dB 表示，并叠加增益。
pub fn magnitude_db(
    center_hz: f64,
    bw_oct: f64,
    slope_db: f64,
    reject: bool,
    gain_linear: f64,
    freq_hz: f64,
    sample_rate: f64,
) -> f64 {
    let m = magnitude(center_hz, bw_oct, slope_db, reject, freq_hz, sample_rate) * gain_linear;
    20.0 * (m + 1e-12).log10()
}
