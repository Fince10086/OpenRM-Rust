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
/// 中文：OPPO Sans 4.0（GB2312 子集，约 2 MB/档）。Outfit 只带 BandPass
/// 用过的少量汉字，Fusion 的“频段 / 延迟 / 反馈 / 随机”等缺字由它补齐。
pub const CJK_REGULAR: &[u8] = include_bytes!("../assets/OPPOSansSC-Regular.ttf");
pub const CJK_SEMI_BOLD: &[u8] = include_bytes!("../assets/OPPOSansSC-SemiBold.ttf");
pub const CJK_BOLD: &[u8] = include_bytes!("../assets/OPPOSansSC-Bold.ttf");

pub const FAMILY_REGULAR: &str = "Mixed";
pub const FAMILY_SEMI_BOLD: &str = "Mixed-SemiBold";
pub const FAMILY_BOLD: &str = "Mixed-Bold";

/// 构造包含三档字重的字体定义（供截图/离线渲染在 `Context::run` 前预装）。
pub fn definitions() -> FontDefinitions {
    let mut defs = FontDefinitions::default();

    // 中文兜底放在各家族最前面：它不含拉丁字母，拉丁仍由 Outfit 渲染；
    // 汉字则统一走 OPPO Sans，避免个别字落在 Outfit 的小字集里。
    defs.font_data.insert(
        "cjk-regular".to_owned(),
        Arc::new(FontData::from_static(CJK_REGULAR)),
    );
    defs.font_data.insert(
        "cjk-semibold".to_owned(),
        Arc::new(FontData::from_static(CJK_SEMI_BOLD)),
    );
    defs.font_data.insert(
        "cjk-bold".to_owned(),
        Arc::new(FontData::from_static(CJK_BOLD)),
    );

    for (name, data) in [
        ("mixed-regular", REGULAR),
        ("mixed-semibold", SEMI_BOLD),
        ("mixed-bold", BOLD),
    ] {
        defs.font_data
            .insert(name.to_owned(), Arc::new(FontData::from_static(data)));
    }

    // 拉丁字符必须排在中文字体之前：OPPO Sans 不含拉丁，但 egui 的默认
    // Proportional 家族会排在后面；若把 Outfit 放到默认字体之后，标准字重
    // 的英文/数字会先命中 egui 自带的 Ubuntu。这里显式重排，再把 egui 的
    // 默认字体放到末尾兜底箭头/符号。
    let fallback = defs
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();
    let mut proportional = vec!["mixed-regular".to_owned(), "cjk-regular".to_owned()];
    proportional.extend(fallback);
    defs.families.insert(FontFamily::Proportional, proportional);

    defs.families.insert(
        FontFamily::Name(FAMILY_SEMI_BOLD.into()),
        vec![
            "mixed-semibold".to_owned(),
            "cjk-semibold".to_owned(),
            "mixed-regular".to_owned(),
        ],
    );
    defs.families.insert(
        FontFamily::Name(FAMILY_BOLD.into()),
        vec!["mixed-bold".to_owned(), "cjk-bold".to_owned(), "mixed-semibold".to_owned()],
    );

    defs
}

/// 把三档字重装进 egui，并保留 egui 自带字体做兜底（箭头、符号等）。
pub fn install(ctx: &egui::Context) {
    // 新编辑器可能复用同一个 egui Context：字体定义被重置后，命名字体族
    // 要到下一帧才重新建立。若沿用上一个实例的 ready 标记，首帧就会去
    // 取尚不存在的 Named family，宿主里重开编辑器时会直接空白。
    NAMED_READY.store(false, Ordering::Relaxed);
    ctx.set_fonts(definitions());
    // egui 默认会把很小的实心圆替换成字体图集里的预栅格圆点。那些圆点
    // 是按字体 atlas 的像素密度生成的，放到 2x 屏或随机球上会直接看出
    // 阶梯。统一走 epaint 的路径 + feathering 抗锯齿。
    ctx.tessellation_options_mut(|o| {
        o.prerasterized_discs = false;
    });
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

/// 字号取整到像素：iPlug2 的字号换算到 0.8 倍后经常是 10.4 / 14.4 这类
/// 小数，ab_glyph 在小数字号下会按亚像素栅格化，中文笔画容易发灰、发毛。
fn px(size: f32) -> f32 {
    (size * TEXT_SCALE).round().max(1.0)
}

pub fn regular(size: f32) -> egui::FontId {
    egui::FontId::new(px(size), FontFamily::Proportional)
}

pub fn semibold(size: f32) -> egui::FontId {
    if has(FAMILY_SEMI_BOLD) {
        egui::FontId::new(px(size), FontFamily::Name(FAMILY_SEMI_BOLD.into()))
    } else {
        egui::FontId::new(px(size), FontFamily::Proportional)
    }
}

pub fn bold(size: f32) -> egui::FontId {
    if has(FAMILY_BOLD) {
        egui::FontId::new(px(size), FontFamily::Name(FAMILY_BOLD.into()))
    } else {
        egui::FontId::new(px(size), FontFamily::Proportional)
    }
}
