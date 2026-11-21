//! 与 `plugins/common/dsp/FastMath.h` 逐行对应的快速数学。

/// 极速单精度 log2 近似（IEEE754 指数位 + 二次极小极大拟合）。
#[inline]
pub fn fast_log2(x: f32) -> f32 {
    let bits = x.to_bits();
    let e = (bits >> 23) as i32 - 127;
    let mantissa_bits = (bits & 0x007F_FFFF) | 0x3F80_0000;
    let m = f32::from_bits(mantissa_bits) - 1.0;
    let log2_1p = m * (1.442_689_6 + m * (-0.718_320_0 + m * (0.435_770_0 - 0.160_139_6 * m)));
    e as f32 + log2_1p
}

#[inline]
pub fn fast_amp_to_db(amp: f32, min_db: f32) -> f32 {
    if amp <= 1e-6 {
        min_db
    } else {
        6.020_599_913_28 * fast_log2(amp)
    }
}

#[inline]
pub fn fast_pwr_to_db(pwr: f32, min_db: f32) -> f32 {
    if pwr <= 1e-12 {
        min_db
    } else {
        3.010_299_956_64 * fast_log2(pwr)
    }
}

#[inline]
pub fn soft_clip(x: f32) -> f32 {
    let t = x.clamp(-1.0, 1.0);
    t - t * t * t * (1.0 / 3.0)
}

#[inline]
pub fn soft_limit(x: f32, knee: f32, span: f32) -> f32 {
    let a = x.abs();
    if a <= knee {
        return x;
    }
    if a >= knee + span {
        return (knee + 0.5 * span).copysign(x);
    }
    let e = a - knee;
    (knee + e - e * e / (2.0 * span)).copysign(x)
}
