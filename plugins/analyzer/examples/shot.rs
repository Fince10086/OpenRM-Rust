//! 生成一张 Analyzer 界面截图（不依赖 DAW / 真实音频线程）：
//!
//! ```bash
//! ~/.cargo/bin/cargo run -p orm-analyzer --example shot
//! ```
//!
//! truce 自带的 `Editor::screenshot` 只跑一帧，而 egui 的 `set_fonts` 要到下一帧
//! 才生效；这里自己建 Context，先跑预热帧让字体两阶段安装完成、控件内部的
//! 滑窗/显示平滑收敛到稳态，最后一帧产物用于截图，保证截图里的字体与真机一致。
//!
//! 共享通道里灌入接近原版 `01-main-default.png` 的默认空闲态数据：低电平粉噪
//! 频谱、贴近下限的 dBTP 小读数、空 LUFS、等幅等相正弦（相关度 +1.00 / 平衡 C）；
//! CPU 读数约 5.18%。

use std::sync::Arc;
use std::sync::atomic::Ordering;

use orm_analyzer::editor;
use orm_analyzer::params::AnalyzerParams;
use orm_analyzer::ui::data::{LevelSnapshot, LoudnessSnapshot, ScopeFrame, SpectrumFrame};
use truce::core::editor::{ClosureBridge, EditorBridge, PluginContext};
use truce::prelude::*;
use truce_egui::EditorUi;

fn mock_bridge(params: Arc<AnalyzerParams>) -> Arc<dyn EditorBridge> {
    let setters = params.clone();
    Arc::new(ClosureBridge {
        begin_edit: Box::new(|_| {}),
        set_param: Box::new(move |id, norm| {
            setters.set_normalized(id, norm);
        }),
        end_edit: Box::new(|_| {}),
        request_resize: Box::new(|_, _| true),
        get_param: Box::new({
            let p = params.clone();
            move |id| p.get_normalized(id).unwrap_or(0.0)
        }),
        get_param_plain: Box::new({
            let p = params.clone();
            move |id| p.get_plain(id).unwrap_or(0.0)
        }),
        format_param: Box::new(move |id| {
            params
                .format_value(id, params.get_plain(id).unwrap_or(0.0))
                .unwrap_or_default()
        }),
        get_meter: Box::new(|_| 0.0),
        get_state: Box::new(Vec::new),
        set_state: Box::new(|_| {}),
        transport: Box::new(|| None),
    })
}

/// 粉噪频谱帧：2048 个 STFT 桶（默认 Res=M → fft_size 4096），幅度对齐原版
/// 空闲噪底：全段 -75 dB 量级噪底，左侧 20–80 Hz 隆起约 -68 dB，右侧 2–5 kHz
/// 一簇随机尖峰峰顶约 -55 dB（线性幅度，控件内再换 dB）。
fn pink_spectrum() -> SpectrumFrame {
    const N: usize = 2048;
    let mut rng = 0x1234_5678u32;
    let mut next = || {
        rng = rng.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (rng >> 8) as f32 / 8_388_608.0 - 1.0
    };

    let mut l = Vec::with_capacity(N);
    let mut r = Vec::with_capacity(N);
    let mut m = Vec::with_capacity(N);
    for i in 0..N {
        let f = (i + 1) as f32 / N as f32;
        let f_hz = f * 24_000.0;
        let noise = 0.5 + 0.5 * next();

        // 控件按 3 dB/Oct 给高频正倾斜，mock 源信号先按 -3 dB/oct 预补偿，
        // 显示出来才是平直的 -75 dB 噪底。
        let tilt_comp = (632.455_53 / f_hz.max(632.455_53)).sqrt();
        let floor = 1.8e-4 * (0.7 + 0.6 * noise) * tilt_comp;
        // 左侧低频隆起（20–80 Hz，最左缘），峰约 -68 dB。
        let left = (-((f - 0.012) / 0.02).powi(2)).exp() * 2.2 * 1.8e-4;
        // 右侧 2–5 kHz 簇（线性 bin  fraction ≈ 0.08–0.21）：窄包络 + 随机尖峰。
        let rc = (-((f - 0.145) / 0.035).powi(2)).exp();
        let spike = 0.35 + 0.65 * next();
        let right = rc * (2.0 + 6.0 * spike) * 1.8e-4 * tilt_comp;

        let amp = (floor + left + right).clamp(1e-5, 2.0e-3);
        l.push(amp);
        r.push(amp * 0.997);
        m.push(amp * 0.999);
    }
    SpectrumFrame { l, r, m, freqs: Vec::new() }
}

/// 示波器 / 声像帧：长缓冲（≥ 默认 0.1s 窗 + ACF 窗），L/R 等幅等相正弦。
/// 原版默认空闲态 Det 2369.4 Hz（0.42 ms 周期）；这里取同量级 2370 Hz，
/// 幅度约 -48 dBFS，轨迹细而居中；L=R → 相关度 +1.00、平衡 C。
fn scope_wave() -> ScopeFrame {
    const N: usize = 6144;
    const SR: f32 = 48_000.0;
    const FREQ: f32 = 2370.0;
    const AMP: f32 = 0.004_2; // ≈ -47.7 dBFS 线性幅度：轨迹细而居中可见
    let mut l = Vec::with_capacity(N);
    let mut r = Vec::with_capacity(N);
    for i in 0..N {
        let t = i as f32 / SR;
        let v = AMP * (2.0 * std::f32::consts::PI * FREQ * t).sin();
        l.push(v);
        r.push(v);
    }
    ScopeFrame { l, r }
}

fn mock_level() -> LevelSnapshot {
    // 默认空闲态：dBTP 仅贴近下限的一小段读数；VU/RMS/hold/persist 全部压到
    // 下限以下（不填充、不画保持线、不弹持久标签）；无 over 锁存。
    LevelSnapshot {
        peak_db_l: -110.0,
        peak_db_r: -110.0,
        true_db_l: -40.0,
        true_db_r: -41.0,
        rms_db_l: -110.0,
        rms_db_r: -110.0,
        vu_db_l: -110.0,
        vu_db_r: -110.0,
        vu_hold_db_l: -110.0,
        vu_hold_db_r: -110.0,
        persist_db_l: -110.0,
        persist_db_r: -110.0,
        hold_db_l: -110.0,
        hold_db_r: -110.0,
        hold_sec: 2.0,
        over_l: 0,
        over_r: 0,
    }
}

fn mock_loudness() -> LoudnessSnapshot {
    // 默认空闲态：响度计尚未积累有效积分 → LUFS 列不填充、LRA 竖排标签不出现。
    LoudnessSnapshot::default()
}

fn main() {
    let params = Arc::new(AnalyzerParams::new());

    // ---- 灌入演示数据（真实运行时由音频线程推入 shared） ----
    let shared = &params.analysis.shared;
    shared.sample_rate.store(48_000, Ordering::Relaxed);
    shared.store_cpu(0.0518);
    *shared.level.lock().unwrap() = mock_level();
    *shared.loudness.lock().unwrap() = mock_loudness();
    *shared.scope.lock().unwrap() = Some(scope_wave());
    *shared.spectrum.lock().unwrap() = Some((0u8, pink_spectrum()));

    let plugin_ctx =
        PluginContext::<AnalyzerParams>::new(mock_bridge(params.clone()), params.clone());
    let mut ui_state = editor::AnalyzerUi::default();

    let egui_ctx = egui::Context::default();
    egui_ctx.set_visuals(egui_theme());
    // 截图没有交互，跳过 tooltip 的悬停延迟与动画时间。
    egui_ctx.global_style_mut(|s| s.animation_time = 0.0);

    let (w_pt, h_pt) = (orm_analyzer::ui::layout::UI_W, orm_analyzer::ui::layout::UI_H);
    let ppp = 2.0f32;

    // 多跑几帧让控件内部的滑窗/显示平滑（相关度、电平爬坡、频谱释放）收敛到
    // 稳态；字体两阶段安装仍在第 0/1 帧完成，最后一帧产物用于截图。
    // renderer 与真实宿主一样跨帧存活：每帧把 textures_delta 增量喂给它，
    // 避免末帧只带图集补丁（patch）时新建 renderer 未分配整图而 panic / 全黑。
    let mut gpu = init_gpu((w_pt, h_pt), ppp).expect("需要可用的 wgpu 适配器");
    let mut output = None;
    const WARMUP_FRAMES: usize = 40;
    for pass in 0..WARMUP_FRAMES {
        let mut raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(w_pt, h_pt),
            )),
            time: Some(0.0),
            focused: true,
            ..Default::default()
        };
        raw.viewports
            .entry(egui::ViewportId::ROOT)
            .or_default()
            .native_pixels_per_point = Some(ppp);
        output = Some(egui_ctx.run_ui(raw, |ui| {
            ui_state.ui(ui, &plugin_ctx);
        }));
        if let Some(o) = &output {
            for (id, delta) in &o.textures_delta.set {
                gpu.renderer.update_texture(&gpu.device, &gpu.queue, *id, delta);
            }
            for id in &o.textures_delta.free {
                gpu.renderer.free_texture(id);
            }
        }
        let _ = pass;
    }
    let output = output.unwrap();
    let (rgba, w, h) = render_last(&egui_ctx, output, &mut gpu).expect("需要可用的 wgpu 适配器");

    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    let path = std::path::Path::new(&manifest)
        .join("../../docs/screenshots/analyzer-m1.png");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let file = std::fs::File::create(&path).unwrap();
    let mut enc = png::Encoder::new(file, w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().unwrap();
    writer.write_image_data(&rgba).unwrap();
    println!("{} {}x{}", path.display(), w, h);
}

fn egui_theme() -> egui::Visuals {
    let mut v = egui::Visuals::light();
    // 界面自身负责上色，基础控件不应带 egui 默认蓝。
    v.selection.bg_fill = egui::Color32::TRANSPARENT;
    v.widgets.noninteractive.bg_fill = egui::Color32::TRANSPARENT;
    v.widgets.noninteractive.bg_stroke = egui::Stroke::NONE;
    v.widgets.inactive.bg_fill = egui::Color32::TRANSPARENT;
    v.widgets.inactive.bg_stroke = egui::Stroke::NONE;
    v.widgets.hovered.bg_fill = egui::Color32::TRANSPARENT;
    v.widgets.hovered.bg_stroke = egui::Stroke::NONE;
    v.window_fill = egui::Color32::TRANSPARENT;
    v.window_stroke = egui::Stroke::NONE;
    v
}

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    _target: wgpu::Texture,
    view: wgpu::TextureView,
    renderer: egui_wgpu::Renderer,
    phys_w: u32,
    phys_h: u32,
    screen: egui_wgpu::ScreenDescriptor,
}

fn init_gpu(size: (f32, f32), ppp: f32) -> Option<Gpu> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .ok()?;
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("orm-shot"),
        required_features: wgpu::Features::empty(),
        required_limits: adapter.limits(),
        experimental_features: wgpu::ExperimentalFeatures::default(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    }))
    .ok()?;

    let format = wgpu::TextureFormat::Rgba8Unorm;
    let phys_w = (size.0 * ppp) as u32;
    let phys_h = (size.1 * ppp) as u32;

    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("shot-target"),
        size: wgpu::Extent3d {
            width: phys_w,
            height: phys_h,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());

    let renderer =
        egui_wgpu::Renderer::new(&device, format, egui_wgpu::RendererOptions::default());
    let screen = egui_wgpu::ScreenDescriptor {
        size_in_pixels: [phys_w, phys_h],
        pixels_per_point: ppp,
    };

    Some(Gpu { device, queue, _target: target, view, renderer, phys_w, phys_h, screen })
}

#[allow(clippy::too_many_lines)]
fn render_last(
    ctx: &egui::Context,
    output: egui::FullOutput,
    gpu: &mut Gpu,
) -> Option<(Vec<u8>, u32, u32)> {
    let clipped = ctx.tessellate(output.shapes, output.pixels_per_point);

    let mut encoder = gpu.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("shot"),
    });
    gpu.renderer
        .update_buffers(&gpu.device, &gpu.queue, &mut encoder, &clipped, &gpu.screen);
    {
        let mut pass = encoder
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("shot-egui"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &gpu.view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: 0.12,
                            g: 0.12,
                            b: 0.14,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            })
            .forget_lifetime();
        gpu.renderer.render(&mut pass, &clipped, &gpu.screen);
    }

    let bytes_per_row = (gpu.phys_w * 4 + 255) & !255;
    let readback = gpu.device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("shot-readback"),
        size: u64::from(bytes_per_row * gpu.phys_h),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &gpu._target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(gpu.phys_h),
            },
        },
        wgpu::Extent3d {
            width: gpu.phys_w,
            height: gpu.phys_h,
            depth_or_array_layers: 1,
        },
    );
    gpu.queue.submit(std::iter::once(encoder.finish()));

    let slice = readback.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        tx.send(r).unwrap();
    });
    let _ = gpu.device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: None,
    });
    rx.recv().unwrap().expect("buffer map failed");

    let mapped = slice.get_mapped_range();
    let mut pixels = Vec::with_capacity((gpu.phys_w * gpu.phys_h * 4) as usize);
    for row in 0..gpu.phys_h {
        let start = (row * bytes_per_row) as usize;
        pixels.extend_from_slice(&mapped[start..start + (gpu.phys_w * 4) as usize]);
    }
    drop(mapped);
    readback.unmap();

    Some((pixels, gpu.phys_w, gpu.phys_h))
}
