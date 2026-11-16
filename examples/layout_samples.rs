//! 开发用脚本：只用 `示例图片/示例_不同位置字幕图片` 文件夹里的图片，模拟 4 张尺寸不一致的图片，
//! 做一次 16:9 的自动排版（不裁剪、画布紧贴内容），输出到 dist 目录。
//!
//! 运行：cargo run --release --example layout_samples

use std::path::PathBuf;

use gistpic_creator::imaging::layout;
use gistpic_creator::imaging::render;
use gistpic_creator::model::{LayoutTarget, Rgba};

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // 示例只用同一个文件夹里的图片，不混用不同文件夹。
    let folder = "示例图片/示例_不同位置字幕图片";
    if !root.join(folder).is_dir() {
        eprintln!("{folder} 不在本地。示例图片不进仓库（见 .gitignore），这个脚本要自己放素材才能跑。");
        return;
    }

    let first = image::open(root.join(format!("{folder}/字幕在下方_粘贴的图像.png")))
        .expect("读取示例图失败")
        .to_rgb8();
    let second = image::open(root.join(format!("{folder}/字幕在上方_粘贴的图像 (2).png")))
        .expect("读取示例图失败")
        .to_rgb8();
    // 用同一批图的中间一竖条模拟两张竖图，制造“尺寸不一样”的输入。
    let portrait_a = image::imageops::crop_imm(&first, 1300, 0, 1215, 2160).to_image();
    let portrait_b = image::imageops::crop_imm(&second, 1300, 0, 1215, 2160).to_image();

    // 故意打乱顺序：横、竖、横、竖
    let images = [first, portrait_a, second, portrait_b];
    let sizes: Vec<(u32, u32)> = images.iter().map(|image| image.dimensions()).collect();
    println!("输入尺寸（粘贴顺序）：{sizes:?}");

    let target = LayoutTarget::Aspect(16.0 / 9.0);
    let result = layout::arrange(&sizes, target, 8);
    println!(
        "排版结果：画布 {}x{}（比例 {:.3}，目标 {:.3}；画布紧贴内容，不补边）",
        result.canvas_w,
        result.canvas_h,
        result.aspect(),
        16.0 / 9.0
    );
    for placement in &result.placements {
        println!(
            "  第 {} 张：{}x{} @ ({}, {})",
            placement.item + 1,
            placement.w,
            placement.h,
            placement.x,
            placement.y
        );
    }

    let refs: Vec<&image::RgbImage> = images.iter().collect();
    let composed = render::compose_layout(&refs, &result, Rgba::BLACK);
    let bytes = render::encode_png(&composed).expect("PNG 编码失败");
    let output = root.join("dist/样例_排版结果.png");
    std::fs::write(&output, bytes).expect("写文件失败");
    println!("输出：{}", output.display());
}
