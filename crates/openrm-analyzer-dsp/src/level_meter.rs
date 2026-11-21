//! LevelMeter — 专业电平测量（L/R、真峰值、RMS、VU、hold/over）。
//! 逐行移植自 `plugins/Analyzer/src/dsp/LevelMeter.h`。

use crate::fastmath::{fast_amp_to_db, fast_pwr_to_db};
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug)]
pub struct LevelSnapshot {
    pub peak_db_l: f32,
    pub peak_db_r: f32,
    pub true_db_l: f32,
    pub true_db_r: f32,
    pub rms_db_l: f32,
    pub rms_db_r: f32,
    pub vu_db_l: f32,
    pub vu_db_r: f32,
    pub vu_hold_db_l: f32,
    pub vu_hold_db_r: f32,
    pub persist_db_l: f32,
    pub persist_db_r: f32,
    pub hold_db_l: f32,
    pub hold_db_r: f32,
    pub hold_sec: f32,
    pub over_l: i32,
    pub over_r: i32,
}

impl Default for LevelSnapshot {
    fn default() -> Self {
        Self {
            peak_db_l: -120.0,
            peak_db_r: -120.0,
            true_db_l: -120.0,
            true_db_r: -120.0,
            rms_db_l: -120.0,
            rms_db_r: -120.0,
            vu_db_l: -120.0,
            vu_db_r: -120.0,
            vu_hold_db_l: -120.0,
            vu_hold_db_r: -120.0,
            persist_db_l: -120.0,
            persist_db_r: -120.0,
            hold_db_l: -120.0,
            hold_db_r: -120.0,
            hold_sec: 2.0,
            over_l: 0,
            over_r: 0,
        }
    }
}

const PHASE_TAPS: usize = 12;
const HIST_CAP: usize = 16;
const HIST_MASK: usize = HIST_CAP - 1;

pub struct LevelMeter {
    sr: f64,
    hist_l: [f32; HIST_CAP],
    hist_r: [f32; HIST_CAP],
    hist_idx_l: usize,
    hist_idx_r: usize,
    rms_ring_l: Vec<f32>,
    rms_ring_r: Vec<f32>,
    rms_head_l: usize,
    rms_head_r: usize,
    rms_sum_l: f64,
    rms_sum_r: f64,
    vu_pos_l: f64,
    vu_vel_l: f64,
    vu_pos_r: f64,
    vu_vel_r: f64,
    disp_peak_l: f32,
    disp_peak_r: f32,
    disp_true_l: f32,
    disp_true_r: f32,
    hold_l: f32,
    hold_r: f32,
    hold_sec: f64,
    hold_t_l: f64,
    hold_t_r: f64,
    vu_hold_l: f32,
    vu_hold_r: f32,
    vu_hold_t_l: f64,
    vu_hold_t_r: f64,
    persist_l: f32,
    persist_r: f32,
    over_l: i32,
    over_r: i32,
}

fn poly_coeffs() -> &'static [[f32; 4]; PHASE_TAPS] {
    static POLY: OnceLock<[[f32; 4]; PHASE_TAPS]> = OnceLock::new();
    POLY.get_or_init(|| {
        const L: usize = 4 * PHASE_TAPS;
        let c = (L - 1) as f32 * 0.5;
        let mut p = [[0.0f32; 4]; PHASE_TAPS];
        for n in 0..L {
            let x = (n as f32 - c) / 4.0;
            let h = if x.abs() < 1e-6 {
                4.0
            } else {
                4.0 * (std::f32::consts::PI * x).sin() / (std::f32::consts::PI * x)
            };
            let theta = 2.0 * std::f32::consts::PI * n as f32 / (L - 1) as f32;
            let w = 0.42 - 0.5 * theta.cos() + 0.08 * (2.0 * theta).cos();
            p[n / 4][n % 4] = h * w;
        }
        let mut g = [0.0f64; 4];
        for j in 0..PHASE_TAPS {
            for k in 0..4 {
                g[k] += p[j][k] as f64;
            }
        }
        for j in 0..PHASE_TAPS {
            for k in 0..4 {
                if g[k].abs() > 1e-9 {
                    p[j][k] /= g[k] as f32;
                }
            }
        }
        p
    })
}

impl LevelMeter {
    pub fn new() -> Self {
        Self {
            sr: 48_000.0,
            hist_l: [0.0; HIST_CAP],
            hist_r: [0.0; HIST_CAP],
            hist_idx_l: 0,
            hist_idx_r: 0,
            rms_ring_l: Vec::new(),
            rms_ring_r: Vec::new(),
            rms_head_l: 0,
            rms_head_r: 0,
            rms_sum_l: 0.0,
            rms_sum_r: 0.0,
            vu_pos_l: 0.0,
            vu_vel_l: 0.0,
            vu_pos_r: 0.0,
            vu_vel_r: 0.0,
            disp_peak_l: -120.0,
            disp_peak_r: -120.0,
            disp_true_l: -120.0,
            disp_true_r: -120.0,
            hold_l: -120.0,
            hold_r: -120.0,
            hold_sec: 0.0,
            hold_t_l: 0.0,
            hold_t_r: 0.0,
            vu_hold_l: -120.0,
            vu_hold_r: -120.0,
            vu_hold_t_l: 0.0,
            vu_hold_t_r: 0.0,
            persist_l: -120.0,
            persist_r: -120.0,
            over_l: 0,
            over_r: 0,
        }
    }

    pub fn set_sample_rate(&mut self, sr: f64) {
        self.sr = sr;
        let ring = (0.3 * sr).round().max(64.0) as usize;
        self.rms_ring_l = vec![0.0; ring];
        self.rms_ring_r = vec![0.0; ring];
        self.rms_head_l = 0;
        self.rms_head_r = 0;
        self.rms_sum_l = 0.0;
        self.rms_sum_r = 0.0;
        self.reset();
    }

    pub fn reset(&mut self) {
        self.hist_l = [0.0; HIST_CAP];
        self.hist_r = [0.0; HIST_CAP];
        self.hist_idx_l = 0;
        self.hist_idx_r = 0;
        self.rms_ring_l.fill(0.0);
        self.rms_ring_r.fill(0.0);
        self.rms_head_l = 0;
        self.rms_head_r = 0;
        self.rms_sum_l = 0.0;
        self.rms_sum_r = 0.0;
        self.vu_pos_l = 0.0;
        self.vu_vel_l = 0.0;
        self.vu_pos_r = 0.0;
        self.vu_vel_r = 0.0;
        self.vu_hold_l = -120.0;
        self.vu_hold_r = -120.0;
        self.vu_hold_t_l = 0.0;
        self.vu_hold_t_r = 0.0;
        self.persist_l = -120.0;
        self.persist_r = -120.0;
        self.disp_peak_l = -120.0;
        self.disp_peak_r = -120.0;
        self.disp_true_l = -120.0;
        self.disp_true_r = -120.0;
        self.hold_l = -120.0;
        self.hold_r = -120.0;
        self.hold_sec = 0.0;
        self.over_l = 0;
        self.over_r = 0;
    }

    pub fn reset_hold_over(&mut self) {
        self.hold_l = -120.0;
        self.hold_r = -120.0;
        self.vu_hold_l = -120.0;
        self.vu_hold_r = -120.0;
        self.persist_l = -120.0;
        self.persist_r = -120.0;
        self.over_l = 0;
        self.over_r = 0;
    }

    pub fn reset_hold(&mut self) {
        self.hold_l = -120.0;
        self.hold_r = -120.0;
        self.hold_t_l = 0.0;
        self.hold_t_r = 0.0;
        self.vu_hold_l = -120.0;
        self.vu_hold_r = -120.0;
        self.vu_hold_t_l = 0.0;
        self.vu_hold_t_r = 0.0;
    }

    pub fn reset_persist(&mut self) {
        self.persist_l = -120.0;
        self.persist_r = -120.0;
    }

    pub fn reset_meter_hold(&mut self) {
        self.hold_l = -120.0;
        self.hold_r = -120.0;
        self.hold_t_l = 0.0;
        self.hold_t_r = 0.0;
    }

    pub fn reset_over(&mut self) {
        self.over_l = 0;
        self.over_r = 0;
    }

    pub fn process(&mut self, l: &[f32], r: &[f32], mode: usize, hold_sec: f64) {
        let n = l.len().min(r.len());
        if n == 0 {
            return;
        }
        let sr = if self.sr > 0.0 { self.sr } else { 48_000.0 };
        self.hold_sec = hold_sec;
        let dt = 1.0 / sr;
        let w = 22.0f64;
        let w2dt = w * w * dt;
        let wdt2 = 2.0 * w * dt;

        let mut raw_peak_l = 0.0f32;
        let mut raw_peak_r = 0.0f32;
        let mut tp_peak_l = 0.0f32;
        let mut tp_peak_r = 0.0f32;

        for i in 0..n {
            let xl = l[i];
            let xr = r[i];
            raw_peak_l = raw_peak_l.max(xl.abs());
            raw_peak_r = raw_peak_r.max(xr.abs());
            step_vu(&mut self.vu_pos_l, &mut self.vu_vel_l, xl.abs(), dt, w2dt, wdt2);
            step_vu(&mut self.vu_pos_r, &mut self.vu_vel_r, xr.abs(), dt, w2dt, wdt2);
            if mode == 0 {
                tp_peak_l = tp_peak_l.max(true_peak_sample(
                    xl,
                    &mut self.hist_l,
                    &mut self.hist_idx_l,
                ));
                tp_peak_r = tp_peak_r.max(true_peak_sample(
                    xr,
                    &mut self.hist_r,
                    &mut self.hist_idx_r,
                ));
            } else {
                let ring = self.rms_ring_l.len();
                self.rms_sum_l += xl as f64 * xl as f64 - self.rms_ring_l[self.rms_head_l] as f64;
                self.rms_ring_l[self.rms_head_l] = xl * xl;
                self.rms_head_l = (self.rms_head_l + 1) % ring;
                self.rms_sum_r += xr as f64 * xr as f64 - self.rms_ring_r[self.rms_head_r] as f64;
                self.rms_ring_r[self.rms_head_r] = xr * xr;
                self.rms_head_r = (self.rms_head_r + 1) % ring;
            }
        }

        let peak_db_l = fast_amp_to_db(raw_peak_l, -120.0);
        let peak_db_r = fast_amp_to_db(raw_peak_r, -120.0);
        let true_db_l = fast_amp_to_db(tp_peak_l, -120.0);
        let true_db_r = fast_amp_to_db(tp_peak_r, -120.0);
        // RMS dB 不在此计算：与 C++ `Process` 一致，环和在 `snapshot()` 读时折算，
        // 避免模式 0（dBTP）下误读残留环和。
        let vu_db_l = vu_to_db(self.vu_pos_l as f32);
        let vu_db_r = vu_to_db(self.vu_pos_r as f32);

        let rate = 20.0 * n as f32 / sr as f32;
        self.disp_peak_l = ball(self.disp_peak_l, peak_db_l, rate);
        self.disp_peak_r = ball(self.disp_peak_r, peak_db_r, rate);
        self.disp_true_l = ball(self.disp_true_l, true_db_l, rate);
        self.disp_true_r = ball(self.disp_true_r, true_db_r, rate);

        if mode == 0 {
            if self.disp_true_l > 0.0 {
                self.persist_l = self.persist_l.max(self.disp_true_l);
            }
            if self.disp_true_r > 0.0 {
                self.persist_r = self.persist_r.max(self.disp_true_r);
            }
        }

        let (over_lv, over_rv, over_thr) = if mode == 0 {
            (true_db_l, true_db_r, -1.0f32)
        } else {
            (peak_db_l, peak_db_r, 0.0f32)
        };
        if over_lv > over_thr {
            self.over_l = 1;
        }
        if over_rv > over_thr {
            self.over_r = 1;
        }

        let main_l = if mode == 0 {
            self.disp_true_l
        } else {
            self.disp_peak_l
        };
        let main_r = if mode == 0 {
            self.disp_true_r
        } else {
            self.disp_peak_r
        };
        let dt_block = n as f32 / sr as f32;
        if hold_sec > 0.0 {
            self.hold_l = hold_step(
                self.hold_l,
                main_l,
                dt_block,
                rate,
                hold_sec,
                &mut self.hold_t_l,
            );
            self.hold_r = hold_step(
                self.hold_r,
                main_r,
                dt_block,
                rate,
                hold_sec,
                &mut self.hold_t_r,
            );
            self.vu_hold_l = hold_step(
                self.vu_hold_l,
                vu_db_l,
                dt_block,
                rate,
                hold_sec,
                &mut self.vu_hold_t_l,
            );
            self.vu_hold_r = hold_step(
                self.vu_hold_r,
                vu_db_r,
                dt_block,
                rate,
                hold_sec,
                &mut self.vu_hold_t_r,
            );
        } else {
            self.hold_l = -1000.0;
            self.hold_r = -1000.0;
            self.vu_hold_l = -1000.0;
            self.vu_hold_r = -1000.0;
        }
    }

    pub fn snapshot(&self) -> LevelSnapshot {
        LevelSnapshot {
            peak_db_l: self.disp_peak_l,
            peak_db_r: self.disp_peak_r,
            true_db_l: self.disp_true_l,
            true_db_r: self.disp_true_r,
            rms_db_l: fast_pwr_to_db(
                (self.rms_sum_l / self.rms_ring_l.len() as f64) as f32,
                -120.0,
            ),
            rms_db_r: fast_pwr_to_db(
                (self.rms_sum_r / self.rms_ring_r.len() as f64) as f32,
                -120.0,
            ),
            vu_db_l: vu_to_db(self.vu_pos_l as f32),
            vu_db_r: vu_to_db(self.vu_pos_r as f32),
            vu_hold_db_l: self.vu_hold_l,
            vu_hold_db_r: self.vu_hold_r,
            persist_db_l: self.persist_l,
            persist_db_r: self.persist_r,
            hold_db_l: self.hold_l,
            hold_db_r: self.hold_r,
            hold_sec: self.hold_sec as f32,
            over_l: self.over_l,
            over_r: self.over_r,
        }
    }

    pub fn peak_hold_state(&self) -> (f32, f32, f32) {
        (self.hold_l, self.hold_r, self.hold_sec as f32)
    }
}

fn true_peak_sample(x: f32, hist: &mut [f32; HIST_CAP], idx: &mut usize) -> f32 {
    hist[*idx] = x;
    *idx = (*idx + 1) & HIST_MASK;
    let poly = poly_coeffs();
    let mut y = [0.0f32; 4];
    for j in 0..PHASE_TAPS {
        let h = hist[(*idx + HIST_CAP - 1 - j) & HIST_MASK];
        for (k, acc) in y.iter_mut().enumerate() {
            *acc += poly[j][k] * h;
        }
    }
    y[0].abs().max(y[1].abs()).max(y[2].abs()).max(y[3].abs())
}

fn step_vu(pos: &mut f64, vel: &mut f64, u: f32, dt: f64, w2dt: f64, wdt2: f64) {
    *vel += w2dt * (u as f64 - *pos) - wdt2 * *vel;
    *pos += *vel * dt;
}

fn ball(disp: f32, raw: f32, rate: f32) -> f32 {
    if raw > disp {
        raw
    } else {
        raw.max(disp - rate)
    }
}

fn hold_step(hold: f32, main: f32, dt_block: f32, rate: f32, hold_sec: f64, hold_t: &mut f64) -> f32 {
    if main > hold {
        *hold_t = 0.0;
        main
    } else {
        *hold_t += dt_block as f64;
        if *hold_t >= hold_sec {
            main.max(hold - rate)
        } else {
            hold
        }
    }
}

fn vu_to_db(avg: f32) -> f32 {
    if avg > 1e-8 {
        fast_amp_to_db(1.1107 * avg, -120.0)
    } else {
        -120.0
    }
}

impl Default for LevelMeter {
    fn default() -> Self {
        Self::new()
    }
}
