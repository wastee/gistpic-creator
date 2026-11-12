//! 画布几何计算：图块位置、接缝热区、拉框相交。
//!
//! 这里只有纯数值计算，不碰界面、不碰应用状态，方便单独测试。

use gpui::{Pixels, Point, Size, point};

use crate::imaging::crop::{self, CropBand};
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

    pub fn origin(&self) -> Point<f32> {
        point(self.x, self.y)
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

/// 接缝的方向：上下相邻的两张图之间是横线，左右相邻的是竖线。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeamAxis {
    /// 两张图上下相邻，接缝横着铺开
    Vertical,
    /// 两张图左右相邻，接缝竖着铺开
    Horizontal,
}

/// 两张图之间的一条接缝。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Seam {
    /// 稳定的序号，用作界面元素 id
    pub index: usize,
    /// 接缝亮线占的长条，画在两张图的交界处
    pub rect: Rect,
    pub axis: SeamAxis,
    /// 长条中心线：上下相邻是 y，左右相邻是 x
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
    /// `hidden` 是正在拖动的那张图：排版时把它摘掉，后面的图就立刻顶上来。
    /// `viewport` 是窗口大小，用来按黄金分割算默认显示比例。
    pub fn new(
        project: &Project,
        zoom: f32,
        hidden: Option<ItemId>,
        viewport: Size<Pixels>,
    ) -> Self {
        let all = crop::resolve(project);
        // 裁剪结果仍按整个项目算（前一张的取舍会影响后一张），只是排版时跳过被拖的那张。
        let mut items: Vec<&crate::model::Item> = Vec::with_capacity(project.items.len());
        let mut inclusions = Vec::with_capacity(project.items.len());
        for (index, item) in project.items.iter().enumerate() {
            if Some(item.id) == hidden {
                continue;
            }
            items.push(item);
            inclusions.push(all[index]);
        }
        let gap_px = project.settings.gap.effective_px();

        match project.mode {
            Mode::Subtitle => {
                let mut raw: Vec<(ItemId, CropBand, u32)> = Vec::new();
                for (index, item) in items.iter().enumerate() {
                    let Some(inclusion) = inclusions.get(index).copied() else {
                        continue;
                    };
                    let Some((width, _)) = inclusion.output_size(item) else {
                        continue;
                    };
                    raw.push((item.id, inclusion.band_for(item), width.max(1)));
                }
                if raw.is_empty() {
                    return CanvasLayout {
                        entries: Vec::new(),
                        seams: Vec::new(),
                        width: 0.0,
                        height: 0.0,
                        gap_px,
                        gap_display: 0.0,
                        scale: 1.0,
                        mode: project.mode,
                    };
                }
                let output_width = raw.iter().map(|(_, _, width)| *width).max().unwrap_or(1);
                // 每张按宽度拉齐到最宽那张，高度同比换算，和 compose_subtitle 保持一致。
                let heights: Vec<u32> = raw
                    .iter()
                    .map(|(_, band, width)| {
                        (band.height() as u64 * output_width as u64 / (*width).max(1) as u64).max(1)
                            as u32
                    })
                    .collect();
                let output_height = heights
                    .iter()
                    .copied()
                    .fold(0u32, |acc, h| acc.saturating_add(h))
                    .saturating_add(gap_px.saturating_mul(raw.len().saturating_sub(1) as u32))
                    .max(1);
                let scale = tokens::display_scale(
                    output_width as f32,
                    output_height as f32,
                    zoom,
                    viewport,
                );
                let display_width = output_width as f32 * scale;
                let gap_display = gap_px as f32 * scale;
                let mut entries = Vec::with_capacity(raw.len());
                let mut y = 0.0f32;
                for (index, (id, band, _)) in raw.iter().enumerate() {
                    let display_height = heights[index] as f32 * scale;
                    entries.push(CanvasEntry {
                        id: *id,
                        band: *band,
                        rect: Rect::new(0.0, y, display_width, display_height),
                        display_width,
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
                    width: display_width,
                    height,
                    gap_px,
                    gap_display,
                    scale,
                    mode: project.mode,
                }
            }
            Mode::Layout => {
                let mut sizes = Vec::new();
                let mut included: Vec<(ItemId, CropBand)> = Vec::new();
                for (index, item) in items.iter().enumerate() {
                    let Some(inclusion) = inclusions.get(index).copied() else {
                        continue;
                    };
                    let Some(size) = inclusion.output_size(item) else {
                        continue;
                    };
                    sizes.push(size);
                    included.push((item.id, inclusion.band_for(item)));
                }
                if sizes.is_empty() {
                    return CanvasLayout {
                        entries: Vec::new(),
                        seams: Vec::new(),
                        width: 0.0,
                        height: 0.0,
                        gap_px,
                        gap_display: 0.0,
                        scale: 1.0,
                        mode: project.mode,
                    };
                }
                let result = layout::arrange(&sizes, project.settings.layout, gap_px);
                let scale = tokens::display_scale(
                    result.canvas_w as f32,
                    result.canvas_h as f32,
                    zoom,
                    viewport,
                );
                let gap_display = gap_px as f32 * scale;
                let mut entries = Vec::with_capacity(result.placements.len());
                for placement in &result.placements {
                    let Some((id, band)) = included.get(placement.item).copied() else {
                        continue;
                    };
                    entries.push(CanvasEntry {
                        id,
                        band,
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

    /// 作品上方的留白。
    ///
    /// 默认把作品在画面里上下居中（最窄的那条边居中）。作品高到会伸进底部胶囊
    /// 底下时，往上让到胶囊上方，保证不被挡住；再高就退到最小留白，靠纵向滚动看全。
    pub fn top_padding(&self, viewport: Size<Pixels>) -> f32 {
        let pad = f32::from(space::XL);
        let height = f32::from(viewport.height);
        let centred = (height - self.height) / 2.0;
        let above_capsule = height - f32::from(tokens::size::CANVAS_BOTTOM_PAD) - self.height;
        centred.min(above_capsule).max(pad)
    }

    /// 作品左上角在窗口里的位置。
    ///
    /// scroll 是滚动条偏移（向右、向下滚动时为负）。画布内容在滚动区里居中：
    /// 内容比窗口窄时按窗口居中，比窗口宽时左右各留 `CANVAS_SIDE_PAD` 像素，
    /// 靠横向滚动看全。左右要留得比上下宽，是因为作品左侧外挂着缝隙控件。
    pub fn origin(&self, viewport: Size<Pixels>, scroll: Point<Pixels>) -> Point<f32> {
        let pad = f32::from(tokens::CANVAS_SIDE_PAD);
        let content_width = (self.width + 2.0 * pad).max(f32::from(viewport.width));
        point(
            (content_width - self.width) / 2.0 + f32::from(scroll.x),
            self.top_padding(viewport) + f32::from(scroll.y),
        )
    }

    /// 图块是横向排开（看横坐标）还是上下堆叠（看纵坐标）。
    ///
    /// 用图块整体跨度的宽高比判断：左右排开的宽大于高，上下堆叠的高大于宽。
    pub fn flows_horizontally(&self) -> bool {
        if self.entries.is_empty() {
            return false;
        }
        let min_x = self
            .entries
            .iter()
            .map(|entry| entry.rect.x)
            .fold(f32::INFINITY, f32::min);
        let max_x = self
            .entries
            .iter()
            .map(|entry| entry.rect.right())
            .fold(f32::NEG_INFINITY, f32::max);
        let min_y = self
            .entries
            .iter()
            .map(|entry| entry.rect.y)
            .fold(f32::INFINITY, f32::min);
        let max_y = self
            .entries
            .iter()
            .map(|entry| entry.rect.bottom())
            .fold(f32::NEG_INFINITY, f32::max);
        (max_x - min_x) > (max_y - min_y)
    }

    /// 拖动排序时，指针落点对应的插入位置。
    ///
    /// 返回「该插到哪张图前面」，`None` 表示插到末尾。图块已按项目顺序排好；
    /// 判断方向看整体形状：上下堆叠的看纵坐标，左右排开的看横坐标，网格取跨度大的方向。
    pub fn insertion_target(&self, position: Point<f32>) -> Option<ItemId> {
        if self.entries.is_empty() {
            return None;
        }
        let horizontal = self.flows_horizontally();
        for entry in &self.entries {
            let center = if horizontal {
                entry.rect.x + entry.rect.w / 2.0
            } else {
                entry.rect.y + entry.rect.h / 2.0
            };
            let coord = if horizontal { position.x } else { position.y };
            if coord < center {
                return Some(entry.id);
            }
        }
        None
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

    /// 作品左侧那个缝隙与颜色控件占的矩形，用作品内容坐标。
    ///
    /// 只有作品里至少有一条接缝时才画这个控件，所以没有接缝就返回 `None`。
    /// 位置只看作品自己的高度：纵向在作品上居中，横向贴在作品左边缘外，
    /// 不跟任何一条接缝对齐。
    pub fn gap_control_rect(&self) -> Option<Rect> {
        if self.seams.is_empty() {
            return None;
        }
        Some(gap_control_rect(self.height))
    }
}

/// 作品左侧缝隙控件的矩形，入参是作品高度。
pub fn gap_control_rect(artwork_height: f32) -> Rect {
    let w = f32::from(tokens::GAP_CONTROL_W);
    let h = f32::from(tokens::GAP_CONTROL_H);
    Rect::new(
        -(f32::from(tokens::GAP_CONTROL_GAP) + w),
        ((artwork_height - h) / 2.0).max(0.0),
        w,
        h,
    )
}

/// 算出所有相邻图块之间的接缝亮线。
///
/// 上下相邻的两张图给一条横向接缝，左右相邻的给一条竖向接缝。
/// 亮线以缝隙中线为中心，最宽 12 像素。
pub fn compute_seams(entries: &[CanvasEntry], gap_display: f32) -> Vec<Seam> {
    let mut seams = Vec::new();
    // 亮线以缝隙中线为中心，宽度不超过缝隙本身太多：
    // 没有缝隙时只剩交界处 8 像素。
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
    use gpui::px;

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::new(x, y, w, h)
    }

    fn entry(id: ItemId, x: f32, y: f32, w: f32, h: f32) -> CanvasEntry {
        CanvasEntry {
            id,
            band: CropBand::rows(200, 0, 100),
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

    #[test]
    fn origin_follows_both_scroll_axes() {
        let layout = CanvasLayout {
            entries: Vec::new(),
            seams: Vec::new(),
            width: 860.0,
            height: 400.0,
            gap_px: 0,
            gap_display: 0.0,
            scale: 1.0,
            mode: Mode::Layout,
        };
        // 窗口比作品宽：作品左右居中
        let origin = layout.origin(
            Size {
                width: px(1240.),
                height: px(800.),
            },
            point(px(0.), px(0.)),
        );
        assert!((origin.x - 190.0).abs() < 1e-3);
        // 纵向同样居中：窗口高 800，作品高 400，上留白 (800-400)/2
        assert!((origin.y - 200.0).abs() < 1e-3);

        // 作品比窗口宽：靠横向滚动看，滚动偏移两个轴都要跟着走
        let origin = layout.origin(
            Size {
                width: px(400.),
                height: px(800.),
            },
            point(px(-100.), px(-40.)),
        );
        assert!((origin.x - (48.0 - 100.0)).abs() < 1e-3);
        assert!((origin.y - (200.0 - 40.0)).abs() < 1e-3);
    }

    #[test]
    fn origin_keeps_minimum_padding_when_artwork_is_taller_than_viewport() {
        let layout = CanvasLayout {
            entries: Vec::new(),
            seams: Vec::new(),
            width: 860.0,
            height: 1720.0,
            gap_px: 0,
            gap_display: 0.0,
            scale: 1.0,
            mode: Mode::Layout,
        };
        // 作品比可视高度高：不留负留白，维持最小留白，靠纵向滚动看全。
        let origin = layout.origin(
            Size {
                width: px(1920.),
                height: px(1080.),
            },
            point(px(0.), px(0.)),
        );
        assert!((origin.y - 24.0).abs() < 1e-3);
    }

    #[test]
    fn narrow_image_is_centered_on_its_narrow_axis() {
        // 10×10000 的窄图在竖排下：宽度 10 远小于窗口，横向必须居中。
        let layout = CanvasLayout {
            entries: Vec::new(),
            seams: Vec::new(),
            width: 10.0,
            height: 860.0,
            gap_px: 0,
            gap_display: 0.0,
            scale: 1.0,
            mode: Mode::Layout,
        };
        let viewport = Size {
            width: px(1920.),
            height: px(1080.),
        };
        let origin = layout.origin(viewport, point(px(0.), px(0.)));
        assert!((origin.x - (1920.0 - 10.0) / 2.0).abs() < 1e-3);
    }

    fn layout_of(entries: Vec<CanvasEntry>) -> CanvasLayout {
        CanvasLayout {
            entries,
            seams: Vec::new(),
            width: 0.0,
            height: 0.0,
            gap_px: 0,
            gap_display: 0.0,
            scale: 1.0,
            mode: Mode::Subtitle,
        }
    }

    #[test]
    fn insertion_target_reads_the_vertical_stack_by_y() {
        // 上下堆叠：指针在上半段插入到第一张前，中段插入到第二张前，越过全部就插到末尾。
        let layout = layout_of(vec![
            entry(1, 0.0, 0.0, 100.0, 100.0),
            entry(2, 0.0, 100.0, 100.0, 100.0),
        ]);
        assert_eq!(layout.insertion_target(point(50.0, 40.0)), Some(1));
        assert_eq!(layout.insertion_target(point(50.0, 140.0)), Some(2));
        assert_eq!(layout.insertion_target(point(50.0, 260.0)), None);
    }

    #[test]
    fn insertion_target_reads_a_row_by_x() {
        // 左右排开：按横坐标判断，越过最后一张就插到末尾。
        let layout = layout_of(vec![
            entry(1, 0.0, 0.0, 100.0, 100.0),
            entry(2, 100.0, 0.0, 100.0, 100.0),
        ]);
        assert_eq!(layout.insertion_target(point(40.0, 50.0)), Some(1));
        assert_eq!(layout.insertion_target(point(140.0, 50.0)), Some(2));
        assert_eq!(layout.insertion_target(point(260.0, 50.0)), None);
    }

    /// 按图块算出接缝，拼一份带接缝的排版。
    fn artwork_with_seams(entries: Vec<CanvasEntry>, width: f32, height: f32) -> CanvasLayout {
        let seams = compute_seams(&entries, 0.0);
        CanvasLayout {
            entries,
            seams,
            width,
            height,
            gap_px: 0,
            gap_display: 0.0,
            scale: 1.0,
            mode: Mode::Subtitle,
        }
    }

    #[test]
    fn gap_control_sits_left_of_the_artwork_and_is_centred() {
        let layout = artwork_with_seams(
            vec![
                entry(1, 0.0, 0.0, 100.0, 100.0),
                entry(2, 0.0, 100.0, 100.0, 100.0),
            ],
            100.0,
            200.0,
        );
        let control = layout.gap_control_rect().expect("有接缝就该有控件");
        assert!(control.right() < 0.0, "控件要挂在作品左边缘外：{control:?}");
        assert!(
            (control.right() - (-8.0)).abs() < 1e-3,
            "控件右边缘离作品左边缘 8 像素：{control:?}"
        );
        assert!(
            (control.y + control.h / 2.0 - 100.0).abs() < 1e-3,
            "控件纵向要对齐作品高度一半：{control:?}"
        );
    }

    #[test]
    fn gap_control_is_absent_with_fewer_than_two_images() {
        let layout = layout_of(vec![entry(1, 0.0, 0.0, 100.0, 100.0)]);
        assert!(layout.seams.is_empty(), "单张图没有接缝");
        assert_eq!(layout.gap_control_rect(), None);
    }

    #[test]
    fn gap_control_stays_inside_a_short_artwork() {
        // 作品比控件还矮：控件顶边不能跑到作品上面去。
        let layout = artwork_with_seams(
            vec![
                entry(1, 0.0, 0.0, 100.0, 10.0),
                entry(2, 0.0, 10.0, 100.0, 10.0),
            ],
            100.0,
            20.0,
        );
        let control = layout.gap_control_rect().expect("有接缝就该有控件");
        assert_eq!(control.y, 0.0, "矮作品里控件顶边贴住 0：{control:?}");
    }

    #[test]
    fn origin_leaves_room_for_the_gap_control() {
        // 窗口比作品窄时，左右各留 48 像素给作品左侧的控件，靠横向滚动看全。
        let layout = CanvasLayout {
            entries: Vec::new(),
            seams: Vec::new(),
            width: 860.0,
            height: 400.0,
            gap_px: 0,
            gap_display: 0.0,
            scale: 1.0,
            mode: Mode::Layout,
        };
        let origin = layout.origin(
            Size {
                width: px(400.),
                height: px(800.),
            },
            point(px(0.), px(0.)),
        );
        assert!(
            (origin.x - 48.0).abs() < 1e-3,
            "作品比窗口宽时应留出控件空位，实际 {}",
            origin.x
        );
    }
}
