//! 源分辨率瓦片金字塔的纯算法层：层级几何、选层、坐标映射和渲染计划。
//!
//! 这一层只做数值计算，不生成像素、不碰应用状态、也不碰界面。调用方负责：
//!
//! - 按 [SourceRevision] 解码并缓存源图；
//! - 按 [TileKey] 缓存已经生成好的图块位图，在 [plan_block] 里通过 get
//!   回调交给本模块；
//! - 按 [PyramidPlan] 先铺占位底色，再从粗到细把顶层和各层图块贴到屏幕上。
//!
//! 坐标有三套，容易混：
//!
//! - 源坐标：原图像素坐标，范围 0..source_w、0..source_h；
//! - 层坐标：某一层缩小后的像素坐标，范围 0..level.width、0..level.height；
//! - 显示坐标：整张图铺在屏幕上时相对图片左上角的逻辑像素坐标，范围
//!   0..full_width、0..full_height，由画布缩放比例决定。
//!
//! 设计见 docs/preview-source-pyramid.md。

#![allow(
    dead_code,
    reason = "source_rect、prefetch_level、LodState::new 留给后续阶段接入；其余公共项已被 app.rs、pyramid_store.rs 和本文件测试使用"
)]

use std::sync::Arc;

use gpui::RenderImage;

use crate::imaging::render::{self, TileRect};
use crate::model::ItemId;
use crate::ui::canvas::Rect;

/// 图片源版本：哪张图、像素指纹、源图尺寸。
///
/// 源路径、剪贴板内容、图片像素或尺寸任一变化都会产生新的版本。金字塔、
/// 图块缓存和源图缓存都以它为键；旧版本的迟到结果直接丢弃。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SourceRevision {
    pub item: ItemId,
    pub fingerprint: u64,
    pub width: u32,
    pub height: u32,
}

/// 金字塔的一层。
///
/// index 是层号：0 是原图分辨率，越粗越大。第 k 层的宽高是
/// ceil(source / 2^k)，最后一层是顶层，最长边不超过生成时给的 top_max。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LevelMeta {
    pub index: u32,
    pub width: u32,
    pub height: u32,
    /// 横向切几块，等于 ceil(width / 512)。
    pub tiles_x: u32,
    /// 纵向切几块，等于 ceil(height / 512)。
    pub tiles_y: u32,
}

/// 算出从原图到顶层的所有层。
///
/// 第 k 层宽高为 ceil(source_w / 2^k)、ceil(source_h / 2^k)。层数取最小的
/// 非负整数 K，使最粗一层的长边不超过 top_max。top_max 为 0 时按 1 处理，
/// 保证至少能算出顶层。
///
/// 返回的层从细到粗排列，最后一个就是顶层。源图本身的长边不超过 top_max
/// 时只有第 0 层。
pub fn levels(source_w: u32, source_h: u32, top_max: u32) -> Vec<LevelMeta> {
    let top_max = top_max.max(1);
    let mut width = source_w;
    let mut height = source_h;
    let mut index = 0;
    let mut result = Vec::new();
    loop {
        result.push(LevelMeta {
            index,
            width,
            height,
            tiles_x: width.div_ceil(render::TILE_SIZE),
            tiles_y: height.div_ceil(render::TILE_SIZE),
        });
        if width.max(height) <= top_max {
            break;
        }
        width = width.div_ceil(2);
        height = height.div_ceil(2);
        index += 1;
    }
    result
}

/// 按屏幕上整张图的物理宽高选理论显示层。
///
/// 规则：从第 0 层开始，只要下一层（更粗的层）的像素宽高都不小于
/// physical_w、physical_h，就选下一层。也就是选「最粗的、像素不少于显示
/// 物理像素的层」：
///
/// - 显示尺寸不小于原图时选第 0 层，不再生成更大的层；
/// - 显示尺寸不大于顶层时选顶层；
/// - 其余情况层像素是显示物理像素的 1 到 2 倍，既不模糊也不过度生成。
///
/// physical_w、physical_h 是已经乘过系统缩放因子的物理像素。传 0、负数或
/// NaN 时按 0 处理，选最粗的层；传正无穷时选第 0 层。levels 为空时返回 0。
pub fn level_for_display(levels: &[LevelMeta], physical_w: f32, physical_h: f32) -> u32 {
    if levels.is_empty() {
        return 0;
    }
    let target_w = non_negative(physical_w);
    let target_h = non_negative(physical_h);
    let mut chosen = 0;
    for candidate in 1..levels.len() {
        let level = &levels[candidate];
        if level.width as f32 >= target_w && level.height as f32 >= target_h {
            chosen = candidate;
        } else {
            break;
        }
    }
    levels[chosen].index
}

/// 一张图当前的显示层选择状态。
///
/// current 是带迟滞的显示层，base 是这一帧按显示尺寸算出的理论层。两者
/// 分开是为了避免缩放停在层级边界附近时来回抖动。last_switch_frame 记录
/// 最近一次真正换层的帧号，调用方可以用它安排预取或判断刚换过层。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LodState {
    /// 用 [LodState::new] 新建时，第一次 update 前是非法层号；用结构体
    /// 字面量构造时按填入的层号处理。
    pub current: u32,
    /// 这一帧按显示尺寸算出的理论层。用 new 新建、第一次 update 前同样未设置。
    pub base: u32,
    pub last_switch_frame: u64,
}

/// 新建状态时用的非法层号。第一次 update 看到它就知道还没有选过层。
const UNSET_LEVEL: u32 = u32::MAX;

impl Default for LodState {
    fn default() -> Self {
        Self {
            current: UNSET_LEVEL,
            base: UNSET_LEVEL,
            last_switch_frame: 0,
        }
    }
}

impl LodState {
    /// 初始状态。current 先放一个非法层号，第一次 [LodState::update] 会直接
    /// 取理论层；用结构体字面量新建时如果 current 填的是合法层号，就按已有
    /// 状态走迟滞规则。
    pub fn new() -> Self {
        Self::default()
    }

    /// 按这一帧的显示尺寸更新显示层，返回新的 current。
    ///
    /// 迟滞规则（当前层像素密度比
    /// r = min(当前层宽 / 显示物理宽, 当前层高 / 显示物理高)）：
    ///
    /// - r 小于 0.98 且当前层比第 0 层粗：当前层不够清晰，切到更细的一层；
    /// - r 大于 2.04 且当前层比顶层细：当前层过度清晰，切到更粗的一层；
    /// - 其它情况保持当前层。
    ///
    /// 当前层号不在 levels 里、或者理论层和当前层相差超过一层（窗口尺寸
    /// 大幅变化）时，直接跳到理论层，不逐层过渡。显示物理宽高不是正数时，
    /// 比例没有意义，保持当前层不变。
    pub fn update(
        &mut self,
        levels: &[LevelMeta],
        physical_w: f32,
        physical_h: f32,
        frame: u64,
    ) -> u32 {
        let Some(top) = levels.last() else {
            self.current = 0;
            self.base = 0;
            return 0;
        };
        let base = level_for_display(levels, physical_w, physical_h);
        self.base = base;
        let Some(current_level) = level_at(levels, self.current) else {
            self.last_switch_frame = frame;
            self.current = base;
            return self.current;
        };
        if self.current.abs_diff(base) > 1 {
            self.last_switch_frame = frame;
            self.current = base;
            return self.current;
        }
        let display_w = non_negative(physical_w);
        let display_h = non_negative(physical_h);
        if display_w <= 0.0 || display_h <= 0.0 {
            return self.current;
        }
        let ratio =
            (current_level.width as f32 / display_w).min(current_level.height as f32 / display_h);
        if ratio < 0.98 && self.current > 0 {
            self.current -= 1;
            self.last_switch_frame = frame;
        } else if ratio > 2.04 && self.current < top.index {
            self.current += 1;
            self.last_switch_frame = frame;
        }
        self.current
    }
}

/// 预取层：当前层快不够清晰时，提前生成更细一层的可见图块。
///
/// 设计 6.5：r 小于 1.25 且 current 大于 0 时返回 current - 1，否则返回
/// None。r 的算法和 [LodState::update] 相同。调用方拿到层号后只生成可见块，
/// 不切换当前显示层。
pub fn prefetch_level(
    levels: &[LevelMeta],
    current: u32,
    physical_w: f32,
    physical_h: f32,
) -> Option<u32> {
    let current_level = level_at(levels, current)?;
    let next = current.checked_sub(1)?;
    level_at(levels, next)?;
    let display_w = non_negative(physical_w);
    let display_h = non_negative(physical_h);
    if display_w <= 0.0 || display_h <= 0.0 {
        return None;
    }
    let ratio =
        (current_level.width as f32 / display_w).min(current_level.height as f32 / display_h);
    (ratio < 1.25).then_some(next)
}

/// 一个图片块的显示几何。
///
/// visible 是这张图在屏幕上真正露出来的部分，坐标换成相对整张图左上角的
/// 逻辑像素。它已经和视口、图片块容器求过交集；完全看不见时宽高为 0。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlockGeometry {
    /// 整张图铺在屏幕上的逻辑宽度（不受裁剪带影响）。
    pub full_width: f32,
    /// 整张图铺在屏幕上的逻辑高度（不受裁剪带影响）。
    pub full_height: f32,
    /// 系统像素缩放因子，例如 1.0、1.5、2.0。
    pub device_scale: f32,
    /// 整张图坐标系里的可见矩形，单位是逻辑像素。
    pub visible: Rect,
}

impl BlockGeometry {
    /// 整张图在屏幕上占的物理像素宽高。
    fn physical_size(&self) -> (f32, f32) {
        let scale = if self.device_scale.is_finite() && self.device_scale > 0.0 {
            self.device_scale
        } else {
            1.0
        };
        (self.full_width * scale, self.full_height * scale)
    }
}

/// 一块层图块的身份：哪张图的哪个源版本、第几层、第几列第几行。
///
/// 缓存键不带显示尺寸、裁剪带、排版、缝隙和圆角，所以换排版或缩放不会让
/// 已经生成好的图块失效。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TileKey {
    pub revision: SourceRevision,
    pub level: u32,
    pub col: u32,
    pub row: u32,
}

/// 一块已经生成好、这一帧要贴的图块。
#[derive(Clone)]
pub struct PyramidTile {
    pub key: TileKey,
    pub image: Arc<RenderImage>,
    /// 纹理相对整张图左上角的逻辑矩形。调用方还要加上 image_left、
    /// image_top 才是屏幕坐标。
    pub rect: Rect,
}

/// 金字塔的一层里这一帧要贴的图块。
#[derive(Clone)]
pub struct PyramidLayer {
    pub level: u32,
    pub tiles: Vec<PyramidTile>,
}

/// 一个图片块这一帧的渲染计划。
pub struct PyramidPlan {
    /// 带迟滞选出的目标层，调用方据此决定还要生成哪些块。
    pub target: u32,
    /// 从粗到细排列的层；每层只包含已经生成好、确实要画的图块。
    /// 顶层作为最粗的一层，只有一个覆盖整张图的图块。
    pub layers: Vec<PyramidLayer>,
    /// 目标层缺少的可见图块，按行优先排列。目标层就是顶层时为空。
    pub missing: Vec<TileKey>,
    /// 顶层纹理还没有生成。为真时调用方必须先在图片块里铺占位底色。
    pub top_missing: bool,
}

impl PyramidPlan {
    fn empty(target: u32, top_missing: bool) -> Self {
        Self {
            target,
            layers: Vec::new(),
            missing: Vec::new(),
            top_missing,
        }
    }
}

/// 一块图块在层坐标里的纹理矩形。
///
/// 每块图块按 512×512 层像素切，四条边各多取 1 个层像素做采样边界，再夹到
/// 层的宽高以内。这里直接复用 [render::tile_rect]，保证和现有图块生成逻辑
/// 使用同一套边界规则。
pub fn texture_rect(level: &LevelMeta, col: u32, row: u32) -> TileRect {
    render::tile_rect(col, row, level.width, level.height)
}

/// 把层坐标里的纹理矩形换算成源图坐标里的矩形。
///
/// 起点向下取整、终点向上取整，保证纹理覆盖的源像素不少于对应层像素，
/// 采样时不会在边上少读一列或一行。
pub fn source_rect(level: &LevelMeta, texture: TileRect, source_w: u32, source_h: u32) -> TileRect {
    if level.width == 0 || level.height == 0 || texture.is_empty() {
        return TileRect {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        };
    }
    let x0 = scale_floor(texture.x, source_w, level.width);
    let y0 = scale_floor(texture.y, source_h, level.height);
    let x1 = scale_ceil(texture.right(), source_w, level.width);
    let y1 = scale_ceil(texture.bottom(), source_h, level.height);
    TileRect {
        x: x0,
        y: y0,
        width: x1.saturating_sub(x0),
        height: y1.saturating_sub(y0),
    }
}

/// 把层坐标里的纹理矩形换算成相对整张图左上角的显示矩形。
///
/// 显示坐标只跟整张图在屏幕上的逻辑宽高有关，裁剪带不参与计算。显示宽高
/// 不是正数或不是有限数时返回空矩形。
pub fn display_rect(
    level: &LevelMeta,
    texture: TileRect,
    full_width: f32,
    full_height: f32,
) -> Rect {
    if level.width == 0
        || level.height == 0
        || !full_width.is_finite()
        || !full_height.is_finite()
        || full_width <= 0.0
        || full_height <= 0.0
    {
        return Rect::new(0.0, 0.0, 0.0, 0.0);
    }
    let scale_x = full_width as f64 / level.width as f64;
    let scale_y = full_height as f64 / level.height as f64;
    Rect::new(
        (texture.x as f64 * scale_x) as f32,
        (texture.y as f64 * scale_y) as f32,
        (texture.width as f64 * scale_x) as f32,
        (texture.height as f64 * scale_y) as f32,
    )
}

/// 屏幕可见范围里这一层必须画出的图块，按行优先排列。
///
/// visible 是相对整张图左上角的逻辑坐标；函数会先把它夹到整张图的范围内。
/// 只和可见范围有面积重叠的块才算需要，只是在边上碰一下不算。图块纹理虽然
/// 向外多带了 1 个层像素，是否需要仍按图块本身的覆盖范围判断。
///
/// 返回的键里带着 revision，所以调用方必须把当前图片的源版本传进来；
/// [TileKey] 的版本、层号、行列都不同，缓存才能区分。
pub fn visible_tiles(
    revision: SourceRevision,
    level: &LevelMeta,
    visible: Rect,
    full_width: f32,
    full_height: f32,
) -> Vec<TileKey> {
    let Some((x0, y0, x1, y1)) = clip_visible(visible, full_width, full_height) else {
        return Vec::new();
    };
    let layer_w = level.width as f64;
    let layer_h = level.height as f64;
    let Some((col0, col1)) = axis_range(x0, x1, full_width as f64, layer_w, level.tiles_x) else {
        return Vec::new();
    };
    let Some((row0, row1)) = axis_range(y0, y1, full_height as f64, layer_h, level.tiles_y) else {
        return Vec::new();
    };
    let mut tiles = Vec::new();
    for row in row0..=row1 {
        for col in col0..=col1 {
            tiles.push(TileKey {
                revision,
                level: level.index,
                col,
                row,
            });
        }
    }
    tiles
}

/// 组装一个图片块这一帧的渲染计划。
///
/// 步骤和设计 7.1 一致：
///
/// 1. 用整张图的逻辑宽高乘系统缩放得到显示物理宽高，更新 [LodState]；
/// 2. 从顶层往目标层逐层收集可见范围内已经生成好的图块，层的顺序从粗到细；
/// 3. 顶层是完整纹理，不查 get，当成只有一块的层处理；
/// 4. 目标层缺的可见块放进 missing，目标层就是顶层时为空；
/// 5. 顶层不存在时 top_missing 为真，调用方先铺占位底色。
///
/// 可见范围为空或显示几何无效时，不返回任何层和缺块，只按顶层是否存在设置
/// top_missing。
pub fn plan_block<F>(
    revision: SourceRevision,
    levels: &[LevelMeta],
    top: Option<&Arc<RenderImage>>,
    lod: &mut LodState,
    geometry: &BlockGeometry,
    frame: u64,
    get: F,
) -> PyramidPlan
where
    F: Fn(&TileKey) -> Option<Arc<RenderImage>>,
{
    let (physical_w, physical_h) = geometry.physical_size();
    let target = lod.update(levels, physical_w, physical_h, frame);
    let top_missing = top.is_none();
    let Some(top_level) = levels.last() else {
        return PyramidPlan::empty(target, top_missing);
    };
    if clip_visible(geometry.visible, geometry.full_width, geometry.full_height).is_none() {
        return PyramidPlan::empty(target, top_missing);
    }

    let top_index = top_level.index;
    let mut layers = Vec::new();
    let mut missing = Vec::new();
    for level in levels.iter().rev() {
        if level.index < target || level.index > top_index {
            continue;
        }
        if level.index == top_index {
            // 顶层是完整纹理，只有一块，矩形就是整张图在屏幕上的大小。
            if let Some(image) = top {
                layers.push(PyramidLayer {
                    level: level.index,
                    tiles: vec![PyramidTile {
                        key: TileKey {
                            revision,
                            level: level.index,
                            col: 0,
                            row: 0,
                        },
                        image: Arc::clone(image),
                        rect: Rect::new(0.0, 0.0, geometry.full_width, geometry.full_height),
                    }],
                });
            }
            continue;
        }
        let keys = visible_tiles(
            revision,
            level,
            geometry.visible,
            geometry.full_width,
            geometry.full_height,
        );
        let mut tiles = Vec::new();
        for key in keys {
            match get(&key) {
                Some(image) => {
                    let rect = display_rect(
                        level,
                        texture_rect(level, key.col, key.row),
                        geometry.full_width,
                        geometry.full_height,
                    );
                    tiles.push(PyramidTile { key, image, rect });
                }
                None => {
                    if level.index == target {
                        missing.push(key);
                    }
                }
            }
        }
        if !tiles.is_empty() {
            layers.push(PyramidLayer {
                level: level.index,
                tiles,
            });
        }
    }
    PyramidPlan {
        target,
        layers,
        missing,
        top_missing,
    }
}

/// 非负化显示尺寸。NaN 和负数按 0 处理，正无穷保留（选层时自然落到第 0 层）。
fn non_negative(value: f32) -> f32 {
    if value.is_nan() || value < 0.0 {
        0.0
    } else {
        value
    }
}

/// 按层号找层。层号连续时通常就是下标，但这里不假设下标和层号一定相等。
fn level_at(levels: &[LevelMeta], index: u32) -> Option<&LevelMeta> {
    levels.iter().find(|level| level.index == index)
}

/// 把纹理矩形的起点从层坐标换成源坐标，向下取整并夹到源图范围内。
fn scale_floor(position: u32, source: u32, layer: u32) -> u32 {
    let scaled = position as u128 * source as u128 / layer as u128;
    scaled.min(source as u128) as u32
}

/// 把纹理矩形的终点从层坐标换成源坐标，向上取整并夹到源图范围内。
fn scale_ceil(position: u32, source: u32, layer: u32) -> u32 {
    let scaled = (position as u128 * source as u128).div_ceil(layer as u128);
    scaled.min(source as u128) as u32
}

/// 把可见矩形夹到整张图的逻辑范围内，返回 (x0, y0, x1, y1)。
/// 没有面积、坐标不是有限数、或者整张图尺寸无效时返回 None。
fn clip_visible(visible: Rect, full_width: f32, full_height: f32) -> Option<(f64, f64, f64, f64)> {
    if !full_width.is_finite()
        || !full_height.is_finite()
        || full_width <= 0.0
        || full_height <= 0.0
        || !visible.x.is_finite()
        || !visible.y.is_finite()
        || !visible.w.is_finite()
        || !visible.h.is_finite()
    {
        return None;
    }
    let x0 = visible.x.max(0.0).min(full_width) as f64;
    let y0 = visible.y.max(0.0).min(full_height) as f64;
    let x1 = visible.right().max(0.0).min(full_width) as f64;
    let y1 = visible.bottom().max(0.0).min(full_height) as f64;
    (x1 > x0 && y1 > y0).then_some((x0, y0, x1, y1))
}

/// 一维方向上和图块范围有面积重叠的列（或行）区间，闭区间。
///
/// start、end 是显示坐标，full 是整张图在屏幕上的逻辑长度，layer 是这一层
/// 的层像素长度，tiles 是这一层的列数（或行数）。先按比例估一个大概范围，
/// 再向两边各放宽一块，最后逐块用显示矩形的真实边界过滤，避免浮点数在块
/// 边界上的舍入误差多算或漏算。
fn axis_range(start: f64, end: f64, full: f64, layer: f64, tiles: u32) -> Option<(u32, u32)> {
    if tiles == 0 || !(full > 0.0) || !(layer > 0.0) || !(end > start) {
        return None;
    }
    let tile = render::TILE_SIZE as f64;
    let max_index = tiles as i64 - 1;
    let approximate_first = (start / full * layer / tile).floor() as i64;
    let approximate_last = (end / full * layer / tile).ceil() as i64 - 1;
    let first_candidate = (approximate_first - 1).clamp(0, max_index) as u32;
    let last_candidate = (approximate_last + 1).clamp(0, max_index) as u32;
    if first_candidate > last_candidate {
        return None;
    }
    let mut first = None;
    let mut last = None;
    for index in first_candidate..=last_candidate {
        let layer_start = (index as f64 * tile).min(layer);
        let layer_end = ((index as f64 + 1.0) * tile).min(layer);
        let display_start = layer_start / layer * full;
        let display_end = layer_end / layer * full;
        if display_end > start && display_start < end {
            first.get_or_insert(index);
            last = Some(index);
        }
    }
    first.zip(last)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{Frame, RgbaImage};

    fn revision(width: u32, height: u32) -> SourceRevision {
        SourceRevision {
            item: 7,
            fingerprint: 0x1234_5678_9abc_def0,
            width,
            height,
        }
    }

    fn fake_image(width: u32, height: u32) -> Arc<RenderImage> {
        Arc::new(RenderImage::new(vec![Frame::new(RgbaImage::new(
            width, height,
        ))]))
    }

    fn by_index(levels: &[LevelMeta], index: u32) -> LevelMeta {
        levels
            .iter()
            .find(|level| level.index == index)
            .copied()
            .expect("层号应该存在")
    }

    fn visible(x: f32, y: f32, w: f32, h: f32) -> Rect {
        Rect::new(x, y, w, h)
    }

    fn geometry(full_width: f32, full_height: f32, visible: Rect) -> BlockGeometry {
        BlockGeometry {
            full_width,
            full_height,
            device_scale: 1.0,
            visible,
        }
    }

    #[test]
    fn levels_halve_until_top_fits() {
        let list = levels(2000, 1000, 512);
        assert_eq!(
            list,
            vec![
                LevelMeta {
                    index: 0,
                    width: 2000,
                    height: 1000,
                    tiles_x: 4,
                    tiles_y: 2,
                },
                LevelMeta {
                    index: 1,
                    width: 1000,
                    height: 500,
                    tiles_x: 2,
                    tiles_y: 1,
                },
                LevelMeta {
                    index: 2,
                    width: 500,
                    height: 250,
                    tiles_x: 1,
                    tiles_y: 1,
                },
            ]
        );
        for level in &list {
            if level.index > 0 {
                let previous = by_index(&list, level.index - 1);
                assert_eq!(level.width, previous.width.div_ceil(2));
                assert_eq!(level.height, previous.height.div_ceil(2));
            }
        }
        let top = list.last().expect("应该有顶层");
        let previous = by_index(&list, top.index - 1);
        assert!(top.width.max(top.height) <= 512);
        assert!(previous.width.max(previous.height) > 512);
    }

    #[test]
    fn levels_handle_small_tall_and_zero_sources() {
        let small = levels(512, 300, 512);
        assert_eq!(small.len(), 1);
        assert_eq!(small[0].width, 512);
        assert_eq!(small[0].height, 300);

        let tiny = levels(1, 1, 512);
        assert_eq!(tiny.len(), 1);
        assert_eq!(tiny[0].tiles_x, 1);
        assert_eq!(tiny[0].tiles_y, 1);

        let empty = levels(0, 0, 512);
        assert_eq!(empty.len(), 1);
        assert_eq!(empty[0].width, 0);
        assert_eq!(empty[0].height, 0);
        assert_eq!(empty[0].tiles_x, 0);
        assert_eq!(empty[0].tiles_y, 0);

        let tall = levels(100, 3000, 512);
        let dims: Vec<(u32, u32)> = tall
            .iter()
            .map(|level| (level.width, level.height))
            .collect();
        assert_eq!(dims, vec![(100, 3000), (50, 1500), (25, 750), (13, 375)]);
        assert_eq!(
            tall.last()
                .expect("顶层")
                .width
                .max(tall.last().expect("顶层").height),
            375
        );

        let forced_top_one = levels(10, 10, 1);
        assert_eq!(forced_top_one.len(), 5);
        assert_eq!(forced_top_one.last().expect("顶层").width, 1);
        assert_eq!(forced_top_one.last().expect("顶层").height, 1);

        let zero_top_max = levels(10, 10, 0);
        assert_eq!(zero_top_max.len(), 5);
        assert_eq!(zero_top_max.last().expect("顶层").width, 1);
    }

    #[test]
    fn level_for_display_picks_coarsest_covering_layer() {
        let list = levels(2000, 1000, 512);
        assert_eq!(level_for_display(&list, 2000.0, 1000.0), 0);
        assert_eq!(level_for_display(&list, 4000.0, 2000.0), 0);
        assert_eq!(level_for_display(&list, 1001.0, 500.0), 0);
        assert_eq!(level_for_display(&list, 1000.0, 501.0), 0);
        assert_eq!(level_for_display(&list, 1000.0, 500.0), 1);
        assert_eq!(level_for_display(&list, 600.0, 300.0), 1);
        assert_eq!(level_for_display(&list, 500.0, 250.0), 2);
        assert_eq!(level_for_display(&list, 5.0, 2.5), 2);
        assert_eq!(level_for_display(&[], 100.0, 100.0), 0);
    }

    #[test]
    fn level_for_display_handles_invalid_extents() {
        let list = levels(2000, 1000, 512);
        assert_eq!(level_for_display(&list, 0.0, 0.0), 2);
        assert_eq!(level_for_display(&list, -10.0, 5.0), 2);
        assert_eq!(level_for_display(&list, f32::NAN, 10.0), 2);
        assert_eq!(level_for_display(&list, f32::INFINITY, 10.0), 0);
    }

    #[test]
    fn lod_jumps_directly_on_large_zoom_change() {
        let list = levels(2000, 1000, 512);
        let mut lod = LodState::new();
        assert_eq!(lod.update(&list, 100.0, 50.0, 9), 2);
        assert_eq!(lod.base, 2);
        assert_eq!(lod.current, 2);
        assert_eq!(lod.last_switch_frame, 9);
    }

    #[test]
    fn lod_avoids_boundary_flicker() {
        let list = levels(2000, 1000, 512);
        let mut lod = LodState {
            current: 1,
            base: 1,
            last_switch_frame: 1,
        };
        for (frame, width) in [(2, 985.0), (3, 1015.0), (4, 985.0), (5, 1015.0)] {
            let height = width / 2.0;
            assert_eq!(lod.update(&list, width, height, frame), 1);
        }
        assert_eq!(lod.base, 0);
        assert_eq!(lod.last_switch_frame, 1);
    }

    #[test]
    fn lod_switches_one_step_when_ratio_crosses_thresholds() {
        let list = levels(2000, 1000, 512);
        // 当前第 0 层，显示只比第 1 层宽一点点：比例超过 2.04，切到第 1 层。
        let mut coarse = LodState {
            current: 0,
            base: 0,
            last_switch_frame: 0,
        };
        assert_eq!(coarse.update(&list, 970.0, 485.0, 3), 1);
        assert_eq!(coarse.last_switch_frame, 3);

        // 当前第 1 层，显示已经超过第 1 层像素：比例低于 0.98，切回第 0 层。
        let mut fine = LodState {
            current: 1,
            base: 1,
            last_switch_frame: 0,
        };
        assert_eq!(fine.update(&list, 1030.0, 515.0, 4), 0);
        assert_eq!(fine.last_switch_frame, 4);
    }

    #[test]
    fn lod_keeps_switch_frame_when_nothing_changes() {
        let list = levels(2000, 1000, 512);
        let mut lod = LodState {
            current: 1,
            base: 1,
            last_switch_frame: 7,
        };
        assert_eq!(lod.update(&list, 1000.0, 500.0, 8), 1);
        assert_eq!(lod.last_switch_frame, 7);
        assert_eq!(lod.update(&list, 990.0, 495.0, 9), 1);
        assert_eq!(lod.base, 1);
        assert_eq!(lod.last_switch_frame, 7);
    }

    #[test]
    fn lod_recovers_from_invalid_current() {
        let list = levels(2000, 1000, 512);
        let mut lod = LodState {
            current: 99,
            base: 0,
            last_switch_frame: 0,
        };
        assert_eq!(lod.update(&list, 600.0, 300.0, 12), 1);
        assert_eq!(lod.base, 1);
        assert_eq!(lod.current, 1);
        assert_eq!(lod.last_switch_frame, 12);
    }

    #[test]
    fn lod_without_levels_is_stable() {
        let mut lod = LodState {
            current: 3,
            base: 1,
            last_switch_frame: 2,
        };
        assert_eq!(lod.update(&[], 100.0, 100.0, 4), 0);
        assert_eq!(lod.current, 0);
        assert_eq!(lod.base, 0);
        assert_eq!(lod.last_switch_frame, 2);
    }

    #[test]
    fn prefetch_only_when_close_to_a_finer_layer() {
        let list = levels(2000, 1000, 512);
        assert_eq!(prefetch_level(&list, 1, 1000.0, 500.0), Some(0));
        assert_eq!(prefetch_level(&list, 1, 850.0, 425.0), Some(0));
        assert_eq!(prefetch_level(&list, 1, 800.0, 400.0), None);
        assert_eq!(prefetch_level(&list, 0, 1000.0, 500.0), None);
        assert_eq!(prefetch_level(&list, 2, 500.0, 250.0), Some(1));
        assert_eq!(prefetch_level(&list, 99, 500.0, 250.0), None);
    }

    #[test]
    fn texture_rect_has_one_layer_pixel_border() {
        let list = levels(2000, 1000, 512);
        let level = by_index(&list, 0);
        assert_eq!(
            texture_rect(&level, 0, 0),
            TileRect {
                x: 0,
                y: 0,
                width: 513,
                height: 513,
            }
        );
        assert_eq!(
            texture_rect(&level, 1, 0),
            TileRect {
                x: 511,
                y: 0,
                width: 514,
                height: 513,
            }
        );
        assert_eq!(
            texture_rect(&level, 3, 1),
            TileRect {
                x: 1535,
                y: 511,
                width: 465,
                height: 489,
            }
        );
        assert!(texture_rect(&level, 99, 0).is_empty());
        assert!(texture_rect(&level, 0, 99).is_empty());
    }

    #[test]
    fn source_rect_rounds_outwards() {
        let list = levels(2000, 1000, 512);
        let level0 = by_index(&list, 0);
        assert_eq!(
            source_rect(&level0, texture_rect(&level0, 1, 0), 2000, 1000),
            TileRect {
                x: 511,
                y: 0,
                width: 514,
                height: 513,
            }
        );

        let level1 = by_index(&list, 1);
        assert_eq!(
            source_rect(&level1, texture_rect(&level1, 1, 0), 2000, 1000),
            TileRect {
                x: 1022,
                y: 0,
                width: 978,
                height: 1000,
            }
        );

        let top = by_index(&list, 2);
        assert_eq!(
            source_rect(&top, texture_rect(&top, 0, 0), 2000, 1000),
            TileRect {
                x: 0,
                y: 0,
                width: 2000,
                height: 1000,
            }
        );
    }

    #[test]
    fn source_rect_of_empty_layer_is_empty() {
        let level = LevelMeta {
            index: 0,
            width: 0,
            height: 0,
            tiles_x: 0,
            tiles_y: 0,
        };
        assert_eq!(
            source_rect(
                &level,
                TileRect {
                    x: 0,
                    y: 0,
                    width: 0,
                    height: 0,
                },
                100,
                100,
            ),
            TileRect {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            }
        );
    }

    #[test]
    fn display_rect_maps_layer_pixels_to_full_image() {
        let list = levels(2000, 1000, 512);
        let level1 = by_index(&list, 1);
        let rect = display_rect(&level1, texture_rect(&level1, 0, 0), 600.0, 300.0);
        assert!((rect.x - 0.0).abs() < 0.001);
        assert!((rect.y - 0.0).abs() < 0.001);
        assert!((rect.w - 307.8).abs() < 0.01);
        assert!((rect.h - 300.0).abs() < 0.01);

        let top = by_index(&list, 2);
        let full = display_rect(&top, texture_rect(&top, 0, 0), 600.0, 300.0);
        assert!((full.x - 0.0).abs() < 0.001);
        assert!((full.y - 0.0).abs() < 0.001);
        assert!((full.w - 600.0).abs() < 0.001);
        assert!((full.h - 300.0).abs() < 0.001);

        assert_eq!(
            display_rect(&level1, texture_rect(&level1, 0, 0), 0.0, 300.0),
            Rect::new(0.0, 0.0, 0.0, 0.0)
        );
    }

    #[test]
    fn visible_tiles_are_row_major_and_clipped() {
        let list = levels(2000, 1000, 512);
        let level = by_index(&list, 0);
        let rev = revision(2000, 1000);
        let keys = visible_tiles(rev, &level, visible(0.0, 0.0, 10.0, 10.0), 2000.0, 1000.0);
        assert_eq!(
            keys,
            vec![TileKey {
                revision: rev,
                level: 0,
                col: 0,
                row: 0,
            }]
        );

        let keys = visible_tiles(
            rev,
            &level,
            visible(511.0, 511.0, 10.0, 10.0),
            2000.0,
            1000.0,
        );
        assert_eq!(
            keys.iter()
                .map(|key| (key.col, key.row))
                .collect::<Vec<_>>(),
            vec![(0, 0), (1, 0), (0, 1), (1, 1)]
        );

        let keys = visible_tiles(
            rev,
            &level,
            visible(1999.0, 999.0, 100.0, 100.0),
            2000.0,
            1000.0,
        );
        assert_eq!(
            keys.iter()
                .map(|key| (key.col, key.row))
                .collect::<Vec<_>>(),
            vec![(3, 1)]
        );

        assert!(
            visible_tiles(
                rev,
                &level,
                visible(2000.0, 1000.0, 10.0, 10.0),
                2000.0,
                1000.0
            )
            .is_empty()
        );
        assert!(
            visible_tiles(
                rev,
                &level,
                visible(-50.0, -50.0, 40.0, 40.0),
                2000.0,
                1000.0
            )
            .is_empty()
        );
    }

    #[test]
    fn visible_tiles_do_not_include_tile_touching_only_the_edge() {
        let list = levels(2000, 1000, 512);
        let level = by_index(&list, 0);
        let rev = revision(2000, 1000);
        let keys = visible_tiles(
            rev,
            &level,
            visible(512.0, 0.0, 512.0, 10.0),
            2000.0,
            1000.0,
        );
        assert_eq!(
            keys.iter()
                .map(|key| (key.col, key.row))
                .collect::<Vec<_>>(),
            vec![(1, 0)]
        );
    }

    #[test]
    fn visible_tiles_map_display_to_layer_with_scale() {
        let list = levels(2000, 1000, 512);
        let level1 = by_index(&list, 1);
        let rev = revision(2000, 1000);
        let keys = visible_tiles(rev, &level1, visible(0.0, 0.0, 600.0, 300.0), 600.0, 300.0);
        assert_eq!(
            keys.iter()
                .map(|key| (key.col, key.row))
                .collect::<Vec<_>>(),
            vec![(0, 0), (1, 0)]
        );

        let level0 = by_index(&list, 0);
        let keys = visible_tiles(
            rev,
            &level0,
            visible(-10.0, -10.0, 610.0, 310.0),
            600.0,
            300.0,
        );
        assert_eq!(
            keys.iter()
                .map(|key| (key.col, key.row))
                .collect::<Vec<_>>(),
            vec![
                (0, 0),
                (1, 0),
                (2, 0),
                (3, 0),
                (0, 1),
                (1, 1),
                (2, 1),
                (3, 1)
            ]
        );
    }

    #[test]
    fn plan_includes_top_under_missing_target_tiles() {
        let list = levels(2000, 1000, 512);
        let rev = revision(2000, 1000);
        let top = fake_image(500, 250);
        let mut lod = LodState::new();
        let plan = plan_block(
            rev,
            &list,
            Some(&top),
            &mut lod,
            &geometry(600.0, 300.0, visible(0.0, 0.0, 600.0, 300.0)),
            1,
            |_| None,
        );
        assert_eq!(plan.target, 1);
        assert_eq!(plan.layers.len(), 1);
        assert_eq!(plan.layers[0].level, 2);
        assert_eq!(plan.layers[0].tiles.len(), 1);
        assert_eq!(
            plan.layers[0].tiles[0].rect,
            Rect::new(0.0, 0.0, 600.0, 300.0)
        );
        assert_eq!(
            plan.missing
                .iter()
                .map(|key| (key.level, key.col, key.row))
                .collect::<Vec<_>>(),
            vec![(1, 0, 0), (1, 1, 0)]
        );
        assert!(plan.missing.iter().all(|key| key.revision == rev));
        assert!(!plan.top_missing);
    }

    #[test]
    fn plan_keeps_ready_target_tiles_and_lists_only_missing_ones() {
        let list = levels(2000, 1000, 512);
        let rev = revision(2000, 1000);
        let top = fake_image(500, 250);
        let target_image = fake_image(514, 513);
        let mut lod = LodState::new();
        let plan = plan_block(
            rev,
            &list,
            Some(&top),
            &mut lod,
            &geometry(600.0, 300.0, visible(0.0, 0.0, 600.0, 300.0)),
            2,
            |key| {
                (key.level == 1 && key.col == 0 && key.row == 0).then(|| Arc::clone(&target_image))
            },
        );
        assert_eq!(plan.layers.len(), 2);
        assert_eq!(plan.layers[0].level, 2);
        assert_eq!(plan.layers[1].level, 1);
        assert_eq!(plan.layers[1].tiles.len(), 1);
        assert_eq!(plan.layers[1].tiles[0].key.col, 0);
        assert_eq!(
            plan.layers[1].tiles[0].rect,
            display_rect(
                &by_index(&list, 1),
                texture_rect(&by_index(&list, 1), 0, 0),
                600.0,
                300.0
            )
        );
        assert_eq!(
            plan.missing
                .iter()
                .map(|key| (key.level, key.col, key.row))
                .collect::<Vec<_>>(),
            vec![(1, 1, 0)]
        );
        assert!(!plan.top_missing);
    }

    #[test]
    fn plan_orders_layers_coarse_to_fine() {
        let list = levels(2000, 1000, 512);
        let rev = revision(2000, 1000);
        let top = fake_image(500, 250);
        let level1_image = fake_image(514, 513);
        let mut lod = LodState::new();
        let plan = plan_block(
            rev,
            &list,
            Some(&top),
            &mut lod,
            &geometry(2000.0, 1000.0, visible(0.0, 0.0, 2000.0, 1000.0)),
            0,
            |key| (key.level == 1).then(|| Arc::clone(&level1_image)),
        );
        assert_eq!(plan.target, 0);
        assert_eq!(
            plan.layers
                .iter()
                .map(|layer| layer.level)
                .collect::<Vec<_>>(),
            vec![2, 1]
        );
        assert_eq!(plan.layers[1].tiles.len(), 2);
        assert_eq!(plan.missing.len(), 8);
        assert!(plan.missing.iter().all(|key| key.level == 0));
    }

    #[test]
    fn plan_uses_top_alone_at_coarsest_display() {
        let list = levels(2000, 1000, 512);
        let rev = revision(2000, 1000);
        let top = fake_image(500, 250);
        let mut lod = LodState::new();
        let plan = plan_block(
            rev,
            &list,
            Some(&top),
            &mut lod,
            &geometry(100.0, 50.0, visible(0.0, 0.0, 100.0, 50.0)),
            0,
            |_| panic!("顶层不应查询图块缓存"),
        );
        assert_eq!(plan.target, 2);
        assert_eq!(plan.layers.len(), 1);
        assert_eq!(plan.layers[0].level, 2);
        assert_eq!(plan.layers[0].tiles.len(), 1);
        assert_eq!(
            plan.layers[0].tiles[0].key,
            TileKey {
                revision: rev,
                level: 2,
                col: 0,
                row: 0,
            }
        );
        assert!(plan.missing.is_empty());
        assert!(!plan.top_missing);
    }

    #[test]
    fn plan_marks_missing_top_and_still_draws_ready_layers() {
        let list = levels(2000, 1000, 512);
        let rev = revision(2000, 1000);
        let target_image = fake_image(514, 513);
        let mut lod = LodState::new();
        let plan = plan_block(
            rev,
            &list,
            None,
            &mut lod,
            &geometry(600.0, 300.0, visible(0.0, 0.0, 600.0, 300.0)),
            0,
            |key| (key.level == 1 && key.col == 0).then(|| Arc::clone(&target_image)),
        );
        assert_eq!(plan.target, 1);
        assert!(plan.top_missing);
        assert_eq!(plan.layers.len(), 1);
        assert_eq!(plan.layers[0].level, 1);
        assert_eq!(plan.layers[0].tiles.len(), 1);
        assert_eq!(plan.missing.len(), 1);
    }

    #[test]
    fn plan_without_top_and_target_top_has_nothing_to_draw() {
        let list = levels(2000, 1000, 512);
        let rev = revision(2000, 1000);
        let mut lod = LodState::new();
        let plan = plan_block(
            rev,
            &list,
            None,
            &mut lod,
            &geometry(100.0, 50.0, visible(0.0, 0.0, 100.0, 50.0)),
            0,
            |_| panic!("目标层是顶层，不应查询图块缓存"),
        );
        assert_eq!(plan.target, 2);
        assert!(plan.top_missing);
        assert!(plan.layers.is_empty());
        assert!(plan.missing.is_empty());
    }

    #[test]
    fn plan_skips_everything_when_visible_area_is_empty() {
        let list = levels(2000, 1000, 512);
        let rev = revision(2000, 1000);
        let top = fake_image(500, 250);
        let mut lod = LodState::new();
        let plan = plan_block(
            rev,
            &list,
            Some(&top),
            &mut lod,
            &geometry(600.0, 300.0, visible(3000.0, 2000.0, 10.0, 10.0)),
            0,
            |_| panic!("看不见的块不应查询图块缓存"),
        );
        assert!(plan.layers.is_empty());
        assert!(plan.missing.is_empty());
        assert!(!plan.top_missing);

        let plan = plan_block(
            rev,
            &list,
            None,
            &mut lod,
            &geometry(600.0, 300.0, visible(0.0, 0.0, 0.0, 0.0)),
            1,
            |_| None,
        );
        assert!(plan.layers.is_empty());
        assert!(plan.missing.is_empty());
        assert!(plan.top_missing);
    }

    #[test]
    fn plan_uses_device_scale_for_target_level() {
        let list = levels(2000, 1000, 512);
        let rev = revision(2000, 1000);
        let top = fake_image(500, 250);
        let mut lod = LodState::new();
        let mut zoomed = geometry(500.0, 250.0, visible(0.0, 0.0, 500.0, 250.0));
        zoomed.device_scale = 2.0;
        let plan = plan_block(rev, &list, Some(&top), &mut lod, &zoomed, 0, |_| None);
        assert_eq!(plan.target, 1);
        assert_eq!(plan.missing.len(), 2);
    }
}
