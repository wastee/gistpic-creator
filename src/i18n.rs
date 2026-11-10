//! 界面语言。默认简体中文，目前支持简体中文和英文。

use crate::model::{LayoutTarget, Mode};

/// 界面语言。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Language {
    #[default]
    ZhCn,
    EnUs,
}

impl Language {
    pub const ALL: [Language; 2] = [Language::ZhCn, Language::EnUs];

    pub fn app_name(self) -> &'static str {
        match self {
            Language::ZhCn => "省流图创造机",
            Language::EnUs => "GistPic Creator",
        }
    }

    /// 语言名称使用该语言自己的写法，方便用户识别。
    pub fn language_name(self) -> &'static str {
        match self {
            Language::ZhCn => "中文",
            Language::EnUs => "English",
        }
    }

    /// 稳定的内部标识，用于界面元素 id，不随语言变化。
    pub fn key(self) -> &'static str {
        match self {
            Language::ZhCn => "zh-cn",
            Language::EnUs => "en-us",
        }
    }

    pub fn is_zh(self) -> bool {
        self == Language::ZhCn
    }

    pub fn mode_label(self, mode: Mode) -> &'static str {
        match (self, mode) {
            (Language::ZhCn, Mode::Subtitle) => "字幕拼接",
            (Language::ZhCn, Mode::Layout) => "排版",
            (Language::EnUs, Mode::Subtitle) => "Subtitle stitch",
            (Language::EnUs, Mode::Layout) => "Layout",
        }
    }

    pub fn layout_label(self, target: LayoutTarget) -> &'static str {
        match target {
            LayoutTarget::Aspect(ratio) => {
                if (ratio - 16.0 / 9.0).abs() < 1e-3 {
                    "16:9"
                } else if (ratio - 9.0 / 16.0).abs() < 1e-3 {
                    "9:16"
                } else if (ratio - 4.0 / 3.0).abs() < 1e-3 {
                    "4:3"
                } else if (ratio - 1.0).abs() < 1e-3 {
                    "1:1"
                } else if self.is_zh() {
                    "自定义比例"
                } else {
                    "Custom ratio"
                }
            }
            LayoutTarget::Row => {
                if self.is_zh() {
                    "横向排列"
                } else {
                    "Row"
                }
            }
            LayoutTarget::Column => {
                if self.is_zh() {
                    "竖向排列"
                } else {
                    "Column"
                }
            }
        }
    }

    // ------------------------------------------------------------------
    // 通用 / 工具栏
    // ------------------------------------------------------------------

    pub fn default_hint(self, shortcut: &str) -> String {
        if self.is_zh() {
            format!("粘贴图片开始：{shortcut} 粘贴，或把图片文件拖进窗口")
        } else {
            format!("Paste to start: press {shortcut}, or drop image files into the window")
        }
    }

    pub fn paste_image(self, shortcut: &str) -> String {
        if self.is_zh() {
            format!("粘贴图片（{shortcut}）")
        } else {
            format!("Paste image ({shortcut})")
        }
    }

    pub fn open_images(self) -> &'static str {
        if self.is_zh() {
            "打开图片"
        } else {
            "Open images"
        }
    }

    pub fn clear_all(self) -> &'static str {
        if self.is_zh() { "清空" } else { "Clear" }
    }

    pub fn undo(self) -> &'static str {
        if self.is_zh() { "撤销" } else { "Undo" }
    }

    pub fn redo(self) -> &'static str {
        if self.is_zh() { "重做" } else { "Redo" }
    }

    pub fn export_png(self) -> &'static str {
        if self.is_zh() {
            "导出 PNG"
        } else {
            "Export PNG"
        }
    }

    pub fn copy_image(self) -> &'static str {
        if self.is_zh() {
            "复制图片"
        } else {
            "Copy image"
        }
    }

    pub fn zoom(self) -> &'static str {
        if self.is_zh() { "缩放" } else { "Zoom" }
    }

    pub fn quit(self) -> &'static str {
        if self.is_zh() { "退出" } else { "Quit" }
    }

    // ------------------------------------------------------------------
    // 侧栏
    // ------------------------------------------------------------------

    pub fn crop_subtitles(self) -> &'static str {
        if self.is_zh() {
            "裁剪字幕"
        } else {
            "Crop subtitles"
        }
    }

    pub fn keep_second_line(self) -> &'static str {
        if self.is_zh() {
            "保留第二行字幕"
        } else {
            "Keep second subtitle line"
        }
    }

    pub fn manual_crop(self) -> &'static str {
        if self.is_zh() {
            "手动框选"
        } else {
            "Manual crop"
        }
    }

    pub fn manual_crop_hint(self) -> &'static str {
        if self.is_zh() {
            "自动识别会看整张图，并优先找下半画面的字幕。识别不准时，拖动预览里的上下两条线框选；按住 Ctrl/Cmd 点击可以选多张，按住 Shift 点击可以选一段。"
        } else {
            "Detection looks at the whole image and prefers subtitles in the lower half. If it is wrong, drag the two lines in the preview. Ctrl/Cmd-click selects multiple images; Shift-click selects a range."
        }
    }

    pub fn manual_edit(self) -> &'static str {
        if self.is_zh() {
            "框选这张"
        } else {
            "Crop this image"
        }
    }

    pub fn manual_done(self) -> &'static str {
        if self.is_zh() { "完成框选" } else { "Done" }
    }

    pub fn manual_position(self, top: u32, bottom: u32) -> String {
        if self.is_zh() {
            format!("上边界 {top}% · 下边界 {bottom}%")
        } else {
            format!("Top {top}% · Bottom {bottom}%")
        }
    }

    pub fn apply_to_selected(self, count: usize) -> String {
        if self.is_zh() {
            format!("应用到选中（{count} 张）")
        } else {
            format!("Apply to selected ({count})")
        }
    }

    pub fn apply_to_all(self) -> &'static str {
        if self.is_zh() {
            "应用到全部"
        } else {
            "Apply to all"
        }
    }

    pub fn clear_manual(self) -> &'static str {
        if self.is_zh() {
            "清除这张的手动框选"
        } else {
            "Clear this image's manual crop"
        }
    }

    pub fn keep_whole_image(self) -> &'static str {
        if self.is_zh() {
            "这张整张保留"
        } else {
            "Keep this image whole"
        }
    }

    pub fn manual_applied(self, count: usize) -> String {
        if self.is_zh() {
            format!("已将框选位置应用到 {count} 张图片")
        } else {
            format!("Applied the crop position to {count} images")
        }
    }

    pub fn manual_cleared(self) -> &'static str {
        if self.is_zh() {
            "已清除手动框选，恢复自动结果"
        } else {
            "Manual crop cleared; automatic result restored"
        }
    }

    pub fn manual_whole_kept(self) -> &'static str {
        if self.is_zh() {
            "已改为整张保留"
        } else {
            "The whole image will be kept"
        }
    }

    pub fn selected_count(self, count: usize) -> String {
        if self.is_zh() {
            format!("已选中 {count} 张图片")
        } else {
            format!("{count} images selected")
        }
    }

    pub fn bleed(self) -> &'static str {
        if self.is_zh() { "出血位" } else { "Bleed" }
    }

    pub fn bleed_hint(self) -> &'static str {
        if self.is_zh() {
            "出血位是字幕上下多留的一点边，避免把字裁掉。"
        } else {
            "Bleed adds a little space above and below the subtitles so they are not cut off."
        }
    }

    pub fn detected_summary(self, detected: usize, missing: usize) -> String {
        if self.is_zh() {
            format!("已识别到字幕 {detected} 张，没找到字幕 {missing} 张")
        } else {
            format!("Subtitles found in {detected} images; none found in {missing}")
        }
    }

    pub fn subtitle_crop_hint(self) -> &'static str {
        if self.is_zh() {
            "没有手动框选的图片才使用自动结果。第一张图保留画面主体：字幕在下方时保留上半部分，字幕在上方时保留下半部分；其余图片只保留字幕那条。"
        } else {
            "Automatic cropping applies only when no manual crop is set. The first image keeps the main picture: the top half when subtitles are at the bottom, or the bottom half when subtitles are at the top. Other images keep only the subtitle band."
        }
    }

    pub fn arrangement(self) -> &'static str {
        if self.is_zh() {
            "排列方式"
        } else {
            "Arrangement"
        }
    }

    pub fn layout_hint(self) -> &'static str {
        if self.is_zh() {
            "按图片宽高自动分组排列，可能不按粘贴顺序；画布紧贴图片，不裁剪、不拉伸。想严格按顺序排，请用下面的横向或竖向排列。"
        } else {
            "Images are grouped by aspect ratio and may not stay in paste order. The canvas fits the images without cropping or stretching. Use row or column to keep strict order."
        }
    }

    pub fn drag_layout_hint(self) -> &'static str {
        if self.is_zh() {
            "可以拖动任意图片到另一张图片上，调整它所在的位置。"
        } else {
            "Drag any image onto another to change its position."
        }
    }

    pub fn image_gap(self) -> &'static str {
        if self.is_zh() {
            "图片缝隙"
        } else {
            "Image gap"
        }
    }

    pub fn add_gap(self) -> &'static str {
        if self.is_zh() {
            "图片之间留缝"
        } else {
            "Add gap between images"
        }
    }

    pub fn gap_width(self) -> &'static str {
        if self.is_zh() {
            "缝隙宽度"
        } else {
            "Gap width"
        }
    }

    pub fn gap_color(self) -> &'static str {
        if self.is_zh() {
            "缝隙颜色"
        } else {
            "Gap color"
        }
    }

    pub fn window(self) -> &'static str {
        if self.is_zh() { "窗口" } else { "Window" }
    }

    pub fn close_to_tray(self) -> &'static str {
        if self.is_zh() {
            "关闭窗口时隐藏到托盘"
        } else {
            "Hide to tray when closing"
        }
    }

    pub fn close_hint(self) -> &'static str {
        if self.is_zh() {
            "开着：点关闭按钮会隐藏窗口，程序继续在托盘运行；关掉：点关闭按钮就退出。点托盘里的“打开主窗口”可以把窗口叫回来。"
        } else {
            "On: the close button hides the window while the app keeps running in the tray. Off: the close button quits. Choose “Open main window” in the tray to show it again."
        }
    }

    pub fn linux_tray_hint(self) -> &'static str {
        if self.is_zh() {
            "Linux 的托盘需要系统支持；GNOME 需要启用 AppIndicator 扩展才能看到图标。"
        } else {
            "Linux needs a tray host; on GNOME, enable the AppIndicator extension to see the icon."
        }
    }

    pub fn selected_image(self) -> &'static str {
        if self.is_zh() {
            "选中的图片"
        } else {
            "Selected image"
        }
    }

    pub fn found_lines(self, lines: usize) -> String {
        if self.is_zh() {
            format!("已找到字幕（{lines} 行）")
        } else {
            format!("Subtitles found ({lines} lines)")
        }
    }

    pub fn found_subtitles(self) -> &'static str {
        if self.is_zh() {
            "已找到字幕"
        } else {
            "Subtitles found"
        }
    }

    pub fn whole_image(self) -> &'static str {
        if self.is_zh() {
            "整张图保留"
        } else {
            "Whole image kept"
        }
    }

    pub fn no_subtitles(self) -> &'static str {
        if self.is_zh() {
            "没找到字幕"
        } else {
            "No subtitles found"
        }
    }

    pub fn detecting(self) -> &'static str {
        if self.is_zh() {
            "正在识别…"
        } else {
            "Detecting…"
        }
    }

    pub fn read_failed(self, err: &str) -> String {
        if self.is_zh() {
            format!("读取失败：{err}")
        } else {
            format!("Failed to read: {err}")
        }
    }

    pub fn delete_this(self) -> &'static str {
        if self.is_zh() {
            "删除这张"
        } else {
            "Delete this image"
        }
    }

    pub fn selection_hint(self) -> &'static str {
        if self.is_zh() {
            "点一张图片查看它；按住 Ctrl/Cmd 点击可以多选，按住 Shift 点击可以选中一段。"
        } else {
            "Click an image to view it. Ctrl/Cmd-click adds or removes an image; Shift-click selects a range."
        }
    }

    pub fn image_count(self, count: usize) -> String {
        if self.is_zh() {
            format!("列表里一共 {count} 张图片")
        } else {
            format!("{count} images in the list")
        }
    }

    pub fn language(self) -> &'static str {
        if self.is_zh() { "语言" } else { "Language" }
    }

    // ------------------------------------------------------------------
    // 状态栏
    // ------------------------------------------------------------------

    pub fn output_size(self, width: u32, height: u32) -> String {
        if self.is_zh() {
            format!("输出 {width}×{height}")
        } else {
            format!("Output {width}×{height}")
        }
    }

    pub fn output_none(self) -> &'static str {
        if self.is_zh() {
            "输出 —"
        } else {
            "Output —"
        }
    }

    pub fn reading_images(self) -> &'static str {
        if self.is_zh() {
            "正在读取图片…"
        } else {
            "Reading images…"
        }
    }

    pub fn detecting_subtitles(self) -> &'static str {
        if self.is_zh() {
            "正在识别字幕…"
        } else {
            "Detecting subtitles…"
        }
    }

    pub fn active_output(self, active: usize, total: usize) -> String {
        if self.is_zh() {
            format!("参与输出 {active}/{total} 张")
        } else {
            format!("{active}/{total} in output")
        }
    }

    // ------------------------------------------------------------------
    // 预览
    // ------------------------------------------------------------------

    pub fn stitched_output(self, output: &str) -> String {
        if self.is_zh() {
            format!("从上到下拼接 · {output}")
        } else {
            format!("Stitched top to bottom · {output}")
        }
    }

    pub fn layout_output(self, target: LayoutTarget, width: u32, height: u32) -> String {
        if self.is_zh() {
            format!("{} · 输出 {width}×{height}", self.layout_label(target))
        } else {
            format!("{} · Output {width}×{height}", self.layout_label(target))
        }
    }

    pub fn drag_insert_before(self) -> &'static str {
        if self.is_zh() {
            "放到目标图片上，插到它前面"
        } else {
            "Drop on a target image to insert before it"
        }
    }

    pub fn drag_reorder(self) -> &'static str {
        if self.is_zh() {
            "拖动排序"
        } else {
            "Drag to reorder"
        }
    }

    pub fn drop_to_end(self) -> &'static str {
        if self.is_zh() {
            "拖到这里，放到最后"
        } else {
            "Drop here to move to the end"
        }
    }

    pub fn empty_title(self) -> &'static str {
        if self.is_zh() {
            "把图片粘贴进来"
        } else {
            "Paste images to start"
        }
    }

    pub fn empty_hint(self) -> &'static str {
        if self.is_zh() {
            "粘贴图片 · 拖入图片文件 · 或点顶部「打开图片」"
        } else {
            "Paste · drop image files · or click “Open images” at the top"
        }
    }

    pub fn supported_formats(self) -> &'static str {
        if self.is_zh() {
            "支持 PNG / JPEG / WebP / BMP / GIF"
        } else {
            "Supports PNG / JPEG / WebP / BMP / GIF"
        }
    }

    // ------------------------------------------------------------------
    // 导入、检测、编辑状态
    // ------------------------------------------------------------------

    pub fn detector_load_failed(self, err: &str) -> String {
        if self.is_zh() {
            format!("字幕检测模型加载失败：{err}")
        } else {
            format!("Failed to load the subtitle detection model: {err}")
        }
    }

    pub fn clipboard_empty(self) -> &'static str {
        if self.is_zh() {
            "剪贴板里没有内容"
        } else {
            "The clipboard is empty"
        }
    }

    pub fn clipboard_no_image(self) -> &'static str {
        if self.is_zh() {
            "剪贴板里没有图片"
        } else {
            "The clipboard contains no image"
        }
    }

    pub fn clipboard_image_name(self, index: usize) -> String {
        if self.is_zh() {
            format!("剪贴板图片 {index}")
        } else {
            format!("Clipboard image {index}")
        }
    }

    pub fn choose_images(self) -> &'static str {
        if self.is_zh() {
            "选择图片"
        } else {
            "Choose images"
        }
    }

    pub fn open_failed(self, err: &str) -> String {
        if self.is_zh() {
            format!("打开文件失败：{err}")
        } else {
            format!("Failed to open files: {err}")
        }
    }

    pub fn unsupported_images(self) -> &'static str {
        if self.is_zh() {
            "拖入的文件里没有支持的图片（PNG / JPEG / WebP / BMP / GIF）"
        } else {
            "The dropped files contain no supported images (PNG / JPEG / WebP / BMP / GIF)"
        }
    }

    pub fn read_image_failed(self, err: &str) -> String {
        if self.is_zh() {
            format!("读取图片失败：{err}")
        } else {
            format!("Failed to read image: {err}")
        }
    }

    pub fn empty_image(self, name: &str) -> String {
        if self.is_zh() {
            format!("「{name}」是空图片")
        } else {
            format!("“{name}” is an empty image")
        }
    }

    pub fn added_detecting(self, count: usize) -> String {
        if self.is_zh() {
            format!("已加入第 {count} 张，正在识别字幕…")
        } else {
            format!("Added image {count}; detecting subtitles…")
        }
    }

    pub fn added_detector_unavailable(self, count: usize) -> String {
        if self.is_zh() {
            format!("已加入第 {count} 张（字幕检测不可用，可用排版模式）")
        } else {
            format!(
                "Added image {count} (subtitle detection unavailable; layout mode is still available)"
            )
        }
    }

    pub fn no_subtitle_kept_whole(self, count: usize) -> String {
        if self.is_zh() {
            format!("有 {count} 张图片没找到字幕，已自动保留整图")
        } else {
            format!("No subtitles found in {count} image(s); they were kept whole")
        }
    }

    pub fn detection_done_with_failures(self, total: usize, failed: usize) -> String {
        if self.is_zh() {
            format!("识别完成：{total} 张图片，{failed} 张读取失败")
        } else {
            format!("Detection finished: {total} images, {failed} failed to read")
        }
    }

    pub fn detection_done(self, total: usize) -> String {
        if self.is_zh() {
            format!("识别完成，共 {total} 张图片")
        } else {
            format!("Detection finished: {total} images")
        }
    }

    pub fn switched_mode(self, mode: Mode) -> String {
        if self.is_zh() {
            format!("已切换到「{}」", self.mode_label(mode))
        } else {
            format!("Switched to “{}”", self.mode_label(mode))
        }
    }

    pub fn crop_enabled(self) -> &'static str {
        if self.is_zh() {
            "已开启字幕裁剪"
        } else {
            "Subtitle cropping enabled"
        }
    }

    pub fn crop_disabled(self) -> &'static str {
        if self.is_zh() {
            "已关闭字幕裁剪，图片会整张原样保留"
        } else {
            "Subtitle cropping disabled; images stay whole"
        }
    }

    pub fn keep_second_enabled(self) -> &'static str {
        if self.is_zh() {
            "会保留字幕的第二行"
        } else {
            "The second subtitle line will be kept"
        }
    }

    pub fn keep_second_disabled(self) -> &'static str {
        if self.is_zh() {
            "只保留字幕的第一行"
        } else {
            "Only the first subtitle line will be kept"
        }
    }

    pub fn gap_enabled(self) -> &'static str {
        if self.is_zh() {
            "图片之间会留出缝隙"
        } else {
            "A gap will be added between images"
        }
    }

    pub fn gap_disabled(self) -> &'static str {
        if self.is_zh() {
            "图片之间不留缝"
        } else {
            "No gap between images"
        }
    }

    pub fn layout_changed(self, target: LayoutTarget) -> String {
        if self.is_zh() {
            format!("排列方式：{}", self.layout_label(target))
        } else {
            format!("Arrangement: {}", self.layout_label(target))
        }
    }

    pub fn deleted_selected(self) -> &'static str {
        if self.is_zh() {
            "已删除选中的图片（可以撤销）"
        } else {
            "Deleted the selected image (undo available)"
        }
    }

    pub fn reordered(self) -> &'static str {
        if self.is_zh() {
            "已调整图片顺序"
        } else {
            "Image order changed"
        }
    }

    pub fn undone(self) -> &'static str {
        if self.is_zh() { "已撤销" } else { "Undone" }
    }

    pub fn nothing_to_undo(self) -> &'static str {
        if self.is_zh() {
            "没有可以撤销的操作"
        } else {
            "Nothing to undo"
        }
    }

    pub fn redone(self) -> &'static str {
        if self.is_zh() { "已重做" } else { "Redone" }
    }

    pub fn nothing_to_redo(self) -> &'static str {
        if self.is_zh() {
            "没有可以重做的操作"
        } else {
            "Nothing to redo"
        }
    }

    pub fn cleared_all(self) -> &'static str {
        if self.is_zh() {
            "已清空所有图片（可以撤销）"
        } else {
            "Cleared all images (undo available)"
        }
    }

    // ------------------------------------------------------------------
    // 导出 / 复制
    // ------------------------------------------------------------------

    pub fn export_kind_subtitle(self) -> &'static str {
        if self.is_zh() { "拼接" } else { "Stitch" }
    }

    pub fn export_kind_layout(self) -> &'static str {
        if self.is_zh() { "排版" } else { "Layout" }
    }

    pub fn export_file_name(self, kind: &str, timestamp: &str) -> String {
        let app_name = self.app_name().replace(' ', "_");
        format!("{app_name}_{kind}_{timestamp}.png")
    }

    pub fn export_busy(self) -> &'static str {
        if self.is_zh() {
            "还在处理图片，请稍等一下再导出"
        } else {
            "Images are still being processed. Try exporting again in a moment."
        }
    }

    pub fn export_no_images(self) -> &'static str {
        if self.is_zh() {
            "没有可导出的图片，先粘贴或打开几张图片"
        } else {
            "No images to export. Paste or open some images first."
        }
    }

    pub fn generating(self) -> &'static str {
        if self.is_zh() {
            "正在生成图片…"
        } else {
            "Generating image…"
        }
    }

    pub fn generate_failed(self, err: &str) -> String {
        if self.is_zh() {
            format!("生成图片失败：{err}")
        } else {
            format!("Failed to generate image: {err}")
        }
    }

    pub fn saved(self, path: &str) -> String {
        if self.is_zh() {
            format!("已保存：{path}")
        } else {
            format!("Saved: {path}")
        }
    }

    pub fn save_failed(self, err: &str) -> String {
        if self.is_zh() {
            format!("保存失败：{err}")
        } else {
            format!("Save failed: {err}")
        }
    }

    pub fn save_cancelled(self) -> &'static str {
        if self.is_zh() {
            "已取消保存"
        } else {
            "Save cancelled"
        }
    }

    pub fn copy_busy(self) -> &'static str {
        if self.is_zh() {
            "还在处理图片，请稍等一下再复制"
        } else {
            "Images are still being processed. Try copying again in a moment."
        }
    }

    pub fn copy_no_images(self) -> &'static str {
        if self.is_zh() {
            "没有可复制的图片，先粘贴或打开几张图片"
        } else {
            "No images to copy. Paste or open some images first."
        }
    }

    pub fn copied(self) -> &'static str {
        if self.is_zh() {
            "已复制到剪贴板"
        } else {
            "Copied to clipboard"
        }
    }

    // ------------------------------------------------------------------
    // 托盘
    // ------------------------------------------------------------------

    pub fn tray_open_window(self) -> &'static str {
        if self.is_zh() {
            "打开主窗口"
        } else {
            "Open main window"
        }
    }

    pub fn tray_quit(self) -> &'static str {
        if self.is_zh() { "退出" } else { "Quit" }
    }

    pub fn tray_no_host(self) -> &'static str {
        if self.is_zh() {
            "看不到托盘图标？Linux 需要桌面提供托盘宿主；GNOME 需要启用 AppIndicator 扩展。也可以点右上角「退出」关闭程序。"
        } else {
            "Tray icon not visible? Linux needs a tray host; on GNOME, enable the AppIndicator extension. You can also click “Quit” at the top right."
        }
    }
}
