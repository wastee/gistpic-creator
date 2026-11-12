//! 裁剪规则单元测试：出血位、第一张保留画面、无字幕跳过。

use std::sync::Arc;

use gistpic_creator::imaging::crop::{
    CropBand, Inclusion, band_for_block, manual_band_for_item, resolve, subject_band_for_block,
};
use gistpic_creator::model::{
    Item, ItemStatus, LineBox, ManualBand, Mode, Project, Settings, SubtitleBlock,
};
use image::RgbImage;

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
        manual: None,
    }
}

#[test]
fn band_adds_ten_percent_bleed() {
    let subtitle = block(vec![line(1027.0, 1912.0, 2814.0, 2077.0)]);
    let band = band_for_block(&subtitle, false, 0.10, (3840, IMAGE_HEIGHT)).expect("应得到裁剪带");
    // 1912 - 165*0.1 = 1895.5 -> 1895；2077 + 165*0.1 = 2093.5 -> 2094
    assert_eq!(band, CropBand::rows(3840, 1895, 2094));
}

#[test]
fn keep_second_line_extends_band() {
    let subtitle = block(vec![
        line(1410.0, 1708.0, 2366.0, 1816.0),
        line(1206.0, 1857.0, 2636.0, 1956.0),
    ]);

    let only_first = band_for_block(&subtitle, false, 0.10, (3840, IMAGE_HEIGHT)).unwrap();
    assert_eq!(only_first.y0, 1697);
    assert_eq!(only_first.y1, 1827); // 1816 + 10.8 = 1826.8 -> 1827

    let with_second = band_for_block(&subtitle, true, 0.10, (3840, IMAGE_HEIGHT)).unwrap();
    assert_eq!(with_second.y0, 1697);
    assert_eq!(with_second.y1, 1966); // 1956 + 9.9 = 1965.9 -> 1966
    assert!(with_second.y1 > only_first.y1);
}

#[test]
fn subject_band_keeps_scene_above_bottom_subtitle() {
    let subtitle = block(vec![line(1027.0, 1912.0, 2814.0, 2077.0)]);
    let band = subject_band_for_block(&subtitle, false, 0.10, (3840, IMAGE_HEIGHT)).unwrap();
    assert_eq!(band.y0, 0, "底部字幕时第一张要从画面顶部开始保留");
    assert_eq!(band.y1, 2094);
}

#[test]
fn subject_band_keeps_scene_below_top_subtitle() {
    let subtitle = block(vec![line(1662.0, 99.0, 2192.0, 197.0)]);
    let band = subject_band_for_block(&subtitle, false, 0.10, (3840, IMAGE_HEIGHT)).unwrap();
    assert_eq!(band.y0, 89); // 99 - 9.9 = 89.1 -> 89
    assert_eq!(band.y1, IMAGE_HEIGHT, "顶部字幕时第一张要保留到画面底部");
}

#[test]
fn band_is_clamped_to_image() {
    let subtitle = block(vec![line(0.0, 2.0, 3840.0, 30.0)]);
    let band = band_for_block(&subtitle, false, 0.30, (3840, IMAGE_HEIGHT)).unwrap();
    assert_eq!(band.y0, 0);
    let subtitle = block(vec![line(0.0, 2140.0, 3840.0, 2159.0)]);
    let band = band_for_block(&subtitle, false, 0.30, (3840, IMAGE_HEIGHT)).unwrap();
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
    let band = band_for_block(
        &subtitle,
        false,
        Settings::default().bleed,
        (3840, IMAGE_HEIGHT),
    )
    .expect("应得到裁剪带");
    assert_eq!(band.y0, 1870);
    assert_eq!(band.y1, 2119);
}

#[test]
fn failed_detection_keeps_whole_image() {
    let mut project = Project::default();
    let mut failed = item(1, None, true);
    failed.status = ItemStatus::Failed("检测失败".to_string());
    project.items.push(failed);

    assert_eq!(
        resolve(&project)[0],
        Inclusion::FullImage,
        "识别失败时应保留整图，不丢失用户图片"
    );

    project.items[0].keep_whole = false;
    assert_eq!(resolve(&project)[0], Inclusion::Skipped);
}

#[test]
fn manual_band_overrides_first_image_subject_rule() {
    let subtitle = block(vec![line(1027.0, 1912.0, 2814.0, 2077.0)]);
    let mut project = Project::default();
    let mut first = item(1, Some(subtitle), false);
    first.manual = ManualBand::clamped(0.0, 1.0, 0.50, 0.75);
    project.items.push(first);

    assert_eq!(
        resolve(&project)[0],
        Inclusion::Crop(CropBand::rows(3840, 1080, 1620)),
        "手动框选必须覆盖第一张保留主体的自动规则"
    );
}

#[test]
fn manual_band_includes_no_subtitle_item() {
    let mut project = Project::default();
    let mut without_subtitle = item(1, None, false);
    without_subtitle.status = ItemStatus::NoSubtitle;
    without_subtitle.manual = ManualBand::clamped(0.0, 1.0, 0.10, 0.40);
    project.items.push(without_subtitle);

    assert_eq!(
        resolve(&project)[0],
        Inclusion::Crop(CropBand::rows(3840, 216, 864)),
        "没有自动字幕时，手动框选仍应让这张图参与输出"
    );
}

#[test]
fn manual_band_is_ignored_when_crop_is_off() {
    let mut project = Project {
        settings: Settings {
            crop: false,
            ..Settings::default()
        },
        ..Project::default()
    };
    let mut first = item(1, None, true);
    first.status = ItemStatus::NoSubtitle;
    first.manual = ManualBand::clamped(0.0, 1.0, 0.10, 0.40);
    project.items.push(first);

    assert_eq!(resolve(&project)[0], Inclusion::FullImage);
}

#[test]
fn manual_band_uses_same_relative_position_for_different_heights() {
    let tall = item(1, None, false);
    let mut short = item(2, None, false);
    short.height = 1080;
    let manual = ManualBand::clamped(0.0, 1.0, 0.25, 0.75).expect("有效保留带");

    let tall_band = manual_band_for_item(&tall, manual).unwrap();
    let short_band = manual_band_for_item(&short, manual).unwrap();
    assert_eq!(tall_band, CropBand::rows(3840, 540, 1620));
    assert_eq!(short_band, CropBand::rows(3840, 270, 810));
}

#[test]
fn layout_mode_honors_manual_band() {
    // 排版模式也遵守手动框选：用户在画布上拉出的范围，导出时必须照做。
    let mut project = Project {
        mode: Mode::Layout,
        ..Project::default()
    };
    let mut first = item(1, None, true);
    first.status = ItemStatus::NoSubtitle;
    first.manual = ManualBand::clamped(0.25, 0.75, 0.10, 0.40);
    project.items.push(first);

    assert_eq!(
        resolve(&project)[0],
        Inclusion::Crop(CropBand {
            x0: 960,
            x1: 2880,
            y0: 216,
            y1: 864,
        }),
        "排版模式下手动框选应同时裁掉左右和上下"
    );
}

#[test]
fn layout_mode_without_manual_band_keeps_whole_image() {
    let mut project = Project {
        mode: Mode::Layout,
        ..Project::default()
    };
    project.items.push(item(1, None, true));
    assert_eq!(resolve(&project)[0], Inclusion::FullImage);
}
