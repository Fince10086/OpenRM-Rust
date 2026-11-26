//! macOS 原生 NSMenu 右键菜单。
//!
//! egui 的自绘菜单不会参与 AppKit 的菜单跟踪循环，点击菜单外区域时焦点
//! 和关闭时机都不可靠；这里直接用 NSMenu，外观、点击外部关闭、键盘导航
//! 都交给系统。

pub enum PopupResult {
    Unsupported,
    Dismissed,
    Selected(usize),
}

#[derive(Clone, Copy)]
pub struct MenuItem<'a> {
    pub label: &'a str,
    pub checked: bool,
    pub enabled: bool,
}

impl<'a> MenuItem<'a> {
    pub fn new(label: &'a str) -> Self {
        Self {
            label,
            checked: false,
            enabled: true,
        }
    }
}

pub fn popup(items: &[MenuItem<'_>]) -> PopupResult {
    #[cfg(target_os = "macos")]
    {
        use std::sync::atomic::{AtomicIsize, Ordering};
        use std::sync::OnceLock;

        use objc2::rc::Retained;
        use objc2::runtime::{AnyClass, AnyObject, ClassBuilder, NSObject, Sel};
        use objc2::{msg_send, sel, ClassType, MainThreadMarker};
        use objc2_app_kit::{
            NSApplication, NSControlStateValueOff, NSControlStateValueOn, NSMenu, NSMenuItem,
        };
        use objc2_foundation::NSString;

        static LAST: AtomicIsize = AtomicIsize::new(-1);
        static TARGET_CLASS: OnceLock<&'static AnyClass> = OnceLock::new();

        extern "C-unwind" fn menu_action(
            _this: &AnyObject,
            _cmd: Sel,
            sender: &NSMenuItem,
        ) {
            LAST.store(sender.tag(), Ordering::SeqCst);
        }

        fn target_class() -> &'static AnyClass {
            *TARGET_CLASS.get_or_init(|| {
                let mut builder = ClassBuilder::new(c"OpenRMMenuTarget", NSObject::class())
                    .expect("OpenRMMenuTarget");
                unsafe {
                    builder.add_method(
                        sel!(openrmMenuAction:),
                        menu_action as extern "C-unwind" fn(_, _, _),
                    );
                }
                builder.register()
            })
        }

        let Some(mtm) = MainThreadMarker::new() else {
            return PopupResult::Unsupported;
        };
        let Some(window) = NSApplication::sharedApplication(mtm).keyWindow() else {
            return PopupResult::Unsupported;
        };
        let Some(view) = window.contentView() else {
            return PopupResult::Unsupported;
        };

        let menu = NSMenu::new(mtm);
        menu.setAutoenablesItems(false);
        let class = target_class();
        let target: Retained<AnyObject> = unsafe {
            let obj: Option<Retained<AnyObject>> = msg_send![class, new];
            let Some(obj) = obj else {
                return PopupResult::Dismissed;
            };
            obj
        };

        for (i, item) in items.iter().enumerate() {
            let menu_item = NSMenuItem::new(mtm);
            menu_item.setTitle(&NSString::from_str(item.label));
            menu_item.setTag(i as isize);
            unsafe {
                menu_item.setAction(Some(sel!(openrmMenuAction:)));
                menu_item.setTarget(Some(&target));
            }
            menu_item.setEnabled(item.enabled);
            menu_item.setState(if item.checked {
                NSControlStateValueOn
            } else {
                NSControlStateValueOff
            });
            menu.addItem(&menu_item);
        }

        LAST.store(-1, Ordering::SeqCst);
        let location = view.convertPoint_fromView(window.mouseLocationOutsideOfEventStream(), None);
        menu.popUpMenuPositioningItem_atLocation_inView(None, location, Some(&view));
        match LAST.load(Ordering::SeqCst) {
            i if i >= 0 && (i as usize) < items.len() => PopupResult::Selected(i as usize),
            _ => PopupResult::Dismissed,
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = items;
        PopupResult::Unsupported
    }
}
