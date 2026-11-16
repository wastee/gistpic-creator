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

/// 缩放相关的活一律走 DynamicImage，不要直接调 image::imageops 里的 resize、thumbnail
/// 这些泛型函数。
///
/// 那些泛型函数会在本 crate 里按开发构建（不优化）编译，一张 3840×2160 的图缩一次
/// 实测要一秒多；DynamicImage 上的方法在依赖里已经实例化好，跟着
/// profile.dev.package."*" 的 opt-level 3 走，同样的活快七倍以上。
pub(crate) fn as_dynamic(rgb: RgbImage) -> image::DynamicImage {
    image::DynamicImage::ImageRgb8(rgb)
}

/// 生成给界面显示的预览图。
///
/// 只做「整张图缩到一个目标宽度」，不裁剪：裁剪范围一变（拖裁剪线、横排竖排共用边）
/// 如果还要重画位图，每帧都得重新缩放整张原图，实测 3840×2160 一张就要三四百毫秒。
/// 现在界面用「容器裁切 + 位图位移」来表现裁剪，位图只在图片尺寸档位变化时才重画。
pub fn preview_image(source: RgbImage, target_width: u32) -> Arc<RenderImage> {
    let (width, height) = source.dimensions();
    let target_width = target_width.max(1);
    let bgra = if width > target_width {
        let new_height =
            ((height as u64 * target_width as u64) / width.max(1) as u64).max(1) as u32;
        // 缩略图用 thumbnail：它是专门做大幅缩小的算法，比 Lanczos3 快五倍，
        // 结果在这个尺寸上看不出差别。
        let preview = as_dynamic(source).thumbnail(target_width, new_height).to_rgb8();
        rgb_to_bgra(&preview)
    } else {
        rgb_to_bgra(&source)
    };
    Arc::new(RenderImage::new(vec![Frame::new(bgra)]))
}

/// 图块的边长（物理像素）。
///
/// 屏幕按物理像素采样，块越大单块贴图越省切换；512 在 4K 屏上大约每屏
/// 三四十块，单块又小到可以只重建屏幕内缺的那几块。
pub const TILE_SIZE: u32 = 512;

/// 相邻图块互相压住的物理像素数。
///
/// 每块在四条边上各多取 1 像素，两块贴到屏幕上会互相压住，缩放采样在接缝
/// 处不会留下一条露出底色的细线。
pub const TILE_OVERLAP: u32 = 1;

/// 一块图块在目标物理像素网格里的范围。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TileRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl TileRect {
    pub fn right(self) -> u32 {
        self.x.saturating_add(self.width)
    }

    pub fn bottom(self) -> u32 {
        self.y.saturating_add(self.height)
    }

    pub fn is_empty(self) -> bool {
        self.width == 0 || self.height == 0
    }
}

/// 网格第 `col` 列、第 `row` 行那块实际要画的像素范围。
///
/// 边缘块截断在目标尺寸以内；内部块在四条边上各多带 `TILE_OVERLAP` 个
/// 物理像素，相邻两块贴到屏幕上会互相压住，接缝处不会露出底色。
pub fn tile_rect(col: u32, row: u32, target_width: u32, target_height: u32) -> TileRect {
    if target_width == 0 || target_height == 0 {
        return TileRect {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        };
    }
    let x0 = col.saturating_mul(TILE_SIZE).min(target_width);
    let y0 = row.saturating_mul(TILE_SIZE).min(target_height);
    if x0 >= target_width || y0 >= target_height {
        return TileRect {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        };
    }
    let x1 = x0.saturating_add(TILE_SIZE).min(target_width);
    let y1 = y0.saturating_add(TILE_SIZE).min(target_height);
    let left = x0.saturating_sub(TILE_OVERLAP);
    let top = y0.saturating_sub(TILE_OVERLAP);
    let right = x1.saturating_add(TILE_OVERLAP).min(target_width);
    let bottom = y1.saturating_add(TILE_OVERLAP).min(target_height);
    TileRect {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    }
}

/// 生成一块图块位图（BGRA 顺序，GPUI 直接显示用）。
///
/// `target_width` / `target_height` 是整张原图铺满屏幕时的物理像素尺寸，
/// `rect` 是这块在目标网格里的像素范围。只裁出这一小块源图再缩放，不生成
/// 整张图的中间大图。
///
/// 按整张原图切、不带裁剪带：裁剪带一变（切模式、拖裁剪线、换排版）图块的
/// 内容不变，屏幕上的块原样接着用，不会先糊一下再变清楚。
pub fn tile_image(
    source: &image::DynamicImage,
    target_width: u32,
    target_height: u32,
    rect: TileRect,
) -> Arc<RenderImage> {
    let source_width = source.width().max(1);
    let source_height = source.height().max(1);
    let target_width = target_width.max(1);
    let target_height = target_height.max(1);
    let width = rect.width.max(1);
    let height = rect.height.max(1);
    let scale_x = source_width as f64 / target_width as f64;
    let scale_y = source_height as f64 / target_height as f64;
    // 目标像素范围换回原图坐标：起点向下取整、终点向上取整，各多带不到
    // 1 像素。相邻块取到的源像素因此有一段重叠，缩放时不会因为源图边界
    // 在接缝处少一条。
    let sx0 = (rect.x as f64 * scale_x).floor() as u32;
    let sx1 = (rect.right() as f64 * scale_x).ceil() as u32;
    let sy0 = (rect.y as f64 * scale_y).floor() as u32;
    let sy1 = (rect.bottom() as f64 * scale_y).ceil() as u32;
    // 记录尺寸和磁盘上的实际尺寸不一致时（图片被换掉了），收进真实源图范围。
    let sx0 = sx0.min(source_width - 1);
    let sx1 = sx1.clamp(sx0 + 1, source_width);
    let sy0 = sy0.min(source_height - 1);
    let sy1 = sy1.clamp(sy0 + 1, source_height);
    let cropped = source.crop_imm(sx0, sy0, sx1 - sx0, sy1 - sy0);
    let resized = if (sx1 - sx0, sy1 - sy0) == (width, height) {
        cropped.to_rgb8()
    } else {
        cropped
            .resize_exact(width, height, FilterType::Triangle)
            .to_rgb8()
    };
    let bgra = rgb_to_bgra(&resized);
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

    #[test]
    fn tile_rect_covers_edges_and_overlaps_neighbours() {
        // 1000×600 的目标网格：第一行第一块名义 512×512，四边各多带 1 像素。
        assert_eq!(
            tile_rect(0, 0, 1000, 600),
            TileRect {
                x: 0,
                y: 0,
                width: 513,
                height: 513
            }
        );
        // 第二列起点 512，左边多带 1 像素压住第一块；右边到 1000 截断。
        assert_eq!(
            tile_rect(1, 0, 1000, 600),
            TileRect {
                x: 511,
                y: 0,
                width: 489,
                height: 513
            }
        );
        // 第二行底边截断在 600。
        assert_eq!(
            tile_rect(0, 1, 1000, 600),
            TileRect {
                x: 0,
                y: 511,
                width: 513,
                height: 89
            }
        );
        // 相邻两块在像素上有重叠，接缝处不会露出底色。
        let first = tile_rect(0, 0, 1000, 600);
        let second = tile_rect(1, 0, 1000, 600);
        assert!(
            first.right() - second.x >= 2,
            "相邻块应至少重叠 2 个物理像素"
        );
        // 网格外没有块。
        assert!(tile_rect(2, 0, 1000, 600).is_empty());
        assert!(tile_rect(0, 2, 1000, 600).is_empty());
        assert!(tile_rect(0, 0, 0, 0).is_empty());
    }

    #[test]
    fn tile_image_has_requested_pixel_size_and_bgra_order() {
        let source = RgbImage::from_pixel(1024, 512, image::Rgb([255, 0, 0]));
        let rect = tile_rect(0, 0, 1024, 512);
        let image = tile_image(&as_dynamic(source.clone()), 1024, 512, rect);
        // 第一块右边多带 1 像素重叠；下边正好到 512，没有额外行。
        assert_eq!(
            image.size(0),
            gpui::size(gpui::DevicePixels(513), gpui::DevicePixels(512))
        );
        // 红色在 BGRA 里是 [0, 0, 255, 255]，第一块左上角就是源图左上角。
        let bytes = image.as_bytes(0).expect("图块只有一帧");
        assert_eq!(&bytes[0..4], &[0, 0, 255, 255]);
    }

    #[test]
    fn tile_image_samples_the_slice_its_rect_covers() {
        // 左半黑、右半白；只取右半那块，生成的图块必须全是白的。
        let mut source = RgbImage::from_pixel(100, 10, image::Rgb([0, 0, 0]));
        for x in 50..100 {
            for y in 0..10 {
                source.put_pixel(x, y, image::Rgb([255, 255, 255]));
            }
        }
        let rect = tile_rect(0, 0, 100, 10);
        let image = tile_image(&as_dynamic(source.clone()), 100, 10, rect);
        // 整张图只有一块，边缘没有多带的像素，宽就是 100。
        let bytes = image.as_bytes(0).expect("图块只有一帧");
        assert_eq!(&bytes[0..4], &[0, 0, 0, 255], "最左边应是黑的");
        let width = 100usize;
        let last = (0 * width + 99) * 4;
        assert_eq!(&bytes[last..last + 4], &[255, 255, 255, 255], "右边缘应是白的");
    }

    #[test]
    fn tile_image_edge_block_is_clamped_to_target_size() {
        let source = RgbImage::from_pixel(700, 300, image::Rgb([12, 34, 56]));
        let rect = tile_rect(1, 0, 700, 300);
        let image = tile_image(&as_dynamic(source.clone()), 700, 300, rect);
        // 第二列从 512 到 700，共 189 像素；整行只有 300 高。
        assert_eq!(
            image.size(0),
            gpui::size(gpui::DevicePixels(189), gpui::DevicePixels(300))
        );
    }

    #[test]
    fn tile_image_clamps_to_actual_source_size() {
        // 磁盘上的图被换成更小的尺寸时，按 Item 记录的尺寸算出的目标网格会越界；
        // 生成图块要收进真实源图范围，不能 panic。
        let source = RgbImage::from_pixel(10, 10, image::Rgb([1, 2, 3]));
        let rect = tile_rect(0, 0, 100, 100);
        let image = tile_image(&as_dynamic(source.clone()), 100, 100, rect);
        assert_eq!(
            image.size(0),
            gpui::size(gpui::DevicePixels(100), gpui::DevicePixels(100))
        );
    }
}
