//! 用 GPUI 的测试平台跑界面：不需要显卡，但会真实创建窗口、执行渲染、
//! 触发粘贴和字幕检测，用来发现只在界面运行时才会出现的问题。

use gpui::{AppContext, ClipboardItem, Image, ImageFormat, TestAppContext};

use super::actions::{DialogSkip, PasteImage};
use super::AppRoot;

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

    paste_png(cx, sample_png("示例_单行字幕图片/粘贴的图像.png"));

    let (count, detecting, dialog, output) = cx.read_entity(&view, |root, _| {
        (
            root.project.items.len(),
            root.detecting,
            root.dialog,
            root.output_size(),
        )
    });
    assert_eq!(count, 1, "应该加入一张图片");
    assert!(!detecting, "检测应该已经结束");
    assert!(dialog.is_none(), "这张图有字幕，不应该弹窗");
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
fn blank_image_opens_dialog_and_skip_removes_it() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);

    paste_png(cx, blank_png());

    let dialog = cx.read_entity(&view, |root, _| root.dialog);
    assert!(dialog.is_some(), "空白图应该触发“没找到字幕”的弹窗");

    // 画一帧带弹窗的界面
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });

    cx.dispatch_action(DialogSkip);
    cx.run_until_parked();
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });

    let (count, dialog) = cx.read_entity(&view, |root, _| (root.project.items.len(), root.dialog));
    assert_eq!(count, 0, "按 Esc 跳过后应该移除这张图");
    assert!(dialog.is_none());
}

#[test]
fn switch_mode_and_render_layout_canvas() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);

    paste_png(cx, sample_png("示例_单行字幕图片/粘贴的图像.png"));
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
        "示例_多行字幕图片/粘贴的图像.png",
        "示例_多行字幕图片/粘贴的图像 (2).png",
        "示例_多行字幕图片/粘贴的图像 (3).png",
    ] {
        paste_png(cx, sample_png(relative));
    }

    let (count, output) = cx.read_entity(&view, |root, _| (root.project.items.len(), root.output_size()));
    assert_eq!(count, 3);
    let (width, height) = output.expect("应该有输出尺寸");
    assert_eq!(width, 3840);
    assert!(height > 2000, "三张图拼接结果高度应大于 2000，实际 {height}");

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

    paste_png(cx, sample_png("示例_单行字幕图片/粘贴的图像.png"));
    paste_png(cx, sample_png("示例_单行字幕图片/粘贴的图像 (2).png"));

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
        "示例_多行字幕图片/粘贴的图像.png",
        "示例_多行字幕图片/粘贴的图像 (2).png",
        "示例_多行字幕图片/粘贴的图像 (3).png",
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

    paste_png(cx, sample_png("示例_单行字幕图片/粘贴的图像.png"));
    let expected = cx
        .read_entity(&view, |root, _| root.output_size())
        .expect("应该有输出尺寸");

    let dir = std::env::temp_dir().join(format!("video-screenshot-export-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("创建临时目录失败");
    let target = dir.join("输出.png");

    cx.dispatch_action(super::actions::ExportPng);
    cx.run_until_parked();
    assert!(
        cx.did_prompt_for_new_path(),
        "导出应该弹出保存路径对话框"
    );

    let selected = target.clone();
    cx.simulate_new_path_selection(move |_| Some(selected));
    cx.run_until_parked();

    let status = cx.read_entity(&view, |root, _| root.status.text.clone());
    assert!(
        target.exists(),
        "导出文件应该存在，当前状态：{status}"
    );
    let decoded = image::open(&target)
        .expect("导出的 PNG 应该能解码")
        .to_rgb8();
    assert_eq!(decoded.dimensions(), expected);
    assert!(status.contains("已保存"), "状态栏应提示已保存，实际：{status}");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn keep_second_line_and_bleed_change_output_height() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);

    paste_png(cx, sample_png("示例_多行字幕图片/粘贴的图像.png"));
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
        "示例_多行字幕图片/粘贴的图像.png",
        "示例_多行字幕图片/粘贴的图像 (2).png",
        "示例_多行字幕图片/粘贴的图像 (3).png",
    ] {
        paste_png(cx, sample_png(relative));
    }

    let ids = cx.read_entity(&view, |root, _| {
        root.project.items.iter().map(|item| item.id).collect::<Vec<_>>()
    });
    assert_eq!(ids.len(), 3);

    // 把第三张拖到第一张前面
    cx.update_entity(&view, |root, cx| {
        root.apply_drop(ids[2], Some(ids[0]), cx);
    });
    let order = cx.read_entity(&view, |root, _| {
        root.project.items.iter().map(|item| item.id).collect::<Vec<_>>()
    });
    assert_eq!(order, vec![ids[2], ids[0], ids[1]]);

    // 再把第一张拖到末尾
    cx.update_entity(&view, |root, cx| {
        root.apply_drop(ids[2], None, cx);
    });
    let order = cx.read_entity(&view, |root, _| {
        root.project.items.iter().map(|item| item.id).collect::<Vec<_>>()
    });
    assert_eq!(order, vec![ids[0], ids[1], ids[2]]);
}

#[test]
fn remove_selected_image_action() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);

    paste_png(cx, sample_png("示例_单行字幕图片/粘贴的图像.png"));
    paste_png(cx, sample_png("示例_单行字幕图片/粘贴的图像 (2).png"));

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
    assert_eq!(
        cx.read_entity(&view, |root, _| root.project.items.len()),
        2
    );
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
    }
}
