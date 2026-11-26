//! 编辑器打开时的首帧遮罩。
//!
//! `truce-egui` 在 `open()` 里同步建 wgpu 设备/管线，冷启动时可能阻塞几百
//! 毫秒；这期间 baseview 子视图尚未画出第一帧，宿主父视图的默认白底会
//! 先露出来。这里在父 NSView 上先放一层和界面同色的普通 NSView，等
//! Metal 首帧覆盖它即可。遮罩位于 baseview 子视图下方，不参与鼠标事件。

use std::sync::Arc;

use egui::Color32;
use truce_core::editor::{Editor, PluginContext, RawWindowHandle};
use truce_params::Params;

/// 把编辑器包一层首帧遮罩。非 macOS 平台原样返回。
pub fn guard(editor: Box<dyn Editor>, background: Color32) -> Box<dyn Editor> {
    #[cfg(target_os = "macos")]
    {
        Box::new(MacFlashGuard {
            inner: editor,
            background,
            overlay: None,
        })
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = background;
        editor
    }
}

#[cfg(target_os = "macos")]
struct MacFlashGuard {
    inner: Box<dyn Editor>,
    background: Color32,
    overlay: Option<Overlay>,
}

#[cfg(target_os = "macos")]
struct Overlay(objc2::rc::Retained<objc2_app_kit::NSView>);

// AppKit 只能在宿主 GUI 线程上访问；Editor 本身就要求在该线程调用。
#[cfg(target_os = "macos")]
unsafe impl Send for Overlay {}

#[cfg(target_os = "macos")]
impl MacFlashGuard {
    fn remove_overlay(&mut self) {
        if let Some(overlay) = self.overlay.take() {
            overlay.0.removeFromSuperview();
        }
    }
}

#[cfg(target_os = "macos")]
impl Editor for MacFlashGuard {
    fn size(&self) -> (u32, u32) {
        self.inner.size()
    }

    fn open(&mut self, parent: RawWindowHandle, context: PluginContext) {
        self.remove_overlay();
        if let RawWindowHandle::AppKit(ptr) = parent {
            self.overlay = unsafe { install_overlay(ptr.cast(), self.background) };
        }
        // 遮罩先挂上，再等后台预热（若还在跑），最后才让 baseview 建
        // 子窗口；这样 wgpu 初始化期间宿主看到的已经是界面底色。
        crate::gpu::wait();
        self.inner.open(parent, context);
    }

    fn close(&mut self) {
        self.remove_overlay();
        self.inner.close();
    }

    fn idle(&mut self) {
        self.inner.idle();
    }

    fn set_size(&mut self, width: u32, height: u32) -> bool {
        self.inner.set_size(width, height)
    }

    fn can_resize(&self) -> bool {
        self.inner.can_resize()
    }

    fn can_maximize(&self) -> bool {
        self.inner.can_maximize()
    }

    fn min_size(&self) -> (u32, u32) {
        self.inner.min_size()
    }

    fn max_size(&self) -> (u32, u32) {
        self.inner.max_size()
    }

    fn size_increment(&self) -> Option<(u32, u32)> {
        self.inner.size_increment()
    }

    fn aspect_ratio(&self) -> Option<(u32, u32)> {
        self.inner.aspect_ratio()
    }

    fn prefers_pow2(&self) -> bool {
        self.inner.prefers_pow2()
    }

    fn set_scale_factor(&mut self, factor: f64) {
        self.inner.set_scale_factor(factor);
    }

    fn set_uses_system_scale(&mut self, yes: bool) {
        self.inner.set_uses_system_scale(yes);
    }

    fn state_changed(&mut self) {
        self.inner.state_changed();
    }

    fn screenshot(
        &mut self,
        params: Arc<dyn Params>,
    ) -> Option<(Vec<u8>, u32, u32)> {
        self.inner.screenshot(params)
    }
}

#[cfg(target_os = "macos")]
unsafe fn install_overlay(
    parent: *mut std::ffi::c_void,
    background: Color32,
) -> Option<Overlay> {
    use objc2::{MainThreadMarker, MainThreadOnly};
    use objc2_app_kit::{NSAutoresizingMaskOptions, NSView};
    use objc2_core_graphics::CGColor;

    if parent.is_null() {
        return None;
    }
    let parent: &NSView = unsafe { &*(parent as *const NSView) };
    let frame = parent.bounds();
    let mtm = MainThreadMarker::new().expect("编辑器必须在 AppKit 主线程创建");
    let view = NSView::initWithFrame(NSView::alloc(mtm), frame);
    view.setWantsLayer(true);
    view.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable
            | NSAutoresizingMaskOptions::ViewHeightSizable,
    );
    if let Some(layer) = view.layer() {
        let [r, g, b, a] = background.to_srgba_unmultiplied();
        let color = CGColor::new_srgb(
            f64::from(r) / 255.0,
            f64::from(g) / 255.0,
            f64::from(b) / 255.0,
            f64::from(a) / 255.0,
        );
        layer.setBackgroundColor(Some(&color));
    }
    parent.addSubview(&view);
    // 父视图会 retain 这个子视图；这里也保留一份，确保 close 时指针有效。
    Some(Overlay(view))
}
