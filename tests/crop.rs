//! 裁剪规则单元测试：出血位、第一张保留画面、无字幕跳过。

use std::sync::Arc;

use image::RgbImage;
use video_screenshot::imaging::crop::{
    band_for_block, resolve, subject_band_for_block, CropBand, Inclusion,
};
use video_screenshot::model::{
    Item, ItemStatus, LineBox, Mode, Project, Settings, SubtitleBlock,
};

const IMAGE_HEIGHT: u32 = 2160;

fn line(x0: f32, y0: f32, x1: f32, y1: f32) -> LineBox {
    LineBox { x0, y0, x1, y1 }
}

fn block(lines: Vec<LineBox>) -> SubtitleBlock {
    SubtitleBlock { lines }
}

fn item(id: u64, block: Option<SubtitleBlock>, keep_whole: bool) -> Item {
    Item {
        id,
        name: format!("test-{id}.png"),
        width: 3840,
        height: IMAGE_HEIGHT,
        rgb: Arc::new(RgbImage::new(8, IMAGE_HEIGHT)),
        block,
        status: ItemStatus::Ready,
        keep_whole,
    }
}

#[test]
fn band_adds_ten_percent_bleed() {
    let subtitle = block(vec![line(1027.0, 1912.0, 2814.0, 2077.0)]);
    let band = band_for_block(&subtitle, false, 0.10, IMAGE_HEIGHT).expect("应得到裁剪带");
    // 1912 - 165*0.1 = 1895.5 -> 1895；2077 + 165*0.1 = 2093.5 -> 2094
    assert_eq!(
        band,
        CropBand {
            y0: 1895,
            y1: 2094
        }
    );
}

#[test]
fn keep_second_line_extends_band() {
    let subtitle = block(vec![
        line(1410.0, 1708.0, 2366.0, 1816.0),
        line(1206.0, 1857.0, 2636.0, 1956.0),
    ]);

    let only_first = band_for_block(&subtitle, false, 0.10, IMAGE_HEIGHT).unwrap();
    assert_eq!(only_first.y0, 1697);
    assert_eq!(only_first.y1, 1827); // 1816 + 10.8 = 1826.8 -> 1827

    let with_second = band_for_block(&subtitle, true, 0.10, IMAGE_HEIGHT).unwrap();
    assert_eq!(with_second.y0, 1697);
    assert_eq!(with_second.y1, 1966); // 1956 + 9.9 = 1965.9 -> 1966
    assert!(with_second.y1 > only_first.y1);
}

#[test]
fn subject_band_keeps_scene_above_bottom_subtitle() {
    let subtitle = block(vec![line(1027.0, 1912.0, 2814.0, 2077.0)]);
    let band = subject_band_for_block(&subtitle, false, 0.10, IMAGE_HEIGHT).unwrap();
    assert_eq!(band.y0, 0, "底部字幕时第一张要从画面顶部开始保留");
    assert_eq!(band.y1, 2094);
}

#[test]
fn subject_band_keeps_scene_below_top_subtitle() {
    let subtitle = block(vec![line(1662.0, 99.0, 2192.0, 197.0)]);
    let band = subject_band_for_block(&subtitle, false, 0.10, IMAGE_HEIGHT).unwrap();
    assert_eq!(band.y0, 89); // 99 - 9.9 = 89.1 -> 89
    assert_eq!(band.y1, IMAGE_HEIGHT, "顶部字幕时第一张要保留到画面底部");
}

#[test]
fn band_is_clamped_to_image() {
    let subtitle = block(vec![line(0.0, 2.0, 3840.0, 30.0)]);
    let band = band_for_block(&subtitle, false, 0.30, IMAGE_HEIGHT).unwrap();
    assert_eq!(band.y0, 0);
    let subtitle = block(vec![line(0.0, 2140.0, 3840.0, 2159.0)]);
    let band = band_for_block(&subtitle, false, 0.30, IMAGE_HEIGHT).unwrap();
    assert_eq!(band.y1, IMAGE_HEIGHT);
}

#[test]
fn first_item_keeps_scene_others_keep_subtitle() {
    let subtitle = block(vec![line(1027.0, 1912.0, 2814.0, 2077.0)]);
    let mut project = Project::default();
    project.items.push(item(1, Some(subtitle.clone()), false));
    project.items.push(item(2, Some(subtitle), false));

    let inclusions = resolve(&project);
    match inclusions[0] {
        Inclusion::Crop(band) => {
            assert_eq!(band.y0, 0, "第一张要保留画面");
            // 默认出血位是字符高度的 25%：2077 + 165 * 0.25 = 2118.25 -> 2119
            assert_eq!(band.y1, 2119);
        }
        other => panic!("第一张应为裁剪，实际 {other:?}"),
    }
    match inclusions[1] {
        Inclusion::Crop(band) => {
            // 1912 - 165 * 0.25 = 1870.75 -> 1870
            assert_eq!(band.y0, 1870, "第二张只留字幕带");
            assert_eq!(band.y1, 2119);
        }
        other => panic!("第二张应为裁剪，实际 {other:?}"),
    }
    let _ = subtitle;
}

#[test]
fn crop_disabled_keeps_full_image() {
    let subtitle = block(vec![line(1027.0, 1912.0, 2814.0, 2077.0)]);
    let mut project = Project {
        settings: Settings {
            crop: false,
            ..Settings::default()
        },
        ..Project::default()
    };
    project.items.push(item(1, Some(subtitle), false));

    let inclusions = resolve(&project);
    assert_eq!(inclusions[0], Inclusion::FullImage);
}

#[test]
fn layout_mode_never_crops() {
    let subtitle = block(vec![line(1027.0, 1912.0, 2814.0, 2077.0)]);
    let mut project = Project {
        mode: Mode::Layout,
        ..Project::default()
    };
    project.items.push(item(1, Some(subtitle), false));

    let inclusions = resolve(&project);
    assert_eq!(inclusions[0], Inclusion::FullImage);
}

#[test]
fn no_subtitle_item_is_skipped_unless_kept() {
    let mut project = Project::default();
    let mut without_subtitle = item(1, None, false);
    without_subtitle.status = ItemStatus::NoSubtitle;
    project.items.push(without_subtitle);

    assert_eq!(resolve(&project)[0], Inclusion::Skipped);

    project.items[0].keep_whole = true;
    assert_eq!(resolve(&project)[0], Inclusion::FullImage);
}

#[test]
fn default_bleed_is_quarter_character_height() {
    assert_eq!(Settings::default().bleed, 0.25);

    let subtitle = block(vec![line(1027.0, 1912.0, 2814.0, 2077.0)]);
    let band = band_for_block(&subtitle, false, Settings::default().bleed, IMAGE_HEIGHT)
        .expect("应得到裁剪带");
    assert_eq!(band.y0, 1870);
    assert_eq!(band.y1, 2119);
}
