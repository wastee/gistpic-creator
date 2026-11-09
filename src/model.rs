//! 应用数据模型：项目、图片条目、设置项。

use std::sync::Arc;

use image::RgbImage;

pub type ItemId = u64;

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
    pub fn label(self) -> &'static str {
        match self {
            Mode::Subtitle => "字幕拼接",
            Mode::Layout => "排版",
        }
    }
}

/// 字幕的搜索范围。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SearchRegion {
    /// 自动：画面顶部 20% 与底部 28% 都找
    #[default]
    Auto,
    /// 只在画面顶部 45% 找
    Top,
    /// 只在画面底部 45% 找
    Bottom,
    /// 整张图都找
    Whole,
}

impl SearchRegion {
    pub const ALL: [SearchRegion; 4] = [
        SearchRegion::Auto,
        SearchRegion::Top,
        SearchRegion::Bottom,
        SearchRegion::Whole,
    ];

    pub fn label(self) -> &'static str {
        match self {
            SearchRegion::Auto => "自动",
            SearchRegion::Top => "仅顶部",
            SearchRegion::Bottom => "仅底部",
            SearchRegion::Whole => "整张图",
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
    /// 没有找到字幕，等待用户在弹窗里决定
    NoSubtitle,
    /// 解码失败
    Failed(String),
}

/// 项目里的一张图片。
#[derive(Clone)]
pub struct Item {
    pub id: ItemId,
    pub name: String,
    pub width: u32,
    pub height: u32,
    /// 原图像素（RGB8），撤销快照共享同一份
    pub rgb: Arc<RgbImage>,
    /// 检测到的字幕块
    pub block: Option<SubtitleBlock>,
    pub status: ItemStatus,
    /// 没找到字幕时用户在弹窗里选了“整张图保留”
    pub keep_whole: bool,
}

impl Item {
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
    pub const BLACK: Rgba = Rgba {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };
    pub const WHITE: Rgba = Rgba {
        r: 255,
        g: 255,
        b: 255,
        a: 255,
    };
    pub const GREY: Rgba = Rgba {
        r: 40,
        g: 40,
        b: 40,
        a: 255,
    };

    pub fn to_image_rgba(self) -> image::Rgba<u8> {
        image::Rgba([self.r, self.g, self.b, self.a])
    }
}

impl Default for Rgba {
    fn default() -> Self {
        Rgba::BLACK
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
            color: Rgba::BLACK,
        }
    }
}

impl GapSetting {
    /// 实际生效的缝隙像素宽度（关闭时为 0）。
    pub fn effective_px(&self) -> u32 {
        if self.enabled { self.px } else { 0 }
    }
}

/// 排版目标。
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LayoutTarget {
    /// 宽高比目标
    Aspect(f32),
    /// 横向一排，统一高度
    Row,
    /// 竖向一列，统一宽度
    Column,
}

impl LayoutTarget {
    pub const PRESETS: [LayoutTarget; 6] = [
        LayoutTarget::Aspect(16.0 / 9.0),
        LayoutTarget::Aspect(9.0 / 16.0),
        LayoutTarget::Aspect(4.0 / 3.0),
        LayoutTarget::Aspect(1.0),
        LayoutTarget::Row,
        LayoutTarget::Column,
    ];

    pub fn label(self) -> &'static str {
        match self {
            LayoutTarget::Aspect(ratio) => {
                if (ratio - 16.0 / 9.0).abs() < 1e-3 {
                    "16:9"
                } else if (ratio - 9.0 / 16.0).abs() < 1e-3 {
                    "9:16"
                } else if (ratio - 4.0 / 3.0).abs() < 1e-3 {
                    "4:3"
                } else if (ratio - 1.0).abs() < 1e-3 {
                    "1:1"
                } else {
                    "自定义比例"
                }
            }
            LayoutTarget::Row => "横向排列",
            LayoutTarget::Column => "竖向排列",
        }
    }
}

impl Default for LayoutTarget {
    fn default() -> Self {
        LayoutTarget::Aspect(16.0 / 9.0)
    }
}

/// 所有可调设置。
#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    /// 是否裁剪字幕
    pub crop: bool,
    /// 字幕搜索范围
    pub region: SearchRegion,
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
            region: SearchRegion::Auto,
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

impl Project {
}
