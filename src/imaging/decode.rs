//! 图片解码：把剪贴板字节或文件路径变成 RGB8 像素。

use std::path::Path;

use image::RgbImage;
use thiserror::Error;

use crate::i18n::Language;

#[derive(Debug, Error)]
pub enum DecodeError {
    #[error("不是可识别的图片，或文件已损坏: {0}")]
    Image(#[from] image::ImageError),
}

impl DecodeError {
    /// 给界面显示用的错误说明。库自己给出的技术细节保持原样。
    pub fn localized(&self, language: Language) -> String {
        let DecodeError::Image(err) = self;
        match language {
            Language::ZhCn => format!("不是可识别的图片，或文件已损坏: {err}"),
            Language::EnUs => format!("Unrecognized or corrupted image: {err}"),
        }
    }
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

/// 解码后原图像素内容的 64 位指纹。
///
/// 用于「删除再添加 / 切换模式」时复用字幕识别结果：内容相同就命中，
/// 与路径、文件名、自增 `ItemId` 都无关。只在后台解码任务里计算，
/// 避免在主线程遍历 4K 图的 25MB 像素。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ImageFingerprint(pub u64);

/// FNV-1a 64 位，逐字节混入 RGB 像素，最后混入宽高。
pub fn fingerprint(rgb: &RgbImage) -> ImageFingerprint {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for &byte in rgb.as_raw() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    let (width, height) = rgb.dimensions();
    for part in [width as u64, height as u64] {
        hash ^= part;
        hash = hash.wrapping_mul(0x100_0000_01b3);
    }
    ImageFingerprint(hash)
}

/// 后台解码任务的结果：像素 + 指纹，一起回传主线程。
pub struct DecodedImage {
    pub rgb: RgbImage,
    pub fingerprint: ImageFingerprint,
}

pub fn decode_bytes_with_fingerprint(bytes: &[u8]) -> Result<DecodedImage, DecodeError> {
    let rgb = decode_bytes(bytes)?;
    let fingerprint = fingerprint(&rgb);
    Ok(DecodedImage { rgb, fingerprint })
}

pub fn decode_path_with_fingerprint(path: &Path) -> Result<DecodedImage, DecodeError> {
    let rgb = decode_path(path)?;
    let fingerprint = fingerprint(&rgb);
    Ok(DecodedImage { rgb, fingerprint })
}
