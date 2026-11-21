//! 生成一张界面截图（不依赖 DAW）：cargo run -p orm-bandpass --example shot
//!
//! truce 自带的 `Editor::screenshot` 只跑一帧，而 egui 的 `set_fonts` 要到下一帧
//! 才生效；单帧会让三档字重全部退化为 Regular。这里自己建 Context，跑两帧
//! （第二帧产物为准），保证截图里的字体与真机一致。

use std::sync::Arc;

use orm_bandpass::{editor, BandPassParams};
use truce::core::editor::{ClosureBridge, EditorBridge, PluginContext};
use truce::prelude::*;
use truce_egui::EditorUi;

fn mock_bridge(params: Arc<BandPassParams>) -> Arc<dyn EditorBridge> {
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

fn main() {
    let params = Arc::new(BandPassParams::new());

    // 灌入一段粉噪，让截图上能看到谱线（真实运行时由音频线程推入）
    let mut rng = 0x1234_5678u32;
    let mut lp = 0.0f32;
    for _ in 0..4800 {
        rng = rng.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let white = (rng >> 8) as f32 / 8_388_608.0 - 1.0;
        lp = lp * 0.92 + white * 0.08;
        // 截图对齐原版那张：原版没有播放信号，谱线只贴底；给一点点电平
        // 让曲线形状仍可辨认。
        let s = (white * 0.35 + lp * 1.2).clamp(-1.0, 1.0) * 0.01;
        params.analysis.tap.push_frames(&[s, s, s * 0.6, s * 0.6]);
    }

    let plugin_ctx =
        PluginContext::<BandPassParams>::new(mock_bridge(params.clone()), params.clone());
    let mut ui_state = editor::BandPassUi::default();

    let egui_ctx = egui::Context::default();
    egui_ctx.set_visuals(egui_theme());
    // 截图没有交互，跳过 tooltip 的悬停延迟与动画时间。
    egui_ctx.global_style_mut(|s| s.animation_time = 0.0);

    let (w_pt, h_pt) = (openrm_ui::UI_WIDTH, openrm_ui::UI_HEIGHT);
    let ppp = 2.0f32;

    let mut output = None;
    for pass in 0..2 {
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
        let _ = pass;
    }
    let output = output.unwrap();
    let (rgba, w, h) =
        render(&egui_ctx, output, (w_pt, h_pt), ppp).expect("需要可用的 wgpu 适配器");

    let path = std::path::Path::new("target/bandpass-ui.png");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let file = std::fs::File::create(path).unwrap();
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

#[allow(clippy::too_many_lines)]
fn render(
    ctx: &egui::Context,
    output: egui::FullOutput,
    size: (f32, f32),
    ppp: f32,
) -> Option<(Vec<u8>, u32, u32)> {
    let clipped = ctx.tessellate(output.shapes, output.pixels_per_point);

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

    let mut renderer =
        egui_wgpu::Renderer::new(&device, format, egui_wgpu::RendererOptions::default());
    for (id, delta) in &output.textures_delta.set {
        renderer.update_texture(&device, &queue, *id, delta);
    }

    let screen = egui_wgpu::ScreenDescriptor {
        size_in_pixels: [phys_w, phys_h],
        pixels_per_point: ppp,
    };
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("shot"),
    });
    renderer.update_buffers(&device, &queue, &mut encoder, &clipped, &screen);
    {
        let mut pass = encoder
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("shot-egui"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
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
        renderer.render(&mut pass, &clipped, &screen);
    }

    let bytes_per_row = (phys_w * 4 + 255) & !255;
    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("shot-readback"),
        size: u64::from(bytes_per_row * phys_h),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(bytes_per_row),
                rows_per_image: Some(phys_h),
            },
        },
        wgpu::Extent3d {
            width: phys_w,
            height: phys_h,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(std::iter::once(encoder.finish()));

    let slice = readback.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| {
        tx.send(r).unwrap();
    });
    let _ = device.poll(wgpu::PollType::Wait {
        submission_index: None,
        timeout: None,
    });
    rx.recv().unwrap().expect("buffer map failed");

    let mapped = slice.get_mapped_range();
    let mut pixels = Vec::with_capacity((phys_w * phys_h * 4) as usize);
    for row in 0..phys_h {
        let start = (row * bytes_per_row) as usize;
        pixels.extend_from_slice(&mapped[start..start + (phys_w * 4) as usize]);
    }
    drop(mapped);
    readback.unmap();
    for id in &output.textures_delta.free {
        renderer.free_texture(id);
    }

    Some((pixels, phys_w, phys_h))
}
