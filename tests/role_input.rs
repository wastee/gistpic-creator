//! 角色分类器输入准备测试：裁剪、缩放、几何特征。

use gistpic_creator::imaging::role_input::{
    ROLE_GEOMETRY_LEN, ROLE_INPUT_HEIGHT, ROLE_INPUT_WIDTH, prepare_candidate,
};
use gistpic_creator::model::{LineBox, SubtitleBlock};
use image::{Rgb, RgbImage};

fn block(x0: f32, y0: f32, x1: f32, y1: f32) -> SubtitleBlock {
    SubtitleBlock {
        lines: vec![LineBox { x0, y0, x1, y1 }],
    }
}

#[test]
fn prepares_fixed_size_crop_and_normalized_geometry() {
    let image = RgbImage::from_pixel(3840, 2160, Rgb([80, 90, 100]));
    let input = prepare_candidate(&image, &block(1027.0, 1912.0, 2814.0, 2077.0))
        .expect("字幕块应能生成分类器输入");

    assert_eq!(
        input.image.dimensions(),
        (ROLE_INPUT_WIDTH, ROLE_INPUT_HEIGHT)
    );
    assert_eq!(input.geometry.len(), ROLE_GEOMETRY_LEN);
    assert!((input.geometry[0] - 0.50013).abs() < 0.001);
    assert!(input.geometry[1] > 0.8);
    assert!(input.geometry[2] > 0.4);
    assert!(input.geometry[3] > 0.0);
    assert!((input.geometry[4] - 0.25).abs() < 0.001);
}

#[test]
fn clamps_crop_at_image_edges() {
    let image = RgbImage::from_pixel(320, 180, Rgb([20, 20, 20]));
    let input =
        prepare_candidate(&image, &block(0.0, 0.0, 100.0, 40.0)).expect("贴边文字块应能生成输入");
    assert_eq!(
        input.image.dimensions(),
        (ROLE_INPUT_WIDTH, ROLE_INPUT_HEIGHT)
    );
    assert!((input.geometry[0] - 100.0 / 320.0 / 2.0).abs() < 1e-5);
    assert!((input.geometry[6] - 0.0).abs() < 1e-5);
}

#[test]
fn rejects_empty_blocks() {
    let image = RgbImage::new(320, 180);
    assert!(prepare_candidate(&image, &SubtitleBlock::default()).is_none());
}
