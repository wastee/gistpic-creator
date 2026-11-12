//! 开发用脚本：只用 `示例图片/示例_多行字幕图片` 文件夹里的截图跑一遍字幕拼接，
//! 把结果写到 dist 目录，方便不用打开界面也能检查输出效果。
//!
//! 运行：cargo run --release --example stitch_samples

use std::path::PathBuf;

use gistpic_creator::imaging::crop::{self, CropBand, Inclusion};
use gistpic_creator::imaging::detect::SubtitleDetector;
use gistpic_creator::imaging::render;
use gistpic_creator::model::{Item, ItemStatus, Project};

fn main() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // 示例只用同一个文件夹里的图片，不混用不同文件夹。
    let folder = "示例图片/示例_多行字幕图片";
    let paths = [
        format!("{folder}/粘贴的图像.png"),
        format!("{folder}/粘贴的图像 (2).png"),
        format!("{folder}/粘贴的图像 (3).png"),
    ];

    let detector = SubtitleDetector::new_embedded().expect("检测模型应加载成功");
    let mut project = Project::default();

    for (index, relative) in paths.iter().enumerate() {
        let path = root.join(relative);
        let rgb = image::open(&path)
            .unwrap_or_else(|err| panic!("读取 {} 失败: {err}", path.display()))
            .to_rgb8();
        let (width, height) = rgb.dimensions();
        let block = detector
            .detect(&rgb)
            .unwrap_or_else(|err| panic!("检测 {relative} 失败: {err}"))
            .unwrap_or_else(|| panic!("{relative} 没有检测到字幕"));
        let first = block.lines[0];
        println!(
            "{}: {}x{}，字幕第一行 y={:.0}..{:.0}，共 {} 行",
            relative,
            width,
            height,
            first.y0,
            first.y1,
            block.lines.len()
        );
        project.items.push(Item {
            id: index as u64 + 1,
            name: (*relative).to_string(),
            width,
            height,
            rgb: std::sync::Arc::new(rgb),
            block: Some(block),
            status: ItemStatus::Ready,
            keep_whole: false,
            manual: None,
            analysis: Default::default(),
            fingerprint: Default::default(),
        });
    }

    println!(
        "出血位：{}% 字符高度",
        (project.settings.bleed * 100.0).round()
    );

    let inclusions = crop::resolve(&project);
    let mut parts: Vec<(std::sync::Arc<image::RgbImage>, CropBand)> = Vec::new();
    for (index, item) in project.items.iter().enumerate() {
        match inclusions[index] {
            Inclusion::Crop(band) => {
                println!(
                    "{}: 保留 y={}..{}（高度 {}）",
                    item.name,
                    band.y0,
                    band.y1,
                    band.height()
                );
                parts.push((item.rgb.clone(), band));
            }
            other => println!("{}: {:?}", item.name, other),
        }
    }

    let gap = project.settings.gap;
    let composed = render::compose_subtitle(
        &parts
            .iter()
            .map(|(rgb, band)| (rgb.as_ref(), *band))
            .collect::<Vec<_>>(),
        &gap,
    );

    std::fs::create_dir_all(root.join("dist")).expect("创建 dist 目录失败");
    let output = root.join("dist/样例_拼接结果.png");
    let bytes = render::encode_png(&composed).expect("PNG 编码失败");
    std::fs::write(&output, bytes).expect("写文件失败");
    println!(
        "输出：{}（{}x{}）",
        output.display(),
        composed.width(),
        composed.height()
    );
}
