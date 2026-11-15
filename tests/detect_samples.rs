//! 字幕检测样例测试：用 `示例图片/示例_*` 目录里的真实截图验证检测结果。
//!
//! 期望值来自对这些图片实测的检测框（原图 3840×2160 坐标），允许 ±20 像素误差。

use std::path::PathBuf;
use std::sync::OnceLock;

use gistpic_creator::imaging::detect::SubtitleDetector;

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
        path: "示例图片/示例_不同位置字幕图片/字幕在上方_粘贴的图像 (2).png",
        y0: 99.0,
        y1: 197.0,
        min_lines: 1,
        second: None,
    },
    SampleCase {
        path: "示例图片/示例_不同位置字幕图片/字幕在上方_粘贴的图像 (3).png",
        y0: 98.0,
        y1: 196.0,
        min_lines: 1,
        second: None,
    },
    SampleCase {
        path: "示例图片/示例_不同位置字幕图片/字幕在下方_粘贴的图像.png",
        y0: 1912.0,
        y1: 2077.0,
        min_lines: 1,
        second: None,
    },
    SampleCase {
        path: "示例图片/示例_不同位置字幕图片/字幕在下方_粘贴的图像 (4).png",
        y0: 1957.0,
        y1: 2054.0,
        min_lines: 1,
        second: None,
    },
    SampleCase {
        path: "示例图片/示例_单行字幕图片/粘贴的图像.png",
        y0: 1950.0,
        y1: 2118.0,
        min_lines: 1,
        second: None,
    },
    SampleCase {
        path: "示例图片/示例_单行字幕图片/粘贴的图像 (2).png",
        y0: 1959.0,
        y1: 2111.0,
        min_lines: 1,
        second: None,
    },
    SampleCase {
        path: "示例图片/示例_多行字幕图片/粘贴的图像.png",
        y0: 1708.0,
        y1: 1816.0,
        min_lines: 2,
        second: Some((1857.0, 1956.0)),
    },
    SampleCase {
        path: "示例图片/示例_多行字幕图片/粘贴的图像 (2).png",
        y0: 1708.0,
        y1: 1814.0,
        min_lines: 2,
        second: Some((1856.0, 1959.0)),
    },
    SampleCase {
        path: "示例图片/示例_多行字幕图片/粘贴的图像 (3).png",
        y0: 1708.0,
        y1: 1814.0,
        min_lines: 2,
        second: Some((1850.0, 1957.0)),
    },
    SampleCase {
        path: "示例图片/问题B示例_不能正确识别字幕/粘贴的图像.png",
        y0: 1810.0,
        y1: 1944.0,
        min_lines: 1,
        second: None,
    },
    SampleCase {
        path: "示例图片/问题B示例_不能正确识别字幕/粘贴的图像 (2).png",
        y0: 1815.0,
        y1: 1951.0,
        min_lines: 1,
        second: None,
    },
    SampleCase {
        path: "示例图片/问题B示例_不能正确识别字幕/粘贴的图像 (3).png",
        y0: 1808.0,
        y1: 1946.0,
        min_lines: 1,
        second: None,
    },
    SampleCase {
        path: "示例图片/问题B示例_不能正确识别字幕/粘贴的图像 (4).png",
        y0: 1814.0,
        y1: 1946.0,
        min_lines: 1,
        second: None,
    },
    SampleCase {
        path: "示例图片/问题B示例_不能正确识别字幕/粘贴的图像 (5).png",
        y0: 1792.0,
        y1: 1950.0,
        min_lines: 1,
        second: None,
    },
    SampleCase {
        path: "示例图片/问题B示例_不能正确识别字幕/粘贴的图像 (6).png",
        y0: 1802.0,
        y1: 1953.0,
        min_lines: 1,
        second: None,
    },
];

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn detector() -> &'static SubtitleDetector {
    static DETECTOR: OnceLock<SubtitleDetector> = OnceLock::new();
    DETECTOR.get_or_init(|| SubtitleDetector::new_embedded().expect("内嵌检测模型应能加载"))
}

/// 读一张示例图。
///
/// `示例图片/` 不进仓库（体积大），只有本地放了素材才能跑这些
/// 断言。拿不到就返回 `None`，调用方跳过这一轮，不把「没有素材」当成检测失败。
fn load_rgb(relative: &str) -> Option<image::RgbImage> {
    let path = project_root().join(relative);
    if !path.is_file() {
        return None;
    }
    Some(
        image::open(&path)
            .unwrap_or_else(|err| panic!("读取 {} 失败: {err}", path.display()))
            .to_rgb8(),
    )
}

fn close(actual: f32, expected: f32) -> bool {
    (actual - expected).abs() <= 20.0
}

#[test]
fn detects_subtitle_in_all_samples() {
    let mut failures: Vec<String> = Vec::new();

    for case in CASES {
        let Some(rgb) = load_rgb(case.path) else {
            continue;
        };
        let detected = detector()
            .detect(&rgb)
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

    assert!(
        failures.is_empty(),
        "检测不符合期望:\n{}",
        failures.join("\n")
    );
}

#[test]
fn finds_top_subtitles_in_whole_image() {
    for (path, y0, y1) in [
        (
            "示例图片/示例_不同位置字幕图片/字幕在上方_粘贴的图像 (2).png",
            99.0,
            197.0,
        ),
        (
            "示例图片/示例_不同位置字幕图片/字幕在上方_粘贴的图像 (3).png",
            98.0,
            196.0,
        ),
    ] {
        let Some(rgb) = load_rgb(path) else {
            continue;
        };
        let block = detector()
            .detect(&rgb)
            .unwrap_or_else(|err| panic!("检测 {path} 失败: {err}"))
            .unwrap_or_else(|| panic!("{path}: 全图检测没有检测到字幕"));
        let first = block.lines[0];
        assert!(
            close(first.y0, y0) && close(first.y1, y1),
            "{path}: 全图检测得到 y={:.0}..{:.0}，期望 {y0}..{y1}",
            first.y0,
            first.y1
        );
    }
}

#[test]
fn finds_bottom_subtitles_in_whole_image() {
    for (path, y0, y1) in [
        (
            "示例图片/示例_不同位置字幕图片/字幕在下方_粘贴的图像.png",
            1912.0,
            2077.0,
        ),
        (
            "示例图片/示例_不同位置字幕图片/字幕在下方_粘贴的图像 (4).png",
            1957.0,
            2054.0,
        ),
        ("示例图片/示例_单行字幕图片/粘贴的图像.png", 1950.0, 2118.0),
    ] {
        let Some(rgb) = load_rgb(path) else {
            continue;
        };
        let block = detector()
            .detect(&rgb)
            .unwrap_or_else(|err| panic!("检测 {path} 失败: {err}"))
            .unwrap_or_else(|| panic!("{path}: 全图检测没有检测到字幕"));
        let first = block.lines[0];
        assert!(
            close(first.y0, y0) && close(first.y1, y1),
            "{path}: 全图检测得到 y={:.0}..{:.0}，期望 {y0}..{y1}",
            first.y0,
            first.y1
        );
    }
}

#[test]
fn blank_image_has_no_subtitle() {
    let rgb = image::RgbImage::from_pixel(1280, 720, image::Rgb([32, 32, 32]));
    let block = detector().detect(&rgb).expect("空白图检测不应报错");
    assert!(block.is_none(), "空白图不应该检测出字幕");
}

#[test]
fn problem_b_caption_is_not_grouped_into_the_subtitle() {
    // 问题 B：字幕上方有一条说明条，六张图都不能把它并成字幕第二行。
    // min_lines 只有下限，这里补上"恰好一行"的上限约束。
    for path in [
        "示例图片/问题B示例_不能正确识别字幕/粘贴的图像.png",
        "示例图片/问题B示例_不能正确识别字幕/粘贴的图像 (2).png",
        "示例图片/问题B示例_不能正确识别字幕/粘贴的图像 (3).png",
        "示例图片/问题B示例_不能正确识别字幕/粘贴的图像 (4).png",
        "示例图片/问题B示例_不能正确识别字幕/粘贴的图像 (5).png",
        "示例图片/问题B示例_不能正确识别字幕/粘贴的图像 (6).png",
    ] {
        let Some(rgb) = load_rgb(path) else {
            continue;
        };
        let block = detector()
            .detect(&rgb)
            .unwrap_or_else(|err| panic!("检测 {path} 失败: {err}"))
            .unwrap_or_else(|| panic!("{path}: 全图检测应找到字幕"));
        assert_eq!(
            block.lines.len(),
            1,
            "{path}: 说明条不应被并成字幕第二行，实际行数 {}，框 {:?}",
            block.lines.len(),
            block.lines
        );
    }
}

#[test]
fn detects_clipped_bottom_subtitles_from_problem_a() {
    let path = "示例图片/问题A示例_不能正确识别字幕/粘贴的图像 (2).png";
    let Some(rgb) = load_rgb(path) else {
        return;
    };
    let block = detector()
        .detect(&rgb)
        .unwrap_or_else(|err| panic!("检测 {path} 失败: {err}"))
        .unwrap_or_else(|| panic!("{path}: 全图检测应找到贴边字幕"));
    let first = block.lines[0];
    assert!(first.y0 > 1700.0, "{path}: 底部字幕位置异常: {first:?}");
    assert!(
        first.y1 <= 1950.0,
        "{path}: 字幕框应被夹紧到图片内: {first:?}"
    );
}
