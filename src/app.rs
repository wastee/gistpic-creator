//! 应用主状态：项目数据、图片导入、字幕检测调度、撤销/重做、导出。

pub mod actions;
pub mod keymap;

mod export;
mod undo;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use gpui::prelude::*;
use gpui::{
    div, px, ClipboardEntry, Context, ExternalPaths, FocusHandle, IntoElement,
    PathPromptOptions, Render, RenderImage, SharedString, Window,
};
use image::RgbImage;

use crate::imaging::crop::{self, CropBand, Inclusion};
use crate::imaging::decode::{decode_bytes, decode_path, is_supported_path};
use crate::imaging::detect::{DetectError, SubtitleDetector};
use crate::imaging::render;
use crate::model::{
    Item, ItemId, ItemStatus, LayoutTarget, Mode, Project, Rgba, SearchRegion, SubtitleBlock,
};
use crate::platform;
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
}

impl Default for Status {
    fn default() -> Self {
        Status::info(format!(
            "粘贴图片开始：{} 粘贴，或把图片文件拖进窗口",
            platform::shortcut_label("V")
        ))
    }
}

/// 拖拽排序时携带的数据。
#[derive(Clone, Copy)]
pub struct DragItem {
    pub id: ItemId,
}

pub struct AppRoot {
    pub(crate) project: Project,
    pub(crate) undo: undo::UndoStack,
    pub(crate) detector: Option<Arc<SubtitleDetector>>,
    pub(crate) next_id: ItemId,
    pub(crate) selected: Option<ItemId>,
    pub(crate) zoom: f32,
    pub(crate) preview_cache: HashMap<(ItemId, u32, u32, u32), Arc<RenderImage>>,
    pub(crate) dialog: Option<ItemId>,
    pub(crate) status: Status,
    pub(crate) detecting: bool,
    pub(crate) detect_generation: u64,
    pub(crate) decoding: usize,
    pub(crate) drag_over: Option<ItemId>,
    pub(crate) clip_counter: usize,
    /// 点窗口关闭按钮时：true 最小化到任务栏让程序继续运行，false 直接退出。
    pub(crate) close_to_taskbar: bool,
    focus_handle: FocusHandle,
}

impl AppRoot {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let (detector, status) = match SubtitleDetector::new_embedded() {
            Ok(detector) => (Some(Arc::new(detector)), Status::default()),
            Err(err) => (
                None,
                Status::error(format!("字幕检测模型加载失败：{err}")),
            ),
        };

        let focus_handle = cx.focus_handle();
        window.focus(&focus_handle);

        // 关闭按钮：默认最小化到任务栏，让程序留在后台；关掉设置后直接退出。
        let this = cx.weak_entity();
        window.on_window_should_close(cx, move |window, cx| {
            let Some(root) = this.upgrade() else {
                return true;
            };
            root.update(cx, |root, cx| root.on_close_requested(window, cx))
        });

        AppRoot {
            project: Project::default(),
            undo: undo::UndoStack::new(UNDO_LIMIT),
            detector,
            next_id: 1,
            selected: None,
            zoom: 1.0,
            preview_cache: HashMap::new(),
            dialog: None,
            status,
            detecting: false,
            detect_generation: 0,
            decoding: 0,
            drag_over: None,
            clip_counter: 0,
            close_to_taskbar: true,
            focus_handle,
        }
    }

    // ------------------------------------------------------------------
    // 通用状态
    // ------------------------------------------------------------------

    pub(crate) fn set_status(&mut self, text: impl Into<SharedString>, error: bool, cx: &mut Context<Self>) {
        self.status = Status {
            text: text.into(),
            error,
        };
        cx.notify();
    }

    /// 点窗口关闭按钮时的处理：默认最小化到任务栏，让程序继续运行。
    fn on_close_requested(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.close_to_taskbar {
            window.minimize_window();
        } else {
            cx.quit();
        }
        // 两种情况下都不让窗口真正关闭，最小化模式继续运行，退出模式由 cx.quit 处理。
        false
    }

    pub(crate) fn set_close_to_taskbar(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.close_to_taskbar == enabled {
            return;
        }
        self.close_to_taskbar = enabled;
        cx.notify();
    }

    fn record_undo(&mut self) {
        self.undo.record(&self.project);
    }

    pub(crate) fn zoom_ratio(&self) -> f32 {
        ((self.zoom - 0.5) / 1.5).clamp(0.0, 1.0)
    }

    pub(crate) fn set_zoom_index(&mut self, index: usize) {
        self.zoom = 0.5 + (index as f32 / 20.0) * 1.5;
    }

    fn zoom_step(&mut self, direction: i32, cx: &mut Context<Self>) {
        self.zoom = (self.zoom + direction as f32 * 0.1).clamp(0.5, 2.0);
        cx.notify();
    }

    // ------------------------------------------------------------------
    // 导入
    // ------------------------------------------------------------------

    pub(crate) fn action_paste(&mut self, _: &PasteImage, _window: &mut Window, cx: &mut Context<Self>) {
        self.paste(cx);
    }

    fn paste(&mut self, cx: &mut Context<Self>) {
        let Some(item) = cx.read_from_clipboard() else {
            self.set_status("剪贴板里没有内容", true, cx);
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
                let name = format!("剪贴板图片 {}", self.clip_counter);
                self.import_bytes(name, bytes, cx);
            }
            None => self.set_status("剪贴板里没有图片", true, cx),
        }
    }

    pub(crate) fn action_open(&mut self, _: &OpenImages, _window: &mut Window, cx: &mut Context<Self>) {
        self.open_images(cx);
    }

    fn open_images(&mut self, cx: &mut Context<Self>) {
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some(SharedString::from("选择图片")),
        });
        cx.spawn(async move |this, cx| {
            match receiver.await {
                Ok(Ok(Some(paths))) => {
                    let _ = this.update(cx, |this, cx| this.open_paths(paths, cx));
                }
                Ok(Ok(None)) => {}
                Ok(Err(err)) => {
                    let _ = this.update(cx, |this, cx| {
                        this.set_status(format!("打开文件失败：{err}"), true, cx)
                    });
                }
                Err(_) => {}
            }
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
            self.set_status("拖入的文件里没有支持的图片（PNG / JPEG / WebP / BMP / GIF）", true, cx);
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
                        this.set_status(format!("读取图片失败：{err}"), true, cx);
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
            self.set_status(format!("「{name}」是空图片"), true, cx);
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
        });
        self.selected = Some(id);
        let count = self.project.items.len();
        if self.detector.is_some() {
            self.set_status(format!("已加入第 {count} 张，正在识别字幕…"), false, cx);
            self.start_detection(cx);
        } else {
            self.set_status(
                format!("已加入第 {count} 张（字幕检测不可用，可用排版模式）"),
                false,
                cx,
            );
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

    fn take_pending_item(&mut self) -> Option<(u64, ItemId, Arc<RgbImage>, SearchRegion)> {
        let region = self.project.settings.region;
        let generation = self.detect_generation;
        let item = self
            .project
            .items
            .iter()
            .find(|item| item.status == ItemStatus::Pending)?;
        Some((generation, item.id, item.rgb.clone(), region))
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
                let Some((generation, id, rgb, region)) = next else {
                    break;
                };
                let detector = detector.clone();
                let detected = cx
                    .background_executor()
                    .spawn(async move { detector.detect(&rgb, region) })
                    .await;
                let _ = this.update(cx, |this, cx| {
                    this.apply_detection(generation, id, detected);
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
        generation: u64,
        id: ItemId,
        result: Result<Option<SubtitleBlock>, DetectError>,
    ) {
        if generation != self.detect_generation {
            // 搜索范围已经改过，这次结果作废，图片保持待检测状态，等待重新识别。
            return;
        }
        let Some(item) = self.project.items.iter_mut().find(|item| item.id == id) else {
            return;
        };
        match result {
            Ok(Some(block)) => {
                item.block = Some(block);
                item.status = ItemStatus::Ready;
            }
            Ok(None) => {
                item.block = None;
                item.status = ItemStatus::NoSubtitle;
            }
            Err(err) => {
                item.block = None;
                item.status = ItemStatus::Failed(err.to_string());
            }
        }
    }

    fn on_detection_finished(&mut self, cx: &mut Context<Self>) {
        self.preview_cache.clear();
        self.dialog = self.next_dialog_item();

        let failed = self
            .project
            .items
            .iter()
            .filter(|item| matches!(item.status, ItemStatus::Failed(_)))
            .count();
        let total = self.project.items.len();
        if let Some(id) = self.dialog {
            let name = self
                .project
                .items
                .iter()
                .find(|item| item.id == id)
                .map(|item| item.name.clone())
                .unwrap_or_default();
            self.set_status(format!("「{name}」没找到字幕，请选择怎么处理"), false, cx);
        } else if failed > 0 {
            self.set_status(
                format!("识别完成：{total} 张图片，{failed} 张读取失败"),
                true,
                cx,
            );
        } else if total > 0 {
            self.set_status(format!("识别完成，共 {total} 张图片"), false, cx);
        }
        cx.notify();
    }

    fn next_dialog_item(&self) -> Option<ItemId> {
        if self.project.mode != Mode::Subtitle || !self.project.settings.crop {
            return None;
        }
        self.project
            .items
            .iter()
            .find(|item| item.status == ItemStatus::NoSubtitle && !item.keep_whole)
            .map(|item| item.id)
    }

    pub(crate) fn confirm_dialog(&mut self, keep_whole: bool, cx: &mut Context<Self>) {
        let Some(id) = self.dialog else {
            return;
        };
        self.record_undo();
        if keep_whole {
            if let Some(item) = self.project.items.iter_mut().find(|item| item.id == id) {
                item.keep_whole = true;
                item.status = ItemStatus::Ready;
            }
            self.set_status("已保留这张整图", false, cx);
        } else {
            self.project.items.retain(|item| item.id != id);
            if self.selected == Some(id) {
                self.selected = None;
            }
            self.set_status("已移除这张没有字幕的图片（可以撤销）", false, cx);
        }
        self.dialog = self.next_dialog_item();
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
        self.dialog = if mode == Mode::Subtitle {
            self.next_dialog_item()
        } else {
            None
        };
        self.set_status(format!("已切换到「{}」", mode.label()), false, cx);
    }

    pub(crate) fn set_crop(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.project.settings.crop == enabled {
            return;
        }
        self.record_undo();
        self.project.settings.crop = enabled;
        self.preview_cache.clear();
        self.dialog = self.next_dialog_item();
        self.set_status(
            if enabled {
                "已开启字幕裁剪"
            } else {
                "已关闭字幕裁剪，图片会整张原样保留"
            },
            false,
            cx,
        );
    }

    pub(crate) fn set_region(&mut self, region: SearchRegion, cx: &mut Context<Self>) {
        if self.project.settings.region == region {
            return;
        }
        self.record_undo();
        self.project.settings.region = region;
        self.detect_generation = self.detect_generation.wrapping_add(1);
        self.dialog = None;
        self.preview_cache.clear();
        // 搜索范围影响检测结果，需要重新识别。
        for item in &mut self.project.items {
            if matches!(item.status, ItemStatus::Ready | ItemStatus::NoSubtitle) {
                item.status = ItemStatus::Pending;
                item.block = None;
            }
        }
        self.set_status(
            format!("字幕搜索范围已改为「{}」，正在重新识别…", region.label()),
            false,
            cx,
        );
        self.start_detection(cx);
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
                "会保留字幕的第二行"
            } else {
                "只保留字幕的第一行"
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
                "图片之间会留出缝隙"
            } else {
                "图片之间不留缝"
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
        self.set_status(format!("排列方式：{}", target.label()), false, cx);
    }


    // ------------------------------------------------------------------
    // 选择、删除、排序
    // ------------------------------------------------------------------

    pub(crate) fn select(&mut self, id: ItemId, cx: &mut Context<Self>) {
        self.selected = Some(id);
        cx.notify();
    }

    pub(crate) fn delete_selected_image(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected else {
            return;
        };
        self.record_undo();
        self.project.items.retain(|item| item.id != id);
        self.selected = None;
        self.set_status("已删除选中的图片（可以撤销）", false, cx);
    }

    fn action_remove(&mut self, _: &RemoveSelected, _window: &mut Window, cx: &mut Context<Self>) {
        self.delete_selected_image(cx);
    }

    pub(crate) fn apply_drop(&mut self, dragged: ItemId, before: Option<ItemId>, cx: &mut Context<Self>) {
        if before == Some(dragged) {
            return;
        }
        let Some(from) = self.project.items.iter().position(|item| item.id == dragged) else {
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
        self.project.items.insert(insert_at.min(self.project.items.len()), item);
        self.drag_over = None;
        self.preview_cache.clear();
        self.set_status("已调整图片顺序", false, cx);
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

    pub(crate) fn action_undo(&mut self, _: &UndoEdit, _window: &mut Window, cx: &mut Context<Self>) {
        if self.undo.undo(&mut self.project) {
            self.after_undo_redo(cx);
            self.set_status("已撤销", false, cx);
        } else {
            self.set_status("没有可以撤销的操作", true, cx);
        }
    }

    pub(crate) fn action_redo(&mut self, _: &RedoEdit, _window: &mut Window, cx: &mut Context<Self>) {
        if self.undo.redo(&mut self.project) {
            self.after_undo_redo(cx);
            self.set_status("已重做", false, cx);
        } else {
            self.set_status("没有可以重做的操作", true, cx);
        }
    }

    fn after_undo_redo(&mut self, cx: &mut Context<Self>) {
        if !self
            .project
            .items
            .iter()
            .any(|item| self.selected == Some(item.id))
        {
            self.selected = None;
        }
        self.drag_over = None;
        self.dialog = if self.project.mode == Mode::Subtitle {
            self.next_dialog_item()
        } else {
            None
        };
        self.preview_cache.clear();
        cx.notify();
        self.start_detection(cx);
    }

    pub(crate) fn action_clear(&mut self, _: &ClearAll, _window: &mut Window, cx: &mut Context<Self>) {
        if self.project.items.is_empty() {
            return;
        }
        self.record_undo();
        self.project.items.clear();
        self.selected = None;
        self.dialog = None;
        self.drag_over = None;
        self.preview_cache.clear();
        self.set_status("已清空所有图片（可以撤销）", false, cx);
    }

    fn action_dialog_skip(
        &mut self,
        _: &DialogSkip,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.dialog.is_some() {
            self.confirm_dialog(false, cx);
        }
    }

    // ------------------------------------------------------------------
    // 缩放
    // ------------------------------------------------------------------

    pub(crate) fn action_zoom_in(&mut self, _: &ZoomIn, _window: &mut Window, cx: &mut Context<Self>) {
        self.zoom_step(1, cx);
    }

    pub(crate) fn action_zoom_out(&mut self, _: &ZoomOut, _window: &mut Window, cx: &mut Context<Self>) {
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
                    let scaled = (*band_height as u64 * width as u64) / (*source_width).max(1) as u64;
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

        let base = div()
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
            .on_action(cx.listener(Self::action_dialog_skip))
            .on_action(cx.listener(Self::action_zoom_in))
            .on_action(cx.listener(Self::action_zoom_out))
            .on_action(cx.listener(Self::action_zoom_reset))
            .on_drop(cx.listener(Self::on_file_drop))
            .child(self.render_toolbar(theme, cx))
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h(px(0.))
                    .child(self.render_preview(theme, cx))
                    .child(self.render_side_panel(theme, cx)),
            )
            .child(self.render_status_bar(theme, cx));

        match self.dialog {
            Some(item_id) => base.child(self.render_dialog(item_id, theme, cx)),
            None => base,
        }
    }
}

// 无头界面测试：需要 gpui 的测试平台（dev-dependencies 里开了 test-support）。
#[cfg(test)]
mod ui_smoke;
