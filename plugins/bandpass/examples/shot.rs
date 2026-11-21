//! 生成一张界面截图（不依赖 DAW）：cargo run -p orm-bandpass --example shot
use std::sync::Arc;

use orm_bandpass::{editor, BandPassParams};
use truce::prelude::*;

fn main() {
    let params = Arc::new(BandPassParams::new());
    let mut ed = editor::create(params.clone());
    let (rgba, w, h) = ed
        .screenshot(params.clone() as Arc<dyn Params>)
        .expect("需要可用的 wgpu 适配器");

    let path = std::path::Path::new("target/bandpass-ui.png");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let file = std::fs::File::create(path).unwrap();
    let mut enc = png::Encoder::new(file, w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().unwrap();
    writer.write_image_data(&rgba).unwrap();
    println!("{} {}x{}", path.display(), w, h);
}
