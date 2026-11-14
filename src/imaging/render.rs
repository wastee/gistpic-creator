//! 图像裁剪、预览图生成、拼接/排版合成与 PNG 编码。

use std::sync::Arc;

use gpui::RenderImage;
use image::imageops::FilterType;
use image::{Frame, RgbImage, RgbaImage};

use crate::model::{GapSetting, Rgba};

use super::crop::CropBand;
use super::layout::LayoutResult;

/// 按矩形范围裁剪。
pub fn crop_band_rgb(source: &RgbImage, band: CropBand) -> RgbImage {
    let width = source.width().max(1);
    let height = source.height().max(1);
    let x0 = band.x0.min(width.saturating_sub(1));
    let x1 = band.x1.min(width).max(x0 + 1);
    let y0 = band.y0.min(height.saturating_sub(1));
    let y1 = band.y1.min(height).max(y0 + 1);
    image::imageops::crop_imm(source, x0, y0, x1 - x0, y1 - y0).to_image()
}

/// 把 RGB 转成 GPUI 需要的 BGRA 顺序。
///
/// 一次遍历整块字节，不走逐像素 `put_pixel`：后者每个像素都要做一次边界检查，
/// 1920 宽的图要写两百多万次，拖动时会明显拖慢主线程。
fn rgb_to_bgra(rgb: &RgbImage) -> RgbaImage {
    let (width, height) = rgb.dimensions();
    let source = rgb.as_raw();
    let mut out = vec![0u8; source.len() / 3 * 4];
    for (src, dst) in source.chunks_exact(3).zip(out.chunks_exact_mut(4)) {
        dst[0] = src[2];
        dst[1] = src[1];
        dst[2] = src[0];
        dst[3] = 255;
    }
    RgbaImage::from_raw(width, height, out).expect("长度按每 3 字节转 4 字节计算，必然匹配")
}

fn rgb_to_rgba(rgb: &RgbImage) -> RgbaImage {
    let (width, height) = rgb.dimensions();
    let source = rgb.as_raw();
    let mut out = vec![0u8; source.len() / 3 * 4];
    for (src, dst) in source.chunks_exact(3).zip(out.chunks_exact_mut(4)) {
        dst[0] = src[0];
        dst[1] = src[1];
        dst[2] = src[2];
        dst[3] = 255;
    }
    RgbaImage::from_raw(width, height, out).expect("长度按每 3 字节转 4 字节计算，必然匹配")
}

/// 生成给界面显示的预览图。
///
/// 只做「整张图缩到一个目标宽度」，不裁剪：裁剪范围一变（拖裁剪线、横排竖排共用边）
/// 如果还要重画位图，每帧都得重新缩放整张原图，实测 3840×2160 一张就要三四百毫秒。
/// 现在界面用「容器裁切 + 位图位移」来表现裁剪，位图只在图片尺寸档位变化时才重画。
pub fn preview_image(source: &RgbImage, target_width: u32) -> Arc<RenderImage> {
    let (width, height) = source.dimensions();
    let target_width = target_width.max(1);
    // 目标宽不小于原图时不再 clone 整张 25MB 4K 图：直接对借用数据做一次
    // BGRA 转换即可（收窄到 1:1 曾经在这里整图复制，是卡顿的一大来源）。
    let bgra = if width > target_width {
        let new_height =
            ((height as u64 * target_width as u64) / width.max(1) as u64).max(1) as u32;
        // 缩略图用 thumbnail：它是专门做大幅缩小的算法，比 Lanczos3 快五倍，
        // 结果在这个尺寸上看不出差别。
        let preview = image::imageops::thumbnail(source, target_width, new_height);
        rgb_to_bgra(&preview)
    } else {
        rgb_to_bgra(source)
    };
    Arc::new(RenderImage::new(vec![Frame::new(bgra)]))
}

/// 字幕模式导出：所有图统一缩放到最宽那张的宽度，从上到下拼接。
pub fn compose_subtitle(parts: &[(&RgbImage, CropBand)], gap: &GapSetting) -> RgbaImage {
    if parts.is_empty() {
        return RgbaImage::new(1, 1);
    }

    // 先裁再比宽度：字幕模式不裁左右，但排版模式的手动框选会，统一按裁剪后的宽度对齐。
    let cropped: Vec<RgbImage> = parts
        .iter()
        .map(|(source, band)| crop_band_rgb(source, *band))
        .collect();
    let target_width = cropped
        .iter()
        .map(|image| image.width())
        .max()
        .unwrap_or(1)
        .max(1);
    let gap_px = gap.effective_px();

    let mut scaled: Vec<RgbaImage> = Vec::with_capacity(cropped.len());
    let mut scaled_heights: Vec<u32> = Vec::with_capacity(cropped.len());
    for cropped in &cropped {
        let (width, height) = cropped.dimensions();
        let new_height =
            ((height as u64 * target_width as u64) / width.max(1) as u64).max(1) as u32;
        let resized =
            image::imageops::resize(cropped, target_width, new_height, FilterType::Lanczos3);
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

/// 合成时最多同时缩放的图片数量。缩放是逐像素的重活，分到几个线程上能
/// 明显缩短「点复制到出结果」的等待；叠放仍然按原顺序单线程做，结果不变。
const COMPOSE_WORKERS: usize = 4;

/// 小于这个像素数的画布不值得开线程：线程开销会盖过省下来的时间。
const PARALLEL_COMPOSE_MIN_PIXELS: u64 = 2_000_000;

fn compose_workers() -> usize {
    std::thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1)
        .clamp(1, COMPOSE_WORKERS)
}

/// 把一张图缩放到它在画布上的位置，并转成画布使用的 RGBA 顺序。
fn resized_placement(source: &RgbImage, width: u32, height: u32) -> RgbaImage {
    let resized =
        image::imageops::resize(source, width.max(1), height.max(1), FilterType::Lanczos3);
    rgb_to_rgba(&resized)
}

/// 排版模式导出：按排版结果把图片画到画布上，缝隙用背景色填充。
pub fn compose_layout(images: &[&RgbImage], layout: &LayoutResult, background: Rgba) -> RgbaImage {
    let mut canvas = RgbaImage::from_pixel(
        layout.canvas_w.max(1),
        layout.canvas_h.max(1),
        background.to_image_rgba(),
    );
    let total_pixels = layout.canvas_w as u64 * layout.canvas_h as u64;
    let parallel = total_pixels >= PARALLEL_COMPOSE_MIN_PIXELS && layout.placements.len() > 1;
    if !parallel {
        for placement in &layout.placements {
            let Some(source) = images.get(placement.item).copied() else {
                continue;
            };
            let rgba = resized_placement(source, placement.w, placement.h);
            image::imageops::overlay(&mut canvas, &rgba, placement.x as i64, placement.y as i64);
        }
        return canvas;
    }

    let workers = compose_workers();
    for chunk in layout.placements.chunks(workers) {
        // 一批图同时缩放，缩完再按原顺序叠到画布上。
        let scaled: Vec<Option<RgbaImage>> = std::thread::scope(|scope| {
            let handles: Vec<_> = chunk
                .iter()
                .map(|placement| {
                    let source = images.get(placement.item).copied();
                    let width = placement.w.max(1);
                    let height = placement.h.max(1);
                    scope.spawn(move || {
                        source.map(|source| resized_placement(source, width, height))
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| match handle.join() {
                    Ok(image) => image,
                    Err(_) => panic!("图片缩放线程失败"),
                })
                .collect()
        });
        for (placement, rgba) in chunk.iter().zip(scaled) {
            let Some(rgba) = rgba else {
                continue;
            };
            image::imageops::overlay(&mut canvas, &rgba, placement.x as i64, placement.y as i64);
        }
    }
    canvas
}

/// 编码成 PNG 字节。
///
/// 直接把像素缓冲交给 PNG 编码器，不再 `image.clone()` 整张 4K RGBA
/// （约 33MB 的一次多余拷贝）；输出与 `DynamicImage::write_to` 完全一致。
pub fn encode_png(image: &RgbaImage) -> Result<Vec<u8>, image::ImageError> {
    use image::{ExtendedColorType, ImageEncoder};

    let mut bytes = Vec::new();
    image::codecs::png::PngEncoder::new(&mut bytes).write_image(
        image.as_raw(),
        image.width(),
        image.height(),
        ExtendedColorType::Rgba8,
    )?;
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
