//! 编辑器 GPU 预热。
//!
//! 第一次 `EguiEditor::open()` 要同步创建 Metal device、wgpu 设备与
//! egui 管线；冷启动在部分机器上接近一秒，期间窗口只有遮罩。插件实例
//! 刚创建时（通常早于用户打开界面）在后台线程做一次同样的初始化，把
//! 驱动/Metal 的着色器缓存和管线缓存热起来。真正 open 时仍走
//! `truce-egui` 自己的路径，不共享设备，避免影响它原有的设备丢失恢复。
//!
//! 只在 macOS 上启用；其它平台保持原来的首帧行为。

use std::sync::{Mutex, Once, OnceLock};
use std::thread::JoinHandle;

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
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let Ok(adapter) = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
    })) else {
        return;
    };
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
    for format in [
        wgpu::TextureFormat::Bgra8Unorm,
        wgpu::TextureFormat::Rgba8Unorm,
    ] {
        let _ = egui_wgpu::Renderer::new(
            &device,
            format,
            egui_wgpu::RendererOptions::default(),
        );
    }
    queue.submit([]);
}
