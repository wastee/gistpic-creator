//! 端到端流程测试：真实截图 → 检测字幕 → 推导裁剪带 → 合成。
//!
//! 用缩小后的图做合成，避免测试占用太多内存；裁剪带坐标仍然来自原图检测结果。

use std::path::PathBuf;
use std::sync::OnceLock;

use video_screenshot::imaging::crop::{self, Inclusion};
use video_screenshot::imaging::detect::SubtitleDetector;
use video_screenshot::imaging::render;
use video_screenshot::model::{
    Item, ItemStatus, LayoutTarget, Mode, Project, SearchRegion,
};

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn detector() -> &'static SubtitleDetector {
    static DETECTOR: OnceLock<SubtitleDetector> = OnceLock::new();
    DETECTOR.get_or_init(|| SubtitleDetector::new_embedded().expect("检测模型应加载成功"))
}

fn load_item(id: u64, relative: &str) -> Item {
    let path = project_root().join(relative);
    let rgb = image::open(&path)
        .unwrap_or_else(|err| panic!("读取 {} 失败: {err}", path.display()))
        .to_rgb8();
    let (width, height) = rgb.dimensions();
    let block = detector()
        .detect(&rgb, SearchRegion::Auto)
        .unwrap_or_else(|err| panic!("检测 {relative} 失败: {err}"))
        .unwrap_or_else(|| panic!("{relative} 没有检测到字幕"));

    Item {
        id,
        name: relative.to_string(),
        width,
        height,
        rgb: std::sync::Arc::new(rgb),
        block: Some(block),
        status: ItemStatus::Ready,
        keep_whole: false,
    }
}

#[test]
fn three_real_screenshots_stitch_into_one_long_image() {
    let mut project = Project::default();
    // 只用同一个示例文件夹里的图片，避免示例混用不同文件夹。
    project.items.push(load_item(
        1,
        "示例_多行字幕图片/粘贴的图像.png",
    ));
    project.items.push(load_item(
        2,
        "示例_多行字幕图片/粘贴的图像 (2).png",
    ));
    project.items.push(load_item(
        3,
        "示例_多行字幕图片/粘贴的图像 (3).png",
    ));

    let inclusions = crop::resolve(&project);
    assert_eq!(inclusions.len(), 3);

    // 第一张保留画面主体，从画面顶部开始
    match inclusions[0] {
        Inclusion::Crop(band) => {
            assert_eq!(band.y0, 0, "第一张应从画面顶部开始");
            assert!(band.height() > 1500, "第一张应保留大片画面，实际高度 {}", band.height());
        }
        other => panic!("第一张应为裁剪，实际 {other:?}"),
    }

    // 第二、三张只留字幕带
    for (index, inclusion) in inclusions.iter().enumerate().skip(1) {
        match inclusion {
            Inclusion::Crop(band) => {
                assert!(band.y0 > 0, "第 {} 张应只留字幕带", index + 1);
                assert!(
                    band.height() < 400,
                    "第 {} 张裁剪后高度 {} 应只包含字幕",
                    index + 1,
                    band.height()
                );
            }
            other => panic!("第 {} 张应为裁剪，实际 {other:?}", index + 1),
        }
    }

    // 把每张图按裁剪带裁好、缩到同一宽度后合成
    let mut parts = Vec::new();
    for (index, item) in project.items.iter().enumerate() {
        let band = match inclusions[index] {
            Inclusion::Crop(band) => band,
            _ => continue,
        };
        let cropped = render::crop_band_rgb(&item.rgb, band);
        let width = 320u32;
        let height = ((cropped.height() as u64 * width as u64) / cropped.width() as u64)
            .max(1) as u32;
        let resized = image::imageops::resize(
            &cropped,
            width,
            height,
            image::imageops::FilterType::Triangle,
        );
        parts.push((resized, video_screenshot::imaging::crop::CropBand { y0: 0, y1: height }));
    }
    assert_eq!(parts.len(), 3);

    let refs: Vec<(&image::RgbImage, video_screenshot::imaging::crop::CropBand)> = parts
        .iter()
        .map(|(image, band)| (image, *band))
        .collect();
    let stitched = render::compose_subtitle(&refs, &Default::default());

    assert_eq!(stitched.width(), 320);
    let expected_height: u32 = parts.iter().map(|(image, _)| image.height()).sum();
    assert_eq!(stitched.height(), expected_height);
    // 第一张占大头，后两张各一小条
    assert!(
        stitched.height() > 150,
        "拼接结果高度 {} 应主要来自第一张保留的画面",
        stitched.height()
    );
}

#[test]
fn two_landscape_and_one_portrait_arrange_as_two_columns() {
    // 两张横图 + 一张竖图：竖图用真实截图裁一段来模拟
    let landscape = project_root().join("示例_单行字幕图片/粘贴的图像.png");
    let rgb = image::open(&landscape).expect("读取示例图失败").to_rgb8();
    let portrait = image::imageops::crop_imm(&rgb, 1200, 0, 1215, 2160).to_image();

    let sizes = [(3840u32, 2160u32), (3840, 2160), (portrait.width(), portrait.height())];
    let layout = video_screenshot::imaging::layout::arrange(&sizes, LayoutTarget::Aspect(16.0 / 9.0), 0);
    assert_eq!(layout.placements.len(), 3);
    // 两张横图排成一列（上下叠放），竖图单独一列，画布紧贴内容
    assert_eq!(
        layout.placements[0].x, layout.placements[1].x,
        "两张横图应在同一列"
    );
    assert_ne!(
        layout.placements[0].x, layout.placements[2].x,
        "竖图应在另一列"
    );
    assert!(
        (layout.aspect() - 1.4515).abs() < 0.03,
        "两横一竖贴合内容后的比例应约 1.45，实际 {:.3}",
        layout.aspect()
    );
    // 每张图仍留在画布内
    for placement in &layout.placements {
        assert!(placement.x + placement.w <= layout.canvas_w);
        assert!(placement.y + placement.h <= layout.canvas_h);
    }
    // 每张图的比例不能被改变
    for placement in &layout.placements {
        let source = sizes[placement.item];
        let source_aspect = source.0 as f32 / source.1 as f32;
        let placed_aspect = placement.w as f32 / placement.h as f32;
        assert!(
            (placed_aspect - source_aspect).abs() / source_aspect < 0.02,
            "第 {} 张比例被改变",
            placement.item
        );
    }
}

#[test]
fn switching_mode_does_not_change_detection_results() {
    let mut project = Project::default();
    project.items.push(load_item(
        1,
        "示例_不同位置字幕图片/字幕在上方_粘贴的图像 (2).png",
    ));

    // 字幕在上方：第一张要从字幕上沿一直保留到画面底部
    let subtitle_mode = crop::resolve(&project);
    match subtitle_mode[0] {
        Inclusion::Crop(band) => {
            assert!(band.y0 < 200, "字幕在上方时，保留部分应从上方字幕开始");
            assert_eq!(band.y1, 2160, "并且一直保留到画面底部");
        }
        other => panic!("应为裁剪，实际 {other:?}"),
    }

    // 切到排版模式：不裁剪
    let project = Project {
        mode: Mode::Layout,
        settings: project.settings.clone(),
        items: project.items.clone(),
    };
    let layout_mode = crop::resolve(&project);
    assert_eq!(layout_mode[0], Inclusion::FullImage);
}
