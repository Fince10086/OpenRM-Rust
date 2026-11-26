//! OpenRM local shim for `truce-gui-utils`.
//!
//! truce's helper decides whether an editor frame may run by looking at
//! `NSWindowOcclusionStateVisible`. On macOS that bit can still be clear while
//! the host is ordering a freshly opened plug-in window front. Every early
//! `on_frame` is then dropped, so the editor paints nothing and the user sees
//! the host's white background until AppKit finally reports the window visible
//! (often one or more seconds; on reopen the window can stay "not visible"
//! until the host does more work).
//!
//! We keep the same "don't queue drawables into an occluded window forever"
//! policy, but let a new editor view paint its first few frames unconditionally.
//! That makes the first visible frame independent of the occlusion bit and also
//! covers the close -> reopen case. After the grace period the normal
//! occlusion guard resumes, so a minimized / fully covered window does not
//! accumulate Metal drawables.
//!
//! This replaces only the three host-side helpers used by truce's GUI backends.

#![allow(clippy::module_name_repetitions)]
#![allow(unexpected_cfgs)]
#![allow(clippy::unnecessary_cast)]

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::c_void;

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy)]
struct NsPoint {
    x: f64,
    y: f64,
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy)]
struct NsSize {
    width: f64,
    height: f64,
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy)]
struct NsRect {
    origin: NsPoint,
    size: NsSize,
}

/// Number of frames a freshly created editor view may paint before the
/// occlusion guard starts dropping frames.
///
/// A 15 ms baseview tick makes this roughly a quarter of a second. It is
/// deliberately more than the one frame egui needs: a host-driven resize can
/// consume a tick or two, and the first frame also has to upload egui's font
/// atlas before anything appears.
#[cfg(target_os = "macos")]
const STARTUP_FRAMES: u32 = 16;

#[cfg(target_os = "macos")]
thread_local! {
    static FRAME_COUNTS: RefCell<HashMap<usize, u32>> = RefCell::new(HashMap::new());
}

#[cfg(target_os = "macos")]
fn note_frame(view: *mut c_void) -> u32 {
    FRAME_COUNTS.with(|counts| {
        let mut counts = counts.borrow_mut();
        // A host can cycle many editor views in one process. Keep the map
        // bounded; entries are only a few bytes and old pointers are never
        // reused while the map holds them, but clearing en masse is enough.
        if counts.len() > 64 {
            counts.clear();
        }
        let count = counts.entry(view as usize).or_insert(0);
        *count = count.saturating_add(1);
        *count
    })
}

/// Pin the embedded view's top edge to its superview in unflipped Cocoa
/// coordinates. Same behaviour truce's helper provides.
#[cfg(target_os = "macos")]
pub fn reanchor_to_superview_top(handle: raw_window_handle::RawWindowHandle) {
    use objc::{msg_send, sel, sel_impl};

    let view_ptr = match handle {
        raw_window_handle::RawWindowHandle::AppKit(h) => h.ns_view,
        _ => return,
    };
    if view_ptr.is_null() {
        return;
    }

    unsafe {
        let view = view_ptr.cast::<objc::runtime::Object>();
        let superview: *mut objc::runtime::Object = msg_send![view, superview];
        if superview.is_null() {
            return;
        }
        let parent_frame: NsRect = msg_send![superview, frame];
        let child_frame: NsRect = msg_send![view, frame];
        let target_y = parent_frame.size.height - child_frame.size.height;
        if (target_y - child_frame.origin.y).abs() < f64::EPSILON {
            return;
        }
        let origin = NsPoint {
            x: child_frame.origin.x,
            y: target_y,
        };
        let _: () = msg_send![view, setFrameOrigin: origin];
    }
}

#[cfg(not(target_os = "macos"))]
pub fn reanchor_to_superview_top(_handle: raw_window_handle::RawWindowHandle) {}

/// Whether the backend should skip this `on_frame` tick.
///
/// `true` only when the view is detached from a window. For the first
/// [`STARTUP_FRAMES`] ticks of a newly attached view, `false` regardless of the
/// occlusion bit so the editor gets a real first paint. Afterwards, skip when
/// macOS says the host window is not visible, matching truce's drawable-growth
/// protection.
#[cfg(target_os = "macos")]
#[must_use]
pub fn should_skip_frame(handle: raw_window_handle::RawWindowHandle) -> bool {
    use objc::{msg_send, sel, sel_impl};

    let view_ptr = match handle {
        raw_window_handle::RawWindowHandle::AppKit(h) => h.ns_view,
        _ => return false,
    };
    if view_ptr.is_null() {
        return true;
    }

    unsafe {
        let view = view_ptr.cast::<objc::runtime::Object>();
        let window: *mut objc::runtime::Object = msg_send![view, window];
        if window.is_null() {
            return true;
        }

        // `occlusionState` can stay clear while the host is first ordering
        // the plug-in window front. `isVisible` is the stable signal the host
        // window is actually on screen; use it instead so an opened editor is
        // never gated on a state AppKit has not updated yet.
        let visible: bool = msg_send![window, isVisible];
        if !visible {
            return true;
        }

        let frame_index = note_frame(view_ptr);
        if frame_index <= STARTUP_FRAMES {
            return false;
        }
        false
    }
}

#[cfg(target_os = "windows")]
#[must_use]
pub fn should_skip_frame(handle: raw_window_handle::RawWindowHandle) -> bool {
    unsafe extern "system" {
        fn IsWindowVisible(hwnd: *mut c_void) -> i32;
        fn IsIconic(hwnd: *mut c_void) -> i32;
    }

    let hwnd = match handle {
        raw_window_handle::RawWindowHandle::Win32(h) => h.hwnd,
        _ => return false,
    };
    if hwnd.is_null() {
        return true;
    }
    unsafe { IsWindowVisible(hwnd) == 0 || IsIconic(hwnd) != 0 }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
#[must_use]
pub fn should_skip_frame(_handle: raw_window_handle::RawWindowHandle) -> bool {
    false
}

/// Walk the parent's direct subviews and pin each top edge to the parent's top.
#[cfg(target_os = "macos")]
pub fn reanchor_all_children_to_top(parent: *mut c_void) {
    use objc::runtime::Object;
    use objc::{msg_send, sel, sel_impl};

    if parent.is_null() {
        return;
    }

    unsafe {
        let parent_obj = parent.cast::<Object>();
        let window: *mut Object = msg_send![parent_obj, window];
        if window.is_null() {
            return;
        }
        let parent_frame: NsRect = msg_send![parent_obj, frame];
        let subviews: *mut Object = msg_send![parent_obj, subviews];
        if subviews.is_null() {
            return;
        }
        let count: usize = msg_send![subviews, count];
        for i in 0..count {
            let child: *mut Object = msg_send![subviews, objectAtIndex: i];
            if child.is_null() {
                continue;
            }
            let child_frame: NsRect = msg_send![child, frame];
            let target_y = parent_frame.size.height - child_frame.size.height;
            if (target_y - child_frame.origin.y).abs() < f64::EPSILON {
                continue;
            }
            let origin = NsPoint {
                x: child_frame.origin.x,
                y: target_y,
            };
            let _: () = msg_send![child, setFrameOrigin: origin];
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub fn reanchor_all_children_to_top(_parent: *mut c_void) {}

/// Not used by OpenRM's macOS builds. Keep the symbol so a Windows build of
/// the workspace still links.
#[cfg(target_os = "windows")]
#[must_use]
pub fn wgl_extensions_available() -> bool {
    false
}
