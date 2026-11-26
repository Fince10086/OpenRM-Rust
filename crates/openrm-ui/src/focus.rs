//! 宿主键盘焦点管理。
//!
//! baseview 的子 NSView 默认会接受 first responder；它在 `open()` 时还会主动
//! 抢一次焦点。结果是插件窗口在前台时，空格、方向键等都被插件吞掉，宿主
//! 收不到。这里在编辑器打开后把 first responder 还给宿主；只有 BandPass 的
//! 文本输入框真正需要键盘时，才把焦点切回插件视图。

use std::ffi::c_void;
use std::sync::atomic::{AtomicU64, Ordering};

#[cfg(target_os = "macos")]
use objc2::rc::Retained;

pub struct PreviousResponder {
    #[cfg(target_os = "macos")]
    inner: objc2::rc::Retained<objc2_app_kit::NSResponder>,
}

pub struct Guard {
    #[cfg(target_os = "macos")]
    id: u64,
}

pub fn capture(parent: *mut c_void) -> Option<PreviousResponder> {
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::NSView;

        if parent.is_null() {
            return None;
        }
        let parent: &NSView = unsafe { &*(parent as *const NSView) };
        let window = parent.window()?;
        Some(PreviousResponder {
            inner: window.firstResponder()?,
        })
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = parent;
        None
    }
}

pub fn install(parent: *mut c_void, previous: Option<PreviousResponder>) -> Option<Guard> {
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::NSView;

        if parent.is_null() {
            return None;
        }
        let parent: &NSView = unsafe { &*(parent as *const NSView) };
        let window = parent.window()?;
        // baseview 会创建一个唯一的 BaseviewNSView_<uuid> 类；遮罩是普通
        // NSView，不能把它当成插件视图。
        let plugin = find_plugin_view(&parent)?;

        let previous = previous.map(|p| p.inner);
        // open 时 baseview 已经把焦点抢到插件视图；先还给宿主，键盘事件
        // 才会继续走宿主。文本输入框打开时再显式切回来。
        if is_first_responder(&window, &plugin) {
            make_first(&window, previous.as_deref());
        }

        static NEXT_ID: AtomicU64 = AtomicU64::new(1);
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        ENTRIES.with_borrow_mut(|v| {
            v.push(FocusEntry {
                id,
                window,
                plugin,
                previous,
            });
        });
        Some(Guard { id })
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (parent, previous);
        None
    }
}

/// `wants` 为 true 时让插件视图成为 first responder，false 时把焦点还给
/// 打开插件前宿主里的那个 responder。
pub fn set_text_focus(wants: bool) {
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::NSResponder;

        ENTRIES.with_borrow_mut(|entries| {
            let Some(entry) = entries.iter().find(|e| e.window.isKeyWindow()) else {
                return;
            };
            let current_is_plugin = is_first_responder(&entry.window, &entry.plugin);
            if wants && !current_is_plugin {
                // NSView 是 NSResponder 的子类；objc2 不做自动向上转型。
                let responder: &NSResponder =
                    unsafe { &*(Retained::as_ptr(&entry.plugin) as *const NSResponder) };
                entry.window.makeFirstResponder(Some(responder));
            } else if !wants && current_is_plugin {
                make_first(&entry.window, entry.previous.as_deref());
            }
        });
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = wants;
    }
}

impl Drop for Guard {
    fn drop(&mut self) {
        #[cfg(target_os = "macos")]
        {
            ENTRIES.with_borrow_mut(|entries| {
                if let Some(pos) = entries.iter().position(|e| e.id == self.id) {
                    let entry = entries.remove(pos);
                    if is_first_responder(&entry.window, &entry.plugin) {
                        make_first(&entry.window, entry.previous.as_deref());
                    }
                }
            });
        }
    }
}

#[cfg(target_os = "macos")]
struct FocusEntry {
    id: u64,
    window: objc2::rc::Retained<objc2_app_kit::NSWindow>,
    plugin: objc2::rc::Retained<objc2_app_kit::NSView>,
    previous: Option<objc2::rc::Retained<objc2_app_kit::NSResponder>>,
}

#[cfg(target_os = "macos")]
thread_local! {
    static ENTRIES: std::cell::RefCell<Vec<FocusEntry>> = const {
        std::cell::RefCell::new(Vec::new())
    };
}

#[cfg(target_os = "macos")]
fn find_plugin_view(view: &objc2_app_kit::NSView) -> Option<objc2::rc::Retained<objc2_app_kit::NSView>> {
    for sub in view.subviews().iter() {
        if sub
            .class()
            .name()
            .to_bytes()
            .starts_with(b"BaseviewNSView_")
        {
            return Some(sub.clone());
        }
        if let Some(found) = find_plugin_view(&sub) {
            return Some(found);
        }
    }
    None
}

#[cfg(target_os = "macos")]
fn is_first_responder(window: &objc2_app_kit::NSWindow, plugin: &objc2_app_kit::NSView) -> bool {
    window
        .firstResponder()
        .is_some_and(|r| {
            std::ptr::eq(
                Retained::as_ptr(&r) as *const (),
                plugin as *const objc2_app_kit::NSView as *const (),
            )
        })
}

#[cfg(target_os = "macos")]
fn make_first(
    window: &objc2_app_kit::NSWindow,
    responder: Option<&objc2_app_kit::NSResponder>,
) {
    window.makeFirstResponder(responder);
}
