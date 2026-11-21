//! HalfbandDec2 — 半带 ×2 抽取器（PBT 用）及其依赖的倒频谱最小相位谱分解。
//!
//! 逐行移植自 `plugins/Analyzer/src/dsp/HalfbandDec2.h`（303 行）：
//! 基-2 迭代 FFT、自相关 + 倒频谱最小相位因子、BH4 窗 sinc 原型、双系数形态
//! （线性相位 / 最小相位）、8 路分块 FIR 抽取。
//!
//! 本模块的 `radix2_fft` 同时供 `vqt.rs` 的 `AntiAliasDec` 复用（两者 FFT 数学等价）。

use std::sync::OnceLock;

pub(crate) const K_PI: f64 = 3.14159265358979323846;

/// 半带抽头数（2M+1，M=50）。
pub const K_N: usize = 101;
/// 线性相位形态的群延迟（样本）。
pub const K_Q: usize = (K_N - 1) / 2;

/// 基-2 迭代 FFT（n 为 2 的幂）。`inverse=true` 时含 1/n 归一化。
/// 对应原版 `HalfbandDec2::detail::Fft`。
pub(crate) fn radix2_fft(re: &mut [f64], im: &mut [f64], inverse: bool) {
    let n = re.len();
    // 位反转重排
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
        let mut i = 0;
        while i < n {
            let mut cr = 1.0_f64;
            let mut ci = 0.0_f64;
            for k in 0..len / 2 {
                let ur = re[i + k];
                let ui = im[i + k];
                let xr = re[i + k + len / 2];
                let xi = im[i + k + len / 2];
                let vr = xr * cr - xi * ci;
                let vi = xr * ci + xi * cr;
                re[i + k] = ur + vr;
                im[i + k] = ui + vi;
                re[i + k + len / 2] = ur - vr;
                im[i + k + len / 2] = ui - vi;
                let ncr = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = ncr;
            }
            i += len;
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

const MINPHASE_DECAY_TARGET: f64 = 5e-4;

/// 指定 FFT 长度与抬底量的倒频谱谱分解（`MinPhaseFactor` 的单次尝试）。
/// 对应原版 `MinPhaseFactorWithLen`。返回长度 N（=Mr+1）的因果因子 g。
fn min_phase_factor_with_len(proto: &[f64], lfft: usize, lift: f64) -> Vec<f64> {
    let n = proto.len();
    let mr = n - 1; // 自相关最高滞后 2M（=N−1）
    let mut re = vec![0.0_f64; lfft];
    let mut im = vec![0.0_f64; lfft];

    // 自相关 r[n] = Σ_k proto[k]·proto[k−n]，零相位装载（bin n 与 bin Lfft−n）
    for nlag in 0..=mr {
        let mut acc = 0.0_f64;
        for k in nlag..n {
            acc += proto[k] * proto[k - nlag];
        }
        re[nlag] = acc;
        if nlag > 0 {
            re[lfft - nlag] = acc;
        }
    }
    re[0] *= 1.0 + lift; // 抬底座：圆上零点 → 圆内
    radix2_fft(&mut re, &mut im, false); // R = A²（实，≥0）

    // log R（floor 兜底）→ IFFT 得实倒频谱
    for k in 0..lfft {
        re[k] = re[k].max(1e-24).ln();
        im[k] = 0.0;
    }
    radix2_fft(&mut re, &mut im, true);

    // 复倒频谱因果化。注意：这里的 c = IDFT(log R) 已是标准实倒频谱的 2 倍，
    // 不可再按 log|G| 的经典公式加倍（加倍等效于对因子再平方，支撑翻倍）。
    re[0] *= 0.5;
    for n in lfft / 2..lfft {
        re[n] = 0.0;
        im[n] = 0.0;
    }
    radix2_fft(&mut re, &mut im, false);

    // exp → 最小相位频谱 → IFFT 得因果因子 g（支撑 0..2M，能量前置）
    for k in 0..lfft {
        let er = re[k].exp();
        re[k] = er * im[k].cos();
        im[k] = er * im[k].sin();
    }
    radix2_fft(&mut re, &mut im, true);

    let mut g = vec![0.0_f64; mr + 1];
    for n in 0..=mr {
        g[n] = re[n];
    }
    g
}

/// 实对称 FIR 原型 → 同幅频最小相位谱因子（自相关 + 倒频谱分解），长度不变。
/// 对应原版 `MinPhaseFactor`。
pub fn min_phase_factor(proto: &[f64], stop_ripple: f64) -> Vec<f64> {
    let n = proto.len();
    let mut lfft = 1024;
    while lfft < 8 * n {
        lfft <<= 1;
    }
    if lfft < 16384 {
        lfft = 16384;
    }
    // 自适应抬底（见原版注）：Δt = sqrt(ε·Σp²)/δNπ → ε = (Δt·δ·N/π)²/Σp²
    let mut r0 = 0.0_f64;
    for &v in proto {
        r0 += v * v;
    }
    let s_est = stop_ripple * n as f64 / K_PI;
    let mut lift = (MINPHASE_DECAY_TARGET * s_est).powi(2) / r0.max(1e-30);
    lift = lift.clamp(1e-15, 1e-3);

    let mut g: Vec<f64>;
    loop {
        g = min_phase_factor_with_len(proto, lfft, lift);
        let mut worst = 0.0_f64;
        for i in 1..=64 {
            let w = K_PI * i as f64 / 64.0;
            // 旋转递推累加 |A0| 与 |G|：每频率只算一次 sin/cos。
            // |A0| 必须取复数模 |Σ proto[n]e^{-jωn}| —— 原型居中对称，其零相位频响
            // 是该和乘相位因子后的实值；只取实部会在零点附近假性跌百 dB。
            let cw0 = w.cos();
            let sw0 = w.sin();
            let (mut ar, mut ai) = (0.0_f64, 0.0_f64);
            let (mut gr, mut gi) = (0.0_f64, 0.0_f64);
            let (mut cw, mut sw) = (1.0_f64, 0.0_f64);
            for k in 0..n {
                ar += proto[k] * cw;
                ai -= proto[k] * sw;
                gr += g[k] * cw;
                gi -= g[k] * sw;
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
            break;
        }
        lfft <<= 1; // 正常一步收敛，此阶梯仅为正确性兜底
    }
    g
}

/// 对称半带原型（sinc × BH4 窗；偶序抽头除中心外严格为零）。
/// 对应原版 `BuildProtoTaps`。
fn build_proto_taps(taps: &mut [f64; K_N]) {
    for (i, v) in taps.iter_mut().enumerate() {
        let n = (i as isize) - K_Q as isize;
        let mut vv: f64;
        if n == 0 {
            vv = 0.5;
        } else if n & 1 == 0 {
            vv = 0.0; // 偶序（除中心）= 0：半带结构
        } else {
            vv = 0.5 * (K_PI * n as f64 / 2.0).sin() / (K_PI * n as f64 / 2.0);
        }
        let theta = 2.0 * K_PI * i as f64 / (K_N - 1) as f64; // 4 项 BH：旁瓣 -92 dB
        vv *= 0.35875 - 0.48829 * theta.cos()
            + 0.14128 * (2.0 * theta).cos()
            - 0.01168 * (3.0 * theta).cos();
        *v = vv;
    }
}

/// MINPH 形态的系数（分解只算一次，全体抽取器共享；Σg=Σp 由分解保证，归一化兜底）。
fn min_phase_taps() -> &'static [f32; K_N] {
    static TAPS: OnceLock<[f32; K_N]> = OnceLock::new();
    TAPS.get_or_init(|| {
        let mut proto_d = [0.0_f64; K_N];
        build_proto_taps(&mut proto_d);
        let proto: Vec<f64> = proto_d.to_vec();
        let proto = min_phase_factor(&proto, 10.0_f64.powf(-92.0 / 20.0));
        let mut sum = 0.0_f64;
        for &v in &proto {
            sum += v;
        }
        let mut out = [0.0_f32; K_N];
        for i in 0..K_N {
            out[i] = (proto[i] / sum) as f32;
        }
        out
    })
}

/// 每级有效群延迟（本级样本，最小相位形态）：分解后冲激的能量重心。
/// 线性相位形态每级 = K_Q。对应原版 `GdPerStage`。
pub fn gd_per_stage() -> f64 {
    static CENT: OnceLock<f64> = OnceLock::new();
    *CENT.get_or_init(|| {
        let t = min_phase_taps();
        let mut e = 0.0_f64;
        let mut c = 0.0_f64;
        for (n, &v) in t.iter().enumerate() {
            e += (v as f64) * (v as f64);
            c += n as f64 * (v as f64) * (v as f64);
        }
        c / e
    })
}

/// 半带 ×2 抽取器（跨帧保持滤波状态）。要求 `nin` 为偶数。
pub struct HalfbandDec2 {
    tap: [f32; K_N],
    tap_rev: [f32; K_N],
    state: [f32; K_N - 1],
    work: Vec<f32>,
    min_phase: bool,
}

impl HalfbandDec2 {
    pub fn new() -> Self {
        let mut me = Self {
            tap: [0.0; K_N],
            tap_rev: [0.0; K_N],
            state: [0.0; K_N - 1],
            work: vec![0.0; K_N - 1 + 2048],
            min_phase: false,
        };
        me.rebuild_taps();
        me
    }

    /// 换系数形态（PBT 固定切到最小相位；滤波状态由其尾部 ResetRuntimeState 清零）。
    pub fn set_min_phase(&mut self, mp: bool) {
        if mp == self.min_phase {
            return;
        }
        self.min_phase = mp;
        self.rebuild_taps();
    }

    fn rebuild_taps(&mut self) {
        if self.min_phase {
            self.tap = *min_phase_taps();
        } else {
            let mut proto_d = [0.0_f64; K_N];
            build_proto_taps(&mut proto_d);
            let mut sum = 0.0_f64;
            for &v in &proto_d {
                sum += v;
            }
            for i in 0..K_N {
                self.tap[i] = (proto_d[i] / sum) as f32;
            }
        }
        for i in 0..K_N {
            self.tap_rev[i] = self.tap[K_N - 1 - i];
        }
    }

    pub fn reset(&mut self) {
        self.state.fill(0.0);
    }

    /// 抽取滤波。返回输出样本数（=nin/2）。`out` 长度至少 `nin/2`。
    /// 对应原版 `Process`：输出驻留 8 路分块 FIR，每输出乘积序列与升序标量版逐位一致。
    pub fn process(&mut self, input: &[f32], output: &mut [f32]) -> usize {
        let nt = K_N;
        let nin = input.len();
        let sz = (nt - 1) + nin;
        for i in 0..nt - 1 {
            self.work[i] = self.state[i];
        }
        for i in 0..nin {
            self.work[nt - 1 + i] = input[i];
        }
        let nout = nin / 2;
        let t = self.tap_rev;
        let mut j = 0;
        while j + 8 <= nout {
            let x = &self.work[2 * j..];
            let (mut a0, mut a1, mut a2, mut a3) = (0.0_f32, 0.0_f32, 0.0_f32, 0.0_f32);
            let (mut a4, mut a5, mut a6, mut a7) = (0.0_f32, 0.0_f32, 0.0_f32, 0.0_f32);
            for i in 0..nt {
                let tv = t[i];
                a0 += tv * x[i];
                a1 += tv * x[i + 2];
                a2 += tv * x[i + 4];
                a3 += tv * x[i + 6];
                a4 += tv * x[i + 8];
                a5 += tv * x[i + 10];
                a6 += tv * x[i + 12];
                a7 += tv * x[i + 14];
            }
            output[j] = a0;
            output[j + 1] = a1;
            output[j + 2] = a2;
            output[j + 3] = a3;
            output[j + 4] = a4;
            output[j + 5] = a5;
            output[j + 6] = a6;
            output[j + 7] = a7;
            j += 8;
        }
        while j < nout {
            let x = &self.work[2 * j..];
            let mut acc = 0.0_f32;
            for i in 0..nt {
                acc += t[i] * x[i];
            }
            output[j] = acc;
            j += 1;
        }
        for i in 0..nt - 1 {
            self.state[i] = self.work[sz - (nt - 1) + i];
        }
        nout
    }

    /// 当前系数（测试比对用）。
    pub fn taps(&self) -> &[f32; K_N] {
        &self.tap
    }
}

impl Default for HalfbandDec2 {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fft_matches_direct_dft() {
        for n in [256, 16384] {
            let mut re: Vec<f64> = (0..n).map(|i| (i as f64 * 0.137).sin() * 0.5).collect();
            let mut im = vec![0.0; n];
            radix2_fft(&mut re, &mut im, false);
            for k in [0, 1, 3, n / 4, n / 2, n - 1] {
                let mut dr = 0.0;
                let mut di = 0.0;
                for i in 0..n {
                    let ph = -2.0 * K_PI * k as f64 * i as f64 / n as f64;
                    dr += ((i as f64 * 0.137).sin() * 0.5) * ph.cos();
                    di += ((i as f64 * 0.137).sin() * 0.5) * ph.sin();
                }
                assert!((re[k] - dr).abs() < 1e-6, "n={n} bin {k} re {} vs {dr}", re[k]);
                assert!((im[k] - di).abs() < 1e-6, "n={n} bin {k} im {} vs {di}", im[k]);
            }
            // 紧支撑 101 抽头对称序列（自相关同款形态），检查高频 bin
            let mut re2: Vec<f64> = vec![0.0; n];
            for i in 0..101.min(n) {
                re2[i] = ((i as f64 - 50.0) * 0.3).sin() * (1.0 - (i as f64 / 50.0).abs().min(1.0));
            }
            let mut im2 = vec![0.0; n];
            radix2_fft(&mut re2, &mut im2, false);
            for k in (0..n).step_by(997).take(20) {
                let mut dr = 0.0;
                let mut di = 0.0;
                for i in 0..101.min(n) {
                    let v = ((i as f64 - 50.0) * 0.3).sin() * (1.0 - (i as f64 / 50.0).abs().min(1.0));
                    let ph = -2.0 * K_PI * k as f64 * i as f64 / n as f64;
                    dr += v * ph.cos();
                    di += v * ph.sin();
                }
                assert!((re2[k] - dr).abs() < 1e-6, "compact n={n} bin {k} re {} vs {dr}", re2[k]);
                assert!((im2[k] - di).abs() < 1e-6, "compact n={n} bin {k} im {} vs {di}", im2[k]);
            }
            radix2_fft(&mut re, &mut im, true);
            for i in 0..n {
                assert!((re[i] - (i as f64 * 0.137).sin() * 0.5).abs() < 1e-8);
            }
        }
    }

    #[test]
    fn proto_odd_taps_zero() {
        let mut p = [0.0_f64; K_N];
        build_proto_taps(&mut p);
        for i in 0..K_N {
            let n = i as isize - K_Q as isize;
            if n != 0 && n & 1 == 0 {
                assert_eq!(p[i], 0.0);
            }
        }
    }

    #[test]
    fn minphase_taps_normalized() {
        let mut hb = HalfbandDec2::new();
        hb.set_min_phase(true);
        let sum: f64 = hb.taps().iter().map(|&v| v as f64).sum();
        assert!((sum - 1.0).abs() < 1e-5, "minphase sum={sum}");
    }
}
