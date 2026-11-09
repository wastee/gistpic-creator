//! 字幕检测样例测试：用 `示例_*` 目录里的真实截图验证检测结果。
//!
//! 期望值来自对这些图片实测的检测框（原图 3840×2160 坐标），允许 ±20 像素误差。

use std::path::PathBuf;
use std::sync::OnceLock;

use video_screenshot::imaging::detect::SubtitleDetector;
use video_screenshot::model::SearchRegion;

struct SampleCase {
    /// 相对项目根目录的路径
    path: &'static str,
    /// 期望的第一行字幕框（y 范围）
    y0: f32,
    y1: f32,
    /// 期望的字幕行数下限
    min_lines: usize,
    /// 第二行的 y 范围（多行字幕时）
    second: Option<(f32, f32)>,
}

const CASES: &[SampleCase] = &[
    SampleCase {
        path: "示例_不同位置字幕图片/字幕在上方_粘贴的图像 (2).png",
        y0: 99.0,
        y1: 197.0,
        min_lines: 1,
        second: None,
    },
    SampleCase {
        path: "示例_不同位置字幕图片/字幕在上方_粘贴的图像 (3).png",
        y0: 98.0,
        y1: 196.0,
        min_lines: 1,
        second: None,
    },
    SampleCase {
        path: "示例_不同位置字幕图片/字幕在下方_粘贴的图像.png",
        y0: 1912.0,
        y1: 2077.0,
        min_lines: 1,
        second: None,
    },
    SampleCase {
        path: "示例_不同位置字幕图片/字幕在下方_粘贴的图像 (4).png",
        y0: 1957.0,
        y1: 2054.0,
        min_lines: 1,
        second: None,
    },
    SampleCase {
        path: "示例_单行字幕图片/粘贴的图像.png",
        y0: 1950.0,
        y1: 2118.0,
        min_lines: 1,
        second: None,
    },
    SampleCase {
        path: "示例_单行字幕图片/粘贴的图像 (2).png",
        y0: 1959.0,
        y1: 2111.0,
        min_lines: 1,
        second: None,
    },
    SampleCase {
        path: "示例_多行字幕图片/粘贴的图像.png",
        y0: 1708.0,
        y1: 1816.0,
        min_lines: 2,
        second: Some((1857.0, 1956.0)),
    },
    SampleCase {
        path: "示例_多行字幕图片/粘贴的图像 (2).png",
        y0: 1708.0,
        y1: 1814.0,
        min_lines: 2,
        second: Some((1856.0, 1959.0)),
    },
    SampleCase {
        path: "示例_多行字幕图片/粘贴的图像 (3).png",
        y0: 1708.0,
        y1: 1814.0,
        min_lines: 2,
        second: Some((1850.0, 1957.0)),
    },
];

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn detector() -> &'static SubtitleDetector {
    static DETECTOR: OnceLock<SubtitleDetector> = OnceLock::new();
    DETECTOR.get_or_init(|| SubtitleDetector::new_embedded().expect("内嵌检测模型应能加载"))
}

fn load_rgb(relative: &str) -> image::RgbImage {
    let path = project_root().join(relative);
    assert!(
        path.exists(),
        "示例图片不存在: {}（项目根目录应为 {}）",
        path.display(),
        project_root().display()
    );
    image::open(&path)
        .unwrap_or_else(|err| panic!("读取 {} 失败: {err}", path.display()))
        .to_rgb8()
}

fn close(actual: f32, expected: f32) -> bool {
    (actual - expected).abs() <= 20.0
}

#[test]
fn detects_subtitle_in_all_samples() {
    let mut failures: Vec<String> = Vec::new();

    for case in CASES {
        let rgb = load_rgb(case.path);
        let detected = detector()
            .detect(&rgb, SearchRegion::Auto)
            .unwrap_or_else(|err| panic!("检测 {} 失败: {err}", case.path));

        let block = match detected {
            Some(block) => block,
            None => {
                failures.push(format!("{}: 没有检测到字幕", case.path));
                continue;
            }
        };

        let first = block.lines[0];
        if !close(first.y0, case.y0) || !close(first.y1, case.y1) {
            failures.push(format!(
                "{}: 第一行 y={:.0}..{:.0}，期望 {:.0}..{:.0}",
                case.path, first.y0, first.y1, case.y0, case.y1
            ));
        }

        if block.lines.len() < case.min_lines {
            failures.push(format!(
                "{}: 只找到 {} 行，期望至少 {} 行",
                case.path,
                block.lines.len(),
                case.min_lines
            ));
        }

        if let Some((second_y0, second_y1)) = case.second {
            match block.lines.get(1) {
                Some(second) => {
                    if !close(second.y0, second_y0) || !close(second.y1, second_y1) {
                        failures.push(format!(
                            "{}: 第二行 y={:.0}..{:.0}，期望 {:.0}..{:.0}",
                            case.path, second.y0, second.y1, second_y0, second_y1
                        ));
                    }
                }
                None => failures.push(format!("{}: 缺少第二行", case.path)),
            }
        }
    }

    assert!(failures.is_empty(), "检测不符合期望:\n{}", failures.join("\n"));
}

#[test]
fn top_region_finds_top_subtitles() {
    for (path, y0, y1) in [
        (
            "示例_不同位置字幕图片/字幕在上方_粘贴的图像 (2).png",
            99.0,
            197.0,
        ),
        (
            "示例_不同位置字幕图片/字幕在上方_粘贴的图像 (3).png",
            98.0,
            196.0,
        ),
    ] {
        let rgb = load_rgb(path);
        let block = detector()
            .detect(&rgb, SearchRegion::Top)
            .unwrap_or_else(|err| panic!("检测 {path} 失败: {err}"))
            .unwrap_or_else(|| panic!("{path}: 仅顶部模式没有检测到字幕"));
        let first = block.lines[0];
        assert!(
            close(first.y0, y0) && close(first.y1, y1),
            "{path}: 仅顶部模式得到 y={:.0}..{:.0}，期望 {y0}..{y1}",
            first.y0,
            first.y1
        );
    }
}

#[test]
fn bottom_region_finds_bottom_subtitles() {
    for (path, y0, y1) in [
        (
            "示例_不同位置字幕图片/字幕在下方_粘贴的图像.png",
            1912.0,
            2077.0,
        ),
        (
            "示例_不同位置字幕图片/字幕在下方_粘贴的图像 (4).png",
            1957.0,
            2054.0,
        ),
        ("示例_单行字幕图片/粘贴的图像.png", 1950.0, 2118.0),
    ] {
        let rgb = load_rgb(path);
        let block = detector()
            .detect(&rgb, SearchRegion::Bottom)
            .unwrap_or_else(|err| panic!("检测 {path} 失败: {err}"))
            .unwrap_or_else(|| panic!("{path}: 仅底部模式没有检测到字幕"));
        let first = block.lines[0];
        assert!(
            close(first.y0, y0) && close(first.y1, y1),
            "{path}: 仅底部模式得到 y={:.0}..{:.0}，期望 {y0}..{y1}",
            first.y0,
            first.y1
        );
    }
}

#[test]
fn blank_image_has_no_subtitle() {
    let rgb = image::RgbImage::from_pixel(1280, 720, image::Rgb([32, 32, 32]));
    let block = detector()
        .detect(&rgb, SearchRegion::Auto)
        .expect("空白图检测不应报错");
    assert!(block.is_none(), "空白图不应该检测出字幕");
}
