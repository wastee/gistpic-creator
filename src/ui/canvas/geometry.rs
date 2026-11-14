//! 画布几何计算：图块位置、接缝热区、拉框相交。
//!
//! 这里只有纯数值计算，不碰界面、不碰应用状态，方便单独测试。

use std::collections::HashMap;

use gpui::{Pixels, Point, Size, point};

use crate::imaging::crop::{self, CropBand};
use crate::imaging::layout;
use crate::app::ManualEdge;
use crate::model::{ItemId, Mode, Project};
use crate::ui::tokens::{self, SEAM_HOTZONE, space};

/// 指针贴近视口边缘时的自动滚动强度，每个轴独立，取值为 -1.0 到 1.0。
///
/// 正数表示把内容往右下推（滚动偏移回升到 0），负数表示往左上推，0 表示不在边缘带里。
/// 绝对值随指针越靠边而增大，拖动时是平滑加速，不会一进边缘就满速。
pub fn auto_scroll_intensity(pointer: Point<f32>, viewport: Size<f32>, edge: f32) -> (f32, f32) {
    (
        axis_scroll_intensity(pointer.x, viewport.width, edge),
        axis_scroll_intensity(pointer.y, viewport.height, edge),
    )
}

fn axis_scroll_intensity(position: f32, extent: f32, edge: f32) -> f32 {
    if extent <= 0.0 || edge <= 0.0 {
        return 0.0;
    }
    // 视口很窄时边缘带不能宽过一半，否则整块视口都在推。
    let band = edge.min(extent / 2.0);
    if position < band {
        ((band - position.max(0.0)) / band).clamp(0.0, 1.0)
    } else if position > extent - band {
        (-((position.min(extent) - (extent - band)) / band)).clamp(-1.0, 0.0)
    } else {
        0.0
    }
}

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
    /// 这一块是拖动排序时的落点占位：按临时顺序排在图块之间，画得半透明。
    pub is_placeholder: bool,
}

/// 拖动排序时，排版要如何对待被拖的那张图。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum DragPreview {
    /// 没有拖动
    #[default]
    None,
    /// 暂时摘掉（算落点、换算坐标时用「其余图片已经合拢」的排版）
    Hidden(ItemId),
    /// 按临时顺序保留在图块之间，画成半透明占位
    Placeholder { id: ItemId, before: Option<ItemId> },
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
    /// 这个比例是自动适应算出来的，还是用户定死的。
    pub view: tokens::ViewScale,
    pub mode: Mode,
    /// 裁剪拖动期间冻结的上方留白。
    ///
    /// 默认按作品高度上下居中，作品高度一变留白就跟着变，会让整个画面在拖动时
    /// 以约半倍指针位移漂移。拖动开始时冻结一次，所有坐标换算看同一份留白。
    frozen_top_padding: Option<f32>,
    /// 裁剪拖动期间撑住滚动内容的下限 `(横向内容宽, 纵向内容高)`。
    ///
    /// 裁剪让作品变小，滚动区的内容跟着变矮，GPUI 每一帧重排都会把滚动偏移夹
    /// 回新范围（`vendor/gpui/src/elements/div.rs` 的 `clamp_scroll_position`），
    /// 画面于是被一股一股地拽向作品开头。拖动期间按按下时的范围撑住内容尺寸，
    /// 夹紧就不会发生。
    frozen_content: Option<(f32, f32)>,
}

impl CanvasLayout {
    /// `hidden` 是正在拖动的那张图：排版时把它摘掉，后面的图就立刻顶上来。
    /// `view` 决定显示比例怎么来：自动适应按 `viewport` 算，手动比例直接用它自己。
    pub fn new(
        project: &Project,
        view: tokens::ViewScale,
        preview: DragPreview,
        viewport: Size<Pixels>,
    ) -> Self {
        let all = crop::resolve(project);
        // 先按真实顺序把每张图和它的裁剪结果配好；预览只调整顺序、不重算裁剪
        // —— 字幕模式「第一张保留主体」必须按真实首张判定，否则拖动时会闪。
        let mut paired: Vec<(&crate::model::Item, crop::Inclusion)> =
            project.items.iter().zip(all).collect();
        let placeholder_id = match preview {
            DragPreview::None => None,
            DragPreview::Hidden(id) => {
                paired.retain(|(item, _)| item.id != id);
                None
            }
            DragPreview::Placeholder { id, before } => {
                let ids: Vec<ItemId> = paired.iter().map(|(item, _)| item.id).collect();
                let order = crate::model::reorder_ids(&ids, &[id], before);
                let mut by_id: HashMap<ItemId, (&crate::model::Item, crop::Inclusion)> =
                    paired.into_iter().map(|(item, inc)| (item.id, (item, inc))).collect();
                paired = order
                    .into_iter()
                    .filter_map(|id| by_id.remove(&id))
                    .collect();
                Some(id)
            }
        };
        let items: Vec<&crate::model::Item> = paired.iter().map(|(item, _)| *item).collect();
        let inclusions: Vec<crop::Inclusion> = paired.iter().map(|(_, inc)| *inc).collect();
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
                        view,
                        mode: project.mode,
                        frozen_top_padding: None,
                        frozen_content: None,
                    };
                }
                // 需求 7：「适合画布」只看每张图自己裁剪后可见带的尺寸，不看整幅
                // 排版画布，所以这里把每张图的可见尺寸收一遍当基准。
                let basis = tokens::fit_basis(raw.iter().map(|(_, band, _)| {
                    (band.width() as f32, band.height() as f32)
                }));
                let output_width = raw.iter().map(|(_, _, width)| *width).max().unwrap_or(1);
                // 每张按宽度拉齐到最宽那张，高度同比换算，和 compose_subtitle 保持一致。
                let heights: Vec<u32> = raw
                    .iter()
                    .map(|(_, band, width)| {
                        (band.height() as u64 * output_width as u64 / (*width).max(1) as u64).max(1)
                            as u32
                    })
                    .collect();
                // 整幅成品的总高不再参与「适合画布」的计算：需求 7 之后基准只看
                // 每张图自己的可见尺寸，横排竖排算出来一样大。
                let scale = view.scale(basis, viewport);
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
                        is_placeholder: placeholder_id == Some(*id),
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
                    view,
                    mode: project.mode,
                    frozen_top_padding: None,
                    frozen_content: None,
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
                        view,
                        mode: project.mode,
                        frozen_top_padding: None,
                        frozen_content: None,
                    };
                }
                // 需求 7：基准取每张图裁剪后可见带的最短边里最大的那个，和横排竖排
                // 都无关，所以同一批图换排版方向显示出来一样大。
                let basis = tokens::fit_basis(
                    sizes
                        .iter()
                        .map(|(width, height)| (*width as f32, *height as f32)),
                );
                let result = layout::arrange(&sizes, project.settings.layout, gap_px);
                let scale = view.scale(basis, viewport);
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
                        is_placeholder: placeholder_id == Some(id),
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
                    view,
                    mode: project.mode,
                    frozen_top_padding: None,
                    frozen_content: None,
                }
            }
        }
    }

    /// 作品上方的留白。
    ///
    /// 默认把作品在画面里上下居中（最窄的那条边居中）。作品高到会伸进底部胶囊
    /// 底下时，往上让到胶囊上方，保证不被挡住；再高就退到最小留白，靠纵向滚动看全。
    /// 拖动裁剪线时冻结上方留白，返回值供 `top_padding` / `origin` 共用。
    pub fn freeze_top_padding(mut self, value: f32) -> Self {
        self.frozen_top_padding = Some(value);
        self
    }

    /// 裁剪拖动期间撑住滚动内容尺寸，参数是两个方向的可滚动范围。
    ///
    /// 传进来的范围会换算成内容尺寸下限：内容宽不低于「视口宽 + 横向范围」，
    /// 内容高不低于「视口高 + 纵向范围」。这样即使作品被裁小了，滚动区也认为
    /// 内容还是原来那么大，偏移不会被夹，画面不会突然跳。
    pub fn freeze_scroll_extent(mut self, max_scroll: (f32, f32)) -> Self {
        self.frozen_content = Some(max_scroll);
        self
    }

    /// 需求 8：图块 `entry` 的这条裁剪边是不是紧贴着另一张图，只有这种边的
    /// 热区才需要往图内让出死区。
    ///
    /// 两条都要满足才算：
    /// 1. 间距为 0。间距大于 0 时两块图之间隔着可见缝隙，热区各自待在自己块里
    ///    根本不会重叠，留死区只会白缩小可抓范围；接缝的相邻判据带 0.75 容差，
    ///    间距大于 0 时照样会算出接缝，所以这一条不能省。
    /// 2. 这条边确实有紧贴的邻居。外沿（第一张的顶边、最后一张的底边、单张图
    ///    的四条边）没有歧义，不用缩影。
    ///
    /// 判据是接缝的中心线正好落在这条边上，而且接缝长条跟这条边有一段重叠
    /// （否则那条接缝属于同一水平线或竖直线上别的图块）。
    pub fn edge_touches_neighbour(&self, entry: &CanvasEntry, edge: ManualEdge) -> bool {
        if self.gap_px != 0 {
            return false;
        }
        let rect = entry.rect;
        let tol = 0.75;
        self.seams.iter().any(|seam| {
            let spans = match seam.axis {
                SeamAxis::Vertical => {
                    rect.right() > seam.rect.x + tol && seam.rect.right() > rect.x + tol
                }
                SeamAxis::Horizontal => {
                    rect.bottom() > seam.rect.y + tol && seam.rect.bottom() > rect.y + tol
                }
            };
            if !spans {
                return false;
            }
            match (seam.axis, edge) {
                // 上下相邻：接缝横着铺开，中心线落在上边或下边上。
                (SeamAxis::Vertical, ManualEdge::Top) => (rect.y - seam.center).abs() < tol,
                (SeamAxis::Vertical, ManualEdge::Bottom) => (rect.bottom() - seam.center).abs() < tol,
                // 左右相邻：接缝竖着铺开，中心线落在左边或右边上。
                (SeamAxis::Horizontal, ManualEdge::Left) => (rect.x - seam.center).abs() < tol,
                (SeamAxis::Horizontal, ManualEdge::Right) => (rect.right() - seam.center).abs() < tol,
                // 纵向的边配横向的接缝这类组合不可能出现，直接不算。
                _ => false,
            }
        })
    }

    /// 滚动区的内容尺寸下限，给渲染层撑住滚动范围和居中基准。
    ///
    /// 返回 `(内容宽下限, 内容高下限)`；没有冻结时都是 0，等于不设下限。
    pub fn scroll_content_floor(&self, viewport: Size<Pixels>) -> (f32, f32) {
        match self.frozen_content {
            Some((max_x, max_y)) => (
                f32::from(viewport.width) + max_x,
                f32::from(viewport.height) + max_y,
            ),
            None => (0.0, 0.0),
        }
    }

    pub fn top_padding(&self, viewport: Size<Pixels>) -> f32 {
        if let Some(frozen) = self.frozen_top_padding {
            return frozen;
        }
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

    /// 这个排版在给定视口里最多能滚多远（正值，往左上滚）。
    ///
    /// 公式和 `origin` 一致：横向内容宽取「作品宽 + 两侧留白」与窗口宽的较大者，
    /// 纵向内容高取「上方留白 + 作品高 + 底部留白」。
    ///
    /// 不读 `ScrollHandle::max_offset()`：那是上一帧布局量出来的值，比例刚改完
    /// 那一帧读到的还是旧范围的数，按它夹紧会把滚动位置截在半路。
    pub fn max_scroll(&self, viewport: Size<Pixels>) -> (f32, f32) {
        let content_width = (self.width + 2.0 * f32::from(tokens::CANVAS_SIDE_PAD))
            .max(f32::from(viewport.width));
        let content_height = self.top_padding(viewport)
            + self.height
            + f32::from(tokens::size::CANVAS_BOTTOM_PAD);
        let (max_x, max_y) = (
            (content_width - f32::from(viewport.width)).max(0.0),
            (content_height - f32::from(viewport.height)).max(0.0),
        );
        // 裁剪拖动期间不支持缩小的范围：渲染层按 `scroll_content_floor` 撑住
        // 内容尺寸，这里返回同一个下限，两边的滚动范围才一致。
        let (floor_x, floor_y) = self.frozen_content.unwrap_or((0.0, 0.0));
        (max_x.max(floor_x), max_y.max(floor_y))
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
        // 二维网格里把指针投影到主轴会把同一行/同一列的图都算错，
        // 所以指针落在某块图上时，按那块图自己的中线决定插到它前还是后。
        if let Some(entry) = self.entries.iter().find(|entry| entry.rect.contains(position)) {
            let center = if horizontal {
                entry.rect.x + entry.rect.w / 2.0
            } else {
                entry.rect.y + entry.rect.h / 2.0
            };
            let coord = if horizontal { position.x } else { position.y };
            if coord < center {
                return Some(entry.id);
            }
            let index = self
                .entries
                .iter()
                .position(|candidate| candidate.id == entry.id)?;
            return self.entries.get(index + 1).map(|next| next.id);
        }
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

    /// 作品左侧那个缝隙与颜色控件占的矩形，用窗口坐标。
    ///
    /// 只有作品里至少有一条接缝时才画这个控件，所以没有接缝就返回 `None`。
    ///
    /// 需求 12：控件固定在「看得见的画布区域」的垂直中央，不再挂在作品上。
    /// 作品很高、需要滚动的时候，挂在作品上会让控件跟着滚出屏幕；固定在
    /// 视口中央才一直够得着。横向仍然贴着作品的左边缘。
    pub fn gap_control_rect(&self, viewport: Size<Pixels>, scroll: Point<Pixels>) -> Option<Rect> {
        if self.seams.is_empty() {
            return None;
        }
        let origin = self.origin(viewport, scroll);
        Some(gap_control_rect(
            f32::from(viewport.height),
            origin.x,
            f32::from(viewport.width),
        ))
    }
}

/// 作品左侧缝隙控件的矩形，用窗口坐标。
///
/// `viewport_height` 是画布可视区域的高度，控件在它中间垂直居中；
/// `artwork_x` 是作品左边缘的窗口横坐标，控件贴在它左边；
/// `viewport_width` 用来把控件夹进可视区域，窗口很窄时也看得见。
pub fn gap_control_rect(viewport_height: f32, artwork_x: f32, viewport_width: f32) -> Rect {
    let w = f32::from(tokens::GAP_CONTROL_W);
    let h = f32::from(tokens::GAP_CONTROL_H);
    let x = artwork_x - f32::from(tokens::GAP_CONTROL_GAP) - w;
    Rect::new(
        x.clamp(0.0, (viewport_width - w).max(0.0)),
        ((viewport_height - h) / 2.0).max(0.0),
        w,
        h,
    )
}

/// 算出所有相邻图块之间的接缝亮线。
///
/// 上下相邻的两张图给一条横向接缝，左右相邻的给一条竖向接缝。
/// 亮线以缝隙中线为中心，最宽 12 像素。
pub fn compute_seams(entries: &[CanvasEntry], gap_display: f32) -> Vec<Seam> {
    // 亮线以缝隙中线为中心，宽度不超过缝隙本身太多：
    // 没有缝隙时只剩交界处 8 像素。
    let half = (gap_display * 0.5).clamp(4.0, f32::from(SEAM_HOTZONE) / 2.0);

    // 只找「正好在下方 / 右方」的邻居：按量化后的 y、x 建索引，把 O(n²)
    // 的两两比较降到 O(n)。容差 0.75 小于一个桶，查相邻三个桶即可覆盖。
    let mut by_y: HashMap<i64, Vec<usize>> = HashMap::with_capacity(entries.len());
    let mut by_x: HashMap<i64, Vec<usize>> = HashMap::with_capacity(entries.len());
    for (i, entry) in entries.iter().enumerate() {
        by_y.entry(quantize(entry.rect.y)).or_default().push(i);
        by_x.entry(quantize(entry.rect.x)).or_default().push(i);
    }

    let mut pairs: Vec<(usize, usize)> = Vec::new();
    for (i, entry) in entries.iter().enumerate() {
        let a = entry.rect;
        collect_candidates(&by_y, a.bottom() + gap_display, i, &mut pairs);
        collect_candidates(&by_x, a.right() + gap_display, i, &mut pairs);
    }
    // 排序去重后按 (i, j) 顺序生成，接缝编号与原两两遍历完全一致。
    pairs.sort_unstable();
    pairs.dedup();

    let mut seams = Vec::new();
    let mut index = 0;
    for (i, j) in pairs {
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
    seams
}

/// 按整数像素分桶，用于接缝邻居的常量时间定位。
fn quantize(value: f32) -> i64 {
    value.round() as i64
}

/// 收集可能在目标坐标这条轴线上的、下标大于 i 的邻居。
fn collect_candidates(
    by_axis: &HashMap<i64, Vec<usize>>,
    target: f32,
    i: usize,
    out: &mut Vec<(usize, usize)>,
) {
    let bucket = quantize(target);
    for key in [bucket - 1, bucket, bucket + 1] {
        if let Some(indices) = by_axis.get(&key) {
            for &j in indices {
                if j > i {
                    out.push((i, j));
                }
            }
        }
    }
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
            is_placeholder: false,
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

    /// 需求 8：只有「间距为 0 + 这条边真有紧贴的邻居」才留死区。
    #[test]
    fn seam_deadzone_only_applies_to_edges_touching_a_neighbour() {
        let layout = |gap_px: u32, entries: Vec<CanvasEntry>| {
            // 接缝按真实的显示间距算：间距大于 0 时接缝中心落在两块图之间。
            let gap_display = gap_px as f32;
            let seams = compute_seams(&entries, gap_display);
            CanvasLayout {
                entries,
                seams,
                width: 100.0,
                height: 200.0,
                gap_px,
                gap_display,
                scale: 1.0,
                view: tokens::ViewScale::fit(1.0),
                mode: Mode::Subtitle,
                frozen_top_padding: None,
                frozen_content: None,
            }
        };
        // 上下紧贴：第一张的底边和第二张的顶边都挨着接缝。
        let stacked = layout(
            0,
            vec![entry(1, 0.0, 0.0, 100.0, 100.0), entry(2, 0.0, 100.0, 100.0, 100.0)],
        );
        let first = stacked.entries[0];
        let second = stacked.entries[1];
        assert!(stacked.edge_touches_neighbour(&first, ManualEdge::Bottom));
        assert!(stacked.edge_touches_neighbour(&second, ManualEdge::Top));
        // 外沿没有邻居，不留死区。
        assert!(!stacked.edge_touches_neighbour(&first, ManualEdge::Top));
        assert!(!stacked.edge_touches_neighbour(&second, ManualEdge::Bottom));
        assert!(!stacked.edge_touches_neighbour(&first, ManualEdge::Left));

        // 左右紧贴：第一张的右边和第二张的左边都挨着接缝。
        let side_by_side = layout(
            0,
            vec![entry(1, 0.0, 0.0, 100.0, 80.0), entry(2, 100.0, 0.0, 100.0, 80.0)],
        );
        let left = side_by_side.entries[0];
        let right = side_by_side.entries[1];
        assert!(side_by_side.edge_touches_neighbour(&left, ManualEdge::Right));
        assert!(side_by_side.edge_touches_neighbour(&right, ManualEdge::Left));
        assert!(!side_by_side.edge_touches_neighbour(&left, ManualEdge::Left));

        // 间距大于 0：接缝照样算得出来，但不能留死区。
        let spaced = layout(
            8,
            vec![entry(1, 0.0, 0.0, 100.0, 100.0), entry(2, 0.0, 108.0, 100.0, 100.0)],
        );
        assert!(
            !spaced.seams.is_empty(),
            "间距大于 0 时接缝仍然存在，所以 gap_px 这一条不能省"
        );
        assert!(!spaced.edge_touches_neighbour(&spaced.entries[0], ManualEdge::Bottom));
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
            view: tokens::ViewScale::fit(1.0),
            mode: Mode::Layout,
            frozen_top_padding: None,
            frozen_content: None,
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
            view: tokens::ViewScale::fit(1.0),
            mode: Mode::Layout,
            frozen_top_padding: None,
            frozen_content: None,
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

    /// 滚动范围按同一份排版算，不依赖上一帧量出来的值。
    #[test]
    fn max_scroll_matches_the_rendered_content_box() {
        let layout = CanvasLayout {
            entries: Vec::new(),
            seams: Vec::new(),
            width: 4000.0,
            height: 3000.0,
            gap_px: 0,
            gap_display: 0.0,
            scale: 1.0,
            view: tokens::ViewScale::manual(1.0),
            mode: Mode::Layout,
            frozen_top_padding: None,
            frozen_content: None,
        };
        let viewport = Size {
            width: px(1280.),
            height: px(860.),
        };
        let (max_x, max_y) = layout.max_scroll(viewport);
        assert!((max_x - (4000.0 + 96.0 - 1280.0)).abs() < 1e-3, "横向：{max_x}");
        assert!(max_y > 0.0, "纵向内容比窗口高，应能滚动：{max_y}");
        // 作品比窗口小时两个方向都滚不动。
        let small = CanvasLayout { width: 400.0, height: 300.0, ..layout };
        assert_eq!(small.max_scroll(viewport), (0.0, 0.0));
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
            view: tokens::ViewScale::fit(1.0),
            mode: Mode::Layout,
            frozen_top_padding: None,
            frozen_content: None,
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
            view: tokens::ViewScale::fit(1.0),
            mode: Mode::Subtitle,
            frozen_top_padding: None,
            frozen_content: None,
        }
    }

    #[test]
    fn compute_seams_has_no_duplicates_on_a_long_stack() {
        // 50 张纵向相邻的图块：接缝数等于相邻对数，编号也不重复。
        let entries: Vec<CanvasEntry> = (0..50)
            .map(|index| {
                entry(
                    index as ItemId + 1,
                    0.0,
                    index as f32 * 100.0,
                    100.0,
                    100.0,
                )
            })
            .collect();
        let seams = compute_seams(&entries, 0.0);
        assert_eq!(seams.len(), 49, "50 张竖排应有 49 条接缝");
        let mut indexes: Vec<usize> = seams.iter().map(|seam| seam.index).collect();
        indexes.sort_unstable();
        indexes.dedup();
        assert_eq!(indexes.len(), 49, "接缝编号不能重复");
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
    fn auto_scroll_pushes_away_from_the_edge_near_the_pointer() {
        let viewport: Size<f32> = gpui::size(1000.0, 800.0);
        // 视口中间不滚。
        assert_eq!(
            auto_scroll_intensity(point(500.0, 400.0), viewport, 96.0),
            (0.0, 0.0)
        );
        // 贴顶、贴左：内容往右下推，强度接近满格。
        let (x, y) = auto_scroll_intensity(point(10.0, 10.0), viewport, 96.0);
        assert!(x > 0.8 && y > 0.8, "左上角应接近满速：{x} {y}");
        // 贴底、贴右：内容往左上推。
        let (x, y) = auto_scroll_intensity(point(990.0, 790.0), viewport, 96.0);
        assert!(x < -0.8 && y < -0.8, "右下角应接近满速：{x} {y}");
        // 正好踩在边缘带的边界上时不滚。
        assert_eq!(
            auto_scroll_intensity(point(96.0, 704.0), viewport, 96.0),
            (0.0, 0.0)
        );
        // 指针跑到视口外，按满速推着走。
        assert_eq!(
            auto_scroll_intensity(point(-50.0, 900.0), viewport, 96.0),
            (1.0, -1.0)
        );
    }

    #[test]
    fn insertion_target_follows_the_block_under_the_pointer_in_a_grid() {
        // 2×2 网格：只看整体纵向投影的话，第二行右半边会被算成「最后面」。
        let layout = layout_of(vec![
            entry(1, 0.0, 0.0, 100.0, 100.0),
            entry(2, 100.0, 0.0, 100.0, 100.0),
            entry(3, 0.0, 100.0, 100.0, 100.0),
            entry(4, 100.0, 100.0, 100.0, 100.0),
        ]);
        assert_eq!(layout.insertion_target(point(40.0, 130.0)), Some(3));
        assert_eq!(
            layout.insertion_target(point(40.0, 170.0)),
            Some(4),
            "指针在第三张下半，应该插到第四张前"
        );
        assert_eq!(layout.insertion_target(point(160.0, 170.0)), None);
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
            view: tokens::ViewScale::fit(1.0),
            mode: Mode::Subtitle,
            frozen_top_padding: None,
            frozen_content: None,
        }
    }

    /// 需求 12：控件贴在作品左边缘外，纵向在可视区域的正中央。
    #[test]
    fn gap_control_sits_left_of_the_artwork_and_is_centred_in_the_viewport() {
        let layout = artwork_with_seams(
            vec![
                entry(1, 0.0, 0.0, 100.0, 100.0),
                entry(2, 0.0, 100.0, 100.0, 100.0),
            ],
            100.0,
            200.0,
        );
        let viewport = Size {
            width: px(800.),
            height: px(600.),
        };
        let control = layout
            .gap_control_rect(viewport, point(px(0.), px(0.)))
            .expect("有接缝就该有控件");
        // 作品宽 100、视口宽 800：作品左边缘在 (800-100)/2 = 350。
        let artwork_left = 350.0;
        assert!(
            (control.right() - (artwork_left - 8.0)).abs() < 1e-3,
            "控件右边缘离作品左边缘 8 像素：{control:?}"
        );
        assert!(
            (control.y + control.h / 2.0 - 300.0).abs() < 1e-3,
            "控件纵向应在可视区域正中央：{control:?}"
        );
    }

    /// 需求 12：作品很高、滚动到底时，控件仍留在可视区域中央。
    #[test]
    fn gap_control_stays_centred_while_the_artwork_scrolls() {
        let layout = artwork_with_seams(
            vec![
                entry(1, 0.0, 0.0, 100.0, 1000.0),
                entry(2, 0.0, 1000.0, 100.0, 1000.0),
            ],
            100.0,
            2000.0,
        );
        let viewport = Size {
            width: px(800.),
            height: px(600.),
        };
        let top = layout
            .gap_control_rect(viewport, point(px(0.), px(0.)))
            .expect("有接缝就该有控件");
        let scrolled = layout
            .gap_control_rect(viewport, point(px(0.), px(-900.)))
            .expect("有接缝就该有控件");
        assert!(
            (top.y - scrolled.y).abs() < 1e-6,
            "滚动不该让控件跟着走：{top:?} -> {scrolled:?}"
        );
        assert!(
            (scrolled.y + scrolled.h / 2.0 - 300.0).abs() < 1e-3,
            "滚动后控件仍在可视区域中央：{scrolled:?}"
        );
    }

    #[test]
    fn gap_control_is_absent_with_fewer_than_two_images() {
        let layout = layout_of(vec![entry(1, 0.0, 0.0, 100.0, 100.0)]);
        assert!(layout.seams.is_empty(), "单张图没有接缝");
        let viewport = Size {
            width: px(800.),
            height: px(600.),
        };
        assert_eq!(
            layout.gap_control_rect(viewport, point(px(0.), px(0.))),
            None
        );
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
            view: tokens::ViewScale::fit(1.0),
            mode: Mode::Layout,
            frozen_top_padding: None,
            frozen_content: None,
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
