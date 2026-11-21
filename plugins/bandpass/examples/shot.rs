//! 生成一张界面截图（不依赖 DAW）：cargo run -p orm-bandpass --example shot
use std::sync::Arc;

use orm_bandpass::{editor, BandPassParams};
use truce::prelude::*;

fn main() {
    let params = Arc::new(BandPassParams::new());

    // 灌入一段粉噪，让截图上能看到谱线（真实运行时由音频线程推入）
    let mut rng = 0x1234_5678u32;
    let mut lp = 0.0f32;
    for _ in 0..4800 {
        rng = rng.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let white = (rng >> 8) as f32 / 8_388_608.0 - 1.0;
        lp = lp * 0.92 + white * 0.08;
        let s = (white * 0.35 + lp * 1.2).clamp(-1.0, 1.0);
        params
            .analysis
            .tap
            .push_frames(&[s, s, s * 0.6, s * 0.6]);
    }
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
