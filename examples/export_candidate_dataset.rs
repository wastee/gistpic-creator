//! 导出角色分类器的候选文字块和清单。
//!
//! 用法：
//! `cargo run --release --example export_candidate_dataset -- --input <截图目录> --out <输出目录>`
//!
//! 输出目录包含：
//! - `crops/`：96×48 的分类器输入 PNG；
//! - `manifest.csv`：每行一个候选块，`label` 列留给人工标注。
//!
//! `source` 取截图相对路径的第一段目录；没有子目录时取文件名。
//! 训练工具按 `source` 切分训练集和测试集，避免同一视频同时出现在两边。

use std::path::{Path, PathBuf};

use gistpic_creator::imaging::detect::SubtitleDetector;
use gistpic_creator::imaging::role_input::prepare_candidate;

fn main() {
    let (input, output) = match parse_args() {
        Ok(paths) => paths,
        Err(message) => {
            eprintln!("{message}");
            eprintln!(
                "用法：cargo run --release --example export_candidate_dataset -- --input <截图目录> --out <输出目录>"
            );
            std::process::exit(2);
        }
    };

    if !input.is_dir() {
        eprintln!("截图目录不存在或不是目录：{}", input.display());
        std::process::exit(2);
    }
    if input == output {
        eprintln!("输出目录不能和截图目录相同");
        std::process::exit(2);
    }

    std::fs::create_dir_all(output.join("crops")).unwrap_or_else(|err| {
        eprintln!("创建输出目录 {} 失败：{err}", output.display());
        std::process::exit(1);
    });

    let mut files = Vec::new();
    collect_images(&input, &output, &mut files).unwrap_or_else(|err| {
        eprintln!("读取截图目录失败：{err}");
        std::process::exit(1);
    });
    files.sort();

    let detector = SubtitleDetector::new_embedded().unwrap_or_else(|err| {
        eprintln!("加载文字检测模型失败：{err}");
        std::process::exit(1);
    });

    let mut manifest =
        String::from("crop,source,label,cx,cy,w,h,lines,max_line_h,top_gap,bottom_gap\n");
    let mut exported = 0usize;
    let mut images = 0usize;
    let mut failed = 0usize;

    for path in files {
        let relative = path.strip_prefix(&input).unwrap_or(&path);
        let source = source_name(relative);
        let image = match image::open(&path) {
            Ok(image) => image.to_rgb8(),
            Err(err) => {
                eprintln!("跳过 {}：{err}", path.display());
                failed += 1;
                continue;
            }
        };
        images += 1;
        let blocks = match detector.detect_candidate_blocks(&image) {
            Ok(blocks) => blocks,
            Err(err) => {
                eprintln!("检测 {} 失败：{err}", path.display());
                failed += 1;
                continue;
            }
        };

        for block in blocks {
            let Some(candidate) = prepare_candidate(&image, &block) else {
                continue;
            };
            let name = format!("crop_{exported:06}.png");
            let crop_path = output.join("crops").join(&name);
            if let Err(err) = candidate.image.save(&crop_path) {
                eprintln!("写入 {} 失败：{err}", crop_path.display());
                std::process::exit(1);
            }
            let [cx, cy, w, h, lines, max_line_h, top_gap, bottom_gap] = candidate.geometry;
            manifest.push_str(&format!(
                "crops/{name},{source},,{cx:.6},{cy:.6},{w:.6},{h:.6},{lines:.6},{max_line_h:.6},{top_gap:.6},{bottom_gap:.6}\n"
            ));
            exported += 1;
        }
    }

    let manifest_path = output.join("manifest.csv");
    if let Err(err) = std::fs::write(&manifest_path, manifest) {
        eprintln!("写入 {} 失败：{err}", manifest_path.display());
        std::process::exit(1);
    }

    println!(
        "完成：读取 {} 张图片，导出 {} 个候选块，失败 {} 张",
        images, exported, failed
    );
    println!("清单：{}", manifest_path.display());
    println!("下一步：编辑 manifest.csv 的 label 列，取值 subtitle / watermark / danmaku / other");
}

fn parse_args() -> Result<(PathBuf, PathBuf), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut input = None;
    let mut output = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--input" => {
                input = args.get(index + 1).map(PathBuf::from);
                index += 2;
            }
            "--out" => {
                output = args.get(index + 1).map(PathBuf::from);
                index += 2;
            }
            other => return Err(format!("未知参数：{other}")),
        }
    }
    match (input, output) {
        (Some(input), Some(output)) => Ok((input, output)),
        _ => Err("缺少 --input 或 --out".to_string()),
    }
}

fn collect_images(dir: &Path, skip: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path == skip {
            continue;
        }
        if path.is_dir() {
            if path
                .file_name()
                .and_then(|name| name.to_str())
                .map(|name| {
                    name.starts_with('.') || matches!(name, "target" | "dist" | "node_modules")
                })
                .unwrap_or(false)
            {
                continue;
            }
            collect_images(&path, skip, files)?;
        } else if is_image(&path) {
            files.push(path);
        }
    }
    Ok(())
}

fn is_image(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "webp" | "bmp" | "gif"
            )
        })
        .unwrap_or(false)
}

fn source_name(relative: &Path) -> String {
    let mut components = relative.components();
    let first = components.next();
    match first {
        Some(std::path::Component::Normal(name)) => name.to_string_lossy().into_owned(),
        _ => relative
            .file_stem()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "unknown".to_string()),
    }
}
