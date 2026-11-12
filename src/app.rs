//! 应用主状态：项目数据、图片导入、字幕检测调度、撤销/重做、导出。

pub mod actions;
mod canvas_gestures;
mod crop_drag;
pub mod keymap;

mod export;
mod undo;

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use gpui::prelude::*;
use gpui::{
    ClipboardEntry, Context, ExternalPaths, FocusHandle, IntoElement, Modifiers, Render,
    RenderImage, SharedString, Window, WindowBounds, div,
};
use image::RgbImage;

use crate::i18n::Language;
use crate::imaging::crop::{self, CropBand, Inclusion};
use crate::imaging::decode::{decode_bytes, decode_path, is_supported_path};
use crate::imaging::detect::{DetectError, SubtitleDetector};
use crate::imaging::render;
use crate::model::{
    Item, ItemId, ItemStatus, LayoutTarget, ManualBand, Mode, Project, SubtitleBlock,
};
use crate::ui::ghost_crop::SnapTier;
use crate::ui::hover;
use crate::ui::theme::Theme;
use crate::ui::tokens;

use actions::*;

/// 预览图宽度的下限，缩放很小时仍保留一点余量。
pub(crate) const PREVIEW_MIN_WIDTH: u32 = 320;
/// 预览图宽度的绝对上限。GPU 按物理像素采样，位图必须按“显示宽 × 系统缩放”
/// 生成才清晰；这个上限覆盖 1600 逻辑像素画布在 200% 缩放（或 2133 在 150%）
/// 的常见高分屏场景，再大的极端情况才允许轻微降采样，同时控制单张内存。
pub(crate) const PREVIEW_MAX_WIDTH: u32 = 3200;
/// 预览缓存最多保留多少张，超出就淘汰最久没用过的一张。位图按物理像素生成后
/// 单张更大，上限从 160 降到 96，避免缓存无限吃内存；淘汰按条进行，不清空整表，
/// 否则下一帧要把整屏位图重新生成一遍。
const PREVIEW_CACHE_LIMIT: usize = 96;
/// 撤销栈最多保留多少步。
const UNDO_LIMIT: usize = 100;
/// 吐司自动消失的时间。
const TOAST_MS: u64 = 2600;
/// 提示浮层出场淡出时长。
pub(crate) const TOAST_EXIT_MS: u64 = 130;

/// 一张预览位图，以及它最近一次被用到的序号。序号用来做最近最少使用淘汰。
pub(crate) struct PreviewEntry {
    pub(crate) image: Arc<RenderImage>,
    used: u64,
}

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

/// 拖裁剪线时携带的数据。
#[derive(Clone, Copy)]
pub struct CropHandleDrag {
    pub id: ItemId,
    pub edge: ManualEdge,
}

/// 拖接缝时携带的数据。
#[derive(Clone, Copy)]
pub struct SeamDrag {
    pub index: usize,
}

/// 拖出血位滑条时携带的数据。
#[derive(Clone, Copy)]
pub struct BleedDrag;

/// 拖排版画布右下角时携带的数据。
#[derive(Clone, Copy)]
pub struct CanvasCornerDrag;

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

pub(crate) use crate::ui::canvas::SeamAxis;
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
    /// 原图左上角在画布中的固定位置
    pub source_origin: gpui::Point<f32>,
    /// 这一拖累计自动滚动掉的像素
    pub auto_scroll_pixels: gpui::Point<f32>,
    /// 原图宽高
    pub image_size: (u32, u32),
    /// 显示像素 / 原图像素，横向和纵向各一份（排版会把某一轴拉满，两个轴不等）
    pub scale: gpui::Point<f32>,
    pub undo_recorded: bool,
    pub snap: Option<SnapTier>,
}

/// 一次接缝拖动中的状态。
pub(crate) struct GapDragState {
    pub index: usize,
    pub axis: SeamAxis,
    pub start_gap_px: u32,
    pub start_pointer: f32,
    /// 显示像素 / 输出像素
    pub scale: f32,
    pub undo_recorded: bool,
}

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

/// 右键卡片的类型和位置。
#[derive(Clone, Copy, Debug)]
pub struct MenuState {
    pub kind: MenuKind,
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

pub struct AppRoot {
    pub(crate) project: Project,
    pub(crate) undo: undo::UndoStack,
    pub(crate) detector: Option<Arc<SubtitleDetector>>,
    detector_loading: bool,
    pub(crate) next_id: ItemId,
    pub(crate) selected: Option<ItemId>,
    pub(crate) selected_ids: BTreeSet<ItemId>,
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
    pub(crate) zoom: f32,
    /// 最近一次渲染时的窗口大小，用来按黄金分割算默认显示比例。
    pub(crate) viewport: gpui::Size<gpui::Pixels>,
    pub(crate) preview_cache: HashMap<(ItemId, u32), PreviewEntry>,
    /// 预览缓存的访问序号，每取一次加一，用来做最近最少使用淘汰。
    preview_clock: u64,
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
    pub(crate) language_submenu_open: bool,
    /// 语言二级菜单已经挂了延迟收起的任务，避免重复挂。
    pub(crate) language_submenu_closing: bool,
    /// 画布的滚动位置。
    pub(crate) canvas_scroll: gpui::ScrollHandle,
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
            hover_gate: hover::HoverGate::new(),
            crop_drag: None,
            gap_drag: None,
            marquee: None,
            right_pending: None,
            dragging: None,
            drag_pointer: None,
            drag_over: None,
            settle: None,
            prev_gap_color: None,
            gap_color_started_at: None,
            gap_color_gen: 0,
            delete_hover: false,
            zoom: restored.zoom,
            viewport: window.viewport_size(),
            preview_cache: HashMap::new(),
            preview_clock: 0,
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
            clip_counter: 0,
            close_to_tray: restored.close_to_tray,
            language,
            close_near: false,
            close_fade_from: tokens::CLOSE_IDLE_OPACITY,
            close_fade_started: None,
            close_reveal_gen: 0,
            windowed_bounds: restored.windowed,
            menu: None,
            language_submenu_open: false,
            language_submenu_closing: false,
            canvas_scroll: gpui::ScrollHandle::new(),
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
                if self.pending_detection_count() > 0 {
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
                    if item.status == ItemStatus::Pending {
                        item.status = ItemStatus::Failed(detail.clone());
                        item.keep_whole = true;
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
            self.zoom,
            self.close_to_tray,
            windowed,
        )
    }

    pub(crate) fn save_state(&mut self, window: &Window) {
        self.remember_windowed_bounds(window);
        let state = self.saved_state(self.windowed_bounds);
        crate::config::save(&state);
    }

    pub(crate) fn set_close_to_tray(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.close_to_tray == enabled {
            return;
        }
        self.close_to_tray = enabled;
        cx.notify();
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
        self.selected = None;
        self.selected_ids.clear();
        self.crop_drag = None;
        self.gap_drag = None;
        self.set_status(self.language.switched_mode(mode), false, cx);
    }

    pub(crate) fn set_keep_second(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.project.settings.keep_second == enabled {
            return;
        }
        self.record_undo();
        self.project.settings.keep_second = enabled;
        self.preview_cache.clear();
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
        self.preview_cache.clear();
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

    pub(crate) fn select(&mut self, id: ItemId, cx: &mut Context<Self>) {
        if !self.project.items.iter().any(|item| item.id == id) {
            return;
        }
        self.selected = Some(id);
        self.selected_ids.clear();
        self.selected_ids.insert(id);
        self.crop_drag = None;
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
            cx.notify();
            return;
        }
        self.select(id, cx);
    }

    pub(crate) fn delete_selected_images(&mut self, cx: &mut Context<Self>) {
        if self.selected_ids.is_empty() {
            return;
        }
        let removed = self.selected_ids.len();
        self.record_undo();
        self.project
            .items
            .retain(|item| !self.selected_ids.contains(&item.id));
        self.selected_ids.clear();
        self.selected = None;
        self.crop_drag = None;
        self.preview_cache.clear();
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
        self.preview_cache.clear();
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
        self.preview_cache.clear();
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
        self.preview_cache.clear();
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
        self.preview_cache.clear();
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

    /// 清空当前作品。托盘和右键卡片都走这里。
    pub(crate) fn clear_all(&mut self, cx: &mut Context<Self>) {
        if self.project.items.is_empty() {
            return;
        }
        self.record_undo();
        self.project.items.clear();
        self.selected = None;
        self.selected_ids.clear();
        self.crop_drag = None;
        self.gap_drag = None;
        self.drag_over = None;
        self.close_menu(cx);
        self.preview_cache.clear();
        self.set_status(self.language.cleared_all(), false, cx);
    }

    // ------------------------------------------------------------------
    // 缩放
    // ------------------------------------------------------------------

    fn zoom_step(&mut self, direction: i32, cx: &mut Context<Self>) {
        self.zoom = tokens::clamp_zoom(self.zoom + direction as f32 * tokens::ZOOM_STEP);
        cx.notify();
    }

    pub(crate) fn zoom_by(&mut self, delta: f32, cx: &mut Context<Self>) {
        let next = tokens::clamp_zoom(self.zoom + delta);
        if (next - self.zoom).abs() < 1e-4 {
            return;
        }
        self.zoom = next;
        cx.notify();
    }

    pub(crate) fn action_zoom_in(
        &mut self,
        _: &ZoomIn,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.zoom_step(1, cx);
    }

    pub(crate) fn action_zoom_out(
        &mut self,
        _: &ZoomOut,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.zoom_step(-1, cx);
    }

    pub(crate) fn action_zoom_reset(&mut self, cx: &mut Context<Self>) {
        if (self.zoom - 1.0).abs() < 1e-4 {
            return;
        }
        self.zoom = 1.0;
        cx.notify();
    }

    fn on_zoom_reset(&mut self, _: &ZoomReset, _window: &mut Window, cx: &mut Context<Self>) {
        self.action_zoom_reset(cx);
    }

    /// 拖排版画布右下角时，按当前宽高比挑最接近的预设比例。
    pub(crate) fn drag_canvas_corner(&mut self, width: f32, height: f32, cx: &mut Context<Self>) {
        if width <= 1.0 || height <= 1.0 {
            return;
        }
        let next = self.nearest_aspect(width / height);
        if self.project.settings.layout == next {
            return;
        }
        self.set_layout_target(next, cx);
    }

    fn nearest_aspect(&self, ratio: f32) -> LayoutTarget {
        let presets: [f32; 4] = [16.0 / 9.0, 9.0 / 16.0, 4.0 / 3.0, 1.0];
        let target = ratio.max(1e-4).ln();
        presets
            .into_iter()
            .min_by(|a, b| (a.ln() - target).abs().total_cmp(&(b.ln() - target).abs()))
            .map(LayoutTarget::Aspect)
            .unwrap_or_default()
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
            let name = path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.display().to_string());
            let task = cx
                .background_executor()
                .spawn(async move { decode_path(&path) });
            self.track_decode(name, task, cx);
        }
    }

    pub(crate) fn import_bytes(&mut self, name: String, bytes: Vec<u8>, cx: &mut Context<Self>) {
        let task = cx
            .background_executor()
            .spawn(async move { decode_bytes(&bytes) });
        self.track_decode(name, task, cx);
    }

    fn track_decode(
        &mut self,
        name: String,
        task: gpui::Task<Result<RgbImage, crate::imaging::decode::DecodeError>>,
        cx: &mut Context<Self>,
    ) {
        self.decoding += 1;
        cx.spawn(async move |this, cx| {
            let result = task.await;
            let _ = this.update(cx, |this, cx| {
                this.decoding = this.decoding.saturating_sub(1);
                match result {
                    Ok(rgb) => this.push_image(name, rgb, cx),
                    Err(err) => {
                        let _ = name;
                        this.set_status(
                            this.language
                                .read_image_failed(&err.localized(this.language)),
                            true,
                            cx,
                        );
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn push_image(&mut self, name: String, rgb: RgbImage, cx: &mut Context<Self>) {
        let (width, height) = rgb.dimensions();
        if width == 0 || height == 0 {
            self.set_status(self.language.empty_image(&name), true, cx);
            return;
        }
        let id = self.next_id;
        self.next_id += 1;
        let (status, keep_whole) = if self.detector.is_some() || self.detector_loading {
            (ItemStatus::Pending, false)
        } else {
            (ItemStatus::Ready, true)
        };
        self.record_undo();
        self.project.items.push(Item {
            id,
            name,
            width,
            height,
            rgb: Arc::new(rgb),
            block: None,
            status,
            keep_whole,
            manual: None,
        });
        self.selected = Some(id);
        self.selected_ids.clear();
        self.selected_ids.insert(id);
        self.crop_drag = None;
        let count = self.project.items.len();
        if self.detector.is_some() {
            self.set_status(self.language.added_detecting(count), false, cx);
            self.start_detection(cx);
        } else if self.detector_loading {
            self.set_status(self.language.added_detector_loading(count), false, cx);
        } else {
            self.set_status(self.language.added_detector_unavailable(count), false, cx);
        }
    }

    pub(crate) fn is_busy(&self) -> bool {
        self.detector_loading
            || self.detecting
            || self.decoding > 0
            || self.pending_detection_count() > 0
    }

    pub(crate) fn pending_detection_count(&self) -> usize {
        self.project
            .items
            .iter()
            .filter(|item| item.status == ItemStatus::Pending)
            .count()
    }

    fn take_pending_item(&mut self) -> Option<(ItemId, Arc<RgbImage>)> {
        let item = self
            .project
            .items
            .iter()
            .find(|item| item.status == ItemStatus::Pending)?;
        Some((item.id, item.rgb.clone()))
    }

    pub(crate) fn start_detection(&mut self, cx: &mut Context<Self>) {
        if self.detecting || self.pending_detection_count() == 0 {
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
                let Some((id, rgb)) = next else {
                    break;
                };
                let detector = detector.clone();
                let detected = cx
                    .background_executor()
                    .spawn(async move { detector.detect(&rgb) })
                    .await;
                let _ = this.update(cx, |this, cx| {
                    this.apply_detection(id, detected);
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

    fn apply_detection(&mut self, id: ItemId, result: Result<Option<SubtitleBlock>, DetectError>) {
        let Some(item) = self.project.items.iter_mut().find(|item| item.id == id) else {
            return;
        };
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
    }

    fn on_detection_finished(&mut self, cx: &mut Context<Self>) {
        self.preview_cache.clear();
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

    /// 取这张图的预览位图。
    ///
    /// 缓存键只有「哪张图 + 目标宽度」，和裁剪范围无关：裁剪是界面用容器裁切
    /// 加位图位移做出来的，所以拖裁剪线、拖横排共用边都不会让位图重画。
    ///
    /// `freeze` 为真时（正在拖动裁剪线、接缝或排序），即使目标宽度落到新档位
    /// 也不生成新位图，改用这张图已有的最合适缓存；松手后再按新宽度生成一次。
    /// 这样拖动全程不会反复做整图缩放和逐像素转换。
    pub(crate) fn preview_for(
        &mut self,
        id: ItemId,
        source: &RgbImage,
        target_width: u32,
        freeze: bool,
    ) -> Arc<RenderImage> {
        self.preview_clock = self.preview_clock.wrapping_add(1);
        let clock = self.preview_clock;
        let key = (id, target_width);
        if let Some(entry) = self.preview_cache.get_mut(&key) {
            entry.used = clock;
            return entry.image.clone();
        }
        if freeze {
            let fallback = self
                .preview_cache
                .iter_mut()
                .filter(|((item_id, _), _)| *item_id == id)
                .max_by_key(|(_, entry)| entry.used)
                .map(|(_, entry)| {
                    entry.used = clock;
                    entry.image.clone()
                });
            if let Some(image) = fallback {
                return image;
            }
        }
        let image = render::preview_image(source, target_width);
        if self.preview_cache.len() >= PREVIEW_CACHE_LIMIT {
            let oldest = self
                .preview_cache
                .iter()
                .min_by_key(|(_, entry)| entry.used)
                .map(|(key, _)| *key);
            if let Some(oldest) = oldest {
                self.preview_cache.remove(&oldest);
            }
        }
        self.preview_cache.insert(
            key,
            PreviewEntry {
                image: image.clone(),
                used: clock,
            },
        );
        image
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

    /// 当前作品的输出尺寸。复制成功的吐司里会带上它。
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

impl Render for AppRoot {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_appearance(window.appearance());
        let viewport = window.viewport_size();
        self.viewport = viewport;
        // 窗口每帧都可能被移动或调整大小，这里记住最近一次普通尺寸；
        // 之后就算用户最大化再退出，保存的仍是最大化之前的尺寸。
        self.remember_windowed_bounds(window);
        // 拖动在画布空白处松手时没有落点元素接手，GPUI 会直接结束拖动。
        // 这里补一次清理，避免图还停在“被摘掉”的状态、跟手小图也留在屏幕上。
        if self.dragging.is_some() && !cx.has_active_drag() {
            self.dragging = None;
            self.drag_over = None;
            self.drag_pointer = None;
            self.delete_hover = false;
        }
        // 跟手小图最后画，保证盖在画布、胶囊、菜单之上。
        let drag_thumb = self.render_drag_thumb(theme, window);

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
            .on_action(cx.listener(Self::action_zoom_in))
            .on_action(cx.listener(Self::action_zoom_out))
            .on_action(cx.listener(Self::on_zoom_reset))
            .on_action(cx.listener(Self::action_dismiss))
            .on_action(cx.listener(Self::action_quit))
            .on_mouse_move(cx.listener(Self::track_close_region))
            .on_drop(cx.listener(Self::on_file_drop))
            .child(self.render_canvas(window, theme, cx))
            .child(self.render_capsule(theme, cx))
            .child(self.render_window_controls(theme, cx))
            .when(self.menu.is_some(), |this| {
                this.child(self.render_context_menu(theme, viewport, cx))
            })
            .when_some(drag_thumb, |this, thumb| this.child(thumb))
    }
}

#[cfg(test)]
mod escape_tests {
    use super::*;
    use gpui::{Bounds, point, px, size};

    #[test]
    fn escape_only_hides_when_close_to_tray_is_on() {
        assert_eq!(escape_outcome(true), EscapeOutcome::HideToTray);
        assert_eq!(escape_outcome(false), EscapeOutcome::DoNothing);
    }

    #[test]
    fn saving_ignores_maximized_and_fullscreen_bounds() {
        let current = Bounds {
            origin: point(px(100.0), px(50.0)),
            size: size(px(1500.0), px(900.0)),
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

#[cfg(test)]
mod ui_smoke;
