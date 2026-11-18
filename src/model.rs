//! 应用数据模型：项目、图片条目、设置项。

use std::path::PathBuf;
use std::sync::Arc;

use image::RgbImage;

use crate::imaging::decode::{self, DecodeError, ImageFingerprint};

pub type ItemId = u64;

/// 按 id 序列重排：把 moving 里的 id 移到 before 之前（None 表示末尾），
/// 其余项和 moving 内部都保持原有先后顺序。
///
/// 拖动排序的「松手前预览」和「松手后提交」共用这一份逻辑，两边的顺序必然一致。
pub fn reorder_ids(ids: &[ItemId], moving: &[ItemId], before: Option<ItemId>) -> Vec<ItemId> {
    let mut rest: Vec<ItemId> = ids
        .iter()
        .copied()
        .filter(|id| !moving.contains(id))
        .collect();
    let insert_at = match before {
        Some(target) => rest.iter().position(|id| *id == target).unwrap_or(rest.len()),
        None => rest.len(),
    };
    let moving_in_order: Vec<ItemId> = ids
        .iter()
        .copied()
        .filter(|id| moving.contains(id))
        .collect();
    for (offset, id) in moving_in_order.into_iter().enumerate() {
        rest.insert(insert_at + offset, id);
    }
    rest
}

/// 两种工作模式。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Mode {
    /// 找字幕、按规则裁剪、从上到下拼接
    #[default]
    Subtitle,
    /// 不裁字幕，把图片自动排成目标比例或横排/竖排
    Layout,
}

impl Mode {
    /// 稳定的内部标识，用于界面元素 id，不随语言变化。
    pub fn key(self) -> &'static str {
        match self {
            Mode::Subtitle => "subtitle",
            Mode::Layout => "layout",
        }
    }
}

/// 一个文字行框，坐标是原图像素。
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LineBox {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
}

impl LineBox {
    pub fn height(self) -> f32 {
        (self.y1 - self.y0).max(1.0)
    }

    pub fn width(self) -> f32 {
        (self.x1 - self.x0).max(1.0)
    }

    pub fn center_x(self) -> f32 {
        (self.x0 + self.x1) * 0.5
    }

    pub fn center_y(self) -> f32 {
        (self.y0 + self.y1) * 0.5
    }

    /// 两行在竖直方向的重叠比例（以较矮的一行为分母）。
    pub fn vertical_overlap_ratio(self, other: LineBox) -> f32 {
        let overlap = (self.y1.min(other.y1) - self.y0.max(other.y0)).max(0.0);
        overlap / self.height().min(other.height())
    }

    /// 两行在水平方向的重叠比例（以较窄的一行为分母）。
    pub fn horizontal_overlap_ratio(self, other: LineBox) -> f32 {
        let overlap = (self.x1.min(other.x1) - self.x0.max(other.x0)).max(0.0);
        overlap / self.width().min(other.width())
    }

    pub fn union(self, other: LineBox) -> LineBox {
        LineBox {
            x0: self.x0.min(other.x0),
            y0: self.y0.min(other.y0),
            x1: self.x1.max(other.x1),
            y1: self.y1.max(other.y1),
        }
    }

    /// 整行是否完全落在 [top, bottom] 这段纵坐标里。
    pub fn inside_vertical(self, top: f32, bottom: f32) -> bool {
        self.y0 >= top && self.y1 <= bottom
    }
}

/// 字幕块：同一处字幕的一行或多行，行按从上到下排序。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SubtitleBlock {
    pub lines: Vec<LineBox>,
}

impl SubtitleBlock {
    pub fn bounds(&self) -> LineBox {
        let mut iter = self.lines.iter().copied();
        let first = match iter.next() {
            Some(line) => line,
            None => return LineBox::default(),
        };
        iter.fold(first, |acc, line| acc.union(line))
    }

    pub fn max_line_height(&self) -> f32 {
        self.lines
            .iter()
            .map(|line| line.height())
            .fold(0.0, f32::max)
    }

    pub fn center_x(&self) -> f32 {
        self.bounds().center_x()
    }

    pub fn center_y(&self) -> f32 {
        self.bounds().center_y()
    }
}

/// 单张图片的处理状态。
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ItemStatus {
    /// 正在解码或正在检测字幕
    Pending,
    /// 字幕检测完成（`block` 可能有值，也可能是“没找到”）
    Ready,
    /// 没有找到字幕时自动保留整张画面，用户也可以手动框选
    NoSubtitle,
    /// 解码失败
    Failed(String),
}

/// 图片的字幕分析状态，与界面呈现用的 `ItemStatus` 正交。
///
/// 拆开这两个维度是为了让「是否参与布局」不再依赖 `ItemStatus::Pending`：
/// 排版模式插入的图从未跑过 OCR，但也必须立刻出现在画布上。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AnalysisState {
    /// OCR 已完成，结果在 `Item.block` / `Item.status`
    #[default]
    Analyzed,
    /// 已入队，等待后台 OCR
    PendingAnalysis,
    /// OCR 正在后台执行
    Analyzing,
    /// 当前模式不需要 OCR，且尚未分析过（排版模式插入）
    NotNeeded,
}

impl AnalysisState {
    /// 是否还需要等待 OCR 结果。
    pub fn is_pending(self) -> bool {
        matches!(self, AnalysisState::PendingAnalysis | AnalysisState::Analyzing)
    }
}

/// 用户手动框选的矩形范围，按图片宽高归一化。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ManualBand {
    /// 左边界，0.0 到 1.0
    pub left: f32,
    /// 右边界，0.0 到 1.0，必须大于 left
    pub right: f32,
    /// 上边界，0.0 到 1.0
    pub top: f32,
    /// 下边界，0.0 到 1.0，必须大于 top
    pub bottom: f32,
}

impl ManualBand {
    /// 整张图的归一化范围。
    pub const FULL: Self = ManualBand {
        left: 0.0,
        right: 1.0,
        top: 0.0,
        bottom: 1.0,
    };

    /// 排版模式拖动边缘时，一条边至少要留下的原图比例：宽和高各 15%。
    ///
    /// 排版模式要把图排成卡片，太小就抓不住、也排不开，所以下限保持在 15%。
    pub const MIN_KEEP_RATIO: f32 = 0.15;

    /// 排版模式保留范围至少要留下的面积比例，按原图面积算：15% 乘 15%，也就是 2.25%。
    /// 宽和高各自都不低于 `MIN_KEEP_RATIO` 时，面积自然满足这一条。
    pub const MIN_KEEP_AREA: f32 = Self::MIN_KEEP_RATIO * Self::MIN_KEEP_RATIO;

    /// 字幕模式拖动上下裁剪线时，高度至少要留下的原图比例：2%。
    ///
    /// 下限按「屏幕上还抓得住裁剪线」来定：作品在窗口里按显示比例缩小后，2% 的
    /// 高度在常见的 4K 截图（2160 像素高）按窗口约 0.22 倍显示时约 10 屏幕像素，
    /// 上下两条裁剪热区还点得中；再小就会细到鼠标对不准，收小以后抓不回来。
    ///
    /// 2% 同时低于自动识别出的最细字幕带：识别时一行字幕至少占原图高度 3%
    /// （见 MIN_LINE_HEIGHT_RATIO），所以手动一定能把范围收到比自动识别更小，
    /// 用户抱怨的「手动最小范围比自动识别范围还大」不再出现。
    ///
    /// 这里不套面积下限：横排字幕带的横向范围由自动识别决定，可能本来就很窄，
    /// 拿面积下限去除会把纵向的最小高度撑得比自动识别范围还大，正是用户抱怨的情况。
    pub const SUBTITLE_MIN_KEEP_RATIO: f32 = 0.02;

    /// 宽或高小于 `MIN_SPAN` 时视为没有面积，返回 `None`。
    ///
    /// 这里只排除「完全没有面积」的范围，不判断最小保留比例。旧数据、外部导入的
    /// 数据里可能有非常窄的裁剪带，原图高度百分之几的带都得正常认。
    /// 早先按固定的 1% 比例判废，这种图的热区照常画在屏幕上，按下去却什么也
    /// 不发生；用户看到的是「最后一张图的下边缘拖不动」，实际是这一步把裁剪带
    /// 判成了无效，拖动还没开始就返回了。最小保留比例由 `shift_edge` 在拖动时
    /// 施加，这里不再重复限制。
    pub const MIN_SPAN: f32 = 1e-6;

    /// 创建并夹紧到图片范围内；任一方向没有面积时返回 `None`。
    pub fn clamped(left: f32, right: f32, top: f32, bottom: f32) -> Option<Self> {
        if !left.is_finite() || !right.is_finite() || !top.is_finite() || !bottom.is_finite() {
            return None;
        }
        let left = left.clamp(0.0, 1.0);
        let right = right.clamp(0.0, 1.0);
        let top = top.clamp(0.0, 1.0);
        let bottom = bottom.clamp(0.0, 1.0);
        if right - left <= Self::MIN_SPAN || bottom - top <= Self::MIN_SPAN {
            return None;
        }
        Some(ManualBand {
            left,
            right,
            top,
            bottom,
        })
    }

    /// 只改纵向范围，左右保持不变。
    pub fn with_rows(self, top: f32, bottom: f32) -> Option<Self> {
        Self::clamped(self.left, self.right, top, bottom)
    }

    /// 只改横向范围，上下保持不变。
    pub fn with_columns(self, left: f32, right: f32) -> Option<Self> {
        Self::clamped(left, right, self.top, self.bottom)
    }

    /// 横向覆盖整幅宽度。
    pub fn spans_full_width(self) -> bool {
        self.left <= 0.0 && self.right >= 1.0
    }

    /// 纵向覆盖整幅高度。
    pub fn spans_full_height(self) -> bool {
        self.top <= 0.0 && self.bottom >= 1.0
    }
}

/// 一张图片的来源。界面只按需解码，不在 Item 里常驻整张像素。
#[derive(Clone)]
pub enum SourceImage {
    /// 文件导入：只记路径，预览和导出时按需读取。
    File(PathBuf),
    /// 剪贴板导入：没有原文件路径，保留编码后的原始字节。
    Clipboard(Arc<Vec<u8>>),
    /// 测试构造：直接给像素，避免每个无头测试都写临时 PNG。
    /// 生产代码不会创建这个分支。
    Pixels(Arc<RgbImage>),
}

impl SourceImage {
    /// 读出整张 RGB 像素。调用方用完必须立刻丢掉，不要存回 Item。
    pub fn load_rgb(&self) -> Result<RgbImage, DecodeError> {
        match self {
            SourceImage::File(path) => decode::decode_path(path),
            SourceImage::Clipboard(bytes) => decode::decode_bytes(bytes),
            SourceImage::Pixels(rgb) => Ok((**rgb).clone()),
        }
    }
}

/// 项目里的一张图片。
#[derive(Clone)]
pub struct Item {
    pub id: ItemId,
    pub name: String,
    pub width: u32,
    pub height: u32,
    /// 原图来源；撤销快照只共享路径或字节，不共享整张像素
    pub source: SourceImage,
    /// 检测到的字幕块
    pub block: Option<SubtitleBlock>,
    pub status: ItemStatus,
    /// 没有自动结果或用户主动选择时保留整张图
    pub keep_whole: bool,
    /// 用户手动框选的矩形范围；有值时优先于自动结果
    pub manual: Option<ManualBand>,
    /// 字幕分析状态（与 `status` 正交）
    pub analysis: AnalysisState,
    /// 原图像素指纹，删除再添加 / 切换模式时用来复用识别结果
    pub fingerprint: ImageFingerprint,
}

impl Item {
    /// 按来源读出整张像素。调用方用完立刻丢掉。
    pub fn load_rgb(&self) -> Result<RgbImage, DecodeError> {
        self.source.load_rgb()
    }

    pub fn aspect(&self) -> f32 {
        self.width as f32 / self.height.max(1) as f32
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}

/// 一个 8 位 RGBA 颜色，用于缝隙等。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgba {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Rgba {
    /// 纯白
    pub const WHITE: Rgba = Rgba {
        r: 255,
        g: 255,
        b: 255,
        a: 255,
    };
    /// 炭黑
    pub const CHARCOAL: Rgba = Rgba {
        r: 24,
        g: 24,
        b: 27,
        a: 255,
    };
    /// 暖灰
    pub const WARM_GREY: Rgba = Rgba {
        r: 113,
        g: 113,
        b: 122,
        a: 255,
    };
    /// 导出时的兜底底色，不在界面上出现
    pub const BLACK: Rgba = Rgba {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };

    pub fn to_image_rgba(self) -> image::Rgba<u8> {
        image::Rgba([self.r, self.g, self.b, self.a])
    }
}

impl Default for Rgba {
    fn default() -> Self {
        Rgba::WHITE
    }
}

/// 缝隙设置：默认不留缝，可开启并选颜色。
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GapSetting {
    pub enabled: bool,
    pub px: u32,
    pub color: Rgba,
}

impl Default for GapSetting {
    fn default() -> Self {
        GapSetting {
            enabled: false,
            px: 8,
            color: Rgba::WHITE,
        }
    }
}

/// 接缝可以循环切换的三种底色，顺序就是点击时的切换顺序。
pub const GAP_COLORS: [Rgba; 3] = [Rgba::WHITE, Rgba::CHARCOAL, Rgba::WARM_GREY];

impl GapSetting {
    /// 实际生效的缝隙像素宽度（关闭时为 0）。
    pub fn effective_px(&self) -> u32 {
        if self.enabled { self.px } else { 0 }
    }

    /// 按拖动换算出新的缝隙设置，颜色保持不变。
    ///
    /// 拉到 0 像素等于把缝隙关掉，和「严丝合缝」是同一件事；
    /// 拉开到 1 像素以上就自动打开，不需要再单独点一个开关。
    pub fn with_px(self, px: u32) -> Self {
        GapSetting {
            enabled: px > 0,
            px,
            color: self.color,
        }
    }

    /// 保持当前宽度和开关，只换颜色。
    pub fn with_color(mut self, color: Rgba) -> Self {
        self.color = color;
        self
    }

    /// 下一个颜色。点击接缝时按 纯白 -> 炭黑 -> 暖灰 -> 纯白 循环。
    pub fn next_color(current: Rgba) -> Rgba {
        let index = GAP_COLORS
            .iter()
            .position(|color| *color == current)
            .unwrap_or(0);
        GAP_COLORS[(index + 1) % GAP_COLORS.len()]
    }
}

/// 排版目标。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LayoutTarget {
    /// 宽高比目标
    Aspect(f32),
    /// 横向一排，统一高度（取现有最高的那张，其它图等比缩放）
    Row,
    /// 竖向一列，统一宽度（取现有最宽的那张，其它图等比缩放）
    Column,
    /// 循环排版：每点一次换下一种拼法，数字是当前第几种版式（从 0 开始）
    Shuffle(usize),
}

impl LayoutTarget {
    /// 底部胶囊里的三个预设：横排、竖排、循环排版。
    ///
    /// 固定比例算法和预设仍保留在代码、测试和示例里，只是不再出现在界面上。
    pub const PRESETS: [LayoutTarget; 3] = [
        LayoutTarget::Row,
        LayoutTarget::Column,
        LayoutTarget::Shuffle(0),
    ];

    /// 稳定的内部标识，用于界面元素 id，不随语言变化。
    ///
    /// 循环排版的序号是动态的，所以用 `String`；其它变体仍返回静态字符串。
    pub fn key(self) -> String {
        match self {
            LayoutTarget::Aspect(ratio) => {
                if (ratio - 16.0 / 9.0).abs() < 1e-3 {
                    "16-9"
                } else if (ratio - 9.0 / 16.0).abs() < 1e-3 {
                    "9-16"
                } else if (ratio - 4.0 / 3.0).abs() < 1e-3 {
                    "4-3"
                } else if (ratio - 1.0).abs() < 1e-3 {
                    "1-1"
                } else {
                    "custom"
                }
                .to_string()
            }
            LayoutTarget::Row => "row".to_string(),
            LayoutTarget::Column => "column".to_string(),
            LayoutTarget::Shuffle(index) => format!("shuffle-{index}"),
        }
    }

    /// 点击循环排版按钮时切到下一种版式。
    ///
    /// 当前不在循环排版时从 0 开始；序号用 `saturating_add` 防止溢出。
    pub fn next_shuffle(current: LayoutTarget) -> LayoutTarget {
        match current {
            LayoutTarget::Shuffle(index) => LayoutTarget::Shuffle(index.saturating_add(1)),
            _ => LayoutTarget::Shuffle(0),
        }
    }
}

impl Default for LayoutTarget {
    fn default() -> Self {
        LayoutTarget::Shuffle(0)
    }
}

/// 所有可调设置。
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// 是否裁剪字幕
    pub crop: bool,
    /// 是否保留第二行字幕
    pub keep_second: bool,
    /// 出血位：相对字幕文字高度的比例（0.0 ~ 0.30）
    pub bleed: f32,
    /// 图片之间的缝隙
    pub gap: GapSetting,
    /// 排版目标
    pub layout: LayoutTarget,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            crop: true,
            keep_second: false,
            bleed: 0.25,
            gap: GapSetting::default(),
            layout: LayoutTarget::default(),
        }
    }
}

/// 整个项目。
pub struct Project {
    pub mode: Mode,
    pub settings: Settings,
    pub items: Vec<Item>,
}

impl Default for Project {
    fn default() -> Self {
        Project {
            mode: Mode::Subtitle,
            settings: Settings::default(),
            items: Vec::new(),
        }
    }
}

impl Project {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gap_colors_cycle_through_three_values() {
        // 点击接缝时的顺序：纯白 -> 炭黑 -> 暖灰 -> 纯白
        assert_eq!(GapSetting::next_color(Rgba::WHITE), Rgba::CHARCOAL);
        assert_eq!(GapSetting::next_color(Rgba::CHARCOAL), Rgba::WARM_GREY);
        assert_eq!(GapSetting::next_color(Rgba::WARM_GREY), Rgba::WHITE);
    }

    #[test]
    fn gap_color_cycle_survives_unknown_color() {
        // 配置文件里手改出来的颜色不在三色里，从第一档重新开始，不会卡住。
        let odd = Rgba {
            r: 1,
            g: 2,
            b: 3,
            a: 255,
        };
        assert_eq!(GapSetting::next_color(odd), Rgba::CHARCOAL);
    }

    #[test]
    fn gap_px_zero_means_closed() {
        let gap = GapSetting::default().with_px(0);
        assert!(!gap.enabled, "拉到 0 像素等于关掉缝隙");
        assert_eq!(gap.effective_px(), 0);

        let gap = gap.with_px(16);
        assert!(gap.enabled, "拉开就要自动打开");
        assert_eq!(gap.effective_px(), 16);
    }

    #[test]
    fn gap_keeps_color_when_width_changes() {
        let gap = GapSetting::default()
            .with_color(Rgba::WARM_GREY)
            .with_px(24);
        assert_eq!(gap.color, Rgba::WARM_GREY);
        assert_eq!(gap.px, 24);
    }

    #[test]
    fn next_shuffle_cycles_and_start_from_zero() {
        assert_eq!(
            LayoutTarget::next_shuffle(LayoutTarget::Aspect(16.0 / 9.0)),
            LayoutTarget::Shuffle(0)
        );
        assert_eq!(
            LayoutTarget::next_shuffle(LayoutTarget::Row),
            LayoutTarget::Shuffle(0)
        );
        assert_eq!(
            LayoutTarget::next_shuffle(LayoutTarget::Shuffle(0)),
            LayoutTarget::Shuffle(1)
        );
        assert_eq!(
            LayoutTarget::next_shuffle(LayoutTarget::Shuffle(2)),
            LayoutTarget::Shuffle(3)
        );
        assert_eq!(
            LayoutTarget::next_shuffle(LayoutTarget::Shuffle(usize::MAX)),
            LayoutTarget::Shuffle(usize::MAX),
            "序号到最大也不能溢出"
        );
        assert_eq!(LayoutTarget::default(), LayoutTarget::Shuffle(0));
        assert_eq!(LayoutTarget::Shuffle(3).key(), "shuffle-3");
        assert_eq!(LayoutTarget::PRESETS.len(), 3);
    }
}
