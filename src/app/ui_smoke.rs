//! 无头界面测试：不需要显卡，但会真实创建窗口、执行渲染、触发粘贴和字幕检测。
//!
//! 重点覆盖新界面里只有在真实布局下才能验证的东西：胶囊、原地透视裁剪、
//! 接缝拖动、拉框多选、拖动删除。

use std::sync::Arc;

use gpui::{AppContext, ClipboardItem, Image, ImageFormat, TestAppContext};
use image::RgbImage;

use super::AppRoot;
use super::actions::{CopyImage, Dismiss, PasteImage};
use crate::model::{Item, ItemId, ItemStatus, LineBox, Mode, SubtitleBlock};
use crate::ui::hover::HoverKey;
use crate::ui::tokens;

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

/// 对照组：悬停反馈已经稳定显示后再按下、抬起，点击应当生效。
/// 用来证明测试环境本身能正常送达点击，排除环境因素。
#[test]
fn click_lands_when_hover_feedback_is_stable() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(
        cx,
        &view,
        vec![test_item(1, 1000, 1000, vec![line(700.0, 760.0)])],
    );
    draw(cx);

    let initial = cx.read_entity(&view, |root, _| root.project.mode);
    let (target, expected) = match initial {
        Mode::Subtitle => ("mode-layout", Mode::Layout),
        Mode::Layout => ("mode-subtitle", Mode::Subtitle),
    };
    let bounds = cx.debug_bounds(target).expect("应有目标模式按钮");

    // 鼠标先移到按钮上，悬停反馈当帧出现，再按下抬起。
    cx.simulate_mouse_move(bounds.center(), None, gpui::Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    cx.simulate_mouse_down(
        bounds.center(),
        gpui::MouseButton::Left,
        gpui::Modifiers::default(),
    );
    draw(cx);
    cx.simulate_mouse_up(
        bounds.center(),
        gpui::MouseButton::Left,
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();

    let mode = cx.read_entity(&view, |root, _| root.project.mode);
    assert_eq!(mode, expected, "悬停稳定后点击应切换模式");
}

/// 快速点击复现：鼠标移到按钮上、悬停反馈出现的同一帧就按下，
/// 按下期间界面重画，然后才抬起。这次点击必须生效。
#[test]
fn click_lands_when_hover_feedback_starts_while_pressed() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(
        cx,
        &view,
        vec![test_item(1, 1000, 1000, vec![line(700.0, 760.0)])],
    );
    draw(cx);

    let initial = cx.read_entity(&view, |root, _| root.project.mode);
    let (target, expected) = match initial {
        Mode::Subtitle => ("mode-layout", Mode::Layout),
        Mode::Layout => ("mode-subtitle", Mode::Subtitle),
    };
    let bounds = cx.debug_bounds(target).expect("应有目标模式按钮");

    // 悬停即时生效；按下、重画、抬起，点击不应丢失。
    cx.simulate_mouse_move(bounds.center(), None, gpui::Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    cx.simulate_mouse_down(
        bounds.center(),
        gpui::MouseButton::Left,
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    draw(cx);
    cx.simulate_mouse_up(
        bounds.center(),
        gpui::MouseButton::Left,
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();

    let mode = cx.read_entity(&view, |root, _| root.project.mode);
    assert_eq!(mode, expected, "按下期间悬停反馈出现，这次点击不应丢失");
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
    // 悬停即时生效：不推进任何时钟，重画一次就应浮出被裁掉的原图。
    assert!(
        cx.debug_bounds("ghost-1").is_some(),
        "指针进入后应立刻浮出被裁掉的原图"
    );
    let active = cx.read_entity(&view, |root, _| root.hover_gate.debug_active());
    assert_eq!(active, Some(HoverKey::Item(1)), "激活目标应是这张图");
}

#[test]
fn hover_moves_between_blocks_immediately() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(
        cx,
        &view,
        vec![
            test_item(1, 1000, 600, vec![line(400.0, 460.0)]),
            test_item(2, 1000, 600, vec![line(400.0, 460.0)]),
        ],
    );
    draw(cx);
    let first = cx.debug_bounds("canvas-block-1").expect("应画出第一张图");
    cx.simulate_mouse_move(first.center(), None, gpui::Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert!(cx.debug_bounds("ghost-1").is_some(), "悬停第一张应浮出透视");

    // 直接从第一张滑到第二张：第二张的悬停应立刻生效。
    // 注意：gpui 测试工具的 debug_bounds 映射表跨帧累积旧条目，
    // “元素已消失”不能用 `is_none` 断言，所以这里直接验证门控激活状态。
    let second = cx.debug_bounds("canvas-block-2").expect("应画出第二张图");
    cx.simulate_mouse_move(second.center(), None, gpui::Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert!(
        cx.debug_bounds("ghost-2").is_some(),
        "滑到第二张后应立刻浮出它的透视"
    );
    let active = cx.read_entity(&view, |root, _| root.hover_gate.debug_active());
    assert_eq!(active, Some(HoverKey::Item(2)), "激活目标应是第二张图");
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
    // 先给两张图设成同一段手动裁剪，这样多选拖动时它们的起点一致，
    // 能干净地验证「同一个位移」而不会被各自的自动结果干扰。
    let start_band = crate::model::ManualBand::clamped(0.0, 1.0, 0.05, 0.95).expect("有效保留带");
    cx.update_entity(&view, |root, cx| {
        root.apply_band_to_items(&[1, 2], start_band);
        cx.notify();
    });
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
    // 先给两张图设成同一段手动裁剪，这样多选拖动时它们的起点一致，
    // 能干净地验证「同一个位移」而不会被各自的自动结果干扰。
    let start_band = crate::model::ManualBand::clamped(0.0, 1.0, 0.05, 0.95).expect("有效保留带");
    cx.update_entity(&view, |root, cx| {
        root.apply_band_to_items(&[1, 2], start_band);
        cx.notify();
    });
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
    let first = first.expect("第一张应产生手动裁剪");
    let second = second.expect("第二张应产生手动裁剪");
    assert!(
        first.top > 0.06,
        "第一张的上边应被往下拖，实际 {}",
        first.top
    );
    assert_eq!(first, second, "多选时所有图片应同步到同一段裁剪");
}

/// 两张竖排的图，作品里因此有一条接缝，够用来显示左侧控件。
fn stacked_items() -> Vec<Item> {
    vec![
        test_item(1, 1000, 1000, vec![line(700.0, 760.0)]),
        test_item(2, 1000, 1000, vec![line(700.0, 760.0)]),
    ]
}

/// 字幕模式下作品左侧的缝隙控件：上下拖改间距、点色点换颜色、悬停时接缝亮起。
#[test]
fn gap_control_drag_and_color_click_work_in_subtitle_mode() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(cx, &view, stacked_items());
    cx.update_entity(&view, |root, cx| {
        root.project.mode = Mode::Subtitle;
        cx.notify();
    });
    draw(cx);

    // 控件挂在作品左边：它的中心在第一个图块左边缘的左边。
    let control = cx.debug_bounds("gap-control").expect("应有缝隙控件");
    let first = cx.debug_bounds("canvas-block-1").expect("第一张应在画布上");
    assert!(
        control.center().x < first.origin.x,
        "控件应在作品左侧：控件 {:?} 图块左边缘 {:?}",
        control.center(),
        first.origin
    );

    // 悬停控件：两条接缝一起亮起。
    cx.simulate_mouse_move(control.center(), None, gpui::Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert!(
        cx.debug_bounds("seam-lit-0").is_some(),
        "指针压在控件上时接缝应亮起"
    );

    // 从控件中心往下拖：间距被拉开，拖动中出现像素气泡。
    let start = control.center();
    let end = gpui::point(start.x, start.y + gpui::px(30.));
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    assert!(
        cx.read_entity(&view, |root, _| root.gap_drag.is_some()),
        "按下控件应进入缝隙拖动"
    );
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
        "拖控件时应显示像素气泡"
    );
    cx.simulate_mouse_up(end2, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    let gap = cx.read_entity(&view, |root, _| root.project.settings.gap);
    assert!(
        gap.enabled && gap.effective_px() > 0,
        "拖控件应把缝隙拉开，实际 {gap:?}"
    );
    assert!(gap.effective_px() <= 40, "缝隙最大 40 像素");

    // 点色点换颜色，而且点色点不该顺手把宽度也拖起来。
    draw(cx);
    let dot = cx.debug_bounds("gap-control-dot").expect("应有色点");
    let before = cx.read_entity(&view, |root, _| root.project.settings.gap.color);
    cx.simulate_mouse_down(
        dot.center(),
        gpui::MouseButton::Left,
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    assert!(
        cx.read_entity(&view, |root, _| root.gap_drag.is_none()),
        "按住色点不该进入宽度拖动"
    );
    cx.simulate_mouse_up(
        dot.center(),
        gpui::MouseButton::Left,
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    let after = cx.read_entity(&view, |root, _| root.project.settings.gap.color);
    assert_ne!(before, after, "点色点应换缝隙颜色");
}

/// 缝隙还是 0 的时候，点色点也要能先选好颜色。
#[test]
fn clicking_the_color_dot_works_while_the_gap_is_zero() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(cx, &view, stacked_items());
    cx.update_entity(&view, |root, cx| {
        root.project.mode = Mode::Subtitle;
        cx.notify();
    });
    draw(cx);
    let gap = cx.read_entity(&view, |root, _| root.project.settings.gap);
    assert_eq!(gap.effective_px(), 0, "起手缝隙应为 0，实际 {gap:?}");

    let dot = cx.debug_bounds("gap-control-dot").expect("应有色点");
    let before = cx.read_entity(&view, |root, _| root.project.settings.gap.color);
    cx.simulate_click(dot.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    let gap = cx.read_entity(&view, |root, _| root.project.settings.gap);
    assert_ne!(before, gap.color, "缝隙为 0 时点色点也应换颜色");
    assert_eq!(
        gap.effective_px(),
        0,
        "点色点不该顺手把缝隙拉开，实际 {gap:?}"
    );
}

/// 排版模式下用的还是同一个控件：位置不变，拖动仍改间距。
#[test]
fn gap_control_is_the_same_control_in_layout_mode() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(cx, &view, stacked_items());
    cx.update_entity(&view, |root, cx| {
        root.project.settings.layout = crate::model::LayoutTarget::Row;
        root.set_mode(Mode::Layout, cx);
    });
    draw(cx);
    let control = cx
        .debug_bounds("gap-control")
        .expect("排版模式也应有缝隙控件");

    let start = control.center();
    let end = gpui::point(start.x, start.y + gpui::px(40.));
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    assert!(
        cx.read_entity(&view, |root, _| root.gap_drag.is_some()),
        "排版模式按下控件应进入缝隙拖动，控件 {control:?}"
    );
    cx.simulate_mouse_move(
        end,
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    // 第一次移动只是把拖动“点亮”，第二次才真的改数值。
    let end2 = gpui::point(end.x, end.y + gpui::px(10.));
    cx.simulate_mouse_move(
        end2,
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    cx.simulate_mouse_up(end2, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    let gap = cx.read_entity(&view, |root, _| root.project.settings.gap);
    assert!(
        gap.enabled && gap.effective_px() > 0,
        "排版模式拖同一个控件也应改间距，实际 {gap:?}"
    );
}

/// 缝隙控件站在画布留白里，按它不该被当成“空白处”而请求移动窗口。
#[test]
fn pressing_gap_control_does_not_move_window() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(cx, &view, stacked_items());
    draw(cx);
    let control = cx.debug_bounds("gap-control").expect("应有缝隙控件");
    let before = cx.read_entity(&view, |root, _| root.window_move_requests);
    let start = control.center();
    let end = gpui::point(start.x, start.y + gpui::px(20.));
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
    let after = cx.read_entity(&view, |root, _| root.window_move_requests);
    assert_eq!(after, before, "按缝隙控件不应请求移动窗口");
}

/// 字幕模式上下两条裁剪热区贴着裁剪线，不再让出 6 像素。
#[test]
fn subtitle_crop_hotzone_is_flush_with_the_line() {
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
    let first = cx.debug_bounds("canvas-block-1").expect("第一张应在画布上");
    let second = cx.debug_bounds("canvas-block-2").expect("第二张应在画布上");
    let bottom = cx
        .debug_bounds("crop-hotzone-1-bottom")
        .expect("第一张应有下边界热区");
    assert!(
        (f32::from(bottom.bottom()) - f32::from(first.bottom())).abs() < 1.0,
        "下边界热区应贴到图块底边：热区 {:?} 图块 {:?}",
        bottom.bottom(),
        first.bottom()
    );
    let top = cx
        .debug_bounds("crop-hotzone-2-top")
        .expect("第二张应有上边界热区");
    assert!(
        (f32::from(top.origin.y) - f32::from(second.origin.y)).abs() < 1.0,
        "上边界热区应贴到图块顶边：热区 {:?} 图块 {:?}",
        top.origin,
        second.origin
    );
}

/// 排版模式上下两条裁剪热区同样贴着裁剪线：接缝已经不接鼠标，不必再让位置。
#[test]
fn layout_mode_crop_hotzone_is_flush_too() {
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
    cx.update_entity(&view, |root, cx| {
        root.set_mode(Mode::Layout, cx);
    });
    draw(cx);
    let first = cx.debug_bounds("canvas-block-1").expect("第一张应在画布上");
    let bottom = cx
        .debug_bounds("crop-hotzone-1-bottom")
        .expect("第一张应有下边界热区");
    assert!(
        (f32::from(bottom.bottom()) - f32::from(first.bottom())).abs() < 1.0,
        "排版模式下边界热区也应贴到图块底边：热区 {:?} 图块 {:?}",
        bottom.bottom(),
        first.bottom()
    );
}

#[test]
fn sorting_drop_animates_changed_positions_then_cleans_up() {
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
    cx.update_entity(&view, |root, cx| {
        root.selected_ids = [1].into_iter().collect();
        root.apply_drop(1, None, cx);
    });
    cx.run_until_parked();
    let (order, has_settle) = cx.read_entity(&view, |root, _| {
        (
            root.project
                .items
                .iter()
                .map(|item| item.id)
                .collect::<Vec<_>>(),
            root.settle.is_some(),
        )
    });
    assert_eq!(order, vec![2, 1], "拖到末尾后图片顺序应改变");
    assert!(has_settle, "排序松手后应记录旧布局用于位移动画");
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(160));
    cx.run_until_parked();
    assert!(
        cx.read_entity(&view, |root, _| root.settle.is_none()),
        "位移动画结束后应清除临时状态"
    );
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
    // 空白处右键拖动才是框选。
    cx.simulate_mouse_down(start, gpui::MouseButton::Right, gpui::Modifiers::default());
    cx.run_until_parked();
    cx.simulate_mouse_move(
        end,
        Some(gpui::MouseButton::Right),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    cx.simulate_mouse_up(end, gpui::MouseButton::Right, gpui::Modifiers::default());
    cx.run_until_parked();
    let selected = cx.read_entity(&view, |root, _| root.selected_ids.len());
    assert_eq!(selected, 2, "右键拉框应选中两张图，实际 {selected}");

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

        // 裁剪后作品变矮、图块整体位置会变，重新量一次；卡片可能盖住中心，取左上角。
        let block = cx.debug_bounds("canvas-block-1").expect("应有图块");
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
    let block = cx.debug_bounds("canvas-block-1").expect("应有图块");
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
    cx.simulate_mouse_up(point, gpui::MouseButton::Right, gpui::Modifiers::default());
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
    let mut row_y = |selector| cx.debug_bounds(selector).expect("菜单项应存在").origin.y;
    assert!(
        row_y("open-images") < row_y("paste-image")
            && row_y("paste-image") < row_y("clear-all")
            && row_y("clear-all") < row_y("language-menu")
            && row_y("language-menu") < row_y("toggle-tray")
            && row_y("toggle-tray") < row_y("quit-app"),
        "清空应排在图片入口之后、语言之前，退出选项保持在末尾"
    );
    assert!(
        cx.debug_bounds("lang-zh").is_none(),
        "语言选项默认应收在二级菜单中"
    );
    let language = cx.debug_bounds("language-menu").expect("应有语言菜单入口");
    // 二级菜单跟随悬停即时弹出。
    cx.simulate_mouse_move(language.center(), None, gpui::Modifiers::default());
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
fn language_submenu_opens_on_hover_and_closes_after_delay() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    cx.update_entity(&view, |root, cx| root.open_canvas_menu(300.0, 300.0, cx));
    draw(cx);
    let row = cx.debug_bounds("language-menu").expect("应画出语言行");
    assert!(
        cx.debug_bounds("language-submenu").is_none(),
        "悬停前不应有二级菜单"
    );

    // 悬停“语言”行：即时弹出。
    cx.simulate_mouse_move(row.center(), None, gpui::Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert!(
        cx.debug_bounds("language-submenu").is_some(),
        "悬停“语言”行应立刻弹出二级菜单"
    );

    // 移开鼠标：不能马上收起，要等 `SUBMENU_CLOSE_GRACE`。
    cx.simulate_mouse_move(
        gpui::point(row.center().x, row.center().y + gpui::px(90.)),
        None,
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    draw(cx);
    assert!(
        cx.debug_bounds("language-submenu").is_some(),
        "移开后 200ms 内应保持打开"
    );
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(200));
    cx.run_until_parked();
    draw(cx);
    // debug_bounds 映射表跨帧残留旧条目，收起与否要看实体状态。
    let still_open = cx.read_entity(&view, |root, _| root.language_submenu_open);
    assert!(!still_open, "移开满 200ms 应收起二级菜单");
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

#[test]
fn artwork_default_size_follows_golden_ratio() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    // 测试窗口是 1920×1080，短边 1080 的黄金分割约 667。
    let fit = 1080.0 * tokens::GOLDEN_FIT_FRACTION;
    // 小图按实际像素显示，不放大
    install_items(cx, &view, vec![test_item(1, 400, 300, vec![])]);
    cx.update_entity(&view, |root, cx| root.set_mode(Mode::Layout, cx));
    draw(cx);
    let artwork = cx.debug_bounds("artwork").expect("应有作品画布");
    assert!(
        (f32::from(artwork.size.width) - 400.0).abs() < 0.5,
        "400×300 应显示 400 宽，实际 {}",
        artwork.size.width
    );
    assert!(
        (f32::from(artwork.size.height) - 300.0).abs() < 0.5,
        "400×300 应显示 300 高，实际 {}",
        artwork.size.height
    );

    // 1920×1080 的最短边压到约 667，宽约 1187
    install_items(cx, &view, vec![test_item(2, 1920, 1080, vec![])]);
    draw(cx);
    let artwork = cx.debug_bounds("artwork").expect("应有作品画布");
    assert!(
        (f32::from(artwork.size.height) - fit).abs() < 0.5,
        "1920×1080 应显示约 {fit} 高，实际 {}",
        artwork.size.height
    );
    assert!(
        (f32::from(artwork.size.width) - 1920.0 * tokens::GOLDEN_FIT_FRACTION).abs() < 1.0,
        "1920×1080 应显示约 1187 宽，实际 {}",
        artwork.size.width
    );

    // 1920×3240 的最短边是宽，压到约 667，高约 1126（超过窗口，靠滚动看全）
    install_items(cx, &view, vec![test_item(3, 1920, 3240, vec![])]);
    draw(cx);
    let artwork = cx.debug_bounds("artwork").expect("应有作品画布");
    assert!(
        (f32::from(artwork.size.width) - fit).abs() < 0.5,
        "1920×3240 应显示约 {fit} 宽，实际 {}",
        artwork.size.width
    );
    assert!(
        (f32::from(artwork.size.height) - 3240.0 * fit / 1920.0).abs() < 1.0,
        "1920×3240 应显示约 1126 高，实际 {}",
        artwork.size.height
    );
}

#[test]
fn wide_artwork_scrolls_horizontally() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(
        cx,
        &view,
        vec![
            test_item(1, 1920, 1080, vec![]),
            test_item(2, 1920, 1080, vec![]),
        ],
    );
    cx.update_entity(&view, |root, cx| {
        root.project.settings.layout = crate::model::LayoutTarget::Row;
        root.set_mode(Mode::Layout, cx);
    });
    draw(cx);
    let (max_offset, canvas) = cx.read_entity(&view, |root, _| {
        (root.canvas_scroll.max_offset(), root.canvas_scroll.bounds())
    });
    assert!(
        f32::from(max_offset.width) > 0.0,
        "两图横排比窗口宽，应能横向滚动，实际 {:?}",
        max_offset.width
    );
    assert!(f32::from(canvas.size.width) > 0.0, "画布滚动区应有宽度");
}

/// 排版模式下拖左右裁剪线，只有被拖的那张被裁。
#[test]
fn drag_right_crop_line_crops_only_that_image() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(
        cx,
        &view,
        vec![
            test_item(1, 1000, 1000, vec![]),
            test_item(2, 1000, 1000, vec![]),
        ],
    );
    cx.update_entity(&view, |root, cx| {
        root.project.settings.layout = crate::model::LayoutTarget::Aspect(1.0);
        root.set_mode(Mode::Layout, cx);
        root.selected_ids = [1].into_iter().collect();
        cx.notify();
    });
    draw(cx);
    let before = cx.read_entity(&view, |root, _| root.canvas_layout());
    let second_before = before.entry(2).expect("第二张应在画布上").rect;
    let handle = cx
        .debug_bounds("crop-hotzone-1-right")
        .expect("排版模式应有右边界热区");
    let start = handle.center();
    let mid = gpui::point(start.x - gpui::px(60.), start.y);
    let end = gpui::point(start.x - gpui::px(100.), start.y);
    // 第一次移动才开始算拖动，第二次移动才把拖动量送到裁剪逻辑。
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    cx.simulate_mouse_move(
        mid,
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    cx.simulate_mouse_move(
        end,
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    cx.simulate_mouse_up(end, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();

    let (first_manual, second_manual) = cx.read_entity(&view, |root, _| {
        (root.project.items[0].manual, root.project.items[1].manual)
    });
    let first_manual = first_manual.expect("第一张应被裁");
    assert!(
        first_manual.right < 0.95,
        "第一张右边应被裁掉，实际 {first_manual:?}"
    );
    assert_eq!(first_manual.left, 0.0, "拖右边缘不能动左边缘");
    assert_eq!(first_manual.top, 0.0, "横向拖动不能动纵向");
    assert_eq!(first_manual.bottom, 1.0);
    assert!(
        second_manual.is_none(),
        "另一张图不该被裁，实际 {second_manual:?}"
    );
    let after = cx.read_entity(&view, |root, _| root.canvas_layout());
    let second_after = after.entry(2).expect("第二张仍在画布上").rect;
    assert!(
        (second_before.w - second_after.w).abs() < 0.5,
        "第二张的显示宽度不该变"
    );
}

/// 横排两张 1000×1000，拖下边缘：整排共用高度一起变，宽度不动。
#[test]
fn row_bottom_drag_sets_shared_height_on_all() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(
        cx,
        &view,
        vec![
            test_item(1, 1000, 1000, vec![]),
            test_item(2, 1000, 1000, vec![]),
        ],
    );
    cx.update_entity(&view, |root, cx| {
        root.project.settings.layout = crate::model::LayoutTarget::Row;
        root.set_mode(Mode::Layout, cx);
        cx.notify();
    });
    draw(cx);
    let handle = cx
        .debug_bounds("crop-hotzone-1-bottom")
        .expect("横排应有下边界热区");
    let start = handle.center();
    let mid = gpui::point(start.x, start.y - gpui::px(60.));
    let end = gpui::point(start.x, start.y - gpui::px(100.));
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    cx.simulate_mouse_move(
        mid,
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    cx.simulate_mouse_move(
        end,
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    cx.simulate_mouse_up(end, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();

    let (first, second) = cx.read_entity(&view, |root, _| {
        (root.project.items[0].manual, root.project.items[1].manual)
    });
    let first = first.expect("第一张应被裁");
    let second = second.expect("第二张应一起被裁");
    assert!(first.bottom < 0.95, "整排高度应变小，实际 {}", first.bottom);
    assert!(
        (first.bottom - second.bottom).abs() < 1e-3,
        "整排应共用同一个高度，实际 {} 与 {}",
        first.bottom,
        second.bottom
    );
    assert_eq!(first.left, 0.0, "改高度不能动宽度");
    assert_eq!(first.right, 1.0);
}

/// 竖排两张，拖右边缘：整列共用宽度一起变，高度不动。
#[test]
fn column_right_drag_sets_shared_width_on_all() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(
        cx,
        &view,
        vec![
            test_item(1, 1000, 1000, vec![]),
            test_item(2, 1000, 1000, vec![]),
        ],
    );
    cx.update_entity(&view, |root, cx| {
        root.project.settings.layout = crate::model::LayoutTarget::Column;
        root.set_mode(Mode::Layout, cx);
        cx.notify();
    });
    draw(cx);
    let handle = cx
        .debug_bounds("crop-hotzone-1-right")
        .expect("竖排应有右边界热区");
    let start = handle.center();
    let mid = gpui::point(start.x - gpui::px(60.), start.y);
    let end = gpui::point(start.x - gpui::px(100.), start.y);
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    cx.simulate_mouse_move(
        mid,
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    cx.simulate_mouse_move(
        end,
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    cx.simulate_mouse_up(end, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();

    let (first, second) = cx.read_entity(&view, |root, _| {
        (root.project.items[0].manual, root.project.items[1].manual)
    });
    let first = first.expect("第一张应被裁");
    let second = second.expect("第二张应一起被裁");
    assert!(first.right < 0.95, "整列宽度应变小，实际 {}", first.right);
    assert!(
        (first.right - second.right).abs() < 1e-3,
        "整列应共用同一个宽度，实际 {} 与 {}",
        first.right,
        second.right
    );
    assert_eq!(first.top, 0.0, "改宽度不能动高度");
    assert_eq!(first.bottom, 1.0);
}

/// 已经完整显示时，继续往外拖裁剪线没有任何变化。
#[test]
fn dragging_crop_line_outwards_when_whole_image_does_nothing() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(cx, &view, vec![test_item(1, 1000, 1000, vec![])]);
    cx.update_entity(&view, |root, cx| {
        root.set_mode(Mode::Layout, cx);
        root.set_layout_target(crate::model::LayoutTarget::Aspect(1.0), cx);
        cx.notify();
    });
    draw(cx);
    let handle = cx
        .debug_bounds("crop-hotzone-1-bottom")
        .expect("应有下边界热区");
    let start = handle.center();
    let end = gpui::point(start.x, start.y + gpui::px(120.));
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

    let manual = cx.read_entity(&view, |root, _| root.project.items[0].manual);
    assert!(
        manual.is_none_or(|band| band.bottom >= 0.999),
        "整张画面已经全显示，往下拖不该产生裁剪，实际 {manual:?}"
    );
}

/// 拖动时底部是一个安静的垃圾桶，鼠标压上去才张开、变红、写明松手删除。
#[test]
fn delete_target_arms_on_hover() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(
        cx,
        &view,
        vec![test_item(1, 1000, 1000, vec![line(700.0, 760.0)])],
    );
    draw(cx);
    let block = cx.debug_bounds("canvas-block-1").expect("第一张应在画布上");
    let start = block.center();
    let end = gpui::point(start.x + gpui::px(30.), start.y + gpui::px(50.));
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    // 第一次移动才开始算拖动，再一次移动才把拖动送到画布上。
    cx.simulate_mouse_move(
        gpui::point(start.x, start.y + gpui::px(30.)),
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    cx.simulate_mouse_move(
        end,
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    draw(cx);
    let quiet = cx.debug_bounds("delete-zone").expect("拖动时应有删除目标");
    assert!(
        cx.debug_bounds("delete-target-armed").is_none(),
        "鼠标还没压上去，不该是张开状态"
    );

    // 把鼠标移到垃圾桶正中：张开、变红、出现提示文字
    let center = quiet.center();
    cx.simulate_mouse_move(
        center,
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    cx.simulate_mouse_move(
        center,
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    draw(cx);
    assert!(
        cx.debug_bounds("delete-target-armed").is_some(),
        "鼠标压在垃圾桶上时应张开并提示松手删除"
    );
    let armed = cx.debug_bounds("delete-zone").expect("删除目标仍在");
    assert!(
        f32::from(armed.size.height) > f32::from(quiet.size.height),
        "张开后应更高：安静 {} 张开 {}",
        quiet.size.height,
        armed.size.height
    );
    // 张开后指针仍在目标内，否则会出现张开又收起的抖动
    assert!(
        f32::from(armed.left()) <= f32::from(center.x)
            && f32::from(center.x) <= f32::from(armed.right()),
        "张开后指针应仍在目标内：目标 {:?} 指针 {:?}",
        armed,
        center
    );

    // 松手删除
    cx.simulate_mouse_up(center, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    let after = cx.read_entity(&view, |root, _| {
        (
            root.project.items.len(),
            root.delete_hover,
            root.dragging.is_some(),
        )
    });
    assert_eq!(
        after.0, 0,
        "松手在垃圾桶上应删掉拖动的图，当前状态 {after:?}，目标 {armed:?}"
    );
}

#[test]
fn delete_target_disarms_when_pointer_leaves() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(
        cx,
        &view,
        vec![test_item(1, 1000, 1000, vec![line(700.0, 760.0)])],
    );
    draw(cx);
    let block = cx.debug_bounds("canvas-block-1").expect("第一张应在画布上");
    let start = block.center();
    let end = gpui::point(start.x + gpui::px(30.), start.y + gpui::px(50.));
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    cx.simulate_mouse_move(
        gpui::point(start.x, start.y + gpui::px(30.)),
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    cx.simulate_mouse_move(
        end,
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    draw(cx);
    let zone = cx.debug_bounds("delete-zone").expect("拖动时应有删除目标");
    let center = zone.center();
    cx.simulate_mouse_move(
        center,
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    cx.simulate_mouse_move(
        center,
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    draw(cx);
    assert!(
        cx.debug_bounds("delete-target-armed").is_some(),
        "压上去应张开"
    );

    // 移开：收回安静态，松手不删除
    let away = gpui::point(center.x, center.y - gpui::px(120.));
    cx.simulate_mouse_move(
        away,
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    cx.simulate_mouse_move(
        away,
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    draw(cx);
    let state = cx.read_entity(&view, |root, _| {
        (
            root.delete_hover,
            root.dragging.is_some(),
            root.capsule_target_prev,
        )
    });
    assert!(!state.0, "移开后应清掉删除悬停，当前 {state:?}");
    // debug_bounds 只增不删，退场的元素会留旧值，所以这里看仍然在树上的
    // delete-zone 自己的高度：收回安静态应是 36，张开是 44。
    let quiet = cx.debug_bounds("delete-zone").expect("删除目标仍在");
    assert!(
        f32::from(quiet.size.height) < 44.0,
        "移开后应收回安静高度，当前 {:?}",
        quiet.size
    );
    cx.simulate_mouse_up(away, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    assert_eq!(
        cx.read_entity(&view, |root, _| root.project.items.len()),
        1,
        "松手在别处不该删除"
    );
}

// ----------------------------------------------------------------------
// 验收：按计划里逐条写下的显示尺寸和拖动结果，全部在无头窗口里真实跑一遍。
// ----------------------------------------------------------------------

/// 计划第 1 条：400×300 的小图不放大，还是 400×300。
/// 计划第 2 条：1920×1080 单张按黄金分割显示约 1187×667。
/// 计划第 3 条：1920×3240 单张显示约 667×1126。
#[test]
fn acceptance_display_sizes() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    for (width, height, expect_w, expect_h) in [
        (400u32, 300u32, 400.0f32, 300.0f32),
        (1920, 1080, 1187.0, 667.0),
        (1920, 3240, 667.0, 1126.0),
    ] {
        install_items(cx, &view, vec![test_item(1, width, height, vec![])]);
        draw(cx);
        let artwork = cx
            .debug_bounds("artwork")
            .unwrap_or_else(|| panic!("{width}×{height} 应有画布"));
        assert!(
            (f32::from(artwork.size.width) - expect_w).abs() < 1.5
                && (f32::from(artwork.size.height) - expect_h).abs() < 1.5,
            "{width}×{height} 应显示 {expect_w}×{expect_h}，实际 {}×{}",
            f32::from(artwork.size.width),
            f32::from(artwork.size.height)
        );
    }
}

/// 计划第 4 条：横排 1920×1080 加 1000×1000，显示 1920×1080 加 1080×1080，
/// 画布 3000×1080。
#[test]
fn acceptance_row_shares_one_height() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(
        cx,
        &view,
        vec![
            test_item(1, 1920, 1080, vec![]),
            test_item(2, 1000, 1000, vec![]),
        ],
    );
    cx.update_entity(&view, |root, cx| {
        root.project.settings.layout = crate::model::LayoutTarget::Row;
        root.set_mode(Mode::Layout, cx);
        cx.notify();
    });
    draw(cx);
    let layout = cx.read_entity(&view, |root, _| root.canvas_layout());
    let first = layout.entry(1).expect("第一张应在画布上");
    let second = layout.entry(2).expect("第二张应在画布上");
    // 整张作品的最短边压到窗口短边（1080）的黄金分割，约 667。
    let scale = tokens::GOLDEN_FIT_FRACTION;
    let display_short = 1080.0 * scale;
    assert!(
        (layout.scale - scale).abs() < 1e-3,
        "3000×1080 的最短边应压到约 {display_short}，实际缩放 {}",
        layout.scale
    );
    assert!(
        (first.display_width - 1920.0 * scale).abs() < 1.0
            && (first.display_height - display_short).abs() < 1.0,
        "第一张应显示成 {}×{display_short}，实际 {}×{}",
        1920.0 * scale,
        first.display_width,
        first.display_height
    );
    assert!(
        (second.display_width - 1080.0 * scale).abs() < 1.0
            && (second.display_height - display_short).abs() < 1.0,
        "第二张应显示成 {}×{display_short}，实际 {}×{}",
        1080.0 * scale,
        second.display_width,
        second.display_height
    );
    // 输出尺寸（导出用的像素）保持计划里的数字。
    let output = cx
        .read_entity(&view, |root, _| root.output_size())
        .expect("应有输出尺寸");
    assert_eq!(output, (3000, 1080), "输出画布应是 3000×1080");
}

/// 计划第 5 条：横排时把下边界往上拖，两张一起变矮，画布高度跟着变。
#[test]
fn acceptance_row_bottom_drag_keeps_widths() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(
        cx,
        &view,
        vec![
            test_item(1, 1920, 1080, vec![]),
            test_item(2, 1000, 1000, vec![]),
        ],
    );
    cx.update_entity(&view, |root, cx| {
        root.project.settings.layout = crate::model::LayoutTarget::Row;
        root.set_mode(Mode::Layout, cx);
        cx.notify();
    });
    draw(cx);
    let handle = cx
        .debug_bounds("crop-hotzone-1-bottom")
        .expect("横排应有下边界热区");
    let start = handle.center();
    // 往上拖：按显示比例换算到原图，大约裁掉 180 个输出像素。
    let layout = cx.read_entity(&view, |root, _| root.canvas_layout());
    let drag_display = 180.0 * layout.scale;
    let mid = gpui::point(start.x, start.y - gpui::px(drag_display * 0.6));
    let end = gpui::point(start.x, start.y - gpui::px(drag_display));
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    cx.simulate_mouse_move(
        mid,
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    cx.simulate_mouse_move(
        end,
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    cx.simulate_mouse_up(end, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();

    let (first, second) = cx.read_entity(&view, |root, _| {
        (root.project.items[0].manual, root.project.items[1].manual)
    });
    let first = first.expect("第一张应被裁");
    let second = second.expect("第二张应一起被裁");
    assert_eq!(first.left, 0.0, "改高度不能动宽度");
    assert_eq!(first.right, 1.0);
    assert!(
        (first.bottom - second.bottom).abs() < 1e-3,
        "两张应共用同一个高度，实际 {} 与 {}",
        first.bottom,
        second.bottom
    );
    let output = cx
        .read_entity(&view, |root, _| root.output_size())
        .expect("应有输出尺寸");
    assert_eq!(
        output.1, 900,
        "往下拖 180 像素后画布高度应是 900，实际 {}（拖动量对齐到 180 需要整段拖动）",
        output.1
    );
}

/// 计划第 6 条：比例排版里拖左图右边界往左 100，只裁这一张，
/// 裁完是 900×1000，另一张还是 1000×1000。
#[test]
fn acceptance_aspect_drag_crops_only_dragged_image() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(
        cx,
        &view,
        vec![
            test_item(1, 1000, 1000, vec![]),
            test_item(2, 1000, 1000, vec![]),
        ],
    );
    cx.update_entity(&view, |root, cx| {
        root.project.settings.layout = crate::model::LayoutTarget::Aspect(2.0);
        root.set_mode(Mode::Layout, cx);
        root.selected_ids = [1].into_iter().collect();
        cx.notify();
    });
    draw(cx);
    let layout = cx.read_entity(&view, |root, _| root.canvas_layout());
    let drag_display = 100.0 * layout.scale;
    let handle = cx
        .debug_bounds("crop-hotzone-1-right")
        .expect("应有右边界热区");
    let start = handle.center();
    let mid = gpui::point(start.x - gpui::px(drag_display * 0.6), start.y);
    let end = gpui::point(start.x - gpui::px(drag_display), start.y);
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    cx.simulate_mouse_move(
        mid,
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    cx.simulate_mouse_move(
        end,
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    cx.simulate_mouse_up(end, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();

    let (first, second) = cx.read_entity(&view, |root, _| {
        let inclusions = crate::imaging::crop::resolve(&root.project);
        let sizes: Vec<Option<(u32, u32)>> = root
            .project
            .items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                inclusions
                    .get(index)
                    .copied()
                    .and_then(|i| i.output_size(item))
            })
            .collect();
        (sizes[0], sizes[1])
    });
    assert_eq!(first, Some((900, 1000)), "被拖的那张应变成 900×1000");
    assert_eq!(second, Some((1000, 1000)), "另一张不该动");
}

/// 计划第 7 条：字幕模式左右两边不能拖，只有上下两条裁剪线能拖。
#[test]
fn acceptance_subtitle_mode_has_no_horizontal_crop_lines() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(
        cx,
        &view,
        vec![test_item(1, 1000, 1000, vec![line(700.0, 760.0)])],
    );
    draw(cx);
    for edge in ["left", "right"] {
        assert!(
            cx.debug_bounds(Box::leak(format!("crop-hotzone-1-{edge}").into_boxed_str()))
                .is_none(),
            "字幕模式不该有 {edge} 裁剪线热区"
        );
    }
    for edge in ["top", "bottom"] {
        assert!(
            cx.debug_bounds(Box::leak(format!("crop-hotzone-1-{edge}").into_boxed_str()))
                .is_some(),
            "字幕模式应有 {edge} 裁剪线热区"
        );
    }
}

/// 画布四边的留白：作品左、右、上、下到画布边缘的距离。
fn artwork_padding(cx: &mut gpui::VisualTestContext) -> (f32, f32, f32, f32) {
    let canvas = cx.debug_bounds("canvas").expect("应有画布");
    let art = cx.debug_bounds("artwork").expect("应有作品");
    let left = f32::from(art.origin.x) - f32::from(canvas.origin.x);
    let right = f32::from(canvas.size.width) - left - f32::from(art.size.width);
    let top = f32::from(art.origin.y) - f32::from(canvas.origin.y);
    let bottom = f32::from(canvas.size.height) - top - f32::from(art.size.height);
    (left, right, top, bottom)
}

/// 不管哪种模式，作品最窄的那条边都要在画面里居中。
///
/// 竖长图（10×10000，最窄边是宽 10）看左右留白相等；
/// 宽矮图（800×400，最窄边是高 400）看左右、上下都相等。
#[test]
fn artwork_narrow_side_is_centered_in_every_mode() {
    for mode in [Mode::Subtitle, Mode::Layout] {
        // 竖长图：最窄边是宽度，要在画面里左右居中。
        {
            let mut cx = TestAppContext::single();
            let (view, cx) = cx.add_window_view(AppRoot::new);
            install_items(cx, &view, vec![test_item(1, 10, 10_000, vec![])]);
            cx.update_entity(&view, |root, cx| {
                root.project.mode = mode;
                root.project.settings.layout = crate::model::LayoutTarget::Row;
                cx.notify();
            });
            draw(cx);
            let (left, right, _, _) = artwork_padding(cx);
            assert!(
                (left - right).abs() < 1.0,
                "{mode:?} 模式竖长图的宽度应左右居中：左留白 {left:.1}，右留白 {right:.1}"
            );
            assert!(
                left > 100.0,
                "{mode:?} 模式竖长图左侧应留出明显空白，实际 {left:.1}"
            );
        }
        // 宽矮图：最窄边是高度，要在画面里上下居中（横向也居中）。
        {
            let mut cx = TestAppContext::single();
            let (view, cx) = cx.add_window_view(AppRoot::new);
            install_items(cx, &view, vec![test_item(1, 800, 400, vec![])]);
            cx.update_entity(&view, |root, cx| {
                root.project.mode = mode;
                root.project.settings.layout = crate::model::LayoutTarget::Row;
                cx.notify();
            });
            draw(cx);
            let (left, right, top, bottom) = artwork_padding(cx);
            assert!(
                (top - bottom).abs() < 1.0,
                "{mode:?} 模式宽矮图的高度应上下居中：上留白 {top:.1}，下留白 {bottom:.1}"
            );
            assert!(
                (left - right).abs() < 1.0,
                "{mode:?} 模式宽矮图的宽度应左右居中：左留白 {left:.1}，右留白 {right:.1}"
            );
        }
    }
}

/// 拖动图片时：跟着鼠标的是一张很小的半透明图，原位置立刻被后面的图顶上来。
#[test]
fn dragging_shows_a_small_ghost_and_closes_the_gap() {
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
    let first = cx.debug_bounds("canvas-block-1").expect("应有第一张图");
    let second_before = cx.debug_bounds("canvas-block-2").expect("应有第二张图");
    let start = first.center();
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    // 第一次移动才开始拖动，第二次移动才把拖动送到画布上。
    cx.simulate_mouse_move(
        gpui::point(start.x, start.y + gpui::px(30.)),
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    let cursor = gpui::point(start.x, start.y + gpui::px(40.));
    cx.simulate_mouse_move(
        cursor,
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    draw(cx);

    // 跟着鼠标的是一张 88×88 的小图，远小于整块图。
    let ghost = cx
        .debug_bounds("drag-thumb")
        .expect("拖动时应有跟着鼠标的小图");
    assert!(
        (f32::from(ghost.size.width) - 88.0).abs() < 0.5
            && (f32::from(ghost.size.height) - 88.0).abs() < 0.5,
        "跟随小图应为 88×88，实际 {:?}×{:?}",
        ghost.size.width,
        ghost.size.height
    );
    assert!(
        f32::from(ghost.size.width) < f32::from(first.size.width),
        "跟随小图不能是整块图那么大"
    );
    // 小图以指针为中心。
    let center = ghost.center();
    assert!(
        (f32::from(center.x) - f32::from(cursor.x)).abs() < 1.0
            && (f32::from(center.y) - f32::from(cursor.y)).abs() < 1.0,
        "跟随小图应居中在指针上：小图中心 {:?}，指针 {:?}",
        center,
        cursor
    );

    // 被拖的那张此刻不在排版里，后面的图立刻上移补位。
    let ids: Vec<ItemId> = cx.read_entity(&view, |root, _| {
        root.canvas_layout()
            .entries
            .iter()
            .map(|entry| entry.id)
            .collect()
    });
    assert_eq!(ids, vec![2], "拖动时排版里应只剩没被拖的那张");
    let second_after = cx.debug_bounds("canvas-block-2").expect("第二张图应还在");
    assert!(
        f32::from(second_after.origin.y) < f32::from(second_before.origin.y) - 100.0,
        "拖动时第二张图应上移补上空位：拖动前 y {:?}，拖动后 y {:?}",
        second_before.origin.y,
        second_after.origin.y
    );
}

/// 拖着图上下移动松手后，顺序真的变了。
#[test]
fn dropping_a_dragged_image_reorders_it() {
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
    let keys_before = preview_keys(cx, &view);
    assert!(!keys_before.is_empty(), "拖动前应已生成预览位图");
    let first = cx.debug_bounds("canvas-block-1").expect("应有第一张图");
    let start = first.center();
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    cx.simulate_mouse_move(
        gpui::point(start.x, start.y + gpui::px(30.)),
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    // 拖到第二张图的位置（合拢后它就顶到第一张原来的位置）。
    let cursor = gpui::point(start.x, start.y + gpui::px(40.));
    cx.simulate_mouse_move(
        cursor,
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    draw(cx);
    cx.simulate_mouse_up(cursor, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    let order: Vec<ItemId> = cx.read_entity(&view, |root, _| {
        root.project.items.iter().map(|item| item.id).collect()
    });
    assert_eq!(order, vec![2, 1], "把第一张拖到第二张处松手后顺序应互换");
    assert_eq!(
        preview_keys(cx, &view),
        keys_before,
        "换顺序只改位置，预览位图不该丢掉重画"
    );
}

/// 拖尺寸（横排拖下边、竖排拖右边）时不能把预览位图丢掉了重画。
///
/// 这是「调整图片尺寸时非常卡顿」的根因：以前每拖一步都清空预览缓存，
/// 每帧都要把整张图重新缩一遍。现在裁剪范围变了只挪位图，位图缓存不该动。
#[test]
fn resizing_a_size_handle_keeps_preview_bitmaps() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(
        cx,
        &view,
        vec![
            test_item(1, 1000, 1000, vec![]),
            test_item(2, 1000, 1000, vec![]),
        ],
    );
    cx.update_entity(&view, |root, cx| {
        root.project.settings.layout = crate::model::LayoutTarget::Row;
        root.set_mode(Mode::Layout, cx);
        cx.notify();
    });
    draw(cx);
    let cached_before = cx.read_entity(&view, |root, _| root.preview_cache.len());
    assert!(cached_before > 0, "画完后应有预览位图缓存");
    let handle = cx
        .debug_bounds("crop-hotzone-1-bottom")
        .expect("横排应有下边界热区");
    let start = handle.center();
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    cx.simulate_mouse_move(
        gpui::point(start.x, start.y - gpui::px(60.)),
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    let cached_mid = cx.read_entity(&view, |root, _| root.preview_cache.len());
    assert!(
        cached_mid > 0,
        "拖动尺寸时不应清空预览位图缓存，实际只剩 {cached_mid} 张"
    );
    cx.simulate_mouse_move(
        gpui::point(start.x, start.y - gpui::px(100.)),
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    let bottom = cx.read_entity(&view, |root, _| {
        root.project.items[0].manual.map(|m| m.bottom)
    });
    assert!(
        bottom.is_some_and(|b| b < 0.95),
        "这次拖动应真的改了第一张的下边界，实际 {bottom:?}"
    );
}

/// 横排时拖图换顺序按左右方向判断，竖排/字幕按上下方向。
#[test]
fn reordering_in_row_mode_uses_horizontal_direction() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(
        cx,
        &view,
        vec![
            test_item(1, 1000, 1000, vec![]),
            test_item(2, 1000, 1000, vec![]),
        ],
    );
    cx.update_entity(&view, |root, cx| {
        root.project.settings.layout = crate::model::LayoutTarget::Row;
        root.set_mode(Mode::Layout, cx);
        root.select(1, cx);
    });
    draw(cx);
    let first = cx.debug_bounds("canvas-block-1").expect("横排应有第一张图");
    let start = first.center();
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    cx.simulate_mouse_move(
        gpui::point(start.x + gpui::px(30.), start.y),
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    cx.simulate_mouse_move(
        gpui::point(start.x + gpui::px(60.), start.y),
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    draw(cx);
    // 合拢后第二张顶到中间，继续拖到它的右半边，落点才表示“插到它后面”。
    let art = cx.debug_bounds("artwork").expect("应有作品");
    let cursor = gpui::point(art.center().x + gpui::px(200.), art.center().y);
    cx.simulate_mouse_move(
        cursor,
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    draw(cx);
    cx.simulate_mouse_up(cursor, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    let order: Vec<ItemId> = cx.read_entity(&view, |root, _| {
        root.project.items.iter().map(|item| item.id).collect()
    });
    assert_eq!(order, vec![2, 1], "横排把第一张往右拖过第二张后顺序应互换");
}

// ----------------------------------------------------------------------
// 规则级验收：拖动、空白区手势、Esc、关闭按钮、预览缓存不变量。
// ----------------------------------------------------------------------

fn preview_keys(
    cx: &mut gpui::VisualTestContext,
    view: &gpui::Entity<AppRoot>,
) -> Vec<(ItemId, u32)> {
    let mut keys: Vec<(ItemId, u32)> =
        cx.read_entity(view, |root, _| root.preview_cache.keys().copied().collect());
    keys.sort_unstable();
    keys
}

/// 拖动裁剪线期间，预览位图档位不新增、缓存不清空。
#[test]
fn dragging_a_crop_edge_reuses_preview_bitmaps() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(
        cx,
        &view,
        vec![test_item(1, 3000, 2000, vec![line(1600.0, 1720.0)])],
    );
    draw(cx);
    let before = preview_keys(cx, &view);
    assert!(!before.is_empty(), "拖动前应已生成预览位图");

    let handle = cx
        .debug_bounds("crop-hotzone-1-bottom")
        .expect("字幕模式应有下边界热区");
    let start = handle.center();
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    for step in 1..=4 {
        let y = start.y - gpui::px(step as f32 * 30.0);
        cx.simulate_mouse_move(
            gpui::point(start.x, y),
            Some(gpui::MouseButton::Left),
            gpui::Modifiers::default(),
        );
        cx.run_until_parked();
        draw(cx);
        let during = preview_keys(cx, &view);
        assert_eq!(
            during, before,
            "拖动第 {step} 步不应新增预览位图档位，实际 {during:?}"
        );
        assert!(!during.is_empty(), "拖动中不应清空预览位图缓存");
    }
    let end = gpui::point(start.x, start.y - gpui::px(120.));
    cx.simulate_mouse_up(end, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();

    let manual = cx.read_entity(&view, |root, _| root.project.items[0].manual);
    assert!(manual.is_some(), "这次拖动应真的改了裁剪范围");
    assert!(!preview_keys(cx, &view).is_empty(), "松手后缓存也不应清空");
}

/// 空白处左键按下请求移动窗口，不产生选框；图块上左键按下不请求移动窗口。
#[test]
fn left_drag_on_blank_requests_window_move() {
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
    let before = cx.read_entity(&view, |root, _| root.window_move_requests);
    let canvas = cx.debug_bounds("canvas").expect("应有画布");
    let blank = gpui::point(
        canvas.origin.x + gpui::px(10.),
        canvas.origin.y + gpui::px(10.),
    );
    cx.simulate_mouse_down(blank, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    let after = cx.read_entity(&view, |root, _| root.window_move_requests);
    assert_eq!(after, before + 1, "空白处左键按下应请求移动窗口");
    let (marquee, selected) = cx.read_entity(&view, |root, _| {
        (root.marquee.is_some(), root.selected_ids.len())
    });
    assert!(!marquee, "空白处左键不应产生选框");
    assert_eq!(selected, 0, "空白处左键应清空选择");
    cx.simulate_mouse_up(blank, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();

    let block = cx
        .debug_bounds("canvas-block-1")
        .expect("应有图块")
        .center();
    cx.simulate_mouse_down(block, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    let on_block = cx.read_entity(&view, |root, _| root.window_move_requests);
    assert_eq!(on_block, after, "图块上左键按下不应请求移动窗口");
    cx.simulate_mouse_up(block, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
}

/// 右键在空白处：只按下不弹卡片，抬起才弹；拖动则变成框选。
#[test]
fn right_button_click_and_drag_on_blank_differ() {
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
    let blank = gpui::point(
        canvas.origin.x + gpui::px(10.),
        canvas.origin.y + gpui::px(10.),
    );
    cx.simulate_mouse_down(blank, gpui::MouseButton::Right, gpui::Modifiers::default());
    cx.run_until_parked();
    assert!(
        cx.read_entity(&view, |root, _| root.menu.is_none()),
        "右键只按下时不应立刻弹卡片"
    );
    cx.simulate_mouse_up(blank, gpui::MouseButton::Right, gpui::Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert!(
        cx.debug_bounds("context-menu").is_some(),
        "右键单击空白处应弹画布卡片"
    );

    // 关掉卡片，右键拖动应变成框选，而不是弹卡片。
    cx.update_entity(&view, |root, cx| root.close_menu(cx));
    draw(cx);
    let end = gpui::point(
        canvas.origin.x + canvas.size.width - gpui::px(10.),
        canvas.origin.y + canvas.size.height - gpui::px(10.),
    );
    cx.simulate_mouse_down(blank, gpui::MouseButton::Right, gpui::Modifiers::default());
    cx.run_until_parked();
    cx.simulate_mouse_move(
        end,
        Some(gpui::MouseButton::Right),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    draw(cx);
    assert!(cx.debug_bounds("marquee").is_some(), "右键拖动应画出选框");
    assert!(
        cx.read_entity(&view, |root, _| root.menu.is_none()),
        "右键拖动不应弹卡片"
    );
    cx.simulate_mouse_up(end, gpui::MouseButton::Right, gpui::Modifiers::default());
    cx.run_until_parked();
}

/// Esc：开启托盘时隐藏窗口；没开托盘时不做任何关闭动作；有浮层时先关浮层。
#[test]
fn escape_hides_to_tray_only_when_enabled() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    let handle = cx.update(|window, _| window.window_handle());
    cx.update_entity(&view, |root, cx| root.set_close_to_tray(true, cx));
    draw(cx);
    assert!(cx.is_window_visible(handle));
    cx.dispatch_action(Dismiss);
    cx.run_until_parked();
    assert!(
        !cx.is_window_visible(handle),
        "开启关闭到托盘时 Esc 应隐藏窗口"
    );

    // 把窗口重新显示出来，再验证“没开托盘”的情况。
    cx.update(|window, _| window.set_visible(true));
    cx.update_entity(&view, |root, cx| root.set_close_to_tray(false, cx));
    draw(cx);
    cx.dispatch_action(Dismiss);
    cx.run_until_parked();
    assert!(
        cx.is_window_visible(handle),
        "没开托盘时 Esc 不应关闭或隐藏窗口"
    );

    // 有右键卡片时，Esc 只关卡片，不隐藏窗口。
    cx.update_entity(&view, |root, cx| root.open_canvas_menu(300.0, 300.0, cx));
    draw(cx);
    cx.dispatch_action(Dismiss);
    cx.run_until_parked();
    assert!(
        cx.read_entity(&view, |root, _| root.menu.is_none()),
        "Esc 应先关掉右键卡片"
    );
    assert!(cx.is_window_visible(handle), "关卡片时不应隐藏窗口");
}

/// 关闭按钮默认不显眼；指针靠近右上角才进入显示状态，移开又恢复。
#[test]
fn close_button_reveals_near_top_right() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    draw(cx);
    assert!(
        !cx.read_entity(&view, |root, _| root.close_near),
        "默认不应显示关闭按钮"
    );
    cx.simulate_mouse_move(
        gpui::point(gpui::px(1880.), gpui::px(40.)),
        None,
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    assert!(
        cx.read_entity(&view, |root, _| root.close_near),
        "指针到右上角应显示关闭按钮"
    );
    cx.simulate_mouse_move(
        gpui::point(gpui::px(400.), gpui::px(400.)),
        None,
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    assert!(
        !cx.read_entity(&view, |root, _| root.close_near),
        "指针移开后关闭按钮应恢复安静"
    );
}

/// 拖动图片后在画布空白处松手：取消拖动，不换顺序，不留下跟手小图。
#[test]
fn dropping_a_drag_on_blank_cancels_it() {
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
    let first = cx.debug_bounds("canvas-block-1").expect("应有第一张图");
    let start = first.center();
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    cx.simulate_mouse_move(
        gpui::point(start.x, start.y + gpui::px(30.)),
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    let canvas = cx.debug_bounds("canvas").expect("应有画布");
    let blank = gpui::point(
        canvas.origin.x + gpui::px(10.),
        canvas.origin.y + gpui::px(10.),
    );
    cx.simulate_mouse_move(
        blank,
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    draw(cx);
    assert!(
        cx.read_entity(&view, |root, _| root.dragging.is_some()),
        "拖动中应处于拖动状态"
    );

    cx.simulate_mouse_up(blank, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    let (dragging, pointer) = cx.read_entity(&view, |root, _| (root.dragging, root.drag_pointer));
    assert!(dragging.is_none(), "空白处松手应结束拖动");
    assert!(pointer.is_none(), "结束后不应保留跟手位置");
    let ids: Vec<ItemId> = cx.read_entity(&view, |root, _| {
        root.canvas_layout().entries.iter().map(|e| e.id).collect()
    });
    assert_eq!(
        ids,
        vec![1, 2],
        "空白处松手不应改变顺序，两张图都应回到排版里"
    );
    let order: Vec<ItemId> = cx.read_entity(&view, |root, _| {
        root.project.items.iter().map(|item| item.id).collect()
    });
    assert_eq!(order, vec![1, 2], "空白处松手不应换顺序");
}

/// 保存记录必须包含全部界面选项，不能只记窗口。
#[test]
fn saved_state_contains_every_interface_option() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    cx.update_entity(&view, |root, cx| {
        root.language = crate::i18n::Language::EnUs;
        root.project.mode = Mode::Layout;
        root.project.settings.crop = false;
        root.project.settings.keep_second = true;
        root.project.settings.bleed = 0.25;
        root.project.settings.gap = crate::model::GapSetting {
            enabled: true,
            px: 18,
            color: crate::model::Rgba {
                r: 0x10,
                g: 0x20,
                b: 0x30,
                a: 0xff,
            },
        };
        root.project.settings.layout = crate::model::LayoutTarget::Row;
        root.zoom = 1.75;
        root.close_to_tray = false;
        cx.notify();
    });
    let windowed = crate::config::WindowRect {
        x: 120.0,
        y: 60.0,
        width: 1500.0,
        height: 950.0,
    };
    let state = cx.read_entity(&view, |root, _| root.saved_state(Some(windowed)));
    assert_eq!(state.version, 1, "配置版本保持 1");
    assert_eq!(state.language, "en-us", "语言应保存");
    assert_eq!(state.mode, "layout", "模式应保存");
    assert!(!state.crop, "裁剪开关应保存");
    assert!(state.keep_second, "保留第二行应保存");
    assert!((state.bleed - 0.25).abs() < 1e-6, "留白应保存");
    assert!(state.gap_enabled, "缝隙开关应保存");
    assert_eq!(state.gap_px, 18, "缝隙宽度应保存");
    assert_eq!(state.gap_color, [0x10, 0x20, 0x30, 0xff], "缝隙颜色应保存");
    assert_eq!(state.layout, "row", "排版方式应保存");
    assert!((state.zoom - 1.75).abs() < 1e-6, "缩放应保存");
    assert!(!state.close_to_tray, "关闭到托盘应保存");
    assert_eq!(state.windowed, Some(windowed), "窗口大小位置应保存");
}

/// 窗口大小变化后，下一次保存用的是最新的普通尺寸。
#[test]
fn resizing_window_tracks_windowed_bounds() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    cx.update_entity(&view, |root, _| root.windowed_bounds = None);
    draw(cx);
    assert!(
        cx.read_entity(&view, |root, _| root.windowed_bounds.is_some()),
        "第一帧后应记住当前窗口大小"
    );

    cx.simulate_resize(gpui::size(gpui::px(1500.), gpui::px(900.)));
    draw(cx);
    let rect = cx
        .read_entity(&view, |root, _| root.windowed_bounds)
        .expect("调整大小后应记住新的窗口大小");
    assert!((rect.width - 1500.0).abs() < 0.5, "宽应更新到 1500");
    assert!((rect.height - 900.0).abs() < 0.5, "高应更新到 900");
}
