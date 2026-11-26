//! ORM Fusion —— truce 插件层。
//!
//! DSP 全在 `openrm-fusion-dsp`；这里只负责把宿主参数有变化时翻译成 bar 操作，
//! 以及块边界的缓冲拷贝。参数是"下一帧生效"，和原版一致。

use std::sync::Arc;

use openrm_fusion_dsp::FusionCore;
use truce::prelude::*;

pub mod editor;
pub mod params;

pub use params::{Bands, FusionParams, PARAM_COUNT};

pub struct Fusion;

pub struct FusionDsp {
    core: FusionCore,
    cache: Vec<f64>,
    in_l: Vec<f32>,
    in_r: Vec<f32>,
    out_l: Vec<f32>,
    out_r: Vec<f32>,
    last_reset: u32,
}

impl Default for FusionDsp {
    fn default() -> Self {
        Self {
            core: FusionCore::new(),
            cache: vec![f64::NAN; PARAM_COUNT],
            in_l: Vec::new(),
            in_r: Vec::new(),
            out_l: Vec::new(),
            out_r: Vec::new(),
            last_reset: 0,
        }
    }
}

impl FusionDsp {
    fn sync_params(&mut self, p: &FusionParams) {
        for id in 0..PARAM_COUNT as u32 {
            let Some(value) = p.get_plain(id) else {
                continue;
            };
            if (value - self.cache[id as usize]).abs() <= 1e-9 {
                continue;
            }
            self.cache[id as usize] = value;
            self.apply(id, value);
        }
    }

    fn apply(&mut self, id: u32, v: f64) {
        match id {
            4 => {
                self.core.set_bands_index((v.round() as usize).min(9));
            }
            5 => self.core.set_gain(v as f32),
            6 => self.core.set_mix(v as f32),
            23 => self.core.set_smoothness(v as f32),
            7..=14 => {
                self.core.bars[(id - 7) as usize].set_amplitude(v as f32);
            }
            15..=22 => {
                self.core.bars[(id - 15) as usize].feedback = v as f32;
            }
            24..=31 => {
                self.core.bars[(id - 24) as usize].set_delay_param(v as f32);
            }
            32..=39 => {
                self.core.bars[(id - 32) as usize].set_freq_param(v as f32);
            }
            40..=47 => {
                self.core.bars[(id - 40) as usize].set_length_param(v as f32);
            }
            48..=55 => {
                self.core.bars[(id - 48) as usize].set_angle_param(v as f32);
            }
            56..=63 => {
                self.core.bars[(id - 56) as usize].pan = v as f32;
            }
            64..=71 => {
                self.core.bars[(id - 64) as usize].width = v as f32;
            }
            72..=79 => {
                self.core.bars[(id - 72) as usize].active = v >= 0.5;
            }
            80..=87 => {
                self.core.bars[(id - 80) as usize].mute = v >= 0.5;
            }
            88 if v >= 0.5 => {
                self.core.reset();
            }
            89 if v >= 0.5 => {
                for bar in &mut self.core.bars {
                    bar.mute = true;
                }
            }
            _ => {}
        }
    }
}

impl PluginLogic for Fusion {
    type Params = FusionParams;
    type DspState = FusionDsp;

    fn init(_params: &FusionParams, _cx: &InitContext) -> FusionDsp {
        // 趁插件实例化、用户还没打开界面时把 Metal/wgpu 管线热起来，
        // 避免第一次 open 时同步建管线造成白屏/长时间空白。
        openrm_ui::gpu::prewarm();
        FusionDsp::default()
    }

    fn reset(state: &mut FusionDsp, params: &FusionParams, config: &AudioConfig) {
        params.host.set_sample_rate(config.sample_rate);
        state.cache.fill(f64::NAN);
        state.sync_params(params);
        let max_block = config.max_block_size.max(1);
        for buf in [
            &mut state.in_l,
            &mut state.in_r,
            &mut state.out_l,
            &mut state.out_r,
        ] {
            buf.clear();
            buf.resize(max_block, 0.0);
        }
    }

    fn process(
        state: &mut FusionDsp,
        params: &FusionParams,
        buffer: &mut AudioBuffer,
        _events: &EventList,
        _context: &mut ProcessContext,
    ) -> ProcessStatus {
        let n = buffer.num_samples();
        let nch = buffer.channels();
        params.host.set_sample_rate(_context.sample_rate);
        let generation = params.host.reset_generation();
        if generation != state.last_reset {
            state.last_reset = generation;
            state.core.reset();
        }
        state.sync_params(params);

        if n > state.in_l.len() {
            let len = n.max(1);
            for buf in [
                &mut state.in_l,
                &mut state.in_r,
                &mut state.out_l,
                &mut state.out_r,
            ] {
                buf.resize(len, 0.0);
            }
        }
        state.in_l[..n].copy_from_slice(&buffer.input(0)[..n]);
        if nch > 1 {
            state.in_r[..n].copy_from_slice(&buffer.input(1)[..n]);
        } else {
            state.in_r[..n].copy_from_slice(&state.in_l[..n]);
        }

        state.core.process(
            &state.in_l[..n],
            &state.in_r[..n],
            &mut state.out_l[..n],
            &mut state.out_r[..n],
        );

        for ch in 0..nch {
            let dst = buffer.output(ch);
            let src = if ch == 0 {
                &state.out_l[..n]
            } else {
                &state.out_r[..n]
            };
            dst[..n].copy_from_slice(src);
        }
        ProcessStatus::Normal
    }

    fn editor(params: Arc<FusionParams>) -> Box<dyn Editor> {
        editor::create(params)
    }
}

truce::plugin! {
    logic: Fusion,
    params: FusionParams,
}
