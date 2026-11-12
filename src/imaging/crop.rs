//! 裁剪带推导：根据字幕块、出血位、第一张规则或用户手动框选，算出每张图要保留的矩形范围。

use crate::model::{AnalysisState, Item, ItemStatus, ManualBand, Mode, Project, Settings, SubtitleBlock};

/// 要保留的矩形范围（原图像素）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CropBand {
    pub x0: u32,
    pub x1: u32,
    pub y0: u32,
    pub y1: u32,
}

impl CropBand {
    /// 整张图的范围。
    pub fn full(item: &Item) -> Self {
        Self {
            x0: 0,
            x1: item.width.max(1),
            y0: 0,
            y1: item.height.max(1),
        }
    }

    /// 只按纵向范围裁剪，左右保留整幅宽度。字幕模式的自动结果都是这一种。
    pub fn rows(width: u32, y0: u32, y1: u32) -> Self {
        Self {
            x0: 0,
            x1: width.max(1),
            y0,
            y1,
        }
    }

    pub fn width(self) -> u32 {
        self.x1.saturating_sub(self.x0)
    }

    pub fn height(self) -> u32 {
        self.y1.saturating_sub(self.y0)
    }

    pub fn size(self) -> (u32, u32) {
        (self.width(), self.height())
    }

    /// 这个范围盖住了整张图。
    pub fn is_whole(self, item: &Item) -> bool {
        self == Self::full(item)
    }
}

/// 一张图在结果里怎么处理。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Inclusion {
    /// 按这个范围裁剪后参与结果
    Crop(CropBand),
    /// 整张图原样参与结果
    FullImage,
    /// 不参与结果
    Skipped,
}

impl Inclusion {
    /// 这张图在结果里占多少输出像素。不参与结果时返回 `None`。
    pub fn output_size(self, item: &Item) -> Option<(u32, u32)> {
        match self {
            Inclusion::Crop(band) => Some(band.size()),
            Inclusion::FullImage => Some((item.width.max(1), item.height.max(1))),
            Inclusion::Skipped => None,
        }
    }

    /// 这张图实际参与输出的范围。
    pub fn band_for(self, item: &Item) -> CropBand {
        match self {
            Inclusion::Crop(band) => band,
            Inclusion::FullImage | Inclusion::Skipped => CropBand::full(item),
        }
    }
}

/// 字幕块的上下边界，加上出血位。左右保留整幅宽度。
pub fn band_for_block(
    block: &SubtitleBlock,
    keep_second: bool,
    bleed_ratio: f32,
    image_size: (u32, u32),
) -> Option<CropBand> {
    let (image_width, image_height) = image_size;
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

    Some(CropBand::rows(
        image_width,
        top.floor() as u32,
        (bottom.ceil() as u32).min(image_height),
    ))
}

/// 第一张图：保留画面主体。
///
/// 字幕在画面下半（常见情况）时保留从顶部到字幕下沿；字幕在画面上半时保留从字幕上沿到底部。
pub fn subject_band_for_block(
    block: &SubtitleBlock,
    keep_second: bool,
    bleed_ratio: f32,
    image_size: (u32, u32),
) -> Option<CropBand> {
    let (image_width, image_height) = image_size;
    let band = band_for_block(block, keep_second, bleed_ratio, image_size)?;
    if block.center_y() <= image_height as f32 * 0.5 {
        Some(CropBand::rows(image_width, band.y0, image_height))
    } else {
        Some(CropBand::rows(image_width, 0, band.y1))
    }
}

/// 把用户手动框选的相对范围换算成原图像素带。
pub fn manual_band_for_item(item: &Item, manual: ManualBand) -> Option<CropBand> {
    let width = item.width as f32;
    let height = item.height as f32;
    if width <= 0.0 || height <= 0.0 {
        return None;
    }
    // 四条边都落到最近的像素：拖动量本来就是从像素换算来的，
    // 用 floor/ceil 会让每次拖动都多留一个像素，越拖越偏。
    let x0 = (manual.left.clamp(0.0, 1.0) * width).round();
    let x1 = (manual.right.clamp(0.0, 1.0) * width).round();
    let y0 = (manual.top.clamp(0.0, 1.0) * height).round();
    let y1 = (manual.bottom.clamp(0.0, 1.0) * height).round();
    let band = CropBand {
        x0: x0.clamp(0.0, width) as u32,
        x1: (x1.clamp(0.0, width) as u32).min(item.width),
        y0: y0.clamp(0.0, height) as u32,
        y1: (y1.clamp(0.0, height) as u32).min(item.height),
    };
    if band.width() == 0 || band.height() == 0 {
        return None;
    }
    Some(band)
}

/// 算出整个项目里每张图的处理方式。返回的数组和 `project.items` 一一对应。
pub fn resolve(project: &Project) -> Vec<Inclusion> {
    let settings = &project.settings;
    let mut result = Vec::with_capacity(project.items.len());

    // 手动框选是用户的明确决定，两种模式都要遵守。
    if project.mode == Mode::Layout {
        for item in &project.items {
            result.push(match manual_inclusion(item) {
                Some(inclusion) => inclusion,
                None => {
                    if participates(item) {
                        Inclusion::FullImage
                    } else {
                        Inclusion::Skipped
                    }
                }
            });
        }
        return result;
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

/// 这张图是否参与输出。
///
/// 只看分析状态与呈现状态的正交组合：排版模式插入的图从没跑过 OCR，
/// 但 `NotNeeded` 让它立刻出现在画布上；只有真正在排队/执行 OCR 的图才跳过。
fn participates(item: &Item) -> bool {
    match item.analysis {
        AnalysisState::NotNeeded => true,
        AnalysisState::PendingAnalysis | AnalysisState::Analyzing => false,
        AnalysisState::Analyzed => match item.status {
            ItemStatus::Ready | ItemStatus::NoSubtitle => true,
            ItemStatus::Failed(_) => item.keep_whole,
            ItemStatus::Pending => false,
        },
    }
}

/// 用户手动框过的图返回裁剪结果；没框过或换算不出来时返回 `None`，交给调用方决定。
fn manual_inclusion(item: &Item) -> Option<Inclusion> {
    if matches!(item.status, ItemStatus::Pending) {
        return Some(Inclusion::Skipped);
    }
    let manual = item.manual?;
    Some(match manual_band_for_item(item, manual) {
        Some(band) => Inclusion::Crop(band),
        None => Inclusion::FullImage,
    })
}

fn inclusion_for_item(
    item: &Item,
    is_first_included: bool,
    settings: &Settings,
    bleeding: f32,
) -> Inclusion {
    if let Some(inclusion) = manual_inclusion(item) {
        return inclusion;
    }

    match &item.status {
        ItemStatus::Pending => Inclusion::Skipped,
        ItemStatus::Failed(_) => {
            if item.keep_whole {
                Inclusion::FullImage
            } else {
                Inclusion::Skipped
            }
        }
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
            // 用户明确要求整张画面时，不再按字幕位置裁剪。
            if item.keep_whole {
                return Inclusion::FullImage;
            }
            let size = (item.width, item.height);
            let band = if is_first_included {
                subject_band_for_block(block, settings.keep_second, bleeding, size)
            } else {
                band_for_block(block, settings.keep_second, bleeding, size)
            };
            match band {
                Some(band) => Inclusion::Crop(band),
                None => Inclusion::FullImage,
            }
        }
    }
}
