//! 裁剪带推导：根据字幕块、出血位、第一张规则，算出每张图要保留的纵向范围。

use crate::model::{Item, ItemStatus, Mode, Project, SearchRegion, Settings, SubtitleBlock};

/// 要保留的纵向范围（原图像素），左右永远保留整幅宽度。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CropBand {
    pub y0: u32,
    pub y1: u32,
}

impl CropBand {
    pub fn height(self) -> u32 {
        self.y1.saturating_sub(self.y0)
    }
}

/// 一张图在结果里怎么处理。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Inclusion {
    /// 按这个纵向范围裁剪后参与结果
    Crop(CropBand),
    /// 整张图原样参与结果
    FullImage,
    /// 不参与结果
    Skipped,
}

/// 字幕块的上下边界，加上出血位。
pub fn band_for_block(
    block: &SubtitleBlock,
    keep_second: bool,
    bleed_ratio: f32,
    image_height: u32,
) -> Option<CropBand> {
    let first = *block.lines.first()?;
    let last = if keep_second && block.lines.len() >= 2 {
        block.lines[1]
    } else {
        first
    };

    let height = image_height as f32;
    let bleed = bleed_ratio.clamp(0.0, 0.30);
    let top = (first.y0 - first.height() * bleed).clamp(0.0, height);
    let bottom = (last.y1 + last.height() * bleed).clamp(0.0, height);
    if bottom <= top {
        return None;
    }

    Some(CropBand {
        y0: top.floor() as u32,
        y1: (bottom.ceil() as u32).min(image_height),
    })
}

/// 第一张图：保留画面主体。
///
/// 字幕在画面下半（常见情况）时保留从顶部到字幕下沿；字幕在画面上半时保留从字幕上沿到底部。
pub fn subject_band_for_block(
    block: &SubtitleBlock,
    keep_second: bool,
    bleed_ratio: f32,
    image_height: u32,
) -> Option<CropBand> {
    let band = band_for_block(block, keep_second, bleed_ratio, image_height)?;
    if block.center_y() <= image_height as f32 * 0.5 {
        Some(CropBand {
            y0: band.y0,
            y1: image_height,
        })
    } else {
        Some(CropBand {
            y0: 0,
            y1: band.y1,
        })
    }
}

/// 算出整个项目里每张图的处理方式。返回的数组和 `project.items` 一一对应。
pub fn resolve(project: &Project) -> Vec<Inclusion> {
    let settings = &project.settings;
    let mut result = Vec::with_capacity(project.items.len());

    match project.mode {
        Mode::Layout => {
            for item in &project.items {
                result.push(if participates(item) {
                    Inclusion::FullImage
                } else {
                    Inclusion::Skipped
                });
            }
            return result;
        }
        Mode::Subtitle => {}
    }

    if !settings.crop {
        for item in &project.items {
            result.push(if participates(item) {
                Inclusion::FullImage
            } else {
                Inclusion::Skipped
            });
        }
        return result;
    }

    let bleeding = settings.bleed;
    let mut saw_included = false;
    for item in &project.items {
        let inclusion = inclusion_for_item(item, !saw_included, settings, bleeding);
        if matches!(inclusion, Inclusion::Crop(_) | Inclusion::FullImage) {
            saw_included = true;
        }
        result.push(inclusion);
    }
    result
}

fn participates(item: &Item) -> bool {
    match item.status {
        ItemStatus::Ready | ItemStatus::NoSubtitle => true,
        ItemStatus::Pending | ItemStatus::Failed(_) => false,
    }
}

fn inclusion_for_item(
    item: &Item,
    is_first_included: bool,
    settings: &Settings,
    bleeding: f32,
) -> Inclusion {
    match &item.status {
        ItemStatus::Pending | ItemStatus::Failed(_) => Inclusion::Skipped,
        ItemStatus::NoSubtitle => {
            if item.keep_whole {
                Inclusion::FullImage
            } else {
                Inclusion::Skipped
            }
        }
        ItemStatus::Ready => {
            let block = match &item.block {
                Some(block) => block,
                None => {
                    // 没有字幕但状态是 Ready（例如关闭裁剪后重新打开），整张保留
                    return if item.keep_whole {
                        Inclusion::FullImage
                    } else {
                        Inclusion::Skipped
                    };
                }
            };
            let band = if is_first_included {
                subject_band_for_block(block, settings.keep_second, bleeding, item.height)
            } else {
                band_for_block(block, settings.keep_second, bleeding, item.height)
            };
            match band {
                Some(band) => Inclusion::Crop(band),
                None => Inclusion::FullImage,
            }
        }
    }
}

/// 便捷方法：某个搜索范围下字幕块是否有效（供界面显示提示用）。
pub fn block_is_usable(block: &SubtitleBlock, region: SearchRegion, image_height: u32) -> bool {
    match block.lines.first() {
        Some(first) => super::classify::line_in_region(*first, region, image_height as f32),
        None => false,
    }
}
