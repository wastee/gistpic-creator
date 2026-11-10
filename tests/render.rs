//! 合成与导出相关函数的测试，用纯色小图验证尺寸、顺序、缝隙颜色。

use image::{Rgb, RgbImage};

use gistpic_creator::imaging::crop::CropBand;
use gistpic_creator::imaging::layout::arrange;
use gistpic_creator::imaging::render::{
    compose_layout, compose_subtitle, crop_band_rgb, encode_png, preview_image,
};
use gistpic_creator::model::{GapSetting, LayoutTarget, Rgba};

const RED: [u8; 3] = [220, 30, 30];
const BLUE: [u8; 3] = [30, 30, 220];

fn solid(width: u32, height: u32, color: [u8; 3]) -> RgbImage {
    RgbImage::from_pixel(width, height, Rgb(color))
}

fn full_band(image: &RgbImage) -> CropBand {
    CropBand {
        y0: 0,
        y1: image.height(),
    }
}

fn pixel(image: &image::RgbaImage, x: u32, y: u32) -> [u8; 3] {
    let value = image.get_pixel(x, y);
    [value[0], value[1], value[2]]
}

fn close(actual: [u8; 3], expected: [u8; 3], tolerance: u8) -> bool {
    actual
        .iter()
        .zip(expected.iter())
        .all(|(a, e)| (*a as i32 - *e as i32).abs() <= tolerance as i32)
}

#[test]
fn crop_band_keeps_only_requested_rows() {
    let mut image = RgbImage::from_pixel(10, 100, Rgb(BLUE));
    for y in 0..10 {
        for x in 0..10 {
            image.put_pixel(x, y, Rgb(RED));
        }
    }
    let cropped = crop_band_rgb(&image, CropBand { y0: 10, y1: 100 });
    assert_eq!(cropped.dimensions(), (10, 90));
    assert_eq!(cropped.get_pixel(5, 0), &Rgb(BLUE));
}

#[test]
fn subtitle_stack_scales_to_widest_and_stacks() {
    let top = solid(100, 80, RED);
    let bottom = solid(200, 40, BLUE);
    let parts = [
        (top.clone(), full_band(&top)),
        (bottom.clone(), full_band(&bottom)),
    ];
    let refs: Vec<(&RgbImage, CropBand)> =
        parts.iter().map(|(image, band)| (image, *band)).collect();

    let result = compose_subtitle(&refs, &GapSetting::default());
    assert_eq!(result.width(), 200, "输出宽度应取最宽的图片");
    // 第一张 100x80 放大到 200 宽后高 160，第二张 200x40，合计 200
    assert_eq!(result.height(), 200);
    assert!(
        close(pixel(&result, 10, 10), RED, 6),
        "上半部分应是第一张图"
    );
    assert!(
        close(pixel(&result, 10, 190), BLUE, 6),
        "下半部分应是第二张图"
    );
}

#[test]
fn subtitle_stack_gap_uses_selected_color() {
    let top = solid(100, 40, RED);
    let bottom = solid(100, 40, BLUE);
    let parts = [
        (top.clone(), full_band(&top)),
        (bottom.clone(), full_band(&bottom)),
    ];
    let refs: Vec<(&RgbImage, CropBand)> =
        parts.iter().map(|(image, band)| (image, *band)).collect();

    let gap = GapSetting {
        enabled: true,
        px: 5,
        color: Rgba::WHITE,
    };
    let result = compose_subtitle(&refs, &gap);
    assert_eq!(result.height(), 40 + 5 + 40);
    assert!(
        close(pixel(&result, 10, 42), [255, 255, 255], 2),
        "缝隙应填白色"
    );
}

#[test]
fn crop_band_is_applied_before_stacking() {
    let top = solid(100, 100, RED);
    let bottom = solid(100, 100, BLUE);
    let parts = [
        (top.clone(), CropBand { y0: 50, y1: 100 }),
        (bottom.clone(), CropBand { y0: 0, y1: 50 }),
    ];
    let refs: Vec<(&RgbImage, CropBand)> =
        parts.iter().map(|(image, band)| (image, *band)).collect();

    let result = compose_subtitle(&refs, &GapSetting::default());
    assert_eq!(result.height(), 100, "两张各裁一半，合计高度 100");
}

#[test]
fn layout_places_images_and_fills_gap_with_background() {
    let left = solid(100, 100, RED);
    let right = solid(100, 100, BLUE);
    let sizes = [(100u32, 100u32), (100, 100)];
    let layout = arrange(&sizes, LayoutTarget::Row, 10);
    assert_eq!(layout.canvas_w, 210);

    let images = [&left, &right];
    let result = compose_layout(&images, &layout, Rgba::BLACK);
    assert_eq!(result.dimensions(), (210, 100));
    assert!(close(pixel(&result, 5, 50), RED, 4));
    assert!(
        close(pixel(&result, 105, 50), [0, 0, 0], 4),
        "缝隙应是背景色"
    );
    assert!(close(pixel(&result, 205, 50), BLUE, 4));
}

#[test]
fn png_encoding_round_trips() {
    let image = solid(37, 19, RED);
    let parts = [(image.clone(), full_band(&image))];
    let refs: Vec<(&RgbImage, CropBand)> =
        parts.iter().map(|(image, band)| (image, *band)).collect();
    let composed = compose_subtitle(&refs, &GapSetting::default());

    let bytes = encode_png(&composed).expect("PNG 编码应成功");
    let decoded = image::load_from_memory(&bytes)
        .expect("PNG 解码应成功")
        .to_rgba8();
    assert_eq!(decoded.dimensions(), (37, 19));
    assert!(close(pixel(&decoded, 10, 10), RED, 2));
}

#[test]
fn preview_image_keeps_aspect_ratio() {
    let image = solid(400, 300, RED);
    let preview = preview_image(&image, full_band(&image), 100);
    let size = preview.size(0);
    assert_eq!(u32::from(size.width), 100);
    assert_eq!(u32::from(size.height), 75);
}
