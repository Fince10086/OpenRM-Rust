//! OpenRM 共用 egui 组件：主题、字体、滤波垫/滑杆/按钮。
//!
//! 设计原则：控件不直接写参数，只把"用户想改成什么"返回给调用方
//! （`PadAction` 之类），由插件层负责 `begin_edit / set_param / end_edit`，
//! 这样宿主自动化手势的边界只有一处。

pub mod fonts;
pub mod theme;
pub mod widgets;

pub use theme::{Theme, ThemeMode};

/// 插件界面统一逻辑尺寸（与原 C++ 版一致）。
pub const UI_WIDTH: f32 = 916.0;
pub const UI_HEIGHT: f32 = 646.0;
