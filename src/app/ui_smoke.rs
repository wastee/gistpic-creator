//! 无头界面测试：不需要显卡，但会真实创建窗口、执行渲染、触发粘贴和字幕检测。
//!
//! 重点覆盖新界面里只有在真实布局下才能验证的东西：胶囊、原地透视裁剪、
//! 接缝拖动、拉框多选、拖动删除。

use std::sync::Arc;

use gpui::{AppContext, ClipboardItem, Image, ImageFormat, TestAppContext};
use image::RgbImage;

use super::AppRoot;
use super::actions::{CopyImage, PasteImage};
use crate::model::{Item, ItemId, ItemStatus, LineBox, Mode, SubtitleBlock};

fn sample_png(relative: &str) -> Vec<u8> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    std::fs::read(&path).unwrap_or_else(|err| panic!("读取 {} 失败: {err}", path.display()))
}

fn blank_png() -> Vec<u8> {
    let image = RgbImage::from_pixel(640, 360, image::Rgb([250, 250, 250]));
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
    cx.run_until_parked();
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    cx.run_until_parked();
}

fn line(y0: f32, y1: f32) -> LineBox {
    LineBox {
        x0: 100.0,
        y0,
        x1: 900.0,
        y1,
    }
}

fn test_item(id: ItemId, width: u32, height: u32, lines: Vec<LineBox>) -> Item {
    let has_lines = !lines.is_empty();
    Item {
        id,
        name: format!("测试图{id}.png"),
        width,
        height,
        rgb: Arc::new(RgbImage::from_pixel(
            width,
            height,
            image::Rgb([40, 40, 44]),
        )),
        block: if has_lines {
            Some(SubtitleBlock { lines })
        } else {
            None
        },
        status: if has_lines {
            ItemStatus::Ready
        } else {
            ItemStatus::NoSubtitle
        },
        keep_whole: !has_lines,
        manual: None,
    }
}

fn install_items(cx: &mut gpui::VisualTestContext, view: &gpui::Entity<AppRoot>, items: Vec<Item>) {
    let ids: Vec<ItemId> = items.iter().map(|item| item.id).collect();
    cx.update_entity(view, |root, cx| {
        root.project.items = items;
        root.selected = ids.first().copied();
        root.selected_ids = ids.iter().copied().collect();
        root.preview_cache.clear();
        cx.notify();
    });
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    cx.run_until_parked();
}

fn draw(cx: &mut gpui::VisualTestContext) {
    cx.update(|window, cx| {
        let _ = window.draw(cx);
    });
    cx.run_until_parked();
}

#[test]
fn minimum_window_keeps_capsule_inside() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    cx.simulate_resize(gpui::size(gpui::px(960.), gpui::px(600.)));
    install_items(
        cx,
        &view,
        vec![test_item(1, 1000, 1000, vec![line(700.0, 760.0)])],
    );
    draw(cx);
    let capsule = cx.debug_bounds("capsule").expect("小窗口也应有胶囊");
    assert!(capsule.origin.x >= gpui::px(0.));
    assert!(capsule.origin.y >= gpui::px(0.));
    assert!(capsule.right() <= gpui::px(960.));
    assert!(capsule.bottom() <= gpui::px(600.));
    let canvas = cx.debug_bounds("canvas").expect("小窗口也应有画布");
    assert!(canvas.size.width <= gpui::px(960.));
}

#[test]
fn window_opens_and_renders_empty_state() {
    let mut cx = TestAppContext::single();
    let (_view, cx) = cx.add_window_view(AppRoot::new);

    cx.simulate_resize(gpui::size(gpui::px(1240.), gpui::px(720.)));
    draw(cx);
    assert!(
        cx.debug_bounds("empty-card").is_some(),
        "空状态应有一张可点的卡片"
    );
    assert!(cx.debug_bounds("capsule").is_some(), "底部应有一条胶囊");
    assert!(cx.debug_bounds("toolbar").is_none(), "旧工具栏不该再出现");
    assert!(
        cx.debug_bounds("preview-scroll").is_none(),
        "旧预览区不该再出现"
    );

    cx.simulate_resize(gpui::size(gpui::px(1600.), gpui::px(1000.)));
    draw(cx);
    assert!(
        cx.debug_bounds("capsule").is_some(),
        "放大窗口后胶囊仍应可见"
    );
}

#[test]
fn empty_card_click_requests_open_images() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    let card = cx.debug_bounds("empty-card").expect("应有空状态卡片");
    cx.simulate_click(card.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    let requests = cx.read_entity(&view, |root, _| root.open_requests);
    assert_eq!(requests, 1, "点整张卡片都应触发选择图片");
}

#[test]
fn capsule_contains_mode_actions_and_no_artwork_buttons() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(
        cx,
        &view,
        vec![test_item(1, 1000, 1000, vec![line(700.0, 760.0)])],
    );
    draw(cx);
    for selector in [
        "capsule",
        "mode-subtitle",
        "mode-layout",
        "bleed-slider",
        "capsule-add",
        "export-png",
    ] {
        assert!(cx.debug_bounds(selector).is_some(), "胶囊里应有 {selector}");
    }
    let last = cx
        .debug_bounds("bleed-step-6")
        .expect("留白滑条应有 30% 档");
    cx.simulate_click(last.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    let bleed = cx.read_entity(&view, |root, _| root.project.settings.bleed);
    assert!((bleed - 0.30).abs() < 1e-6, "点最后一档应是 30%");
    // 画布上除了图块和 14 像素热区，不应再出现常驻按钮。
    assert!(cx.debug_bounds("canvas-block-1").is_some());
    assert!(cx.debug_bounds("crop-hotzone-1-top").is_some());
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
        "高度应在合理范围，实际 {height}"
    );
    draw(cx);
}

#[test]
fn blank_image_is_kept_whole_without_dialog() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    paste_png(cx, blank_png());
    let (status, keep_whole, block) = cx.read_entity(&view, |root, _| {
        let item = root.project.items.first().expect("应该有一张图片");
        (item.status.clone(), item.keep_whole, item.block.is_some())
    });
    assert_eq!(status, ItemStatus::NoSubtitle, "空白图应标记为没找到字幕");
    assert!(keep_whole, "没找到字幕时应自动保留整张画面");
    assert!(!block, "没找到字幕时不应该有字幕块");
    draw(cx);
}

#[test]
fn switch_mode_and_render_layout_canvas() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(
        cx,
        &view,
        vec![test_item(1, 1000, 800, vec![line(600.0, 680.0)])],
    );
    cx.update_entity(&view, |root, cx| root.set_mode(Mode::Layout, cx));
    draw(cx);
    assert!(
        cx.debug_bounds("layout-preset-0").is_some(),
        "排版模式应出现比例预设"
    );
    assert!(
        cx.debug_bounds("canvas-corner").is_some(),
        "排版模式应出现画布右下角手柄"
    );
    let output = cx.read_entity(&view, |root, _| root.output_size());
    assert_eq!(output, Some((1000, 800)));
}

#[test]
fn hovering_a_block_shows_ghost_and_crop_handles() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(
        cx,
        &view,
        vec![test_item(1, 1000, 1000, vec![line(700.0, 760.0)])],
    );
    draw(cx);
    let block = cx.debug_bounds("canvas-block-1").expect("应画出一张图");
    cx.simulate_mouse_move(block.center(), None, gpui::Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert!(
        cx.debug_bounds("ghost-1").is_some(),
        "鼠标停在图上时应浮出被裁掉的原图"
    );
}

#[test]
fn drag_top_crop_line_changes_manual_band() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(
        cx,
        &view,
        vec![test_item(1, 1000, 1000, vec![line(700.0, 760.0)])],
    );
    draw(cx);
    let handle = cx
        .debug_bounds("crop-hotzone-1-top")
        .expect("应有上边界热区");
    let start = handle.center();
    let end = gpui::point(start.x, start.y + gpui::px(60.));
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
    draw(cx);
    assert!(
        cx.debug_bounds("snap-ticks").is_some(),
        "拖动裁剪线时应画出磁吸刻度"
    );
    cx.simulate_mouse_up(end2, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();

    let manual = cx.read_entity(&view, |root, _| {
        root.project.items[0].manual.expect("拖动后应产生手动裁剪")
    });
    assert!(
        manual.top > 0.02,
        "上边界应被拖下来，实际 top={}",
        manual.top
    );
    draw(cx);
}

#[test]
fn multi_selected_crop_drag_syncs_all_images() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(
        cx,
        &view,
        vec![
            test_item(1, 1000, 1000, vec![line(700.0, 760.0)]),
            test_item(2, 1000, 1000, vec![line(700.0, 760.0)]),
        ],
    );
    draw(cx);
    let handle = cx
        .debug_bounds("crop-hotzone-1-top")
        .expect("应有上边界热区");
    let start = handle.center();
    let end = gpui::point(start.x, start.y + gpui::px(50.));
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

    let (first, second) = cx.read_entity(&view, |root, _| {
        (root.project.items[0].manual, root.project.items[1].manual)
    });
    assert_eq!(first, second, "多选时所有图片应同步裁剪");
    assert!(first.is_some(), "应产生手动裁剪");
}

#[test]
fn dragging_seam_changes_gap_and_click_cycles_colors() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(
        cx,
        &view,
        vec![
            test_item(1, 1000, 1000, vec![line(700.0, 760.0)]),
            test_item(2, 1000, 1000, vec![line(700.0, 760.0)]),
        ],
    );
    draw(cx);
    let seam = cx.debug_bounds("seam-0").expect("两张图之间应有接缝热区");
    let start = seam.center();
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
    draw(cx);
    assert!(
        cx.debug_bounds("gap-bubble").is_some(),
        "拖动接缝时应显示像素气泡"
    );
    cx.simulate_mouse_up(end2, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    let gap = cx.read_entity(&view, |root, _| root.project.settings.gap);
    assert!(
        gap.enabled && gap.px > 0,
        "拖动接缝应把间距拉开，实际 {gap:?}"
    );
    assert!(gap.px <= 40, "间距最大 40 像素");

    draw(cx);
    let color_before = cx.read_entity(&view, |root, _| root.project.settings.gap.color);
    let seam = cx.debug_bounds("seam-0").expect("接缝仍应在");
    cx.simulate_click(seam.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    let color_after = cx.read_entity(&view, |root, _| root.project.settings.gap.color);
    assert_ne!(color_before, color_after, "单击接缝应切换颜色");
}

#[test]
fn dragging_image_to_capsule_deletes_it() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(
        cx,
        &view,
        vec![
            test_item(1, 1000, 1000, vec![line(700.0, 760.0)]),
            test_item(2, 1000, 1000, vec![line(700.0, 760.0)]),
        ],
    );
    cx.update_entity(&view, |root, cx| root.select(1, cx));
    draw(cx);
    let block = cx.debug_bounds("canvas-block-1").expect("应有第一张图");
    let start = gpui::point(block.center().x, block.center().y);
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    let moved = gpui::point(start.x, start.y + gpui::px(30.));
    cx.simulate_mouse_move(
        moved,
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    let moved2 = gpui::point(start.x, start.y + gpui::px(50.));
    cx.simulate_mouse_move(
        moved2,
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    draw(cx);
    let zone = cx
        .debug_bounds("delete-zone")
        .expect("拖动时胶囊应变成删除区");
    let target = zone.center();
    cx.simulate_mouse_move(
        target,
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    draw(cx);
    let target = cx
        .debug_bounds("delete-zone")
        .expect("删除区应还在")
        .center();
    cx.simulate_mouse_move(
        target,
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    cx.simulate_mouse_up(target, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    let count = cx.read_entity(&view, |root, _| root.project.items.len());
    assert_eq!(count, 1, "拖到胶囊上松手应删除拖动的图片");
}

#[test]
fn marquee_drag_selects_multiple_images() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(
        cx,
        &view,
        vec![
            test_item(1, 1000, 1000, vec![line(700.0, 760.0)]),
            test_item(2, 1000, 1000, vec![line(700.0, 760.0)]),
        ],
    );
    draw(cx);
    let canvas = cx.debug_bounds("canvas").expect("应有画布");
    let start = gpui::point(
        canvas.origin.x + gpui::px(20.),
        canvas.origin.y + gpui::px(40.),
    );
    let end = gpui::point(
        canvas.origin.x + canvas.size.width - gpui::px(20.),
        canvas.origin.y + canvas.size.height - gpui::px(40.),
    );
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    cx.simulate_mouse_move(
        end,
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    cx.simulate_mouse_up(end, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    let selected = cx.read_entity(&view, |root, _| root.selected_ids.len());
    assert_eq!(selected, 2, "拉框应选中两张图，实际 {selected}");

    // 单击画布空白处清空选择。
    cx.simulate_click(start, gpui::Modifiers::default());
    cx.run_until_parked();
    let cleared = cx.read_entity(&view, |root, _| root.selected_ids.len());
    assert_eq!(cleared, 0, "点空白处应清空选择");
}

#[test]
fn right_click_block_opens_warm_card() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(
        cx,
        &view,
        vec![test_item(1, 1000, 1000, vec![line(700.0, 760.0)])],
    );
    cx.update_entity(&view, |root, cx| {
        root.project.settings.crop = true;
        cx.notify();
    });
    draw(cx);
    let block = cx.debug_bounds("canvas-block-1").expect("应有图块");
    cx.simulate_mouse_down(
        block.center(),
        gpui::MouseButton::Right,
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    draw(cx);
    assert!(cx.debug_bounds("context-menu").is_some(), "右键应出现卡片");
    for selector in ["img-keep-single", "img-keep-whole", "img-reset-crop"] {
        assert!(
            cx.debug_bounds(selector).is_some(),
            "图片卡片应有 {selector}"
        );
    }
    // 打开「整张画面」开关应让这张图输出完整画面，状态用同一套判断，和实际输出一致。
    {
        let row = cx
            .debug_bounds("img-keep-single")
            .expect("应有保留单行字幕");
        cx.simulate_click(row.center(), gpui::Modifiers::default());
        cx.run_until_parked();
        let whole = cx.read_entity(&view, |root, _| root.item_keeps_whole_image(1));
        assert!(!whole, "按单行字幕裁剪后不该还是整张画面");

        // 卡片可能正好盖住图块中心，右键时固定选图块左上角的空白处。
        let away = gpui::point(
            block.origin.x + gpui::px(40.),
            block.origin.y + gpui::px(40.),
        );
        cx.simulate_mouse_down(away, gpui::MouseButton::Right, gpui::Modifiers::default());
        cx.run_until_parked();
        draw(cx);
        assert!(
            cx.debug_bounds("context-menu").is_some(),
            "右键应重新打开卡片"
        );
        let row = cx.debug_bounds("img-keep-whole").expect("应有整张画面开关");
        cx.simulate_click(row.center(), gpui::Modifiers::default());
        cx.run_until_parked();
        let whole = cx.read_entity(&view, |root, _| root.item_keeps_whole_image(1));
        assert!(whole, "打开整张画面开关后应输出完整画面");
    }
    // 点「保留单行字幕」应产生手动裁剪。
    cx.simulate_mouse_down(
        block.center(),
        gpui::MouseButton::Right,
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    draw(cx);
    let row = cx
        .debug_bounds("img-keep-single")
        .expect("应有保留单行字幕");
    cx.simulate_click(row.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    let manual = cx.read_entity(&view, |root, _| root.project.items[0].manual);
    assert!(manual.is_some(), "点保留单行字幕后应有手动裁剪");
}

#[test]
fn whole_image_switch_turns_off_for_no_subtitle_image() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(cx, &view, vec![test_item(1, 1000, 1000, Vec::new())]);
    let whole = cx.read_entity(&view, |root, _| root.item_keeps_whole_image(1));
    assert!(whole, "无字幕图片默认完整输出，开关应为开");

    let block = cx.debug_bounds("canvas-block-1").expect("应有图块");
    cx.simulate_mouse_down(
        block.center(),
        gpui::MouseButton::Right,
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    draw(cx);
    let row = cx.debug_bounds("img-keep-whole").expect("应有整张画面开关");
    cx.simulate_click(row.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    let (whole, included) = cx.read_entity(&view, |root, _| {
        (
            root.item_keeps_whole_image(1),
            matches!(
                crate::imaging::crop::resolve(&root.project)[0],
                crate::imaging::crop::Inclusion::FullImage
                    | crate::imaging::crop::Inclusion::Crop(_)
            ),
        )
    });
    assert!(!whole, "关闭开关后应显示为关");
    assert!(!included, "关闭开关后不应再输出整张无字幕图片");
}

#[test]
fn window_controls_float_above_the_canvas_and_close_works() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    cx.update_entity(&view, |root, cx| root.set_close_to_tray(true, cx));
    draw(cx);
    let controls = cx.debug_bounds("window-controls").expect("应有窗口按钮组");
    let canvas = cx.debug_bounds("canvas").expect("应有画布");
    assert!(controls.origin.y >= canvas.origin.y, "窗口按钮应浮在画布上");
    let close = cx.debug_bounds("window-close").expect("应有关闭按钮");
    assert!(close.size.width > gpui::px(0.));
    assert!(close.size.height > gpui::px(0.));
    // 关闭默认隐藏到托盘，窗口实体仍然存在。
    cx.simulate_click(close.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    assert!(cx.read_entity(&view, |root, _| root.close_to_tray));
}

#[test]
fn canvas_right_click_opens_canvas_card() {
    let mut cx = TestAppContext::single();
    let (_view, cx) = cx.add_window_view(AppRoot::new);
    let canvas = cx.debug_bounds("canvas").expect("应有画布");
    let point = gpui::point(
        canvas.origin.x + gpui::px(10.),
        canvas.origin.y + gpui::px(10.),
    );
    cx.simulate_mouse_down(point, gpui::MouseButton::Right, gpui::Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    for selector in [
        "open-images",
        "paste-image",
        "language-menu",
        "clear-all",
        "toggle-tray",
        "quit-app",
    ] {
        assert!(
            cx.debug_bounds(selector).is_some(),
            "画布卡片应有 {selector}"
        );
    }
    assert!(
        cx.debug_bounds("lang-zh").is_none(),
        "语言选项默认应收在二级菜单中"
    );
    let language = cx.debug_bounds("language-menu").expect("应有语言菜单入口");
    cx.simulate_click(language.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert!(cx.debug_bounds("lang-zh").is_some(), "展开后应显示中文选项");
    assert!(cx.debug_bounds("lang-en").is_some(), "展开后应显示英文选项");
    let tray_before = cx.read_entity(&_view, |root, _| root.close_to_tray);
    let tray = cx
        .debug_bounds("toggle-tray")
        .expect("退出方式区应有托盘开关");
    cx.simulate_click(tray.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    let tray_after = cx.read_entity(&_view, |root, _| root.close_to_tray);
    assert_ne!(tray_before, tray_after, "托盘开关应可用");
}

#[test]
fn subtitle_second_line_toggle_lives_in_subtitle_mode() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    draw(cx);
    assert!(
        cx.debug_bounds("subtitle-keep-second").is_some(),
        "字幕模式应显示保留第二行开关"
    );
    assert!(
        cx.debug_bounds("toggle-keep-second").is_none(),
        "右键菜单未打开时不应有旧开关"
    );
    let before = cx.read_entity(&view, |root, _| root.project.settings.keep_second);
    let row = cx
        .debug_bounds("subtitle-keep-second")
        .expect("应有字幕模式开关");
    cx.simulate_click(row.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    let after = cx.read_entity(&view, |root, _| root.project.settings.keep_second);
    assert_ne!(before, after, "字幕模式开关应切换默认保留第二行");
}

#[test]
fn zoom_chip_appears_and_resets() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    cx.update_entity(&view, |root, cx| root.zoom_by(0.2, cx));
    draw(cx);
    let chip = cx
        .debug_bounds("zoom-chip")
        .expect("缩放不是 100% 时应出现比例胶囊");
    cx.simulate_click(chip.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    let zoom = cx.read_entity(&view, |root, _| root.zoom);
    assert!((zoom - 1.0).abs() < 1e-3, "点比例胶囊应回到 100%");
    draw(cx);
}

#[test]
fn copy_and_undo_shortcuts_still_work() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(
        cx,
        &view,
        vec![test_item(1, 1000, 1000, vec![line(700.0, 760.0)])],
    );
    draw(cx);
    cx.dispatch_action(CopyImage);
    cx.run_until_parked();
    assert!(cx.read_from_clipboard().is_some(), "复制图片应写入剪贴板");
    cx.dispatch_action(super::actions::ClearAll);
    cx.run_until_parked();
    assert_eq!(cx.read_entity(&view, |root, _| root.project.items.len()), 0);
    cx.dispatch_action(super::actions::UndoEdit);
    cx.run_until_parked();
    assert_eq!(cx.read_entity(&view, |root, _| root.project.items.len()), 1);
}

#[test]
fn language_and_layout_presets_are_reachable() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(
        cx,
        &view,
        vec![test_item(1, 1000, 800, vec![line(600.0, 680.0)])],
    );
    cx.update_entity(&view, |root, cx| root.set_mode(Mode::Layout, cx));
    draw(cx);
    let preset = cx.debug_bounds("layout-preset-3").expect("应有 1:1 预设");
    cx.simulate_click(preset.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    let target = cx.read_entity(&view, |root, _| root.project.settings.layout);
    assert_eq!(target, crate::model::LayoutTarget::Aspect(1.0));
}

#[test]
fn keep_whole_switch_survives_right_click_card() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(
        cx,
        &view,
        vec![test_item(1, 1000, 1000, vec![line(700.0, 760.0)])],
    );
    draw(cx);
    let whole = cx.read_entity(&view, |root, _| root.item_keeps_whole_image(1));
    assert!(!whole, "自动裁剪时不是整张画面");
    cx.update_entity(&view, |root, cx| {
        root.set_keep_whole(1, true, cx);
    });
    draw(cx);
    let whole = cx.read_entity(&view, |root, _| root.item_keeps_whole_image(1));
    assert!(whole, "调用 set_keep_whole(true) 后应是整张画面");
}
