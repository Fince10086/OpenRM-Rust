//! 字体：把 BandPass 原有的三档字重嵌进二进制。
//!
//! C++ 版把 ttf 拷进 bundle Resources 再注册 iPlug2 字体；Rust 版直接
//! `include_bytes!`，避免部署顺序、签名前后资源丢失这类问题。

use egui::{FontData, FontDefinitions, FontFamily};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub const REGULAR: &[u8] = include_bytes!("../assets/Mixed-Regular.ttf");
pub const SEMI_BOLD: &[u8] = include_bytes!("../assets/Mixed-SemiBold.ttf");
pub const BOLD: &[u8] = include_bytes!("../assets/Mixed-Bold.ttf");

pub const FAMILY_REGULAR: &str = "Mixed";
pub const FAMILY_SEMI_BOLD: &str = "Mixed-SemiBold";
pub const FAMILY_BOLD: &str = "Mixed-Bold";

/// 构造包含三档字重的字体定义（供截图/离线渲染在 `Context::run` 前预装）。
pub fn definitions() -> FontDefinitions {
    let mut defs = FontDefinitions::default();

    for (name, data) in [
        ("mixed-regular", REGULAR),
        ("mixed-semibold", SEMI_BOLD),
        ("mixed-bold", BOLD),
    ] {
        defs.font_data
            .insert(name.to_owned(), Arc::new(FontData::from_static(data)));
    }

    // 默认（Regular）作为正文字体，放在 Proportional 家族首位。
    let proportional = defs.families.entry(FontFamily::Proportional).or_default();
    proportional.insert(0, "mixed-regular".to_owned());

    defs.families.insert(
        FontFamily::Name(FAMILY_SEMI_BOLD.into()),
        vec!["mixed-semibold".to_owned(), "mixed-regular".to_owned()],
    );
    defs.families.insert(
        FontFamily::Name(FAMILY_BOLD.into()),
        vec!["mixed-bold".to_owned(), "mixed-semibold".to_owned()],
    );

    defs
}

/// 把三档字重装进 egui，并保留 egui 自带字体做兜底（箭头、符号等）。
pub fn install(ctx: &egui::Context) {
    ctx.set_fonts(definitions());
}

/// 首帧的字体集在 `ui()` 里才装上，egui 要到下一帧才把命名字体族生效。
/// 这个标记让首帧退回默认字体族，避免 `FontFamily::Name` 绑定失败而 panic；
/// 第二帧起才用真正的 SemiBold / Bold。截图是单帧渲染，因此权重会退化为 Regular。
static NAMED_READY: AtomicBool = AtomicBool::new(false);

pub fn set_named_ready(v: bool) {
    NAMED_READY.store(v, Ordering::Relaxed);
}

fn has(name: &str) -> bool {
    if !NAMED_READY.load(Ordering::Relaxed) {
        return false;
    }
    let _ = name;
    true
}

/// iPlug2 的 IText 字号是「像素」，在 macOS 上按 72dpi → pt 的换算落到
/// 0.8 倍；egui 的 FontId 直接是逻辑像素，因此统一乘这个系数才能与原版
/// 的字宽、行高对齐。
const TEXT_SCALE: f32 = 0.8;

pub fn regular(size: f32) -> egui::FontId {
    egui::FontId::new(size * TEXT_SCALE, FontFamily::Proportional)
}

pub fn semibold(size: f32) -> egui::FontId {
    if has(FAMILY_SEMI_BOLD) {
        egui::FontId::new(size * TEXT_SCALE, FontFamily::Name(FAMILY_SEMI_BOLD.into()))
    } else {
        egui::FontId::new(size * TEXT_SCALE, FontFamily::Proportional)
    }
}

pub fn bold(size: f32) -> egui::FontId {
    if has(FAMILY_BOLD) {
        egui::FontId::new(size * TEXT_SCALE, FontFamily::Name(FAMILY_BOLD.into()))
    } else {
        egui::FontId::new(size * TEXT_SCALE, FontFamily::Proportional)
    }
}
