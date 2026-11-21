//! ORM BandPass —— truce 版插件入口。
//!
//! 这一层只做三件事：
//!   1. 把宿主参数（`BandPassParams`）换算成 DSP 核心单位（`params::to_core`）；
//!   2. 处理块边界：拷贝进出缓冲、每块推进一次参数平滑；
//!   3. 给出编辑器（当前是 truce 内置栅格 UI，正式界面见 docs/rust-port-plan.md）。
//!
//! DSP 算法全部在 `openrm-dsp` 里，与本文件无关——那条边界和 C++ 版的
//! `BandPassCore` / `iPlug2` 分界一致。

use std::sync::Arc;

use openrm_dsp::bandpass::BandPassCore;
use truce::prelude::*;

pub mod editor;
pub mod params;

pub use params::{BandPassParams, RandomColor, Slope};

/// 插件描述符（零大小）。实例状态在 `BandPassDsp`。
pub struct BandPass;

pub struct BandPassDsp {
    core: BandPassCore,
    in_l: Vec<f32>,
    in_r: Vec<f32>,
    out_l: Vec<f32>,
    out_r: Vec<f32>,
    wet_l: Vec<f32>,
    wet_r: Vec<f32>,
    tap: Option<Arc<truce::core::audio_tap::AudioTap<f32>>>,
}

impl Default for BandPassDsp {
    fn default() -> Self {
        Self {
            core: BandPassCore::new(),
            in_l: Vec::new(),
            in_r: Vec::new(),
            out_l: Vec::new(),
            out_r: Vec::new(),
            wet_l: Vec::new(),
            wet_r: Vec::new(),
            tap: None,
        }
    }
}

impl PluginLogic for BandPass {
    type Params = BandPassParams;
    type DspState = BandPassDsp;

    fn init(params: &BandPassParams, _cx: &InitContext) -> BandPassDsp {
        // 采样率在 reset() 才确定，这里只建空状态。
        let mut state = BandPassDsp::default();
        state.tap = Some(Arc::clone(&params.analysis.tap));
        state
    }

    fn reset(state: &mut BandPassDsp, params: &BandPassParams, config: &AudioConfig) {
        state.core.prepare(config.sample_rate);
        params.analysis.sample_rate.store(
            config.sample_rate.round() as u32,
            std::sync::atomic::Ordering::Relaxed,
        );
        let max_block = config.max_block_size.max(1);
        for buf in [
            &mut state.in_l,
            &mut state.in_r,
            &mut state.out_l,
            &mut state.out_r,
            &mut state.wet_l,
            &mut state.wet_r,
        ] {
            buf.clear();
            buf.resize(max_block, 0.0);
        }
    }

    fn process(
        state: &mut BandPassDsp,
        params: &BandPassParams,
        buffer: &mut AudioBuffer,
        _events: &EventList,
        _context: &mut ProcessContext,
    ) -> ProcessStatus {
        let n = buffer.num_samples();
        let nch = buffer.channels();

        state.in_l[..n].copy_from_slice(&buffer.input(0)[..n]);
        if nch > 1 {
            state.in_r[..n].copy_from_slice(&buffer.input(1)[..n]);
        } else {
            state.in_r[..n].copy_from_slice(&state.in_l[..n]);
        }

        // 参数在块起始处取快照：DSP 内部自己做 15 ms 一阶平滑（与原版一致），
        // 因此 truce 侧参数全部声明 smooth = "none"，避免二次平滑。
        state.core.set_params(&params.to_core());
        state.core.update_smoothing(n);

        state.core.process_stereo(
            &state.in_l[..n],
            &state.in_r[..n],
            &mut state.out_l[..n],
            &mut state.out_r[..n],
            Some(&mut state.wet_l[..n]),
            Some(&mut state.wet_r[..n]),
            n,
        );

        params
            .analysis
            .store_deltas(state.core.random_deltas(), nch as u32);

        // 频谱源：干/湿各两路，按帧交织后推给 UI（UI 侧做 STFT）。
        if let Some(tap) = &state.tap {
            let frames: Vec<[f32; 4]> = (0..n)
                .map(|i| [state.in_l[i], state.in_r[i], state.wet_l[i], state.wet_r[i]])
                .collect();
            tap.push_frames(&frames.concat());
        }

        let out0 = &state.out_l[..n];
        let out1 = &state.out_r[..n];
        for ch in 0..nch {
            let dst = buffer.output(ch);
            dst[..n].copy_from_slice(if ch == 0 { out0 } else { out1 });
        }

        ProcessStatus::Normal
    }

    fn editor(params: Arc<BandPassParams>) -> Box<dyn Editor> {
        editor::create(params)
    }
}

truce::plugin! {
    logic: BandPass,
    params: BandPassParams,
}
