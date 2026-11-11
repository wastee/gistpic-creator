//! 画布几何计算：图块位置、接缝热区、拉框相交。
//!
//! 这里只有纯数值计算，不碰界面、不碰应用状态，方便单独测试。

use gpui::{Pixels, Point, Size, point};

use crate::imaging::crop::{self, CropBand, Inclusion};
use crate::imaging::layout;
use crate::model::{ItemId, Mode, Project};
use crate::ui::tokens::{self, SEAM_HOTZONE, space};

/// 一个矩形，坐标单位是屏幕像素。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Rect { x, y, w, h }
    }

    /// 由任意两个对角点得到一个规整矩形（左上角 + 正数宽高）。
    pub fn from_corners(a: Point<f32>, b: Point<f32>) -> Self {
        Rect {
            x: a.x.min(b.x),
            y: a.y.min(b.y),
            w: (b.x - a.x).abs(),
            h: (b.y - a.y).abs(),
        }
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub fn contains(&self, point: Point<f32>) -> bool {
        point.x >= self.x
            && point.x <= self.right()
            && point.y >= self.y
            && point.y <= self.bottom()
    }

    /// 两个矩形是否有面积重叠。只碰一条边不算碰到。
    pub fn intersects(&self, other: &Rect) -> bool {
        self.x < other.right()
            && other.x < self.right()
            && self.y < other.bottom()
            && other.y < self.bottom()
    }

    /// 两个矩形在水平方向的重叠长度。
    pub fn overlap_x(&self, other: &Rect) -> f32 {
        self.right().min(other.right()) - self.x.max(other.x)
    }

    /// 两个矩形在竖直方向的重叠长度。
    pub fn overlap_y(&self, other: &Rect) -> f32 {
        self.bottom().min(other.bottom()) - self.y.max(other.y)
    }
}

/// 接缝的方向。上下相邻的两张图，接缝是横线，拖动方向是上下。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeamAxis {
    /// 两张图上下相邻，鼠标上下拖动改变间距
    Vertical,
    /// 两张图左右相邻，鼠标左右拖动改变间距
    Horizontal,
}

/// 两张图之间的一条接缝。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Seam {
    /// 稳定的序号，用作界面元素 id
    pub index: usize,
    /// 感应热区（12 像素宽），画在两张图的交界处
    pub rect: Rect,
    pub axis: SeamAxis,
    /// 热区中心线：上下相邻是 y，左右相邻是 x
    pub center: f32,
}

/// 画布上的一块图。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasEntry {
    pub id: ItemId,
    pub band: CropBand,
    pub rect: Rect,
    pub display_width: f32,
    pub display_height: f32,
}

/// 一整张画布的布局结果。
#[derive(Clone, Debug)]
pub struct CanvasLayout {
    pub entries: Vec<CanvasEntry>,
    pub seams: Vec<Seam>,
    pub width: f32,
    pub height: f32,
    /// 设置里的缝隙像素（0 表示没有缝隙）
    pub gap_px: u32,
    /// 缝隙在屏幕上的高度（已经按缩放换算）
    pub gap_display: f32,
    /// 显示像素 / 输出像素
    pub scale: f32,
    pub mode: Mode,
}

impl CanvasLayout {
    pub fn new(project: &Project, zoom: f32) -> Self {
        let base_width = tokens::CANVAS_BASE_WIDTH * tokens::clamp_zoom(zoom);
        let inclusions = crop::resolve(project);
        let gap_px = project.settings.gap.effective_px();

        match project.mode {
            Mode::Subtitle => {
                let mut raw: Vec<(ItemId, CropBand, u32)> = Vec::new();
                for (index, item) in project.items.iter().enumerate() {
                    let band = match inclusions.get(index).copied() {
                        Some(Inclusion::Crop(band)) => band,
                        Some(Inclusion::FullImage) => CropBand {
                            y0: 0,
                            y1: item.height,
                        },
                        _ => continue,
                    };
                    raw.push((item.id, band, item.width.max(1)));
                }
                if raw.is_empty() {
                    return CanvasLayout {
                        entries: Vec::new(),
                        seams: Vec::new(),
                        width: base_width,
                        height: 0.0,
                        gap_px,
                        gap_display: 0.0,
                        scale: 1.0,
                        mode: project.mode,
                    };
                }
                let output_width = raw.iter().map(|(_, _, width)| *width).max().unwrap_or(1) as f32;
                let scale = base_width / output_width;
                let gap_display = gap_px as f32 * scale;
                let mut entries = Vec::with_capacity(raw.len());
                let mut y = 0.0f32;
                for (index, (id, band, _)) in raw.iter().enumerate() {
                    let display_height = band.height() as f32 * scale;
                    entries.push(CanvasEntry {
                        id: *id,
                        band: *band,
                        rect: Rect::new(0.0, y, base_width, display_height),
                        display_width: base_width,
                        display_height,
                    });
                    y += display_height;
                    if index + 1 < raw.len() {
                        y += gap_display;
                    }
                }
                let height = y;
                let seams = compute_seams(&entries, gap_display);
                CanvasLayout {
                    entries,
                    seams,
                    width: base_width,
                    height,
                    gap_px,
                    gap_display,
                    scale,
                    mode: project.mode,
                }
            }
            Mode::Layout => {
                let mut sizes = Vec::new();
                let mut ids = Vec::new();
                for (index, item) in project.items.iter().enumerate() {
                    if matches!(
                        inclusions.get(index),
                        Some(Inclusion::Crop(_)) | Some(Inclusion::FullImage)
                    ) {
                        sizes.push((item.width.max(1), item.height.max(1)));
                        ids.push((item.id, item.height));
                    }
                }
                if sizes.is_empty() {
                    return CanvasLayout {
                        entries: Vec::new(),
                        seams: Vec::new(),
                        width: base_width,
                        height: 0.0,
                        gap_px,
                        gap_display: 0.0,
                        scale: 1.0,
                        mode: project.mode,
                    };
                }
                let result = layout::arrange(&sizes, project.settings.layout, gap_px);
                let scale = if result.canvas_w == 0 {
                    1.0
                } else {
                    base_width / result.canvas_w as f32
                };
                let gap_display = gap_px as f32 * scale;
                let mut entries = Vec::with_capacity(result.placements.len());
                for placement in &result.placements {
                    let Some((id, image_height)) = ids.get(placement.item).copied() else {
                        continue;
                    };
                    entries.push(CanvasEntry {
                        id,
                        band: CropBand {
                            y0: 0,
                            y1: image_height,
                        },
                        rect: Rect::new(
                            placement.x as f32 * scale,
                            placement.y as f32 * scale,
                            placement.w as f32 * scale,
                            placement.h as f32 * scale,
                        ),
                        display_width: placement.w as f32 * scale,
                        display_height: placement.h as f32 * scale,
                    });
                }
                let width = result.canvas_w as f32 * scale;
                let height = result.canvas_h as f32 * scale;
                let seams = compute_seams(&entries, gap_display);
                CanvasLayout {
                    entries,
                    seams,
                    width,
                    height,
                    gap_px,
                    gap_display,
                    scale,
                    mode: project.mode,
                }
            }
        }
    }

    /// 作品左上角在窗口里的位置。画布内容左右居中，上边留 24 像素呼吸空间。
    /// scroll 是滚动条偏移（向下滚动时为负）。
    pub fn origin(&self, viewport: Size<Pixels>, scroll: Point<Pixels>) -> Point<f32> {
        let pad = f32::from(space::XL);
        point(
            ((f32::from(viewport.width) - self.width) / 2.0).max(0.0),
            pad + f32::from(scroll.y),
        )
    }

    pub fn block_at(&self, position: Point<f32>) -> Option<ItemId> {
        self.entries
            .iter()
            .find(|entry| entry.rect.contains(position))
            .map(|entry| entry.id)
    }

    pub fn entry(&self, id: ItemId) -> Option<&CanvasEntry> {
        self.entries.iter().find(|entry| entry.id == id)
    }

    pub fn seam_at(&self, position: Point<f32>) -> Option<usize> {
        self.seams
            .iter()
            .find(|seam| seam.rect.contains(position))
            .map(|seam| seam.index)
    }
}

/// 算出所有相邻图块之间的接缝热区。
///
/// 上下相邻的两张图给一条横向接缝，左右相邻的给一条竖向接缝。
/// 热区固定 12 像素宽，画在交界线正中。
pub fn compute_seams(entries: &[CanvasEntry], gap_display: f32) -> Vec<Seam> {
    let mut seams = Vec::new();
    // 接缝热区以缝隙中线为中心，宽度不超过缝隙本身太多：
    // 没有缝隙时只剩交界处 8 像素，其余位置让给两边的裁剪线热区。
    let half = (gap_display * 0.5).clamp(4.0, f32::from(SEAM_HOTZONE) / 2.0);
    let mut index = 0;
    for i in 0..entries.len() {
        for j in (i + 1)..entries.len() {
            let a = entries[i].rect;
            let b = entries[j].rect;
            let overlap_x = a.overlap_x(&b);
            let overlap_y = a.overlap_y(&b);
            // 上下相邻：a 在上，b 在下
            let vertical_gap = b.y - a.bottom();
            if overlap_x > 1.0 && (vertical_gap - gap_display).abs() < 0.75 {
                let center = a.bottom() + gap_display / 2.0;
                seams.push(Seam {
                    index,
                    rect: Rect::new(a.x.max(b.x), center - half, overlap_x, half * 2.0),
                    axis: SeamAxis::Vertical,
                    center,
                });
                index += 1;
            }
            // 左右相邻：a 在左，b 在右
            let horizontal_gap = b.x - a.right();
            if overlap_y > 1.0 && (horizontal_gap - gap_display).abs() < 0.75 {
                let center = a.right() + gap_display / 2.0;
                seams.push(Seam {
                    index,
                    rect: Rect::new(center - half, a.y.max(b.y), half * 2.0, overlap_y),
                    axis: SeamAxis::Horizontal,
                    center,
                });
                index += 1;
            }
        }
    }
    seams
}

/// 拉框多选：返回被这个矩形碰到的所有图块 id，顺序和画布上的顺序一致。
pub fn marquee_hits(rect: Rect, entries: &[(ItemId, Rect)]) -> Vec<ItemId> {
    entries
        .iter()
        .filter(|(_, block)| rect.intersects(block))
        .map(|(id, _)| *id)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::new(x, y, w, h)
    }

    fn entry(id: ItemId, x: f32, y: f32, w: f32, h: f32) -> CanvasEntry {
        CanvasEntry {
            id,
            band: CropBand { y0: 0, y1: 100 },
            rect: rect(x, y, w, h),
            display_width: w,
            display_height: h,
        }
    }

    #[test]
    fn marquee_selects_every_block_it_touches() {
        let blocks = vec![
            (1, rect(0.0, 0.0, 100.0, 100.0)),
            (2, rect(0.0, 100.0, 100.0, 100.0)),
            (3, rect(0.0, 200.0, 100.0, 100.0)),
        ];
        // 只盖住第一张的下半和第二张的上半
        let hits = marquee_hits(rect(-10.0, 50.0, 120.0, 100.0), &blocks);
        assert_eq!(hits, vec![1, 2]);

        // 从下往上拉，矩形交换角点之后还能正确选中
        let drag = Rect::from_corners(point(110.0, 320.0), point(-10.0, 180.0));
        let hits = marquee_hits(drag, &blocks);
        assert_eq!(hits, vec![2, 3], "从下往上拉也要选中碰到的图");
    }

    #[test]
    fn marquee_ignores_blocks_outside_the_box() {
        let blocks = vec![(1, rect(0.0, 0.0, 100.0, 100.0))];
        assert!(marquee_hits(rect(200.0, 200.0, 50.0, 50.0), &blocks).is_empty());
        // 只碰到边不算碰到
        assert!(marquee_hits(rect(100.0, 0.0, 50.0, 100.0), &blocks).is_empty());
    }

    #[test]
    fn rect_normalizes_from_any_corner() {
        let a = Rect::from_corners(point(10.0, 20.0), point(30.0, 40.0));
        let b = Rect::from_corners(point(30.0, 40.0), point(10.0, 20.0));
        assert_eq!(a, b);
        assert_eq!(a, rect(10.0, 20.0, 20.0, 20.0));
    }

    #[test]
    fn seams_appear_between_stacked_blocks() {
        let entries = vec![
            entry(1, 0.0, 0.0, 100.0, 100.0),
            entry(2, 0.0, 108.0, 100.0, 100.0),
        ];
        let seams = compute_seams(&entries, 8.0);
        assert_eq!(seams.len(), 1);
        let seam = seams[0];
        assert_eq!(seam.axis, SeamAxis::Vertical);
        // 接缝中心在 104；缝隙 8 像素时热区正好盖住缝隙：100..108
        assert!((seam.center - 104.0).abs() < 1e-3);
        assert!((seam.rect.y - 100.0).abs() < 1e-3);
        assert!((seam.rect.h - 8.0).abs() < 1e-3);
    }

    #[test]
    fn seams_ignore_blocks_that_are_not_adjacent() {
        let entries = vec![
            entry(1, 0.0, 0.0, 100.0, 100.0),
            entry(2, 0.0, 130.0, 100.0, 100.0),
        ];
        assert!(compute_seams(&entries, 8.0).is_empty(), "离得远就不算接缝");
    }

    #[test]
    fn seams_appear_between_side_by_side_blocks() {
        let entries = vec![
            entry(1, 0.0, 0.0, 100.0, 100.0),
            entry(2, 108.0, 0.0, 100.0, 100.0),
        ];
        let seams = compute_seams(&entries, 8.0);
        assert_eq!(seams.len(), 1);
        assert_eq!(seams[0].axis, SeamAxis::Horizontal);
        assert!((seams[0].center - 104.0).abs() < 1e-3);
    }
}
