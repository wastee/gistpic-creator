//! 图像裁剪、预览图生成、拼接/排版合成与 PNG 编码。

use std::io::Cursor;
use std::sync::Arc;

use gpui::RenderImage;
use image::imageops::FilterType;
use image::{Frame, RgbImage, RgbaImage};

use crate::model::{GapSetting, Rgba};

use super::crop::CropBand;
use super::layout::LayoutResult;

/// 按纵向范围裁剪（左右保留整幅宽度）。
pub fn crop_band_rgb(source: &RgbImage, band: CropBand) -> RgbImage {
    let height = source.height().max(1);
    let y0 = band.y0.min(height.saturating_sub(1));
    let y1 = band.y1.min(height).max(y0 + 1);
    image::imageops::crop_imm(source, 0, y0, source.width().max(1), y1 - y0).to_image()
}

/// 把 RGB 转成 GPUI 需要的 BGRA 顺序。
fn rgb_to_bgra(rgb: &RgbImage) -> RgbaImage {
    let (width, height) = rgb.dimensions();
    let mut out = RgbaImage::new(width, height);
    for (x, y, pixel) in rgb.enumerate_pixels() {
        out.put_pixel(x, y, image::Rgba([pixel[2], pixel[1], pixel[0], 255]));
    }
    out
}

fn rgb_to_rgba(rgb: &RgbImage) -> RgbaImage {
    let (width, height) = rgb.dimensions();
    let mut out = RgbaImage::new(width, height);
    for (x, y, pixel) in rgb.enumerate_pixels() {
        out.put_pixel(x, y, image::Rgba([pixel[0], pixel[1], pixel[2], 255]));
    }
    out
}

/// 生成给界面显示的预览图。`target_width` 是预览图的最大宽度。
pub fn preview_image(source: &RgbImage, band: CropBand, target_width: u32) -> Arc<RenderImage> {
    let cropped = crop_band_rgb(source, band);
    let (width, height) = cropped.dimensions();
    let target_width = target_width.max(1);
    let preview = if width > target_width {
        let new_height =
            ((height as u64 * target_width as u64) / width.max(1) as u64).max(1) as u32;
        image::imageops::resize(&cropped, target_width, new_height, FilterType::Triangle)
    } else {
        cropped
    };
    let bgra = rgb_to_bgra(&preview);
    Arc::new(RenderImage::new(vec![Frame::new(bgra)]))
}

/// 整张图的预览，不做裁剪。
pub fn preview_full(source: &RgbImage, target_width: u32) -> Arc<RenderImage> {
    preview_image(
        source,
        CropBand {
            y0: 0,
            y1: source.height().max(1),
        },
        target_width,
    )
}

/// 字幕模式导出：所有图统一缩放到最宽那张的宽度，从上到下拼接。
pub fn compose_subtitle(parts: &[(&RgbImage, CropBand)], gap: &GapSetting) -> RgbaImage {
    if parts.is_empty() {
        return RgbaImage::new(1, 1);
    }

    let target_width = parts
        .iter()
        .map(|(source, _)| source.width())
        .max()
        .unwrap_or(1)
        .max(1);
    let gap_px = gap.effective_px();

    let mut scaled: Vec<RgbaImage> = Vec::with_capacity(parts.len());
    let mut scaled_heights: Vec<u32> = Vec::with_capacity(parts.len());
    for (source, band) in parts {
        let cropped = crop_band_rgb(source, *band);
        let (width, height) = cropped.dimensions();
        let new_height =
            ((height as u64 * target_width as u64) / width.max(1) as u64).max(1) as u32;
        let resized =
            image::imageops::resize(&cropped, target_width, new_height, FilterType::Lanczos3);
        scaled_heights.push(new_height);
        scaled.push(rgb_to_rgba(&resized));
    }

    let total_height = scaled_heights
        .iter()
        .copied()
        .fold(0u32, |acc, h| acc.saturating_add(h))
        .saturating_add(gap_px.saturating_mul(scaled.len().saturating_sub(1) as u32))
        .max(1);

    let background = if gap.enabled { gap.color } else { Rgba::BLACK };
    let mut canvas = RgbaImage::from_pixel(
        target_width.max(1),
        total_height,
        background.to_image_rgba(),
    );

    let mut y = 0i64;
    for image in scaled {
        image::imageops::overlay(&mut canvas, &image, 0, y);
        y += image.height() as i64 + gap_px as i64;
    }

    canvas
}

/// 排版模式导出：按排版结果把图片画到画布上，缝隙用背景色填充。
pub fn compose_layout(images: &[&RgbImage], layout: &LayoutResult, background: Rgba) -> RgbaImage {
    let mut canvas = RgbaImage::from_pixel(
        layout.canvas_w.max(1),
        layout.canvas_h.max(1),
        background.to_image_rgba(),
    );
    for placement in &layout.placements {
        let Some(source) = images.get(placement.item).copied() else {
            continue;
        };
        let resized = image::imageops::resize(
            source,
            placement.w.max(1),
            placement.h.max(1),
            FilterType::Lanczos3,
        );
        let rgba = rgb_to_rgba(&resized);
        image::imageops::overlay(&mut canvas, &rgba, placement.x as i64, placement.y as i64);
    }
    canvas
}

/// 编码成 PNG 字节。
pub fn encode_png(image: &RgbaImage) -> Result<Vec<u8>, image::ImageError> {
    let mut bytes = Vec::new();
    image::DynamicImage::ImageRgba8(image.clone())
        .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgb_to_bgra_swaps_red_and_blue() {
        // GPUI 的 RenderImage 要求 BGRA 顺序，红色像素应变成 [0, 0, 255, 255]。
        let red = RgbImage::from_pixel(1, 1, image::Rgb([255, 0, 0]));
        let bgra = rgb_to_bgra(&red);
        let pixel = bgra.get_pixel(0, 0);
        assert_eq!([pixel[0], pixel[1], pixel[2], pixel[3]], [0, 0, 255, 255]);
    }
}
