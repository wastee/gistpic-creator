//! 角色分类器的输入准备：裁剪候选文字块、缩放、几何特征。
//!
//! 训练数据导出和运行时推理必须使用同一套规则，否则模型看到的输入会不一致。

use image::imageops::FilterType;
use image::{Rgb, RgbImage};

use crate::model::SubtitleBlock;

/// 分类器输入的宽度。
pub const ROLE_INPUT_WIDTH: u32 = 96;
/// 分类器输入的高度。
pub const ROLE_INPUT_HEIGHT: u32 = 48;
/// 文字块左右扩展比例。
pub const ROLE_CONTEXT_X: f32 = 0.08;
/// 文字块上下扩展比例。
pub const ROLE_CONTEXT_Y: f32 = 0.50;
/// 缩放不足时填充的背景色。
pub const ROLE_PADDING: [u8; 3] = [32, 32, 32];
/// 几何特征数量。
pub const ROLE_GEOMETRY_LEN: usize = 8;

/// 一张候选文字块送给分类器的输入。
#[derive(Clone, Debug)]
pub struct CandidateInput {
    /// 已缩放并补边的 96×48 RGB 图像。
    pub image: RgbImage,
    /// 归一化几何特征，见 [`prepare_candidate`]。
    pub geometry: [f32; ROLE_GEOMETRY_LEN],
}

/// 把候选字幕块裁剪成分类器输入。
///
/// 几何特征顺序：中心 x、中心 y、宽度、高度、行数、最大行高、上边距、下边距。
/// 所有位置都以原图宽高归一化，范围夹在 0 到 1。
pub fn prepare_candidate(rgb: &RgbImage, block: &SubtitleBlock) -> Option<CandidateInput> {
    let (image_width, image_height) = rgb.dimensions();
    if image_width == 0 || image_height == 0 || block.lines.is_empty() {
        return None;
    }

    let bounds = block.bounds();
    let width = (bounds.x1 - bounds.x0).max(0.0);
    let height = (bounds.y1 - bounds.y0).max(0.0);
    if width <= 0.0 || height <= 0.0 {
        return None;
    }

    let pad_x = (width * ROLE_CONTEXT_X).max(1.0);
    let pad_y = (height * ROLE_CONTEXT_Y).max(1.0);
    let x0 = (bounds.x0 - pad_x).floor().max(0.0) as u32;
    let y0 = (bounds.y0 - pad_y).floor().max(0.0) as u32;
    let x1 = (bounds.x1 + pad_x).ceil().min(image_width as f32) as u32;
    let y1 = (bounds.y1 + pad_y).ceil().min(image_height as f32) as u32;
    if x1 <= x0 || y1 <= y0 {
        return None;
    }

    let context = image::imageops::crop_imm(rgb, x0, y0, x1 - x0, y1 - y0).to_image();
    let scale = (ROLE_INPUT_WIDTH as f32 / context.width() as f32)
        .min(ROLE_INPUT_HEIGHT as f32 / context.height() as f32);
    let resized_width = (context.width() as f32 * scale).round().max(1.0) as u32;
    let resized_height = (context.height() as f32 * scale).round().max(1.0) as u32;
    let resized = image::imageops::resize(
        &context,
        resized_width.min(ROLE_INPUT_WIDTH),
        resized_height.min(ROLE_INPUT_HEIGHT),
        FilterType::Triangle,
    );

    let mut image = RgbImage::from_pixel(ROLE_INPUT_WIDTH, ROLE_INPUT_HEIGHT, Rgb(ROLE_PADDING));
    let offset_x = (ROLE_INPUT_WIDTH.saturating_sub(resized.width())) / 2;
    let offset_y = (ROLE_INPUT_HEIGHT.saturating_sub(resized.height())) / 2;
    image::imageops::replace(&mut image, &resized, offset_x as i64, offset_y as i64);

    let width_norm = image_width as f32;
    let height_norm = image_height as f32;
    let geometry = [
        (bounds.center_x() / width_norm).clamp(0.0, 1.0),
        (bounds.center_y() / height_norm).clamp(0.0, 1.0),
        (width / width_norm).clamp(0.0, 1.0),
        (height / height_norm).clamp(0.0, 1.0),
        (block.lines.len().min(4) as f32 / 4.0).clamp(0.0, 1.0),
        (block.max_line_height() / height_norm).clamp(0.0, 1.0),
        (bounds.y0 / height_norm).clamp(0.0, 1.0),
        ((height_norm - bounds.y1) / height_norm).clamp(0.0, 1.0),
    ];

    Some(CandidateInput { image, geometry })
}
