//! 图片解码：把剪贴板字节或文件路径变成 RGB8 像素。

use std::path::Path;

use image::RgbImage;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DecodeError {
    #[error("不是可识别的图片，或文件已损坏: {0}")]
    Image(#[from] image::ImageError),
}

/// 从字节解码（剪贴板、文件内容）。
pub fn decode_bytes(bytes: &[u8]) -> Result<RgbImage, DecodeError> {
    let dynamic = image::load_from_memory(bytes)?;
    Ok(dynamic.to_rgb8())
}

/// 从文件路径解码。
pub fn decode_path(path: &Path) -> Result<RgbImage, DecodeError> {
    let dynamic = image::open(path)?;
    Ok(dynamic.to_rgb8())
}

/// 支持的图片扩展名（用于文件选择框过滤与拖入过滤）。
pub const SUPPORTED_EXTENSIONS: [&str; 6] = ["png", "jpg", "jpeg", "webp", "bmp", "gif"];

pub fn is_supported_path(path: &Path) -> bool {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some(ext) => SUPPORTED_EXTENSIONS
            .iter()
            .any(|supported| supported.eq_ignore_ascii_case(ext)),
        None => false,
    }
}
