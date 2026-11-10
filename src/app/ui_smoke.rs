//! 用 GPUI 的测试平台跑界面：不需要显卡，但会真实创建窗口、执行渲染、
//! 触发粘贴和字幕检测，用来发现只在界面运行时才会出现的问题。

use gpui::{AppContext, ClipboardItem, Image, ImageFormat, TestAppContext};

use super::AppRoot;
use super::actions::{CopyImage, PasteImage};
use crate::i18n::Language;
use crate::model::ItemStatus;

fn sample_png(relative: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    std::fs::read(&path).unwrap_or_else(|err| panic!("读取 {} 失败: {err}", path.display()))
}

fn blank_png() -> Vec<u8> {
    let image = image::RgbImage::from_pixel(640, 360, image::Rgb([250, 250, 250]));
    let mut bytes = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgb8(image)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .expect("生成空白图片失败");
    bytes.into_inner()
}

fn paste_png(cx: &mut gpui::VisualTestContext, bytes: Vec<u8>) {
    cx.write_to_clipboard(ClipboardItem::new_image(&Image::from_bytes(
        ImageFormat::Png,
        bytes,
    )));
    cx.dispatch_action(PasteImage);
    // 解码和字幕检测都在后台线程，等它跑完
    cx.run_until_parked();
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    cx.run_until_parked();
}

#[test]
fn window_opens_and_renders_empty_state() {
    let mut cx = TestAppContext::single();
    let (_view, cx) = cx.add_window_view(AppRoot::new);

    // 窗口缩到最小尺寸、再放大，都应该能正常绘制
    cx.simulate_resize(gpui::size(gpui::px(1240.), gpui::px(720.)));
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    cx.run_until_parked();
    cx.simulate_resize(gpui::size(gpui::px(1600.), gpui::px(1000.)));
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    cx.run_until_parked();
}

#[test]
fn paste_real_screenshot_detects_subtitle_and_produces_output_size() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);

    paste_png(cx, sample_png("示例图片/示例_单行字幕图片/粘贴的图像.png"));

    let (count, detecting, output) = cx.read_entity(&view, |root, _| {
        (root.project.items.len(), root.detecting, root.output_size())
    });
    assert_eq!(count, 1, "应该加入一张图片");
    assert!(!detecting, "检测应该已经结束");
    let (width, height) = output.expect("应该算得出输出尺寸");
    assert_eq!(width, 3840);
    assert!(
        (2000..=2160).contains(&height),
        "第一张保留画面后高度应在合理范围，实际 {height}"
    );

    // 再画一帧，走一遍带图片的预览渲染
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    cx.run_until_parked();
}

#[test]
fn blank_image_is_kept_whole_without_dialog() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);

    paste_png(cx, blank_png());

    let (count, status, keep_whole, block) = cx.read_entity(&view, |root, _| {
        let item = root.project.items.first().expect("应该有一张图片");
        (
            root.project.items.len(),
            item.status.clone(),
            item.keep_whole,
            item.block.is_some(),
        )
    });
    assert_eq!(count, 1, "空白图也应该保留在列表里");
    assert_eq!(status, ItemStatus::NoSubtitle, "空白图应标记为没找到字幕");
    assert!(keep_whole, "没找到字幕时应自动保留整图");
    assert!(!block, "没找到字幕时不应该有字幕块");

    // 再画一帧，确认不会渲染询问弹窗
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    cx.run_until_parked();

    let status_text = cx.read_entity(&view, |root, _| root.status.text.to_string());
    assert!(
        status_text.contains("保留整图"),
        "状态栏应说明已保留整图，实际：{status_text}"
    );
}

#[test]
fn switch_mode_and_render_layout_canvas() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);

    paste_png(cx, sample_png("示例图片/示例_单行字幕图片/粘贴的图像.png"));
    cx.update_entity(&view, |root, cx| {
        root.set_mode(crate::model::Mode::Layout, cx);
    });
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    cx.run_until_parked();

    let (mode, output) = cx.read_entity(&view, |root, _| (root.project.mode, root.output_size()));
    assert_eq!(mode, crate::model::Mode::Layout);
    let (width, height) = output.expect("排版模式应该有输出尺寸");
    // 只有一张图时不强制补边，保持原图尺寸
    assert_eq!((width, height), (3840, 2160));
}

#[test]
fn multiple_images_stack_and_undo_redo_clear() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);

    for relative in [
        "示例图片/示例_多行字幕图片/粘贴的图像.png",
        "示例图片/示例_多行字幕图片/粘贴的图像 (2).png",
        "示例图片/示例_多行字幕图片/粘贴的图像 (3).png",
    ] {
        paste_png(cx, sample_png(relative));
    }

    let (count, output) = cx.read_entity(&view, |root, _| {
        (root.project.items.len(), root.output_size())
    });
    assert_eq!(count, 3);
    let (width, height) = output.expect("应该有输出尺寸");
    assert_eq!(width, 3840);
    assert!(
        height > 2000,
        "三张图拼接结果高度应大于 2000，实际 {height}"
    );

    cx.dispatch_action(super::actions::UndoEdit);
    cx.run_until_parked();
    assert_eq!(
        cx.read_entity(&view, |root, _| root.project.items.len()),
        2,
        "撤销一次应该回到两张图"
    );

    cx.dispatch_action(super::actions::RedoEdit);
    cx.run_until_parked();
    assert_eq!(
        cx.read_entity(&view, |root, _| root.project.items.len()),
        3,
        "重做应该恢复到三张图"
    );

    cx.dispatch_action(super::actions::ClearAll);
    cx.run_until_parked();
    assert_eq!(
        cx.read_entity(&view, |root, _| root.project.items.len()),
        0,
        "清空后应该没有图片"
    );
}

#[test]
fn gap_setting_changes_output_height() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);

    paste_png(cx, sample_png("示例图片/示例_单行字幕图片/粘贴的图像.png"));
    paste_png(
        cx,
        sample_png("示例图片/示例_单行字幕图片/粘贴的图像 (2).png"),
    );

    let before = cx
        .read_entity(&view, |root, _| root.output_size())
        .expect("应该有输出尺寸");

    cx.update_entity(&view, |root, cx| {
        root.set_gap_enabled(true, cx);
        for _ in 0..4 {
            root.step_gap_px(1);
        }
        root.set_gap_color(crate::model::Rgba::WHITE, cx);
    });

    let (after, gap) = cx.read_entity(&view, |root, _| {
        (root.output_size(), root.project.settings.gap)
    });
    let after = after.expect("应该有输出尺寸");
    assert_eq!(before.0, after.0);
    assert!(gap.enabled);
    assert_eq!(gap.px, 16, "默认 8 像素，再加 4 步共 16 像素");
    assert_eq!(
        after.1,
        before.1 + gap.px,
        "两张图之间加 {} 像素缝隙，总高度应增加同样的值",
        gap.px
    );

    // 画一帧，走一遍带缝隙的合成预览
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
}

#[test]
fn layout_mode_hugs_content_and_draws() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);

    for relative in [
        "示例图片/示例_多行字幕图片/粘贴的图像.png",
        "示例图片/示例_多行字幕图片/粘贴的图像 (2).png",
        "示例图片/示例_多行字幕图片/粘贴的图像 (3).png",
    ] {
        paste_png(cx, sample_png(relative));
    }

    cx.update_entity(&view, |root, cx| {
        root.set_mode(crate::model::Mode::Layout, cx);
    });
    let output = cx
        .read_entity(&view, |root, _| root.output_size())
        .expect("应该有输出尺寸");
    // 三张 16:9 的横图排成一列两张 + 一列一张，画布紧贴内容，比例约 2.67，不补边
    let aspect = output.0 as f32 / output.1 as f32;
    assert!(
        (aspect - 2.6667).abs() < 0.02,
        "三张横图自动分列并紧贴内容，实际比例 {aspect}"
    );
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    cx.run_until_parked();
}

#[test]
fn layout_mode_reorders_mixed_sizes_and_draws() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);

    // 直接构造尺寸不一样的图片：粘贴顺序是 横、竖、横、竖
    cx.update_entity(&view, |root, cx| {
        root.project.items = vec![
            synthetic_item(1, 3840, 2160),
            synthetic_item(2, 1080, 1920),
            synthetic_item(3, 3840, 2160),
            synthetic_item(4, 1080, 1920),
        ];
        root.selected = None;
        root.set_mode(crate::model::Mode::Layout, cx);
    });

    let output = cx
        .read_entity(&view, |root, _| root.output_size())
        .expect("应该有输出尺寸");
    let aspect = output.0 as f32 / output.1 as f32;
    assert!(
        (aspect - 2.0137).abs() < 0.03,
        "两横两竖应自动按方向分列、画布紧贴内容，实际比例 {aspect}"
    );

    // 画一帧，检查尺寸不一的图片在预览里能正常排布
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    cx.run_until_parked();
}

#[test]
fn export_writes_png_file() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);

    paste_png(cx, sample_png("示例图片/示例_单行字幕图片/粘贴的图像.png"));
    let expected = cx
        .read_entity(&view, |root, _| root.output_size())
        .expect("应该有输出尺寸");

    let dir = std::env::temp_dir().join(format!("gistpic-creator-export-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("创建临时目录失败");
    let target = dir.join("输出.png");

    cx.dispatch_action(super::actions::ExportPng);
    cx.run_until_parked();
    assert!(cx.did_prompt_for_new_path(), "导出应该弹出保存路径对话框");

    let selected = target.clone();
    cx.simulate_new_path_selection(move |_| Some(selected));
    cx.run_until_parked();

    let status = cx.read_entity(&view, |root, _| root.status.text.clone());
    assert!(target.exists(), "导出文件应该存在，当前状态：{status}");
    let decoded = image::open(&target)
        .expect("导出的 PNG 应该能解码")
        .to_rgb8();
    assert_eq!(decoded.dimensions(), expected);
    assert!(
        status.contains("已保存"),
        "状态栏应提示已保存，实际：{status}"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn manual_crop_export_matches_output_size() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);

    paste_png(cx, sample_png("示例图片/示例_单行字幕图片/粘贴的图像.png"));
    cx.update_entity(&view, |root, cx| {
        let id = root.project.items[0].id;
        root.select(id, cx);
        root.begin_manual_edit(cx);
        root.begin_manual_drag(id, super::ManualEdge::Top, cx);
        root.update_manual_drag(id, super::ManualEdge::Top, 0.30, cx);
        root.end_manual_drag();
        root.begin_manual_drag(id, super::ManualEdge::Bottom, cx);
        root.update_manual_drag(id, super::ManualEdge::Bottom, 0.60, cx);
        root.end_manual_drag();
    });
    let expected = cx
        .read_entity(&view, |root, _| root.output_size())
        .expect("应该有输出尺寸");

    let dir = std::env::temp_dir().join(format!(
        "gistpic-creator-manual-export-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("创建临时目录失败");
    let target = dir.join("手动框选输出.png");

    cx.dispatch_action(super::actions::ExportPng);
    cx.run_until_parked();
    assert!(cx.did_prompt_for_new_path(), "导出应该弹出保存路径对话框");
    let selected = target.clone();
    cx.simulate_new_path_selection(move |_| Some(selected));
    cx.run_until_parked();

    assert!(target.exists(), "导出文件应该存在");
    let decoded = image::open(&target)
        .expect("导出的 PNG 应该能解码")
        .to_rgb8();
    assert_eq!(decoded.dimensions(), expected);

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn keep_second_line_and_bleed_change_output_height() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);

    paste_png(cx, sample_png("示例图片/示例_多行字幕图片/粘贴的图像.png"));
    let base = cx
        .read_entity(&view, |root, _| root.output_size())
        .expect("应该有输出尺寸");

    // 保留第二行字幕：裁剪范围变高
    cx.update_entity(&view, |root, cx| root.set_keep_second(true, cx));
    let with_second = cx
        .read_entity(&view, |root, _| root.output_size())
        .expect("应该有输出尺寸");
    assert!(
        with_second.1 > base.1,
        "保留第二行后高度应变大：{} -> {}",
        base.1,
        with_second.1
    );

    // 出血位加到 30%（滑条第 6 档，每档 5%）：高度继续变大
    cx.update_entity(&view, |root, _| root.set_bleed_index(6));
    let with_bleed = cx
        .read_entity(&view, |root, _| root.output_size())
        .expect("应该有输出尺寸");
    assert!(
        with_bleed.1 > with_second.1,
        "出血位变大后高度应变大：{} -> {}",
        with_second.1,
        with_bleed.1
    );

    // 关掉保留第二行、出血位关到 0：应该比默认 25% 的更矮
    cx.update_entity(&view, |root, cx| {
        root.set_keep_second(false, cx);
        root.set_bleed_index(0);
    });
    let no_bleed = cx
        .read_entity(&view, |root, _| root.output_size())
        .expect("应该有输出尺寸");
    assert!(
        no_bleed.1 < base.1,
        "出血位为 0 时高度应比默认 25% 小：{} -> {}",
        base.1,
        no_bleed.1
    );
}

#[test]
fn drag_reorder_moves_item_before_target_and_to_end() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);

    for relative in [
        "示例图片/示例_多行字幕图片/粘贴的图像.png",
        "示例图片/示例_多行字幕图片/粘贴的图像 (2).png",
        "示例图片/示例_多行字幕图片/粘贴的图像 (3).png",
    ] {
        paste_png(cx, sample_png(relative));
    }

    let ids = cx.read_entity(&view, |root, _| {
        root.project
            .items
            .iter()
            .map(|item| item.id)
            .collect::<Vec<_>>()
    });
    assert_eq!(ids.len(), 3);

    // 把第三张拖到第一张前面
    cx.update_entity(&view, |root, cx| {
        root.apply_drop(ids[2], Some(ids[0]), cx);
    });
    let order = cx.read_entity(&view, |root, _| {
        root.project
            .items
            .iter()
            .map(|item| item.id)
            .collect::<Vec<_>>()
    });
    assert_eq!(order, vec![ids[2], ids[0], ids[1]]);

    // 再把第一张拖到末尾
    cx.update_entity(&view, |root, cx| {
        root.apply_drop(ids[2], None, cx);
    });
    let order = cx.read_entity(&view, |root, _| {
        root.project
            .items
            .iter()
            .map(|item| item.id)
            .collect::<Vec<_>>()
    });
    assert_eq!(order, vec![ids[0], ids[1], ids[2]]);
}

#[test]
fn remove_selected_image_action() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);

    paste_png(cx, sample_png("示例图片/示例_单行字幕图片/粘贴的图像.png"));
    paste_png(
        cx,
        sample_png("示例图片/示例_单行字幕图片/粘贴的图像 (2).png"),
    );

    let selected = cx.read_entity(&view, |root, _| root.selected);
    assert!(selected.is_some(), "粘贴后最后一张应该是选中状态");

    cx.dispatch_action(super::actions::RemoveSelected);
    cx.run_until_parked();
    assert_eq!(
        cx.read_entity(&view, |root, _| root.project.items.len()),
        1,
        "删除选中图片后应该只剩一张"
    );

    // 撤销可以把删掉的图片找回来
    cx.dispatch_action(super::actions::UndoEdit);
    cx.run_until_parked();
    assert_eq!(cx.read_entity(&view, |root, _| root.project.items.len()), 2);
}

/// 生成一张只有尺寸有意义、内容为空的图片，用来测排版。
fn synthetic_item(id: u64, width: u32, height: u32) -> super::Item {
    super::Item {
        id,
        name: format!("合成-{id}.png"),
        width,
        height,
        rgb: std::sync::Arc::new(image::RgbImage::new(
            (width / 8).max(1),
            (height / 8).max(1),
        )),
        block: None,
        status: crate::model::ItemStatus::Ready,
        keep_whole: true,
        manual: None,
    }
}

#[test]
fn switching_language_updates_status_and_renders_english_panel() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);

    let (language, status) =
        cx.read_entity(&view, |root, _| (root.language, root.status.text.clone()));
    assert_eq!(language, Language::ZhCn);
    assert!(status.contains("粘贴图片开始"));

    cx.update(|window, cx| {
        view.update(cx, |root, cx| {
            root.set_language(Language::EnUs, window, cx);
        });
    });
    cx.run_until_parked();
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    cx.run_until_parked();

    let (language, status) =
        cx.read_entity(&view, |root, _| (root.language, root.status.text.clone()));
    assert_eq!(language, Language::EnUs);
    assert!(
        status.contains("Paste to start"),
        "状态栏应切换为英文，实际：{status}"
    );
}

#[test]
fn closing_hides_window_and_showing_brings_it_back() {
    let mut cx = TestAppContext::single();
    let (_view, cx) = cx.add_window_view(AppRoot::new);

    let handle = cx.update(|window, _| window.window_handle());
    let window = handle
        .downcast::<AppRoot>()
        .expect("窗口根视图类型应该是 AppRoot");
    assert!(cx.is_window_visible(handle), "新窗口应该是可见的");

    let should_close = window
        .update(cx, |root, window, cx| root.on_close_requested(window, cx))
        .expect("关闭回调应该执行");
    assert!(!should_close, "关闭回调应该阻止平台销毁窗口");
    assert!(!cx.is_window_visible(handle), "关闭后窗口应该隐藏");

    window
        .update(cx, |_root, window, _cx| window.set_visible(true))
        .expect("重新显示应该成功");
    assert!(cx.is_window_visible(handle), "托盘打开后窗口应该重新可见");
}

#[test]
fn copy_image_puts_png_on_the_clipboard() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);

    paste_png(cx, sample_png("示例图片/示例_单行字幕图片/粘贴的图像.png"));
    cx.dispatch_action(CopyImage);
    cx.run_until_parked();

    let item = cx.read_from_clipboard().expect("复制后剪贴板里应该有内容");
    let image = item
        .entries()
        .iter()
        .find_map(|entry| match entry {
            gpui::ClipboardEntry::Image(image) => Some(image),
            gpui::ClipboardEntry::String(_) => None,
        })
        .expect("复制后剪贴板里应该有图片");
    assert_eq!(image.format(), ImageFormat::Png, "剪贴板图片应该是 PNG");
    assert!(!image.bytes().is_empty(), "剪贴板图片不应该为空");

    let status = cx.read_entity(&view, |root, _| root.status.text.clone());
    assert_eq!(status, Language::ZhCn.copied());
}

#[test]
fn problem_a_bottom_subtitle_is_detected_without_dialog() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);

    paste_png(
        cx,
        sample_png("示例图片/问题A示例_不能正确识别字幕/粘贴的图像 (2).png"),
    );

    let (status, block) = cx.read_entity(&view, |root, _| {
        let item = root.project.items.first().expect("应该有一张图片");
        (item.status.clone(), item.block.clone())
    });
    assert_eq!(status, ItemStatus::Ready, "贴底字幕应该识别成功");
    let block = block.expect("应该有字幕块");
    assert!(
        block.lines[0].y0 > 1700.0,
        "不应选中顶部水印，实际 {:?}",
        block.lines[0]
    );
    assert!(
        block.lines[0].y1 <= 1950.0,
        "字幕框应夹紧到图片底边: {:?}",
        block.lines[0]
    );

    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    cx.run_until_parked();
}

#[test]
fn manual_crop_can_be_applied_to_all_and_undone_in_one_step() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);

    paste_png(cx, sample_png("示例图片/示例_多行字幕图片/粘贴的图像.png"));
    paste_png(
        cx,
        sample_png("示例图片/示例_多行字幕图片/粘贴的图像 (2).png"),
    );

    let ids = cx.read_entity(&view, |root, _| {
        root.project
            .items
            .iter()
            .map(|item| item.id)
            .collect::<Vec<_>>()
    });
    assert_eq!(ids.len(), 2);

    // 把第一张的手动范围改成上半 40% 到 90%，再应用到全部。
    cx.update_entity(&view, |root, cx| {
        root.select(ids[0], cx);
        root.begin_manual_edit(cx);
        root.begin_manual_drag(ids[0], super::ManualEdge::Top, cx);
        root.update_manual_drag(ids[0], super::ManualEdge::Top, 0.40, cx);
        root.end_manual_drag();
        root.begin_manual_drag(ids[0], super::ManualEdge::Bottom, cx);
        root.update_manual_drag(ids[0], super::ManualEdge::Bottom, 0.90, cx);
        root.end_manual_drag();
        root.apply_manual_to_all(cx);
    });

    let manuals = cx.read_entity(&view, |root, _| {
        root.project
            .items
            .iter()
            .map(|item| item.manual)
            .collect::<Vec<_>>()
    });
    assert!(manuals[0].is_some(), "第一张应有手动框选");
    assert_eq!(manuals[0], manuals[1], "应用全部后两张范围应一致");
    let manual = manuals[0].expect("手动范围");
    assert!((manual.top - 0.40).abs() < 0.001);
    assert!((manual.bottom - 0.90).abs() < 0.001);

    // 再画一帧，走一遍框选状态下的预览渲染
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    cx.run_until_parked();

    // 应用全部只记一个撤销点：撤销后第一张保留自己的手动值，第二张恢复自动。
    cx.dispatch_action(super::actions::UndoEdit);
    cx.run_until_parked();
    let after_undo = cx.read_entity(&view, |root, _| {
        root.project
            .items
            .iter()
            .map(|item| item.manual)
            .collect::<Vec<_>>()
    });
    assert!(after_undo[0].is_some());
    assert!(after_undo[1].is_none());
}

#[test]
fn multi_select_can_apply_a_crop_to_only_some_images() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);

    for relative in [
        "示例图片/示例_多行字幕图片/粘贴的图像.png",
        "示例图片/示例_多行字幕图片/粘贴的图像 (2).png",
        "示例图片/示例_多行字幕图片/粘贴的图像 (3).png",
    ] {
        paste_png(cx, sample_png(relative));
    }

    let ids = cx.read_entity(&view, |root, _| {
        root.project
            .items
            .iter()
            .map(|item| item.id)
            .collect::<Vec<_>>()
    });
    assert_eq!(ids.len(), 3);

    // 先把第一张改成 40% 到 90%，再加上第三张；主选中应仍是第一张。
    cx.update_entity(&view, |root, cx| {
        root.select(ids[0], cx);
        root.begin_manual_edit(cx);
        root.begin_manual_drag(ids[0], super::ManualEdge::Top, cx);
        root.update_manual_drag(ids[0], super::ManualEdge::Top, 0.40, cx);
        root.end_manual_drag();
        root.begin_manual_drag(ids[0], super::ManualEdge::Bottom, cx);
        root.update_manual_drag(ids[0], super::ManualEdge::Bottom, 0.90, cx);
        root.end_manual_drag();
        root.select_with_modifiers(
            ids[2],
            gpui::Modifiers {
                control: true,
                ..Default::default()
            },
            cx,
        );
        root.apply_manual_to_selected(cx);
    });

    let derived = cx.read_entity(&view, |root, _| {
        (
            root.selected_count(),
            root.selected,
            root.project
                .items
                .iter()
                .map(|item| (item.id, item.manual))
                .collect::<Vec<_>>(),
        )
    });
    assert_eq!(derived.0, 2, "应只选中两张图片");
    assert_eq!(
        derived.1,
        Some(ids[0]),
        "主选中应保持第一张，批量应用以它为来源"
    );
    let manuals: std::collections::HashMap<_, _> = derived.2.into_iter().collect();
    assert!(manuals[&ids[0]].is_some(), "第一张应被应用手动范围");
    assert!(manuals[&ids[1]].is_none(), "未选中的第二张不应被修改");
    assert!(manuals[&ids[2]].is_some(), "第三张应被应用手动范围");
    assert_eq!(manuals[&ids[0]], manuals[&ids[2]]);
    let applied = manuals[&ids[0]].expect("第一张的手动范围");
    assert!((applied.top - 0.40).abs() < 0.001);
    assert!((applied.bottom - 0.90).abs() < 0.001);

    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    cx.run_until_parked();
}

#[test]
fn manual_crop_overrides_first_image_subject_rule_in_output() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);

    paste_png(cx, sample_png("示例图片/示例_单行字幕图片/粘贴的图像.png"));
    let base = cx
        .read_entity(&view, |root, _| root.output_size())
        .expect("自动结果应有输出尺寸");
    assert!(base.1 > 1900, "第一张自动裁剪应保留大半画面");

    let id = cx.read_entity(&view, |root, _| root.selected.expect("应选中图片"));
    cx.update_entity(&view, |root, cx| {
        root.begin_manual_edit(cx);
        root.set_selected_keep_whole(false, cx);
        root.clear_manual_band(cx);
        // 直接设成一条很窄的范围，验证输出严格跟随手动值。
        root.begin_manual_drag(id, super::ManualEdge::Top, cx);
        root.update_manual_drag(id, super::ManualEdge::Top, 0.50, cx);
        root.end_manual_drag();
        root.begin_manual_drag(id, super::ManualEdge::Bottom, cx);
        root.update_manual_drag(id, super::ManualEdge::Bottom, 0.60, cx);
        root.end_manual_drag();
    });

    let output = cx
        .read_entity(&view, |root, _| root.output_size())
        .expect("手动裁剪应有输出尺寸");
    assert_eq!(output.0, 3840);
    assert!(
        (210..=230).contains(&output.1),
        "手动范围 50% 到 60% 应输出约 216 像素高，实际 {}",
        output.1
    );

    cx.update_entity(&view, |root, cx| {
        root.clear_manual_band(cx);
    });
    let restored = cx
        .read_entity(&view, |root, _| root.output_size())
        .expect("清除手动后应有输出尺寸");
    assert_eq!(restored, base, "清除手动框选后应恢复自动结果");
}

#[test]
fn dragging_manual_handle_changes_the_band() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);

    paste_png(cx, sample_png("示例图片/示例_单行字幕图片/粘贴的图像.png"));
    cx.update_entity(&view, |root, cx| {
        root.begin_manual_edit(cx);
    });
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    cx.run_until_parked();

    let handle = cx
        .debug_bounds("manual-top-1")
        .expect("框选状态应画出上边界手柄");
    let start = handle.center();
    let end = gpui::point(start.x, start.y + gpui::px(30.));
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    cx.simulate_mouse_move(
        end,
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    let end2 = gpui::point(end.x, end.y + gpui::px(20.));
    cx.simulate_mouse_move(
        end2,
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    cx.simulate_mouse_up(end2, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();

    let manual = cx.read_entity(&view, |root, _| {
        root.project.items[0].manual.expect("拖动后应产生手动范围")
    });
    assert!(
        manual.top > 0.02,
        "上边界应被明显向下拖动，实际 top={}",
        manual.top
    );
    assert!(manual.bottom > manual.top);

    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    cx.run_until_parked();
}
