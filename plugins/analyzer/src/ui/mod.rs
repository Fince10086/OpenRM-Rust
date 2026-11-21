//! Analyzer 界面控件集合。每个模块对应原版一个 `controls/*.h`，
//! 一律「给定矩形 + 自绘」，坐标与尺寸来自原版常量。

pub mod cpu;
pub mod data;
pub mod field;
pub mod gonio;
pub mod layout;
pub mod meter;
pub mod scope;
pub mod settings;
pub mod spectrum;
pub mod widgets;
