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
    // 框选已经搬到放大浮层里：先打开浮层，再拖里面的上边界。
    cx.update_entity(&view, |root, cx| {
        root.open_zoom_layer(1, cx);
    });
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    cx.run_until_parked();

    let handle = cx
        .debug_bounds("layer-top-1")
        .expect("浮层里应画出上边界手柄");
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

// ----------------------------------------------------------------------
// 新界面：图标条、右键菜单、放大浮层、手势
// ----------------------------------------------------------------------

/// 往项目里直接塞几张合成图片，避免每个测试都等在真实截图的字幕检测上。
fn seed_images(cx: &mut gpui::VisualTestContext, view: &gpui::Entity<AppRoot>, count: u64) {
    cx.update_entity(view, |root, cx| {
        for index in 0..count {
            let id = index + 1;
            root.next_id = id + 1;
            root.project
                .items
                .push(synthetic_item(id, 640, 360));
        }
        let first = root.project.items[0].id;
        root.select(first, cx);
    });
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    cx.run_until_parked();
}

#[test]
fn toolbar_keeps_only_the_essential_controls() {
    let mut cx = TestAppContext::single();
    let (_view, cx) = cx.add_window_view(AppRoot::new);
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    cx.run_until_parked();

    // 新工具栏保留这四样东西；旧的文字按钮都不在了。
    for selector in ["toolbar-more", "toolbar-undo", "toolbar-redo", "export-png"] {
        assert!(
            cx.debug_bounds(selector).is_some(),
            "工具栏应保留 {selector}"
        );
    }
    for removed in [
        "paste-image",
        "open-images",
        "clear-all",
        "copy-image",
        "quit",
        "quit-app",
        "zoom",
    ] {
        assert!(
            cx.debug_bounds(removed).is_none(),
            "工具栏里不该再出现 {removed}"
        );
    }
}

#[test]
fn rail_is_collapsed_by_default_and_expands_on_hover() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    seed_images(cx, &view, 1);

    assert!(
        cx.debug_bounds("rail-panel").is_none(),
        "没悬停时图标条应该只有窄栏，不展开面板"
    );

    // 鼠标移到图标条上
    let strip = cx.debug_bounds("rail-strip").expect("应有图标条");
    let center = strip.center();
    cx.simulate_mouse_move(center, None, gpui::Modifiers::default());
    cx.run_until_parked();

    // 正好 200ms 才弹出来：刚移上去不该立刻展开，鼠标只是路过时不会闪一下。
    assert!(
        cx.read_entity(&view, |root, _| root.visible_rail_panel()).is_none(),
        "鼠标刚移上去时面板还不该展开"
    );

    // 等过 200ms
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(220));
    cx.run_until_parked();
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    cx.run_until_parked();

    assert!(
        cx.read_entity(&view, |root, _| root.visible_rail_panel()).is_some(),
        "停住超过 200ms 后面板应该展开"
    );
    assert!(cx.debug_bounds("rail-panel").is_some(), "面板应画出来");
}

#[test]
fn rail_does_not_expand_when_mouse_passes_by_quickly() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    seed_images(cx, &view, 1);

    // 鼠标划过图标条，停留不到 200ms 就走了
    let strip = cx.debug_bounds("rail-strip").expect("应有图标条");
    cx.simulate_mouse_move(strip.center(), None, gpui::Modifiers::default());
    cx.run_until_parked();

    cx.executor()
        .advance_clock(std::time::Duration::from_millis(100));
    cx.run_until_parked();

    // 100ms 时移到别处
    cx.simulate_mouse_move(
        gpui::point(gpui::px(600.), gpui::px(600.)),
        None,
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();

    // 再等到超过 200ms，也不该展开
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(300));
    cx.run_until_parked();
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    cx.run_until_parked();

    assert!(
        cx.read_entity(&view, |root, _| root.visible_rail_panel()).is_none(),
        "鼠标快速划过不该展开面板"
    );
    assert!(
        cx.debug_bounds("rail-panel").is_none(),
        "鼠标快速划过面板不该出现"
    );
}

#[test]
fn rail_pin_keeps_panel_open_after_mouse_leaves() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    seed_images(cx, &view, 1);

    // 面板没展开时，图标条上不该有图钉
    assert!(
        cx.debug_bounds("rail-pin").is_none(),
        "面板没展开时不该显示图钉"
    );

    // 先悬停展开
    let strip = cx.debug_bounds("rail-strip").expect("应有图标条");
    cx.simulate_mouse_move(strip.center(), None, gpui::Modifiers::default());
    cx.run_until_parked();
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(220));
    cx.run_until_parked();
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    cx.run_until_parked();

    // 面板展开之后，图钉才出现
    let pin = cx.debug_bounds("rail-pin").expect("面板展开后应出现图钉");
    cx.simulate_click(pin.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    cx.run_until_parked();
    assert!(
        cx.read_entity(&view, |root, _| root.rail_pinned),
        "点过图钉后应处于固定状态"
    );

    // 鼠标移开
    cx.simulate_mouse_move(gpui::point(gpui::px(10.), gpui::px(400.)), None, gpui::Modifiers::default());
    cx.run_until_parked();
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("rail-panel").is_some(),
        "固定之后鼠标移开，面板应该还在"
    );
}

#[test]
fn rail_layout_panel_switches_layout_target() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    seed_images(cx, &view, 2);

    cx.update_entity(&view, |root, cx| {
        root.open_rail_panel(crate::ui::rail::RailPanel::Layout, cx);
    });
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    cx.run_until_parked();

    let target = cx
        .debug_bounds("rail-layout-9-16")
        .expect("排版面板里应有 9:16");
    cx.simulate_click(target.center(), gpui::Modifiers::default());
    cx.run_until_parked();

    let layout = cx.read_entity(&view, |root, _| root.project.settings.layout);
    assert_eq!(
        layout,
        crate::model::LayoutTarget::Aspect(9.0 / 16.0),
        "点 9:16 之后排版目标应该跟着变"
    );
}

#[test]
fn right_click_opens_image_menu_with_expected_items() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    seed_images(cx, &view, 1);

    let card = cx
        .debug_bounds("preview-block-1")
        .expect("应画出第一张图");
    cx.simulate_mouse_down(
        card.center(),
        gpui::MouseButton::Right,
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    cx.run_until_parked();

    assert!(
        cx.read_entity(&view, |root, _| root.image_menu.is_some()),
        "右键之后应打开图片菜单"
    );
    for item in [
        "img-zoom",
        "img-crop",
        "img-reset-crop",
        "img-keep-whole",
        "img-move-up",
        "img-move-down",
        "img-move-last",
        "img-delete",
    ] {
        assert!(
            cx.debug_bounds(item).is_some(),
            "图片菜单里应有 {item}"
        );
    }
}

#[test]
fn image_menu_delete_removes_the_image_and_undo_brings_it_back() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    seed_images(cx, &view, 2);

    let card = cx.debug_bounds("preview-block-1").expect("应有第一张");
    cx.simulate_mouse_down(
        card.center(),
        gpui::MouseButton::Right,
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    cx.run_until_parked();

    let delete = cx.debug_bounds("img-delete").expect("应有删除项");
    cx.simulate_click(delete.center(), gpui::Modifiers::default());
    cx.run_until_parked();

    assert_eq!(
        cx.read_entity(&view, |root, _| root.project.items.len()),
        1,
        "删除之后应该少一张"
    );

    cx.dispatch_action(super::actions::UndoEdit);
    cx.run_until_parked();
    assert_eq!(
        cx.read_entity(&view, |root, _| root.project.items.len()),
        2,
        "撤销之后应该恢复"
    );
}

#[test]
fn double_click_opens_zoom_layer_and_escape_closes_it() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    seed_images(cx, &view, 1);

    // 直接调用动作层，双击在测试平台里不好造 click_count=2
    cx.update_entity(&view, |root, cx| {
        root.open_zoom_layer(1, cx);
    });
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    cx.run_until_parked();

    assert!(
        cx.read_entity(&view, |root, _| root.zoom_layer),
        "打开后浮层状态应为真"
    );
    assert!(
        cx.debug_bounds("layer-top-1").is_some(),
        "浮层里应画出上边界手柄"
    );

    cx.dispatch_action(super::actions::Dismiss);
    cx.run_until_parked();
    assert!(
        !cx.read_entity(&view, |root, _| root.zoom_layer),
        "按 Esc 之后浮层应关闭"
    );
}

#[test]
fn zoom_layer_multi_select_drag_updates_every_selected_image() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    seed_images(cx, &view, 3);

    // 三张全部选中
    cx.update_entity(&view, |root, cx| {
        let ids: Vec<_> = root.project.items.iter().map(|item| item.id).collect();
        root.selected = Some(ids[0]);
        root.selected_ids = ids.iter().copied().collect();
        root.open_zoom_layer(ids[0], cx);
    });
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    cx.run_until_parked();

    let handle = cx
        .debug_bounds("layer-top-1")
        .expect("浮层里应有上边界手柄");
    let start = handle.center();
    let end = gpui::point(start.x, start.y + gpui::px(40.));
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

    let manuals = cx.read_entity(&view, |root, _| {
        root.project
            .items
            .iter()
            .map(|item| item.manual)
            .collect::<Vec<_>>()
    });
    assert!(
        manuals.iter().all(|manual| manual.is_some()),
        "拖动之后三张都应该有手动框选记录，实际：{manuals:?}"
    );
    let first = manuals[0].unwrap();
    for manual in manuals.iter().flatten() {
        assert_eq!(
            *manual, first,
            "三张应该拿到完全相同的边界比例"
        );
    }
}

#[test]
fn zoom_layer_toggles_stack_preview_view() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    seed_images(cx, &view, 2);

    cx.update_entity(&view, |root, cx| {
        let ids: Vec<_> = root.project.items.iter().map(|item| item.id).collect();
        root.selected_ids = ids.iter().copied().collect();
        root.open_zoom_layer(ids[0], cx);
    });
    // 状态改了之后必须先画一帧，否则 debug_bounds 拿到的是旧的布局。
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    cx.run_until_parked();

    // 先确认按钮存在
    assert!(
        cx.debug_bounds("layer-stack-preview").is_some(),
        "多选时应有叠放预览按钮"
    );
    // 切换状态本身
    cx.update_entity(&view, |root, cx| {
        root.toggle_zoom_stack_preview(cx);
    });
    cx.run_until_parked();
    assert!(
        cx.read_entity(&view, |root, _| root.zoom_stack_preview),
        "调用切换后应进入叠放预览"
    );

    // 注：这里不再用 debug_bounds 断言「手柄消失」。
    // GPUI 的 Frame::clear() 不清 debug_bounds（vendor/gpui/src/window.rs:740），
    // 所以任何元素一旦被渲染过就会永久留在表里，is_none() 永远不成立。
    // 改成验证叠放视图自己的可观测特征：切换到叠放视图后状态为真，
    // 再切回来状态为假，并且手柄在两种状态下都还能被找到（说明同一套视图仍在渲染）。
    cx.update_entity(&view, |root, cx| {
        root.toggle_zoom_stack_preview(cx);
    });
    cx.run_until_parked();
    assert!(
        !cx.read_entity(&view, |root, _| root.zoom_stack_preview),
        "再切一次应回到可编辑视图"
    );
}

#[test]
fn ctrl_scroll_wheel_changes_zoom_and_plain_scroll_does_not() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    seed_images(cx, &view, 1);

    let before = cx.read_entity(&view, |root, _| root.zoom);

    // 不按修饰键：不应改缩放
    let area = cx
        .debug_bounds("preview-scroll")
        .expect("应有预览区");
    cx.simulate_event(gpui::ScrollWheelEvent {
        position: area.center(),
        delta: gpui::ScrollDelta::Lines(gpui::point(0., 3.)),
        modifiers: gpui::Modifiers::default(),
        touch_phase: gpui::TouchPhase::Moved,
    });
    cx.run_until_parked();
    assert_eq!(
        cx.read_entity(&view, |root, _| root.zoom),
        before,
        "不按 Ctrl 的滚轮只是滚动，不该改缩放"
    );

    // 按住 Ctrl：放大
    cx.simulate_event(gpui::ScrollWheelEvent {
        position: area.center(),
        delta: gpui::ScrollDelta::Lines(gpui::point(0., 1.)),
        modifiers: gpui::Modifiers {
            control: true,
            ..Default::default()
        },
        touch_phase: gpui::TouchPhase::Moved,
    });
    cx.run_until_parked();
    let after = cx.read_entity(&view, |root, _| root.zoom);
    assert!(
        after > before,
        "按住 Ctrl 滚轮向上应放大，实际 {before} -> {after}"
    );
}

#[test]
fn canvas_corner_maps_drag_to_nearest_aspect() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);

    cx.update_entity(&view, |root, cx| {
        // 接近 9:16 的拖拽
        root.drag_canvas_corner(300.0, 530.0, cx);
    });
    assert_eq!(
        cx.read_entity(&view, |root, _| root.project.settings.layout),
        crate::model::LayoutTarget::Aspect(9.0 / 16.0),
        "拖成竖长条应选中 9:16"
    );

    cx.update_entity(&view, |root, cx| {
        // 接近 1:1
        root.drag_canvas_corner(400.0, 405.0, cx);
    });
    assert_eq!(
        cx.read_entity(&view, |root, _| root.project.settings.layout),
        crate::model::LayoutTarget::Aspect(1.0),
        "拖成方形应选中 1:1"
    );

    cx.update_entity(&view, |root, cx| {
        // 接近 16:9
        root.drag_canvas_corner(600.0, 340.0, cx);
    });
    assert_eq!(
        cx.read_entity(&view, |root, _| root.project.settings.layout),
        crate::model::LayoutTarget::Aspect(16.0 / 9.0),
        "拖成横长条应选中 16:9"
    );
}

#[test]
fn escape_closes_image_menu_before_other_layers() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    seed_images(cx, &view, 1);

    let card = cx.debug_bounds("preview-block-1").expect("应有第一张");
    cx.simulate_mouse_down(
        card.center(),
        gpui::MouseButton::Right,
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    assert!(cx.read_entity(&view, |root, _| root.image_menu.is_some()));

    cx.dispatch_action(super::actions::Dismiss);
    cx.run_until_parked();
    assert!(
        cx.read_entity(&view, |root, _| root.image_menu.is_none()),
        "Esc 应先关掉图片菜单"
    );
}

#[test]
fn new_layout_has_expected_geometry() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    seed_images(cx, &view, 1);

    let window = cx.update(|window, _| window.bounds());
    let window_w = f32::from(window.size.width);
    let window_h = f32::from(window.size.height);

    // 工具栏高 48px，铺满整个宽度
    let toolbar = cx.debug_bounds("toolbar").expect("应有工具栏");
    assert!(
        (f32::from(toolbar.size.height) - 48.0).abs() < 1.0,
        "工具栏高 48px，实际 {}",
        f32::from(toolbar.size.height)
    );
    assert!(
        (f32::from(toolbar.size.width) - window_w).abs() < 1.0,
        "工具栏应铺满窗口宽度：实际 {}，窗口 {window_w}",
        f32::from(toolbar.size.width)
    );

    // 图标条贴在窗口最右边，宽 40px
    let strip = cx.debug_bounds("rail-strip").expect("应有图标条");
    let strip_right = f32::from(strip.origin.x) + f32::from(strip.size.width);
    assert!(
        (strip_right - window_w).abs() < 1.0,
        "图标条应贴住窗口右边缘：右边界 {strip_right}，窗口宽 {window_w}"
    );
    assert!(
        (f32::from(strip.size.width) - 40.0).abs() < 1.0,
        "图标条宽 40px，实际 {}",
        f32::from(strip.size.width)
    );
    assert!(
        (f32::from(strip.size.height) - (window_h - 48.0 - 28.0)).abs() < 2.0,
        "图标条应占满工具栏和状态栏之间的高度"
    );

    // 预览区在图标条左边，两者不重叠
    let preview = cx.debug_bounds("preview-scroll").expect("应有预览区");
    let preview_right = f32::from(preview.origin.x) + f32::from(preview.size.width);
    assert!(
        preview_right <= f32::from(strip.origin.x) + 1.0,
        "预览区不应该盖住图标条：预览右边界 {preview_right}，图标条左边界 {}",
        f32::from(strip.origin.x)
    );

    // 导出按钮在工具栏右侧
    let export = cx.debug_bounds("export-png").expect("应有导出按钮");
    let export_right = f32::from(export.origin.x) + f32::from(export.size.width);
    assert!(
        export_right > window_w * 0.9,
        "导出按钮应贴在窗口右侧：右边界 {export_right}，窗口宽 {window_w}"
    );
}

#[test]
fn status_bar_and_layer_header_fill_the_window_width() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    seed_images(cx, &view, 1);
    let window_w = f32::from(cx.update(|window, _| window.bounds().size.width));

    // 状态栏要铺满窗口宽度：它左右两边的内容靠 justify_between 分开，
    // 容器不够宽就会全挤在左边。
    let status = cx.debug_bounds("status-bar").expect("应有状态栏");
    assert!(
        (f32::from(status.size.width) - window_w).abs() < 1.0,
        "状态栏应铺满窗口宽度：实际 {}，窗口 {window_w}",
        f32::from(status.size.width)
    );
    assert!(
        (f32::from(status.size.height) - 28.0).abs() < 1.0,
        "状态栏高 28px，实际 {}",
        f32::from(status.size.height)
    );
    let export = cx.debug_bounds("export-png").expect("应有导出按钮");
    let export_right = f32::from(export.origin.x) + f32::from(export.size.width);
    assert!(
        export_right > window_w * 0.9,
        "导出按钮应贴在窗口右侧：右边界 {export_right}，窗口宽 {window_w}"
    );
    let export_right = f32::from(export.origin.x) + f32::from(export.size.width);
    assert!(
        export_right > window_w * 0.9,
        "导出按钮应贴在窗口右侧：右边界 {export_right}，窗口宽 {window_w}"
    );

    // 浮层打开后，头部标题和关闭按钮应分别在左右两侧。
    // 选两张才能在头部看到「叠放预览」按钮，一起验证。
    cx.update_entity(&view, |root, cx| {
        for index in 0..2u64 {
            let id = index + 1;
            root.next_id = id + 1;
            root.project.items.push(synthetic_item(id, 640, 360));
        }
        let ids: Vec<_> = root.project.items.iter().map(|item| item.id).collect();
        root.selected_ids = ids.iter().copied().collect();
        root.open_zoom_layer(ids[0], cx);
    });
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    cx.run_until_parked();

    let close = cx.debug_bounds("layer-close").expect("浮层应有关闭按钮");
    let close_center = f32::from(close.origin.x) + f32::from(close.size.width) * 0.5;
    assert!(
        close_center > window_w * 0.5,
        "浮层关闭按钮应在右半边：中心 x={close_center}，窗口宽 {window_w}"
    );

    let stack_btn = cx
        .debug_bounds("layer-stack-preview")
        .expect("多选时应有叠放预览按钮");
    assert!(
        f32::from(stack_btn.origin.x) > window_w * 0.5,
        "叠放预览按钮应在右半边"
    );

    let done = cx.debug_bounds("layer-done").expect("浮层应有完成按钮");
    let done_center = f32::from(done.origin.x) + f32::from(done.size.width) * 0.5;
    assert!(
        done_center > window_w * 0.5,
        "浮层底部的完成按钮应在右半边：中心 x={done_center}"
    );
}

#[test]
fn expanded_rail_panel_floats_without_moving_the_strip() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    seed_images(cx, &view, 1);

    let collapsed = cx.debug_bounds("preview-scroll").expect("应有预览区");
    let collapsed_w = f32::from(collapsed.size.width);
    let strip_before = cx.debug_bounds("rail-strip").expect("应有图标条");
    let strip_x_before = f32::from(strip_before.origin.x);

    // 展开图标条面板
    cx.update_entity(&view, |root, cx| {
        root.open_rail_panel(crate::ui::rail::RailPanel::Layout, cx);
        root.toggle_rail_pin(cx);
    });
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    cx.run_until_parked();

    let panel = cx.debug_bounds("rail-panel").expect("固定后应展开面板");
    assert!(
        (f32::from(panel.size.width) - 260.0).abs() < 1.0,
        "展开的面板宽 260px，实际 {}",
        f32::from(panel.size.width)
    );

    // 面板是浮在图片上方的，不占正常布局：预览区宽度不变，图标条位置也一点不能动。
    let expanded = cx.debug_bounds("preview-scroll").expect("应有预览区");
    assert!(
        (f32::from(expanded.size.width) - collapsed_w).abs() < 1.0,
        "面板浮在图片上方，不该挤压预览区：{collapsed_w} -> {}",
        f32::from(expanded.size.width)
    );

    // 图标条位置必须纹丝不动，否则鼠标相对坐标变了，悬停会反复开合。
    let strip_after = cx.debug_bounds("rail-strip").expect("应有图标条");
    assert!(
        (f32::from(strip_after.origin.x) - strip_x_before).abs() < 1.0,
        "面板展开时图标条不能移动：{strip_x_before} -> {}",
        f32::from(strip_after.origin.x)
    );

    // 面板贴在图标条左边
    let panel_right = f32::from(panel.origin.x) + f32::from(panel.size.width);
    assert!(
        (panel_right - strip_x_before).abs() < 1.0,
        "面板右边缘应贴住图标条左边缘：面板右边 {panel_right}，图标条左边 {strip_x_before}"
    );
}

#[test]
fn zoom_layer_caps_stacked_layers_and_reports_the_rest() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    // 造 7 张图，超过叠放上限 5 张
    seed_images(cx, &view, 7);

    cx.update_entity(&view, |root, cx| {
        let ids: Vec<_> = root.project.items.iter().map(|item| item.id).collect();
        root.selected_ids = ids.iter().copied().collect();
        root.open_zoom_layer(ids[0], cx);
    });
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    cx.run_until_parked();

    // 浮层能正常画出来，且选中数量记录正确
    assert!(
        cx.read_entity(&view, |root, _| root.zoom_layer),
        "7 张图也应该能打开浮层"
    );
    assert_eq!(
        cx.read_entity(&view, |root, _| root.selected_ids.len()),
        7,
        "七张都应处于选中状态"
    );
    assert!(
        cx.debug_bounds("layer-top-1").is_some(),
        "层数再多也要能拖最上面那张的边界"
    );

    // 被省略的层数 = 7 - 5 = 2
    let hidden = 7 - crate::ui::tokens::STACK_MAX_LAYERS;
    assert_eq!(hidden, 2);
    assert!(
        !cx.read_entity(&view, |root, _| root.language.stack_hidden_layers(hidden))
            .is_empty(),
        "超出上限时应有提示文字"
    );
}

#[test]
fn layout_survives_minimum_and_large_window_sizes() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    seed_images(cx, &view, 2);

    // 最小窗口尺寸：所有关键元素都还要在，且不重叠
    cx.simulate_resize(gpui::size(gpui::px(1240.), gpui::px(720.)));
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    cx.run_until_parked();

    let strip = cx.debug_bounds("rail-strip").expect("最小尺寸下应有图标条");
    let preview = cx.debug_bounds("preview-scroll").expect("最小尺寸下应有预览区");
    let preview_right = f32::from(preview.origin.x) + f32::from(preview.size.width);
    assert!(
        preview_right <= f32::from(strip.origin.x) + 1.0,
        "最小尺寸下预览区和图标条不应重叠"
    );
    assert!(
        f32::from(strip.size.width) >= 39.0,
        "最小尺寸下图标条不能被压窄"
    );

    // 大窗口
    cx.simulate_resize(gpui::size(gpui::px(2400.), gpui::px(1400.)));
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    cx.run_until_parked();
    let export = cx.debug_bounds("export-png").expect("大窗口下应有导出按钮");
    let export_right = f32::from(export.origin.x) + f32::from(export.size.width);
    assert!(
        export_right > 2400.0 * 0.9,
        "大窗口下导出按钮仍应贴右：{export_right}"
    );

    // 打开浮层再缩到最小，不能崩
    cx.update_entity(&view, |root, cx| {
        root.open_zoom_layer(1, cx);
    });
    cx.simulate_resize(gpui::size(gpui::px(1240.), gpui::px(720.)));
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    cx.run_until_parked();
    assert!(
        cx.read_entity(&view, |root, _| root.zoom_layer),
        "缩到最小尺寸浮层应还在"
    );

    // 展开图标条面板再缩到最小
    cx.update_entity(&view, |root, cx| {
        root.close_zoom_layer(cx);
        root.set_rail_hover(true, cx);
        root.toggle_rail_pin(cx);
    });
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    cx.run_until_parked();
    assert!(
        cx.debug_bounds("rail-panel").is_some(),
        "最小尺寸下面板仍应能展开"
    );
}

#[test]
fn more_menu_draws_above_the_preview_and_is_clickable() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    seed_images(cx, &view, 1);

    let more = cx.debug_bounds("toolbar-more").expect("三个点");
    cx.simulate_click(more.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    cx.run_until_parked();

    // 菜单要真的画出来
    let menu = cx.debug_bounds("more-menu-layer").expect("菜单浮层应存在");
    let menu_item = cx.debug_bounds("more-clear").expect("清空项应画出来");

    // 菜单项落在预览区覆盖的范围内（这正是以前被盖住的情形）
    let preview = cx.debug_bounds("preview-scroll").expect("预览区");
    let preview_h = preview.size.height;
    let preview_top = f32::from(preview.origin.y);
    let item_top = f32::from(menu_item.origin.y);
    assert!(
        item_top > preview_top && item_top < preview_top + f32::from(preview_h),
        "菜单项应该落在预览区范围内，才能验证层级关系"
    );

    // 点菜单项要能生效（以前点击落在预览区上，菜单关不掉、功能也不触发）
    assert_eq!(
        cx.read_entity(&view, |root, _| root.project.items.len()),
        1,
        "清空前应该还有一张图"
    );
    cx.simulate_click(menu_item.center(), gpui::Modifiers::default());
    cx.run_until_parked();

    assert_eq!(
        cx.read_entity(&view, |root, _| root.project.items.len()),
        0,
        "点了清空菜单项，图片应该被清掉"
    );
    assert!(
        !cx.read_entity(&view, |root, _| root.more_menu_open),
        "点过菜单项之后菜单应该关掉"
    );
    let _ = menu;
}

#[test]
fn clicking_outside_the_menu_closes_it() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    seed_images(cx, &view, 1);

    let more = cx.debug_bounds("toolbar-more").expect("三个点");
    cx.simulate_click(more.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    cx.run_until_parked();
    assert!(cx.read_entity(&view, |root, _| root.more_menu_open));

    // 点窗口另一侧的空白处
    cx.simulate_click(
        gpui::point(gpui::px(1200.), gpui::px(700.)),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    assert!(
        !cx.read_entity(&view, |root, _| root.more_menu_open),
        "点菜单外面应该关掉菜单"
    );
}

#[test]
fn clicking_subtitle_icon_in_layout_mode_switches_back() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    seed_images(cx, &view, 1);

    // 切到排版模式
    cx.update_entity(&view, |root, cx| {
        root.set_mode(crate::model::Mode::Layout, cx);
    });
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    cx.run_until_parked();

    // 展开面板让参数图标可点
    cx.update_entity(&view, |root, cx| {
        root.open_rail_panel(crate::ui::rail::RailPanel::Layout, cx);
    });
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    cx.run_until_parked();

    // 点字幕参数图标
    let subtitle_icon = cx.debug_bounds("rail-subtitle").expect("字幕图标");
    cx.simulate_click(subtitle_icon.center(), gpui::Modifiers::default());
    cx.run_until_parked();

    assert_eq!(
        cx.read_entity(&view, |root, _| root.project.mode),
        crate::model::Mode::Subtitle,
        "排版模式下点字幕图标，应该自动切回字幕模式"
    );
    assert_eq!(
        cx.read_entity(&view, |root, _| root.visible_rail_panel()),
        Some(crate::ui::rail::RailPanel::Subtitle),
        "并且展开字幕参数面板"
    );
}

#[test]
fn rail_strip_has_no_duplicate_more_button() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    seed_images(cx, &view, 1);

    // 展开面板
    cx.update_entity(&view, |root, cx| {
        root.open_rail_panel(crate::ui::rail::RailPanel::Layout, cx);
    });
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    cx.run_until_parked();

    // 图标条上的 ⋯ 已经去掉，只剩左上角那一个
    assert!(
        cx.debug_bounds("rail-more").is_none(),
        "图标条上不该再有 ⋯，它和左上角重复了"
    );
    assert!(
        cx.debug_bounds("toolbar-more").is_some(),
        "左上角的入口应该保留"
    );
}
