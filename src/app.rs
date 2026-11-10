//! 应用主状态：项目数据、图片导入、字幕检测调度、撤销/重做、导出。

pub mod actions;
mod canvas_gestures;
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
    RenderImage, SharedString, Window, div,
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
use crate::ui::theme::Theme;
use crate::ui::tokens;

use actions::*;

/// 预览图统一生成的最大宽度（显示时再按缩放比例拉伸）。
pub(crate) const PREVIEW_TARGET_WIDTH: u32 = 640;
/// 预览缓存最多保留多少张，超出就整体清空。
const PREVIEW_CACHE_LIMIT: usize = 160;
/// 撤销栈最多保留多少步。
const UNDO_LIMIT: usize = 100;
/// 吐司自动消失的时间。
const TOAST_MS: u64 = 2600;

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

/// 裁剪线拖的是上边界还是下边界。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManualEdge {
    Top,
    Bottom,
}

pub(crate) use crate::ui::canvas::SeamAxis;

/// 一次裁剪线拖动中的全部状态。
pub(crate) struct CropDragState {
    pub id: ItemId,
    pub edge: ManualEdge,
    /// 按下时的保留带（归一化比例）
    pub start: ManualBand,
    pub start_pointer_y: f32,
    /// 原图顶边在画布中的固定位置。
    pub source_origin_y: f32,
    pub auto_scroll_pixels: f32,
    pub image_height: u32,
    /// 显示像素 / 原图像素
    pub scale: f32,
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

pub struct AppRoot {
    pub(crate) project: Project,
    pub(crate) undo: undo::UndoStack,
    pub(crate) detector: Option<Arc<SubtitleDetector>>,
    detector_loading: bool,
    pub(crate) next_id: ItemId,
    pub(crate) selected: Option<ItemId>,
    pub(crate) selected_ids: BTreeSet<ItemId>,
    /// 鼠标停在哪个图块上：用于原地透视被裁掉的原图。
    pub(crate) hover_item: Option<ItemId>,
    /// 鼠标停在哪个接缝上：用于显示一条微光。
    pub(crate) hover_seam: Option<usize>,
    /// 正在拖动哪一段的裁剪线。
    pub(crate) crop_drag: Option<CropDragState>,
    /// 正在拖动哪条接缝。
    pub(crate) gap_drag: Option<GapDragState>,
    /// 正在拉框多选。
    pub(crate) marquee: Option<MarqueeDrag>,
    /// 正在拖动的图片；有值时底部胶囊变成删除区。
    pub(crate) dragging: Option<ItemId>,
    /// 拖动时的插入位置：Some(None) 表示插到末尾。
    pub(crate) drag_over: Option<Option<ItemId>>,
    /// 鼠标是否在底部删除区上。
    pub(crate) delete_hover: bool,
    pub(crate) zoom: f32,
    pub(crate) preview_cache: HashMap<(ItemId, u32, u32, u32), Arc<RenderImage>>,
    pub(crate) status: Status,
    /// 吐司是否显示。默认不显示，避免常驻一行提示。
    pub(crate) toast_visible: bool,
    toast_token: usize,
    pub(crate) detecting: bool,
    pub(crate) decoding: usize,
    pub(crate) clip_counter: usize,
    /// 点窗口关闭按钮时：true 隐藏窗口让程序继续在托盘运行，false 直接退出。
    pub(crate) close_to_tray: bool,
    /// 当前界面语言。默认中文，只在本次运行内切换。
    pub(crate) language: Language,
    /// 当前打开的右键卡片。
    pub(crate) menu: Option<MenuState>,
    pub(crate) language_submenu_open: bool,
    /// 画布的滚动位置。
    pub(crate) canvas_scroll: gpui::ScrollHandle,
    #[cfg(test)]
    pub(crate) open_requests: usize,
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
            hover_item: None,
            hover_seam: None,
            crop_drag: None,
            gap_drag: None,
            marquee: None,
            dragging: None,
            drag_over: None,
            delete_hover: false,
            zoom: restored.zoom,
            preview_cache: HashMap::new(),
            status,
            toast_visible,
            toast_token: 0,
            detecting: false,
            decoding: 0,
            clip_counter: 0,
            close_to_tray: restored.close_to_tray,
            language,
            menu: None,
            language_submenu_open: false,
            canvas_scroll: gpui::ScrollHandle::new(),
            #[cfg(test)]
            open_requests: 0,
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
        self.toast_token = self.toast_token.wrapping_add(1);
        let token = self.toast_token;
        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(TOAST_MS))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.toast_token == token {
                    this.toast_visible = false;
                    cx.notify();
                }
            });
        })
        .detach();
        cx.notify();
    }

    pub(crate) fn on_close_requested(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        self.save_state(window);
        if self.close_to_tray {
            window.set_visible(false);
        } else {
            cx.quit();
        }
        false
    }

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
        let Some(band) = crop::band_for_block(block, keep_second, bleed, item.height) else {
            return;
        };
        let Some(manual) = ManualBand::clamped(
            band.y0 as f32 / item.height as f32,
            band.y1 as f32 / item.height as f32,
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
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dismiss_overlays(cx);
    }

    pub(crate) fn dismiss_overlays(&mut self, cx: &mut Context<Self>) {
        if self.menu.is_some() {
            self.close_menu(cx);
        } else if self.crop_drag.is_some() {
            self.end_crop_drag(cx);
        } else if self.gap_drag.is_some() {
            self.end_gap_drag(cx);
        } else if self.marquee.is_some() {
            self.marquee = None;
            cx.notify();
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

    /// 当前作品的输出尺寸。复制成功的吐司里会带上它。
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
            Some(Inclusion::Crop(band)) => item.width as u64 * band.height() as u64 == full,
            Some(Inclusion::FullImage) => true,
            _ => false,
        }
    }

    pub(crate) fn output_size(&self) -> Option<(u32, u32)> {
        let gap = self.project.settings.gap.effective_px();
        let inclusions = crop::resolve(&self.project);
        match self.project.mode {
            Mode::Subtitle => {
                let mut width = 0u32;
                let mut height = 0u64;
                let mut count = 0u64;
                for (index, item) in self.project.items.iter().enumerate() {
                    let band = match inclusions.get(index).copied() {
                        Some(Inclusion::Crop(band)) => band,
                        Some(Inclusion::FullImage) => CropBand {
                            y0: 0,
                            y1: item.height,
                        },
                        _ => continue,
                    };
                    width = width.max(item.width);
                    height += band.height() as u64;
                    count += 1;
                }
                if count == 0 {
                    return None;
                }
                height += gap as u64 * count.saturating_sub(1);
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

    fn action_remove(&mut self, _: &RemoveSelected, _window: &mut Window, cx: &mut Context<Self>) {
        self.delete_selected_images(cx);
    }
}

impl Render for AppRoot {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_appearance(window.appearance());
        let viewport = window.viewport_size();

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
            .on_drop(cx.listener(Self::on_file_drop))
            .child(self.render_canvas(window, theme, cx))
            .child(self.render_capsule(theme, cx))
            .child(self.render_window_controls(theme, cx))
            .when(self.menu.is_some(), |this| {
                this.child(self.render_context_menu(theme, viewport, cx))
            })
    }
}

#[cfg(test)]
mod ui_smoke;
