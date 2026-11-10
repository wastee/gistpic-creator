//! 应用主状态：项目数据、图片导入、字幕检测调度、撤销/重做、导出。

pub mod actions;
pub mod keymap;

mod export;
mod undo;

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::Arc;

use gpui::prelude::*;
use gpui::{
    ClipboardEntry, Context, ExternalPaths, FocusHandle, IntoElement, Modifiers, PathPromptOptions,
    Render, RenderImage, SharedString, Window, div, px,
};
use image::RgbImage;

use crate::i18n::Language;
use crate::imaging::crop::{self, CropBand, Inclusion};
use crate::imaging::decode::{decode_bytes, decode_path, is_supported_path};
use crate::imaging::detect::{DetectError, SubtitleDetector};
use crate::imaging::render;
use crate::model::{
    Item, ItemId, ItemStatus, LayoutTarget, ManualBand, Mode, Project, Rgba, SubtitleBlock,
};
use crate::platform;
use crate::ui::rail::RailPanel;
use crate::ui::tokens::{size, space};
use crate::ui::theme::Theme;

use actions::*;

/// 预览图统一生成的最大宽度（显示时再按缩放比例拉伸）。
pub(crate) const PREVIEW_TARGET_WIDTH: u32 = 640;
/// 预览缓存最多保留多少张，超出就整体清空。
const PREVIEW_CACHE_LIMIT: usize = 160;
/// 字幕模式下预览的基准显示宽度。
pub(crate) const BASE_PREVIEW_WIDTH: f32 = 560.0;
/// 撤销栈最多保留多少步。
const UNDO_LIMIT: usize = 100;

/// 状态栏里的一条消息。
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

    pub fn default_for(language: Language) -> Self {
        Status::info(language.default_hint(&platform::shortcut_label("V")))
    }
}

impl Default for Status {
    fn default() -> Self {
        Status::default_for(Language::default())
    }
}

/// 拖拽排序时携带的数据。
#[derive(Clone, Copy)]
pub struct DragItem {
    pub id: ItemId,
}

/// 手动框选拖的是上边界还是下边界。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManualEdge {
    Top,
    Bottom,
}

/// 拖动手动框选横线时携带的数据。
#[derive(Clone, Copy)]
pub struct ManualDrag {
    pub id: ItemId,
    pub edge: ManualEdge,
}

/// 一次手动框选拖动中的状态。第一次真正改变范围时才记录撤销点。
pub(crate) struct ManualDragState {
    pub id: ItemId,
    pub edge: ManualEdge,
    pub start: ManualBand,
    pub undo_recorded: bool,
}

pub struct AppRoot {
    pub(crate) project: Project,
    pub(crate) undo: undo::UndoStack,
    pub(crate) detector: Option<Arc<SubtitleDetector>>,
    pub(crate) next_id: ItemId,
    pub(crate) selected: Option<ItemId>,
    pub(crate) selected_ids: BTreeSet<ItemId>,
    /// 正在手动框选的图片；没有值时不显示框选控件。
    pub(crate) crop_edit: Option<ItemId>,
    pub(crate) manual_drag: Option<ManualDragState>,
    pub(crate) zoom: f32,
    pub(crate) preview_cache: HashMap<(ItemId, u32, u32, u32), Arc<RenderImage>>,
    pub(crate) status: Status,
    pub(crate) detecting: bool,
    pub(crate) decoding: usize,
    pub(crate) drag_over: Option<ItemId>,
    pub(crate) clip_counter: usize,
    /// 点窗口关闭按钮时：true 隐藏窗口让程序继续在托盘运行，false 直接退出。
    pub(crate) close_to_tray: bool,
    /// 当前界面语言。默认中文，只在本次运行内切换。
    pub(crate) language: Language,
    /// 右边缘图标条是否被图钉固定住。
    pub(crate) rail_pinned: bool,
    /// 图标条当前展开的是哪一块。
    pub(crate) rail_active: Option<RailPanel>,
    /// 鼠标是否停在图标条或展开的面板上。
    pub(crate) rail_open: Option<RailPanel>,
    /// 左上角更多菜单是否展开。
    pub(crate) more_menu_open: bool,
    /// 放大浮层是否打开。
    pub(crate) zoom_layer: bool,
    /// 浮层里是否切到了「叠放预览」的纯展示视图。
    pub(crate) zoom_stack_preview: bool,
    /// 图片右键菜单的位置和指向的图片。
    pub(crate) image_menu: Option<ImageMenu>,
    focus_handle: FocusHandle,
}

/// 图片右键菜单：出现在哪个位置，指向哪张图。
#[derive(Clone, Copy, Debug)]
pub struct ImageMenu {
    pub id: ItemId,
    /// 菜单左上角在窗口里的坐标。
    pub x: f32,
    pub y: f32,
}

impl AppRoot {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        // 启动时读回上次的界面设置。读不到就用默认值。
        let saved = crate::config::load();
        let restored = saved.restore();
        let language = restored.language;

        #[cfg(any(debug_assertions, feature = "dev-tools"))]
        let started = std::time::Instant::now();
        let (detector, status) = match SubtitleDetector::new_embedded() {
            Ok(detector) => (Some(Arc::new(detector)), Status::default_for(language)),
            Err(err) => (
                None,
                Status::error(language.detector_load_failed(&err.localized(language))),
            ),
        };

        #[cfg(any(debug_assertions, feature = "dev-tools"))]
        log::info!("字幕检测模型加载完成，用时 {:?}", started.elapsed());

        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle);

        // 关闭按钮：默认隐藏窗口，让程序留在托盘；关掉设置后直接退出。
        let this = cx.weak_entity();
        window.on_window_should_close(cx, move |window, cx| {
            let Some(root) = this.upgrade() else {
                return true;
            };
            root.update(cx, |root, cx| root.on_close_requested(window, cx))
        });

        // 窗口标题和托盘菜单要跟着记住的语言走。
        window.set_window_title(language.app_name());
        crate::tray::set_language(language);

        AppRoot {
            project: Project {
                mode: restored.mode,
                settings: restored.settings.clone(),
                items: Vec::new(),
            },
            undo: undo::UndoStack::new(UNDO_LIMIT),
            detector,
            next_id: 1,
            selected: None,
            selected_ids: BTreeSet::new(),
            crop_edit: None,
            manual_drag: None,
            zoom: restored.zoom,
            preview_cache: HashMap::new(),
            status,
            detecting: false,
            decoding: 0,
            drag_over: None,
            clip_counter: 0,
            close_to_tray: restored.close_to_tray,
            language,
            rail_pinned: restored.rail_pinned,
            rail_active: None,
            rail_open: None,
            more_menu_open: false,
            zoom_layer: false,
            zoom_stack_preview: false,
            image_menu: None,
            focus_handle,
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
        cx.notify();
    }

    /// 点窗口关闭按钮时的处理：默认隐藏窗口，让程序继续在托盘运行。
    pub(crate) fn on_close_requested(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        #[cfg(any(debug_assertions, feature = "dev-tools"))]
        log::info!("关闭按钮：close_to_tray={}", self.close_to_tray);
        // 隐藏到托盘时也要记住设置：用户可能以后都不再打开主窗口，
        // 只在真正退出时保存会丢掉这次的改动。隐藏和退出都保存一次。
        self.save_state(window);
        if self.close_to_tray {
            // 隐藏窗口但保留窗口对象、GPU 表面和托盘事件循环。
            window.set_visible(false);
        } else {
            cx.quit();
        }
        // 返回 false 阻止平台销毁窗口；隐藏模式保留窗口，退出模式由 cx.quit 处理。
        false
    }

    /// 把当前界面状态写到配置文件。写失败只打日志，不影响退出。
    pub(crate) fn save_state(&self, window: &Window) {
        let window_rect = window.window_bounds().get_bounds();
        let window_rect = crate::config::WindowRect {
            x: f32::from(window_rect.origin.x),
            y: f32::from(window_rect.origin.y),
            width: f32::from(window_rect.size.width),
            height: f32::from(window_rect.size.height),
        };
        let state = crate::config::SavedState::capture(
            self.language,
            self.project.mode,
            &self.project.settings,
            self.zoom,
            self.rail_pinned,
            self.close_to_tray,
            Some(window_rect),
        );
        crate::config::save(&state);
    }

    pub(crate) fn set_close_to_tray(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.close_to_tray == enabled {
            return;
        }
        self.close_to_tray = enabled;
        cx.notify();
    }

    /// 切换界面语言。窗口标题、托盘菜单和状态栏提示一起更新。
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
        self.status = Status::default_for(language);
        cx.notify();
    }

    pub(crate) fn record_undo(&mut self) {
        self.undo.record(&self.project);
    }

    // ------------------------------------------------------------------
    // 右边缘图标条与左上角菜单
    // ------------------------------------------------------------------

    /// 鼠标停在图标条所在的竖条上：展开的面板要跟着鼠标在不在而变。
    pub(crate) fn set_rail_hover(&mut self, hovered: bool, cx: &mut Context<Self>) {
        if hovered {
            // 第一次碰到图标条时，默认展开当前模式下最相关的那一块。
            if self.rail_open.is_none() {
                self.rail_open = Some(self.default_rail_panel());
            }
        } else if !self.rail_pinned {
            self.rail_open = None;
        } else {
            // 固定住时移开鼠标，面板内容不变，但仍然记录鼠标已离开。
            self.rail_open = self.rail_active;
        }
        cx.notify();
    }

    /// 当前模式下默认展开哪一块。
    fn default_rail_panel(&self) -> RailPanel {
        match self.project.mode {
            Mode::Subtitle => RailPanel::Subtitle,
            Mode::Layout => RailPanel::Layout,
        }
    }

    /// 点图标条上的某个图标：展开它；已经展开且固定时再点就收起。
    pub(crate) fn toggle_rail_panel(&mut self, panel: RailPanel, cx: &mut Context<Self>) {
        if self.rail_pinned && self.rail_active == Some(panel) {
            self.rail_pinned = false;
            self.rail_active = None;
            self.rail_open = None;
        } else {
            self.rail_active = Some(panel);
            self.rail_open = Some(panel);
        }
        cx.notify();
    }

    /// 点图钉：固定或取消固定。
    pub(crate) fn toggle_rail_pin(&mut self, cx: &mut Context<Self>) {
        self.rail_pinned = !self.rail_pinned;
        if self.rail_pinned {
            if self.rail_active.is_none() {
                self.rail_active = Some(self.default_rail_panel());
            }
            self.rail_open = self.rail_active;
        }
        cx.notify();
    }

    /// 点左上角或右下角的更多按钮：展开低频项菜单。
    pub(crate) fn toggle_more_menu(&mut self, cx: &mut Context<Self>) {
        self.more_menu_open = !self.more_menu_open;
        cx.notify();
    }

    pub(crate) fn close_more_menu(&mut self, cx: &mut Context<Self>) {
        if self.more_menu_open {
            self.more_menu_open = false;
            cx.notify();
        }
    }

    // ------------------------------------------------------------------
    // 放大浮层与图片菜单
    // ------------------------------------------------------------------

    /// 双击一张图：打开浮层并进入框选状态。
    pub(crate) fn open_zoom_layer(&mut self, id: ItemId, cx: &mut Context<Self>) {
        if !self.project.items.iter().any(|item| item.id == id) {
            return;
        }
        // 双击的这张不在当前选中集合里时，改成只选它。
        if !self.selected_ids.contains(&id) {
            self.selected = Some(id);
            self.selected_ids.clear();
            self.selected_ids.insert(id);
        } else {
            self.selected = Some(id);
        }
        self.image_menu = None;
        self.zoom_layer = true;
        self.zoom_stack_preview = false;
        // 打开发层就直接能拖边界，不再要求先点「框选这张」。
        self.crop_edit = None;
        self.manual_drag = None;
        if self.can_edit_band(id) {
            self.crop_edit = Some(id);
        }
        cx.notify();
    }

    /// 这张图能不能手动框选：要有尺寸、不是失败状态、并且能看到裁剪结果。
    fn can_edit_band(&self, id: ItemId) -> bool {
        let Some(item) = self.project.items.iter().find(|item| item.id == id) else {
            return false;
        };
        if matches!(item.status, ItemStatus::Pending | ItemStatus::Failed(_)) {
            return false;
        }
        item.height > 0 && self.current_band_for(id).is_some()
    }

    /// 关闭浮层。框选状态一起清掉，但拖出来的结果保留。
    pub(crate) fn close_zoom_layer(&mut self, cx: &mut Context<Self>) {
        if !self.zoom_layer {
            return;
        }
        self.zoom_layer = false;
        self.zoom_stack_preview = false;
        self.crop_edit = None;
        self.manual_drag = None;
        cx.notify();
    }

    /// 浮层里切换「叠放预览」的纯展示视图。
    pub(crate) fn toggle_zoom_stack_preview(&mut self, cx: &mut Context<Self>) {
        self.zoom_stack_preview = !self.zoom_stack_preview;
        cx.notify();
    }

    /// 在图上点右键：打开图片菜单。
    pub(crate) fn open_image_menu(
        &mut self,
        id: ItemId,
        x: f32,
        y: f32,
        cx: &mut Context<Self>,
    ) {
        if !self.project.items.iter().any(|item| item.id == id) {
            return;
        }
        // 右键点在没有被选中的图上时，先把它变成唯一选中项。
        if !self.selected_ids.contains(&id) {
            self.selected = Some(id);
            self.selected_ids.clear();
            self.selected_ids.insert(id);
        } else {
            self.selected = Some(id);
        }
        self.more_menu_open = false;
        self.image_menu = Some(ImageMenu { id, x, y });
        cx.notify();
    }

    pub(crate) fn close_image_menu(&mut self, cx: &mut Context<Self>) {
        if self.image_menu.take().is_some() {
            cx.notify();
        }
    }

    /// 按 Esc：从最上面一层开始关。
    pub(crate) fn dismiss_overlays(&mut self, cx: &mut Context<Self>) {
        if self.image_menu.is_some() {
            self.close_image_menu(cx);
        } else if self.more_menu_open {
            self.close_more_menu(cx);
        } else if self.zoom_layer {
            self.close_zoom_layer(cx);
        }
    }

    fn zoom_step(&mut self, direction: i32, cx: &mut Context<Self>) {
        self.zoom = (self.zoom + direction as f32 * 0.1).clamp(0.5, 2.0);
        cx.notify();
    }

    /// 按指定幅度改变预览缩放。Ctrl+滚轮用这个。
    pub(crate) fn zoom_by(&mut self, delta: f32, cx: &mut Context<Self>) {
        let next = crate::ui::tokens::clamp_zoom(self.zoom + delta);
        if (next - self.zoom).abs() < 1e-4 {
            return;
        }
        self.zoom = next;
        cx.notify();
    }

    /// 拖画布右下角时，按当前宽高比挑最接近的预设比例。
    ///
    /// `width` 和 `height` 是拖到的位置相对画布左上角的尺寸。
    pub(crate) fn drag_canvas_corner(
        &mut self,
        width: f32,
        height: f32,
        cx: &mut Context<Self>,
    ) {
        if width <= 1.0 || height <= 1.0 {
            return;
        }
        let ratio = width / height;
        let next = self.nearest_aspect(ratio);
        if self.project.settings.layout == next {
            return;
        }
        self.set_layout_target(next, cx);
    }

    /// 从四个固定比例里挑和目标最接近的那个。
    fn nearest_aspect(&self, ratio: f32) -> LayoutTarget {
        let presets: [f32; 4] = [16.0 / 9.0, 9.0 / 16.0, 4.0 / 3.0, 1.0];
        let target = ratio.max(1e-4).ln();
        presets
            .into_iter()
            .min_by(|a, b| {
                // 用比例的对数距离，竖图横图之间的比较才公平
                (a.ln() - target).abs().total_cmp(&(b.ln() - target).abs())
            })
            .map(LayoutTarget::Aspect)
            .unwrap_or_default()
    }

    // ------------------------------------------------------------------
    // 导入
    // ------------------------------------------------------------------

    pub(crate) fn action_paste(
        &mut self,
        _: &PasteImage,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.paste(cx);
    }

    fn paste(&mut self, cx: &mut Context<Self>) {
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

    fn open_images(&mut self, cx: &mut Context<Self>) {
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

    fn on_file_drop(
        &mut self,
        paths: &ExternalPaths,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_paths(paths.paths().to_vec(), cx);
    }

    fn open_paths(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
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

    fn import_bytes(&mut self, name: String, bytes: Vec<u8>, cx: &mut Context<Self>) {
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

        // 模型没加载成功时，字幕功能不可用，但图片仍然应该能用于排版模式。
        let (status, keep_whole) = if self.detector.is_some() {
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
        self.crop_edit = None;
        self.manual_drag = None;
        let count = self.project.items.len();
        if self.detector.is_some() {
            self.set_status(self.language.added_detecting(count), false, cx);
            self.start_detection(cx);
        } else {
            self.set_status(self.language.added_detector_unavailable(count), false, cx);
        }
    }

    // ------------------------------------------------------------------
    // 字幕检测
    // ------------------------------------------------------------------

    pub(crate) fn is_busy(&self) -> bool {
        self.detecting || self.decoding > 0 || self.pending_detection_count() > 0
    }

    fn pending_detection_count(&self) -> usize {
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
                // 没有字幕时自动保留整图，不弹窗询问用户。
                item.keep_whole = true;
            }
            Err(err) => {
                item.block = None;
                item.status = ItemStatus::Failed(err.localized(self.language));
                // 识别失败也保留整图，不丢失用户内容。
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
        self.crop_edit = None;
        self.manual_drag = None;
        self.set_status(self.language.switched_mode(mode), false, cx);
    }

    pub(crate) fn set_crop(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.project.settings.crop == enabled {
            return;
        }
        self.record_undo();
        self.project.settings.crop = enabled;
        self.crop_edit = None;
        self.manual_drag = None;
        self.preview_cache.clear();
        self.set_status(
            if enabled {
                self.language.crop_enabled()
            } else {
                self.language.crop_disabled()
            },
            false,
            cx,
        );
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

    pub(crate) fn set_bleed_index(&mut self, index: usize) {
        // 滑条一共 7 档：0%、5%、10%、15%、20%、25%、30%
        let bleed = (index.min(6) as f32) * 0.05;
        if (self.project.settings.bleed - bleed).abs() < 1e-4 {
            return;
        }
        self.record_undo();
        self.project.settings.bleed = bleed;
        self.preview_cache.clear();
    }

    pub(crate) fn set_gap_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.project.settings.gap.enabled == enabled {
            return;
        }
        self.record_undo();
        self.project.settings.gap.enabled = enabled;
        self.set_status(
            if enabled {
                self.language.gap_enabled()
            } else {
                self.language.gap_disabled()
            },
            false,
            cx,
        );
    }

    pub(crate) fn step_gap_px(&mut self, direction: i32) {
        let current = self.project.settings.gap.px as i32;
        let next = (current + direction * 2).clamp(0, 40) as u32;
        if next == self.project.settings.gap.px {
            return;
        }
        self.record_undo();
        self.project.settings.gap.px = next;
    }

    pub(crate) fn set_gap_color(&mut self, color: Rgba, cx: &mut Context<Self>) {
        if self.project.settings.gap.color == color {
            return;
        }
        self.record_undo();
        self.project.settings.gap.color = color;
        cx.notify();
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

    /// 普通点击：只选中这一张。
    pub(crate) fn select(&mut self, id: ItemId, cx: &mut Context<Self>) {
        if !self.project.items.iter().any(|item| item.id == id) {
            return;
        }
        self.selected = Some(id);
        self.selected_ids.clear();
        self.selected_ids.insert(id);
        if self.crop_edit != Some(id) {
            self.crop_edit = None;
        }
        self.manual_drag = None;
        cx.notify();
    }

    /// 点击时带 Ctrl/Cmd 或 Shift：增加、移除或选中一段。
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
            if self.crop_edit != self.selected {
                self.crop_edit = None;
            }
            self.manual_drag = None;
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
            if self
                .crop_edit
                .is_some_and(|editing| !self.selected_ids.contains(&editing))
            {
                self.crop_edit = None;
            }
            self.manual_drag = None;
            cx.notify();
            return;
        }

        self.select(id, cx);
    }

    pub(crate) fn delete_selected_image(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected else {
            return;
        };
        self.record_undo();
        self.project.items.retain(|item| item.id != id);
        self.selected_ids.remove(&id);
        self.selected = self.selected_ids.iter().next().copied();
        self.crop_edit = None;
        self.manual_drag = None;
        self.preview_cache.clear();
        self.set_status(self.language.deleted_selected(), false, cx);
    }

    /// 删除当前选中的所有图片。只选中一张时效果和删除这张一样。
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
        self.crop_edit = None;
        self.manual_drag = None;
        self.zoom_layer = false;
        self.zoom_stack_preview = false;
        self.preview_cache.clear();
        let message = if removed > 1 {
            self.language.deleted_selected_count(removed)
        } else {
            self.language.deleted_selected().to_string()
        };
        self.set_status(message, false, cx);
    }

    // ------------------------------------------------------------------
    // 手动框选
    // ------------------------------------------------------------------

    /// 当前这张图实际使用的裁剪带：优先手动值，否则用自动结果。
    pub(crate) fn current_band_for(&self, id: ItemId) -> Option<CropBand> {
        let index = self.project.items.iter().position(|item| item.id == id)?;
        let item = &self.project.items[index];
        if let Some(manual) = item.manual {
            return crop::manual_band_for_item(item, manual);
        }
        match crop::resolve(&self.project).get(index).copied() {
            Some(Inclusion::Crop(band)) => Some(band),
            Some(Inclusion::FullImage) => Some(CropBand {
                y0: 0,
                y1: item.height,
            }),
            _ => None,
        }
    }

    pub(crate) fn current_manual_for(&self, id: ItemId) -> Option<ManualBand> {
        let band = self.current_band_for(id)?;
        let item = self.project.items.iter().find(|item| item.id == id)?;
        if item.height == 0 {
            return None;
        }
        let height = item.height as f32;
        ManualBand::clamped(band.y0 as f32 / height, band.y1 as f32 / height)
    }

    /// 把某一张设成「正在手动框选」状态。界面入口是打开浮层，
    /// 这里只改数据，供界面测试直接调用。
    #[cfg(test)]
    pub(crate) fn begin_manual_edit(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected else {
            return;
        };
        let Some(item) = self.project.items.iter().find(|item| item.id == id) else {
            return;
        };
        if matches!(item.status, ItemStatus::Pending | ItemStatus::Failed(_)) {
            return;
        }
        if item.height == 0 || self.current_band_for(id).is_none() {
            return;
        }
        self.crop_edit = Some(id);
        self.manual_drag = None;
        cx.notify();
    }

    pub(crate) fn begin_manual_drag(
        &mut self,
        id: ItemId,
        edge: ManualEdge,
        cx: &mut Context<Self>,
    ) {
        if self.crop_edit != Some(id) {
            return;
        }
        let Some(start) = self.current_manual_for(id) else {
            return;
        };
        self.selected = Some(id);
        self.selected_ids.insert(id);
        self.manual_drag = Some(ManualDragState {
            id,
            edge,
            start,
            undo_recorded: false,
        });
        cx.notify();
    }

    /// 拖动横线时按图片实际高度换算比例。
    pub(crate) fn update_manual_drag(
        &mut self,
        id: ItemId,
        edge: ManualEdge,
        ratio: f32,
        cx: &mut Context<Self>,
    ) {
        let Some(state) = self.manual_drag.as_ref() else {
            return;
        };
        if state.id != id || state.edge != edge {
            return;
        }
        let start = state.start;
        let ratio = ratio.clamp(0.0, 1.0);
        let (top, bottom) = match edge {
            ManualEdge::Top => (ratio.min(start.bottom - 0.01), start.bottom),
            ManualEdge::Bottom => (start.top, ratio.max(start.top + 0.01)),
        };
        let Some(manual) = ManualBand::clamped(top, bottom) else {
            return;
        };
        if manual == start {
            return;
        }

        if !self
            .manual_drag
            .as_ref()
            .is_some_and(|state| state.undo_recorded)
        {
            self.record_undo();
            if let Some(state) = self.manual_drag.as_mut() {
                state.undo_recorded = true;
            }
        }

        // 浮层里拖一张的边界，所有选中的图按相同的画面高度百分比一起变。
        let targets: Vec<ItemId> = if self.zoom_layer {
            self.selected_ids
                .iter()
                .copied()
                .filter(|target| {
                    self.project.items.iter().any(|item| {
                        item.id == *target
                            && item.width > 0
                            && item.height > 0
                            && !matches!(item.status, ItemStatus::Failed(_))
                    })
                })
                .collect()
        } else {
            vec![id]
        };

        for target in targets {
            let Some(item) = self.project.items.iter_mut().find(|item| item.id == target) else {
                continue;
            };
            item.manual = Some(manual);
            item.keep_whole = false;
        }
        self.preview_cache.clear();
        cx.notify();
    }

    pub(crate) fn end_manual_drag(&mut self) {
        self.manual_drag = None;
    }

    fn apply_manual_band_to(&mut self, ids: &[ItemId], cx: &mut Context<Self>) {
        let Some(active) = self.selected else {
            return;
        };
        let Some(manual) = self.current_manual_for(active) else {
            return;
        };
        let ids: Vec<ItemId> = ids
            .iter()
            .copied()
            .filter(|id| {
                self.project.items.iter().any(|item| {
                    item.id == *id
                        && item.width > 0
                        && item.height > 0
                        && !matches!(item.status, ItemStatus::Failed(_))
                })
            })
            .collect();
        if ids.is_empty() {
            return;
        }

        let needs_change = ids.iter().any(|id| {
            self.project
                .items
                .iter()
                .find(|item| item.id == *id)
                .is_some_and(|item| item.manual != Some(manual) || item.keep_whole)
        });
        if !needs_change {
            return;
        }

        self.record_undo();
        for item in self.project.items.iter_mut() {
            if ids.contains(&item.id) {
                item.manual = Some(manual);
                item.keep_whole = false;
            }
        }
        self.preview_cache.clear();
        self.set_status(self.language.manual_applied(ids.len()), false, cx);
    }

    pub(crate) fn apply_manual_to_selected(&mut self, cx: &mut Context<Self>) {
        if self.selected_ids.len() < 2 {
            return;
        }
        let ids: Vec<ItemId> = self.selected_ids.iter().copied().collect();
        self.apply_manual_band_to(&ids, cx);
    }

    pub(crate) fn apply_manual_to_all(&mut self, cx: &mut Context<Self>) {
        let ids: Vec<ItemId> = self.project.items.iter().map(|item| item.id).collect();
        self.apply_manual_band_to(&ids, cx);
    }

    fn update_active_item(
        &mut self,
        update: impl FnOnce(&mut Item),
        status: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) {
        let Some(id) = self.selected else {
            return;
        };
        let Some(item) = self.project.items.iter_mut().find(|item| item.id == id) else {
            return;
        };
        update(item);
        self.preview_cache.clear();
        self.set_status(status, false, cx);
    }

    pub(crate) fn clear_manual_band(&mut self, cx: &mut Context<Self>) {
        let status = self.language.manual_cleared();
        self.update_active_item(
            |item| {
                item.manual = None;
                item.keep_whole = item.block.is_none();
            },
            status,
            cx,
        );
    }

    pub(crate) fn set_selected_keep_whole(&mut self, keep: bool, cx: &mut Context<Self>) {
        let status = if keep {
            self.language.manual_whole_kept()
        } else {
            self.language.manual_cleared()
        };
        self.update_active_item(
            |item| {
                item.manual = None;
                item.keep_whole = keep || item.block.is_none();
            },
            status,
            cx,
        );
    }

    fn action_remove(&mut self, _: &RemoveSelected, _window: &mut Window, cx: &mut Context<Self>) {
        self.delete_selected_image(cx);
    }

    pub(crate) fn apply_drop(
        &mut self,
        dragged: ItemId,
        before: Option<ItemId>,
        cx: &mut Context<Self>,
    ) {
        if before == Some(dragged) {
            return;
        }
        let Some(from) = self
            .project
            .items
            .iter()
            .position(|item| item.id == dragged)
        else {
            return;
        };
        self.record_undo();
        let item = self.project.items.remove(from);
        let insert_at = match before {
            Some(target) => self
                .project
                .items
                .iter()
                .position(|item| item.id == target)
                .unwrap_or(self.project.items.len()),
            None => self.project.items.len(),
        };
        self.project
            .items
            .insert(insert_at.min(self.project.items.len()), item);
        self.drag_over = None;
        self.preview_cache.clear();
        self.set_status(self.language.reordered(), false, cx);
    }

    pub(crate) fn set_drag_over(&mut self, id: Option<ItemId>, cx: &mut Context<Self>) {
        if self.drag_over != id {
            self.drag_over = id;
            cx.notify();
        }
    }

    // ------------------------------------------------------------------
    // 撤销 / 重做 / 清空
    // ------------------------------------------------------------------

    pub(crate) fn action_undo(
        &mut self,
        _: &UndoEdit,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.undo.undo(&mut self.project) {
            self.after_undo_redo(cx);
            self.set_status(self.language.undone(), false, cx);
        } else {
            self.set_status(self.language.nothing_to_undo(), true, cx);
        }
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

    /// 按 Esc：关掉最上面那一层。
    pub(crate) fn action_dismiss(
        &mut self,
        _: &Dismiss,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dismiss_overlays(cx);
    }

    /// 按回车：把当前选中的第一张放大。
    pub(crate) fn action_zoom_layer(
        &mut self,
        _: &OpenZoomLayer,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.zoom_layer {
            return;
        }
        if let Some(id) = self.selected {
            self.open_zoom_layer(id, cx);
        }
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
        self.crop_edit = None;
        self.manual_drag = None;
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
        if self.project.items.is_empty() {
            return;
        }
        self.record_undo();
        self.project.items.clear();
        self.selected = None;
        self.selected_ids.clear();
        self.crop_edit = None;
        self.manual_drag = None;
        self.drag_over = None;
        self.preview_cache.clear();
        self.set_status(self.language.cleared_all(), false, cx);
    }

    // ------------------------------------------------------------------
    // 缩放
    // ------------------------------------------------------------------

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

    fn action_zoom_reset(&mut self, _: &ZoomReset, _window: &mut Window, cx: &mut Context<Self>) {
        self.zoom = 1.0;
        cx.notify();
    }

    // ------------------------------------------------------------------
    // 预览缓存
    // ------------------------------------------------------------------

    pub(crate) fn preview_for(
        &mut self,
        id: ItemId,
        source: &RgbImage,
        band: CropBand,
        target_width: u32,
    ) -> Arc<RenderImage> {
        let key = (id, band.y0, band.y1, target_width);
        if let Some(image) = self.preview_cache.get(&key) {
            return image.clone();
        }
        let image = render::preview_image(source, band, target_width);
        if self.preview_cache.len() < PREVIEW_CACHE_LIMIT {
            self.preview_cache.insert(key, image.clone());
        }
        image
    }

    // ------------------------------------------------------------------
    // 输出尺寸
    // ------------------------------------------------------------------

    /// 当前结果的理论输出尺寸（宽, 高）。没有图片时返回 None。
    pub(crate) fn output_size(&self) -> Option<(u32, u32)> {
        let inclusions = crop::resolve(&self.project);
        let gap = self.project.settings.gap.effective_px();

        match self.project.mode {
            Mode::Subtitle => {
                let mut parts: Vec<(u32, u32, u32)> = Vec::new(); // (原宽, 高, 裁剪带高)
                for (index, item) in self.project.items.iter().enumerate() {
                    let band_height = match inclusions.get(index).copied() {
                        Some(Inclusion::Crop(band)) => band.height(),
                        Some(Inclusion::FullImage) => item.height,
                        _ => continue,
                    };
                    parts.push((item.width, item.height, band_height));
                }
                if parts.is_empty() {
                    return None;
                }
                let width = parts.iter().map(|(w, _, _)| *w).max().unwrap_or(1).max(1);
                let mut height: u64 = 0;
                for (source_width, _, band_height) in &parts {
                    let scaled =
                        (*band_height as u64 * width as u64) / (*source_width).max(1) as u64;
                    height += scaled.max(1);
                }
                height += gap as u64 * parts.len().saturating_sub(1) as u64;
                Some((width, height.min(u32::MAX as u64) as u32))
            }
            Mode::Layout => {
                let sizes: Vec<(u32, u32)> = self
                    .project
                    .items
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| {
                        matches!(
                            inclusions.get(*index),
                            Some(Inclusion::Crop(_)) | Some(Inclusion::FullImage)
                        )
                    })
                    .map(|(_, item)| item.size())
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
}

impl Render for AppRoot {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_appearance(window.appearance());

        let main = div()
            .relative()
            .flex()
            .flex_1()
            .min_h(px(0.))
            .child(self.render_preview(theme, cx))
            .child(self.render_rail(theme, cx))
            // 浮层和菜单盖在最上面
            .when(self.zoom_layer, |this| {
                this.child(self.render_zoom_layer(theme, cx))
            })
            .when(self.image_menu.is_some(), |this| {
                this.child(self.render_image_menu(theme, cx))
            });

        div()
            .size_full()
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
            .on_action(cx.listener(Self::action_zoom_reset))
            .on_action(cx.listener(Self::action_dismiss))
            .on_action(cx.listener(Self::action_zoom_layer))
            .on_drop(cx.listener(Self::on_file_drop))
            .child(
                div()
                    .relative()
                    .flex()
                    .items_center()
                    // 必须撑满窗口宽度，否则工具栏里的 justify_between 没有多余空间可分，
                    // 右侧的撤销、重做、导出会全挤到左边。
                    .w_full()
                    .child(self.render_toolbar(theme, cx))
                    // 更多菜单挂在工具栏左上角按钮下面
                    .when(self.more_menu_open, |this| {
                        this.child(
                            div()
                                .absolute()
                                .left(space::MD)
                                .top(size::TOOLBAR - px(4.))
                                .child(self.render_more_menu(theme, cx)),
                        )
                    }),
            )
            .child(main)
            .child(self.render_status_bar(theme, cx))
    }
}

// 无头界面测试：需要 gpui 的测试平台（dev-dependencies 里开了 test-support）。
#[cfg(test)]
mod ui_smoke;
