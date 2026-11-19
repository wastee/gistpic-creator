//! 应用主状态：项目数据、图片导入、字幕检测调度、撤销/重做、导出。

pub mod actions;
mod canvas_gestures;
mod crop_drag;
pub mod keymap;

mod export;
pub(crate) mod tiles;
mod undo;

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::i18n::Language;
use crate::imaging::crop::{self, CropBand, Inclusion};
use crate::imaging::decode::{
    DecodeError, DecodedImage, ImageFingerprint, decode_bytes_with_fingerprint,
    decode_path_with_fingerprint, is_supported_path,
};
use crate::imaging::detect::{DetectError, SubtitleDetector};
use crate::imaging::render;
use crate::model::{
    AnalysisState, Item, ItemId, ItemStatus, LayoutTarget, ManualBand, Mode,
    Project, SubtitleBlock,
};
use crate::ui::canvas::Rect;
use crate::ui::ghost_crop::SnapTier;
use crate::ui::hover::{self, HoverKey};
use crate::ui::theme::Theme;
use crate::ui::tokens;
use gpui::prelude::*;
use gpui::{
    Animation, AnimationExt, ClipboardEntry, Context, ExternalPaths, FocusHandle, IntoElement,
    Modifiers, Render, RenderImage, SharedString, Window, WindowBounds, div, pulsating_between,
};

use actions::*;

/// 缩略图的宽度（像素）。导航器、图块没生成时的占位、拖动跟手小图都用它；
/// 只按宽缩放、高度等比，整张图都保留，显示时再按裁剪带裁切。
pub(crate) const THUMB_WIDTH: u32 = 256;
/// 撤销栈最多保留多少步。
const UNDO_LIMIT: usize = 100;
/// 吐司自动消失的时间。
const TOAST_MS: u64 = 2600;
/// 提示浮层出场淡出时长。
pub(crate) const TOAST_EXIT_MS: u64 = 130;

/// 一条轻量提示。吐司和托盘通知共用这个类型。
#[derive(Clone, Debug)]
pub struct Status {
    pub text: SharedString,
    pub error: bool,
}

impl Status {
    pub fn info(text: impl Into<SharedString>) -> Self {
        Status {
            text: text.into(),
            error: false,
        }
    }

    pub fn error(text: impl Into<SharedString>) -> Self {
        Status {
            text: text.into(),
            error: true,
        }
    }
}

// ----------------------------------------------------------------------
// 拖拽时携带的数据
// ----------------------------------------------------------------------

/// 拖拽排序或拖到删除区时携带的数据。
#[derive(Clone, Copy)]
pub struct DragItem {
    pub id: ItemId,
}

/// 导入队列里的一条请求：文件路径或剪贴板字节。
///
/// 解码在后台任务里执行，指纹也在同一任务里算，主线程只拿到 `DecodedImage`。
enum DecodeRequest {
    Path(PathBuf),
    Bytes { name: String, bytes: Arc<Vec<u8>> },
}

impl DecodeRequest {
    fn name(&self) -> String {
        match self {
            DecodeRequest::Path(path) => path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string()),
            DecodeRequest::Bytes { name, .. } => name.clone(),
        }
    }

    fn source(&self) -> crate::model::SourceImage {
        match self {
            DecodeRequest::Path(path) => crate::model::SourceImage::File(path.clone()),
            DecodeRequest::Bytes { bytes, .. } => {
                crate::model::SourceImage::Clipboard(bytes.clone())
            }
        }
    }

    fn decode(&self) -> Result<DecodedImage, DecodeError> {
        match self {
            DecodeRequest::Path(path) => decode_path_with_fingerprint(path),
            DecodeRequest::Bytes { bytes, .. } => decode_bytes_with_fingerprint(bytes),
        }
    }
}

/// 拖裁剪线时携带的数据。
#[derive(Clone, Copy)]
pub struct CropHandleDrag {
    pub id: ItemId,
    pub edge: ManualEdge,
}

/// 拖作品左侧缝隙控件时携带的数据。
///
/// 拖动只改缝隙宽度，跟具体是哪条接缝无关，所以不带字段，只当类型标识用。
#[derive(Clone, Copy)]
pub struct GapDrag;

/// 拖出血位滑条时携带的数据。
#[derive(Clone, Copy)]
pub struct BleedDrag;

/// 复制按钮当前用哪种尺寸输出成品。
///
/// 小图把成品最长边压到 2000 像素，原图按项目原尺寸输出。默认小图，
/// 在复制按钮上右键来回切换。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyMode {
    /// 小图：成品最长边不超过 2000 像素。
    Small,
    /// 原图：按项目原尺寸输出。
    Original,
}

impl Default for CopyMode {
    fn default() -> Self {
        CopyMode::Small
    }
}

/// 底部胶囊当前是哪种形态。
///
/// 拖动图片时它缩成一个安静的垃圾桶投放目标，鼠标压上去才张开变成危险色，
/// 松手在别处就取消。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapsuleTarget {
    /// 正常胶囊
    Idle,
    /// 拖动中，安静的垃圾桶加张数
    Dragging,
    /// 鼠标停在垃圾桶上，可以松手删除
    Armed,
}

impl CapsuleTarget {
    /// 根据拖动和悬停状态算出当前形态。
    pub fn for_state(dragging: bool, delete_hover: bool) -> Self {
        if !dragging {
            CapsuleTarget::Idle
        } else if delete_hover {
            CapsuleTarget::Armed
        } else {
            CapsuleTarget::Dragging
        }
    }
}

/// 裁剪线拖的是哪条边。
///
/// 字幕模式只有上下两条边（只裁纵向）；排版模式四条边都能拖，
/// 上下边改纵向范围，左右边改横向范围。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManualEdge {
    Top,
    Bottom,
    Left,
    Right,
}

impl ManualEdge {
    /// 调试选择器和元素 id 里用的英文名。
    pub fn name(self) -> &'static str {
        match self {
            ManualEdge::Top => "top",
            ManualEdge::Bottom => "bottom",
            ManualEdge::Left => "left",
            ManualEdge::Right => "right",
        }
    }

    /// 这条边改的是纵向范围还是横向范围。
    pub fn is_columns(self) -> bool {
        matches!(self, ManualEdge::Left | ManualEdge::Right)
    }
}

pub(crate) use canvas_gestures::MiddlePan;
pub(crate) use crop_drag::edges_for as crop_edges_for;

/// 一次裁剪线拖动中的全部状态。
#[derive(Clone)]
pub(crate) struct CropDragState {
    pub id: ItemId,
    pub edge: ManualEdge,
    /// 被拖的这张图在按下时的保留带
    pub start: ManualBand,
    /// 这一拖会改到的所有图，以及它们在按下时各自的保留带。
    /// 每次都从按下时的值算起，不能拿已经改过的值再叠加。
    pub targets: Vec<(ItemId, ManualBand)>,
    /// 按下时指针在窗口里的位置
    pub start_pointer: gpui::Point<f32>,
    /// 按下时钉住的画布边：拖哪条边就钉住对面那条边，画面朝被拖的那条边伸缩。
    pub frame: crate::ui::canvas::FrameAnchor,
    /// 按下时的滚动偏移，锚定补偿以它为基准。
    pub scroll_start: gpui::Point<f32>,
    /// 按下时的可滚动范围 `(横向, 纵向)`，用来把滚动区的内容尺寸钉住。
    ///
    /// 裁剪会让作品变小，内容跟着变矮，滚动区每一帧都会把偏移夹回新的范围里，
    /// 画面于是被一股一股地拽向作品开头。拖第 3 张的上边缘时，看到的就是「突然
    /// 跳到第一张的视角」。按下时记下这个范围，拖动期间按它撑住内容尺寸，滚动
    /// 位置就不会被夹。
    pub frozen_max_scroll: (f32, f32),
    /// 这一拖累计自动滚动掉的像素
    pub auto_scroll_pixels: gpui::Point<f32>,
    /// 原图宽高
    pub image_size: (u32, u32),
    /// 冻结的显示比例（显示像素 / 原图像素），拖动全程不变。
    pub scale: gpui::Point<f32>,
    pub undo_recorded: bool,
    pub snap: Option<SnapTier>,
}

/// 一次缝隙宽度拖动中的状态。
///
/// 两种模式共用同一个控件，拖的永远是作品缝隙宽度，所以不记是哪条接缝。
pub(crate) struct GapDragState {
    pub start_gap_px: u32,
    pub start_pointer: f32,
    /// 显示像素 / 输出像素
    pub scale: f32,
    pub undo_recorded: bool,
    /// 当前指针的窗口坐标，横排气泡跟着它走。
    pub pointer: gpui::Point<gpui::Pixels>,
}

/// 需求 15：正在拖导航器里的视口框。
///
/// 只记录「按下时指针相对框左上角的偏移」，每次移动都从这个偏移重新算，
/// 不做累加：累加会把每一帧的取整误差攒起来，拖久了框会跑偏。
#[derive(Clone, Copy)]
pub(crate) struct NavigatorDragState {
    /// 按下时指针相对视口框左上角的偏移（导航器自己的坐标系）。
    pub grab: gpui::Point<f32>,
}

/// 拖导航器视口框时交给 GPUI 的类型标识，不画跟手小图。
#[derive(Clone, Copy)]
pub(crate) struct NavigatorDrag;

/// 一次拉框多选的状态。坐标是作品内容坐标（左上角为原点）。
pub(crate) struct MarqueeDrag {
    pub start: gpui::Point<f32>,
    pub current: gpui::Point<f32>,
    pub moved: bool,
    pub additive: bool,
}

/// 右键在画布空白处按下、还没抬起的状态。
///
/// 右键既要能弹画布卡片，又要能拖动框选：按下先记下来，移动超过阈值才开始框选，
/// 没移动就抬起则当成一次右键单击，弹出画布卡片。
#[derive(Clone, Copy)]
pub(crate) struct RightPress {
    pub start_content: gpui::Point<f32>,
    pub start_window: gpui::Point<gpui::Pixels>,
    pub additive: bool,
}

/// 没有浮层可关时，Esc 应该做什么。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EscapeOutcome {
    /// 隐藏窗口，程序继续在托盘运行。
    HideToTray,
    /// 什么都不做。
    DoNothing,
}

/// 根据“关闭窗口缩到托盘”的设置，决定 Esc 的行为。
pub(crate) fn escape_outcome(close_to_tray: bool) -> EscapeOutcome {
    if close_to_tray {
        EscapeOutcome::HideToTray
    } else {
        EscapeOutcome::DoNothing
    }
}

/// 这次保存该写哪个窗口矩形。
///
/// 最大化、全屏、macOS 的 zoom 状态都不能覆盖普通窗口尺寸，
/// 否则下次启动会把最大化后的屏幕尺寸当成普通尺寸恢复。
/// X11 的全屏窗口仍然报 `Windowed`，所以除了看矩形种类，还要单独看全屏标记。
pub(crate) fn windowed_rect_for_save(
    current: &WindowBounds,
    maximized: bool,
    fullscreen: bool,
    previous: Option<crate::config::WindowRect>,
) -> Option<crate::config::WindowRect> {
    match current {
        WindowBounds::Windowed(bounds) if !maximized && !fullscreen => {
            Some(crate::config::WindowRect::from_bounds(*bounds))
        }
        _ => previous,
    }
}

/// 复制或导出成功后正在倒计时的清空状态。
///
/// `generation` 用来区分先后几次倒计时：点掉覆盖或重新复制时旧计时器还活着，
/// 靠这个代号让过期的计时器不能清空图片。`remaining_secs` 直接给覆盖层显示。
#[derive(Clone, Copy, Debug)]
pub(crate) struct ClearCountdown {
    pub generation: u64,
    pub remaining_secs: u64,
}

impl ClearCountdown {
    /// 倒计时总秒数。
    pub(crate) const SECONDS: u64 = 3;
}

/// 右键卡片的类型和位置。
#[derive(Clone, Copy, Debug)]
pub struct MenuState {
    pub kind: MenuKind,
    /// 鼠标在主窗口内的坐标，菜单从这里紧贴鼠标弹出。
    pub x: f32,
    pub y: f32,
}

#[derive(Clone, Copy, Debug)]
pub enum MenuKind {
    Image(ItemId),
    Canvas,
}

/// 右键卡片里调整图片顺序的方向。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MoveDirection {
    Up,
    Down,
}

/// 排序松手后的位移动画数据：每个元素从哪个位置滑到新位置。
#[derive(Clone)]
pub(crate) struct DropSettle {
    /// 本次松手的代号，进动画 id 用。
    pub generation: u64,
    /// 图块旧位置（x, y）。
    pub from: HashMap<ItemId, (f32, f32)>,
    /// 接缝旧位置（x, y），键为接缝序号。
    pub from_seams: HashMap<usize, (f32, f32)>,
}

impl DropSettle {
    /// 松手后位移动画的时长。画面上的动画和这里必须用同一个值。
    pub(crate) const DURATION: std::time::Duration = std::time::Duration::from_millis(160);
}

/// 一次选中状态切换的起止集合，用来算每张图的选中、变淡和方勾进度。
pub(crate) struct SelectionAnim {
    pub(crate) from: BTreeSet<ItemId>,
    pub(crate) to: BTreeSet<ItemId>,
    pub(crate) started: std::time::Instant,
    pub(crate) generation: u64,
}

pub struct AppRoot {
    pub(crate) project: Project,
    pub(crate) undo: undo::UndoStack,
    pub(crate) detector: Option<Arc<SubtitleDetector>>,
    detector_loading: bool,
    pub(crate) next_id: ItemId,
    pub(crate) selected: Option<ItemId>,
    pub(crate) selected_ids: BTreeSet<ItemId>,
    /// 正在播放的选中切换动画。
    pub(crate) selection_anim: Option<SelectionAnim>,
    /// 选中切换动画的代号，连点时分清新旧任务。
    pub(crate) selection_anim_gen: u64,
    /// 全局悬停状态：同一时刻最多一个目标处于激活状态。
    pub(crate) hover_gate: hover::HoverGate,
    /// 正在拖动哪一段的裁剪线。
    pub(crate) crop_drag: Option<CropDragState>,
    /// 正在拖动哪条接缝。
    pub(crate) gap_drag: Option<GapDragState>,
    /// 正在拉框多选。
    pub(crate) marquee: Option<MarqueeDrag>,
    /// 右键在空白处按下、还没抬起。
    pub(crate) right_pending: Option<RightPress>,
    /// 正在拖动的图片；有值时底部胶囊变成删除区。
    pub(crate) dragging: Option<ItemId>,
    /// 拖动图片时指针在窗口里的位置，用来画跟手的半透明小图。
    pub(crate) drag_pointer: Option<gpui::Point<gpui::Pixels>>,
    /// 拖动时的插入位置：Some(None) 表示插到末尾。
    pub(crate) drag_over: Option<Option<ItemId>>,
    /// 拖动排序时是否已经挂上「贴边自动滚动」的定时器。
    pub(crate) auto_scroll_active: bool,
    /// 自动滚动的代号：离开边缘或结束拖动时递增，让还在等计时器的旧任务退出。
    pub(crate) auto_scroll_gen: u64,
    /// 最近一次排序松手的位移动画数据。
    pub(crate) settle: Option<DropSettle>,
    /// 最近一次接缝换色前的颜色，用于交叉淡化。
    pub(crate) prev_gap_color: Option<gpui::Hsla>,
    /// 当前接缝换色动画的开始时间，用于快速连点时衔接当前颜色。
    pub(crate) gap_color_started_at: Option<std::time::Instant>,
    /// 接缝换色的次数，作为动画 id 的代号。
    pub(crate) gap_color_gen: u64,
    /// 鼠标是否在底部删除区上.
    pub(crate) delete_hover: bool,
    /// 显示比例的来源：自动适应，还是用户定死的手动比例。
    pub(crate) scale_mode: tokens::ScaleMode,
    /// 最近一次渲染时的窗口大小，用来按黄金分割算默认显示比例。
    pub(crate) viewport: gpui::Size<gpui::Pixels>,
    /// 每张图当前屏幕可见的图块缓存，键是图片 id。只保留屏幕内该画的块。
    pub(crate) tile_cache: HashMap<ItemId, tiles::TileGrid>,
    /// 已经派出、正在后台生成图块的任务：图片 id -> 任务对应的代次。
    pub(crate) tile_jobs: HashMap<ItemId, u64>,
    /// 图块生成失败过的代次，避免同一次失败反复重试、反复弹提示。
    pub(crate) tile_failed: HashMap<ItemId, u64>,
    /// 每张图 256 宽整图缩略图：导航器、图块没生成时的占位、拖动跟手小图共用。
    pub(crate) thumb_cache: HashMap<ItemId, Arc<RenderImage>>,
    /// 正在后台生成缩略图的图片 id，避免同一张重复起任务。
    pub(crate) thumb_pending: HashSet<ItemId>,
    /// 缩略图读取失败过的图片，避免每帧都重新起解码任务、反复弹提示。
    pub(crate) thumb_failed: HashSet<ItemId>,
    /// 已经不需要、等下一帧交给窗口释放 GPU 纹理的位图。
    pub(crate) images_to_drop: Vec<Arc<RenderImage>>,
    /// 清空或删除后置为 true：等这些位图真正交给窗口释放完，再做一次
    /// malloc_trim。滚动/缩放淘汰图块不置位，避免每帧都 trim。
    pub(crate) trim_pending: bool,
    /// 什么时候允许做这一次 malloc_trim。延迟到旧帧被换掉之后再 trim，
    /// 否则上一帧元素树里还挂着这些位图，free 的只是这一帧的副本，回收不到。
    pub(crate) trim_due: Option<std::time::Instant>,
    /// 是否已经排了一个唤醒任务，避免 trim 期间重复起任务。
    trim_timer_armed: bool,
    /// 是否已经有一个导出/复制任务在后台跑。
    pub(crate) export_in_flight: bool,
    /// 是否在点击当下先声明剪贴板所有权、后台编码完成后只替换内容。
    ///
    /// Wayland 写剪贴板要求窗口有焦点和用户事件序列号，点击当下这两样都在，
    /// 但编码是后台异步的，等编码完再写剪贴板时焦点/序列号可能已经失效。
    /// 只有 Wayland 环境才走这条路径，其它平台保持原来的「编码完再写」。
    /// 测试通过 `set_clipboard_early_selection` 改写，不依赖运行机器的环境变量。
    pub(crate) clipboard_early_selection: bool,
    /// 导出任务代号：迟到的结果直接丢弃，不会覆盖新任务的状态。
    pub(crate) export_generation: u64,
    /// 清空作品后置为 true；下一次往空作品里导入第一张图时，把旧的撤销/重做
    /// 历史一起清掉，避免撤销又把上一个作品整个恢复回来。
    pub(crate) undo_history_stale: bool,
    /// 作品代次：清空作品时加一。后台解码/生成任务回来时代次对不上就丢弃，
    /// 迟到的结果不会给已经清掉的作品复活缓存或剪贴板。
    pub(crate) session_epoch: u64,
    pub(crate) status: Status,
    /// 吐司是否显示。默认不显示，避免常驻一行提示。
    pub(crate) toast_visible: bool,
    pub(crate) toast_token: usize,
    /// 提示浮层正在播出场淡出。出场期间浮层仍挂在树上，播完才卸载。
    pub(crate) toast_closing: bool,
    /// 胶囊上一次渲染时的形态；None 表示还没渲染过。
    pub(crate) capsule_target_prev: Option<CapsuleTarget>,
    /// 胶囊形态变化的次数，作为动画 id 的代号。
    pub(crate) capsule_morph_gen: u64,
    /// 最近一次操作成功后要点亮的按钮及其代号。
    pub(crate) success_flash: Option<(&'static str, u64)>,
    /// 成功闪烁的次数代号。
    pub(crate) flash_gen: u64,
    pub(crate) detecting: bool,
    pub(crate) decoding: usize,
    /// 待解码的导入请求，先进先出，保证按用户拖入顺序入列。
    /// 每项带入队时的作品代号：清空后迟到的结果按老代号丢弃。
    decode_queue: VecDeque<(u64, DecodeRequest)>,
    /// 是否已有常驻解码 worker 在跑，避免重复起 worker。
    decoding_worker: bool,
    /// 已入队但还没解码完的图片尺寸，顺序与 `decode_queue` 一致。
    ///
    /// 「适合画布」的基准要按整批图算。导入是一张一张解码的，只按已进项目的图
    /// 算基准的话，先解码完的那张会先按自己的尺寸定比例，后面的图一到，画面就
    /// 缩一下。入队时先把文件头里的宽高读出来记在这里，第一张画出来之前比例
    /// 就已经按整批图算好了。
    pending_sizes: VecDeque<(u32, u32)>,
    /// 字幕识别结果缓存（纯内存，不落盘）：像素指纹 -> 识别出的字幕块。
    detection_cache: HashMap<ImageFingerprint, Option<SubtitleBlock>>,
    pub(crate) clip_counter: usize,
    /// 点窗口关闭按钮时：true 隐藏窗口让程序继续在托盘运行，false 直接退出。
    pub(crate) close_to_tray: bool,
    /// 当前界面语言。默认中文，只在本次运行内切换。
    pub(crate) language: Language,
    /// 指针是否在窗口右上角的关闭按钮显示区里。
    pub(crate) close_near: bool,
    /// 关闭按钮淡入淡出的起点透明度。
    pub(crate) close_fade_from: f32,
    /// 关闭按钮淡入淡出的开始时间；None 表示没有正在播放的淡入淡出。
    pub(crate) close_fade_started: Option<std::time::Instant>,
    /// 关闭按钮显示/隐藏的次数，用来让旧的淡入任务退出。
    pub(crate) close_reveal_gen: u64,
    /// 上一次非最大化时的窗口大小和位置，退出时写进配置。
    pub(crate) windowed_bounds: Option<crate::config::WindowRect>,
    /// 当前打开的右键卡片。
    pub(crate) menu: Option<MenuState>,
    /// 复制或导出成功后是否倒计时清空图片。界面开关和存档共用这个值。
    pub(crate) clear_after_copy_export: bool,
    /// 复制按钮用的尺寸模式：小图或原图。右键在按钮上切换。
    pub(crate) copy_mode: CopyMode,
    /// 正在倒计时的清空图片状态。None 表示没有倒计时。
    pub(crate) clear_countdown: Option<ClearCountdown>,
    /// 倒计时代号计数，只增不减。取消后再启动也不会和旧计时器重号。
    pub(crate) clear_countdown_gen: u64,
    pub(crate) language_submenu_open: bool,
    /// 语言二级菜单已经挂了延迟收起的任务，避免重复挂。
    pub(crate) language_submenu_closing: bool,
    /// 画布的滚动位置。
    pub(crate) canvas_scroll: gpui::ScrollHandle,
    /// 裁剪边缘提示的显现进度，以及它当前正对着哪张图。
    ///
    /// 需求 10：指针靠近图块时约 120 毫秒内显现，拖动中保持满格，松手后
    /// 约 500 毫秒淡回默认的半透明状态。
    pub(crate) edge_hint: crate::ui::fade::Fade,
    /// 边缘提示当前服务的图块。换图块要先把进度归零，否则新图块会带着
    /// 上一张的进度直接跳出来。
    pub(crate) edge_hint_item: Option<ItemId>,
    /// 需求 12：间隔胶囊的透明度动画。默认半透明，指针靠近不透明，
    /// 离开后 500 毫秒淡回半透明。
    pub(crate) gap_control_fade: crate::ui::fade::Fade,
    /// 需求 13：间隔数值提示的透明度动画。只在拖动时显示，松手后淡出。
    pub(crate) gap_label_fade: crate::ui::fade::Fade,
    /// 需求 15：正在拖导航器视口框时的状态。
    pub(crate) navigator_drag: Option<NavigatorDragState>,
    /// 需求 15：导航器的透明度动画。默认半透明，悬停不透明，离开后淡出。
    pub(crate) navigator_fade: crate::ui::fade::Fade,
    /// 需求 6：按住鼠标滚轮平移画布时的起点；`None` 表示没在平移。
    pub(crate) middle_pan: Option<MiddlePan>,

    #[cfg(test)]
    pub(crate) open_requests: usize,
    /// 空白处左键按下时请求移动窗口的次数，仅测试读取。
    #[cfg(test)]
    pub(crate) window_move_requests: usize,
    focus_handle: FocusHandle,
}

impl AppRoot {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let saved = crate::config::load();
        let restored = saved.restore();
        let language = restored.language;

        let status = Status::info("");

        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle);
        // 快捷键在这里注册，和界面的创建绑在一起。
        //
        // 早先只有 `main.rs` 在启动时注册，无头测试直接建窗口就没有任何快捷键，
        // 「Ctrl+A 不起作用」这类问题在测试里根本量不出来。
        cx.bind_keys(crate::app::keymap::bindings());

        let this = cx.weak_entity();
        window.on_window_should_close(cx, move |window, cx| {
            let Some(root) = this.upgrade() else {
                return true;
            };
            root.update(cx, |root, cx| root.on_close_requested(window, cx))
        });

        window.set_window_title(language.app_name());
        crate::tray::set_language(language);

        let toast_visible = !status.text.is_empty();
        let root = AppRoot {
            project: Project {
                mode: restored.mode,
                settings: restored.settings.clone(),
                items: Vec::new(),
            },
            undo: undo::UndoStack::new(UNDO_LIMIT),
            detector: None,
            detector_loading: true,
            next_id: 1,
            selected: None,
            selected_ids: BTreeSet::new(),
            selection_anim: None,
            selection_anim_gen: 0,
            hover_gate: hover::HoverGate::new(),
            crop_drag: None,
            gap_drag: None,
            marquee: None,
            right_pending: None,
            dragging: None,
            drag_pointer: None,
            drag_over: None,
            auto_scroll_active: false,
            auto_scroll_gen: 0,
            settle: None,
            prev_gap_color: None,
            gap_color_started_at: None,
            gap_color_gen: 0,
            delete_hover: false,
            scale_mode: restored.scale_mode,
            viewport: window.viewport_size(),
            tile_cache: HashMap::new(),
            tile_jobs: HashMap::new(),
            tile_failed: HashMap::new(),
            thumb_cache: HashMap::new(),
            thumb_pending: HashSet::new(),
            thumb_failed: HashSet::new(),
            images_to_drop: Vec::new(),
            trim_pending: false,
            trim_due: None,
            trim_timer_armed: false,
            export_in_flight: false,
            export_generation: 0,
            clipboard_early_selection: std::env::var_os("WAYLAND_DISPLAY").is_some(),
            undo_history_stale: false,
            session_epoch: 0,
            status,
            toast_visible,
            toast_token: 0,
            toast_closing: false,
            capsule_target_prev: None,
            capsule_morph_gen: 0,
            success_flash: None,
            flash_gen: 0,
            detecting: false,
            decoding: 0,
            decode_queue: VecDeque::new(),
            decoding_worker: false,
            pending_sizes: VecDeque::new(),
            detection_cache: HashMap::new(),
            clip_counter: 0,
            close_to_tray: restored.close_to_tray,
            language,
            close_near: false,
            close_fade_from: tokens::CLOSE_IDLE_OPACITY,
            close_fade_started: None,
            close_reveal_gen: 0,
            windowed_bounds: restored.windowed,
            menu: None,
            clear_after_copy_export: restored.clear_after_copy_export,
            copy_mode: if restored.copy_small_mode {
                CopyMode::Small
            } else {
                CopyMode::Original
            },
            clear_countdown: None,
            clear_countdown_gen: 0,
            language_submenu_open: false,
            language_submenu_closing: false,
            canvas_scroll: gpui::ScrollHandle::new(),
            edge_hint: crate::ui::fade::Fade::new(tokens::CROP_HINT_IDLE),
            edge_hint_item: None,
            gap_control_fade: crate::ui::fade::Fade::new(tokens::GAP_CONTROL_IDLE_OPACITY),
            gap_label_fade: crate::ui::fade::Fade::new(0.0),
            navigator_drag: None,
            navigator_fade: crate::ui::fade::Fade::new(tokens::NAVIGATOR_IDLE_OPACITY),
            middle_pan: None,
            #[cfg(test)]
            open_requests: 0,
            #[cfg(test)]
            window_move_requests: 0,
            focus_handle,
        };

        cx.spawn(async move |this, cx| {
            #[cfg(any(debug_assertions, feature = "dev-tools"))]
            let started = std::time::Instant::now();
            let loaded = cx
                .background_executor()
                .spawn(async move { SubtitleDetector::new_embedded().map(Arc::new) })
                .await;
            #[cfg(any(debug_assertions, feature = "dev-tools"))]
            log::info!("字幕检测模型后台加载完成，用时 {:?}", started.elapsed());
            let _ = this.update(cx, |root, cx| root.finish_detector_load(loaded, cx));
        })
        .detach();

        root
    }

    fn finish_detector_load(
        &mut self,
        loaded: Result<Arc<SubtitleDetector>, DetectError>,
        cx: &mut Context<Self>,
    ) {
        self.detector_loading = false;
        match loaded {
            Ok(detector) => {
                self.detector = Some(detector);
                if self.needs_analysis() && self.pending_detection_count() > 0 {
                    self.set_status(
                        self.language.added_detecting(self.project.items.len()),
                        false,
                        cx,
                    );
                    self.start_detection(cx);
                } else {
                    cx.notify();
                }
            }
            Err(err) => {
                let detail = err.localized(self.language);
                for item in &mut self.project.items {
                    if item.analysis.is_pending() {
                        item.status = ItemStatus::Failed(detail.clone());
                        item.keep_whole = true;
                        item.analysis = AnalysisState::Analyzed;
                    }
                }
                self.set_status(self.language.detector_load_failed(&detail), true, cx);
            }
        }
    }

    // ------------------------------------------------------------------
    // 通用状态
    // ------------------------------------------------------------------

    pub(crate) fn set_status(
        &mut self,
        text: impl Into<SharedString>,
        error: bool,
        cx: &mut Context<Self>,
    ) {
        self.status = Status {
            text: text.into(),
            error,
        };
        self.toast_visible = true;
        self.toast_closing = false;
        self.toast_token = self.toast_token.wrapping_add(1);
        let token = self.toast_token;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(TOAST_MS))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.toast_token == token {
                    this.toast_closing = true;
                    cx.notify();
                    cx.spawn(async move |this, cx| {
                        cx.background_executor()
                            .timer(Duration::from_millis(TOAST_EXIT_MS))
                            .await;
                        let _ = this.update(cx, |this, cx| {
                            if this.toast_token == token {
                                this.toast_visible = false;
                                this.toast_closing = false;
                                cx.notify();
                            }
                        });
                    })
                    .detach();
                }
            });
        })
        .detach();
        cx.notify();
    }

    /// 记录一次成功操作，让对应胶囊按钮闪一下。
    pub(crate) fn mark_success_flash(&mut self, key: &'static str) {
        self.flash_gen = self.flash_gen.wrapping_add(1);
        self.success_flash = Some((key, self.flash_gen));
    }

    /// 退出前统一保存并清理：关浮层、结束拖动、清掉跟手状态，再交给 GPUI 退出。
    ///
    /// 所有退出入口都走这里，避免退出时还挂着菜单、拖动或重绘任务。
    pub(crate) fn request_quit(&mut self, window: &Window, cx: &mut Context<Self>) {
        self.save_state(window);
        self.close_menu(cx);
        self.crop_drag = None;
        self.gap_drag = None;
        self.marquee = None;
        self.right_pending = None;
        self.dragging = None;
        self.drag_over = None;
        self.drag_pointer = None;
        self.auto_scroll_active = false;
        cx.quit();
    }

    pub(crate) fn action_quit(&mut self, _: &Quit, window: &mut Window, cx: &mut Context<Self>) {
        self.request_quit(window, cx);
    }

    pub(crate) fn on_close_requested(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.close_to_tray {
            self.hide_to_tray(window, cx);
        } else {
            self.request_quit(window, cx);
        }
        false
    }

    /// 记住这次非最大化时的窗口大小和位置。
    pub(crate) fn remember_windowed_bounds(&mut self, window: &Window) {
        self.windowed_bounds = windowed_rect_for_save(
            &window.window_bounds(),
            window.is_maximized(),
            window.is_fullscreen(),
            self.windowed_bounds,
        );
    }

    /// 把当前全部界面选项组装成可保存的记录。
    pub(crate) fn saved_state(
        &self,
        windowed: Option<crate::config::WindowRect>,
    ) -> crate::config::SavedState {
        crate::config::SavedState::capture(
            self.language,
            self.project.mode,
            &self.project.settings,
            self.view_scale(),
            self.close_to_tray,
            self.clear_after_copy_export,
            self.copy_mode == CopyMode::Small,
            windowed,
        )
    }

    pub(crate) fn save_state(&mut self, window: &Window) {
        self.remember_windowed_bounds(window);
        let state = self.saved_state(self.windowed_bounds);
        crate::config::save(&state);
    }

    /// 右键点复制按钮时在「小图」和「原图」之间切换。
    ///
    /// 提示用吐司说明当前模式，选择写进配置，下次启动还是这个模式。
    pub(crate) fn toggle_copy_mode(&mut self, window: &Window, cx: &mut Context<Self>) {
        self.copy_mode = match self.copy_mode {
            CopyMode::Small => CopyMode::Original,
            CopyMode::Original => CopyMode::Small,
        };
        let message = match self.copy_mode {
            CopyMode::Small => self.language.copy_mode_small(),
            CopyMode::Original => self.language.copy_mode_original(),
        };
        self.set_status(message, false, cx);
        self.save_state(window);
        cx.notify();
    }

    /// 测试用：设置「点击当下先声明剪贴板所有权」这条路径是否生效。
    ///
    /// 正常运行由 `new` 按 WAYLAND_DISPLAY 决定；测试通过它改写，不依赖
    /// 运行机器上有没有 Wayland 环境变量。
    #[cfg(test)]
    pub(crate) fn set_clipboard_early_selection(&mut self, enabled: bool) {
        self.clipboard_early_selection = enabled;
    }

    pub(crate) fn set_close_to_tray(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.close_to_tray == enabled {
            return;
        }
        self.close_to_tray = enabled;
        cx.notify();
    }

    /// 右键菜单里切换「复制或导出后清空图片」。
    pub(crate) fn set_clear_after_copy_export(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.clear_after_copy_export == enabled {
            return;
        }
        self.clear_after_copy_export = enabled;
        cx.notify();
    }

    /// 复制或导出成功后调用：开关开着就启动一次 3 秒倒计时。
    ///
    /// 每次都换一个新代号。屏幕上同时只显示一个覆盖层，新的倒计时顶掉旧的；
    /// 旧计时器到点时因为代号已经对不上，不会误清空图片。
    pub(crate) fn maybe_start_clear_countdown(&mut self, cx: &mut Context<Self>) {
        if !self.clear_after_copy_export {
            return;
        }
        self.clear_countdown_gen = self.clear_countdown_gen.wrapping_add(1);
        let generation = self.clear_countdown_gen;
        self.clear_countdown = Some(ClearCountdown {
            generation,
            remaining_secs: ClearCountdown::SECONDS,
        });
        cx.notify();
        self.arm_clear_countdown_tick(generation, cx);
    }

    /// 每过一秒把数字减一；减到 0 时清空图片。
    ///
    /// 每一秒都重新核对代号：点掉覆盖或重新复制后，旧任务下一秒发现自己代号
    /// 过期就直接退出，不会误清空。
    fn arm_clear_countdown_tick(&self, generation: u64, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(1)).await;
            let _ = this.update(cx, |this, cx| {
                let Some(mut state) = this.clear_countdown else {
                    return;
                };
                if state.generation != generation {
                    return;
                }
                if state.remaining_secs <= 1 {
                    this.clear_countdown = None;
                    // 走普通清空流程：照常记撤销，清空后用户能像平时一样撤销回来。
                    this.clear_all(cx);
                } else {
                    state.remaining_secs -= 1;
                    this.clear_countdown = Some(state);
                    cx.notify();
                    this.arm_clear_countdown_tick(generation, cx);
                }
            });
        })
        .detach();
    }

    /// 点击覆盖层任意处取消倒计时。只清状态，不动已经复制/导出的图。
    pub(crate) fn cancel_clear_countdown(&mut self, cx: &mut Context<Self>) {
        if self.clear_countdown.take().is_some() {
            cx.notify();
        }
    }

    /// 覆盖层上的倒计时数字。
    pub(crate) fn clear_countdown_remaining(&self) -> u64 {
        self.clear_countdown
            .map(|state| state.remaining_secs)
            .unwrap_or(0)
    }

    /// 跟随指针，维护“指针是否靠近窗口右上角”。
    ///
    /// 只在进入/离开这个区域时改状态并重画，普通移动不做任何事，
    /// 避免每次鼠标移动都触发整窗重绘。
    pub(crate) fn track_close_region(
        &mut self,
        event: &gpui::MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let near = crate::ui::window_controls::close_region_revealed(
            event.position,
            window.viewport_size(),
        );
        if near == self.close_near {
            return;
        }
        self.close_fade_from = self.close_button_opacity();
        self.close_fade_started = Some(std::time::Instant::now());
        self.close_near = near;
        self.close_reveal_gen = self.close_reveal_gen.wrapping_add(1);
        let generation = self.close_reveal_gen;
        cx.notify();
        // 只在淡入淡出的 150 毫秒里按帧重画，播完自动退出。
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(tokens::CLOSE_FADE_TICK)
                    .await;
                let stop = this
                    .update(cx, |root, cx| {
                        if root.close_reveal_gen != generation {
                            return true;
                        }
                        if root
                            .close_fade_started
                            .is_some_and(|started| started.elapsed() >= tokens::CLOSE_FADE)
                        {
                            root.close_fade_started = None;
                        }
                        cx.notify();
                        root.close_fade_started.is_none()
                    })
                    .unwrap_or(true);
                if stop {
                    break;
                }
            }
        })
        .detach();
    }

    /// 需求 12：间隔胶囊此刻的不透明度。
    ///
    /// 指针停在胶囊上（或者正在拖间隔）时不透明，其余情况淡回半透明。
    pub(crate) fn gap_control_opacity_at(&self, now: std::time::Instant) -> f32 {
        self.gap_control_fade.value_at(now)
    }

    /// 需求 15：导航器此刻的不透明度。
    ///
    /// 指针停在导航器上（或者正在拖视口框）时不透明，其余情况淡回半透明。
    pub(crate) fn navigator_opacity_at(&self, now: std::time::Instant) -> f32 {
        self.navigator_fade.value_at(now)
    }

    /// 需求 15：指针进入/离开导航器、开始/结束拖框时，更新动画目标值。
    pub(crate) fn update_navigator_fade(&mut self, cx: &mut Context<Self>) {
        let now = cx.background_executor().now();
        let engaged = self.navigator_drag.is_some()
            || self.hover_gate.is_active(&HoverKey::widget("navigator"));
        let changed = self.navigator_fade.toward(
            if engaged {
                1.0
            } else {
                tokens::NAVIGATOR_IDLE_OPACITY
            },
            if engaged {
                tokens::CROP_HINT_FADE_IN
            } else {
                tokens::CROP_HINT_FADE_OUT
            },
            now,
        );
        if changed {
            cx.notify();
            self.drive_fade(cx, |root| &root.navigator_fade);
        }
    }

    /// 需求 12/13：指针进入/离开胶囊、开始/结束拖动间隔时，更新两处动画的目标值。
    ///
    /// 胶囊：悬停或拖动时朝不透明走（120 毫秒），离开后 500 毫秒淡回半透明。
    /// 数值提示：只在拖动间隔时出现，松手后淡出。
    pub(crate) fn update_gap_control_fade(&mut self, cx: &mut Context<Self>) {
        let now = cx.background_executor().now();
        let engaged =
            self.gap_drag.is_some() || self.hover_gate.is_active(&HoverKey::widget("gap-control"));
        let mut changed = self.gap_control_fade.toward(
            if engaged {
                1.0
            } else {
                tokens::GAP_CONTROL_IDLE_OPACITY
            },
            if engaged {
                tokens::CROP_HINT_FADE_IN
            } else {
                tokens::CROP_HINT_FADE_OUT
            },
            now,
        );
        // 需求 13：数值提示只在拖动时出现。
        let dragging = self.gap_drag.is_some();
        changed |= self.gap_label_fade.toward(
            if dragging { 1.0 } else { 0.0 },
            if dragging {
                tokens::CROP_HINT_FADE_IN
            } else {
                tokens::CROP_HINT_FADE_OUT
            },
            now,
        );
        if changed {
            cx.notify();
            self.drive_fade(cx, |root| &root.gap_control_fade);
            self.drive_fade(cx, |root| &root.gap_label_fade);
        }
    }

    /// 需求 10：裁剪边缘提示此刻的不透明度。
    ///
    /// 正在拖这张图、或者指针停在它上面时是满格；其余情况退回默认的半透明。
    /// 换图块的时候先把进度归零，再按时间涨上来，避免新图块一闪就满。
    pub(crate) fn edge_hint_opacity_at(&self, id: ItemId, now: std::time::Instant) -> f32 {
        if self.edge_hint_item != Some(id) {
            return tokens::CROP_HINT_IDLE;
        }
        self.edge_hint.value_at(now)
    }

    /// 指针进入/离开某个图块、或者开始/结束裁剪拖动时，更新边缘提示的目标值。
    ///
    /// 进入和开始拖动都朝满格走，用 120 毫秒；离开和松手朝半透明走，用 500 毫秒。
    pub(crate) fn update_edge_hint(&mut self, cx: &mut Context<Self>) {
        let now = cx.background_executor().now();
        // 拖动中的那张图按拖动算，否则按指针悬停算。
        let wanted = match self.crop_drag.as_ref() {
            Some(drag) => Some(drag.id),
            None => self.hover_gate.active_item(),
        };
        let changed = match wanted {
            None => {
                // 指针离开图块：朝完全透明淡回。
                //
                // 这里不能把 `edge_hint_item` 清掉：清了之后读取端会直接返回
                // 全透明，正在播的淡出就白播了，画面会一瞬间闪掉。留在原地
                // 让读取端按时间取值，淡完自然就是全透明。
                if self.edge_hint_item.is_none() {
                    return;
                }
                self.edge_hint
                    .toward(tokens::CROP_HINT_IDLE, tokens::CROP_HINT_FADE_OUT, now)
            }
            Some(id) => {
                if self.edge_hint_item != Some(id) {
                    // 换了一张图：先归到默认状态，再从那里涨上来。
                    self.edge_hint.settle(tokens::CROP_HINT_IDLE);
                    self.edge_hint_item = Some(id);
                }
                // 已经在淡回的路上、指针又回来了：从现在的位置往满格走。
                self.edge_hint.toward(1.0, tokens::CROP_HINT_FADE_IN, now)
            }
        };
        if changed {
            cx.notify();
            self.drive_fade(cx, |root| &root.edge_hint);
        }
    }

    /// 关闭按钮此刻的透明度：淡入淡出按时间插值，没有动画时就是目标值。
    pub(crate) fn close_button_opacity(&self) -> f32 {
        let target = if self.close_near {
            1.0
        } else {
            tokens::CLOSE_IDLE_OPACITY
        };
        let Some(started) = self.close_fade_started else {
            return target;
        };
        let total = tokens::CLOSE_FADE.as_secs_f32().max(f32::EPSILON);
        let ratio = (started.elapsed().as_secs_f32() / total).clamp(0.0, 1.0);
        let eased = 1.0 - (1.0 - ratio).powi(5);
        self.close_fade_from + (target - self.close_fade_from) * eased
    }

    pub(crate) fn set_language(
        &mut self,
        language: Language,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.language == language {
            return;
        }
        self.language = language;
        window.set_window_title(language.app_name());
        crate::tray::set_language(language);
        cx.notify();
    }

    pub(crate) fn record_undo(&mut self) {
        self.undo.record(&self.project);
    }

    // ------------------------------------------------------------------
    // 模式与设置
    // ------------------------------------------------------------------

    pub(crate) fn set_mode(&mut self, mode: Mode, cx: &mut Context<Self>) {
        if self.project.mode == mode {
            return;
        }
        self.record_undo();
        self.project.mode = mode;
        let from = self.selected_ids.clone();
        self.selected = None;
        self.selected_ids.clear();
        self.crop_drag = None;
        self.animate_selection_from(from, cx);
        self.gap_drag = None;

        if mode == Mode::Layout {
            // 排队/执行中的 OCR 不再需要：转成 NotNeeded，让图片立刻出现在画布上。
            for item in &mut self.project.items {
                if item.analysis.is_pending() {
                    item.analysis = AnalysisState::NotNeeded;
                    item.status = ItemStatus::Ready;
                    item.block = None;
                    item.keep_whole = true;
                }
            }
        } else {
            // 切回字幕：未分析过的图先查内存缓存，没命中再排队识别。
            let mut needs_detection = false;
            for index in 0..self.project.items.len() {
                if self.project.items[index].analysis != AnalysisState::NotNeeded {
                    continue;
                }
                let fingerprint = self.project.items[index].fingerprint;
                let cached = self.detection_cache.get(&fingerprint).cloned();
                let item = &mut self.project.items[index];
                match cached {
                    Some(block) => {
                        item.keep_whole = block.is_none();
                        item.status = if block.is_some() {
                            ItemStatus::Ready
                        } else {
                            ItemStatus::NoSubtitle
                        };
                        item.block = block;
                        item.analysis = AnalysisState::Analyzed;
                    }
                    None => {
                        item.status = ItemStatus::Pending;
                        item.keep_whole = false;
                        item.analysis = AnalysisState::PendingAnalysis;
                        needs_detection = true;
                    }
                }
            }
            if needs_detection {
                self.start_detection(cx);
            }
        }
        self.set_status(self.language.switched_mode(mode), false, cx);
    }

    pub(crate) fn set_keep_second(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.project.settings.keep_second == enabled {
            return;
        }
        self.record_undo();
        self.project.settings.keep_second = enabled;
        self.set_status(
            if enabled {
                self.language.keep_second_enabled()
            } else {
                self.language.keep_second_disabled()
            },
            false,
            cx,
        );
    }

    pub(crate) fn set_bleed_index(&mut self, index: usize, cx: &mut Context<Self>) {
        let bleed = (index.min(tokens::BLEED_STEPS - 1) as f32) * 0.05;
        if (self.project.settings.bleed - bleed).abs() < 1e-4 {
            return;
        }
        self.record_undo();
        self.project.settings.bleed = bleed;
        cx.notify();
    }

    /// 滑条上的加减一步。滚轮和键盘都能用。
    pub(crate) fn step_bleed(&mut self, delta: isize, cx: &mut Context<Self>) {
        let current = tokens::bleed_to_index(self.project.settings.bleed) as isize;
        let next = (current + delta).clamp(0, tokens::BLEED_STEPS as isize - 1);
        if next == current {
            return;
        }
        self.set_bleed_index(next as usize, cx);
    }

    pub(crate) fn set_layout_target(&mut self, target: LayoutTarget, cx: &mut Context<Self>) {
        if self.project.settings.layout == target {
            return;
        }
        self.record_undo();
        self.project.settings.layout = target;
        self.set_status(self.language.layout_changed(target), false, cx);
    }

    // ------------------------------------------------------------------
    // 选择、删除、排序
    // ------------------------------------------------------------------

    pub(crate) fn is_selected(&self, id: ItemId) -> bool {
        self.selected_ids.contains(&id)
    }

    pub(crate) fn selected_count(&self) -> usize {
        self.selected_ids.len()
    }

    /// 一张图当前的选中动画进度：选中、变淡、方勾，都是 0 到 1。
    ///
    /// 第一个值现在只驱动圆角，不再移动卡片位置。
    pub(crate) fn selection_progress(
        &self,
        id: ItemId,
        now: std::time::Instant,
    ) -> (f32, f32, f32) {
        let selected = self.is_selected(id);
        let any_selected = !self.selected_ids.is_empty();
        // 动画目标已经过期（拉框多选、导入自动选中、撤销/删除改过选择）就直接
        // 按当前状态画终值，不能让旧动画继续顶着。
        let Some(anim) = self
            .selection_anim
            .as_ref()
            .filter(|anim| anim.to == self.selected_ids)
        else {
            let lift = if selected { 1. } else { 0. };
            let dim = if any_selected && !selected { 1. } else { 0. };
            return (lift, dim, lift);
        };
        let Some(elapsed_ms) = now
            .checked_duration_since(anim.started)
            .map(|duration| duration.as_secs_f32() * 1000.)
        else {
            let lift = if selected { 1. } else { 0. };
            let dim = if any_selected && !selected { 1. } else { 0. };
            return (lift, dim, lift);
        };
        let from_selected = anim.from.contains(&id);
        let to_selected = anim.to.contains(&id);
        let lift = animate_selection_flag(
            from_selected,
            to_selected,
            elapsed_ms,
            tokens::selection::CARD_IN_MS,
            tokens::selection::CARD_OUT_MS,
        );
        let from_dim = !anim.from.is_empty() && !from_selected;
        let to_dim = !anim.to.is_empty() && !to_selected;
        let dim = animate_selection_flag(
            from_dim,
            to_dim,
            elapsed_ms,
            tokens::selection::DIM_IN_MS,
            tokens::selection::DIM_OUT_MS,
        );
        let check = if to_selected {
            let delayed = (elapsed_ms - tokens::selection::CHECK_DELAY_MS as f32).max(0.);
            animate_selection_flag(
                from_selected,
                true,
                delayed,
                tokens::selection::CHECK_IN_MS,
                tokens::selection::CHECK_IN_MS,
            )
        } else {
            animate_selection_flag(
                from_selected,
                false,
                elapsed_ms,
                tokens::selection::CHECK_OUT_MS,
                tokens::selection::CHECK_OUT_MS,
            )
        };
        (lift, dim, check)
    }

    /// 选中状态已经改好之后调用，用改动前的集合补一段过渡动画。
    pub(crate) fn animate_selection_from(
        &mut self,
        from: BTreeSet<ItemId>,
        cx: &mut Context<Self>,
    ) {
        if from == self.selected_ids {
            return;
        }
        self.selection_anim_gen = self.selection_anim_gen.wrapping_add(1);
        let generation = self.selection_anim_gen;
        self.selection_anim = Some(SelectionAnim {
            from,
            to: self.selected_ids.clone(),
            started: cx.background_executor().now(),
            generation,
        });
        cx.spawn(async move |this, cx| {
            let tick = Duration::from_millis(tokens::selection::SELECTION_TICK_MS);
            loop {
                cx.background_executor().timer(tick).await;
                let done = this
                    .update(cx, |root, cx| {
                        let Some(anim) = root.selection_anim.as_ref() else {
                            return true;
                        };
                        if anim.generation != generation {
                            return true;
                        }
                        let elapsed = cx
                            .background_executor()
                            .now()
                            .saturating_duration_since(anim.started);
                        if elapsed >= Duration::from_millis(tokens::selection::TOTAL_MS) {
                            root.selection_anim = None;
                        }
                        cx.notify();
                        root.selection_anim.is_none()
                    })
                    .unwrap_or(true);
                if done {
                    break;
                }
            }
        })
        .detach();
    }

    pub(crate) fn select(&mut self, id: ItemId, cx: &mut Context<Self>) {
        if !self.project.items.iter().any(|item| item.id == id) {
            return;
        }
        let from = self.selected_ids.clone();
        self.selected = Some(id);
        self.selected_ids.clear();
        self.selected_ids.insert(id);
        self.crop_drag = None;
        self.animate_selection_from(from, cx);
        cx.notify();
    }

    pub(crate) fn select_with_modifiers(
        &mut self,
        id: ItemId,
        modifiers: Modifiers,
        cx: &mut Context<Self>,
    ) {
        if !self.project.items.iter().any(|item| item.id == id) {
            return;
        }
        let previous_selected = self.selected_ids.clone();
        if modifiers.secondary() {
            if !self.selected_ids.remove(&id) {
                self.selected_ids.insert(id);
                if self.selected.is_none() {
                    self.selected = Some(id);
                }
            } else if self.selected == Some(id) {
                self.selected = self.selected_ids.iter().next().copied();
            }
            self.crop_drag = None;
            self.animate_selection_from(previous_selected, cx);
            cx.notify();
            return;
        }
        if modifiers.shift
            && let Some(anchor) = self.selected
            && let Some(from) = self.project.items.iter().position(|item| item.id == anchor)
            && let Some(to) = self.project.items.iter().position(|item| item.id == id)
        {
            let (start, end) = if from <= to { (from, to) } else { (to, from) };
            self.selected_ids = self.project.items[start..=end]
                .iter()
                .map(|item| item.id)
                .collect();
            if self.selected.is_none() {
                self.selected = Some(id);
            }
            self.crop_drag = None;
            self.animate_selection_from(previous_selected, cx);
            cx.notify();
            return;
        }
        self.select(id, cx);
    }

    pub(crate) fn delete_selected_images(&mut self, cx: &mut Context<Self>) {
        if self.selected_ids.is_empty() {
            return;
        }
        let removed_ids: Vec<ItemId> = self.selected_ids.iter().copied().collect();
        let removed = removed_ids.len();
        self.record_undo();
        self.project
            .items
            .retain(|item| !removed_ids.contains(&item.id));
        self.selected_ids.clear();
        self.selected = None;
        self.crop_drag = None;
        self.release_previews_for(&removed_ids);
        let message = if removed > 1 {
            self.language.deleted_selected_count(removed)
        } else {
            self.language.deleted_selected().to_string()
        };
        self.set_status(message, false, cx);
    }

    /// 把一张图往上或往下挪一格。
    pub(crate) fn move_item(
        &mut self,
        id: ItemId,
        direction: MoveDirection,
        cx: &mut Context<Self>,
    ) {
        let Some(from) = self.project.items.iter().position(|item| item.id == id) else {
            return;
        };
        let to = match direction {
            MoveDirection::Up => from.saturating_sub(1),
            MoveDirection::Down => (from + 1).min(self.project.items.len().saturating_sub(1)),
        };
        if from == to {
            return;
        }
        self.record_undo();
        let item = self.project.items.remove(from);
        self.project.items.insert(to, item);
        self.close_menu(cx);
        cx.notify();
    }

    // ------------------------------------------------------------------
    // 裁剪带与手动调节
    // ------------------------------------------------------------------

    pub(crate) fn current_band_for(&self, id: ItemId) -> Option<CropBand> {
        let index = self.project.items.iter().position(|item| item.id == id)?;
        let item = &self.project.items[index];
        if let Some(manual) = item.manual {
            return crop::manual_band_for_item(item, manual);
        }
        match crop::resolve(&self.project).get(index).copied() {
            Some(Inclusion::Crop(band)) => Some(band),
            Some(Inclusion::FullImage) => Some(CropBand::full(item)),
            _ => None,
        }
    }

    pub(crate) fn current_manual_for(&self, id: ItemId) -> Option<ManualBand> {
        let band = self.current_band_for(id)?;
        let item = self.project.items.iter().find(|item| item.id == id)?;
        if item.width == 0 || item.height == 0 {
            return None;
        }
        let width = item.width as f32;
        let height = item.height as f32;
        ManualBand::clamped(
            band.x0 as f32 / width,
            band.x1 as f32 / width,
            band.y0 as f32 / height,
            band.y1 as f32 / height,
        )
    }

    /// 把同一段归一化裁剪应用到一批图片；返回实际改动的数量。
    pub(crate) fn apply_band_to_items(&mut self, ids: &[ItemId], band: ManualBand) {
        for item in self.project.items.iter_mut() {
            if ids.contains(&item.id) && item.width > 0 && item.height > 0 {
                item.manual = Some(band);
                item.keep_whole = false;
            }
        }
    }

    /// 把这张图设成保留整张（或恢复自动）。
    pub(crate) fn set_keep_whole(&mut self, id: ItemId, keep: bool, cx: &mut Context<Self>) {
        if !self.selected_ids.contains(&id) {
            self.select(id, cx);
        }
        let targets: Vec<ItemId> = if self.selected_ids.len() > 1 {
            self.selected_ids.iter().copied().collect()
        } else {
            vec![id]
        };
        self.record_undo();
        for item in self
            .project
            .items
            .iter_mut()
            .filter(|item| targets.contains(&item.id))
        {
            item.manual = None;
            item.keep_whole = keep;
        }
        self.close_menu(cx);
        let status = if keep {
            self.language.manual_whole_kept()
        } else {
            self.language.manual_cleared()
        };
        self.set_status(status, false, cx);
    }

    /// 右键卡片里的「保留单行 / 双行字幕」：按识别结果重新算一段裁剪。
    pub(crate) fn set_keep_lines(&mut self, id: ItemId, keep_second: bool, cx: &mut Context<Self>) {
        let Some(item) = self.project.items.iter().find(|item| item.id == id) else {
            return;
        };
        let Some(block) = item.block.as_ref() else {
            return;
        };
        let bleed = self.project.settings.bleed;
        let Some(band) = crop::band_for_block(block, keep_second, bleed, item.size()) else {
            return;
        };
        let width = item.width as f32;
        let height = item.height as f32;
        let Some(manual) = ManualBand::clamped(
            band.x0 as f32 / width,
            band.x1 as f32 / width,
            band.y0 as f32 / height,
            band.y1 as f32 / height,
        ) else {
            return;
        };
        if !self.selected_ids.contains(&id) {
            self.select(id, cx);
        }
        let targets: Vec<ItemId> = if self.selected_ids.len() > 1 {
            self.selected_ids.iter().copied().collect()
        } else {
            vec![id]
        };
        self.record_undo();
        self.apply_band_to_items(&targets, manual);
        self.close_menu(cx);
        self.set_status(self.language.manual_applied(targets.len()), false, cx);
    }

    /// 右键卡片里的「恢复自动识别结果」。
    pub(crate) fn reset_item_auto(&mut self, id: ItemId, cx: &mut Context<Self>) {
        let targets: Vec<ItemId> = if self.selected_ids.contains(&id) && self.selected_ids.len() > 1
        {
            self.selected_ids.iter().copied().collect()
        } else {
            vec![id]
        };
        self.record_undo();
        for item in self
            .project
            .items
            .iter_mut()
            .filter(|item| targets.contains(&item.id))
        {
            item.manual = None;
            item.keep_whole = item.block.is_none();
        }
        self.close_menu(cx);
        self.set_status(self.language.manual_cleared(), false, cx);
    }

    /// 右键卡片里的「把当前裁剪同步到全部图片」。
    pub(crate) fn sync_crop_to_all(&mut self, id: ItemId, cx: &mut Context<Self>) {
        let Some(manual) = self.current_manual_for(id) else {
            return;
        };
        let ids: Vec<ItemId> = self.project.items.iter().map(|item| item.id).collect();
        self.record_undo();
        self.apply_band_to_items(&ids, manual);
        self.close_menu(cx);
        self.set_status(self.language.manual_applied(ids.len()), false, cx);
    }

    // ------------------------------------------------------------------
    // 撤销 / 重做 / 清空
    // ------------------------------------------------------------------

    pub(crate) fn undo_edit(&mut self, cx: &mut Context<Self>) {
        if self.undo.undo(&mut self.project) {
            self.after_undo_redo(cx);
            self.set_status(self.language.undone(), false, cx);
        } else {
            self.set_status(self.language.nothing_to_undo(), true, cx);
        }
    }

    pub(crate) fn action_undo(
        &mut self,
        _: &UndoEdit,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.undo_edit(cx);
    }

    pub(crate) fn action_redo(
        &mut self,
        _: &RedoEdit,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.undo.redo(&mut self.project) {
            self.after_undo_redo(cx);
            self.set_status(self.language.redone(), false, cx);
        } else {
            self.set_status(self.language.nothing_to_redo(), true, cx);
        }
    }

    pub(crate) fn action_dismiss(
        &mut self,
        _: &Dismiss,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.dismiss_overlays(cx) {
            return;
        }
        // 没有浮层可关时，Esc 只在“关闭窗口缩到托盘”开启时隐藏窗口；
        // 没开托盘时 Esc 不做任何关闭动作，避免误退出。
        match escape_outcome(self.close_to_tray) {
            EscapeOutcome::HideToTray => {
                self.hide_to_tray(window, cx);
            }
            EscapeOutcome::DoNothing => {}
        }
    }

    /// 把窗口隐藏到托盘。Esc 走这里，不写配置文件。
    pub(crate) fn hide_to_tray(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.save_state(window);
        window.set_visible(false);
        cx.notify();
    }

    /// 关掉最上面一层浮层或结束进行中的拖动。关掉了返回 true。
    pub(crate) fn dismiss_overlays(&mut self, cx: &mut Context<Self>) -> bool {
        // 倒计时覆盖层不在这里处理：Esc 是拿来关窗口的，不该取消倒计时；
        // 倒计时只认鼠标点覆盖层（见 render_clear_countdown 的 on_click）。
        if self.menu.is_some() {
            self.close_menu(cx);
        } else if self.crop_drag.is_some() {
            self.end_crop_drag(cx);
        } else if self.gap_drag.is_some() {
            self.end_gap_drag(cx);
        } else if self.marquee.is_some() || self.right_pending.is_some() {
            self.marquee = None;
            self.right_pending = None;
            cx.notify();
        } else {
            return false;
        }
        true
    }

    fn after_undo_redo(&mut self, cx: &mut Context<Self>) {
        let valid_ids: BTreeSet<ItemId> = self.project.items.iter().map(|item| item.id).collect();
        // 撤销/重做之后项目里已经没有的图，预览缓存全部释放，别让位图留在内存里。
        let stale_ids: Vec<ItemId> = self
            .tile_cache
            .keys()
            .chain(self.thumb_cache.keys())
            .copied()
            .filter(|id| !valid_ids.contains(id))
            .collect();
        self.release_previews_for(&stale_ids);
        if valid_ids.is_empty() && self.project.items.is_empty() {
            self.release_all_previews();
        }
        // 撤销/重做之后历史还在用，不能当“清空后的旧历史”处理。
        self.undo_history_stale = false;
        self.selected_ids.retain(|id| valid_ids.contains(id));
        if !self
            .project
            .items
            .iter()
            .any(|item| self.selected == Some(item.id))
        {
            self.selected = self.selected_ids.iter().next().copied();
        }
        if let Some(selected) = self.selected
            && !self.selected_ids.contains(&selected)
        {
            self.selected_ids.insert(selected);
        }
        self.crop_drag = None;
        self.gap_drag = None;
        self.marquee = None;
        self.drag_over = None;
        self.schedule_trim(cx);
        cx.notify();
        self.start_detection(cx);
    }

    pub(crate) fn action_clear(
        &mut self,
        _: &ClearAll,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.clear_all(cx);
    }

    pub(crate) fn action_select_all(
        &mut self,
        _: &SelectAll,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_all(cx);
    }

    /// 选中当前作品里的每一张图。作品为空时什么都不做：不清选择、不记撤销、
    /// 不弹提示，也不会因为空集合而让后续手势拿到一个「选了但没图」的状态。
    pub(crate) fn select_all(&mut self, cx: &mut Context<Self>) {
        if self.project.items.is_empty() {
            return;
        }
        let from = self.selected_ids.clone();
        self.selected_ids = self.project.items.iter().map(|item| item.id).collect();
        // 主选中项保持原来的那张（它决定右键卡片和单张操作的落点）；
        // 原来没选任何图时，从当前顺序里取第一张当主选中项。
        if !self
            .selected
            .is_some_and(|id| self.selected_ids.contains(&id))
        {
            self.selected = self.project.items.first().map(|item| item.id);
        }
        self.crop_drag = None;
        self.animate_selection_from(from, cx);
        cx.notify();
    }

    /// 清空当前作品。托盘和右键卡片都走这里。
    ///
    /// 撤销快照照常记（还能撤销回来），但所有预览位图当场释放：图块、缩略图
    /// 全部交出来，下一帧统一 `drop_image`，再调一次 `malloc_trim` 把空页
    /// 还给系统。清空之后再导入新图时，旧的撤销/重做历史会被清掉。
    pub(crate) fn clear_all(&mut self, cx: &mut Context<Self>) {
        if self.project.items.is_empty() {
            // 画面已经空了，但清空前在途的导入仍然要作废，不能让图稍后自己
            // 冒出来。没有在途导入时直接返回，不产生无意义的撤销记录。
            if self.decode_queue.is_empty() && self.decoding == 0 {
                return;
            }
            self.session_epoch = self.session_epoch.wrapping_add(1);
            // 队列里没开始的、已经 pop 出去正在解码的，都在这里一次归零；
            // 旧任务回传时按作品代号判定，不会再扣新作品的那一份。
            self.decode_queue.clear();
            self.pending_sizes.clear();
            self.decoding = 0;
            self.release_all_previews();
            self.schedule_trim(cx);
            self.set_status(self.language.cleared_all(), false, cx);
            return;
        }
        self.record_undo();
        self.project.items.clear();
        self.selected = None;
        self.selected_ids.clear();
        self.crop_drag = None;
        self.gap_drag = None;
        self.drag_over = None;
        self.dragging = None;
        self.drag_pointer = None;
        self.delete_hover = false;
        self.close_menu(cx);
        self.release_all_previews();
        self.schedule_trim(cx);
        self.undo_history_stale = true;
        self.session_epoch = self.session_epoch.wrapping_add(1);
        // 清空前进来的导入请求也一起作废，别在清空之后又把图加回来。
        // 队列和在途的一次归零；旧任务回传按作品代号判定，不再扣新导入。
        self.decode_queue.clear();
        self.pending_sizes.clear();
        self.decoding = 0;
        self.set_status(self.language.cleared_all(), false, cx);
    }

    // ------------------------------------------------------------------
    // 缩放
    // ------------------------------------------------------------------

    /// 当前显示比例的完整描述：自动适应还是手动比例。
    ///
    /// 自动适应没有任何用户可调的倍数，比例只由窗口和作品尺寸决定；放大缩小一律
    /// 走手动比例。这样点「适合画布」一定回到纯拟合值，不会残留上一次的倍数。
    pub(crate) fn view_scale(&self) -> tokens::ViewScale {
        match self.scale_mode {
            tokens::ScaleMode::Fit => tokens::ViewScale::fit(),
            tokens::ScaleMode::Manual(scale) => tokens::ViewScale::manual(scale),
        }
    }

    /// 设定手动比例。切进手动模式时不再重新拟合，画面比例保持不变。
    pub(crate) fn set_manual_scale(&mut self, scale: f32, cx: &mut Context<Self>) {
        let scale = tokens::clamp_manual_scale(scale);
        let changed = self.scale_mode.manual() != Some(scale);
        self.scale_mode = tokens::ScaleMode::Manual(scale);
        if changed {
            cx.notify();
        }
    }

    /// 滚轮和加减键共用：按当前真正生效的显示比例乘一个步进。
    ///
    /// 自动适应下先取当前实际显示比例（`canvas_layout().scale`），乘完切进手动比例；
    /// 手动比例直接乘自己的值。起点用实际比例，画面不会跳；只有一套范围
    /// （`clamp_manual_scale` 的 5%~400%），键盘和滚轮能到的最大最小倍数一样。
    pub(crate) fn zoom_by(&mut self, delta: f32, cx: &mut Context<Self>) {
        let scale = match self.scale_mode {
            tokens::ScaleMode::Fit => self.canvas_layout().scale,
            tokens::ScaleMode::Manual(scale) => scale,
        };
        self.set_manual_scale(scale * (1.0 + delta), cx);
    }

    /// 适合画布：回到自动适应，作品重新按窗口算比例，滚动位置归零居中。
    ///
    /// 重绘要无条件发：`ScrollHandle::set_offset` 只改偏移量、不通知，单靠它
    /// 挪回原位不会触发下一帧，界面上就停在旧位置。
    pub(crate) fn action_zoom_fit(&mut self, cx: &mut Context<Self>) {
        self.scale_mode = tokens::ScaleMode::Fit;
        self.canvas_scroll
            .set_offset(gpui::point(gpui::px(0.), gpui::px(0.)));
        cx.notify();
    }

    /// 1:1：每个输出像素对应一个画布逻辑像素，并且留住当前视口中心看到的位置。
    pub(crate) fn action_zoom_one_to_one(&mut self, cx: &mut Context<Self>) {
        self.set_manual_scale_keeping_centre(1.0, cx);
    }

    /// 换到新的手动比例，先把当前视口中心对应的作品坐标记下来，再换算成新比例下
    /// 需要的滚动偏移。
    ///
    /// 不这么做的话，1:1 之后作品的显示尺寸变化极大，滚动偏移还停在老位置，用户
    /// 眼前会跳到作品的另一个角落，得自己重新找回来。
    fn set_manual_scale_keeping_centre(&mut self, scale: f32, cx: &mut Context<Self>) {
        let scale = tokens::clamp_manual_scale(scale);
        let before = self.canvas_layout();
        let viewport = self.viewport;
        let origin = before.origin(viewport, self.canvas_scroll.offset());
        // 视口中心对应的作品内容坐标（相对于作品左上角，单位是显示像素）。
        let centre = gpui::point(
            (f32::from(viewport.width) / 2.0 - origin.x) / before.scale.max(1e-6),
            (f32::from(viewport.height) / 2.0 - origin.y) / before.scale.max(1e-6),
        );
        let changed = self.scale_mode.manual() != Some(scale);
        self.scale_mode = tokens::ScaleMode::Manual(scale);
        // 按新比例重算排版，求出让同一个作品点回到视口中心需要的滚动偏移。
        // 作品位置和滚动范围都算自同一份排版，不用上一帧量出来的最大滚动量。
        let after = self.canvas_layout();
        let centred_x = ((after.width + 2.0 * f32::from(tokens::CANVAS_SIDE_PAD))
            .max(f32::from(viewport.width))
            - after.width)
            / 2.0;
        let desired_x = f32::from(viewport.width) / 2.0 - (centred_x + centre.x * after.scale);
        let desired_y = f32::from(viewport.height) / 2.0
            - (after.top_padding(viewport) + centre.y * after.scale);
        let (max_x, max_y) = after.max_scroll(viewport);
        self.canvas_scroll.set_offset(gpui::point(
            gpui::px(desired_x.clamp(-max_x, 0.0)),
            gpui::px(desired_y.clamp(-max_y, 0.0)),
        ));
        if changed {
            cx.notify();
        }
    }

    pub(crate) fn action_zoom_in(
        &mut self,
        _: &ZoomIn,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.zoom_by(tokens::ZOOM_STEP, cx);
    }

    pub(crate) fn action_zoom_out(
        &mut self,
        _: &ZoomOut,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.zoom_by(-tokens::ZOOM_STEP, cx);
    }

    /// 键盘上的「重置缩放」：回到适合画布，和点击胶囊里的这一项同一个行为。
    pub(crate) fn action_zoom_reset(&mut self, cx: &mut Context<Self>) {
        self.action_zoom_fit(cx);
    }

    fn on_zoom_reset(&mut self, _: &ZoomReset, _window: &mut Window, cx: &mut Context<Self>) {
        self.action_zoom_reset(cx);
    }
}

// ----------------------------------------------------------------------
// 导入、字幕检测、预览缓存
// ----------------------------------------------------------------------

impl AppRoot {
    pub(crate) fn action_paste(
        &mut self,
        _: &PasteImage,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.paste(cx);
    }

    pub(crate) fn paste(&mut self, cx: &mut Context<Self>) {
        let Some(item) = cx.read_from_clipboard() else {
            self.set_status(self.language.clipboard_empty(), true, cx);
            return;
        };
        let mut payload = None;
        for entry in item.entries() {
            if let ClipboardEntry::Image(image) = entry {
                payload = Some(image.bytes.clone());
                break;
            }
        }
        match payload {
            Some(bytes) => {
                self.clip_counter += 1;
                let name = self.language.clipboard_image_name(self.clip_counter);
                self.import_bytes(name, bytes, cx);
            }
            None => self.set_status(self.language.clipboard_no_image(), true, cx),
        }
    }

    pub(crate) fn action_open(
        &mut self,
        _: &OpenImages,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_images(cx);
    }

    /// 打开文件选择框。测试环境里平台没有实现这个对话框，只记一次请求。
    pub(crate) fn open_images(&mut self, cx: &mut Context<Self>) {
        #[cfg(test)]
        {
            self.open_requests += 1;
            cx.notify();
        }
        #[cfg(not(test))]
        {
            use gpui::PathPromptOptions;
            let receiver = cx.prompt_for_paths(PathPromptOptions {
                files: true,
                directories: false,
                multiple: true,
                prompt: Some(self.language.choose_images().into()),
            });
            cx.spawn(async move |this, cx| match receiver.await {
                Ok(Ok(Some(paths))) => {
                    let _ = this.update(cx, |this, cx| this.open_paths(paths, cx));
                }
                Ok(Ok(None)) => {}
                Ok(Err(err)) => {
                    let _ = this.update(cx, |this, cx| {
                        this.set_status(this.language.open_failed(&err.to_string()), true, cx)
                    });
                }
                Err(_) => {}
            })
            .detach();
        }
    }

    pub(crate) fn on_file_drop(
        &mut self,
        paths: &ExternalPaths,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_paths(paths.paths().to_vec(), cx);
    }

    pub(crate) fn open_paths(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        let images: Vec<PathBuf> = paths
            .into_iter()
            .filter(|path| is_supported_path(path))
            .collect();
        if images.is_empty() {
            self.set_status(self.language.unsupported_images(), true, cx);
            return;
        }
        for path in images {
            // 先读文件头拿宽高，让「适合画布」的基准在第一张画出来之前就是整批值。
            // 读文件头只读几十字节；读失败就不记尺寸，等解码那一步去报错。
            let size = crate::imaging::decode::read_dimensions_path(&path).ok();
            self.enqueue_decode(DecodeRequest::Path(path), size, cx);
        }
    }

    pub(crate) fn import_bytes(&mut self, name: String, bytes: Vec<u8>, cx: &mut Context<Self>) {
        let size = crate::imaging::decode::read_dimensions_bytes(&bytes).ok();
        self.enqueue_decode(
            DecodeRequest::Bytes {
                name,
                bytes: Arc::new(bytes),
            },
            size,
            cx,
        );
    }

    /// 入队一条解码请求。只有一个常驻 worker 串行处理，因此
    /// `push_image_with` 的调用顺序恒等于用户拖入/选择的顺序，与单张图的
    /// 解码耗时、后台线程池的并行度都无关。
    fn enqueue_decode(
        &mut self,
        request: DecodeRequest,
        size: Option<(u32, u32)>,
        cx: &mut Context<Self>,
    ) {
        self.decode_queue.push_back((self.session_epoch, request));
        self.pending_sizes.push_back(size.unwrap_or((0, 0)));
        self.decoding += 1;
        self.start_decode(cx);
    }

    /// 单个常驻解码 worker：先进先出，解码与指纹计算在后台，结果回主线程入项目。
    fn start_decode(&mut self, cx: &mut Context<Self>) {
        if self.decoding_worker {
            return;
        }
        self.decoding_worker = true;
        cx.spawn(async move |this, cx| {
            loop {
                let next = match this.update(cx, |this, _cx| this.decode_queue.pop_front()) {
                    Ok(next) => next,
                    Err(_) => break,
                };
                let Some((epoch, request)) = next else { break };
                let name = request.name();
                let source = request.source();
                let decoded = cx
                    .background_executor()
                    .spawn(async move { request.decode() })
                    .await;
                let _ = this.update(cx, |this, cx| {
                    // 只减当前这一次作品的计数。清空时已经把计数归零，旧任务的
                    // 回传不能再扣掉清空后新导入的那一份。
                    this.decoding = settle_decoding(this.decoding, epoch, this.session_epoch);
                    // 解码期间作品被清空：这张图不再属于当前作品，结果直接丢掉，
                    // 不能让它在清空之后又冒出来。在途尺寸表也已经被清空，这里
                    // 不能再从里面摘一项，否则会错删清空后新导入的那张的尺寸。
                    if this.session_epoch != epoch {
                        cx.notify();
                        return;
                    }
                    // 这张图的尺寸到这里才从在途表里摘掉：解码期间它必须继续计入
                    // 「适合画布」的基准，否则解码那几百毫秒里比例会跳一下。
                    // 成功时它的尺寸已经写进项目，失败时这张图不会出现，两种都该摘。
                    this.pending_sizes.pop_front();
                    match decoded {
                        Ok(image) => this.push_source_with(name, source, image, cx),
                        Err(err) => this.set_status(
                            this.language
                                .read_image_failed(&err.localized(this.language)),
                            true,
                            cx,
                        ),
                    }
                    cx.notify();
                });
            }
            let _ = this.update(cx, |this, cx| {
                this.decoding_worker = false;
                // 收尾瞬间又入了新请求：重新启动 worker，避免卡住队列。
                if !this.decode_queue.is_empty() {
                    this.start_decode(cx);
                }
            });
        })
        .detach();
    }

    /// 把一张解码好的图片加入项目。
    ///
    /// 识别结果先看内存缓存（按像素指纹），命中就不进 OCR 队列；排版模式直接
    /// 标记 `NotNeeded`，既不识别，也不会被 `crop::resolve` 判成 Skipped 而消失。
    fn push_source_with(
        &mut self,
        name: String,
        source: crate::model::SourceImage,
        decoded: DecodedImage,
        cx: &mut Context<Self>,
    ) {
        let DecodedImage { rgb, fingerprint } = decoded;
        let (width, height) = rgb.dimensions();
        // 来源已经拿到；解码像素只用来读宽高和指纹，读完立刻丢掉。
        drop(rgb);
        if width == 0 || height == 0 {
            self.set_status(self.language.empty_image(&name), true, cx);
            return;
        }
        let id = self.next_id;
        self.next_id += 1;

        let mut item = Item {
            id,
            name,
            width,
            height,
            source,
            block: None,
            status: ItemStatus::Ready,
            keep_whole: true,
            manual: None,
            analysis: AnalysisState::Analyzed,
            fingerprint,
        };

        // 记忆本次加入的提示类型，push 之后再统一发，避免在持有 item 时借 self。
        enum Added {
            Plain,
            Loading,
            Unavailable,
            Detecting,
        }
        let added = if !self.needs_analysis() {
            item.analysis = AnalysisState::NotNeeded;
            Added::Plain
        } else if let Some(cached) = self.detection_cache.get(&fingerprint) {
            item.block = cached.clone();
            item.status = if item.block.is_some() {
                ItemStatus::Ready
            } else {
                ItemStatus::NoSubtitle
            };
            item.keep_whole = item.block.is_none();
            Added::Plain
        } else if self.detector.is_some() {
            item.status = ItemStatus::Pending;
            item.keep_whole = false;
            item.analysis = AnalysisState::PendingAnalysis;
            Added::Detecting
        } else if self.detector_loading {
            item.status = ItemStatus::Pending;
            item.keep_whole = false;
            item.analysis = AnalysisState::PendingAnalysis;
            Added::Loading
        } else {
            Added::Unavailable
        };

        // 清空作品之后第一次导入：上一个作品的撤销/重做历史到此为止，
        // 免得 Ctrl+Z 又把它整个恢复回来。
        if self.undo_history_stale && self.project.items.is_empty() {
            self.undo.clear();
            self.undo_history_stale = false;
            // 清空那一帧交出来的位图可能还排在释放队列里：这时候直接 trim 收不到
            // 它们，而且 pending 一清就再也没人回收。队列非空时保留 pending，
            // 交给正常的延迟回收路径。
            if self.images_to_drop.is_empty() {
                self.trim_pending = false;
                self.trim_due = None;
                trim_allocator();
            } else {
                self.schedule_trim(cx);
            }
        }
        self.record_undo();
        self.project.items.push(item);
        // 导入只加图，不碰当前选择：用户选中别的图时导入新图，选择应该留在原图。
        self.crop_drag = None;
        let count = self.project.items.len();
        match added {
            Added::Plain => self.set_status(self.language.added_image(count), false, cx),
            Added::Loading => {
                self.set_status(self.language.added_detector_loading(count), false, cx)
            }
            Added::Unavailable => {
                self.set_status(self.language.added_detector_unavailable(count), false, cx)
            }
            Added::Detecting => {
                self.set_status(self.language.added_detecting(count), false, cx);
                self.start_detection(cx);
            }
        }
    }

    /// 无头测试专用：直接把像素包成测试来源，避免每个测试写临时 PNG。
    #[cfg(test)]
    fn push_image_with(&mut self, name: String, decoded: DecodedImage, cx: &mut Context<Self>) {
        let source = crate::model::SourceImage::Pixels(Arc::new(decoded.rgb.clone()));
        self.push_source_with(name, source, decoded, cx);
    }

    /// 只有字幕模式才需要 OCR。门控收敛在这里，别处只管调 `start_detection`。
    fn needs_analysis(&self) -> bool {
        self.project.mode == Mode::Subtitle
    }

    pub(crate) fn is_busy(&self) -> bool {
        self.decoding > 0
            || (self.needs_analysis()
                && (self.detector_loading || self.detecting || self.pending_detection_count() > 0))
    }

    pub(crate) fn pending_detection_count(&self) -> usize {
        self.project
            .items
            .iter()
            .filter(|item| item.analysis.is_pending())
            .count()
    }

    /// 取下一张待识别的图，并立刻标记为 Analyzing（认领），避免两个 worker 取到同一张。
    fn take_pending_item(
        &mut self,
    ) -> Option<(
        ItemId,
        crate::model::SourceImage,
        ImageFingerprint,
        u32,
        u32,
    )> {
        let item = self
            .project
            .items
            .iter_mut()
            .find(|item| item.analysis == AnalysisState::PendingAnalysis)?;
        item.analysis = AnalysisState::Analyzing;
        Some((
            item.id,
            item.source.clone(),
            item.fingerprint,
            item.width,
            item.height,
        ))
    }

    pub(crate) fn start_detection(&mut self, cx: &mut Context<Self>) {
        if !self.needs_analysis() || self.detecting || self.pending_detection_count() == 0 {
            return;
        }
        let Some(detector) = self.detector.clone() else {
            return;
        };
        self.detecting = true;
        cx.spawn(async move |this, cx| {
            loop {
                let next = match this.update(cx, |this, _cx| this.take_pending_item()) {
                    Ok(next) => next,
                    Err(_) => break,
                };
                let Some((id, source, fingerprint, width, height)) = next else {
                    break;
                };
                let detector = detector.clone();
                let detected = cx
                    .background_executor()
                    .spawn(async move {
                        let rgb = match source.load_rgb() {
                            Ok(rgb) => rgb,
                            Err(err) => return Err(DetectError::Source(err.to_string())),
                        };
                        if rgb.dimensions() != (width, height) {
                            return Err(DetectError::Source(format!(
                                "图片尺寸已变化：原来是 {width}×{height}，现在是 {}×{}",
                                rgb.width(),
                                rgb.height()
                            )));
                        }
                        detector.detect(&rgb)
                    })
                    .await;
                let _ = this.update(cx, |this, cx| {
                    this.apply_detection(id, fingerprint, detected);
                    cx.notify();
                });
            }
            let _ = this.update(cx, |this, cx| {
                this.detecting = false;
                this.on_detection_finished(cx);
                if this.pending_detection_count() > 0 {
                    this.start_detection(cx);
                }
            });
        })
        .detach();
    }

    fn apply_detection(
        &mut self,
        id: ItemId,
        fingerprint: ImageFingerprint,
        result: Result<Option<SubtitleBlock>, DetectError>,
    ) {
        // 成功的两种结果都值得缓存；失败不缓存，保留重试的机会。
        let cacheable = match &result {
            Ok(block) => Some(block.clone()),
            Err(_) => None,
        };
        let Some(item) = self.project.items.iter_mut().find(|item| item.id == id) else {
            return;
        };
        item.analysis = AnalysisState::Analyzed;
        match result {
            Ok(Some(block)) => {
                item.block = Some(block);
                item.status = ItemStatus::Ready;
                item.keep_whole = false;
            }
            Ok(None) => {
                item.block = None;
                item.status = ItemStatus::NoSubtitle;
                item.keep_whole = true;
            }
            Err(err) => {
                item.block = None;
                item.status = ItemStatus::Failed(err.localized(self.language));
                item.keep_whole = true;
            }
        }
        if let Some(cached) = cacheable {
            self.detection_cache.insert(fingerprint, cached);
        }
    }

    fn on_detection_finished(&mut self, cx: &mut Context<Self>) {
        let failed = self
            .project
            .items
            .iter()
            .filter(|item| matches!(item.status, ItemStatus::Failed(_)))
            .count();
        let missing = self
            .project
            .items
            .iter()
            .filter(|item| item.status == ItemStatus::NoSubtitle)
            .count();
        let total = self.project.items.len();
        if failed > 0 {
            self.set_status(
                self.language.detection_done_with_failures(total, failed),
                true,
                cx,
            );
        } else if missing > 0 {
            self.set_status(self.language.no_subtitle_kept_whole(missing), false, cx);
        } else if total > 0 {
            self.set_status(self.language.detection_done(total), false, cx);
        }
        cx.notify();
    }

    // ------------------------------------------------------------------
    // 预览图块 / 缩略图 / GPU 位图释放
    // ------------------------------------------------------------------

    /// 删掉这些图片时，把它们名下的图块、缩略图和任务登记一起撤掉，并把位图
    /// 排进待释放队列（下一帧 Render 里统一交给窗口的 `drop_image`）。
    /// `detection_cache` 一律保留 —— 删除再添加要命中识别结果。
    pub(crate) fn release_previews_for(&mut self, removed: &[ItemId]) {
        if !removed.is_empty() {
            self.trim_pending = true;
        }
        for id in removed {
            if let Some(mut grid) = self.tile_cache.remove(id) {
                self.images_to_drop.extend(grid.take_images());
            }
            self.tile_jobs.remove(id);
            self.tile_failed.remove(id);
            if let Some(image) = self.thumb_cache.remove(id) {
                self.images_to_drop.push(image);
            }
            self.thumb_pending.remove(id);
            self.thumb_failed.remove(id);
        }
    }

    /// 画面里已经看不见的图不再保留图块缓存；缩略图留着（导航器要画全部图）。
    pub(crate) fn release_offscreen_tiles(&mut self, visible: &[ItemId]) {
        let stale: Vec<ItemId> = self
            .tile_cache
            .keys()
            .copied()
            .filter(|id| !visible.contains(id))
            .collect();
        for id in stale {
            if let Some(mut grid) = self.tile_cache.remove(&id) {
                self.images_to_drop.extend(grid.take_images());
            }
            self.tile_jobs.remove(&id);
            self.tile_failed.remove(&id);
        }
    }

    /// 把全部预览缓存交出来。清空作品、撤销回空作品时用。
    pub(crate) fn release_all_previews(&mut self) {
        let ids: Vec<ItemId> = self
            .tile_cache
            .keys()
            .chain(self.thumb_cache.keys())
            .copied()
            .collect();
        self.release_previews_for(&ids);
        self.tile_jobs.clear();
        self.tile_failed.clear();
        self.thumb_pending.clear();
        self.thumb_failed.clear();
    }

    /// 安排一次延迟回收：等旧帧 swap 掉、释放队列排空后再 malloc_trim。
    fn schedule_trim(&mut self, cx: &mut Context<Self>) {
        self.trim_pending = true;
        self.trim_due = Some(cx.background_executor().now() + Duration::from_millis(TRIM_DELAY_MS));
        self.arm_trim_timer(cx);
    }

    /// 到点用一次定时器唤醒下一帧，避免应用空闲时再也等不到下一次 Render，
    /// trim 永远不执行。只排一个任务，不在 Render 里反复轮询，否则测试的
    /// 假时钟不前进时会一直自我唤醒。
    fn arm_trim_timer(&mut self, cx: &mut Context<Self>) {
        if self.trim_timer_armed {
            return;
        }
        self.trim_timer_armed = true;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(TRIM_DELAY_MS))
                .await;
            let _ = this.update(cx, |this, cx| {
                this.trim_timer_armed = false;
                if this.trim_pending {
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// 把这一轮不再需要的位图交给窗口释放 GPU 纹理。每帧 Render 里做一次。
    ///
    /// 返回真正释放掉了几张：调用方据此决定要不要顺手 `malloc_trim`，位图
    /// 还挂在队列里、CPU 像素没 free 时调它是回收不到的。
    pub(crate) fn drop_released_images(&mut self, window: &mut Window) -> usize {
        let mut dropped = 0;
        for image in self.images_to_drop.drain(..) {
            let _ = window.drop_image(image);
            dropped += 1;
        }
        dropped
    }

    /// 取这张图这一帧要贴的图块，并按需排后台生成任务。
    ///
    /// `spec` 是这一帧的显示规格（只按当前裁剪带切），`visible` 是屏幕可见范围
    /// 相对图块左上角的矩形。屏幕外的块由 `TileGrid::sync_with` 当场交出来，这里统一
    /// 排进待释放队列；不冻结时缺的块派一个后台任务补齐。
    /// `keep_tiles` 为真时暂时滚出屏幕的块继续留在缓存里，不进待释放队列，供
    /// 排序拖动期间图片来回平移时立刻贴回；其它调用方传 false。
    pub(crate) fn tile_views_for(
        &mut self,
        item: &crate::model::Item,
        spec: tiles::TileSpec,
        visible: Rect,
        freeze: bool,
        keep_tiles: bool,
        cx: &mut Context<Self>,
    ) -> Vec<tiles::TileView> {
        let id = item.id;
        let source = item.source.clone();
        let name = item.name.clone();
        let item_size = (item.width, item.height);
        let (views, missing, generation, released) = {
            let grid = self
                .tile_cache
                .entry(id)
                .or_insert_with(|| tiles::TileGrid::new(spec));
            let plan = grid.sync_with(spec, visible, keep_tiles);
            let views = grid.render_tiles();
            let released = grid.take_released();
            (views, plan.missing, plan.generation, released)
        };
        if !released.is_empty() {
            self.images_to_drop.extend(released);
        }
        let should_build = !freeze
            && !missing.is_empty()
            && self.tile_jobs.get(&id) != Some(&generation)
            && self.tile_failed.get(&id) != Some(&generation);
        if should_build {
            self.spawn_tile_build(id, name, source, item_size, spec, generation, missing, cx);
        }
        views
    }

    /// 给一张图派后台生成任务：解一次源图，把缺的图块全部生成好。
    ///
    /// 每张图同一时刻只允许一个任务在跑；代次在任务期间变了，回来的块由
    /// `TileGrid::insert` 拒绝，调用方直接 drop_image，不会复活旧代次。
    fn spawn_tile_build(
        &mut self,
        id: ItemId,
        name: String,
        source: crate::model::SourceImage,
        item_size: (u32, u32),
        spec: tiles::TileSpec,
        generation: u64,
        missing: Vec<tiles::TileKey>,
        cx: &mut Context<Self>,
    ) {
        self.tile_jobs.insert(id, generation);
        let epoch = self.session_epoch;
        cx.spawn(async move |this, cx| {
            let built = cx
                .background_executor()
                .spawn(async move { build_tile_images(source, &name, item_size, spec, &missing) })
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.session_epoch != epoch {
                    // 作品已经清空：迟到的图块直接丢掉，不写回任何缓存。
                    if let Ok(images) = built {
                        let lost: Vec<_> = images.into_iter().map(|(_, image)| image).collect();
                        if !lost.is_empty() {
                            this.images_to_drop.extend(lost);
                            // 迟到的位图也要等下一帧释放后回收一次。
                            this.schedule_trim(cx);
                        }
                    }
                    cx.notify();
                    return;
                }
                if this.tile_jobs.get(&id) == Some(&generation) {
                    this.tile_jobs.remove(&id);
                }
                match built {
                    Ok(images) => {
                        let mut released = Vec::new();
                        match this.tile_cache.get_mut(&id) {
                            Some(grid) => {
                                for (key, image) in images {
                                    if !grid.insert(key, image.clone()) {
                                        released.push(image);
                                    }
                                }
                                released.extend(grid.take_released());
                            }
                            None => {
                                // 图已经被删了（epoch 不一定变，比如只删这一张）：生成
                                // 好的位图全部排进待释放队列，不能直接 drop 掉留 GPU 纹理。
                                released.extend(images.into_iter().map(|(_, image)| image));
                            }
                        }
                        if !released.is_empty() {
                            this.images_to_drop.extend(released);
                            // 删图/换代之后迟到的图块：等交给窗口释放后再 trim。
                            this.schedule_trim(cx);
                        }
                    }
                    Err(err) => {
                        this.tile_failed.insert(id, generation);
                        this.set_status(err, true, cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// 取这张图的缩略图；没有就在后台生成一次。
    ///
    /// 缩略图是整张图按宽 256 缩小的结果，导航器、图块没生成时的占位和拖动
    /// 跟手小图都用它，显示时按裁剪带裁切，不再为不同用途重复解码。
    pub(crate) fn thumbnail_for(
        &mut self,
        id: ItemId,
        source: &crate::model::SourceImage,
        name: &str,
        cx: &mut Context<Self>,
    ) -> Option<Arc<RenderImage>> {
        if let Some(image) = self.thumb_cache.get(&id) {
            return Some(image.clone());
        }
        if self.thumb_failed.contains(&id) {
            return None;
        }
        if self.thumb_pending.insert(id) {
            let source = source.clone();
            let name = name.to_string();
            let epoch = self.session_epoch;
            cx.spawn(async move |this, cx| {
                let generated = cx
                    .background_executor()
                    .spawn(async move {
                        source
                            .load_rgb()
                            .map(|rgb| render::preview_image(rgb, THUMB_WIDTH))
                            .map_err(|err| format!("读取图片 {name} 失败：{err}"))
                    })
                    .await;
                let _ = this.update(cx, |this, cx| {
                    this.thumb_pending.remove(&id);
                    // 等待期间这张图可能被删了、作品可能被清空了：迟到的缩略图
                    // 排进待释放队列，不写回缓存，也不留 GPU 纹理。
                    let still_wanted = this.session_epoch == epoch
                        && this.project.items.iter().any(|item| item.id == id);
                    match generated {
                        Ok(image) => {
                            if !still_wanted {
                                this.images_to_drop.push(image);
                                // 迟到缩略图也要等下一帧释放后回收一次。
                                this.schedule_trim(cx);
                                cx.notify();
                                return;
                            }
                            this.thumb_cache.insert(id, image);
                            cx.notify();
                        }
                        Err(err) => {
                            if !still_wanted {
                                return;
                            }
                            this.thumb_failed.insert(id);
                            this.set_status(err, true, cx);
                        }
                    }
                });
            })
            .detach();
        }
        None
    }

    /// 复制/导出后倒计时清空图片的覆盖层。
    ///
    /// 这只是临时遮蔽：铺满整个窗口、压暗画面，中间一个大号秒数呼吸明暗。
    /// 没有面板、边框、提示文字这些多余元素。点任意处就取消倒计时。
    /// Esc 不会取消倒计时——Esc 留给关闭窗口，这里只认鼠标点击。
    pub(crate) fn render_clear_countdown(
        &self,
        theme: Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let seconds = self.clear_countdown_remaining();

        // 数字一秒一个来回地呼吸明暗，尺寸不变，秒数变化时数字跟着换。
        let digits = div()
            .font_family(tokens::FONT_FAMILY)
            .text_size(gpui::px(120.))
            .font_weight(tokens::weight::STRONG)
            .text_color(theme.text)
            .child(seconds.to_string())
            .with_animation(
                "clear-countdown-digits",
                Animation::new(Duration::from_secs(1)).repeat(),
                move |el, delta| {
                    let alpha = 0.35 + 0.65 * pulsating_between(0.0, 1.0)(delta);
                    el.opacity(alpha)
                },
            );

        div()
            .id("clear-countdown-overlay")
            .debug_selector(|| "clear-countdown-overlay".to_string())
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .bg(theme.ghost_mask.opacity(0.92))
            // 用左键按下而不是 click：Wayland 下贴着窗口右上角按下会丢松开事件，
            // click 凑不齐按下/松开一对就不会触发。这里按下即处理。
            .on_mouse_down(
                gpui::MouseButton::Left,
                cx.listener(
                    |this: &mut AppRoot, event: &gpui::MouseDownEvent, window, cx| {
                        if crate::ui::window_controls::close_button_hit(
                            event.position,
                            window.viewport_size(),
                        ) {
                            this.on_close_requested(window, cx);
                            return;
                        }
                        this.cancel_clear_countdown(cx);
                    },
                ),
            )
            .child(digits)
    }

    /// 这张图在当前输出范围里是不是整张画面。
    ///
    /// 用输出范围和原图面积直接比，手动框选、自动裁剪、识别失败都走同一条判断，
    /// 不会出现界面显示和实际输出不一致的情况。
    pub(crate) fn item_keeps_whole_image(&self, id: ItemId) -> bool {
        let index = self
            .project
            .items
            .iter()
            .position(|item| item.id == id)
            .unwrap_or(usize::MAX);
        let Some(item) = self.project.items.get(index) else {
            return false;
        };
        let full = item.width as u64 * item.height as u64;
        match crop::resolve(&self.project).get(index).copied() {
            Some(Inclusion::Crop(band)) => band.width() as u64 * band.height() as u64 == full,
            Some(Inclusion::FullImage) => true,
            _ => false,
        }
    }

    /// 当前作品的输出尺寸。测试用它核对成品和吐司里报的尺寸。
    #[cfg(test)]
    pub(crate) fn output_size(&self) -> Option<(u32, u32)> {
        let gap = self.project.settings.gap.effective_px();
        let inclusions = crop::resolve(&self.project);
        match self.project.mode {
            Mode::Subtitle => {
                let mut widths: Vec<u32> = Vec::new();
                let mut heights: Vec<u32> = Vec::new();
                for (index, item) in self.project.items.iter().enumerate() {
                    let Some((width, height)) = inclusions
                        .get(index)
                        .copied()
                        .and_then(|i| i.output_size(item))
                    else {
                        continue;
                    };
                    widths.push(width.max(1));
                    heights.push(height);
                }
                if widths.is_empty() {
                    return None;
                }
                // 每张按宽度拉齐到最宽那张，高度同比换算，与 compose_subtitle 一致。
                let output_width = widths.iter().copied().max().unwrap_or(1);
                let mut height = 0u64;
                for (width, cropped_height) in widths.iter().zip(heights.iter()) {
                    height += (u64::from(*cropped_height) * u64::from(output_width)
                        / u64::from((*width).max(1)))
                    .max(1);
                }
                let count = widths.len() as u64;
                height += gap as u64 * count.saturating_sub(1);
                Some((output_width, height.min(u32::MAX as u64) as u32))
            }
            Mode::Layout => {
                let sizes: Vec<(u32, u32)> = self
                    .project
                    .items
                    .iter()
                    .enumerate()
                    .filter_map(|(index, item)| {
                        inclusions
                            .get(index)
                            .copied()
                            .and_then(|inclusion| inclusion.output_size(item))
                    })
                    .collect();
                if sizes.is_empty() {
                    return None;
                }
                let layout =
                    crate::imaging::layout::arrange(&sizes, self.project.settings.layout, gap);
                Some((layout.canvas_w, layout.canvas_h))
            }
        }
    }

    fn action_remove(&mut self, _: &RemoveSelected, _window: &mut Window, cx: &mut Context<Self>) {
        self.delete_selected_images(cx);
    }
}

/// 解码任务回传时更新忙碌计数。
///
/// 只有和当前作品同一段的任务才减一；清空前已经在跑的任务回传时作品代号已经
/// 变了，不能再扣清空后新导入的那一份计数（清空时已经统一归零）。
fn settle_decoding(decoding: usize, task_epoch: u64, current_epoch: u64) -> usize {
    if task_epoch == current_epoch {
        decoding.saturating_sub(1)
    } else {
        decoding
    }
}

/// 后台：解一次源图，把一批图块全部生成好，解码图只活在这一趟里。
///
/// 源图读不到或实际尺寸和项目里记录的不一致时返回带文件名的明确错误，
/// 由调用方弹提示，不静默跳过。
fn build_tile_images(
    source: crate::model::SourceImage,
    name: &str,
    item_size: (u32, u32),
    spec: tiles::TileSpec,
    missing: &[tiles::TileKey],
) -> Result<Vec<(tiles::TileKey, Arc<RenderImage>)>, String> {
    // 包成 DynamicImage 再做裁剪缩放：泛型的 imageops 函数会在本 crate 里按开发
    // 构建（不优化）编译，一张 4K 图切两块实测要一秒四，DynamicImage 只要两百毫秒。
    let rgb = render::as_dynamic(
        source
            .load_rgb()
            .map_err(|err| format!("读取图片 {name} 失败：{err}"))?,
    );
    let actual = (rgb.width(), rgb.height());
    if actual != item_size {
        return Err(format!(
            "图片 {name} 的尺寸变了：记录 {}×{}，实际 {}×{}；请重新导入这张图",
            item_size.0, item_size.1, actual.0, actual.1
        ));
    }
    let mut built = Vec::with_capacity(missing.len());
    for key in missing {
        let rect = spec.tile_rect(key.col, key.row);
        built.push((
            *key,
            render::tile_image(&rgb, spec.physical_width, spec.physical_height, rect),
        ));
    }
    drop(rgb);
    Ok(built)
}

/// 清空/删除后等多久再 malloc_trim。要等上一帧的元素树被换掉，之前 free
/// 的只是本帧副本，回收不到那些位图。
const TRIM_DELAY_MS: u64 = 50;
/// 释放大块位图之后把空闲内存还给系统。
///
/// Linux 的 glibc 在 free 之后不一定会把空页还给内核，连续导入/清空几张 4K
/// 图时进程常驻内存会一直停在高位；这里显式调一次 malloc_trim。
/// 把 glibc 的分配区（arena）数量限制成 1，让清空作品后的 `malloc_trim` 真能
/// 把空闲页还给系统。
///
/// 为什么是 1：解码 4K 大图跑在 `background_executor().spawn` 的后台线程上，
/// glibc 默认会按线程给它们各开一个 arena。`malloc_trim(0)` 只整理主 arena，
/// 次级 arena 里的空闲页收不回来，所以清空后进程常驻内存仍停在高位（实测 102MB，
/// 限成 1 之后掉到 35MB）。限成 1 后所有线程共用主 arena，一次 trim 就能整块归还。
///
/// 为什么必须在启动早期调用：mallopt 只影响之后新建的 arena 和分配策略，等后台
/// 线程已经拿到独立 arena 再调用就晚了，所以放在 `run()` 的第一行、任何大块分配
/// 之前执行。
///
/// 代价是多线程分配会串行一点；本程序的大内存分配集中在解码、识别这类少数后台
/// 任务上，实测没有可感知的变慢，可以接受。
///
/// 返回是否设置成功。失败就算了：内存优化不该让程序启动不了。
#[cfg(all(target_os = "linux", target_env = "gnu"))]
pub fn limit_malloc_arenas() -> bool {
    // SAFETY: mallopt 只改 glibc 分配器的参数；M_ARENA_MAX 是 glibc 公开的
    // 调参项，传 1 表示只保留一个分配区，不涉及内存访问。
    unsafe { libc::mallopt(libc::M_ARENA_MAX, 1) == 1 }
}

/// 非 glibc 平台没有分配区这个概念，什么都不用做。
#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
pub fn limit_malloc_arenas() -> bool {
    false
}

fn trim_allocator() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    unsafe {
        libc::malloc_trim(0);
    }
}

impl Render for AppRoot {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::new();
        let viewport = window.viewport_size();
        self.viewport = viewport;
        // 窗口每帧都可能被移动或调整大小，这里记住最近一次普通尺寸；
        // 之后就算用户最大化再退出，保存的仍是最大化之前的尺寸。
        self.remember_windowed_bounds(window);
        // 上一帧释放掉的预览位图在这里统一交给窗口回收 GPU 纹理；真释放过
        // 位图才顺手把空页还给系统，不然调 malloc_trim 也回收不到刚释放的像素。
        let _dropped = self.drop_released_images(window);
        if self.trim_pending {
            let now = cx.background_executor().now();
            // 只交出了 pending、还没定时间的释放路径（删图等）：从这一帧起算延迟，
            // 并排一个到点唤醒的定时器；之后不再重复排。
            if self.trim_due.is_none() {
                self.trim_due = Some(now + Duration::from_millis(TRIM_DELAY_MS));
                self.arm_trim_timer(cx);
            }
            let due = now >= self.trim_due.expect("上面刚补上");
            if due && self.images_to_drop.is_empty() {
                self.trim_pending = false;
                self.trim_due = None;
                trim_allocator();
            } else if !self.trim_timer_armed {
                // 定时器已经到点醒过一次，但这一帧发现还没到期（两次释放间隔很短，
                // 后一次把 trim_due 推后了），或者释放队列还没排空。此时没人会再来
                // 唤醒，必须再排一次，否则 trim_pending 永远挂着。
                self.arm_trim_timer(cx);
            }
        }
        // 拖动在画布空白处松手时没有落点元素接手，GPUI 会直接结束拖动。
        // 这里补一次清理，避免图还停在“被摘掉”的状态、跟手小图也留在屏幕上。
        if self.dragging.is_some() && !cx.has_active_drag() {
            self.dragging = None;
            self.drag_over = None;
            self.drag_pointer = None;
            self.delete_hover = false;
            self.auto_scroll_active = false;
        }
        // 跟手小图、间隔胶囊与间隔数值提示都是窗口级浮层，最后画，保证盖在画布、
        // 胶囊、菜单之上。
        //
        // 需求 12：间隔胶囊必须画在滚动区外面。画在滚动区里会跟着作品一起滚走，
        // 作品一长就够不着了。
        let drag_thumb = self.render_drag_thumb(theme, window, cx);
        let gap_control = self.render_gap_control(theme, window, cx);
        let gap_bubble = self.render_gap_bubble(theme, window, cx);
        // 需求 15：画布左下角的全局预览导航器。
        let navigator = self.render_navigator(theme, window, cx);
        // 需求 1：右下角缩放胶囊。同样必须在滚动区外面：挂在滚动区里会跟着
        // 作品一起滚走，作品一长、「1:1 溢出后想看回适合画布」时就点不到了。
        let zoom_chip = self.render_zoom_chip(theme, cx);

        div()
            .size_full()
            .relative()
            .flex()
            .flex_col()
            .bg(theme.bg)
            .text_color(theme.text)
            .track_focus(&self.focus_handle)
            .on_action(cx.listener(Self::action_paste))
            .on_action(cx.listener(Self::action_open))
            .on_action(cx.listener(Self::action_export))
            .on_action(cx.listener(Self::action_copy))
            .on_action(cx.listener(Self::action_undo))
            .on_action(cx.listener(Self::action_redo))
            .on_action(cx.listener(Self::action_remove))
            .on_action(cx.listener(Self::action_clear))
            .on_action(cx.listener(Self::action_select_all))
            .on_action(cx.listener(Self::action_zoom_in))
            .on_action(cx.listener(Self::action_zoom_out))
            .on_action(cx.listener(Self::on_zoom_reset))
            .on_action(cx.listener(Self::action_dismiss))
            .on_action(cx.listener(Self::action_quit))
            .on_mouse_move(cx.listener(Self::track_close_region))
            .on_drop(cx.listener(Self::on_file_drop))
            // 图片拖到作品块之外松手时，作品块自己的 on_drop 不会被触发（GPUI 按
            // 位置命中的元素派发），这次拖动就会被静默丢掉。这里在整窗兜一次：
            // 作品块接手过就会把 drag_over 清空；删除区接手过会连 dragging 一起清，
            // 所以走到这里还留着 drag_over，就是谁都没接手的排序拖动。
            .on_drop(
                cx.listener(|this: &mut AppRoot, dragged: &DragItem, _window, cx| {
                    if let Some(before) = this.drag_over {
                        this.apply_drop(dragged.id, before, cx);
                    }
                    this.clear_drag_state(cx);
                }),
            )
            .child(self.render_canvas(window, theme, cx))
            .child(self.render_capsule(theme, cx))
            .child(self.render_window_controls(theme, cx))
            .when_some(drag_thumb, |this, thumb| this.child(thumb))
            .when_some(gap_control, |this, control| this.child(control))
            .when_some(gap_bubble, |this, bubble| this.child(bubble))
            .when_some(navigator, |this, nav| this.child(nav))
            .when_some(zoom_chip, |this, chip| this.child(chip))
            // 菜单画在主窗口内，紧贴鼠标弹出；放不下时翻到鼠标另一侧。
            .when(self.menu.is_some(), |this| {
                this.child(self.render_context_menu(theme, viewport, cx))
            })
            // 倒计时覆盖层最后画，盖在菜单、胶囊、画布之上；点哪里都能取消。
            .when(self.clear_countdown.is_some(), |this| {
                this.child(self.render_clear_countdown(theme, cx))
            })
    }
}

#[cfg(test)]
mod escape_tests {
    use super::*;
    use gpui::Bounds;

    #[test]
    fn escape_only_hides_when_close_to_tray_is_on() {
        assert_eq!(escape_outcome(true), EscapeOutcome::HideToTray);
        assert_eq!(escape_outcome(false), EscapeOutcome::DoNothing);
    }

    #[test]
    fn saving_ignores_maximized_and_fullscreen_bounds() {
        let current = Bounds {
            origin: gpui::point(gpui::px(100.0), gpui::px(50.0)),
            size: gpui::size(gpui::px(1500.0), gpui::px(900.0)),
        };
        let previous = Some(crate::config::WindowRect {
            x: 120.0,
            y: 60.0,
            width: 1400.0,
            height: 850.0,
        });
        let next = crate::config::WindowRect::from_bounds(current);

        assert_eq!(
            windowed_rect_for_save(&WindowBounds::Windowed(current), false, false, previous),
            Some(next),
            "普通窗口应更新保存的尺寸"
        );
        assert_eq!(
            windowed_rect_for_save(&WindowBounds::Maximized(current), true, false, previous),
            previous,
            "最大化应保留上一次普通尺寸"
        );
        assert_eq!(
            windowed_rect_for_save(&WindowBounds::Fullscreen(current), false, false, previous),
            previous,
            "全屏应保留上一次普通尺寸"
        );
        assert_eq!(
            windowed_rect_for_save(&WindowBounds::Windowed(current), true, false, previous),
            previous,
            "macOS 的 zoom 也算最大化，不能覆盖普通尺寸"
        );
        assert_eq!(
            windowed_rect_for_save(&WindowBounds::Windowed(current), false, true, previous),
            previous,
            "X11 全屏仍报 Windowed，要靠全屏标记拦住"
        );
        assert_eq!(
            windowed_rect_for_save(&WindowBounds::Maximized(current), true, false, None),
            None,
            "没有更早的普通尺寸时保持空，下次按最小尺寸打开"
        );
    }
}

/// 选中动画里在一个布尔状态之间插值。出现用缓出，收回用缓入。
fn animate_selection_flag(from: bool, to: bool, elapsed_ms: f32, in_ms: u64, out_ms: u64) -> f32 {
    let from: f32 = if from { 1. } else { 0. };
    let to: f32 = if to { 1. } else { 0. };
    if (from - to).abs() < f32::EPSILON {
        return to;
    }
    let duration = if to > from { in_ms } else { out_ms } as f32;
    let t = (elapsed_ms / duration.max(1.)).clamp(0., 1.);
    let eased = if to > from {
        1. - (1. - t).powi(5)
    } else {
        t * t
    };
    from + (to - from) * eased
}

#[cfg(test)]
mod selection_math_tests {
    use super::animate_selection_flag;

    #[test]
    fn selection_flag_moves_from_start_to_end() {
        assert_eq!(animate_selection_flag(false, true, 0.0, 170, 140), 0.0);
        assert_eq!(animate_selection_flag(false, true, 170.0, 170, 140), 1.0);
        assert_eq!(animate_selection_flag(true, false, 0.0, 170, 140), 1.0);
        assert_eq!(animate_selection_flag(true, false, 140.0, 170, 140), 0.0);
        assert_eq!(animate_selection_flag(true, true, 80.0, 170, 140), 1.0);
    }

    #[test]
    fn selection_flag_midpoint_is_between_ends() {
        let rising = animate_selection_flag(false, true, 85.0, 170, 140);
        assert!(
            rising > 0.4 && rising < 1.0,
            "选中中点应已过渡大半：{rising}"
        );
        let falling = animate_selection_flag(true, false, 105.0, 170, 140);
        assert!(
            falling > 0.0 && falling < 0.6,
            "取消后段应已收回大半：{falling}"
        );
    }
}

#[cfg(test)]
mod decoding_counter_tests {
    use super::settle_decoding;

    #[test]
    fn same_epoch_task_reduces_counter() {
        assert_eq!(settle_decoding(1, 3, 3), 0);
        assert_eq!(settle_decoding(4, 3, 3), 3);
        // 清空后新导入又加了一条时，旧任务不能把它扣掉（这里本来就是同一段
        // 任务，正常减一，但永远不会减到 0 以下）。
        assert_eq!(settle_decoding(0, 3, 3), 0);
    }

    #[test]
    fn cleared_epoch_task_does_not_touch_counter() {
        // 清空前入队/在途的任务（代号 3）在清空后（代号 4）回传，不能扣新作品。
        assert_eq!(settle_decoding(1, 3, 4), 1);
        assert_eq!(settle_decoding(0, 3, 4), 0);
    }
}

#[cfg(test)]
mod ui_smoke;
