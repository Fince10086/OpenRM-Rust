//! 编辑器 GPU 预热。
//!
//! 第一次 `EguiEditor::open()` 要同步创建 Metal device、wgpu 设备与
//! egui 管线；冷启动在部分机器上接近一秒，期间窗口只有遮罩。插件实例
//! 刚创建时（通常早于用户打开界面）在后台线程做一次同样的初始化，把
//! 驱动/Metal 的着色器缓存和管线缓存热起来。真正 open 时仍走
//! `truce-egui` 自己的路径，不共享设备，避免影响它原有的设备丢失恢复。
//!
//! 只在 macOS 上启用；其它平台保持原来的首帧行为。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, Once, OnceLock};
use std::thread::JoinHandle;

/// 用户已经开始真正打开编辑器。预热线程一旦看到这个标志就尽快退出，
/// 避免它在 `open()` 同步创建设备的同时再建一套 Metal 设备，反而把
/// 首次打开拖慢。
static OPEN_STARTED: AtomicBool = AtomicBool::new(false);

static PREWARM: Once = Once::new();
static PREWARM_THREAD: OnceLock<Mutex<Option<JoinHandle<()>>>> = OnceLock::new();

pub fn prewarm() {
    #[cfg(target_os = "macos")]
    PREWARM.call_once(|| {
        if let Ok(handle) = std::thread::Builder::new()
            .name("openrm-gpu-prewarm".to_owned())
            .spawn(warm)
        {
            *PREWARM_THREAD
                .get_or_init(|| Mutex::new(None))
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(handle);
        }
    });
    #[cfg(not(target_os = "macos"))]
    let _ = &PREWARM;
}

/// 编辑器 open 前清理预热线程。**绝不阻塞 open**：用户点开界面时预热带
/// 可能还没跑完，join 会把编辑器打开拖成明显的白屏等待。预热只是一个
/// 缓存优化；若已经结束就 join 回收，否则直接脱离线程，让 open 立刻走
/// 自己的 wgpu 路径。
pub fn wait() {
    OPEN_STARTED.store(true, Ordering::Release);
    #[cfg(target_os = "macos")]
    if let Some(slot) = PREWARM_THREAD.get() {
        let handle = slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(handle) = handle {
            if handle.is_finished() {
                let _ = handle.join();
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn warm() {
    if OPEN_STARTED.load(Ordering::Acquire) {
        return;
    }
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let Ok(adapter) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
    })) else {
        return;
    };
    if OPEN_STARTED.load(Ordering::Acquire) {
        return;
    }
    let limits = adapter.limits();
    let Ok((device, queue)) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("openrm-prewarm"),
        required_features: wgpu::Features::empty(),
        required_limits: limits,
        experimental_features: wgpu::ExperimentalFeatures::default(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    })) else {
        return;
    };
    // macOS 的 wgpu surface 会落到 BGRA8 或 RGBA8；两种目标格式的管线
    // 都编译一遍，保证真正 open 时命中已经热好的着色器。
    //
    // 重点：`egui_wgpu::Renderer::new` 本身不会编译 pipeline，只有真正
    // 提交一次 draw 才会。这里用一个 1x1 的离屏 render pass 画一个三角形，
    // 把 egui 的顶点/片元着色器和顶点布局都编译进驱动缓存。否则原预热
    // 只是创建了 device，首次 open 仍要同步编译，白屏等待不会消失。
    for format in [
        wgpu::TextureFormat::Bgra8Unorm,
        wgpu::TextureFormat::Rgba8Unorm,
    ] {
        if OPEN_STARTED.load(Ordering::Acquire) {
            return;
        }
        compile_egui_pipeline(&device, &queue, format);
    }
    queue.submit([]);
}

#[cfg(target_os = "macos")]
fn compile_egui_pipeline(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    format: wgpu::TextureFormat,
) {
    use egui::epaint::{ClippedPrimitive, Mesh, Primitive, Vertex};
    use egui::{Color32, Pos2, Rect};

    let mut mesh = Mesh::default();
    for (pos, uv) in [
        (Pos2::new(0.0, 0.0), Pos2::new(0.0, 0.0)),
        (Pos2::new(1.0, 0.0), Pos2::new(0.0, 0.0)),
        (Pos2::new(0.0, 1.0), Pos2::new(0.0, 0.0)),
    ] {
        mesh.vertices.push(Vertex {
            pos,
            uv,
            color: Color32::WHITE,
        });
    }
    mesh.indices = vec![0, 1, 2];

    let primitive = ClippedPrimitive {
        clip_rect: Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
        primitive: Primitive::Mesh(mesh),
    };
    let screen = egui_wgpu::ScreenDescriptor {
        size_in_pixels: [1, 1],
        pixels_per_point: 1.0,
    };

    let mut renderer =
        egui_wgpu::Renderer::new(device, format, egui_wgpu::RendererOptions::default());
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("openrm-prewarm-target"),
        size: wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("openrm-prewarm"),
    });
    renderer.update_buffers(device, queue, &mut encoder, &[primitive.clone()], &screen);
    {
        let mut pass = encoder
            .begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("openrm-prewarm-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
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
        renderer.render(&mut pass, &[primitive], &screen);
    }
    queue.submit([encoder.finish()]);
}
