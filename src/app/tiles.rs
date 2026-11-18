//! 屏幕可见图块：把一张图在屏幕上看得到的部分切成 512×512 物理像素的块。
//!
//! 这一层只有纯数值和缓存规则，不碰界面也不碰应用状态：
//!
//! - 一张图当前这一代图块按什么规格生成（显示尺寸、物理尺寸）；
//! - 屏幕上此刻必须画哪几块、还缺哪几块要后台生成；
//! - 显示比例或系统缩放变了什么时候换代号（代次）；
//! - 旧代次的块只顶到新代次补齐为止，屏幕外的块当场释放。
//!
//! 画布只负责把 [`TileGrid::render_tiles`] 给出的矩形和位图贴上去。
//!
//! # 为什么按整张原图切，不按裁剪带切
//!
//! 按整张原图切，规格只跟「原图尺寸 × 画布显示比例」有关。裁剪带一变（切模式、
//! 拖裁剪线、换排版）规格不变，屏幕上的图块原样接着用，不用重新解码、重新缩放，
//! 切换和拖动不会先糊一下再变清楚。看得见的那部分在画的时候按裁剪带裁掉，只生成
//! 屏幕内那几块，生成的像素量仍然跟屏幕上看到的一样多。

use std::collections::HashMap;
use std::sync::Arc;

use gpui::RenderImage;

use crate::imaging::render;
use crate::model::ItemId;
use crate::ui::canvas::Rect;

/// 一块图块在物理像素网格里的边长，和 [`render::TILE_SIZE`] 是同一个数。
pub const TILE_SIZE: u32 = render::TILE_SIZE;

/// 一张图当前这一代图块按什么规格生成。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TileSpec {
    pub item: ItemId,
    /// 整张原图在屏幕上的逻辑宽高（已经乘过画布显示比例）。
    pub display_width: f32,
    pub display_height: f32,
    /// 目标物理像素宽高：显示尺寸乘系统缩放。图块按这个网格切分。
    pub physical_width: u32,
    pub physical_height: u32,
}

impl TileSpec {
    /// 按整张原图的显示尺寸造规格。`scale_factor` 是系统的像素缩放
    /// （1.0、1.5、2.0 这种），GPU 按物理像素采样，位图也得按物理像素生成。
    pub fn for_image(
        item: ItemId,
        display_width: f32,
        display_height: f32,
        scale_factor: f32,
    ) -> Self {
        let display_width = positive(display_width);
        let display_height = positive(display_height);
        let (physical_width, physical_height) =
            physical_size(display_width, display_height, scale_factor);
        TileSpec {
            item,
            display_width,
            display_height,
            physical_width,
            physical_height,
        }
    }

    /// 两代规格的内容是不是同一份。物理像素尺寸没变，图块位图就可以继续用，
    /// 只是贴的位置按新的显示尺寸重算。
    ///
    /// 系统缩放或画布显示比例真的变了，物理尺寸就会不同，
    /// [crate::app::tiles::TileGrid::sync] 会换代号。
    pub fn same_content(&self, other: &TileSpec) -> bool {
        self.item == other.item
            && self.physical_width == other.physical_width
            && self.physical_height == other.physical_height
    }

    /// 横向切几块。
    #[cfg(test)]
    pub fn columns(&self) -> u32 {
        self.physical_width.div_ceil(TILE_SIZE)
    }

    /// 纵向切几块。
    #[cfg(test)]
    pub fn rows(&self) -> u32 {
        self.physical_height.div_ceil(TILE_SIZE)
    }

    /// 第 `col` 列、第 `row` 行的图块在目标网格里的像素范围。
    pub fn tile_rect(&self, col: u32, row: u32) -> render::TileRect {
        render::tile_rect(col, row, self.physical_width, self.physical_height)
    }
}

/// 显示尺寸乘系统缩放，取整成物理像素。数值异常时至少 1 像素，上限 65535
/// 防止极端缩放把网格列数撑到没有意义。
fn physical_size(display_width: f32, display_height: f32, scale_factor: f32) -> (u32, u32) {
    let scale = if scale_factor.is_finite() && scale_factor > 0.0 {
        scale_factor
    } else {
        1.0
    };
    (
        physical_dimension(display_width * scale),
        physical_dimension(display_height * scale),
    )
}

fn physical_dimension(pixels: f32) -> u32 {
    if !pixels.is_finite() || pixels <= 0.0 {
        return 1;
    }
    pixels.round().min(65_535.0) as u32
}

fn positive(value: f32) -> f32 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        1.0
    }
}

/// 矩形有没有面积。宽或高不是正数的都当看不见。
#[cfg(test)]
pub fn rect_has_area(rect: Rect) -> bool {
    rect.w > 0.0 && rect.h > 0.0
}

/// 一块图块的身份：哪张图、哪一代、网格第几列第几行。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TileKey {
    pub item: ItemId,
    /// 生成这一块时的代次。显示比例或系统缩放变了代次就加一，旧代次的块
    /// 不能再写回当前缓存。
    pub generation: u64,
    pub col: u32,
    pub row: u32,
}

impl TileKey {
    /// 网格格子坐标，同一代次里唯一确定一块。
    pub fn cell(self) -> (u32, u32) {
        (self.col, self.row)
    }
}

/// 缓存里的一块图块位图，外加生成它的规格。
pub struct TileEntry {
    pub key: TileKey,
    /// 生成时的规格。旧代次的块要靠它换算现在该贴在哪一格。
    pub spec: TileSpec,
    pub image: Arc<RenderImage>,
}

impl TileEntry {
    /// 这块位图在当前显示规格下的逻辑矩形，坐标相对这张图自己的左上角。
    ///
    /// 同一代次里显示尺寸变了也能直接换算；旧代次的块按它生成时的物理网格
    /// 做归一化再乘当前显示尺寸，贴上去就是等比拉伸。
    pub fn display_rect(&self, current: &TileSpec) -> Rect {
        let pixels = render::tile_rect(
            self.key.col,
            self.key.row,
            self.spec.physical_width,
            self.spec.physical_height,
        );
        let scale_x = current.display_width / self.spec.physical_width.max(1) as f32;
        let scale_y = current.display_height / self.spec.physical_height.max(1) as f32;
        Rect::new(
            pixels.x as f32 * scale_x,
            pixels.y as f32 * scale_y,
            pixels.width as f32 * scale_x,
            pixels.height as f32 * scale_y,
        )
    }
}

/// 一帧要贴到屏幕上的一块图块。
///
/// 画的时候只用 `rect` 和 `image`：`render_tiles` 已经把旧代次排在前面、
/// 当前代次排在后面，后画的自然盖住先画的，调用方不用再判断新旧。
/// `key` 和 `current` 留在结构里给测试用，用来按格查块、以及断言
/// 「补齐前整屏都是旧代次」这个顺序保证。
#[derive(Clone)]
pub struct TileView {
    #[allow(dead_code, reason = "调用方按 rect 贴图，字段只给测试按格查块")]
    pub key: TileKey,
    pub image: Arc<RenderImage>,
    /// 相对整张图左上角的逻辑矩形。
    pub rect: Rect,
    /// 是不是当前代次。`false` 表示旧代次临时顶着，新代次的块一到就换掉。
    #[allow(dead_code, reason = "顺序保证由 render_tiles 提供，字段只给测试读")]
    pub current: bool,
}

/// 一次同步的结果：屏幕内哪些块必须画、还缺哪些要后台生成。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TilePlan {
    pub generation: u64,
    /// 这次同步有没有换代号。
    pub generation_changed: bool,
    /// 当前代次必须画出的块，按行优先排序。
    pub needed: Vec<TileKey>,
    /// 当前代次还缺的块，按行优先排序。调用方拿它排后台生成任务。
    pub missing: Vec<TileKey>,
    /// 当前代次是不是已经补齐（补齐后旧代次已经释放）。
    pub complete: bool,
}

/// 一张图的可见图块缓存。
pub struct TileGrid {
    spec: TileSpec,
    generation: u64,
    /// 当前这一帧屏幕上看得到的范围，坐标相对图片左上角（逻辑像素）。
    visible: Rect,
    /// 当前代次必须画出的块，用来拒绝屏幕外和过时的后台结果。
    needed: Vec<TileKey>,
    /// 当前代次已经生成好的块。
    current: HashMap<(u32, u32), TileEntry>,
    /// 上一代次还没被替换掉的块。只保留屏幕内的部分，新代次补齐全后释放。
    stale: HashMap<(u32, u32), TileEntry>,
    /// 这一轮同步里从缓存里拿出来的位图。调用方必须取走并交给窗口的
    /// `drop_image`，否则 GPUI 图集里的 GPU 纹理不会释放。
    released: Vec<Arc<RenderImage>>,
}

/// 把缓存里的位图整批拿出来，配合 `HashMap::drain` 用。
fn drain_images(map: &mut HashMap<(u32, u32), TileEntry>) -> Vec<Arc<RenderImage>> {
    map.drain().map(|(_, entry)| entry.image).collect()
}

impl TileGrid {
    pub fn new(spec: TileSpec) -> Self {
        TileGrid {
            spec,
            generation: 0,
            visible: Rect::new(0.0, 0.0, 0.0, 0.0),
            needed: Vec::new(),
            current: HashMap::new(),
            stale: HashMap::new(),
            released: Vec::new(),
        }
    }

    #[cfg(test)]
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// 屏幕内必须画出的当前代次图块。
    #[cfg(test)]
    pub fn needed(&self) -> &[TileKey] {
        &self.needed
    }

    /// 按这一帧的显示规格和屏幕可见范围同步缓存。屏幕外的块照旧释放。
    ///
    /// 等价于 sync_with 的 keep_tiles = false，原有调用方不用改。
    #[cfg(test)]
    pub fn sync(&mut self, spec: TileSpec, visible: Rect) -> TilePlan {
        self.sync_with(spec, visible, false)
    }

    /// 按这一帧的显示规格和屏幕可见范围同步缓存。
    ///
    /// - 显示比例或系统缩放变了：换代号，旧的当前代次整体降级成
    ///   兜底代次，新代次的块补齐前继续给屏幕用；
    /// - 屏幕外的块（包括旧代次里已经滚出屏幕的）默认在这里直接被丢掉，缓存里
    ///   始终只有屏幕内该画的那几块。keep_tiles = true 时暂时不在可见范围的
    ///   当前代次和兜底代次块继续留在缓存里，不进 released，这样排序拖动让
    ///   图片来回平移时能立刻贴回已经生成好的块，松手后下一次同步会照旧清理；
    /// - 换代号、旧代次降级、补齐后释放兜底代次这些逻辑和 keep_tiles 无关，
    ///   照旧执行；保留下来的位图只是从「这次屏幕外」这个条件里豁免；
    /// - 返回还缺哪些块，调用方据此排后台生成。留在缓存里的屏幕外块不在
    ///   needed 里，不会被当成 missing，也不影响当前代次的补齐判断。
    pub fn sync_with(&mut self, spec: TileSpec, visible: Rect, keep_tiles: bool) -> TilePlan {
        let mut generation_changed = false;
        if spec.item != self.spec.item {
            // 同一张缓存位置换了图：旧图块和它对应的位置全部作废，不能当兜底。
            self.released.extend(drain_images(&mut self.current));
            self.released.extend(drain_images(&mut self.stale));
            self.generation = self.generation.wrapping_add(1);
            generation_changed = true;
        } else if !self.spec.same_content(&spec) {
            // 更早的兜底代次已经没有用，先交出来；这一代降级成新的兜底。
            self.released.extend(drain_images(&mut self.stale));
            self.generation = self.generation.wrapping_add(1);
            self.stale = std::mem::take(&mut self.current);
            generation_changed = true;
        }
        self.spec = spec;
        self.visible = visible;

        let needed = visible_tiles(&self.spec, visible, self.generation);
        // 屏幕外、或者已经不属于当前代次的块当场交出来。keep_tiles 为真时
        // 屏幕外的块留在缓存里，只对「不属于当前代次」这一条照旧释放。
        let mut released = Vec::new();
        self.current.retain(|cell, entry| {
            let generation_ok = entry.key.generation == self.generation;
            let on_screen = needed.iter().any(|key| key.cell() == *cell);
            let keep = generation_ok && (on_screen || keep_tiles);
            if !keep {
                released.push(entry.image.clone());
            }
            keep
        });
        self.released.append(&mut released);
        // 旧代次只留屏幕内还盖得到的地方；keep_tiles 为真时滚出屏幕的也留着，
        // 等当前代次补齐时统一释放。
        let mut released = Vec::new();
        self.stale.retain(|_, entry| {
            let keep = keep_tiles || entry.display_rect(&self.spec).intersects(&visible);
            if !keep {
                released.push(entry.image.clone());
            }
            keep
        });
        self.released.append(&mut released);

        let missing: Vec<TileKey> = needed
            .iter()
            .copied()
            .filter(|key| !self.current.contains_key(&key.cell()))
            .collect();
        let complete = missing.is_empty();
        if complete {
            // 新代次已经补齐，旧代次没有存在意义了。
            self.released.extend(drain_images(&mut self.stale));
        }
        self.needed = needed.clone();
        TilePlan {
            generation: self.generation,
            generation_changed,
            needed,
            missing,
            complete,
        }
    }

    /// 当前代次还缺哪些块。
    #[cfg(test)]
    pub fn missing(&self) -> Vec<TileKey> {
        self.needed
            .iter()
            .copied()
            .filter(|key| !self.current.contains_key(&key.cell()))
            .collect()
    }

    /// 把这一轮同步里拿出来的位图交给调用方。调用方负责逐块 `drop_image`。
    pub fn take_released(&mut self) -> Vec<Arc<RenderImage>> {
        std::mem::take(&mut self.released)
    }

    /// 收下一块后台生成好的图块。
    ///
    /// 只收仍然属于当前代次、而且这一帧还在屏幕内的块；迟到结果（代次已经
    /// 变了）和屏幕外的结果返回 `false`，调用方负责把它的 GPU 纹理释放掉。
    pub fn insert(&mut self, key: TileKey, image: Arc<RenderImage>) -> bool {
        if key.item != self.spec.item || key.generation != self.generation {
            return false;
        }
        if !self.needed.iter().any(|needed| needed.cell() == key.cell()) {
            return false;
        }
        self.current.insert(
            key.cell(),
            TileEntry {
                key,
                spec: self.spec,
                image,
            },
        );
        if self
            .needed
            .iter()
            .all(|needed| self.current.contains_key(&needed.cell()))
        {
            // 当前代次补齐了，旧代次立刻释放。
            self.released.extend(drain_images(&mut self.stale));
        }
        true
    }

    /// 这一帧要贴的图块：旧代次在前、当前代次在后，后画的盖住先画的。
    ///
    /// 新代次只补了一部分时，旧代次整屏垫底、新块逐块盖上去，屏幕不会空。
    pub fn render_tiles(&self) -> Vec<TileView> {
        let mut views = Vec::with_capacity(self.current.len() + self.stale.len());
        let mut stale: Vec<&TileEntry> = self.stale.values().collect();
        stale.sort_by_key(|entry| (entry.key.row, entry.key.col));
        for entry in stale {
            let rect = entry.display_rect(&self.spec);
            if !rect.intersects(&self.visible) {
                continue;
            }
            views.push(TileView {
                key: entry.key,
                image: entry.image.clone(),
                rect,
                current: false,
            });
        }
        let mut current: Vec<&TileEntry> = self.current.values().collect();
        current.sort_by_key(|entry| (entry.key.row, entry.key.col));
        for entry in current {
            views.push(TileView {
                key: entry.key,
                image: entry.image.clone(),
                rect: entry.display_rect(&self.spec),
                current: true,
            });
        }
        views
    }

    /// 缓存里一块图块都没有时返回 true。测试用。
    #[cfg(test)]
    pub fn is_empty(&self) -> bool {
        self.current.is_empty() && self.stale.is_empty()
    }

    /// 当前代次和兜底代次各有几块，测试和调试用。
    #[cfg(test)]
    pub fn counts(&self) -> (usize, usize) {
        (self.current.len(), self.stale.len())
    }

    /// 把缓存里的位图全部取出来（包括已经排队等释放的）。调用方负责逐块
    /// 交给窗口的 `drop_image` 释放 GPU 纹理；取完这条缓存就空了。
    pub fn take_images(&mut self) -> Vec<Arc<RenderImage>> {
        let mut images = std::mem::take(&mut self.released);
        images.append(&mut drain_images(&mut self.current));
        images.append(&mut drain_images(&mut self.stale));
        self.needed.clear();
        images
    }
}

/// 屏幕上真正能看到的区域：视口和这张图在屏幕上的矩形相交的部分，换成
/// 相对这张图左上角的坐标。完全看不见时返回宽高都为零的矩形。
pub fn visible_rect(viewport: Rect, image_screen: Rect) -> Rect {
    let x0 = viewport.x.max(image_screen.x);
    let y0 = viewport.y.max(image_screen.y);
    let x1 = viewport.right().min(image_screen.right());
    let y1 = viewport.bottom().min(image_screen.bottom());
    if x1 <= x0 || y1 <= y0 {
        Rect::new(0.0, 0.0, 0.0, 0.0)
    } else {
        Rect::new(x0 - image_screen.x, y0 - image_screen.y, x1 - x0, y1 - y0)
    }
}

/// 屏幕可见范围里必须画出的图块，按行优先排序。
///
/// `visible` 是相对整张图左上角的逻辑坐标（见 [`visible_rect`]）。只和可见
/// 范围相交的块都算需要，因为块边缘和相邻块有 1 像素重叠。
pub fn visible_tiles(spec: &TileSpec, visible: Rect, generation: u64) -> Vec<TileKey> {
    let (display_width, display_height) = (spec.display_width, spec.display_height);
    let (physical_width, physical_height) = (spec.physical_width, spec.physical_height);
    if physical_width == 0 || physical_height == 0 {
        return Vec::new();
    }
    let x0 = visible.x.max(0.0);
    let y0 = visible.y.max(0.0);
    let x1 = visible.right().min(display_width);
    let y1 = visible.bottom().min(display_height);
    if x1 <= x0 || y1 <= y0 {
        return Vec::new();
    }
    let px0 = ((x0 / display_width) * physical_width as f32)
        .floor()
        .clamp(0.0, physical_width as f32) as u32;
    let px1 = ((x1 / display_width) * physical_width as f32)
        .ceil()
        .clamp(0.0, physical_width as f32) as u32;
    let py0 = ((y0 / display_height) * physical_height as f32)
        .floor()
        .clamp(0.0, physical_height as f32) as u32;
    let py1 = ((y1 / display_height) * physical_height as f32)
        .ceil()
        .clamp(0.0, physical_height as f32) as u32;
    if px1 <= px0 || py1 <= py0 {
        return Vec::new();
    }
    let col0 = px0 / TILE_SIZE;
    let row0 = py0 / TILE_SIZE;
    let col1 = (px1 - 1) / TILE_SIZE;
    let row1 = (py1 - 1) / TILE_SIZE;
    let mut tiles = Vec::new();
    for row in row0..=row1 {
        for col in col0..=col1 {
            tiles.push(TileKey {
                item: spec.item,
                generation,
                col,
                row,
            });
        }
    }
    tiles
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Frame, RgbaImage};

    fn spec(width: f32, height: f32) -> TileSpec {
        TileSpec::for_image(1, width, height, 1.0)
    }

    fn visible(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::new(x, y, w, h)
    }

    fn fake_image() -> Arc<RenderImage> {
        Arc::new(RenderImage::new(vec![Frame::new(RgbaImage::new(
            4,
            4,
        ))]))
    }

    fn fill_all(grid: &mut TileGrid, missing: &[TileKey]) {
        for key in missing {
            assert!(grid.insert(*key, fake_image()), "当前代次的块应被收下");
        }
    }

    #[test]
    fn physical_size_follows_display_and_system_scale() {
        let spec = TileSpec::for_image(1, 1000.0, 500.0, 2.0);
        assert_eq!((spec.physical_width, spec.physical_height), (2000, 1000));
        assert_eq!(spec.columns(), 4);
        assert_eq!(spec.rows(), 2);
    }

    #[test]
    fn visible_rect_clips_viewport_to_image() {
        let viewport = Rect::new(0.0, 0.0, 800.0, 600.0);
        let image = Rect::new(100.0, 50.0, 400.0, 300.0);
        // 整张图都在视口里：可见范围就是整张图，坐标相对图左上角。
        assert_eq!(visible_rect(viewport, image), Rect::new(0.0, 0.0, 400.0, 300.0));

        // 视口只露出图右下一角。
        let small_viewport = Rect::new(300.0, 100.0, 800.0, 600.0);
        assert_eq!(
            visible_rect(small_viewport, image),
            Rect::new(200.0, 50.0, 200.0, 250.0)
        );

        // 完全看不见。
        let away = Rect::new(1000.0, 1000.0, 100.0, 100.0);
        assert!(!rect_has_area(visible_rect(away, image)));
    }

    #[test]
    fn visible_tiles_only_cover_the_visible_screen_part() {
        // 1000×600 显示、1000×600 物理：2 列 2 行。
        let spec = spec(1000.0, 600.0);
        let all = visible_tiles(&spec, visible(0.0, 0.0, 1000.0, 600.0), 7);
        assert_eq!(all.len(), 4);
        assert!(all.iter().all(|key| key.generation == 7));

        // 只看左上角一小块：只有第一块。
        assert_eq!(
            visible_tiles(&spec, visible(0.0, 0.0, 100.0, 100.0), 0)
                .iter()
                .map(|key| key.cell())
                .collect::<Vec<_>>(),
            vec![(0, 0)]
        );

        // 只看右边：第一列不再需要，两块都在第二列。
        assert_eq!(
            visible_tiles(&spec, visible(600.0, 0.0, 400.0, 600.0), 0)
                .iter()
                .map(|key| key.cell())
                .collect::<Vec<_>>(),
            vec![(1, 0), (1, 1)]
        );

        // 屏幕外：一块都不要。
        assert!(visible_tiles(&spec, visible(2000.0, 2000.0, 100.0, 100.0), 0).is_empty());
        assert!(visible_tiles(&spec, Rect::new(0.0, 0.0, 0.0, 0.0), 0).is_empty());
    }


    #[test]
    fn sync_bumps_generation_when_scale_changes() {
        let mut grid = TileGrid::new(spec(1000.0, 600.0));
        let first = grid.sync(spec(1000.0, 600.0), visible(0.0, 0.0, 1000.0, 600.0));
        assert!(!first.generation_changed, "同一规格不该换代");
        assert_eq!(first.missing.len(), 4);
        fill_all(&mut grid, &first.missing);
        assert_eq!(grid.counts(), (4, 0));

        // 显示比例变了：物理尺寸跟着变，换代号，旧块降级成兜底。
        let second = grid.sync(spec(2000.0, 1200.0), visible(0.0, 0.0, 2000.0, 1200.0));
        assert!(second.generation_changed, "显示比例变了必须换代");
        assert_ne!(second.generation, first.generation);
        let (current, stale) = grid.counts();
        assert_eq!(current, 0, "新代次还没生成");
        assert!(stale > 0, "旧代次应继续顶着屏幕");
        assert!(
            grid.render_tiles().iter().all(|view| !view.current),
            "补齐前渲染的是旧代次"
        );

        // 补齐后旧代次立刻释放。
        fill_all(&mut grid, &second.missing);
        assert_eq!(grid.counts().1, 0, "新代次补齐后旧代次应释放");
        assert!(grid.render_tiles().iter().all(|view| view.current));
    }

    #[test]
    fn sync_keeps_same_generation_for_sub_pixel_display_change() {
        // 物理像素尺寸没变（只是显示尺寸微调）时不必重新生成图块，贴的位置按新尺寸算。
        let mut grid = TileGrid::new(spec(1000.0, 600.0));
        let plan = grid.sync(spec(1000.0, 600.0), visible(0.0, 0.0, 1000.0, 600.0));
        fill_all(&mut grid, &plan.missing);

        let mut same = spec(1000.0, 600.0);
        same.display_width = 1000.4;
        let plan = grid.sync(same, visible(0.0, 0.0, 1000.4, 600.0));
        assert!(!plan.generation_changed);
        assert!(plan.complete, "同一内容不用重新生成");
        assert_eq!(grid.counts(), (4, 0));
        // 位置按新的显示尺寸算：第一块宽度是 513/1000 * 1000.4。
        let first = grid
            .render_tiles()
            .into_iter()
            .find(|view| view.key.cell() == (0, 0))
            .expect("应有第一块");
        assert!((first.rect.w - 513.0 / 1000.0 * 1000.4).abs() < 1e-2);
    }

    #[test]
    fn grid_drops_tiles_outside_screen_and_rejects_their_late_results() {
        let mut grid = TileGrid::new(spec(1000.0, 600.0));
        let left = visible(0.0, 0.0, 100.0, 600.0);
        let plan = grid.sync(spec(1000.0, 600.0), left);
        fill_all(&mut grid, &plan.missing);
        // 左边 100 像素宽的可见范围横跨两行（512 一行的切法），共两块。
        assert_eq!(grid.counts(), (2, 0));

        // 滚到右边：左边的块当场释放。
        let right = visible(600.0, 0.0, 400.0, 600.0);
        let plan = grid.sync(spec(1000.0, 600.0), right);
        assert_eq!(grid.counts(), (0, 0), "屏幕外的块不保留");
        // 右侧的块能正常收下；左边那块（屏幕外）不能再写回缓存。
        assert!(grid.insert(plan.needed[0], fake_image()), "右侧块应能收下");
        assert!(
            !grid.insert(
                TileKey {
                    item: 1,
                    generation: plan.generation,
                    col: 0,
                    row: 0,
                },
                fake_image()
            ),
            "屏幕外/不需要的块应被拒绝"
        );
    }

    #[test]
    fn keep_tiles_retains_offscreen_current_tiles_until_next_sync() {
        let mut grid = TileGrid::new(spec(1000.0, 600.0));
        let plan = grid.sync_with(spec(1000.0, 600.0), visible(0.0, 0.0, 1000.0, 600.0), false);
        fill_all(&mut grid, &plan.missing);
        assert_eq!(grid.counts(), (4, 0));

        // 排序拖动滚到右半边：keep_tiles=true 时屏幕外两块留在缓存里。
        let plan = grid.sync_with(spec(1000.0, 600.0), visible(600.0, 0.0, 400.0, 600.0), true);
        assert_eq!(plan.needed.len(), 2);
        assert!(plan.missing.is_empty(), "保留的块不该算 missing");
        assert!(plan.complete);
        assert_eq!(grid.counts(), (4, 0), "屏幕外两块继续留在缓存里");
        assert!(grid.take_released().is_empty(), "没有位图交出去");

        // 滚回全屏：左右两边的块都还在，不用重新生成。
        let plan = grid.sync_with(spec(1000.0, 600.0), visible(0.0, 0.0, 1000.0, 600.0), true);
        assert!(plan.missing.is_empty());
        assert_eq!(grid.counts(), (4, 0));

        // 下一次用 false 同步到看不见的地方：四块一次性交出去。
        let plan = grid.sync_with(spec(1000.0, 600.0), visible(2000.0, 2000.0, 100.0, 100.0), false);
        assert!(plan.needed.is_empty());
        assert_eq!(grid.counts(), (0, 0));
        assert_eq!(grid.take_released().len(), 4, "四张位图都要交出去");
    }

    #[test]
    fn keep_tiles_false_releases_offscreen_like_sync() {
        let mut grid = TileGrid::new(spec(1000.0, 600.0));
        let plan = grid.sync_with(spec(1000.0, 600.0), visible(0.0, 0.0, 1000.0, 600.0), false);
        fill_all(&mut grid, &plan.missing);
        assert_eq!(grid.counts(), (4, 0));

        // keep_tiles=false 滚到右半边：屏幕外两块当场释放。
        let plan = grid.sync_with(spec(1000.0, 600.0), visible(600.0, 0.0, 400.0, 600.0), false);
        assert_eq!(plan.needed.len(), 2);
        assert_eq!(grid.counts(), (2, 0), "屏幕外两块不保留");
        let released = grid.take_released();
        assert_eq!(released.len(), 2, "滚出去的两张位图要交出去");
    }

    #[test]
    fn keep_tiles_retains_offscreen_stale_tiles() {
        // 两组过程完全一样，只有最后一次同步的 keep_tiles 不同。
        let mut kept = TileGrid::new(spec(1000.0, 600.0));
        let plan = kept.sync_with(spec(1000.0, 600.0), visible(0.0, 0.0, 1000.0, 600.0), false);
        fill_all(&mut kept, &plan.missing);
        let plan = kept.sync_with(spec(2000.0, 1200.0), visible(0.0, 0.0, 2000.0, 1200.0), false);
        let stale_after_switch = kept.counts().1;
        assert!(stale_after_switch > 0);
        // 只补一块，保持未补齐，避免补齐逻辑把 stale 清掉。
        kept.insert(plan.missing[0], fake_image());
        let region = visible(1500.0, 0.0, 500.0, 300.0);
        let p_keep = kept.sync_with(spec(2000.0, 1200.0), region, true);
        assert!(!p_keep.complete, "未补齐时 stale 不应被清掉");
        assert_eq!(kept.counts().1, stale_after_switch, "keep_tiles=true 时 stale 全留");
        assert!(kept.take_released().is_empty());

        let mut dropped = TileGrid::new(spec(1000.0, 600.0));
        let plan = dropped.sync_with(spec(1000.0, 600.0), visible(0.0, 0.0, 1000.0, 600.0), false);
        fill_all(&mut dropped, &plan.missing);
        let plan = dropped.sync_with(spec(2000.0, 1200.0), visible(0.0, 0.0, 2000.0, 1200.0), false);
        dropped.insert(plan.missing[0], fake_image());
        let p_drop = dropped.sync_with(spec(2000.0, 1200.0), region, false);
        assert!(!p_drop.complete);
        assert!(dropped.counts().1 < stale_after_switch, "屏幕外的 stale 应被释放");
        assert!(!dropped.take_released().is_empty(), "释放的 stale 位图要交出去");
    }

    #[test]
    fn grid_rejects_results_from_old_generation() {
        let mut grid = TileGrid::new(spec(1000.0, 600.0));
        let plan = grid.sync(spec(1000.0, 600.0), visible(0.0, 0.0, 1000.0, 600.0));
        let old_key = plan.missing[0];
        // 还没生成，规格就变了（比如快速缩放）。
        let plan = grid.sync(spec(2000.0, 1200.0), visible(0.0, 0.0, 2000.0, 1200.0));
        assert!(
            !grid.insert(old_key, fake_image()),
            "旧代次的迟到结果必须丢弃"
        );
        assert_eq!(grid.counts().0, 0, "旧代次的结果不能写进当前代次");
        assert!(!plan.complete);
        assert!(!plan.missing.is_empty());
    }

    #[test]
    fn grid_releases_stale_tiles_that_scrolled_out_of_screen() {
        let mut grid = TileGrid::new(spec(1000.0, 600.0));
        let plan = grid.sync(spec(1000.0, 600.0), visible(0.0, 0.0, 1000.0, 600.0));
        fill_all(&mut grid, &plan.missing);

        // 换代后旧代次先兜底。
        let _plan = grid.sync(spec(2000.0, 1200.0), visible(0.0, 0.0, 1000.0, 600.0));
        assert!(grid.counts().1 > 0);

        // 换到已经完全看不见的范围：旧代次也没有留下来的理由。
        let plan = grid.sync(spec(2000.0, 1200.0), visible(5000.0, 5000.0, 100.0, 100.0));
        assert_eq!(grid.counts(), (0, 0));
        assert!(plan.needed.is_empty());
        assert!(plan.missing.is_empty());
    }

    #[test]
    fn grid_switching_item_drops_everything_without_reuse() {
        let mut grid = TileGrid::new(spec(1000.0, 600.0));
        let plan = grid.sync(spec(1000.0, 600.0), visible(0.0, 0.0, 1000.0, 600.0));
        fill_all(&mut grid, &plan.missing);

        let other = TileSpec::for_image(2, 1000.0, 600.0, 1.0);
        let plan = grid.sync(other, visible(0.0, 0.0, 1000.0, 600.0));
        assert!(plan.generation_changed);
        assert_eq!(grid.counts(), (0, 0), "换图不能拿旧图块兜底");
        assert_eq!(grid.render_tiles().len(), 0);
    }

    #[test]
    fn render_tiles_puts_stale_first_then_current() {
        let mut grid = TileGrid::new(spec(1000.0, 600.0));
        let plan = grid.sync(spec(1000.0, 600.0), visible(0.0, 0.0, 1000.0, 600.0));
        fill_all(&mut grid, &plan.missing);
        let plan = grid.sync(spec(2000.0, 1200.0), visible(0.0, 0.0, 2000.0, 1200.0));
        // 新代次只补一块。
        grid.insert(plan.missing[0], fake_image());
        let views = grid.render_tiles();
        assert!(
            views.iter().any(|view| !view.current),
            "补齐前旧代次应垫底"
        );
        assert!(views.iter().any(|view| view.current), "新块应画在旧块上面");
        let first_current = views.iter().position(|view| view.current).unwrap();
        let last_stale = views.iter().rposition(|view| !view.current).unwrap();
        assert!(last_stale < first_current, "旧代次必须先画");
    }

    #[test]
    fn take_images_empties_the_grid() {
        let mut grid = TileGrid::new(spec(1000.0, 600.0));
        let plan = grid.sync(spec(1000.0, 600.0), visible(0.0, 0.0, 1000.0, 600.0));
        fill_all(&mut grid, &plan.missing);
        let images = grid.take_images();
        assert_eq!(images.len(), 4);
        assert!(grid.is_empty());
        assert!(grid.render_tiles().is_empty());
    }

    #[test]
    fn sync_hands_out_every_image_it_drops() {
        // 换代后新代次补齐：旧代次的 4 张位图必须交出来给调用方 drop_image，
        // 不能只是从 HashMap 里删掉（那样 GPU 纹理不会释放）。
        let mut grid = TileGrid::new(spec(1000.0, 600.0));
        let plan = grid.sync(spec(1000.0, 600.0), visible(0.0, 0.0, 1000.0, 600.0));
        fill_all(&mut grid, &plan.missing);
        let plan = grid.sync(spec(2000.0, 1200.0), visible(0.0, 0.0, 2000.0, 1200.0));
        fill_all(&mut grid, &plan.missing);
        let released = grid.take_released();
        assert_eq!(released.len(), 4, "旧代次的 4 张位图都要交出来");

        // 滚出屏幕的块也要交出来。
        let plan = grid.sync(spec(2000.0, 1200.0), visible(0.0, 0.0, 100.0, 100.0));
        assert_eq!(plan.needed.len(), 1);
        let released = grid.take_released();
        // 2000×1200 的目标切出 4×3 共 12 块，只留屏幕内那 1 块。
        assert_eq!(released.len(), 11, "屏幕外的 11 张位图要交出来");
    }

    #[test]
    fn switching_item_hands_out_old_images() {
        let mut grid = TileGrid::new(spec(1000.0, 600.0));
        let plan = grid.sync(spec(1000.0, 600.0), visible(0.0, 0.0, 1000.0, 600.0));
        fill_all(&mut grid, &plan.missing);
        let other = TileSpec::for_image(2, 1000.0, 600.0, 1.0);
        grid.sync(other, visible(0.0, 0.0, 1000.0, 600.0));
        assert_eq!(grid.take_released().len(), 4, "换图时旧位图要交出来");
    }

    #[test]
    fn missing_lists_only_unsatisfied_current_tiles() {
        let mut grid = TileGrid::new(spec(1000.0, 600.0));
        let plan = grid.sync(spec(1000.0, 600.0), visible(0.0, 0.0, 1000.0, 600.0));
        assert_eq!(grid.missing().len(), 4);
        let first = plan.missing[0];
        assert!(grid.insert(first, fake_image()));
        let missing = grid.missing();
        assert_eq!(missing.len(), 3);
        assert!(!missing.contains(&first));
        assert_eq!(missing, plan.missing[1..].to_vec());
    }
}
