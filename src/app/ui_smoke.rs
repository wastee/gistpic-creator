//! 无头界面测试：不需要显卡，但会真实创建窗口、执行渲染、触发粘贴和字幕检测。
//!
//! 重点覆盖新界面里只有在真实布局下才能验证的东西：胶囊、原地透视裁剪、
//! 接缝拖动、拉框多选、拖动删除。

use std::sync::Arc;

use gpui::{
    AppContext, ClipboardEntry, ClipboardItem, CursorStyle, Image, ImageFormat, TestAppContext,
};
use image::RgbImage;

use super::AppRoot;
use super::actions::{CopyImage, Dismiss, ExportPng, PasteImage};
use crate::model::{
    GapSetting, Item, ItemId, ItemStatus, LayoutTarget, LineBox, Mode, SubtitleBlock,
};
use crate::ui::hover::HoverKey;
use crate::ui::tokens;

/// 读一张本地示例图。
///
/// `示例图片/` 不进仓库（体积大），没放素材的环境返回 `None`，
/// 用到真实截图的用例跳过即可。
fn sample_png(relative: &str) -> Option<Vec<u8>> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    std::fs::read(&path).ok()
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
        source: crate::model::SourceImage::Pixels(Arc::new(RgbImage::from_pixel(
            width,
            height,
            image::Rgb([40, 40, 44]),
        ))),
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
        analysis: Default::default(),
        fingerprint: Default::default(),
    }
}

/// 测试直接整批替换 project.items，绕过了 push_source_with 里 next_id 的自增。
/// 不同步的话，push_image_with 会取到和已有图相同的 id：图块缓存槽位被两张
/// 尺寸不同的图共用、识别结果写错对象，测试会陷进无限重生成和重复识别。
/// 这里把下一次 id 推到项目里最大 id 之后。
fn sync_next_id_for_test(root: &mut AppRoot) {
    let next = root
        .project
        .items
        .iter()
        .map(|item| item.id)
        .max()
        .map_or(1, |max| max.saturating_add(1));
    root.next_id = next;
}

fn install_items(cx: &mut gpui::VisualTestContext, view: &gpui::Entity<AppRoot>, items: Vec<Item>) {
    let ids: Vec<ItemId> = items.iter().map(|item| item.id).collect();
    cx.update_entity(view, |root, cx| {
        root.project.items = items;
        root.selected = ids.first().copied();
        root.selected_ids = ids.iter().copied().collect();
        sync_next_id_for_test(root);
        root.release_all_previews();
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
        .expect("字幕边距滑条应有 30% 档");
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
    let Some(png) = sample_png("示例图片/示例_单行字幕图片/粘贴的图像.png") else {
        return;
    };
    let (view, cx) = cx.add_window_view(AppRoot::new);
    paste_png(cx, png);

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
    // 需求 14：右下角用来换比例的小手柄已经删除，排版模式不应再画它。
    assert!(
        cx.debug_bounds("canvas-corner").is_none(),
        "排版模式不应再出现画布右下角手柄"
    );
    let output = cx.read_entity(&view, |root, _| root.output_size());
    assert_eq!(output, Some((1000, 800)));
}

#[test]
fn hovering_a_block_shows_crop_lines_without_the_ghost() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(
        cx,
        &view,
        vec![test_item(1, 1000, 1000, vec![line(700.0, 760.0)])],
    );
    draw(cx);
    // 需求 3：悬停只说明「这张图可以裁」，不浮出整张原图的预览。
    // debug_bounds 只保留当前这一帧的渲染表（每帧清空），所以这一帧里
    // ghost-1 不在，就说明没悬停时确实不画预览；再移进去看下一帧。
    assert!(
        cx.debug_bounds("ghost-1").is_none(),
        "还没悬停时不该有透视预览"
    );
    let block = cx.debug_bounds("canvas-block-1").expect("应画出一张图");
    cx.simulate_mouse_move(block.center(), None, gpui::Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    // 悬停即时生效：不推进任何时钟，重画一次就该有裁剪线。
    let active = cx.read_entity(&view, |root, _| root.hover_gate.debug_active());
    assert_eq!(active, Some(HoverKey::Item(1)), "激活目标应是这张图");
    assert!(
        cx.debug_bounds("crop-line-top").is_some(),
        "悬停时应画上边界裁剪线"
    );
    assert!(
        cx.debug_bounds("crop-line-bottom").is_some(),
        "悬停时应画下边界裁剪线"
    );
    assert!(
        cx.debug_bounds("block-focus-1").is_some(),
        "需求 8：被作用的那张图应有整圈细亮边"
    );
    assert!(
        cx.debug_bounds("ghost-1").is_none(),
        "需求 3：悬停不该浮出被裁掉的原图"
    );
}

/// 用户要求拖裁剪边时不画整图大小的预览影子：拖着的时候最需要看清裁剪线
/// 落在图片的哪个位置，整张原图垫在下面会挡视线。所以拖动全程都不该出现
/// `ghost-{id}`，突出显示的裁剪条仍然要有。
#[test]
fn dragging_a_crop_edge_does_not_reveal_the_ghost() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(
        cx,
        &view,
        vec![test_item(1, 1000, 1000, vec![line(700.0, 760.0)])],
    );
    draw(cx);
    assert!(
        cx.debug_bounds("ghost-1").is_none(),
        "还没开始拖时不该有透视预览"
    );
    let zone = cx
        .debug_bounds("crop-hotzone-1-bottom")
        .expect("应有下边界热区");
    let start = zone.center();
    cx.simulate_mouse_move(start, None, gpui::Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert!(
        cx.debug_bounds("ghost-1").is_none(),
        "指针只是停在图上时也不该浮预览"
    );
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    cx.simulate_mouse_move(
        gpui::point(start.x, start.y - gpui::px(10.)),
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    cx.simulate_mouse_move(
        gpui::point(start.x, start.y - gpui::px(60.)),
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    draw(cx);
    assert!(
        cx.debug_bounds("ghost-1").is_none(),
        "拖裁剪边时也不该浮出整图预览影子"
    );
    assert!(
        cx.debug_bounds("crop-bar-bottom").is_some(),
        "拖动中的那条边应突出显示"
    );
    cx.simulate_mouse_up(start, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
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
    // 需求 3 之后悬停不再浮预览，判断「停在哪张」用门控状态和聚焦亮边。
    let active = cx.read_entity(&view, |root, _| root.hover_gate.debug_active());
    assert_eq!(
        active,
        Some(HoverKey::Item(1)),
        "指针在第一张上时激活目标应是第一张"
    );
    assert!(
        cx.debug_bounds("block-focus-1").is_some(),
        "第一张应有聚焦亮边"
    );

    // 直接从第一张滑到第二张：第二张的悬停应立刻生效。
    // 注意：gpui 测试工具的 debug_bounds 映射表跨帧累积旧条目，
    // “元素已消失”不能用 `is_none` 断言，所以这里直接验证门控激活状态。
    let second = cx.debug_bounds("canvas-block-2").expect("应画出第二张图");
    cx.simulate_mouse_move(second.center(), None, gpui::Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    let active = cx.read_entity(&view, |root, _| root.hover_gate.debug_active());
    assert_eq!(
        active,
        Some(HoverKey::Item(2)),
        "滑到第二张后激活目标应立刻变成第二张"
    );
    assert!(
        cx.debug_bounds("block-focus-2").is_some(),
        "第二张应有聚焦亮边"
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

/// 字幕模式下多选拖裁剪线：选中的图一起同步到同一段裁剪。
///
/// 字幕模式一条一条改裁剪线，多选时按选中集合一起改，这是用户要的行为。
/// 横排竖排那种「整组一起改」是另一条规则（见 `crop_drag::group_follows`），
/// 只在排版模式、且拖的是该排共用的那条边时生效。
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
    // 两张图先设成同一段手动裁剪，起点一致，这样「同步」和「各改各的」产生的结果
    // 会不同：起点相同、位移相同，同步的结果必然是两张一模一样。
    let start_band = crate::model::ManualBand::clamped(0.0, 1.0, 0.05, 0.95).expect("有效保留带");
    cx.update_entity(&view, |root, cx| {
        root.apply_band_to_items(&[1, 2], start_band);
        root.selected_ids = [1, 2].into_iter().collect();
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
    let second = second.expect("第二张应一起产生手动裁剪");
    assert!(
        first.top > 0.06,
        "第一张的上边应被往下拖，实际 {}",
        first.top
    );
    assert_ne!(first, start_band, "第一张的裁剪应真的变了");
    assert_eq!(first, second, "多选时选中的图应同步到同一段裁剪");
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

    // 需求 11：改间距要按胶囊里的拖动按钮，不是整个胶囊。
    // 从按钮往下拖：间距被拉开，拖动中出现像素提示。
    let start = cx
        .debug_bounds("gap-control-handle")
        .expect("应有拖动按钮")
        .center();
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
    assert!(
        gap.effective_px() <= crate::ui::tokens::GAP_MAX,
        "缝隙不能超过上限 {} 像素",
        crate::ui::tokens::GAP_MAX
    );

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

/// 需求 13：拖间隙时数值提示横排、固定在胶囊正下方、留在视口内。
#[test]
fn gap_bubble_is_horizontal_and_never_covers_artwork() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(cx, &view, stacked_items());
    cx.update_entity(&view, |root, cx| {
        root.project.mode = Mode::Subtitle;
        cx.notify();
    });
    draw(cx);

    let control = cx.debug_bounds("gap-control").expect("应有缝隙控件");
    // 需求 11：改间距要按胶囊里的拖动按钮。
    let start = cx
        .debug_bounds("gap-control-handle")
        .expect("应有拖动按钮")
        .center();
    let end = gpui::point(start.x, start.y + gpui::px(40.));
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.simulate_mouse_move(
        end,
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    draw(cx);

    let bubble = cx.debug_bounds("gap-bubble").expect("拖动中应显示像素提示");
    assert!(
        f32::from(bubble.size.width) >= 56.0 && f32::from(bubble.size.height) <= 28.0,
        "提示应是横排、不是竖着的：{bubble:?}"
    );
    // 需求 13：提示挂在胶囊正下方并水平居中。
    let capsule_mid = f32::from(control.origin.x) + f32::from(control.size.width) / 2.0;
    let label_mid = f32::from(bubble.origin.x) + f32::from(bubble.size.width) / 2.0;
    assert!(
        (label_mid - capsule_mid).abs() < 1.5,
        "提示应在胶囊正下方居中：胶囊中心 {capsule_mid:.1}，提示中心 {label_mid:.1}"
    );
    assert!(
        f32::from(bubble.origin.y) >= f32::from(control.bottom()),
        "提示应在胶囊下方：胶囊底 {:.1}，提示顶 {:.1}",
        f32::from(control.bottom()),
        f32::from(bubble.origin.y)
    );
    let viewport = cx.read_entity(&view, |root, _| root.viewport);
    assert!(
        f32::from(bubble.origin.x) >= 0.0
            && f32::from(bubble.origin.x) + f32::from(bubble.size.width)
                <= f32::from(viewport.width),
        "提示横向要留在视口内"
    );
    assert!(
        f32::from(bubble.origin.y) >= 0.0
            && f32::from(bubble.origin.y) + f32::from(bubble.size.height)
                <= f32::from(viewport.height),
        "提示纵向要留在视口内"
    );
    cx.simulate_mouse_up(end, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
}

/// 需求 1：气泡跟手移动，松手后消失。
#[test]
fn gap_bubble_follows_the_pointer_and_hides_on_release() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(cx, &view, stacked_items());
    cx.update_entity(&view, |root, cx| {
        root.project.mode = Mode::Subtitle;
        cx.notify();
    });
    draw(cx);

    // 需求 11：改间距要按胶囊里的拖动按钮。
    let start = cx
        .debug_bounds("gap-control-handle")
        .expect("应有拖动按钮")
        .center();
    let first = gpui::point(start.x, start.y + gpui::px(30.));
    let second = gpui::point(start.x, start.y + gpui::px(70.));
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::default());
    // GPUI 用越过 DRAG_THRESHOLD 的第一次移动来「激活」拖动，这一步不会派发
    // DragMoveEvent；先走一步建立拖动，后面两次才是真正的跟手移动。
    cx.simulate_mouse_move(
        gpui::point(start.x, start.y + gpui::px(10.)),
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    cx.simulate_mouse_move(
        first,
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    draw(cx);
    let bubble_first = cx.debug_bounds("gap-bubble").expect("第一次移动应有提示");
    cx.simulate_mouse_move(
        second,
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    draw(cx);
    let bubble_second = cx.debug_bounds("gap-bubble").expect("第二次移动应有提示");
    // 需求 13：提示固定在胶囊下方，指针走它不走。
    assert!(
        (f32::from(bubble_second.origin.y) - f32::from(bubble_first.origin.y)).abs() < 1.0,
        "提示应固定在原位、不跟随指针：{bubble_first:?} -> {bubble_second:?}"
    );

    cx.simulate_mouse_up(second, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    // debug_bounds 映射表跨帧残留旧条目，判断「气泡已消失」要读实体状态：
    // 气泡完全由 gap_drag 是否存在决定。
    assert!(
        cx.read_entity(&view, |root, _| root.gap_drag.is_none()),
        "松手后气泡应随拖动状态一起消失"
    );
}

/// 需求 1 + 2：间隙到上限 "100 px" 时气泡仍是横排、不折行。
#[test]
fn gap_bubble_shows_max_value_without_wrapping() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(cx, &view, stacked_items());
    cx.update_entity(&view, |root, cx| {
        root.project.mode = Mode::Subtitle;
        root.project.settings.gap = root.project.settings.gap.with_px(tokens::GAP_MAX);
        cx.notify();
    });
    draw(cx);

    // 需求 11：改间距要按胶囊里的拖动按钮。
    let start = cx
        .debug_bounds("gap-control-handle")
        .expect("应有拖动按钮")
        .center();
    let end = gpui::point(start.x, start.y + gpui::px(8.));
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.simulate_mouse_move(
        end,
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    draw(cx);

    let bubble = cx.debug_bounds("gap-bubble").expect("拖动中应显示像素气泡");
    assert!(
        f32::from(bubble.size.height) <= 28.0,
        "上限值 \"100 px\" 不该把气泡撑成竖排：{bubble:?}"
    );
    let gap = cx.read_entity(&view, |root, _| root.project.settings.gap);
    assert_eq!(gap.effective_px(), tokens::GAP_MAX);
    cx.simulate_mouse_up(end, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
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

    // 需求 11：改间距要按胶囊里的拖动按钮。
    let start = cx
        .debug_bounds("gap-control-handle")
        .expect("排版模式也应有拖动按钮")
        .center();
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

/// 需求 8：外沿（没有紧贴邻居的边）的裁剪热区仍然贴着裁剪线，不让位置。
///
/// 第一张的顶边和第二张的底边是整幅作品的外沿，没有歧义，所以照旧贴边。
#[test]
fn outer_crop_hotzone_stays_flush_with_the_line() {
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
    let top = cx
        .debug_bounds("crop-hotzone-1-top")
        .expect("第一张应有上边界热区");
    assert!(
        (f32::from(top.origin.y) - f32::from(first.origin.y)).abs() < 1.0,
        "外沿的上边界热区应贴到图块顶边：热区 {:?} 图块 {:?}",
        top.origin,
        first.origin
    );
    let bottom = cx
        .debug_bounds("crop-hotzone-2-bottom")
        .expect("第二张应有下边界热区");
    assert!(
        (f32::from(bottom.bottom()) - f32::from(second.bottom())).abs() < 1.0,
        "外沿的下边界热区应贴到图块底边：热区 {:?} 图块 {:?}",
        bottom.bottom(),
        second.bottom()
    );
}

/// 需求 8：紧贴邻居的那条边往图块内缩 SEAM_DEADZONE 的一半（3 像素）。
///
/// 只缩「朝接缝的那个方向」，另一条轴不动：底边的热区宽度仍等于图块宽度，
/// 竖向热区的高度仍等于图块高度。两张图都裁掉一小段、不贴成满幅，是为了让
/// 相邻的那两条边各自留有余量，不会被最小保留范围的夹紧提前顶住。
#[test]
fn seam_crop_hotzone_is_inset_by_half_the_deadzone() {
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
        root.project.items[0].manual = Some(crate::model::ManualBand {
            left: 0.0,
            right: 1.0,
            top: 0.0,
            bottom: 0.8,
        });
        root.project.items[1].manual = Some(crate::model::ManualBand {
            left: 0.0,
            right: 1.0,
            top: 0.2,
            bottom: 1.0,
        });
        cx.notify();
    });
    draw(cx);
    let first = cx.debug_bounds("canvas-block-1").expect("第一张应在画布上");
    let second = cx.debug_bounds("canvas-block-2").expect("第二张应在画布上");
    let inset = f32::from(tokens::SEAM_DEADZONE) / 2.0;
    assert!(inset > 0.0, "需求 8 的死区常量应大于 0");
    let bottom = cx
        .debug_bounds("crop-hotzone-1-bottom")
        .expect("第一张应有下边界热区");
    assert!(
        (f32::from(bottom.bottom()) - f32::from(first.bottom()) + inset).abs() < 1.0,
        "贴接缝的下边界热区应往图内缩 {inset} 像素：热区底 {:?} 图块底 {:?}",
        bottom.bottom(),
        first.bottom()
    );
    // 只缩它自己那条边的方向：横向热区横着仍然铺满整个图块。
    assert!(
        (f32::from(bottom.size.width) - f32::from(first.size.width)).abs() < 1.0,
        "横向热区不该横向缩：热区宽 {} 图块宽 {}",
        bottom.size.width,
        first.size.width
    );
    let top = cx
        .debug_bounds("crop-hotzone-2-top")
        .expect("第二张应有上边界热区");
    assert!(
        (f32::from(top.origin.y) - f32::from(second.origin.y) - inset).abs() < 1.0,
        "贴接缝的上边界热区应往图内缩 {inset} 像素：热区顶 {:?} 图块顶 {:?}",
        top.origin,
        second.origin
    );
    // 内缩只挪位置，不改厚度：贴接缝的上边界热区和外沿的上边界热区一样厚。
    let outer_top = cx
        .debug_bounds("crop-hotzone-1-top")
        .expect("第一张应有外沿上边界热区");
    assert!(
        (f32::from(top.size.height) - f32::from(outer_top.size.height)).abs() < 1.0,
        "内缩不该改热区厚度：接缝侧 {} 外沿 {}",
        top.size.height,
        outer_top.size.height
    );
    assert!(
        (f32::from(top.size.width) - f32::from(second.size.width)).abs() < 1.0,
        "横向热区横着仍应铺满图块：热区宽 {} 图块宽 {}",
        top.size.width,
        second.size.width
    );
}

/// 需求 8：间距大于 0 时两块图之间隔着可见缝隙，热区回到原来的位置，不内缩。
#[test]
fn crop_hotzone_is_flush_again_when_the_gap_is_positive() {
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
        root.project.settings.gap = root.project.settings.gap.with_px(24);
        cx.notify();
    });
    draw(cx);
    let first = cx.debug_bounds("canvas-block-1").expect("第一张应在画布上");
    let bottom = cx
        .debug_bounds("crop-hotzone-1-bottom")
        .expect("第一张应有下边界热区");
    assert!(
        (f32::from(bottom.bottom()) - f32::from(first.bottom())).abs() < 1.0,
        "间距大于 0 时下边界热区应贴回图块底边：热区 {:?} 图块 {:?}",
        bottom.bottom(),
        first.bottom()
    );
}

/// 排版模式同样按需求 8 处理：外沿贴边。
#[test]
fn layout_mode_outer_crop_hotzone_is_flush_too() {
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
    let top = cx
        .debug_bounds("crop-hotzone-1-top")
        .expect("第一张应有上边界热区");
    assert!(
        (f32::from(top.origin.y) - f32::from(first.origin.y)).abs() < 1.0,
        "排版模式外沿的上边界热区应贴到图块顶边：热区 {:?} 图块 {:?}",
        top.origin,
        first.origin
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

/// 需求 1：缩放胶囊常驻显示，读数是「显示像素 / 成品输出像素」的真实比值。
#[test]
fn zoom_chip_reads_the_real_display_ratio() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(cx, &view, vec![test_item(1, 3200, 2400, vec![])]);
    // 3200×2400 的大图在默认窗口里必然被压到 100% 以下，读数得跟着变小。
    let (scale, fit) = cx.read_entity(&view, |root, _| {
        (root.canvas_layout().scale, root.view_scale().is_fit())
    });
    assert!(fit, "默认应是自动适应");
    assert!(scale < 1.0, "大图应被压到 100% 以下，实际 {scale}");
    assert!(
        cx.debug_bounds("zoom-chip").is_some(),
        "缩放胶囊应常驻显示，不再只在缩放值不为 1 时出现"
    );
    assert!(
        cx.debug_bounds("zoom-item-zoom-reading").is_some(),
        "应显示百分比读数"
    );
    assert!(
        cx.debug_bounds("zoom-item-zoom-fit").is_some(),
        "应有「适合画布」点击项"
    );
    assert!(
        cx.debug_bounds("zoom-item-zoom-one-to-one").is_some(),
        "应有「1:1」点击项"
    );
}

/// 需求 1：1:1 让每个输出像素对应一个画布逻辑像素，适合画布回到自动适应。
#[test]
fn zoom_chip_switches_between_fit_and_one_to_one() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(cx, &view, vec![test_item(1, 3200, 2400, vec![])]);
    draw(cx);
    let one_to_one = cx
        .debug_bounds("zoom-item-zoom-one-to-one")
        .expect("应有 1:1 项");
    cx.simulate_click(one_to_one.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    let scale = cx.read_entity(&view, |root, _| root.canvas_layout().scale);
    assert!((scale - 1.0).abs() < 1e-6, "1:1 时比例应是 1.0，实际 {scale}");
    let manual = cx.read_entity(&view, |root, _| root.view_scale().mode.manual());
    assert_eq!(manual, Some(1.0), "1:1 应切到手动比例");
    draw(cx);
    let fit = cx
        .debug_bounds("zoom-item-zoom-fit")
        .expect("应有适合画布项");
    cx.simulate_click(fit.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    let (fit_again, scale) = cx.read_entity(&view, |root, _| {
        (root.view_scale().is_fit(), root.canvas_layout().scale)
    });
    assert!(fit_again, "点「适合画布」应回到自动适应");
    assert!(scale < 1.0, "自动适应下大图仍应被压小，实际 {scale}");
    draw(cx);

    // 需求 1：1:1 下作品必然溢出，必须真的能滚动查看。
    cx.simulate_click(one_to_one.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    let max = cx.read_entity(&view, |root, _| root.canvas_scroll.max_offset());
    assert!(
        f32::from(max.height) > 0.0,
        "1:1 下纵向应可滚动，实际可滚 {}",
        f32::from(max.height)
    );
    assert!(
        f32::from(max.width) > 0.0,
        "1:1 下横向应可滚动，实际可滚 {}",
        f32::from(max.width)
    );
}

/// 需求 1：切到 1:1 时留住视口中心对着的作品位置，画面不跳到别的角落。
#[test]
fn one_to_one_keeps_the_viewport_centre() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(cx, &view, vec![test_item(1, 2000, 5000, vec![])]);
    draw(cx);
    let centre_of = |cx: &mut gpui::VisualTestContext| {
        cx.read_entity(&view, |root, _| {
            let layout = root.canvas_layout();
            let viewport = root.viewport;
            let origin = layout.origin(viewport, root.canvas_scroll.offset());
            (f32::from(viewport.height) / 2.0 - origin.y) / layout.scale.max(1e-6)
        })
    };
    let before = centre_of(cx);
    cx.update_entity(&view, |root, cx| root.action_zoom_one_to_one(cx));
    draw(cx);
    let after = centre_of(cx);
    assert!(
        (after - before).abs() < 80.0,
        "1:1 后视口中心对应的作品位置应基本不变：{before} -> {after}"
    );
}

/// 需求 2：调整图片边缘不能改变缩放，拖动期间和松手之后都锁着。
#[test]
fn resizing_a_block_edge_does_not_change_zoom() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    cx.simulate_resize(gpui::size(gpui::px(960.), gpui::px(2000.)));
    install_items(cx, &view, vec![test_item(1, 1000, 2000, vec![line(600.0, 1400.0)])]);
    draw(cx);
    let (fit, scale) = cx.read_entity(&view, |root, _| {
        (root.view_scale().is_fit(), root.canvas_layout().scale)
    });
    assert!(fit, "起点应是自动适应");
    let handle = cx
        .debug_bounds("crop-hotzone-1-bottom")
        .expect("应能命中底边热区");
    let start = handle.center();
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    let locked = cx.read_entity(&view, |root, _| root.view_scale().mode.manual());
    assert_eq!(locked, Some(scale), "按下边缘时应把当时的比例固定下来");
    // 拖动中把图块改短，自动适应会重算比例，锁定之后不会。
    cx.simulate_mouse_move(
        gpui::point(start.x, start.y - gpui::px(10.)),
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    cx.simulate_mouse_move(
        gpui::point(start.x, start.y - gpui::px(120.)),
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    draw(cx);
    let during = cx.read_entity(&view, |root, _| root.canvas_layout().scale);
    assert!(
        (during - scale).abs() < 1e-6,
        "拖动中比例不能变：按下时 {scale}，拖动中 {during}"
    );
    cx.simulate_mouse_up(start, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    let after = cx.read_entity(&view, |root, _| {
        (root.crop_drag.is_some(), root.canvas_layout().scale)
    });
    assert!(!after.0, "松手后拖动状态应清掉");
    assert!(
        (after.1 - scale).abs() < 1e-6,
        "松手后比例也不能重新拟合：按下时 {scale}，松手后 {}",
        after.1
    );
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

fn clipboard_image_bytes(cx: &mut gpui::VisualTestContext) -> Option<Vec<u8>> {
    let item = cx.read_from_clipboard()?;
    item.entries().iter().find_map(|entry| match entry {
        ClipboardEntry::Image(image) => Some(image.bytes().to_vec()),
        ClipboardEntry::String(_) => None,
    })
}

fn png_dimensions(bytes: &[u8]) -> (u32, u32) {
    let image = image::load_from_memory(bytes).expect("剪贴板里应是 PNG");
    (image.width(), image.height())
}

/// 反复推任务队列，直到判定条件成立。复制/导出在后台算图，测试里只能这样等它回主线程。
fn wait_until(
    cx: &mut gpui::VisualTestContext,
    mut ready: impl FnMut(&mut gpui::VisualTestContext) -> bool,
) -> bool {
    for _ in 0..60 {
        if ready(cx) {
            return true;
        }
        cx.run_until_parked();
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(20));
    }
    ready(cx)
}

fn prepare_layout_window_with_two_images(
    cx: &mut gpui::VisualTestContext,
    view: &gpui::Entity<AppRoot>,
) {
    install_items(
        cx,
        view,
        vec![
            test_item(1, 400, 300, vec![]),
            test_item(2, 300, 200, vec![]),
        ],
    );
    cx.update_entity(view, |root, cx| {
        root.project.settings.layout = LayoutTarget::Row;
        root.project.settings.gap = GapSetting::default();
        root.set_mode(Mode::Layout, cx);
        cx.notify();
    });
    draw(cx);
}

/// 复制按需生成：内容稳定后不预先缓存成品，点复制才在后台编码。
#[test]
fn copy_generates_png_on_demand() {
    let mut app = TestAppContext::single();
    let (view, mut cx) = app.add_window_view(AppRoot::new);
    prepare_layout_window_with_two_images(&mut cx, &view);
    assert!(
        cx.read_entity(&view, |root, _| !root.export_in_flight),
        "刚装好图片时不该已经挂着导出任务"
    );
    cx.dispatch_action(CopyImage);
    assert!(
        wait_until(&mut cx, |cx| clipboard_image_bytes(cx).is_some()),
        "点复制后应把 PNG 写进剪贴板"
    );
    let bytes = clipboard_image_bytes(&mut cx).expect("剪贴板里应有图");
    let size = cx
        .read_entity(&view, |root, _| root.output_size())
        .expect("成品应有尺寸");
    assert_eq!(png_dimensions(&bytes), size, "剪贴板里的图应是完整成品");
    assert!(
        wait_until(&mut cx, |cx| cx.read_entity(&view, |root, _| !root.export_in_flight)),
        "复制完成后不应还挂着导出任务"
    );
}

/// 内容改了以后复制必须反映新内容，不能沿用旧结果。
#[test]
fn copy_follows_content_changes() {
    let mut app = TestAppContext::single();
    let (view, mut cx) = app.add_window_view(AppRoot::new);
    prepare_layout_window_with_two_images(&mut cx, &view);
    cx.update_entity(&view, |root, cx| {
        root.project.settings.gap = GapSetting {
            enabled: true,
            px: 20,
            ..GapSetting::default()
        };
        cx.notify();
    });
    draw(&mut cx);
    cx.dispatch_action(CopyImage);
    assert!(
        wait_until(&mut cx, |cx| clipboard_image_bytes(cx).is_some()),
        "点复制后应把 PNG 写进剪贴板"
    );
    let bytes = clipboard_image_bytes(&mut cx).expect("剪贴板里应有图");
    // 横排统一高度 300：400×300 保持 400 宽，300×200 放大成 450 宽，再加 20 缝隙。
    assert_eq!(png_dimensions(&bytes), (870, 300), "复制结果应反映新的缝隙宽度");
}

/// 导出按需生成，并且直接写进用户选的文件。
#[test]
fn export_writes_png_file_on_demand() {
    let mut app = TestAppContext::single();
    let (view, mut cx) = app.add_window_view(AppRoot::new);
    prepare_layout_window_with_two_images(&mut cx, &view);
    let directory = std::env::temp_dir().join(format!("gistpic-export-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&directory);
    let target = directory.join("成品.png");
    let _ = std::fs::remove_file(&target);
    let save_to = target.clone();
    cx.dispatch_action(ExportPng);
    assert!(cx.did_prompt_for_new_path(), "导出应先弹出保存框");
    cx.simulate_new_path_selection(move |_| Some(save_to));
    assert!(
        wait_until(&mut cx, |cx| cx.read_entity(&view, |root, _| !root.export_in_flight)
            && target.exists()),
        "选好路径后应把 PNG 写到文件"
    );
    let bytes = std::fs::read(&target).expect("导出文件应写出来");
    // 横排统一高度 300：400×300 保持 400 宽，300×200 放大成 450 宽，总宽 850。
    assert_eq!(png_dimensions(&bytes), (850, 300), "导出的应是完整成品");
    let leftovers: Vec<_> = std::fs::read_dir(&directory)
        .expect("临时目录应能读")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name())
        .filter(|name| name.to_string_lossy().ends_with(".part"))
        .collect();
    assert!(leftovers.is_empty(), "成功导出后不该留下临时文件：{leftovers:?}");
    let _ = std::fs::remove_dir_all(&directory);
}

/// 中途有图读不到时整次导出失败：目标文件不能留下半个 PNG，旧文件也不能被清掉。
#[test]
fn export_failure_leaves_no_broken_file() {
    let mut app = TestAppContext::single();
    let (view, mut cx) = app.add_window_view(AppRoot::new);
    prepare_layout_window_with_two_images(&mut cx, &view);
    let directory =
        std::env::temp_dir().join(format!("gistpic-export-fail-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&directory);
    let target = directory.join("成品.png");
    let old = b"old file content".to_vec();
    std::fs::write(&target, &old).expect("先放一个同名旧文件");
    // 第一张记成文件来源，但磁盘上并不存在。
    let missing = directory.join("缺失的图.png");
    let _ = std::fs::remove_file(&missing);
    cx.update_entity(&view, |root, _| {
        let item = &mut root.project.items[0];
        item.source = crate::model::SourceImage::File(missing.clone());
        item.name = "缺失的图.png".to_string();
    });
    let save_to = target.clone();
    cx.dispatch_action(ExportPng);
    assert!(cx.did_prompt_for_new_path(), "导出应先弹出保存框");
    cx.simulate_new_path_selection(move |_| Some(save_to));
    assert!(
        wait_until(&mut cx, |cx| {
            !cx.read_entity(&view, |root, _| root.export_in_flight)
                && cx.read_entity(&view, |root, _| root.status.error)
        }),
        "缺图时导出应失败"
    );
    let text = cx.read_entity(&view, |root, _| root.status.text.to_string());
    assert!(text.contains("缺失的图.png"), "错误提示应带上文件名：{text}");
    let after = std::fs::read(&target).expect("旧文件应原样留着");
    assert_eq!(after, old, "失败后旧文件内容不能被清掉");
    let leftovers: Vec<_> = std::fs::read_dir(&directory)
        .expect("临时目录应能读")
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name())
        .filter(|name| name.to_string_lossy().ends_with(".part"))
        .collect();
    assert!(leftovers.is_empty(), "失败后不该留下临时文件：{leftovers:?}");
    let _ = std::fs::remove_dir_all(&directory);
}

/// 用户按 Ctrl+C（macOS 是 Cmd+C）复制成品，和点胶囊上的复制按钮一样。
#[test]
fn ctrl_c_copies_the_ready_result() {
    let mut app = TestAppContext::single();
    let (view, mut cx) = app.add_window_view(AppRoot::new);
    prepare_layout_window_with_two_images(&mut cx, &view);
    cx.simulate_keystrokes(&crate::platform::shortcut("c"));
    assert!(
        wait_until(&mut cx, |cx| clipboard_image_bytes(cx).is_some()),
        "Ctrl/Cmd+C 应把成品写进剪贴板"
    );
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

/// 横排两张 1000×1000，拖其中一张的下边缘，整排一起收成同一个高度。
///
/// 横排的每张图共用一个高度，所以下边缘是整排共用的那条边，拖它整排都要跟着变；
/// 左右两条边各图独立，不受影响。
#[test]
fn row_bottom_drag_sets_shared_height_on_all_images() {
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
        root.selected_ids = [1].into_iter().collect();
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
    let second = second.expect("整排共用一个高度，第二张也应被裁");
    assert!(
        first.bottom < 0.95,
        "被拖的那张高度应变小，实际 {}",
        first.bottom
    );
    assert_eq!(first, second, "横排拖下边缘应让整排高度一致");
    assert_eq!(first.left, 0.0, "改高度不能动宽度");
    assert_eq!(first.right, 1.0);
}

/// 竖排两张 1000×1000，拖其中一张的右边缘，整列一起收成同一个宽度。
///
/// 竖排的每张图共用一个宽度，所以右边缘是整列共用的那条边；上下两条边各图独立。
#[test]
fn column_right_drag_sets_shared_width_on_all_images() {
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
        // 只选第一张：这样「第二张也变了」只能来自竖排共用宽度这条规则，
        // 不能是「多选一起改」那条分支顺带做出来的。
        root.selected_ids = [1].into_iter().collect();
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
    let second = second.expect("整列共用一个宽度，第二张也应被裁");
    assert!(first.right < 0.95, "被拖的那张宽度应变小，实际 {}", first.right);
    assert_eq!(first, second, "竖排拖右边缘应让整列宽度一致");
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

/// 计划第 4 条：横排 1920×1080 加 1000×1000，两张统一高度 1080，画布 3000×1080。
///
/// 横排把所有图拉到同一高度（取现有最高的那张）：1920×1080 保持 1920×1080，
/// 1000×1000 按原比例放大成 1080×1080，两张上下边齐平、画布不留空白。
/// 横排两张共用同一高度：基准只看每张原图自己的尺寸，与排版方向和窗口宽度无关。
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
    // 这条测的是「横排统一高度」，比例取画布自己算出来的值就行：比例的绝对值
    // 由窗口尺寸决定，和这里要验证的排版行为无关。
    let scale = layout.scale;
    assert!(
        scale > 0.0 && scale <= 1.0,
        "自动适应的比例应在 0 到 1 之间且不放大，实际 {scale}"
    );
    // 两张统一高度：第一张保持自己的比例，第二张放大到和第一张一样高。
    let expected = [
        (1, 1920.0_f32, 1080.0_f32, first),
        (2, 1080.0, 1080.0, second),
    ];
    for (id, want_w, want_h, entry) in expected {
        assert!(
            (entry.display_width - want_w * scale).abs() < 1.0
                && (entry.display_height - want_h * scale).abs() < 1.0,
            "第 {id} 张应显示成 {:.1}×{:.1}，实际 {:.1}×{:.1}",
            want_w * scale,
            want_h * scale,
            entry.display_width,
            entry.display_height
        );
    }
    // 关键对照：两张显示高度必须一样，这正是「横排统一高度」盯着的事。
    assert!(
        (first.display_height - second.display_height).abs() < 0.5,
        "横排两张应一样高：第一张 {:.1}，第二张 {:.1}",
        first.display_height,
        second.display_height
    );
    // 输出尺寸（导出用的像素）：两张缩放后宽度相加 1920 + 1080，高度是统一后的 1080。
    let output = cx
        .read_entity(&view, |root, _| root.output_size())
        .expect("应有输出尺寸");
    assert_eq!(output, (3000, 1080), "输出画布应是 3000×1080");
}

/// 横排时把第一张的下边界往上拖：整排共用高度，两张一起收，宽度都不动。
///
/// 横排的每张图共用一个高度，下边界就是整排共用的那条边，所以两张一起从 1080 收到
/// 900（第二张 1000 等比算成 833）。左右宽度各图独立，拖上下边不动它们。
/// 裁完后横排重新统一高度：两张都按 900 高，宽度分别是 1920 和 1080，画布 3000×900。
///
/// 名字里的 `keeps_widths` 保留：裁剪带宽度不动正是这个测试盯着的事。
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
    let second = second.expect("整排共用高度，第二张也应被裁");
    assert_eq!(first, second, "横排拖下边界应让整排高度一致");
    assert_eq!(first.left, 0.0, "改高度不能动宽度");
    assert_eq!(first.right, 1.0);
    assert!(
        (second.left - 0.0).abs() < 1e-6 && (second.right - 1.0).abs() < 1e-6,
        "第二张的左右边界也不该动，实际 {second:?}"
    );
    // 两张的左右边界都没动，所以输出宽度仍是各自原宽：1920 和 1000。
    let (first_size, second_size) = cx.read_entity(&view, |root, _| {
        let inclusions = crate::imaging::crop::resolve(&root.project);
        let size = |index: usize| {
            inclusions
                .get(index)
                .copied()
                .and_then(|inclusion| inclusion.output_size(&root.project.items[index]))
        };
        (size(0), size(1))
    });
    let (first_w, first_h) = first_size.expect("第一张应参与输出");
    let (second_w, second_h) = second_size.expect("第二张应参与输出");
    assert_eq!(first_w, 1920, "拖下边界不该改宽度");
    assert_eq!(second_w, 1000, "第二张的宽度也不该动");
    assert!(
        (first_h as f32 - 900.0).abs() <= 2.0,
        "第一张应裁到约 900 高，实际 {first_h}"
    );
    assert!(
        (second_h as f32 - 833.0).abs() <= 2.0,
        "第二张应等比收到约 833 高，实际 {second_h}"
    );
    // 输出画布：两张统一到 900 高后，第一张 1920 宽、第二张按比例 1080 宽。
    let output = cx
        .read_entity(&view, |root, _| root.output_size())
        .expect("应有输出尺寸");
    assert_eq!(output, (3000, 900), "输出画布应按统一高度后的排版算");
}

/// 横排统一高度之后，长宽比不同的两张图拖裁剪边仍然跟手：
/// 拖第一张下边往上 60，下边跟手移动、上边和左边不动，整组一起被裁。
#[test]
fn dragging_a_row_edge_with_mixed_aspects_keeps_the_opposite_edge() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    cx.simulate_resize(gpui::size(gpui::px(960.), gpui::px(1600.)));
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
    draw(cx);
    let before = canvas_block_edges(cx, 1);
    drag_crop_edge(cx, 1, "bottom", 0.0, -60.0);
    let after = canvas_block_edges(cx, 1);
    assert!(
        (after.3 - before.3 + 60.0).abs() < 2.0,
        "第一张的下边应跟着指针往上走 60px：拖动前 {:.1}，现在 {:.1}",
        before.3,
        after.3
    );
    assert!(
        (after.1 - before.1).abs() < 2.0,
        "拖下边时上边应钉住不动：拖动前 {:.1}，现在 {:.1}",
        before.1,
        after.1
    );
    assert!(
        (after.0 - before.0).abs() < 2.0,
        "拖下边不该让画面横向漂移：拖动前 {:.1}，现在 {:.1}",
        before.0,
        after.0
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

/// 拖动图片时：跟着鼠标的是一张很小的半透明图，原位置显示为半透明落点占位。
#[test]
fn dragging_shows_a_small_ghost_and_placeholder() {
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

    // 被拖的那张按临时顺序留在排版里，画成半透明占位，而不是整块消失。
    let (ids, placeholder): (Vec<ItemId>, Option<ItemId>) =
        cx.read_entity(&view, |root, _| {
            let layout = root.canvas_layout();
            (
                layout.entries.iter().map(|entry| entry.id).collect(),
                layout
                    .entries
                    .iter()
                    .find(|entry| entry.is_placeholder)
                    .map(|entry| entry.id),
            )
        });
    assert_eq!(ids.len(), 2, "拖动时排版里仍应有两张图：{ids:?}");
    assert_eq!(placeholder, Some(1), "被拖的那张应作为半透明占位");
    assert!(
        cx.debug_bounds("canvas-block-1").is_some(),
        "占位块仍应挂在画布上"
    );
}

/// 需求 9：松手前就按落点顺序显示半透明占位（1 3 2），不必等松手才换。
#[test]
fn placeholder_shows_drop_target_before_release() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    // 三张方形图竖排要 2000 多像素高，窗口得够高才能同时看到第一张和第三张。
    // 无头测试里可以直接把视口调大，不必真的去滚动。
    cx.simulate_resize(gpui::size(gpui::px(960.), gpui::px(3000.)));
    install_items(
        cx,
        &view,
        vec![
            test_item(1, 1000, 1000, vec![]),
            test_item(2, 1000, 1000, vec![]),
            test_item(3, 1000, 1000, vec![]),
        ],
    );
    cx.update_entity(&view, |root, cx| {
        root.project.mode = Mode::Subtitle;
        // 单选第三张，否则多选拖动会整组搬走，落点落在被拖组里就直接不换了。
        root.select(3, cx);
        cx.notify();
    });
    draw(cx);

    let second = cx.debug_bounds("canvas-block-2").expect("应有第二张");
    let third = cx.debug_bounds("canvas-block-3").expect("应有第三张");
    assert!(
        f32::from(third.center().y) > 0.0 && f32::from(third.center().y) < 3000.0,
        "第三张要落在视口里才能拖到：{third:?}"
    );

    let start = third.center();
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    // 第一次移动激活拖动（不再派发 DragMoveEvent），第二次才真正更新落点。
    cx.simulate_mouse_move(
        gpui::point(start.x, start.y + gpui::px(10.)),
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    // 把第三张拖到第一、二张之间：落在第二张的上沿里一点点，
    // 既在第二张中线之上（插入到它前面），又稳稳落在图块内，方便命中放置。
    let between = gpui::point(second.center().x, second.origin.y + gpui::px(6.));
    cx.simulate_mouse_move(
        between,
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    draw(cx);

    let (ids, placeholder): (Vec<ItemId>, Option<ItemId>) =
        cx.read_entity(&view, |root, _| {
            let layout = root.canvas_layout();
            (
                layout.entries.iter().map(|entry| entry.id).collect(),
                layout
                    .entries
                    .iter()
                    .find(|entry| entry.is_placeholder)
                    .map(|entry| entry.id),
            )
        });
    assert_eq!(ids, vec![1, 3, 2], "松手前排版应已按落点排成 1 3 2");
    assert_eq!(placeholder, Some(3), "3 应被标成半透明占位");
    assert!(
        cx.debug_bounds("canvas-block-3").is_some(),
        "占位块应真实挂在画布上"
    );

    // 松手前真实顺序不变。
    let items: Vec<ItemId> = cx.read_entity(&view, |root, _| {
        root.project.items.iter().map(|item| item.id).collect()
    });
    assert_eq!(items, vec![1, 2, 3], "松手前真实顺序不应改变");

    cx.simulate_mouse_up(between, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    let items: Vec<ItemId> = cx.read_entity(&view, |root, _| {
        root.project.items.iter().map(|item| item.id).collect()
    });
    assert_eq!(items, vec![1, 3, 2], "松手后才真正换顺序");
}

/// 需求 8：把图拖到视口边缘后，指针停着不动，画布也要自己继续滚。
#[test]
fn dragging_near_the_edge_auto_scrolls_while_the_pointer_is_still() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    let items: Vec<Item> = (1..=6)
        .map(|id| test_item(id, 1000, 1000, vec![]))
        .collect();
    install_items(cx, &view, items);
    cx.update_entity(&view, |root, cx| {
        root.project.mode = Mode::Subtitle;
        root.select(1, cx);
        cx.notify();
    });
    draw(cx);

    let max = cx.read_entity(&view, |root, _| root.canvas_scroll.max_offset());
    assert!(
        f32::from(max.height) > 0.0,
        "六张方图应该撑出纵向可滚空间：{max:?}"
    );

    let first = cx.debug_bounds("canvas-block-1").expect("应有第一张");
    let start = first.center();
    cx.simulate_mouse_down(
        start,
        gpui::MouseButton::Left,
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    // 第一次移动只负责激活拖动，第二次才真正更新落点。
    cx.simulate_mouse_move(
        gpui::point(start.x, start.y + gpui::px(10.)),
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();

    // 贴到视口下沿；横向避开底部胶囊，让拖动移动一定落在作品上。
    let viewport = cx.update(|window, _| window.viewport_size());
    let edge = gpui::point(
        first.origin.x + gpui::px(40.),
        viewport.height - gpui::px(20.),
    );
    cx.simulate_mouse_move(
        edge,
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    assert!(
        cx.read_entity(&view, |root, _| root.auto_scroll_active),
        "指针压到视口边缘时应挂上自动滚动"
    );

    let before = cx.read_entity(&view, |root, _| root.canvas_scroll.offset());
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(64));
    cx.run_until_parked();
    let after = cx.read_entity(&view, |root, _| root.canvas_scroll.offset());
    assert!(
        f32::from(after.y) < f32::from(before.y),
        "指针不动也要继续往上滚：{before:?} -> {after:?}"
    );

    // 指针回到视口中间，自动滚动要停，停下后不再自己动。
    cx.simulate_mouse_move(
        gpui::point(edge.x, gpui::px(400.)),
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    assert!(
        !cx.read_entity(&view, |root, _| root.auto_scroll_active),
        "离开边缘应停掉自动滚动"
    );
    let stopped = cx.read_entity(&view, |root, _| root.canvas_scroll.offset());
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(64));
    cx.run_until_parked();
    assert_eq!(
        cx.read_entity(&view, |root, _| root.canvas_scroll.offset()),
        stopped,
        "停掉之后不该还在滚"
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
    assert!(
        wait_until(cx, |cx| !tile_keys(cx, &view).is_empty()),
        "拖动前应已生成图块"
    );
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
    assert!(
        !tile_keys(cx, &view).is_empty(),
        "换顺序只改位置，图块不该被清空"
    );
}

/// 在作品块之外松手也要完成排序。
///
/// 用户反馈：字幕模式 5 张图，把第 3 张拖到第 5 张的位置，有时候失败。
/// 落点本身算得对（`drag_over` 已经是 `Some(None)`，表示插到末尾），
/// 但 `on_drop` 只挂在作品块上。指针一旦离开作品块——5 张短图拼起来只有
/// 600 高，视口 804，作品上下各有约 100 像素空白——GPUI 按位置命中的元素
/// 里没有那个监听，松手事件就没人接手，这次拖动被静默丢掉，顺序不变。
///
/// 这条测试把图 3 拖到作品块下边缘以外的空白处松手，断言顺序真的变了。
#[test]
fn dropping_below_the_artwork_still_reorders() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    cx.simulate_resize(gpui::size(gpui::px(1381.), gpui::px(804.)));
    let items: Vec<_> = (1..=5u64)
        .map(|id| test_item(id, 3840, 540, vec![]))
        .collect();
    install_items(cx, &view, items);
    cx.update_entity(&view, |root, cx| {
        root.set_mode(Mode::Subtitle, cx);
        root.select(3, cx);
        cx.notify();
    });
    draw(cx);
    let start = cx.debug_bounds("canvas-block-3").expect("应有第 3 张图").center();
    let artwork = cx.debug_bounds("artwork").expect("应有作品块");
    let viewport_bottom = gpui::px(804.);
    // 落点在作品块之外、视口之内，正是用户拖到底部时会经过的区域。
    let outside_y = artwork.origin.y + artwork.size.height + gpui::px(40.);
    assert!(
        outside_y < viewport_bottom && outside_y > artwork.origin.y + artwork.size.height,
        "这个落点必须在作品块之外、视口之内：{outside_y:?}"
    );
    let cursor = gpui::point(start.x, outside_y);
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    cx.simulate_mouse_move(
        gpui::point(start.x, start.y + gpui::px(30.)),
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
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
    assert_eq!(
        order,
        vec![1, 2, 4, 5, 3],
        "第 3 张拖到作品下边缘之外松手，仍应插到末尾"
    );
}

/// 作品块内松手走作品块自己的 on_drop，窗口兜底不能再提交一次。
///
/// 兜底只在谁都没接手时才动手：作品块接手时会先把 drag_over 清空，兜底看到
/// drag_over 已是 None 就什么都不做。这条把两种松手位置放在一起比，确认
/// 「块内」和「块外」各自只排一次序，结果都正确。
#[test]
fn dropping_inside_and_outside_the_artwork_each_reorder_once() {
    // (落点说明, 相对作品块上边缘的纵向偏移占作品高度的比例, 期望顺序)
    type Case = (&'static str, f32, Vec<ItemId>);
    let cases: Vec<Case> = vec![
        // 作品块内、越过第 5 张的中线：插到末尾。
        ("块内越过最后一张", 0.9, vec![1, 2, 4, 5, 3]),
        // 作品块下方空白：也插到末尾。
        ("块外下方空白", 1.15, vec![1, 2, 4, 5, 3]),
    ];
    for (label, ratio, want) in cases {
        let mut cx = TestAppContext::single();
        let (view, cx) = cx.add_window_view(AppRoot::new);
        cx.simulate_resize(gpui::size(gpui::px(1381.), gpui::px(804.)));
        let items: Vec<_> = (1..=5u64)
            .map(|id| test_item(id, 3840, 540, vec![]))
            .collect();
        install_items(cx, &view, items);
        cx.update_entity(&view, |root, cx| {
            root.set_mode(Mode::Subtitle, cx);
            root.select(3, cx);
            cx.notify();
        });
        draw(cx);
        let start = cx.debug_bounds("canvas-block-3").expect("应有第 3 张图").center();
        let artwork = cx.debug_bounds("artwork").expect("应有作品块");
        let y = artwork.origin.y + artwork.size.height * ratio;
        let cursor = gpui::point(start.x, y);
        cx.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::default());
        cx.run_until_parked();
        cx.simulate_mouse_move(
            gpui::point(start.x, start.y + gpui::px(30.)),
            Some(gpui::MouseButton::Left),
            gpui::Modifiers::default(),
        );
        cx.run_until_parked();
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
        assert_eq!(order, want, "{label} 的顺序应是 {want:?}，实际 {order:?}");
        // 兜底重复提交会把图多插一次（5 张变 5 张但位移不对），这里确认张数没变。
        assert_eq!(order.len(), 5, "{label} 不该多出或少掉图片");
    }
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
    assert!(
        wait_until(cx, |cx| !tile_keys(cx, &view).is_empty()),
        "画完后应有图块缓存"
    );
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
    assert!(
        !tile_keys(cx, &view).is_empty(),
        "拖动裁剪线时不应清空图块缓存"
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

fn tile_keys(
    cx: &mut gpui::VisualTestContext,
    view: &gpui::Entity<AppRoot>,
) -> Vec<(ItemId, (u64, u32, u32))> {
    let mut keys: Vec<(ItemId, (u64, u32, u32))> = cx.read_entity(view, |root, _| {
        root.tile_cache
            .iter()
            .flat_map(|(id, grid)| {
                grid.needed()
                    .iter()
                    .map(move |key| (*id, (key.generation, key.col, key.row)))
            })
            .collect()
    });
    keys.sort_unstable();
    keys
}

/// 需求 5：图块在后台生成，UI 线程只登记任务、不在当帧缩图。
#[test]
fn tiles_are_built_in_the_background() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(
        cx,
        &view,
        vec![test_item(1, 3000, 2000, vec![line(1600.0, 1720.0)])],
    );
    draw(cx);
    // 清掉首帧已经生成的图块，制造一次干净的 miss。
    cx.update_entity(&view, |root, _| root.release_all_previews());
    assert!(
        cx.read_entity(&view, |root, _| root.tile_cache.is_empty()),
        "释放后不该还留着图块"
    );
    draw(cx);
    assert!(
        wait_until(cx, |cx| {
            cx.read_entity(&view, |root, _| {
                root.tile_cache
                    .get(&1)
                    .is_some_and(|grid| grid.counts().0 > 0)
            })
        }),
        "后台任务应把可见图块补进缓存"
    );
    assert!(
        wait_until(cx, |cx| cx.read_entity(&view, |root, _| root.tile_jobs.is_empty())),
        "图块补齐后任务登记应清空"
    );
}

/// 需求 5：识别结束不得清空图块缓存（否则所有 4K 图回前台重画）。
#[test]
fn detection_finished_keeps_tile_cache() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(
        cx,
        &view,
        vec![test_item(1, 3000, 2000, vec![line(1600.0, 1720.0)])],
    );
    draw(cx);
    assert!(
        wait_until(cx, |cx| !tile_keys(cx, &view).is_empty()),
        "识别前应已有图块"
    );
    let before = tile_keys(cx, &view);
    cx.update_entity(&view, |root, cx| root.on_detection_finished(cx));
    cx.run_until_parked();
    assert_eq!(
        tile_keys(cx, &view),
        before,
        "识别结束前后图块集合应完全一致"
    );
}

/// 拖裁剪边不重建图块。
///
/// 图块按整张原图切，裁剪带只是画的时候裁掉外面那部分。所以拖裁剪线时屏幕上的
/// 图块一直沿用同一代，不会一边拖一边重新解码缩放、画面先糊一下再清楚。
#[test]
fn dragging_a_crop_edge_keeps_the_same_tiles() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(
        cx,
        &view,
        vec![test_item(1, 3000, 2000, vec![line(1600.0, 1720.0)])],
    );
    draw(cx);
    assert!(
        wait_until(cx, |cx| !tile_keys(cx, &view).is_empty()),
        "拖动前应已生成图块"
    );
    let before = cx
        .read_entity(&view, |root, _| root.tile_cache.get(&1).map(|grid| grid.generation()))
        .expect("应有图块缓存");

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
        let during = cx
            .read_entity(&view, |root, _| {
                root.tile_cache.get(&1).map(|grid| grid.generation())
            })
            .expect("拖动中图块缓存应还在");
        assert_eq!(during, before, "第 {step} 步拖裁剪线不该换图块代次");
        assert!(!tile_keys(cx, &view).is_empty(), "拖动中不该清空图块缓存");
    }
    let end = gpui::point(start.x, start.y - gpui::px(120.));
    cx.simulate_mouse_up(end, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();

    let manual = cx.read_entity(&view, |root, _| root.project.items[0].manual);
    assert!(manual.is_some(), "这次拖动应真的改了裁剪范围");
    let after = cx
        .read_entity(&view, |root, _| root.tile_cache.get(&1).map(|grid| grid.generation()))
        .expect("松手后图块缓存应还在");
    assert_eq!(after, before, "松手后也不该重建图块");
    assert!(!tile_keys(cx, &view).is_empty(), "松手后缓存也不应清空");
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
    assert_eq!(state.scale_mode, "fit", "自动适应应保存为 fit");
    assert!(!state.close_to_tray, "关闭到托盘应保存");
    assert_eq!(state.windowed, Some(windowed), "窗口大小位置应保存");
}

/// 需求 1：手动比例要能存能读，旧配置缺字段时按自动适应处理。
#[test]
fn saved_state_round_trips_the_manual_scale() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    cx.update_entity(&view, |root, cx| root.action_zoom_one_to_one(cx));
    let state = cx.read_entity(&view, |root, _| root.saved_state(None));
    assert_eq!(state.scale_mode, "manual", "1:1 应保存成手动比例");
    assert!((state.manual_scale - 1.0).abs() < 1e-6, "手动比例应是 1.0");
    let restored = state.restore();
    assert_eq!(
        restored.scale_mode,
        crate::ui::tokens::ScaleMode::Manual(1.0),
        "读回来应还是手动 1:1"
    );
    // 旧配置：没有 scale_mode / manual_scale 两个字段，还留着一个已删掉的 zoom。
    // 那个 zoom 是旧版乘在拟合比例上的相对倍数，现在没有对应含义，直接忽略；
    // 读出来必须回到纯自动适应，不能让升级前按过的倍数继续放大画面。
    let legacy: crate::config::SavedState =
        serde_json::from_str(r#"{"version":1,"zoom":1.4}"#).expect("旧配置应能读出来");
    let restored = legacy.restore();
    assert!(
        restored.scale_mode.is_fit(),
        "旧配置缺字段时应按自动适应处理"
    );
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

// ---------------------------------------------------------------------------
// 导入、识别门控与内存缓存（需求 3 / 5 / 6 / 7）
// ---------------------------------------------------------------------------

/// 纯色图打包成解码结果：指纹与真实解码走同一套算法。
fn decoded_solid(width: u32, height: u32, shade: u8) -> crate::imaging::decode::DecodedImage {
    let rgb = RgbImage::from_pixel(width, height, image::Rgb([shade, shade, shade]));
    let fingerprint = crate::imaging::decode::fingerprint(&rgb);
    crate::imaging::decode::DecodedImage { rgb, fingerprint }
}

/// 排版模式导入的图不识别 OCR，但必须立刻出现在画布上。
#[test]
fn layout_mode_never_starts_detection_and_new_items_are_visible() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    cx.update_entity(&view, |root, cx| {
        root.project.mode = Mode::Layout;
        root.push_image_with("排版.png".into(), decoded_solid(1200, 900, 30), cx);
    });
    draw(cx);
    let (detecting, entries, analysis, busy) = cx.read_entity(&view, |root, _| {
        (
            root.detecting,
            root.canvas_layout()
                .entries
                .iter()
                .map(|entry| entry.id)
                .collect::<Vec<_>>(),
            root.project.items[0].analysis,
            root.is_busy(),
        )
    });
    assert!(!detecting, "排版模式不该启动识别");
    assert_eq!(entries, vec![1], "新图必须立刻出现在画布上，不能被当成待识别而隐藏");
    assert_eq!(analysis, crate::model::AnalysisState::NotNeeded);
    assert!(!busy, "排版模式不识别，不该算忙");
}

/// 删掉再添加同一张图：命中内存里的识别缓存，不再进识别队列。
#[test]
fn readding_deleted_image_hits_detection_cache() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    let block = SubtitleBlock {
        lines: vec![line(700.0, 780.0)],
    };
    let cached_block = block.clone();
    cx.update_entity(&view, |root, cx| {
        root.project.mode = Mode::Subtitle;
        let decoded = decoded_solid(1000, 1000, 60);
        let fingerprint = decoded.fingerprint;
        root.push_image_with("第一次.png".into(), decoded, cx);
        let id = root.project.items[0].id;
        root.apply_detection(id, fingerprint, Ok(Some(cached_block)));
        assert_eq!(root.detection_cache.len(), 1, "识别结果应写进内存缓存");
        // 导入不再自动选中，删除前要显式选中这张图。
        root.selected = Some(id);
        root.selected_ids.clear();
        root.selected_ids.insert(id);
        root.delete_selected_images(cx);
        assert!(root.project.items.is_empty(), "先把这张删干净");
        // 同一份像素再添加一次，id 会变，指纹不变。
        root.push_image_with("第二次.png".into(), decoded_solid(1000, 1000, 60), cx);
    });
    let (analysis, block_after, cache_len, pending) = cx.read_entity(&view, |root, _| {
        let item = &root.project.items[0];
        (
            item.analysis,
            item.block.clone(),
            root.detection_cache.len(),
            root.pending_detection_count(),
        )
    });
    assert_eq!(
        analysis,
        crate::model::AnalysisState::Analyzed,
        "命中缓存应直接是已分析"
    );
    assert_eq!(block_after, Some(block), "识别结果应原样复用");
    assert_eq!(cache_len, 1, "同一张图不该把缓存撑大");
    assert_eq!(pending, 0, "命中缓存不该再排队识别");
}

/// 模式来回切换不改动已有识别结果，缓存也一直在。
#[test]
fn switching_modes_keeps_detection_results() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    let block = SubtitleBlock {
        lines: vec![line(700.0, 780.0)],
    };
    let cached_block = block.clone();
    cx.update_entity(&view, |root, cx| {
        root.project.mode = Mode::Subtitle;
        let decoded = decoded_solid(1000, 1000, 90);
        // 先用同一份像素把缓存预热，导入时就该命中，不进识别队列。
        root.detection_cache
            .insert(decoded.fingerprint, Some(cached_block));
        root.push_image_with("图.png".into(), decoded, cx);
        root.set_mode(Mode::Layout, cx);
        root.set_mode(Mode::Subtitle, cx);
    });
    let (analysis, block_after, cache_len, detecting) = cx.read_entity(&view, |root, _| {
        (
            root.project.items[0].analysis,
            root.project.items[0].block.clone(),
            root.detection_cache.len(),
            root.detecting,
        )
    });
    assert_eq!(analysis, crate::model::AnalysisState::Analyzed);
    assert_eq!(block_after, Some(block), "切换模式不该丢掉识别结果");
    assert_eq!(cache_len, 1, "缓存不该被清空");
    assert!(!detecting, "已识别的图不该再排队");
}

/// 粘贴和选择文件两条真实入口，落地的成品尺寸、两种模式下的比例完全一致。
///
/// 用户问的是「粘贴、导入、拖入是不是都按同一套规则」。拖入要用 gpui 的
/// `ExternalPaths`，它的构造函数只在 crate 内可见，测试里造不出来，所以拖入这一路
/// 另有用例在源码层面盯住「拖入直接转给 open_paths」；这里把两条能真跑的入口
/// 逐项比对，确认它们没有各走一套规则。
#[test]
fn paste_and_open_land_on_the_same_fit() {
    let dir = std::env::temp_dir().join(format!("gistpic-entries-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("建临时目录失败");
    let path = dir.join("拖入的图.png");
    RgbImage::from_pixel(1920, 1080, image::Rgb([30, 30, 34]))
        .save(&path)
        .expect("写测试图片失败");
    let bytes = std::fs::read(&path).expect("读回测试图片失败");

    // 两条入口各走一遍，记下 (成品尺寸, 字幕模式比例, 排版模式比例)。
    // (入口名, (成品尺寸, 字幕模式比例, 排版模式比例))
    type Measured = (&'static str, ((u32, u32), f32, f32));
    let mut measured: Vec<Measured> = Vec::new();
    for entry in ["paste", "open"] {
        let mut cx = TestAppContext::single();
        let (view, cx) = cx.add_window_view(AppRoot::new);
        cx.simulate_resize(gpui::size(gpui::px(1381.), gpui::px(804.)));
        match entry {
            // 粘贴：真的写一次剪贴板，再派发键盘上那条粘贴动作。
            "paste" => {
                cx.write_to_clipboard(ClipboardItem::new_image(&Image::from_bytes(
                    ImageFormat::Png,
                    bytes.clone(),
                )));
                cx.dispatch_action(PasteImage);
            }
            // 选择文件：走动作对应的 `open_images`，再喂进同一批路径。
            "open" => {
                cx.dispatch_action(super::actions::OpenImages);
                cx.update_entity(&view, |root, cx| root.open_paths(vec![path.clone()], cx));
            }
            _ => unreachable!(),
        }
        cx.run_until_parked();
        draw(cx);
        let output = cx
            .read_entity(&view, |root, _| root.output_size())
            .expect("应有成品尺寸");
        let count = cx.read_entity(&view, |root, _| root.project.items.len());
        assert_eq!(count, 1, "{entry} 导入后应有一张图");
        let subtitle_scale = cx.read_entity(&view, |root, _| root.canvas_layout().scale);
        cx.update_entity(&view, |root, cx| {
            root.set_mode(Mode::Layout, cx);
            cx.notify();
        });
        draw(cx);
        let layout_scale = cx.read_entity(&view, |root, _| root.canvas_layout().scale);
        measured.push((entry, (output, subtitle_scale, layout_scale)));
    }

    let (first_entry, first) = measured[0];
    for (entry, value) in &measured[1..] {
        assert_eq!(
            value.0, first.0,
            "{entry} 和 {first_entry} 的成品尺寸应一致"
        );
        assert!(
            (value.1 - first.1).abs() < 1e-4,
            "{entry} 和 {first_entry} 的字幕模式比例应一致：{} 对 {}",
            value.1,
            first.1
        );
        assert!(
            (value.2 - first.2).abs() < 1e-4,
            "{entry} 和 {first_entry} 的排版模式比例应一致：{} 对 {}",
            value.2,
            first.2
        );
    }
    // 两条入口共用的比例：1920×1080 的短边上限是 804 × 0.618 ÷ 1080 ≈ 0.460，
    // 宽度上限是 1381 × 0.618 ÷ 1920 ≈ 0.445。宽度这条更紧，取它。
    let expected = 1381.0 * tokens::GOLDEN_FIT_FRACTION / 1920.0;
    assert!(
        (first.1 - expected).abs() < 1e-4 && (first.2 - expected).abs() < 1e-4,
        "两条入口都应落在 {expected}，实际 字幕 {} / 排版 {}",
        first.1,
        first.2
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// 排版模式下，缩放胶囊的三项和两条导入入口都要正常工作。
///
/// 用户要求「所有模式下的粘贴和导入和拖入」都查一遍。字幕模式那几条用例已经覆盖
/// 了；这里换排版模式，走文件导入（不是粘贴），逐项确认：百分比读数就是真实比例、
/// 「适合画布」按整图最短边算、「1:1」回到一比一，并且都能滚得动。
#[test]
fn layout_mode_zoom_chip_works_after_importing_files() {
    let dir = std::env::temp_dir().join(format!("gistpic-layout-zoom-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("建临时目录失败");
    // 一张宽图一张方图：最短边取大的那个（1080）。
    let wide = dir.join("宽图.png");
    RgbImage::from_pixel(1920, 1080, image::Rgb([20, 20, 24]))
        .save(&wide)
        .expect("写测试图片失败");
    let square = dir.join("方图.png");
    RgbImage::from_pixel(1080, 1080, image::Rgb([30, 30, 34]))
        .save(&square)
        .expect("写测试图片失败");

    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    cx.simulate_resize(gpui::size(gpui::px(1381.), gpui::px(804.)));
    cx.update_entity(&view, |root, cx| {
        root.project.settings.layout = crate::model::LayoutTarget::Row;
        root.set_mode(Mode::Layout, cx);
        cx.notify();
    });
    cx.update_entity(&view, |root, cx| {
        root.open_paths(vec![wide.clone(), square.clone()], cx);
    });
    cx.run_until_parked();
    draw(cx);
    let count = cx.read_entity(&view, |root, _| root.project.items.len());
    assert_eq!(count, 2, "排版模式下导入两张图应都在");

    // 一、读数：胶囊上的百分比就是画布真实比例。
    let layout_scale = cx.read_entity(&view, |root, _| root.canvas_layout().scale);
    assert!(
        cx.debug_bounds("zoom-item-zoom-reading").is_some(),
        "排版模式下也应有百分比读数"
    );
    // 方图 1080×1080 给的短边上限是 804 × 0.618 ÷ 1080 ≈ 0.460；宽图 1920
    // 给的宽度上限是 1381 × 0.618 ÷ 1920 ≈ 0.445。宽度这条更紧，取它。
    let expected = 1381.0 * tokens::GOLDEN_FIT_FRACTION / 1920.0;
    assert!(
        (layout_scale - expected).abs() < 1e-4,
        "排版模式的比例应按两条上限里更紧的宽度上限算成 {expected}，实际 {layout_scale}"
    );
    assert!(
        cx.read_entity(&view, |root, _| root.view_scale().is_fit()),
        "导入后应停在适合画布"
    );

    // 二、1:1：点完必须真的是 1.0，并且溢出到能滚。
    let one_to_one = cx
        .debug_bounds("zoom-item-zoom-one-to-one")
        .expect("排版模式下也应有 1:1 项");
    cx.simulate_click(one_to_one.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    let scale = cx.read_entity(&view, |root, _| root.canvas_layout().scale);
    assert!((scale - 1.0).abs() < 1e-6, "1:1 时比例应是 1.0，实际 {scale}");
    let max = cx.read_entity(&view, |root, _| root.canvas_scroll.max_offset());
    assert!(
        f32::from(max.width) > 0.0,
        "两张图横排一比一必然超宽，应能横向滚动，实际可滚 {}",
        f32::from(max.width)
    );

    // 三、适合画布：点回去要回到纯拟合值，不能留下刚才的 1:1。
    let fit = cx
        .debug_bounds("zoom-item-zoom-fit")
        .expect("排版模式下也应有适合画布项");
    cx.simulate_click(fit.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    let back = cx.read_entity(&view, |root, _| root.canvas_layout().scale);
    assert!(
        (back - expected).abs() < 1e-4,
        "点回适合画布应回到 {expected}，实际 {back}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
/// 拖入文件直接转给 `open_paths`，不另走一套导入逻辑。
///
/// 拖入的入参是 gpui 的 `ExternalPaths`，构造函数只在 gpui crate 内部可见，
/// 测试里造不出这个值，所以这里读源码确认 `on_file_drop` 的函数体只有一条
/// 「把路径交给 `open_paths`」的动作。改成自己解析、自己入队就说明有人给拖入
/// 单开了第二条路，覆盖范围会立刻和粘贴/选择文件分叉。
#[test]
fn dropping_files_only_forwards_to_open_paths() {
    let source = include_str!("../app.rs");
    let body = rust_function_body(source, "pub(crate) fn on_file_drop(");
    assert!(
        body.contains("self.open_paths("),
        "拖入必须转给 open_paths，实际函数体：{body}"
    );
    for forbidden in ["enqueue_decode(", "push_source_with(", "self.project.items.push("] {
        assert!(
            !body.contains(forbidden),
            "拖入不该自己调用 {forbidden}，那样就和另外两条入口分叉了：{body}"
        );
    }
}
/// 连续两次导入按传入顺序落进项目，不随解码快慢重排。
#[test]
fn two_drops_preserve_drop_order() {
    let dir = std::env::temp_dir().join(format!("gistpic-order-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("建临时目录失败");
    let mut paths = Vec::new();
    // 第一张最大、最后一张最小：按解码完成顺序排的话结果会反过来。
    for (index, side) in [900u32, 600, 300].into_iter().enumerate() {
        let path = dir.join(format!("第{}张.png", index + 1));
        RgbImage::from_pixel(side, side, image::Rgb([20, 20, 20]))
            .save(&path)
            .expect("写测试图片失败");
        paths.push(path);
    }

    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    cx.update_entity(&view, |root, cx| root.open_paths(paths.clone(), cx));
    cx.run_until_parked();
    let names: Vec<String> = cx.read_entity(&view, |root, _| {
        root.project.items.iter().map(|item| item.name.clone()).collect()
    });
    assert_eq!(
        names,
        vec!["第1张.png", "第2张.png", "第3张.png"],
        "导入顺序必须等于拖入顺序"
    );

    // 再来一批，接在后面，顺序仍然保持。
    cx.update_entity(&view, |root, cx| root.open_paths(paths.clone(), cx));
    cx.run_until_parked();
    let names: Vec<String> = cx.read_entity(&view, |root, _| {
        root.project.items.iter().map(|item| item.name.clone()).collect()
    });
    assert_eq!(
        names,
        vec![
            "第1张.png", "第2张.png", "第3张.png", "第1张.png", "第2张.png", "第3张.png"
        ],
        "第二批要按传入顺序接在后面"
    );
    let _ = std::fs::remove_dir_all(&dir);
}


/// 第一张还在识别时再导入第二张，两张都要留下，最终都分析完。
#[test]
fn dropping_while_detecting_keeps_both_images() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    cx.update_entity(&view, |root, cx| {
        root.project.mode = Mode::Subtitle;
        root.push_image_with("一.png".into(), decoded_solid(900, 600, 10), cx);
        assert!(
            root.project.items[0].analysis.is_pending(),
            "第一张应已进入识别队列"
        );
        root.push_image_with("二.png".into(), decoded_solid(900, 600, 200), cx);
    });
    cx.run_until_parked();
    let items: Vec<(String, crate::model::AnalysisState)> = cx.read_entity(&view, |root, _| {
        root.project
            .items
            .iter()
            .map(|item| (item.name.clone(), item.analysis))
            .collect()
    });
    assert_eq!(items.len(), 2, "识别期间再导入不能丢图：{items:?}");
    assert!(
        items
            .iter()
            .all(|(_, analysis)| *analysis == crate::model::AnalysisState::Analyzed),
        "两张最终都应分析完成：{items:?}"
    );
}

/// 需求 4/11：字幕模式按住拖图片边缘时，裁剪线要和指针 1:1 走，不能差一截。
#[test]
fn subtitle_edge_drag_lines_follow_pointer_without_offset() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    // 两张竖长图叠起来比默认视口高，加高视口才能按到第二张的下边界。
    cx.simulate_resize(gpui::size(gpui::px(960.), gpui::px(2600.)));
    install_items(
        cx,
        &view,
        vec![
            test_item(1, 1000, 2000, vec![line(1400.0, 1500.0)]),
            test_item(2, 1000, 2000, vec![line(1400.0, 1500.0)]),
        ],
    );
    draw(cx);

    let handle = cx
        .debug_bounds("crop-hotzone-2-bottom")
        .expect("字幕模式第二张应有下边界热区");
    let start = handle.center();
    cx.simulate_mouse_down(
        start,
        gpui::MouseButton::Left,
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    assert!(
        cx.read_entity(&view, |root, _| root.crop_drag.is_some()),
        "按在热区上应进入裁剪拖动"
    );
    // 第一次越过阈值的移动只激活拖动，不派发拖动事件。
    cx.simulate_mouse_move(
        gpui::point(start.x, start.y + gpui::px(10.)),
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    // 再走一小步拿到基准位置（拖动距离从按下的那一刻累计）。
    cx.simulate_mouse_move(
        gpui::point(start.x, start.y + gpui::px(20.)),
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    draw(cx);
    let line_before = cx
        .debug_bounds("crop-line-bottom")
        .expect("拖动中应显示下边界线");

    let step = 20.0;
    cx.simulate_mouse_move(
        gpui::point(start.x, start.y + gpui::px(40.)),
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    draw(cx);
    let line_after = cx
        .debug_bounds("crop-line-bottom")
        .expect("拖动中应继续显示下边界线");
    let moved = f32::from(line_after.origin.y) - f32::from(line_before.origin.y);
    // 磁吸档位最多再把线拉回几个像素，所以留 10px 余量；
    // 旧实现是整张图铺开、原点减两次，偏差是上百像素。
    assert!(
        (moved - step).abs() < 10.0,
        "裁剪线应跟着指针走：指针 {step}px，线走了 {moved}px"
    );
}

/// 拖动某张的上边缘时，别的图块不能被整体顶走（旧的 top padding 补偿会这样）。
#[test]
fn crop_drag_does_not_push_neighbour_blocks() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    cx.simulate_resize(gpui::size(gpui::px(960.), gpui::px(2600.)));
    install_items(
        cx,
        &view,
        vec![
            test_item(1, 1000, 2000, vec![line(1400.0, 1500.0)]),
            test_item(2, 1000, 2000, vec![line(1400.0, 1500.0)]),
        ],
    );
    draw(cx);
    let first_before = cx.debug_bounds("canvas-block-1").expect("应有第一张");
    let second_before = cx.debug_bounds("canvas-block-2").expect("应有第二张");

    let handle = cx
        .debug_bounds("crop-hotzone-2-top")
        .expect("字幕模式第二张应有上边界热区");
    let start = handle.center();
    cx.simulate_mouse_down(
        start,
        gpui::MouseButton::Left,
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    assert!(
        cx.read_entity(&view, |root, _| root.crop_drag.is_some()),
        "按在热区上应进入裁剪拖动"
    );
    cx.simulate_mouse_move(
        gpui::point(start.x, start.y - gpui::px(20.)),
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    draw(cx);

    let first_during = cx.debug_bounds("canvas-block-1").expect("第一张应还在");
    let second_during = cx.debug_bounds("canvas-block-2").expect("第二张应还在");
    assert!(
        (f32::from(first_during.origin.y) - f32::from(first_before.origin.y)).abs() < 0.5,
        "拖第二张的上边缘不该把第一张顶走"
    );
    assert!(
        (f32::from(second_during.origin.y) - f32::from(second_before.origin.y)).abs() < 0.5,
        "拖动期间图块原点应冻结"
    );
}

/// 需求 10：排版模式收窄时图块缓存不该被整个丢掉。
#[test]
fn narrowing_in_layout_mode_keeps_tile_cache() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(cx, &view, vec![test_item(1, 3000, 2000, vec![])]);
    cx.update_entity(&view, |root, cx| {
        root.project.mode = Mode::Layout;
        cx.notify();
    });
    draw(cx);
    assert!(
        wait_until(cx, |cx| !tile_keys(cx, &view).is_empty()),
        "收窄前应已有图块"
    );

    let handle = cx
        .debug_bounds("crop-hotzone-1-right")
        .expect("排版模式应有右边界热区");
    let start = handle.center();
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    for step in 1..=4 {
        cx.simulate_mouse_move(
            gpui::point(start.x - gpui::px(step as f32 * 30.0), start.y),
            Some(gpui::MouseButton::Left),
            gpui::Modifiers::default(),
        );
        cx.run_until_parked();
        draw(cx);
        let has_cache = cx.read_entity(&view, |root, _| root.tile_cache.contains_key(&1));
        assert!(has_cache, "第 {step} 步收窄不该丢掉图块缓存");
    }
}

/// 需求 4/5/6：每条热区按下都必须进入裁剪拖动，且不能请求移动窗口。
#[test]
fn every_crop_hotzone_starts_a_drag_without_moving_the_window() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    cx.simulate_resize(gpui::size(gpui::px(960.), gpui::px(1600.)));
    install_items(
        cx,
        &view,
        vec![
            test_item(1, 1000, 1000, vec![line(400.0, 500.0)]),
            test_item(2, 1000, 1000, vec![line(400.0, 500.0)]),
            test_item(3, 1000, 1000, vec![line(400.0, 500.0)]),
        ],
    );
    draw(cx);
    for id in [1, 2, 3] {
        for edge in ["top", "bottom"] {
            let name = format!("crop-hotzone-{id}-{edge}");
            let (zone_selector, block_selector) = match (id, edge) {
                (1, "top") => ("crop-hotzone-1-top", "canvas-block-1"),
                (1, "bottom") => ("crop-hotzone-1-bottom", "canvas-block-1"),
                (2, "top") => ("crop-hotzone-2-top", "canvas-block-2"),
                (2, "bottom") => ("crop-hotzone-2-bottom", "canvas-block-2"),
                (3, "top") => ("crop-hotzone-3-top", "canvas-block-3"),
                _ => ("crop-hotzone-3-bottom", "canvas-block-3"),
            };
            let Some(bounds) = cx.debug_bounds(zone_selector) else {
                panic!("{name} 不存在");
            };
            // 图块本身也要在画布上：热区挂在图块里，图块没画出来时这条测试就没意义。
            // 原来这里只是为了打印图块坐标，收尾删掉打印后保留这个存在性检查。
            assert!(
                cx.debug_bounds(block_selector).is_some(),
                "canvas-block-{id} 不存在"
            );
            let moves_before = cx.read_entity(&view, |root, _| root.window_move_requests);
            cx.simulate_mouse_down(
                bounds.center(),
                gpui::MouseButton::Left,
                gpui::Modifiers::default(),
            );
            cx.run_until_parked();
            let (dragging, moves_after) = cx.read_entity(&view, |root, _| {
                (root.crop_drag.is_some(), root.window_move_requests)
            });
            assert!(dragging, "按 {name} 应进入裁剪拖动");
            assert_eq!(
                moves_after, moves_before,
                "按 {name} 不该请求移动窗口（热区落在图块外会漏到空白画布）"
            );
            cx.simulate_mouse_up(
                bounds.center(),
                gpui::MouseButton::Left,
                gpui::Modifiers::default(),
            );
            cx.run_until_parked();
        }
    }
}











// ----------------------------------------------------------------------
// 需求 4：图片很窄时，四条边分别都能按到，拖边不会变成拖窗口。
// ----------------------------------------------------------------------

/// 需求 4/5 的公共准备：一张图，按下它某条边的热区，返回实际拿到的边。
///
/// 先把指针移过去、跑一帧再按下：GPUI 只在指针已经悬停在这个元素上时才会
/// 派发按下事件，少了这一步，按下会被直接丢掉，量出来的「按不到」是假的。
/// 返回按下那一刻记下的 `(图号, 边名)`：松手会把拖动状态清掉，所以必须在
/// 按下的同一帧里读出来。第二个返回值是这次按下引起的「拖窗口」次数。
fn press_edge(
    cx: &mut gpui::VisualTestContext,
    view: &gpui::Entity<AppRoot>,
    selector: &'static str,
) -> (Option<(ItemId, String)>, usize) {
    let zone = cx.debug_bounds(selector).expect("这条边的热区应该画出来");
    let target = zone.center();
    let moves_before = cx.read_entity(view, |root, _| root.window_move_requests);
    cx.simulate_mouse_move(target, None, gpui::Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    cx.simulate_mouse_down(target, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    let (got, moves) = cx.read_entity(view, |root, _| {
        (
            root.crop_drag
                .as_ref()
                .map(|state| (state.id, state.edge.name().to_string())),
            root.window_move_requests,
        )
    });
    cx.simulate_mouse_up(target, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    (got, moves - moves_before)
}

/// 需求 4：很窄的图，左右两条边各自按得到，不会拿到对面那条边。
///
/// 早先两条竖热区各占图块整个高度，在四角互相盖住，按右上角会拿到上边。
#[test]
fn narrow_image_both_side_edges_are_grabbable() {
    for width in [150_u32, 50, 20] {
        let mut cx = TestAppContext::single();
        let (view, cx) = cx.add_window_view(AppRoot::new);
        cx.simulate_resize(gpui::size(gpui::px(1200.), gpui::px(1200.)));
        install_items(
            cx,
            &view,
            vec![test_item(1, width, 1000, vec![line(600.0, 700.0)])],
        );
        cx.update_entity(&view, |root, cx| root.set_mode(Mode::Layout, cx));
        draw(cx);
        for (selector, expected) in [
            ("crop-hotzone-1-left", "left"),
            ("crop-hotzone-1-right", "right"),
        ] {
            let (got, window_moves) = press_edge(cx, &view, selector);
            assert_eq!(
                got.as_ref().map(|(_, edge)| edge.as_str()),
                Some(expected),
                "图宽 {width} 时按 {selector} 应拖 {expected} 边，实际 {got:?}"
            );
            assert_eq!(
                window_moves, 0,
                "图宽 {width} 时按 {selector} 不该变成拖窗口"
            );
        }
    }
}

/// 需求 4：很矮的图，上下两条边各自按得到。
#[test]
fn short_image_both_row_edges_are_grabbable() {
    for height in [150_u32, 50, 20] {
        let mut cx = TestAppContext::single();
        let (view, cx) = cx.add_window_view(AppRoot::new);
        cx.simulate_resize(gpui::size(gpui::px(1200.), gpui::px(1200.)));
        install_items(
            cx,
            &view,
            vec![test_item(1, 1000, height, vec![line(20.0, 40.0)])],
        );
        draw(cx);
        for (selector, expected) in [
            ("crop-hotzone-1-top", "top"),
            ("crop-hotzone-1-bottom", "bottom"),
        ] {
            let (got, window_moves) = press_edge(cx, &view, selector);
            assert_eq!(
                got.as_ref().map(|(_, edge)| edge.as_str()),
                Some(expected),
                "图高 {height} 时按 {selector} 应拖 {expected} 边，实际 {got:?}"
            );
            assert_eq!(
                window_moves, 0,
                "图高 {height} 时按 {selector} 不该变成拖窗口"
            );
        }
    }
}

/// 需求 4：排版模式下四条边的热区互不重叠，按哪条拿到哪条。
#[test]
fn layout_mode_four_edges_do_not_steal_each_other() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    cx.simulate_resize(gpui::size(gpui::px(1200.), gpui::px(1200.)));
    install_items(
        cx,
        &view,
        vec![test_item(1, 1000, 1000, vec![])],
    );
    cx.update_entity(&view, |root, cx| root.set_mode(Mode::Layout, cx));
    draw(cx);
    for (selector, expected) in [
        ("crop-hotzone-1-top", "top"),
        ("crop-hotzone-1-bottom", "bottom"),
        ("crop-hotzone-1-left", "left"),
        ("crop-hotzone-1-right", "right"),
    ] {
        let (got, window_moves) = press_edge(cx, &view, selector);
        assert_eq!(got.as_ref().map(|(_, edge)| edge.as_str()), Some(expected), "按 {selector} 实际 {got:?}");
        assert_eq!(window_moves, 0, "按 {selector} 不该变成拖窗口");
    }
}

// ----------------------------------------------------------------------
// 需求 5：最后一张图的下边缘（以及任意被裁得很薄的带）按下去要拖它自己。
// ----------------------------------------------------------------------

/// 需求 5：第 2 张被裁成很薄的一条时，按它的下边缘拖的是它自己，
/// 不会变成第 1 张的下边缘、也不会变成拖窗口。
///
/// 早先薄带的裁剪范围不到原图高度的 1%，`ManualBand::clamped` 把它判成无效，
/// 拖动还没开始就返回；事件继续往下冒泡，被上面那张图的热区接走。
#[test]
fn thin_last_image_bottom_edge_drags_itself() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    // 视口要够高，薄带的下边缘才落在画面里。画面外的热区按不到是正常的，
    // 这里要量的是「看得见的时候按不按得到」。
    cx.simulate_resize(gpui::size(gpui::px(960.), gpui::px(1200.)));
    install_items(
        cx,
        &view,
        vec![
            test_item(1, 1000, 2000, vec![line(500.0, 900.0)]),
            test_item(2, 1000, 2000, vec![line(500.0, 900.0)]),
        ],
    );
    // 第 2 张裁到只剩原图高度 0.5% 的一条：3000 像素高的图上正是「一行 15 像素
    // 的字幕」，这里用 2000 像素高的图，比例更极端。
    cx.update_entity(&view, |root, cx| {
        root.project.items[1].manual = Some(crate::model::ManualBand {
            left: 0.0,
            right: 1.0,
            top: 0.9,
            bottom: 0.905,
        });
        cx.notify();
    });
    draw(cx);
    let (got, window_moves) = press_edge(cx, &view, "crop-hotzone-2-bottom");
    assert_eq!(
        got,
        Some((2, "bottom".to_string())),
        "薄带的下边缘应该能开始拖动它自己，实际 {got:?}"
    );
    assert_eq!(window_moves, 0, "按薄带的下边缘不该变成拖窗口");
}

/// 需求 5：拖动薄带的下边缘时，两张选中的图一起同步到同一段裁剪。
///
/// 两张都只剩原图高度 0.5% 的一条，就是「最后一张图的下边缘」那种情况。
#[test]
fn thin_last_image_edge_drag_syncs_all_images() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    cx.simulate_resize(gpui::size(gpui::px(960.), gpui::px(1200.)));
    install_items(
        cx,
        &view,
        vec![
            test_item(1, 1000, 2000, vec![line(500.0, 900.0)]),
            test_item(2, 1000, 2000, vec![line(500.0, 900.0)]),
        ],
    );
    // 两张都裁到只剩原图高度 0.5% 的一条，就是「最后一张图的下边缘」那种情况。
    // 两张起点相同、拖动量相同，同步之后必然一模一样；如果这一拖只作用在
    // 被按的那张上，第 1 张就会停在 0.905 不动。
    cx.update_entity(&view, |root, cx| {
        let thin = crate::model::ManualBand {
            left: 0.0,
            right: 1.0,
            top: 0.9,
            bottom: 0.905,
        };
        root.project.items[0].manual = Some(thin);
        root.project.items[1].manual = Some(thin);
        // 两张都被选中：字幕模式下选中的图一起改。
        root.selected_ids = [1, 2].into_iter().collect();
        cx.notify();
    });
    draw(cx);
    let zone = cx
        .debug_bounds("crop-hotzone-2-bottom")
        .expect("第 2 张应有下边缘热区");
    let start = zone.center();
    cx.simulate_mouse_move(start, None, gpui::Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    // 往下拖：这条带已经比最小保留范围还窄，只许往外放，所以下边缘往外走。
    // 第一次移动才开始算拖动，第二次移动才把拖动量送进裁剪逻辑，所以走两步，
    // 而且两步位置要不同，否则第二次会被当成原地不动丢掉。
    let mut end = start;
    for step in 1..=4 {
        end = gpui::point(start.x, start.y + gpui::px(step as f32 * 20.0));
        cx.simulate_mouse_move(end, Some(gpui::MouseButton::Left), gpui::Modifiers::default());
        cx.run_until_parked();
    }
    cx.simulate_mouse_up(end, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    let (first, second) = cx.read_entity(&view, |root, _| {
        (root.project.items[0].manual, root.project.items[1].manual)
    });
    let first = first.expect("第 1 张的裁剪范围不该被丢掉");
    let second = second.expect("第 2 张的裁剪范围不该被丢掉");
    assert!(
        second.bottom > 0.905,
        "往外拖下边缘应把保留范围放大，实际 {second:?}"
    );
    assert_eq!(second.top, 0.9, "拖下边缘不能动上边缘");
    assert_eq!(second.left, 0.0, "纵向拖动不能动横向");
    assert_eq!(second.right, 1.0);
    assert_eq!(first, second, "两张都选中时，这一拖应把两张同步到同一段裁剪");
}

// ----------------------------------------------------------------------
// 需求 6：拖动某张图的上边缘时，画面不会突然跳到第一张的视角。
// ----------------------------------------------------------------------

/// 需求 6：滚到下面的图、拖第 3 张的上边缘往里收，视口位置只跟着指针走，
/// 不会被拽回作品开头。
///
/// 早先裁剪让作品变矮，滚动区每帧把偏移夹回变小的范围，画面一股一股地往上冲。
#[test]
fn dragging_third_top_edge_does_not_jump_to_the_first_image() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    cx.simulate_resize(gpui::size(gpui::px(960.), gpui::px(600.)));
    install_items(
        cx,
        &view,
        vec![
            test_item(1, 1000, 3000, vec![]),
            test_item(2, 1000, 3000, vec![]),
            test_item(3, 1000, 3000, vec![]),
        ],
    );
    cx.update_entity(&view, |root, cx| root.select(3, cx));
    draw(cx);
    draw(cx);
    // 滚到「第 3 张的顶边刚好在视口上方一点」，也就是用户正在看第 3 张的位置。
    cx.update_entity(&view, |root, cx| {
        let layout = root.canvas_layout();
        let third = layout.entry(3).expect("应有第 3 张").rect.y;
        root.canvas_scroll
            .set_offset(gpui::point(gpui::px(0.), gpui::px(20.0 - third)));
        cx.notify();
    });
    draw(cx);
    let before = cx.read_entity(&view, |root, _| {
        let layout = root.canvas_layout();
        let origin = layout.origin(root.viewport, root.canvas_scroll.offset());
        (
            f32::from(root.canvas_scroll.offset().y),
            origin.y + layout.entry(1).expect("应有第 1 张").rect.y,
            origin.y + layout.entry(3).expect("应有第 3 张").rect.bottom(),
        )
    });
    let zone = cx
        .debug_bounds("crop-hotzone-3-top")
        .expect("第 3 张应有上边缘热区");
    let start = zone.center();
    cx.simulate_mouse_move(start, None, gpui::Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    let dragged = cx.read_entity(&view, |root, _| {
        root.crop_drag
            .as_ref()
            .map(|state| (state.id, state.edge.name().to_string()))
    });
    assert_eq!(
        dragged,
        Some((3, "top".to_string())),
        "按第 3 张的上边缘应拖它自己的上边，实际 {dragged:?}"
    );
    // 往下拖 10 步：拖上边缘时，上方空间跟着指针往下走，第 3 张的下边钉住不动，
    // 留白能吸收的位移不该动滚动位置，更不该把视口拽回作品开头。
    // 每步 25px，指针不会走到视口底边触发自动滚动，只看留白和锚点本身。
    let mut last = start;
    for step in 1..=10 {
        last = gpui::point(start.x, start.y + gpui::px(step as f32 * 25.0));
        cx.simulate_mouse_move(
            last,
            Some(gpui::MouseButton::Left),
            gpui::Modifiers::default(),
        );
        cx.run_until_parked();
        draw(cx);
        let (scroll_y, first_top, third_bottom) = cx.read_entity(&view, |root, _| {
            let layout = root.canvas_layout();
            let origin = layout.origin(root.viewport, root.canvas_scroll.offset());
            (
                f32::from(root.canvas_scroll.offset().y),
                origin.y + layout.entry(1).expect("应有第 1 张").rect.y,
                origin.y + layout.entry(3).expect("应有第 3 张").rect.bottom(),
            )
        });
        // 按下后的第一次移动只用来越过拖动判定，还没派发拖动事件，所以第 1 步不动。
        let expected_down = if step == 1 { 0.0 } else { step as f32 * 25.0 };
        assert!(
            (first_top - before.1 - expected_down).abs() < 2.0,
            "第 {step} 步：上方空间应跟着指针下移 {expected_down:.0}px，实际从 {:.1} 到 {first_top:.1}",
            before.1
        );
        assert!(
            (third_bottom - before.2).abs() < 1.0,
            "第 {step} 步：被拖边的对面（第 3 张下边）应钉住不动（拖动前 {:.1}，现在 {third_bottom:.1}）",
            before.2
        );
        assert!(
            (scroll_y - before.0).abs() < 0.5,
            "第 {step} 步：留白能吸收的位移不该动滚动位置（拖动前 {:.1}，现在 {scroll_y:.1}）",
            before.0
        );
    }
    cx.simulate_mouse_up(last, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
}

/// 图块在窗口里的四条边位置：(左, 上, 右, 下)。
fn canvas_block_edges(
    cx: &mut gpui::VisualTestContext,
    id: crate::model::ItemId,
) -> (f32, f32, f32, f32) {
    let bounds = debug_bounds_dynamic(cx, format!("canvas-block-{id}")).expect("画布上应有这张图");
    let left = f32::from(bounds.origin.x);
    let top = f32::from(bounds.origin.y);
    (
        left,
        top,
        left + f32::from(bounds.size.width),
        top + f32::from(bounds.size.height),
    )
}

/// 按住一条裁剪边，沿 (dx, dy) 拖过去。
///
/// 按下之后先走一小步：GPUI 的拖动判定要用第一步把拖动激活，这一步不派发
/// 拖动事件；第二步才按起点算总位移，所以真正生效的位移就是 (dx, dy)。
fn drag_crop_edge(
    cx: &mut gpui::VisualTestContext,
    id: crate::model::ItemId,
    edge: &str,
    dx: f32,
    dy: f32,
) {
    let start = debug_bounds_dynamic(cx, format!("crop-hotzone-{id}-{edge}"))
        .unwrap_or_else(|| panic!("第 {id} 张图应有 {edge} 裁剪边热区"))
        .center();
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    let activation = gpui::point(start.x + gpui::px(dx * 0.1), start.y + gpui::px(dy * 0.1));
    cx.simulate_mouse_move(
        activation,
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    let end = gpui::point(start.x + gpui::px(dx), start.y + gpui::px(dy));
    cx.simulate_mouse_move(
        end,
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    draw(cx);
}

/// 用户反馈的原始场景：字幕模式 5 张图，拖第 3 张的上边缘。
///
/// 规则是「拖哪条边，画面就往哪条边的方向伸缩，对面不动」：
/// 上方两张跟着上边往上走，第 3 张的上边跟着指针走，
/// 第 3 张的下边和下面两张留在原来的屏幕位置。
#[test]
fn dragging_a_subtitle_top_edge_moves_the_space_above_it() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    cx.simulate_resize(gpui::size(gpui::px(960.), gpui::px(1600.)));
    install_items(
        cx,
        &view,
        (1..=5)
            .map(|id| test_item(id, 1000, 3000, vec![line(1400.0, 1500.0)]))
            .collect(),
    );
    cx.update_entity(&view, |root, cx| {
        root.set_mode(Mode::Subtitle, cx);
        root.select(3, cx);
        cx.notify();
    });
    draw(cx);
    let before: Vec<(f32, f32, f32, f32)> = (1..=5).map(|id| canvas_block_edges(cx, id)).collect();
    drag_crop_edge(cx, 3, "top", 0.0, -60.0);
    let after: Vec<(f32, f32, f32, f32)> = (1..=5).map(|id| canvas_block_edges(cx, id)).collect();

    let moved = -60.0;
    assert!(
        (after[2].1 - before[2].1 - moved).abs() < 1.5,
        "第 3 张的上边应跟着指针走 {moved}px：拖动前 {:.1}，现在 {:.1}",
        before[2].1,
        after[2].1
    );
    assert!(
        (after[2].3 - before[2].3).abs() < 1.5,
        "拖上边时第 3 张的下边应钉住不动：拖动前 {:.1}，现在 {:.1}",
        before[2].3,
        after[2].3
    );
    for id in [0, 1] {
        assert!(
            (after[id].1 - before[id].1 - moved).abs() < 1.5,
            "第 {} 张在上方，应跟着上边一起往上走 {moved}px：拖动前 {:.1}，现在 {:.1}",
            id + 1,
            before[id].1,
            after[id].1
        );
    }
    for id in [3, 4] {
        assert!(
            (after[id].1 - before[id].1).abs() < 1.5,
            "第 {} 张在下方，拖上边时不该动：拖动前 {:.1}，现在 {:.1}",
            id + 1,
            before[id].1,
            after[id].1
        );
    }
    for id in 0..5 {
        assert!(
            (after[id].0 - before[id].0).abs() < 1.0,
            "拖上边不该让第 {} 张左右漂：拖动前 {:.1}，现在 {:.1}",
            id + 1,
            before[id].0,
            after[id].0
        );
    }
}

/// 字幕模式拖第 3 张的下边缘：上方不动，下方空间跟着往下。
#[test]
fn dragging_a_subtitle_bottom_edge_moves_the_space_below_it() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    cx.simulate_resize(gpui::size(gpui::px(960.), gpui::px(1600.)));
    install_items(
        cx,
        &view,
        (1..=5)
            .map(|id| test_item(id, 1000, 3000, vec![line(1400.0, 1500.0)]))
            .collect(),
    );
    cx.update_entity(&view, |root, cx| {
        root.set_mode(Mode::Subtitle, cx);
        root.select(3, cx);
        cx.notify();
    });
    draw(cx);
    let before: Vec<(f32, f32, f32, f32)> = (1..=5).map(|id| canvas_block_edges(cx, id)).collect();
    drag_crop_edge(cx, 3, "bottom", 0.0, 60.0);
    let after: Vec<(f32, f32, f32, f32)> = (1..=5).map(|id| canvas_block_edges(cx, id)).collect();

    let moved = 60.0;
    assert!(
        (after[2].3 - before[2].3 - moved).abs() < 1.5,
        "第 3 张的下边应跟着指针走 {moved}px：拖动前 {:.1}，现在 {:.1}",
        before[2].3,
        after[2].3
    );
    assert!(
        (after[2].1 - before[2].1).abs() < 1.5,
        "拖下边时第 3 张的上边应钉住不动：拖动前 {:.1}，现在 {:.1}",
        before[2].1,
        after[2].1
    );
    for id in [0, 1] {
        assert!(
            (after[id].1 - before[id].1).abs() < 1.5,
            "第 {} 张在上方，拖下边时不该动：拖动前 {:.1}，现在 {:.1}",
            id + 1,
            before[id].1,
            after[id].1
        );
    }
    for id in [3, 4] {
        assert!(
            (after[id].1 - before[id].1 - moved).abs() < 1.5,
            "第 {} 张在下方，应跟着下边一起往下走 {moved}px：拖动前 {:.1}，现在 {:.1}",
            id + 1,
            before[id].1,
            after[id].1
        );
    }
    for id in 0..5 {
        assert!(
            (after[id].0 - before[id].0).abs() < 1.0,
            "拖下边不该让第 {} 张左右漂：拖动前 {:.1}，现在 {:.1}",
            id + 1,
            before[id].0,
            after[id].0
        );
    }
}

/// 排版模式三种排法、四条边，都遵守同一条规则：
/// 被拖的那条边跟着指针走，对面那条边留在原来的屏幕位置，另一条轴不动。
///
/// 横排和竖排再各拖一张排在后头的图：它的左/上位置不是 0，能验证锚点按
/// 那张图自己的边算，而不是碰巧贴着画布边缘。
#[test]
fn every_layout_edge_keeps_its_opposite_edge_in_place() {
    // 比例模式里改一张图的宽高比会重新分组，只验证第一张；
    // 横排、竖排每张图各占自己的位置，两张都拖一遍。
    let cases = [
        (crate::model::LayoutTarget::Aspect(1.0), 1),
        (crate::model::LayoutTarget::Row, 1),
        (crate::model::LayoutTarget::Row, 2),
        (crate::model::LayoutTarget::Column, 1),
        (crate::model::LayoutTarget::Column, 2),
    ];
    for (target, dragged) in cases {
        for edge in ["top", "bottom", "left", "right"] {
            let mut cx = TestAppContext::single();
            let (view, cx) = cx.add_window_view(AppRoot::new);
            cx.simulate_resize(gpui::size(gpui::px(960.), gpui::px(1600.)));
            install_items(
                cx,
                &view,
                vec![
                    test_item(1, 400, 400, vec![]),
                    test_item(2, 400, 400, vec![]),
                ],
            );
            cx.update_entity(&view, |root, cx| {
                root.project.settings.layout = target;
                root.set_mode(Mode::Layout, cx);
                root.selected_ids = [dragged].into_iter().collect();
                cx.notify();
            });
            draw(cx);
            draw(cx);
            let before = canvas_block_edges(cx, dragged);
            // 初始保留带是整张图，往外拖会被「已经顶住」挡住，所以一律往里拖 60px。
            let (dx, dy) = match edge {
                "top" => (0.0, 60.0),
                "bottom" => (0.0, -60.0),
                "left" => (60.0, 0.0),
                _ => (-60.0, 0.0),
            };
            drag_crop_edge(cx, dragged, edge, dx, dy);
            let after = canvas_block_edges(cx, dragged);
            let label = format!("{target:?} 第 {dragged} 张拖 {edge} 边");

            match edge {
                "top" => {
                    assert!(
                        (after.1 - before.1 - 60.0).abs() < 2.0,
                        "{label}：上边应跟着指针往下走 60px，实际 {:.1} → {:.1}",
                        before.1,
                        after.1
                    );
                    assert!(
                        (after.3 - before.3).abs() < 2.0,
                        "{label}：下边应钉住不动，实际 {:.1} → {:.1}",
                        before.3,
                        after.3
                    );
                    assert!(
                        (after.0 - before.0).abs() < 2.0,
                        "{label}：左右不应漂，实际 {:.1} → {:.1}",
                        before.0,
                        after.0
                    );
                }
                "bottom" => {
                    assert!(
                        (after.3 - before.3 + 60.0).abs() < 2.0,
                        "{label}：下边应跟着指针往上走 60px，实际 {:.1} → {:.1}",
                        before.3,
                        after.3
                    );
                    assert!(
                        (after.1 - before.1).abs() < 2.0,
                        "{label}：上边应钉住不动，实际 {:.1} → {:.1}",
                        before.1,
                        after.1
                    );
                    assert!(
                        (after.0 - before.0).abs() < 2.0,
                        "{label}：左右不应漂，实际 {:.1} → {:.1}",
                        before.0,
                        after.0
                    );
                }
                "left" => {
                    assert!(
                        (after.0 - before.0 - 60.0).abs() < 2.0,
                        "{label}：左边应跟着指针往右走 60px，实际 {:.1} → {:.1}",
                        before.0,
                        after.0
                    );
                    assert!(
                        (after.2 - before.2).abs() < 2.0,
                        "{label}：右边应钉住不动，实际 {:.1} → {:.1}",
                        before.2,
                        after.2
                    );
                    assert!(
                        (after.1 - before.1).abs() < 2.0,
                        "{label}：上下不应漂，实际 {:.1} → {:.1}",
                        before.1,
                        after.1
                    );
                }
                _ => {
                    assert!(
                        (after.2 - before.2 + 60.0).abs() < 2.0,
                        "{label}：右边应跟着指针往左走 60px，实际 {:.1} → {:.1}",
                        before.2,
                        after.2
                    );
                    assert!(
                        (after.0 - before.0).abs() < 2.0,
                        "{label}：左边应钉住不动，实际 {:.1} → {:.1}",
                        before.0,
                        after.0
                    );
                    assert!(
                        (after.1 - before.1).abs() < 2.0,
                        "{label}：上下不应漂，实际 {:.1} → {:.1}",
                        before.1,
                        after.1
                    );
                }
            }
        }
    }
}

/// 作品比视口高、已经滚到中间时，把上边缘往上拖：
/// 上方留白只剩最小留白，吸收不完的位移由滚动补，下面的图仍然不动。
#[test]
fn dragging_a_top_edge_up_while_scrolled_scrolls_the_view() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    cx.simulate_resize(gpui::size(gpui::px(960.), gpui::px(600.)));
    install_items(
        cx,
        &view,
        (1..=4)
            .map(|id| test_item(id, 1000, 3000, vec![line(200.0, 2800.0)]))
            .collect(),
    );
    cx.update_entity(&view, |root, cx| {
        // 给每张图一条很高的保留带：作品比视口高才滚得动，同时上边还留着余量，
        // 往上拖不会马上撞到图片边界。
        let band = crate::model::ManualBand::clamped(0.0, 1.0, 0.15, 0.95).expect("有效保留带");
        for item in root.project.items.iter_mut() {
            item.manual = Some(band);
            item.keep_whole = false;
        }
        root.set_mode(Mode::Subtitle, cx);
        root.select(3, cx);
        cx.notify();
    });
    draw(cx);
    // 滚到第 3 张的上边在视口中间：指针离视口上下边都远，不会触发自动滚动。
    cx.update_entity(&view, |root, cx| {
        let layout = root.canvas_layout();
        let third = layout.entry(3).expect("应有第 3 张").rect.y;
        let padding = f32::from(layout.top_padding(root.viewport));
        root.canvas_scroll
            .set_offset(gpui::point(gpui::px(0.), gpui::px(300.0 - padding - third)));
        cx.notify();
    });
    draw(cx);
    let before = cx.read_entity(&view, |root, _| {
        let layout = root.canvas_layout();
        let origin = layout.origin(root.viewport, root.canvas_scroll.offset());
        let third = layout.entry(3).expect("应有第 3 张");
        let fourth = layout.entry(4).expect("应有第 4 张");
        (
            f32::from(root.canvas_scroll.offset().y),
            origin.y + third.rect.y,
            origin.y + third.rect.bottom(),
            origin.y + fourth.rect.y,
        )
    });
    drag_crop_edge(cx, 3, "top", 0.0, -60.0);
    let after = cx.read_entity(&view, |root, _| {
        let layout = root.canvas_layout();
        let origin = layout.origin(root.viewport, root.canvas_scroll.offset());
        let third = layout.entry(3).expect("应有第 3 张");
        let fourth = layout.entry(4).expect("应有第 4 张");
        (
            f32::from(root.canvas_scroll.offset().y),
            origin.y + third.rect.y,
            origin.y + third.rect.bottom(),
            origin.y + fourth.rect.y,
        )
    });
    assert!(
        (after.1 - before.1 + 60.0).abs() < 1.5,
        "第 3 张的上边应跟着指针往上走 60px：拖动前 {:.1}，现在 {:.1}",
        before.1,
        after.1
    );
    assert!(
        (after.2 - before.2).abs() < 1.5,
        "拖上边时第 3 张的下边应钉住不动：拖动前 {:.1}，现在 {:.1}",
        before.2,
        after.2
    );
    assert!(
        (after.3 - before.3).abs() < 1.5,
        "第 4 张在下方，拖上边时不该动：拖动前 {:.1}，现在 {:.1}",
        before.3,
        after.3
    );
    // 上留白只有最小留白，最多让出这么多，剩下的位移必须由滚动补。
    assert!(
        (after.0 - before.0 + 36.0).abs() < 1.5,
        "留白让完后应由滚动补上剩下的位移：滚动位置应从 {:.1} 变到 {:.1}",
        before.0,
        before.0 - 36.0
    );
    assert!(
        (after.0 - before.0).abs() > 30.0,
        "这个场景必须真的滚了，否则这条测试没有覆盖到滚动补位"
    );
}

/// 作品比视口宽、已经滚到中间时，把第 2 张的左边缘往左拖：
/// 左边留白只剩最小留白，吸收不完的位移由横向滚动补，右边的图不动。
#[test]
fn dragging_a_left_edge_outwards_while_scrolled_scrolls_sideways() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    cx.simulate_resize(gpui::size(gpui::px(960.), gpui::px(600.)));
    install_items(
        cx,
        &view,
        vec![
            test_item(1, 3000, 1000, vec![]),
            test_item(2, 3000, 1000, vec![]),
        ],
    );
    cx.update_entity(&view, |root, cx| {
        root.project.settings.layout = crate::model::LayoutTarget::Row;
        // 左右两边各留一段余量：默认保留带是整张图，往左拖会立刻顶到图片边界，
        // 测不到留白让完之后的滚动补位。
        let band = crate::model::ManualBand::clamped(0.15, 0.85, 0.0, 1.0).expect("有效保留带");
        for item in root.project.items.iter_mut() {
            item.manual = Some(band);
            item.keep_whole = false;
        }
        root.set_mode(Mode::Layout, cx);
        root.select(2, cx);
        // 固定成 1:1。这条测的是拖裁剪时「留白让完由横向滚动补位」，需要作品
        // 真的比视口宽。自动适应会把两张 3000 宽的图压到 830 左右，比 960 的
        // 视口还窄，滚不动，测不到要测的东西。
        root.set_manual_scale(1.0, cx);
        cx.notify();
    });
    draw(cx);
    // 滚到第 2 张左边缘在视口左侧三分之一处：指针只横向移动，不会碰到上下自动滚动区。
    cx.update_entity(&view, |root, cx| {
        let layout = root.canvas_layout();
        let second = layout.entry(2).expect("应有第 2 张").rect.x;
        let padding = f32::from(layout.left_padding(root.viewport));
        root.canvas_scroll.set_offset(gpui::point(
            gpui::px(300.0 - padding - second),
            gpui::px(0.),
        ));
        cx.notify();
    });
    draw(cx);
    let before = cx.read_entity(&view, |root, _| {
        let layout = root.canvas_layout();
        let origin = layout.origin(root.viewport, root.canvas_scroll.offset());
        let second = layout.entry(2).expect("应有第 2 张");
        (
            f32::from(root.canvas_scroll.offset().x),
            origin.x + second.rect.x,
            origin.x + second.rect.right(),
            origin.y + second.rect.y,
        )
    });
    drag_crop_edge(cx, 2, "left", -60.0, 0.0);
    let after = cx.read_entity(&view, |root, _| {
        let layout = root.canvas_layout();
        let origin = layout.origin(root.viewport, root.canvas_scroll.offset());
        let second = layout.entry(2).expect("应有第 2 张");
        (
            f32::from(root.canvas_scroll.offset().x),
            origin.x + second.rect.x,
            origin.x + second.rect.right(),
            origin.y + second.rect.y,
        )
    });
    assert!(
        (after.1 - before.1 + 60.0).abs() < 1.5,
        "第 2 张的左边应跟着指针往左走 60px：拖动前 {:.1}，现在 {:.1}",
        before.1,
        after.1
    );
    assert!(
        (after.2 - before.2).abs() < 1.5,
        "拖左边时第 2 张的右边应钉住不动：拖动前 {:.1}，现在 {:.1}",
        before.2,
        after.2
    );
    assert!(
        (after.3 - before.3).abs() < 1.5,
        "拖左边不该让第 2 张上下漂：拖动前 {:.1}，现在 {:.1}",
        before.3,
        after.3
    );
    // 左边留白只有最小留白，最多让出这么多，剩下的位移必须由横向滚动补。
    assert!(
        (after.0 - before.0 + 12.0).abs() < 1.5,
        "留白让完后应由横向滚动补上剩下的位移：滚动位置应从 {:.1} 变到 {:.1}",
        before.0,
        before.0 - 12.0
    );
    assert!(
        (after.0 - before.0).abs() > 8.0,
        "这个场景必须真的横向滚了，否则这条测试没有覆盖到滚动补位"
    );
}

/// 需求 5/6：最后一张图的下边缘在视口底部附近时，拖它不会变成拖窗口。
#[test]
fn last_image_bottom_edge_near_viewport_bottom_drags_itself() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    cx.simulate_resize(gpui::size(gpui::px(960.), gpui::px(700.)));
    install_items(
        cx,
        &view,
        vec![
            test_item(1, 1000, 1000, vec![line(700.0, 760.0)]),
            test_item(2, 1000, 1000, vec![line(700.0, 760.0)]),
        ],
    );
    draw(cx);
    let (got, window_moves) = press_edge(cx, &view, "crop-hotzone-2-bottom");
    assert_eq!(got.as_ref().map(|(_, edge)| edge.as_str()), Some("bottom"), "最后一张的下边缘应能拖动");
    assert_eq!(window_moves, 0, "按最后一张的下边缘不该变成拖窗口");
}



// ----------------------------------------------------------------------
// 需求 10：裁剪边缘提示改成细短圆角条，显现和淡回都有动画。
// ----------------------------------------------------------------------

/// 需求 10：调哪条边，就只有那条边画成短圆角条，另外三条只留细线。
///
/// 短圆角条比细线粗，量宽度就能分辨：`crop-line-*` 的高度（横边）或
/// 宽度（竖边）等于 `CROP_HINT_BAR` 的是当前边，等于 `CROP_GHOST_LINE` 的是参照线。
#[test]
fn only_the_edge_being_dragged_is_highlighted() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    cx.simulate_resize(gpui::size(gpui::px(1200.), gpui::px(1200.)));
    install_items(
        cx,
        &view,
        vec![test_item(1, 1000, 1000, vec![])],
    );
    cx.update_entity(&view, |root, cx| root.set_mode(Mode::Layout, cx));
    draw(cx);
    let zone = cx.debug_bounds("crop-hotzone-1-right").expect("应有右边热区");
    let start = zone.center();
    cx.simulate_mouse_move(start, None, gpui::Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    cx.simulate_mouse_move(
        gpui::point(start.x + gpui::px(20.), start.y),
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    draw(cx);
    let bar = f32::from(tokens::size::CROP_HINT_BAR);
    let ghost = f32::from(tokens::size::CROP_GHOST_LINE);
    // 正在调的右边自己有一条短圆角条，只有它会画出来。
    let short_bar = cx
        .debug_bounds("crop-bar-right")
        .expect("正在调的右边应画出短圆角条");
    assert!(
        (f32::from(short_bar.size.width) - bar).abs() < 0.5,
        "正在调的右边应是 {bar}px 粗的短条，实际 {}",
        f32::from(short_bar.size.width)
    );
    // 另外三条边不该有短圆角条。
    for selector in ["crop-bar-top", "crop-bar-bottom", "crop-bar-left"] {
        assert!(
            cx.debug_bounds(selector).is_none(),
            "{selector} 不是正在调的边，不该突出显示"
        );
    }
    for selector in ["crop-line-top", "crop-line-bottom", "crop-line-left"] {
        let other = cx.debug_bounds(selector).expect("参照线应画出来");
        let thickness = if selector == "crop-line-top" || selector == "crop-line-bottom" {
            f32::from(other.size.height)
        } else {
            f32::from(other.size.width)
        };
        assert!(
            (thickness - ghost).abs() < 0.5,
            "{selector} 是参照线，应是 {ghost}px 细线，实际 {thickness}"
        );
    }
    // 短条居中：上下留白对称，长度不超过 CROP_HINT_LEN。
    let block = cx.debug_bounds("canvas-block-1").expect("应有图块");
    let block_h = f32::from(block.size.height);
    let bar_h = f32::from(short_bar.size.height);
    assert!(
        bar_h <= f32::from(tokens::size::CROP_HINT_LEN) + 0.5,
        "短条长度不该超过 {}px，实际 {bar_h}",
        f32::from(tokens::size::CROP_HINT_LEN)
    );
    let top_gap = f32::from(short_bar.origin.y) - f32::from(block.origin.y);
    let bottom_gap = f32::from(block.bottom()) - f32::from(short_bar.bottom());
    assert!(
        (top_gap - bottom_gap).abs() < 1.5,
        "短条该居中：上留白 {top_gap:.1}，下留白 {bottom_gap:.1}（图块高 {block_h:.1}）"
    );
    cx.simulate_mouse_up(
        gpui::point(start.x + gpui::px(20.), start.y),
        gpui::MouseButton::Left,
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
}

/// 需求 10：指针刚靠近时边缘提示从很淡开始，过一会儿才到满格，不是一帧跳出来。
#[test]
fn edge_hint_fades_in_instead_of_appearing_at_once() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    cx.simulate_resize(gpui::size(gpui::px(1200.), gpui::px(1200.)));
    install_items(
        cx,
        &view,
        vec![test_item(1, 1000, 1000, vec![])],
    );
    draw(cx);
    let idle = cx.read_entity(&view, |root, app| root.edge_hint_opacity_at(1, app.background_executor().now()));
    assert!(
        (idle - tokens::CROP_HINT_IDLE).abs() < 1e-6,
        "没碰它的时候应是默认的半透明，实际 {idle}"
    );
    let zone = cx.debug_bounds("crop-hotzone-1-top").expect("应有上边热区");
    cx.simulate_mouse_move(zone.center(), None, gpui::Modifiers::default());
    cx.run_until_parked();
    let just_entered = cx.read_entity(&view, |root, app| root.edge_hint_opacity_at(1, app.background_executor().now()));
    assert!(
        just_entered < 1.0,
        "刚进入的一瞬间不该已经满格，实际 {just_entered}"
    );
    // 让时间往前走，动画推进到满格。
    cx.executor()
        .advance_clock(tokens::CROP_HINT_FADE_IN + std::time::Duration::from_millis(50));
    cx.run_until_parked();
    let settled = cx.read_entity(&view, |root, app| root.edge_hint_opacity_at(1, app.background_executor().now()));
    assert!(
        settled > just_entered,
        "等够 120 毫秒后应比刚进入时更明显（刚进入 {just_entered}，等待后 {settled}）"
    );
    assert!((settled - 1.0).abs() < 1e-3, "等够了应到满格，实际 {settled}");
}

/// 需求 10：指针离开图块后，边缘提示用 500 毫秒淡回半透明。
#[test]
fn edge_hint_fades_back_after_leaving() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    cx.simulate_resize(gpui::size(gpui::px(1200.), gpui::px(1200.)));
    install_items(
        cx,
        &view,
        vec![test_item(1, 1000, 1000, vec![])],
    );
    draw(cx);
    let zone = cx.debug_bounds("crop-hotzone-1-top").expect("应有上边热区");
    cx.simulate_mouse_move(zone.center(), None, gpui::Modifiers::default());
    cx.run_until_parked();
    cx.executor()
        .advance_clock(tokens::CROP_HINT_FADE_IN + std::time::Duration::from_millis(50));
    cx.run_until_parked();
    let full = cx.read_entity(&view, |root, app| root.edge_hint_opacity_at(1, app.background_executor().now()));
    assert!((full - 1.0).abs() < 1e-3, "先到满格，实际 {full}");
    // 指针移到画布外面（窗口左上角之外的留白）。
    cx.simulate_mouse_move(
        gpui::point(gpui::px(2.), gpui::px(2.)),
        None,
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    let just_left = cx.read_entity(&view, |root, app| root.edge_hint_opacity_at(1, app.background_executor().now()));
    assert!(
        just_left > tokens::CROP_HINT_IDLE,
        "刚离开的一瞬间还在往下走，不该立刻掉到半透明，实际 {just_left}"
    );
    cx.executor().advance_clock(
        tokens::CROP_HINT_FADE_OUT + std::time::Duration::from_millis(50),
    );
    cx.run_until_parked();
    let settled = cx.read_entity(&view, |root, app| root.edge_hint_opacity_at(1, app.background_executor().now()));
    assert!(
        (settled - tokens::CROP_HINT_IDLE).abs() < 1e-3,
        "等够 500 毫秒后应淡回半透明 {}，实际 {settled}",
        tokens::CROP_HINT_IDLE
    );
}

/// 需求 10：很窄的图，边缘提示的短条仍然落在图块里，不会伸出去。
#[test]
fn edge_hint_bar_stays_inside_a_narrow_block() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    cx.simulate_resize(gpui::size(gpui::px(1200.), gpui::px(1200.)));
    install_items(
        cx,
        &view,
        vec![test_item(1, 40, 1000, vec![])],
    );
    cx.update_entity(&view, |root, cx| root.set_mode(Mode::Layout, cx));
    draw(cx);
    let zone = cx.debug_bounds("crop-hotzone-1-top").expect("窄图也该有上边热区");
    let start = zone.center();
    cx.simulate_mouse_move(start, None, gpui::Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    cx.simulate_mouse_move(
        gpui::point(start.x, start.y + gpui::px(20.)),
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    draw(cx);
    let block = cx.debug_bounds("canvas-block-1").expect("应有图块");
    let line = cx.debug_bounds("crop-line-top").expect("应有上边提示");
    assert!(
        f32::from(line.origin.x) >= f32::from(block.origin.x) - 0.5,
        "提示不该伸到图块左边：图块左 {:.1}，提示左 {:.1}",
        f32::from(block.origin.x),
        f32::from(line.origin.x)
    );
    assert!(
        f32::from(line.right()) <= f32::from(block.right()) + 0.5,
        "提示不该伸到图块右边：图块右 {:.1}，提示右 {:.1}",
        f32::from(block.right()),
        f32::from(line.right())
    );
    cx.simulate_mouse_up(
        gpui::point(start.x, start.y + gpui::px(20.)),
        gpui::MouseButton::Left,
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
}

// ----------------------------------------------------------------------
// 需求 11：间隔调节只在胶囊的拖动按钮上，外沿和色点不能改间隔。
// ----------------------------------------------------------------------

/// 备好一个有两张图、因而有接缝与间隔胶囊的画布。
fn canvas_with_gap_control(cx: &mut gpui::VisualTestContext, view: &gpui::Entity<AppRoot>) {
    cx.simulate_resize(gpui::size(gpui::px(900.), gpui::px(700.)));
    install_items(cx, view, stacked_items());
    cx.update_entity(view, |root, cx| {
        root.project.mode = Mode::Subtitle;
        root.project.settings.gap = root.project.settings.gap.with_px(24);
        cx.notify();
    });
    draw(cx);
}

/// 需求 11：按胶囊最外沿（圆角处）不该改间隔。
///
/// 早先整个药丸都挂拖动，点在圆角外沿上也会改间隔：用户只是想点色点换颜色，
/// 结果缝隙被拖动了。
#[test]
fn pressing_the_capsule_rim_does_not_change_the_gap() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    canvas_with_gap_control(cx, &view);
    let control = cx.debug_bounds("gap-control").expect("应有间隔胶囊");
    let before = cx.read_entity(&view, |root, _| root.project.settings.gap.effective_px());
    // 胶囊左上角：在最外沿上，但不在拖动按钮所在的区域。
    let corner = gpui::point(control.origin.x + gpui::px(2.), control.origin.y + gpui::px(2.));
    cx.simulate_mouse_move(corner, None, gpui::Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    cx.simulate_mouse_down(corner, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    // 试着拖一段距离。
    let moved = gpui::point(corner.x, corner.y + gpui::px(50.));
    cx.simulate_mouse_move(moved, Some(gpui::MouseButton::Left), gpui::Modifiers::default());
    cx.run_until_parked();
    cx.simulate_mouse_move(
        gpui::point(corner.x, corner.y + gpui::px(90.)),
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    draw(cx);
    let after = cx.read_entity(&view, |root, _| root.project.settings.gap.effective_px());
    assert_eq!(after, before, "按胶囊外沿不该改间隔：{before} -> {after}");
    let dragging = cx.read_entity(&view, |root, _| root.gap_drag.is_some());
    assert!(!dragging, "按胶囊外沿不该启动间隔拖动");
    cx.simulate_mouse_up(
        gpui::point(corner.x, corner.y + gpui::px(90.)),
        gpui::MouseButton::Left,
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
}

/// 需求 11：按色点心只换颜色，不改间隔，也不启动间隔拖动。
#[test]
fn pressing_the_colour_dot_only_changes_the_colour() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    canvas_with_gap_control(cx, &view);
    let dot = cx.debug_bounds("gap-control-dot").expect("应有色点");
    let before = cx.read_entity(&view, |root, _| {
        (
            root.project.settings.gap.effective_px(),
            root.project.settings.gap.color,
        )
    });
    let center = dot.center();
    cx.simulate_mouse_move(center, None, gpui::Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    cx.simulate_mouse_down(center, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    let dragging = cx.read_entity(&view, |root, _| root.gap_drag.is_some());
    assert!(!dragging, "按色点不该启动间隔拖动");
    cx.simulate_mouse_up(center, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    let after = cx.read_entity(&view, |root, _| {
        (
            root.project.settings.gap.effective_px(),
            root.project.settings.gap.color,
        )
    });
    assert_eq!(after.0, before.0, "按色点不该改间隔：{before:?} -> {after:?}");
    assert_ne!(after.1, before.1, "按色点应换颜色：{before:?} -> {after:?}");
}

/// 需求 11：拖动按钮能改间隔。
#[test]
fn dragging_the_handle_does_change_the_gap() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    canvas_with_gap_control(cx, &view);
    let handle = cx
        .debug_bounds("gap-control-handle")
        .expect("应有拖动按钮");
    let start = handle.center();
    let before = cx.read_entity(&view, |root, _| root.project.settings.gap.effective_px());
    cx.simulate_mouse_move(start, None, gpui::Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    for step in 1..=3 {
        cx.simulate_mouse_move(
            gpui::point(start.x, start.y + gpui::px(step as f32 * 20.)),
            Some(gpui::MouseButton::Left),
            gpui::Modifiers::default(),
        );
        cx.run_until_parked();
    }
    let after = cx.read_entity(&view, |root, _| root.project.settings.gap.effective_px());
    assert!(
        after > before,
        "往下拖按钮应把间隔调大：{before} -> {after}"
    );
    cx.simulate_mouse_up(
        gpui::point(start.x, start.y + gpui::px(60.)),
        gpui::MouseButton::Left,
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
}

// ----------------------------------------------------------------------
// 需求 12：间隔胶囊固定在可视画布区域垂直中央，默认半透明、悬停不透明、
// 离开后 500ms 淡出。
// ----------------------------------------------------------------------

/// 需求 12：作品滚到下面时，胶囊仍留在可视区域中央，不跟着滚走。
#[test]
fn gap_capsule_stays_put_when_the_artwork_scrolls() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    cx.simulate_resize(gpui::size(gpui::px(900.), gpui::px(500.)));
    install_items(
        cx,
        &view,
        vec![
            test_item(1, 1000, 3000, vec![line(500.0, 700.0)]),
            test_item(2, 1000, 3000, vec![line(500.0, 700.0)]),
        ],
    );
    draw(cx);
    let at_top = cx.debug_bounds("gap-control").expect("应有间隔胶囊");
    cx.update_entity(&view, |root, cx| {
        root.canvas_scroll
            .set_offset(gpui::point(gpui::px(0.), gpui::px(-900.)));
        cx.notify();
    });
    draw(cx);
    let scrolled = cx.debug_bounds("gap-control").expect("滚动后仍应有间隔胶囊");
    assert!(
        (f32::from(scrolled.origin.y) - f32::from(at_top.origin.y)).abs() < 1.0,
        "滚动后胶囊不该跟着走：滚前 y {:.1}，滚后 y {:.1}",
        f32::from(at_top.origin.y),
        f32::from(scrolled.origin.y)
    );
    let viewport = cx.read_entity(&view, |root, _| root.viewport);
    let mid = f32::from(scrolled.origin.y) + f32::from(scrolled.size.height) / 2.0;
    assert!(
        (mid - f32::from(viewport.height) / 2.0).abs() < 1.5,
        "胶囊纵向应在可视区域中央：中心 {mid:.1}，视口高 {:.1}",
        f32::from(viewport.height)
    );
}

/// 需求 12：没被指针碰到的胶囊是半透明的，指针压上去才不透明。
#[test]
fn gap_capsule_fades_in_on_hover() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    canvas_with_gap_control(cx, &view);
    let idle = cx.read_entity(&view, |root, app| {
        root.gap_control_opacity_at(app.background_executor().now())
    });
    assert!(
        (idle - tokens::GAP_CONTROL_IDLE_OPACITY).abs() < 1e-6,
        "没碰它时应是半透明，实际 {idle}"
    );
    let control = cx.debug_bounds("gap-control").expect("应有间隔胶囊");
    cx.simulate_mouse_move(control.center(), None, gpui::Modifiers::default());
    cx.run_until_parked();
    let just_entered = cx.read_entity(&view, |root, app| {
        root.gap_control_opacity_at(app.background_executor().now())
    });
    assert!(
        just_entered < 1.0,
        "刚进入的一瞬间不该已经满格，实际 {just_entered}"
    );
    cx.executor()
        .advance_clock(tokens::CROP_HINT_FADE_IN + std::time::Duration::from_millis(50));
    cx.run_until_parked();
    let settled = cx.read_entity(&view, |root, app| {
        root.gap_control_opacity_at(app.background_executor().now())
    });
    assert!(
        (settled - 1.0).abs() < 1e-3,
        "悬停够时间后应不透明，实际 {settled}"
    );
}

/// 需求 12：指针离开后，胶囊用 500 毫秒淡回半透明。
#[test]
fn gap_capsule_fades_back_after_leaving() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    canvas_with_gap_control(cx, &view);
    let control = cx.debug_bounds("gap-control").expect("应有间隔胶囊");
    cx.simulate_mouse_move(control.center(), None, gpui::Modifiers::default());
    cx.run_until_parked();
    cx.executor()
        .advance_clock(tokens::CROP_HINT_FADE_IN + std::time::Duration::from_millis(50));
    cx.run_until_parked();
    let full = cx.read_entity(&view, |root, app| {
        root.gap_control_opacity_at(app.background_executor().now())
    });
    assert!((full - 1.0).abs() < 1e-3, "先到不透明，实际 {full}");
    // 指针移到窗口别的角落。
    cx.simulate_mouse_move(
        gpui::point(gpui::px(860.), gpui::px(20.)),
        None,
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    let just_left = cx.read_entity(&view, |root, app| {
        root.gap_control_opacity_at(app.background_executor().now())
    });
    assert!(
        just_left > tokens::GAP_CONTROL_IDLE_OPACITY,
        "刚离开时还在往下走，不该立刻掉到半透明，实际 {just_left}"
    );
    cx.executor().advance_clock(
        tokens::CROP_HINT_FADE_OUT + std::time::Duration::from_millis(50),
    );
    cx.run_until_parked();
    let settled = cx.read_entity(&view, |root, app| {
        root.gap_control_opacity_at(app.background_executor().now())
    });
    assert!(
        (settled - tokens::GAP_CONTROL_IDLE_OPACITY).abs() < 1e-3,
        "等够 500 毫秒后应淡回半透明 {}，实际 {settled}",
        tokens::GAP_CONTROL_IDLE_OPACITY
    );
}

// ----------------------------------------------------------------------
// 需求 7：Ctrl+A（各平台的主修饰键 + A）全选。
// ----------------------------------------------------------------------

/// 需求 7：按平台快捷键全选，选中作品里的每一张图。
#[test]
fn select_all_shortcut_selects_every_image() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(
        cx,
        &view,
        vec![
            test_item(1, 1000, 1000, vec![line(700.0, 760.0)]),
            test_item(2, 1000, 1000, vec![line(700.0, 760.0)]),
            test_item(3, 1000, 1000, vec![line(700.0, 760.0)]),
        ],
    );
    // 先只选一张，确认快捷键真的把它扩成全选。
    cx.update_entity(&view, |root, cx| {
        root.select(2, cx);
    });
    let before = cx.read_entity(&view, |root, _| root.selected_ids.clone());
    assert_eq!(before.len(), 1, "开始只应选中一张");
    cx.simulate_keystrokes(crate::app::keymap::select_all_keystroke());
    cx.run_until_parked();
    let after = cx.read_entity(&view, |root, _| root.selected_ids.clone());
    assert_eq!(after.len(), 3, "快捷键应选中全部三张，实际 {after:?}");
    for id in [1, 2, 3] {
        assert!(after.contains(&id), "第 {id} 张应在选中集合里：{after:?}");
    }
}

/// 需求 7：作品为空时按全选不panic、不留下「选了但没图」的状态。
#[test]
fn select_all_on_an_empty_project_does_nothing() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    cx.simulate_resize(gpui::size(gpui::px(900.), gpui::px(700.)));
    draw(cx);
    cx.simulate_keystrokes(crate::app::keymap::select_all_keystroke());
    cx.run_until_parked();
    let (selected, ids) = cx.read_entity(&view, |root, _| (root.selected, root.selected_ids.clone()));
    assert_eq!(selected, None, "空作品按全选不该选中任何东西");
    assert!(ids.is_empty(), "空作品按全选选中集合应为空：{ids:?}");
}

/// 需求 7：全选之后不能再靠单选把它取消（全选是集合操作，不是切换）。
#[test]
fn select_all_keeps_the_main_selection_when_it_is_still_selected() {
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
    cx.update_entity(&view, |root, cx| root.select(2, cx));
    cx.update_entity(&view, |root, cx| root.select_all(cx));
    let selected = cx.read_entity(&view, |root, _| root.selected);
    assert_eq!(selected, Some(2), "原来的主选中项还在集合里就该留着");
}

// ----------------------------------------------------------------------
// 需求 8：调哪张图的边缘就动哪些图（选中的一起改）；已经满尺寸就不能再调。
// ----------------------------------------------------------------------

/// 需求 8：第 2 张的上边缘拉到顶之后，两张选中的图都停在同一段裁剪上。
///
/// 两张都选中时会同步，所以这里同时验两件事：第 2 张的上边缘确实拉到了最上面，
/// 而且第 1 张跟它一样（同步），不会出现一张到顶、另一张还停在半路。
#[test]
fn top_edge_past_the_top_syncs_every_selected_image() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    cx.simulate_resize(gpui::size(gpui::px(900.), gpui::px(1400.)));
    install_items(
        cx,
        &view,
        vec![
            test_item(1, 1000, 1000, vec![line(500.0, 600.0)]),
            test_item(2, 1000, 1000, vec![line(500.0, 600.0)]),
        ],
    );
    draw(cx);
    // 两张都先裁掉上面一段，这样上边缘可以继续往外拖到顶。
    cx.update_entity(&view, |root, cx| {
        let cut = crate::model::ManualBand {
            left: 0.0,
            right: 1.0,
            top: 0.3,
            bottom: 1.0,
        };
        root.project.items[0].manual = Some(cut);
        root.project.items[1].manual = Some(cut);
        root.crop_drag = None;
        // 两张都选中：字幕模式下选中的图一起改。
        root.selected_ids = [1, 2].into_iter().collect();
        cx.notify();
    });
    draw(cx);
    let zone = cx
        .debug_bounds("crop-hotzone-2-top")
        .expect("第 2 张应有上边缘热区");
    let start = zone.center();
    cx.simulate_mouse_move(start, None, gpui::Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    // 一直往上拖，拖动量远超过把上边缘拉到顶所需的距离。
    let mut last = start;
    for step in 1..=10 {
        last = gpui::point(start.x, start.y - gpui::px(step as f32 * 30.));
        cx.simulate_mouse_move(last, Some(gpui::MouseButton::Left), gpui::Modifiers::default());
        cx.run_until_parked();
    }
    draw(cx);
    let (first, second) = cx.read_entity(&view, |root, _| {
        (root.project.items[0].manual, root.project.items[1].manual)
    });
    let first = first.expect("第 1 张应还在裁剪状态");
    let second = second.expect("第 2 张应还在裁剪状态");
    assert!(
        second.top <= 1e-6,
        "第 2 张的上边缘应该已经拉到最上面，实际 top={}",
        second.top
    );
    assert_eq!(first, second, "两张都选中时，这一拖应把两张同步到同一段裁剪");
    cx.simulate_mouse_up(last, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
}

/// 需求 8：拖第 1 张的下边缘，两张选中的图同步到同一段裁剪。
///
/// 两张都选中时会同步，所以这里验的是两张改完一样（第 2 张的上边缘不会被单独动），
/// 而且这一次拖动只改纵向、不动横向。
#[test]
fn bottom_edge_drag_syncs_every_selected_image() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    cx.simulate_resize(gpui::size(gpui::px(900.), gpui::px(1400.)));
    install_items(
        cx,
        &view,
        vec![
            test_item(1, 1000, 1000, vec![line(500.0, 600.0)]),
            test_item(2, 1000, 1000, vec![line(500.0, 600.0)]),
        ],
    );
    // 两张都选中：字幕模式下选中的图一起改。
    // 两张的起点先设成同一段完整画面，这样「同步」和「各改各的」结果会不同：
    // 起点相同、位移相同，同步之后必然一模一样。
    cx.update_entity(&view, |root, cx| {
        let full = crate::model::ManualBand::clamped(0.0, 1.0, 0.0, 1.0).expect("有效保留带");
        root.apply_band_to_items(&[1, 2], full);
        root.selected_ids = [1, 2].into_iter().collect();
        cx.notify();
    });
    draw(cx);
    let zone = cx
        .debug_bounds("crop-hotzone-1-bottom")
        .expect("第 1 张应有下边缘热区");
    let start = zone.center();
    cx.simulate_mouse_move(start, None, gpui::Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    // 往上拖是把下边缘往里收，这样一定真的改到东西。往下拖是从满尺寸继续往外，
    // 会被「已经顶住」的短路直接挡掉，两张都停在原位，断言会变成空转。
    let mut last = start;
    for step in 1..=10 {
        last = gpui::point(start.x, start.y - gpui::px(step as f32 * 30.));
        cx.simulate_mouse_move(last, Some(gpui::MouseButton::Left), gpui::Modifiers::default());
        cx.run_until_parked();
    }
    draw(cx);
    let (first, second) = cx.read_entity(&view, |root, _| {
        (root.project.items[0].manual, root.project.items[1].manual)
    });
    let first = first.expect("第 1 张应被裁");
    let second = second.expect("整组一起改，第 2 张也应被裁");
    // 正面证据：这一拖真的把下边缘收进来了。没有这一条，事件被丢掉时
    // 两张都停在满尺寸，`first == second` 照样成立，测试会变成空转。
    assert!(
        first.bottom < 0.95,
        "这一拖应把下边缘收进来，实际 {first:?}"
    );
    assert_eq!(first, second, "两张都选中时，这一拖应把两张同步到同一段裁剪");
    assert!(first.left == 0.0 && first.right == 1.0, "纵向拖动不该改横向");
    assert_eq!(first.top, 0.0, "收下边缘不该动上边缘");
    cx.simulate_mouse_up(last, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
}

// ----------------------------------------------------------------------
// 需求 9：最小保留范围要够大，不能把图收到只剩一条缝。
// ----------------------------------------------------------------------

/// 需求 9：一直往里收，保留范围不会小于 15%×15%。
#[test]
fn an_edge_can_never_shrink_below_the_minimum_area() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    cx.simulate_resize(gpui::size(gpui::px(900.), gpui::px(1400.)));
    install_items(
        cx,
        &view,
        vec![test_item(1, 1000, 1000, vec![line(500.0, 600.0)])],
    );
    draw(cx);
    // 先横向收到最小。
    for (selector, dx, dy) in [
        ("crop-hotzone-1-left", 400.0_f32, 0.0_f32),
        ("crop-hotzone-1-right", -400.0, 0.0),
        ("crop-hotzone-1-top", 0.0, 400.0),
        ("crop-hotzone-1-bottom", 0.0, -400.0),
    ] {
        cx.update_entity(&view, |root, _| root.crop_drag = None);
        draw(cx);
        let Some(zone) = cx.debug_bounds(selector) else {
            continue;
        };
        let start = zone.center();
        cx.simulate_mouse_move(start, None, gpui::Modifiers::default());
        cx.run_until_parked();
        draw(cx);
        cx.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::default());
        cx.run_until_parked();
        let mut last = start;
        for step in 1..=12 {
            last = gpui::point(
                start.x + gpui::px(dx * step as f32 / 12.0),
                start.y + gpui::px(dy * step as f32 / 12.0),
            );
            cx.simulate_mouse_move(last, Some(gpui::MouseButton::Left), gpui::Modifiers::default());
            cx.run_until_parked();
        }
        cx.simulate_mouse_up(last, gpui::MouseButton::Left, gpui::Modifiers::default());
        cx.run_until_parked();
        let band = cx.read_entity(&view, |root, _| root.project.items[0].manual);
        if let Some(band) = band {
            let w = band.right - band.left;
            let h = band.bottom - band.top;
            assert!(
                w >= 0.15 - 1e-3,
                "按 {selector} 收完之后宽度应不低于 15%，实际 {w}"
            );
            assert!(
                h >= 0.15 - 1e-3,
                "按 {selector} 收完之后高度应不低于 15%，实际 {h}"
            );
            assert!(
                w * h >= 0.0225 - 1e-3,
                "按 {selector} 收完之后面积应不低于 2.25%，实际 {}",
                w * h
            );
        }
    }
}

// ----------------------------------------------------------------------
// 需求 15：画布左下角的全局预览导航器。
// ----------------------------------------------------------------------

/// 备好一个「成品比窗口大很多」的画布，导航器才有理由出现。
fn canvas_that_overflows(cx: &mut gpui::VisualTestContext, view: &gpui::Entity<AppRoot>) {
    cx.simulate_resize(gpui::size(gpui::px(700.), gpui::px(500.)));
    install_items(
        cx,
        view,
        vec![
            test_item(1, 1000, 2000, vec![line(500.0, 700.0)]),
            test_item(2, 1000, 2000, vec![line(500.0, 700.0)]),
            test_item(3, 1000, 2000, vec![line(500.0, 700.0)]),
        ],
    );
    draw(cx);
    draw(cx);
}

/// 需求 15：作品超出可视区时，导航器出现在画布左下角。
#[test]
fn navigator_appears_at_the_bottom_left_when_the_artwork_overflows() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    canvas_that_overflows(cx, &view);
    let nav = cx.debug_bounds("navigator").expect("作品溢出时应显示导航器");
    let viewport = cx.read_entity(&view, |root, _| root.viewport);
    assert!(
        f32::from(nav.origin.x) < f32::from(viewport.width) / 2.0,
        "导航器应贴左侧：{:?}",
        nav.origin
    );
    assert!(
        f32::from(nav.origin.y) > f32::from(viewport.height) / 2.0,
        "导航器应贴下侧：{:?}",
        nav.origin
    );
    assert!(
        f32::from(nav.bottom()) <= f32::from(viewport.height),
        "导航器不该超出画布底部：{nav:?}"
    );
    // 缩略图和视口框都要画出来。
    assert!(cx.debug_bounds("navigator-sheet").is_some(), "应有缩略图");
    assert!(cx.debug_bounds("navigator-frame").is_some(), "应有视口框");
}

/// 把一个已建好的窗口摆成「成品在默认缩放下就比窗口大」的排版，导航器一定出现。
///
/// 六种排版预设和单张长图共用这个入口，保证各处量到的都是真实的界面布局，
/// 不是纯函数自己算出来的值。
fn prepare_layout_window(
    cx: &mut gpui::VisualTestContext,
    view: &gpui::Entity<AppRoot>,
    target: crate::model::LayoutTarget,
    items: Vec<Item>,
) {
    cx.simulate_resize(gpui::size(gpui::px(900.), gpui::px(700.)));
    install_items(cx, view, items);
    cx.update_entity(view, |root, cx| {
        root.project.settings.layout = target;
        root.set_mode(Mode::Layout, cx);
        cx.notify();
    });
    draw(cx);
    draw(cx);
}

/// 按图号拼选择器的 debug_bounds：调试选择器接口只收 'static 字符串。
fn debug_bounds_dynamic(
    cx: &mut gpui::VisualTestContext,
    selector: String,
) -> Option<gpui::Bounds<gpui::Pixels>> {
    cx.debug_bounds(Box::leak(selector.into_boxed_str()))
}

/// 一组长宽比差别很大的图，让每种排版预设都真的分组排布，不会退回单列。
fn mixed_aspect_items() -> Vec<Item> {
    vec![
        test_item(1, 1920, 1080, vec![]),
        test_item(2, 1000, 1000, vec![]),
        test_item(3, 1080, 1920, vec![]),
        test_item(4, 1600, 900, vec![]),
    ]
}

/// 需求 15：六种排版预设下，导航器里的缩略图都要居中。
///
/// 用户反馈：横排时预览图下方一大片空白，竖排时预览图右侧一大片空白。
/// 根因是缩略图只按尺寸摆了一个盒子、没有真的摆到中间：横排作品缩略图
/// 宽而矮，贴上边就全空在下边；竖排窄而高，贴左边就全空在右边。
/// 16:9、9:16、4:3、1:1 也走同一个预览框，所以一起量，不能只验横排竖排。
#[test]
fn navigator_thumbnail_is_centred_for_every_layout_preset() {
    let mut cases: Vec<(String, crate::model::LayoutTarget, Vec<Item>)> =
        crate::model::LayoutTarget::PRESETS
            .iter()
            .map(|target| {
                (
                    format!("预设 {}", target.key()),
                    *target,
                    mixed_aspect_items(),
                )
            })
            .collect();
    // 单张长图不分组、不走多列，但一样会溢出并显示预览框。
    cases.push((
        "单张长图".to_string(),
        crate::model::LayoutTarget::Aspect(16.0 / 9.0),
        vec![test_item(1, 1000, 8000, vec![])],
    ));

    for (label, target, items) in cases {
        let mut app = TestAppContext::single();
        let (view, cx) = app.add_window_view(AppRoot::new);
        prepare_layout_window(cx, &view, target, items);
        let nav = cx
            .debug_bounds("navigator")
            .unwrap_or_else(|| panic!("{label} 下作品溢出，应显示预览框"));
        let sheet = cx
            .debug_bounds("navigator-sheet")
            .unwrap_or_else(|| panic!("{label} 下预览框里应有缩略图"));
        let frame = cx
            .debug_bounds("navigator-frame")
            .unwrap_or_else(|| panic!("{label} 下缩略图上应有视口框"));
        let layout_ratio = cx.read_entity(&view, |root, _| {
            let layout = root.canvas_layout();
            layout.width / layout.height.max(1.0)
        });

        // 内框 = 面板去掉 1 像素描边和四边内边距。
        let inset = 1.0 + f32::from(tokens::NAVIGATOR_PAD);
        let inner_left = f32::from(nav.origin.x) + inset;
        let inner_top = f32::from(nav.origin.y) + inset;
        let inner_right = f32::from(nav.right()) - inset;
        let inner_bottom = f32::from(nav.bottom()) - inset;
        let sheet_left = f32::from(sheet.origin.x);
        let sheet_top = f32::from(sheet.origin.y);
        let sheet_right = f32::from(sheet.right());
        let sheet_bottom = f32::from(sheet.bottom());

        // 缩略图不能超出内框。
        assert!(
            sheet_left >= inner_left - 1.0
                && sheet_top >= inner_top - 1.0
                && sheet_right <= inner_right + 1.0
                && sheet_bottom <= inner_bottom + 1.0,
            "{label} 的缩略图超出了预览框内框：缩略图 ({sheet_left:.1}, {sheet_top:.1}, {sheet_right:.1}, {sheet_bottom:.1})，内框 ({inner_left:.1}, {inner_top:.1}, {inner_right:.1}, {inner_bottom:.1})"
        );
        // 左右留白、上下留白必须一样宽，这就是本次反馈的现象。
        let left_gap = sheet_left - inner_left;
        let right_gap = inner_right - sheet_right;
        let top_gap = sheet_top - inner_top;
        let bottom_gap = inner_bottom - sheet_bottom;
        assert!(
            (left_gap - right_gap).abs() <= 1.0,
            "{label} 的缩略图应左右居中：左边空 {left_gap:.1}，右边空 {right_gap:.1}"
        );
        assert!(
            (top_gap - bottom_gap).abs() <= 1.0,
            "{label} 的缩略图应上下居中：上边空 {top_gap:.1}，下边空 {bottom_gap:.1}"
        );
        // 缩略图比例必须和作品比例一致，不能被拉扁或拉长。
        // 细长作品（比如单张 1000×8000 的长图）缩略后只有十几像素宽，
        // 用相对误差会卡到 1 像素的取整误差，这里按像素比：宽高最多差 2 像素。
        let sheet_w = sheet_right - sheet_left;
        let sheet_h = (sheet_bottom - sheet_top).max(1.0);
        assert!(
            (sheet_w - layout_ratio * sheet_h).abs() <= 2.0,
            "{label} 的缩略图比例应和作品一致：作品 {layout_ratio:.3}，缩略图宽 {sheet_w:.1} 高 {sheet_h:.1}"
        );
        // 视口框画在缩略图里面，不能漂到缩略图外面去。
        assert!(
            f32::from(frame.origin.x) >= sheet_left - 1.0
                && f32::from(frame.origin.y) >= sheet_top - 1.0
                && f32::from(frame.right()) <= sheet_right + 1.0
                && f32::from(frame.bottom()) <= sheet_bottom + 1.0,
            "{label} 的视口框应落在缩略图里"
        );
    }
}

/// 需求 15：六种排版预设下，预览框里每张图的位置要和画布上的相对位置一致。
///
/// 只看缩略图盒子居中还不够：如果映射比例算错，图片会在缩略图里挤在一边、
/// 彼此错位，看起来照样是空白。这里把每张图缩略块的实际屏幕矩形，和
/// 「画布矩形 × 缩略图缩放」逐个比对；再要求所有缩略块的外框正好铺满缩略图，
/// 固定比例排版下画布被图片填满，预览里也不该露出空白条。
#[test]
fn navigator_preview_blocks_follow_the_canvas_for_every_layout_preset() {
    // 两组长宽比不同的图都要过一遍：一组四种比例混着来，一组四张一样大。
    // 固定比例排版在两种输入下会选出不同的分列方式，都要保证预览映射正确。
    let mut cases: Vec<(String, crate::model::LayoutTarget, Vec<Item>)> = Vec::new();
    for (set_name, items) in [
        ("长宽比混合", mixed_aspect_items()),
        (
            "四张同尺寸",
            (1..=4)
                .map(|id| test_item(id, 1920, 1080, vec![]))
                .collect::<Vec<Item>>(),
        ),
    ] {
        for target in crate::model::LayoutTarget::PRESETS {
            cases.push((
                format!("{set_name}、预设 {}", target.key()),
                target,
                items.clone(),
            ));
        }
    }
    for (label, target, items) in cases {
        let mut app = TestAppContext::single();
        let (view, mut cx) = app.add_window_view(AppRoot::new);
        prepare_layout_window(cx, &view, target, items);
        let sheet = cx
            .debug_bounds("navigator-sheet")
            .unwrap_or_else(|| panic!("{label} 下预览框里应有缩略图"));
        let (layout_w, layout_h, entries) = cx.read_entity(&view, |root, _| {
            let layout = root.canvas_layout();
            (
                layout.width,
                layout.height.max(1.0),
                layout
                    .entries
                    .iter()
                    .map(|entry| (entry.id, entry.rect))
                    .collect::<Vec<_>>(),
            )
        });
        let sheet_left = f32::from(sheet.origin.x);
        let sheet_top = f32::from(sheet.origin.y);
        let sheet_w = f32::from(sheet.size.width);
        let sheet_h = f32::from(sheet.size.height);
        let scale_x = sheet_w / layout_w.max(1.0);
        let scale_y = sheet_h / layout_h;

        let (mut min_x, mut min_y) = (f32::INFINITY, f32::INFINITY);
        let (mut max_x, mut max_y) = (f32::NEG_INFINITY, f32::NEG_INFINITY);
        let mut covered = 0.0f32;
        for (id, rect) in &entries {
            let block = debug_bounds_dynamic(&mut cx, format!("navigator-block-{id}"))
                .unwrap_or_else(|| panic!("{label} 下预览框里应有第 {id} 张图的缩略块"));
            let (left, top) = (f32::from(block.origin.x), f32::from(block.origin.y));
            let (right, bottom) = (f32::from(block.right()), f32::from(block.bottom()));
            let expected_left = sheet_left + rect.x * scale_x;
            let expected_top = sheet_top + rect.y * scale_y;
            let expected_right = expected_left + rect.w * scale_x;
            let expected_bottom = expected_top + rect.h * scale_y;
            assert!(
                (left - expected_left).abs() <= 1.5
                    && (top - expected_top).abs() <= 1.5
                    && (right - expected_right).abs() <= 1.5
                    && (bottom - expected_bottom).abs() <= 1.5,
                "{label} 下第 {id} 张图的缩略块位置不对：实际 ({left:.1}, {top:.1}, {right:.1}, {bottom:.1})，按画布换算应为 ({expected_left:.1}, {expected_top:.1}, {expected_right:.1}, {expected_bottom:.1})"
            );
            min_x = min_x.min(left);
            min_y = min_y.min(top);
            max_x = max_x.max(right);
            max_y = max_y.max(bottom);
            covered += rect.w * rect.h;
        }
        // 所有缩略块的外框铺满缩略图：起点贴左上，终点贴右下。
        assert!(
            (min_x - sheet_left).abs() <= 1.5
                && (min_y - sheet_top).abs() <= 1.5
                && (max_x - (sheet_left + sheet_w)).abs() <= 1.5
                && (max_y - (sheet_top + sheet_h)).abs() <= 1.5,
            "{label} 下缩略块没有铺满缩略图：外框 ({min_x:.1}, {min_y:.1}, {max_x:.1}, {max_y:.1})，缩略图 ({sheet_left:.1}, {sheet_top:.1}, {sheet_w:.1}, {sheet_h:.1})"
        );
        // 固定比例排版会把画布填满；预览里也不该出现成片的空白条。
        if let crate::model::LayoutTarget::Aspect(_) = target {
            let coverage = covered / (layout_w * layout_h);
            // layout_w/layout_h 是画布的设计尺寸，entries 的矩形也是这个坐标系。
            assert!(
                coverage > 0.99,
                "{label} 的画布应被图片填满，实际覆盖率 {coverage:.3}"
            );
        }
    }
}

/// 需求 15：作品没超出可视区时不显示导航器。
#[test]
fn navigator_is_hidden_when_the_artwork_fits() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    cx.simulate_resize(gpui::size(gpui::px(1400.), gpui::px(1400.)));
    install_items(
        cx,
        &view,
        vec![test_item(1, 400, 400, vec![line(280.0, 320.0)])],
    );
    draw(cx);
    let reason = cx.read_entity(&view, |root, _| {
        let layout = root.canvas_layout();
        format!("max_scroll={:?}", layout.max_scroll(root.viewport))
    });
    assert!(
        cx.debug_bounds("navigator").is_none(),
        "作品一屏放得下时不该显示导航器（{reason}）"
    );
}

/// 需求 15：拖视口框只平移画布，不改缩放，也不改作品数据。
#[test]
fn dragging_the_viewport_frame_only_pans() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    canvas_that_overflows(cx, &view);
    let before = cx.read_entity(&view, |root, _| {
        (
            f32::from(root.canvas_scroll.offset().y),
            root.scale_mode,
            root.project.items.len(),
            root.project.items.iter().map(|item| item.manual).collect::<Vec<_>>(),
        )
    });
    let nav = cx.debug_bounds("navigator").expect("应有导航器");
    // 按在导航器下半部，往下拖：等价于「看更下面」。
    let start = gpui::point(
        nav.origin.x + nav.size.width / 2.0,
        nav.origin.y + nav.size.height * 0.7,
    );
    cx.simulate_mouse_move(start, None, gpui::Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    let entered = cx.read_entity(&view, |root, _| root.navigator_drag.is_some());
    assert!(entered, "按在导航器上应进入视口框拖动");
    // GPUI 用越过阈值的第一次移动激活拖动，第二步才真的派发。
    let mut last = start;
    for step in 1..=4 {
        last = gpui::point(start.x, start.y + gpui::px(step as f32 * 6.0));
        cx.simulate_mouse_move(last, Some(gpui::MouseButton::Left), gpui::Modifiers::default());
        cx.run_until_parked();
    }
    let after = cx.read_entity(&view, |root, _| {
        (
            f32::from(root.canvas_scroll.offset().y),
            root.scale_mode,
            root.project.items.len(),
            root.project.items.iter().map(|item| item.manual).collect::<Vec<_>>(),
        )
    });
    assert!(
        after.0 < before.0,
        "往下拖视口框应看到更下面（滚动偏移应更负）：{} -> {}",
        before.0,
        after.0
    );
    assert_eq!(after.1, before.1, "拖视口框不该改缩放模式");
    assert_eq!(after.2, before.2, "拖视口框不该改图片数量");
    assert_eq!(after.3, before.3, "拖视口框不该改任何一张图的裁剪范围");
    cx.simulate_mouse_up(last, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    let released = cx.read_entity(&view, |root, _| root.navigator_drag.is_some());
    assert!(!released, "松手后应结束视口框拖动");
}

/// 需求 15：把视口框拖出缩略图边界，滚动偏移夹在有效范围内，不会滚过头。
#[test]
fn dragging_the_frame_out_of_bounds_clamps_the_scroll() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    canvas_that_overflows(cx, &view);
    let max_scroll = cx.read_entity(&view, |root, _| {
        let layout = root.canvas_layout();
        layout.max_scroll(root.viewport)
    });
    let nav = cx.debug_bounds("navigator").expect("应有导航器");
    let start = gpui::point(
        nav.origin.x + nav.size.width / 2.0,
        nav.origin.y + nav.size.height / 2.0,
    );
    cx.simulate_mouse_move(start, None, gpui::Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    // 一路拖到窗口外面，远超过缩略图边界。
    let mut last = start;
    for step in 1..=8 {
        last = gpui::point(start.x, start.y + gpui::px(step as f32 * 120.0));
        cx.simulate_mouse_move(last, Some(gpui::MouseButton::Left), gpui::Modifiers::default());
        cx.run_until_parked();
    }
    let offset = cx.read_entity(&view, |root, _| {
        (
            f32::from(root.canvas_scroll.offset().x),
            f32::from(root.canvas_scroll.offset().y),
        )
    });
    assert!(
        offset.0 >= -max_scroll.0 - 0.5 && offset.0 <= 0.5,
        "横向应夹在范围内：偏移 {} ，上限 {}",
        offset.0,
        max_scroll.0
    );
    assert!(
        offset.1 >= -max_scroll.1 - 0.5 && offset.1 <= 0.5,
        "纵向应夹在范围内：偏移 {} ，上限 {}",
        offset.1,
        max_scroll.1
    );
    cx.simulate_mouse_up(last, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
}

/// 需求 15：视口框跟着滚动走，看的东西变了框也要跟着动。
#[test]
fn the_viewport_frame_follows_the_scroll_position() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    canvas_that_overflows(cx, &view);
    let at_top = cx.debug_bounds("navigator-frame").expect("应有视口框");
    let (_, thumb, _) = cx
        .read_entity(&view, |root, _| root.navigator_geometry(root.viewport))
        .expect("应有导航器几何");
    cx.update_entity(&view, |root, cx| {
        root.canvas_scroll.set_offset(gpui::point(gpui::px(0.), gpui::px(-600.)));
        cx.notify();
    });
    draw(cx);
    let scrolled = cx.debug_bounds("navigator-frame").expect("滚动后仍应有视口框");
    assert!(
        f32::from(scrolled.origin.y) > f32::from(at_top.origin.y),
        "往下滚之后框应往下走：滚前 y {:.1}，滚后 y {:.1}",
        f32::from(at_top.origin.y),
        f32::from(scrolled.origin.y)
    );
    assert!(
        f32::from(scrolled.bottom()) <= thumb.bottom() + 1.0,
        "框不该跑出缩略图：框下 {:.1}，缩略图下 {:.1}",
        f32::from(scrolled.bottom()),
        thumb.bottom()
    );
}

/// 需求 15：默认半透明，指针压上去不透明，离开后 500ms 淡回。
#[test]
fn navigator_fades_in_on_hover_and_back_after_leaving() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    canvas_that_overflows(cx, &view);
    let idle = cx.read_entity(&view, |root, app| {
        root.navigator_opacity_at(app.background_executor().now())
    });
    assert!(
        (idle - tokens::NAVIGATOR_IDLE_OPACITY).abs() < 1e-6,
        "没碰它时应是半透明，实际 {idle}"
    );
    let nav = cx.debug_bounds("navigator").expect("应有导航器");
    cx.simulate_mouse_move(nav.center(), None, gpui::Modifiers::default());
    cx.run_until_parked();
    let just_entered = cx.read_entity(&view, |root, app| {
        root.navigator_opacity_at(app.background_executor().now())
    });
    assert!(
        just_entered < 1.0,
        "刚进入的一瞬间不该已经满格，实际 {just_entered}"
    );
    cx.executor()
        .advance_clock(tokens::CROP_HINT_FADE_IN + std::time::Duration::from_millis(50));
    cx.run_until_parked();
    let full = cx.read_entity(&view, |root, app| {
        root.navigator_opacity_at(app.background_executor().now())
    });
    assert!((full - 1.0).abs() < 1e-3, "悬停够时间后应不透明，实际 {full}");
    // 移到画布右上角，离导航器远一点。
    cx.simulate_mouse_move(
        gpui::point(gpui::px(660.), gpui::px(20.)),
        None,
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    cx.executor().advance_clock(
        tokens::CROP_HINT_FADE_OUT + std::time::Duration::from_millis(50),
    );
    cx.run_until_parked();
    let settled = cx.read_entity(&view, |root, app| {
        root.navigator_opacity_at(app.background_executor().now())
    });
    assert!(
        (settled - tokens::NAVIGATOR_IDLE_OPACITY).abs() < 1e-3,
        "等够 500 毫秒后应淡回半透明 {}，实际 {settled}",
        tokens::NAVIGATOR_IDLE_OPACITY
    );
}

// ----------------------------------------------------------------------
// 需求 3：横排、竖排预设换成方向图标，文字走悬停提示。
// ----------------------------------------------------------------------

/// 需求 3：横排和竖排两个预设用图标画出来，比例预设仍然写文字。
#[test]
fn row_and_column_presets_show_icons_and_ratio_presets_keep_their_text() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(
        cx,
        &view,
        vec![test_item(1, 1000, 800, vec![line(600.0, 680.0)])],
    );
    cx.update_entity(&view, |root, cx| root.set_mode(Mode::Layout, cx));
    draw(cx);
    // 每一个预设都要画出来，且每个都有自己的命中区域。
    for index in 0..crate::model::LayoutTarget::PRESETS.len() {
        let selector = match index {
            0 => "layout-preset-0",
            1 => "layout-preset-1",
            2 => "layout-preset-2",
            3 => "layout-preset-3",
            4 => "layout-preset-4",
            5 => "layout-preset-5",
            6 => "layout-preset-6",
            7 => "layout-preset-7",
            8 => "layout-preset-8",
            _ => continue,
        };
        let bounds = cx.debug_bounds(selector);
        assert!(bounds.is_some(), "预设 {index}（{selector}）应画出来");
        let bounds = bounds.expect("上面刚断言过存在");
        assert!(
            bounds.size.width > gpui::px(0.) && bounds.size.height > gpui::px(0.),
            "预设 {index} 不该是零尺寸：{bounds:?}"
        );
    }
    // 横排、竖排两个预设画的是方向图标，没有常驻的文字标签；
    // 它们的中文说明只在悬停提示里出现，下面把它等出来。
    let row = cx.debug_bounds("layout-preset-4").expect("第 5 个预设是横排");
    assert!(
        cx.debug_bounds("hover-hint-layout-row").is_none(),
        "没悬停时不该有横排提示"
    );
    cx.simulate_mouse_move(row.center(), None, gpui::Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(700));
    cx.run_until_parked();
    draw(cx);
    assert!(
        cx.debug_bounds("hover-hint-layout-row").is_some(),
        "悬停在横排预设上应出现提示"
    );
}

// ----------------------------------------------------------------------
// 需求 13：间隔像素提示固定在胶囊下方，只在拖动时出现。
// ----------------------------------------------------------------------

/// 需求 13：没拖的时候不画提示，拖动时出现，松手后淡出到看不见。
#[test]
fn the_gap_label_only_shows_while_dragging_and_fades_out_after_release() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    canvas_with_gap_control(cx, &view);
    let idle = cx.read_entity(&view, |root, app| {
        root.gap_label_fade.value_at(app.background_executor().now())
    });
    assert!(idle <= 0.001, "没拖的时候提示应完全透明，实际 {idle}");

    let handle = cx
        .debug_bounds("gap-control-handle")
        .expect("应有拖动按钮");
    let start = handle.center();
    cx.simulate_mouse_move(start, None, gpui::Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    let mut last = start;
    for step in 1..=3 {
        last = gpui::point(start.x, start.y + gpui::px(step as f32 * 8.0));
        cx.simulate_mouse_move(last, Some(gpui::MouseButton::Left), gpui::Modifiers::default());
        cx.run_until_parked();
    }
    cx.executor()
        .advance_clock(tokens::CROP_HINT_FADE_IN + std::time::Duration::from_millis(50));
    cx.run_until_parked();
    draw(cx);
    let during = cx.read_entity(&view, |root, app| {
        root.gap_label_fade.value_at(app.background_executor().now())
    });
    assert!(during > 0.9, "拖动中提示应完全不透明，实际 {during}");
    assert!(
        cx.debug_bounds("gap-bubble").is_some(),
        "拖动中应画像素提示"
    );
    // 提示固定在胶囊下方，不跟着指针跑。
    let label = cx.debug_bounds("gap-bubble").expect("拖动中应有提示");
    let control = cx.debug_bounds("gap-control").expect("应有间隔胶囊");
    assert!(
        f32::from(label.origin.y) > f32::from(control.bottom()),
        "提示应固定在胶囊下方：提示 y {:.1}，胶囊下边 {:.1}",
        f32::from(label.origin.y),
        f32::from(control.bottom())
    );
    cx.simulate_mouse_up(last, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    // 松手后走完 500ms 淡出。
    cx.executor().advance_clock(
        tokens::CROP_HINT_FADE_OUT + std::time::Duration::from_millis(80),
    );
    cx.run_until_parked();
    let after = cx.read_entity(&view, |root, app| {
        root.gap_label_fade.value_at(app.background_executor().now())
    });
    assert!(after <= 0.001, "松手 500 毫秒后提示应淡到看不见，实际 {after}");
}

/// 需求 15）：导航器和画布上其它控件不重叠（计划验收条件）。
#[test]
fn navigator_does_not_overlap_other_canvas_controls() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    canvas_that_overflows(cx, &view);
    let nav = cx.debug_bounds("navigator").expect("作品溢出时应显示导航器");
    // 缩放胶囊一直在画布右下角，两者不能叠在一起。两个都必须画出来：
    // 用条件判断会让「胶囊没渲染」也算通过，等于白测。
    let chip = cx
        .debug_bounds("zoom-chip")
        .expect("缩放胶囊应常驻");
    assert!(
        !nav.intersects(&chip),
        "导航器不该和缩放胶囊重叠：导航器 {nav:?}，胶囊 {chip:?}"
    );
    // 间隔胶囊挂在作品左边、纵向居中，同样不该和左下角的导航器叠上。
    let control = cx
        .debug_bounds("gap-control")
        .expect("三张图之间应有接缝，应显示间隔胶囊");
    assert!(
        !nav.intersects(&control),
        "导航器不该和间隔胶囊重叠：导航器 {nav:?}，胶囊 {control:?}"
    );
}

/// 需求 1：缩放胶囊固定在窗口右下角，作品滚动它不动，滚完仍然点得到。
#[test]
fn zoom_chip_stays_put_and_stays_clickable_when_the_artwork_scrolls() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    canvas_that_overflows(cx, &view);
    let before = cx.debug_bounds("zoom-chip").expect("缩放胶囊应常驻");
    let viewport = cx.read_entity(&view, |root, _| root.viewport);
    // 没滚动时它在窗口右下角。
    assert!(
        f32::from(before.bottom()) <= f32::from(viewport.height),
        "胶囊应在可视区内：{before:?}"
    );
    // 往下滚 900 像素，远超一屏。
    cx.update_entity(&view, |root, cx| {
        root.canvas_scroll
            .set_offset(gpui::point(gpui::px(0.), gpui::px(-900.)));
        cx.notify();
    });
    draw(cx);
    let after = cx.debug_bounds("zoom-chip").expect("滚动后胶囊仍应存在");
    assert_eq!(
        (f32::from(after.origin.x), f32::from(after.origin.y)),
        (f32::from(before.origin.x), f32::from(before.origin.y)),
        "滚动后胶囊位置不该变：滚前 {before:?}，滚后 {after:?}"
    );
    assert!(
        f32::from(after.bottom()) <= f32::from(viewport.height)
            && f32::from(after.origin.y) >= 0.0,
        "滚动后胶囊应仍在可视区内：{after:?}，视口高 {:.1}",
        f32::from(viewport.height)
    );
    // 滚完之后点「1:1」仍然生效。
    let one_to_one = cx
        .debug_bounds("zoom-item-zoom-one-to-one")
        .expect("应有 1:1 项");
    assert!(
        f32::from(one_to_one.origin.y) >= 0.0
            && f32::from(one_to_one.bottom()) <= f32::from(viewport.height),
        "滚动后 1:1 项应完全落在可视区内：{one_to_one:?}"
    );
    cx.simulate_click(one_to_one.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    let manual = cx.read_entity(&view, |root, _| root.view_scale().mode.manual());
    assert_eq!(
        manual,
        Some(1.0),
        "滚动之后点 1:1 仍应切到手动比例 1.0"
    );
    let scale = cx.read_entity(&view, |root, _| root.canvas_layout().scale);
    assert!(
        (scale - 1.0).abs() < 1e-6,
        "1:1 时画布比例应是 1.0，实际 {scale}"
    );
    // 再点「适合画布」能切回自动适应。
    draw(cx);
    let fit = cx
        .debug_bounds("zoom-item-zoom-fit")
        .expect("应有适合画布项");
    cx.simulate_click(fit.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    let back_to_fit = cx.read_entity(&view, |root, _| root.view_scale().is_fit());
    assert!(back_to_fit, "点适合画布应切回自动适应");
}

/// 需求 1：按在缩放胶囊上不该变成拖窗口。
#[test]
fn pressing_the_zoom_chip_does_not_start_a_window_move() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    cx.simulate_resize(gpui::size(gpui::px(900.), gpui::px(700.)));
    // 用一张又窄又高的图：作品在画布中间，胶囊所在的右下角是空白画布。
    // 这样「按在胶囊上」才会真的落到空白拖拽区，测到胶囊自己那道拦截；
    // 用满宽的图时胶囊正好压在图片上，画布会因为按在图片上提前返回，
    // 测试就算过了也证明不了什么。
    install_items(cx, &view, vec![test_item(1, 240, 2000, vec![line(300.0, 400.0)])]);
    draw(cx);
    let chip = cx.debug_bounds("zoom-chip").expect("缩放胶囊应常驻");
    let artwork = cx.debug_bounds("canvas-block-1").expect("应有图块");
    assert!(
        f32::from(chip.origin.x) > f32::from(artwork.right()),
        "胶囊应落在作品右边的空白画布上：胶囊 x {:.1}，图片右边缘 {:.1}",
        f32::from(chip.origin.x),
        f32::from(artwork.right())
    );
    let before = cx.read_entity(&view, |root, _| root.window_move_requests);
    let point = chip.center();
    cx.simulate_mouse_move(point, None, gpui::Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    cx.simulate_mouse_down(point, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    let after = cx.read_entity(&view, |root, _| root.window_move_requests);
    assert_eq!(
        after, before,
        "按在缩放胶囊上不该请求移动窗口（胶囊下面是空白画布）"
    );
    cx.simulate_mouse_up(point, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    // 右键同理：按在胶囊上不该弹出画布卡片。
    cx.read_entity(&view, |root, _| root.menu.clone());
    cx.simulate_mouse_move(point, None, gpui::Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    cx.simulate_mouse_down(point, gpui::MouseButton::Right, gpui::Modifiers::default());
    cx.run_until_parked();
    cx.simulate_mouse_up(point, gpui::MouseButton::Right, gpui::Modifiers::default());
    cx.run_until_parked();
    let menu = cx.read_entity(&view, |root, _| root.menu.is_some());
    assert!(!menu, "按在缩放胶囊上不该弹出画布卡片");
}







// ----------------------------------------------------------------------
// 需求 3 后半段：底部胶囊的添加 / 复制 / 导出只画图标，文字收进悬停说明。
// ----------------------------------------------------------------------

/// 需求 3：添加、复制、导出三个按钮只画图标，按钮里没有文字。
///
/// 怎么判断「没有文字」：这三个按钮的宽度能算出来。`action_button` 里画图标的
/// 按钮左右各留 8 像素、图标本身 16 像素，加起来正好 32 像素；一旦挂上文字，
/// 左右内边距会变成 11 像素，还要再加 5 像素间距和文字本身的宽度，一定超过 32。
/// 这个判定不依赖渲染出来的文字内容，只依赖按钮自己的排版规则。
#[test]
fn the_add_copy_export_buttons_are_icon_only() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(
        cx,
        &view,
        vec![test_item(1, 1000, 1000, vec![line(700.0, 760.0)])],
    );
    draw(cx);
    // 8 像素左内边距 + 16 像素图标 + 8 像素右内边距。
    let icon_only_width = 8.0 + 16.0 + 8.0;
    for (selector, name) in [
        ("capsule-add", "添加"),
        ("capsule-copy", "复制"),
        ("export-png", "导出"),
    ] {
        let Some(bounds) = cx.debug_bounds(selector) else {
            panic!("胶囊里应有{name}按钮（{selector}）");
        };
        let width = f32::from(bounds.size.width);
        assert!(
            (width - icon_only_width).abs() < 0.51,
            "{name}按钮（{selector}）应该只有图标，宽度应是 {icon_only_width} 像素，实际 {width} 像素；变宽说明按钮上挂了文字"
        );
        assert!(
            (f32::from(bounds.size.height) - 30.0).abs() < 0.51,
            "{name}按钮高度应是胶囊按钮的标准高度 30 像素，实际 {}",
            f32::from(bounds.size.height)
        );
    }
}

/// 需求 3：三个按钮各自的说明放在悬停提示里，指到哪个说哪个。
#[test]
fn icon_only_buttons_explain_themselves_on_hover() {
    let all = [
        ("capsule-add", "hover-hint-capsule-add", "添加"),
        ("capsule-copy", "hover-hint-capsule-copy", "复制"),
        ("export-png", "hover-hint-export-png", "导出"),
    ];
    // 每个按钮用一个全新的窗口，让每次检查都从「指针还没压上去」的确定状态开始：
    // `debug_bounds` 只保留当前帧（每帧清空），第一帧里没有悬停，三个说明
    // 都应该不在；随后再把指针移上去看下一帧出现的那一个。
    for (button, hint, name) in all {
        let mut cx = TestAppContext::single();
        let (view, cx) = cx.add_window_view(AppRoot::new);
        install_items(
            cx,
            &view,
            vec![test_item(1, 1000, 1000, vec![line(700.0, 760.0)])],
        );
        draw(cx);
        // 新窗口里指针还没压上去，三个说明一个都不该在树里。
        for (_, other_hint, _) in all {
            assert!(
                cx.debug_bounds(other_hint).is_none(),
                "还没悬停时不该出现 {other_hint}"
            );
        }
        let Some(bounds) = cx.debug_bounds(button) else {
            panic!("胶囊里应有{name}按钮");
        };
        cx.simulate_mouse_move(bounds.center(), None, gpui::Modifiers::default());
        cx.run_until_parked();
        draw(cx);
        assert!(
            cx.debug_bounds(hint).is_some(),
            "指针压在{name}按钮上时应显示它的说明（{hint}）"
        );
        // 只该说被指的那一个，别的按钮别跟着冒出来。
        for (_, other_hint, other_name) in all {
            if other_hint == hint {
                continue;
            }
            assert!(
                cx.debug_bounds(other_hint).is_none(),
                "指着{name}时不该同时冒出{other_name}的说明（{other_hint}）"
            );
        }
    }
}

// ----------------------------------------------------------------------
// 需求 1：能移动图片的位置光标要是「移动」的形状，全平台一致。
// ----------------------------------------------------------------------

/// 需求 1：画布空白处是箭头，指针压到图片上变成「张开的手」。
///
/// 读光标的方式：先把指针移过去（GPUI 的命中测试按指针位置算），再画一帧，
/// 然后读这一帧里生效的光标。少画这一帧读到的是上一帧的命中结果，不是当前
/// 指针位置该有的形状。所以这里每一处断言前都先 draw 一次。
#[test]
fn hovering_a_block_uses_open_hand_cursor() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    cx.simulate_resize(gpui::size(gpui::px(1200.), gpui::px(1200.)));
    install_items(cx, &view, vec![test_item(1, 1000, 1000, vec![])]);
    draw(cx);
    let canvas = cx.debug_bounds("canvas").expect("应有画布");
    let blank = gpui::point(
        canvas.origin.x + gpui::px(12.),
        canvas.origin.y + gpui::px(12.),
    );
    cx.simulate_mouse_move(blank, None, gpui::Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert_eq!(
        cx.cursor_style(),
        CursorStyle::Arrow,
        "画布空白处应是箭头（空白处左键是拖窗口，不是拖图片）"
    );

    let block = cx.debug_bounds("canvas-block-1").expect("应画出图块");
    cx.simulate_mouse_move(block.center(), None, gpui::Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert_eq!(
        cx.cursor_style(),
        CursorStyle::OpenHand,
        "指针压在图片上时应是张开的手，表示这张图可以抓起来移动"
    );

    // 移回空白处要变回箭头：光标跟着指针走，不是只在一帧里生效。
    cx.simulate_mouse_move(blank, None, gpui::Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert_eq!(
        cx.cursor_style(),
        CursorStyle::Arrow,
        "指针离开图片回到空白处后应恢复箭头"
    );
}

/// 需求 1：按住图片拖动排序时，光标变成「握紧的手」。
///
/// 为什么单独测这一条：gpui 在拖动启动那一刻就把源元素当时的光标冻进拖动记录，
/// 之后每帧由拖动记录接管，元素自己的 cursor 设置不再起作用。所以「拖起来之后
/// 还是张开的手」是一个真实可能发生的状态，必须实测而不是照着 cursor 的写法推断。
#[test]
fn dragging_a_block_uses_closed_hand_cursor() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    cx.simulate_resize(gpui::size(gpui::px(1200.), gpui::px(1200.)));
    install_items(cx, &view, vec![test_item(1, 1000, 1000, vec![])]);
    draw(cx);
    let start = cx
        .debug_bounds("canvas-block-1")
        .expect("应画出图块")
        .center();
    cx.simulate_mouse_move(start, None, gpui::Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert_eq!(
        cx.cursor_style(),
        CursorStyle::OpenHand,
        "抓住之前应是张开的手"
    );

    cx.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    // GPUI 用越过阈值的第一次移动激活拖动，第二步才把拖动派发到作品上，
    // 拖动中的光标是在那一步才被改成握紧的手。
    let mut last = start;
    for step in 1..=3 {
        last = gpui::point(
            start.x + gpui::px(step as f32 * 10.0),
            start.y + gpui::px(step as f32 * 10.0),
        );
        cx.simulate_mouse_move(last, Some(gpui::MouseButton::Left), gpui::Modifiers::default());
        cx.run_until_parked();
    }
    draw(cx);
    assert!(
        cx.read_entity(&view, |root, _| root.dragging.is_some()),
        "指针移动超过阈值后应进入拖动排序"
    );
    assert_eq!(
        cx.cursor_style(),
        CursorStyle::ClosedHand,
        "拖动图片排序时应是握紧的手"
    );
    cx.simulate_mouse_up(last, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
}

/// 需求 1：四条裁剪边各自显示对应方向的双向箭头。
///
/// 左右两条边改宽度，所以是左右箭头；上下两条边改高度，所以是上下箭头。
/// 排到排版模式才有左右两条边（字幕模式只裁上下）。
///
/// 每条边测之前都先把指针挪到画布空白处，断言一次箭头，再挪到热区断言该边的
/// 光标。这一步不能省：左右两条边期望值相同、上下两条边期望值也相同，如果哪一次
/// 移动事件丢了，断言会读到上一条边的残留光标值而照样通过，用例就成了空转。
/// 先回空白处并断言箭头，等于把每个点的起点复位，四次读数才互相独立。
#[test]
fn crop_hotzones_use_resize_cursors() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    cx.simulate_resize(gpui::size(gpui::px(1200.), gpui::px(1200.)));
    install_items(cx, &view, vec![test_item(1, 1000, 1000, vec![])]);
    cx.update_entity(&view, |root, cx| root.set_mode(Mode::Layout, cx));
    draw(cx);
    let canvas = cx.debug_bounds("canvas").expect("应有画布");
    let blank = gpui::point(
        canvas.origin.x + gpui::px(12.),
        canvas.origin.y + gpui::px(12.),
    );
    for (selector, expected, axis) in [
        ("crop-hotzone-1-left", CursorStyle::ResizeLeftRight, "左"),
        ("crop-hotzone-1-right", CursorStyle::ResizeLeftRight, "右"),
        ("crop-hotzone-1-top", CursorStyle::ResizeUpDown, "上"),
        ("crop-hotzone-1-bottom", CursorStyle::ResizeUpDown, "下"),
    ] {
        let zone = cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("排版模式下应有{axis}边热区（{selector}）"));
        // 复位：先回空白处，确认光标确实变回箭头，后面读到的才是这一步的结果。
        cx.simulate_mouse_move(blank, None, gpui::Modifiers::default());
        cx.run_until_parked();
        draw(cx);
        assert_eq!(
            cx.cursor_style(),
            CursorStyle::Arrow,
            "去{axis}边之前把指针挪回空白处应恢复箭头，否则{axis}边的读数不可信"
        );
        cx.simulate_mouse_move(zone.center(), None, gpui::Modifiers::default());
        cx.run_until_parked();
        draw(cx);
        assert_eq!(
            cx.cursor_style(),
            expected,
            "{axis}边热区（{selector}）的光标应是 {expected:?}"
        );
    }
}

/// 需求 1/6：按住滚轮平移画布时是握紧的手，松开恢复箭头。
#[test]
fn middle_pan_uses_closed_hand_cursor() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    cx.simulate_resize(gpui::size(gpui::px(1200.), gpui::px(1200.)));
    install_items(cx, &view, vec![test_item(1, 1000, 2000, vec![])]);
    draw(cx);
    let canvas = cx.debug_bounds("canvas").expect("应有画布");
    let blank = gpui::point(
        canvas.origin.x + gpui::px(12.),
        canvas.origin.y + gpui::px(12.),
    );
    cx.simulate_mouse_move(blank, None, gpui::Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert_eq!(
        cx.cursor_style(),
        CursorStyle::Arrow,
        "没按滚轮时空白处应是箭头"
    );

    cx.simulate_mouse_down(blank, gpui::MouseButton::Middle, gpui::Modifiers::default());
    cx.run_until_parked();
    assert!(
        cx.read_entity(&view, |root, _| root.is_middle_panning()),
        "按住滚轮应进入画布平移"
    );
    draw(cx);
    assert_eq!(
        cx.cursor_style(),
        CursorStyle::ClosedHand,
        "按住滚轮平移画布时应是握紧的手"
    );

    cx.simulate_mouse_up(blank, gpui::MouseButton::Middle, gpui::Modifiers::default());
    cx.run_until_parked();
    assert!(
        !cx.read_entity(&view, |root, _| root.is_middle_panning()),
        "松开滚轮应结束画布平移"
    );
    draw(cx);
    assert_eq!(
        cx.cursor_style(),
        CursorStyle::Arrow,
        "松开滚轮后应恢复箭头"
    );
}

// ----------------------------------------------------------------------
// 需求 4：滚轮缩放不受「适合画布」的限制，5%~400% 都能到。
// ----------------------------------------------------------------------

/// 需求 4：从「适合画布」起步，滚轮也能一路放大到上限、缩小到下限。
///
/// 这一条走的是真实事件路径：直接构造 ScrollWheelEvent 交给 simulate_event，
/// 由 gpui 自己派发到画布的 on_scroll_wheel，不直接调状态层的 zoom_by。
///
/// 不断言「一次滚轮涨多少」：gpui 可能对同一个事件在捕获和冒泡两个阶段各调一次
/// 处理函数，步数不是这条需求要保证的东西。这里反复滚到值不再变化再断言终值，
/// 覆盖四个点：从适合画布起步能超过 200%、能顶到上限 400%、能从 400% 缩到下限、
/// 缩的过程中会经过并低于 100%（也就是「从适合画布起步也能缩小」）。
#[test]
fn wheel_zoom_is_not_capped_by_fit() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    cx.simulate_resize(gpui::size(gpui::px(500.), gpui::px(400.)));
    // 图要比窗口大，适合画布才会把它压到 100% 以下，「从适合画布起步能超过 200%」
    // 才有意义：起点远低于 200%。700×525 在 500×400 里约 47%，够用。
    // 尺寸取小是为了快：这条测试要滚几十次，每次滚轮都重绘一次画布，
    // 重绘开销跟图片像素量成正比，像素多一倍这条测试就慢一倍。
    install_items(cx, &view, vec![test_item(1, 700, 525, vec![])]);
    draw(cx);
    let canvas = cx.debug_bounds("canvas").expect("应有画布");
    // 指针停在画布左上角的空白处，避开作品本身。
    let pointer = gpui::point(
        canvas.origin.x + gpui::px(12.),
        canvas.origin.y + gpui::px(12.),
    );
    let (fit, fit_scale) = cx.read_entity(&view, |root, _| {
        (root.view_scale().is_fit(), root.canvas_layout().scale)
    });
    assert!(fit, "起点应是自动适应（适合画布）");
    assert!(
        fit_scale < 0.5,
        "大图在适合画布下应远低于 100%，实际 {fit_scale}"
    );

    let wheel = |dy: f32| gpui::ScrollWheelEvent {
        position: pointer,
        delta: gpui::ScrollDelta::Lines(gpui::point(0.0, dy)),
        modifiers: gpui::Modifiers {
            control: true,
            ..Default::default()
        },
        touch_phase: gpui::TouchPhase::Moved,
    };
    let scale_now = |cx: &mut gpui::VisualTestContext| {
        cx.read_entity(&view, |root, _| root.canvas_layout().scale)
    };

    // 往上滚：一直滚到值不再变大为止。
    //
    // 上限用 60 而不是更大的数：每次滚轮乘 1.1，从头滚到 400% 约 31 次、从 400%
    // 滚回 5% 约 42 次，60 有富余。循环本来就会在值不再变化时 break，这个上限
    // 只是防止逻辑出错时死循环，给太大会让出问题时白跑几百次重绘。
    const MAX_WHEEL_STEPS: usize = 60;
    let mut previous = scale_now(cx);
    let mut past_two_hundred = false;
    let mut highest = previous;
    for _ in 0..MAX_WHEEL_STEPS {
        cx.simulate_event(wheel(1.0));
        cx.run_until_parked();
        let now = scale_now(cx);
        if (now - previous).abs() < 1e-6 {
            break;
        }
        previous = now;
        highest = highest.max(now);
        past_two_hundred |= now > 2.0;
    }
    let top = scale_now(cx);
    assert!(
        past_two_hundred,
        "从适合画布起步往上滚应该能超过 200%，实际最高只到过 {highest}"
    );
    assert!(
        (top - tokens::MANUAL_SCALE_MAX).abs() < 1e-4,
        "一直往上滚应到手动比例上限 {}，实际 {top}",
        tokens::MANUAL_SCALE_MAX
    );
    let mode = cx.read_entity(&view, |root, _| root.scale_mode.manual());
    assert_eq!(mode, Some(tokens::MANUAL_SCALE_MAX), "滚轮之后应切到手动比例");

    // 往下滚：一路缩到下限，中途会穿过并低于 100%。
    let mut previous = top;
    let mut below_one = false;
    let mut lowest = top;
    for _ in 0..MAX_WHEEL_STEPS {
        cx.simulate_event(wheel(-1.0));
        cx.run_until_parked();
        let now = scale_now(cx);
        if (now - previous).abs() < 1e-6 {
            break;
        }
        previous = now;
        lowest = lowest.min(now);
        below_one |= now < 1.0;
    }
    let bottom = scale_now(cx);
    assert!(
        below_one,
        "往下滚应该能缩到 100% 以下，实际最低只到过 {lowest}"
    );
    assert!(
        (bottom - tokens::MANUAL_SCALE_MIN).abs() < 1e-4,
        "一直往下滚应到手动比例下限 {}，实际 {bottom}",
        tokens::MANUAL_SCALE_MIN
    );
}
// ----------------------------------------------------------------------
// 需求 6：按住鼠标滚轮平移画布；左下角预览区平时是 30% 透明。
// ----------------------------------------------------------------------

/// 需求 6：按住滚轮拖动，画布偏移跟着指针走，并且被可滚范围夹住。
///
/// 这里只看两件事：偏移值和「请求移动窗口」的次数。不检查有没有触发重绘，
/// 因为模拟指针移动时会经过窗口右上角的关闭按钮区，那一块本来就会让界面刷新一次，
/// 重绘次数不能用来证明平移逻辑对不对。
#[test]
fn middle_button_drag_pans_the_canvas() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    canvas_that_overflows(cx, &view);
    // 缩到 1:1：三张 1000×2000 的图叠起来 6000 像素高、1000 像素宽，
    // 在 700×500 的窗口里横竖都能滚，跟手和夹紧才有得测。
    cx.update_entity(&view, |root, cx| root.set_manual_scale(1.0, cx));
    draw(cx);

    // 先把画布滚到中间，这样上下左右都有余量，能同时验证跟手和夹紧。
    cx.update_entity(&view, |root, _| {
        root.canvas_scroll
            .set_offset(gpui::point(gpui::px(-150.), gpui::px(-400.)));
    });
    draw(cx);
    let (start_offset, max) = cx.read_entity(&view, |root, _| {
        (root.canvas_scroll.offset(), root.canvas_scroll.max_offset())
    });
    assert!(
        f32::from(max.width) > 200.0 && f32::from(max.height) > 500.0,
        "三张 1000×2000 的图缩到 1:1 后在 700×500 窗口里应留出足够的可滚范围，实际 {max:?}"
    );

    let canvas = cx.debug_bounds("canvas").expect("应有画布");
    let start = gpui::point(
        canvas.origin.x + gpui::px(350.),
        canvas.origin.y + gpui::px(250.),
    );
    // 先让指针真的停在这里：画布的鼠标按下监听只在「指针压在画布上」时才响应，
    // 命中判断用的是指针位置，不先移过来按下去不会有人接。
    cx.simulate_mouse_move(start, None, gpui::Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    let before_moves = cx.read_entity(&view, |root, _| root.window_move_requests);
    cx.simulate_mouse_down(start, gpui::MouseButton::Middle, gpui::Modifiers::default());
    cx.run_until_parked();
    assert!(
        cx.read_entity(&view, |root, _| root.is_middle_panning()),
        "在画布上按下滚轮应进入平移"
    );

    // 指针往右下走 120/80 像素，画布内容应该跟着往右下走同样的距离。
    let end = gpui::point(start.x + gpui::px(120.), start.y + gpui::px(80.));
    cx.simulate_mouse_move(end, Some(gpui::MouseButton::Middle), gpui::Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    let after = cx.read_entity(&view, |root, _| root.canvas_scroll.offset());
    let dx = f32::from(after.x) - f32::from(start_offset.x);
    let dy = f32::from(after.y) - f32::from(start_offset.y);
    assert!(
        (dx - 120.0).abs() < 1.0,
        "指针右移 120 像素，画布偏移也该右移 120：{start_offset:?} -> {after:?}"
    );
    assert!(
        (dy - 80.0).abs() < 1.0,
        "指针下移 80 像素，画布偏移也该下移 80：{start_offset:?} -> {after:?}"
    );

    cx.simulate_mouse_up(end, gpui::MouseButton::Middle, gpui::Modifiers::default());
    cx.run_until_parked();

    // 夹紧要单独测：指针必须一直待在画布范围内，移到窗口外就没人接这个事件了，
    // 那样读出来的还是旧值，断言会「通过」但什么也没证明。所以两次都从一个内角
    // 拖到另一个内角，位移够大足以推出可滚范围。
    let inside = |fx: f32, fy: f32| {
        gpui::point(
            canvas.origin.x + canvas.size.width * fx,
            canvas.origin.y + canvas.size.height * fy,
        )
    };
    let reset_offset = |cx: &mut gpui::VisualTestContext| {
        cx.update_entity(&view, |root, _| {
            root.canvas_scroll
                .set_offset(gpui::point(gpui::px(-150.), gpui::px(-400.)));
        });
        draw(cx);
    };

    // 一次中键拖动：按住、移动、读偏移、松手。
    let middle_drag = |cx: &mut gpui::VisualTestContext,
                       from: gpui::Point<gpui::Pixels>,
                       to: gpui::Point<gpui::Pixels>| {
        cx.simulate_mouse_move(from, None, gpui::Modifiers::default());
        cx.run_until_parked();
        draw(cx);
        cx.simulate_mouse_down(from, gpui::MouseButton::Middle, gpui::Modifiers::default());
        cx.run_until_parked();
        assert!(
            cx.read_entity(&view, |root, _| root.is_middle_panning()),
            "在画布上按下滚轮应进入平移"
        );
        cx.simulate_mouse_move(to, Some(gpui::MouseButton::Middle), gpui::Modifiers::default());
        cx.run_until_parked();
        let offset = cx.read_entity(&view, |root, _| root.canvas_scroll.offset());
        cx.simulate_mouse_up(to, gpui::MouseButton::Middle, gpui::Modifiers::default());
        cx.run_until_parked();
        offset
    };

    // 往左上推超界：偏移应正好停在 -max，不会攒出范围外的值。
    //
    // 一次拖动最多只能走「窗口那么大」的距离，而纵向可滚范围有 1770 像素，
    // 所以这里连拖几次才够把纵向也推到尽头。
    reset_offset(cx);
    let first_push = middle_drag(cx, inside(0.97, 0.97), inside(0.03, 0.03));
    // 这一次横向位移是 -658 像素，从 -150 出发本来会到 -808，范围只允许到 -396。
    // 读到的正好是 -396，说明确实是被夹住的，不是碰巧算出来的。
    assert!(
        (f32::from(first_push.x) + f32::from(max.width)).abs() < 0.5,
        "横向推出范围后应正好停在 -max.width={:?}，实际 {:?}",
        max.width,
        first_push.x
    );
    let mut clamped = first_push;
    for _ in 0..8 {
        clamped = middle_drag(cx, inside(0.97, 0.97), inside(0.03, 0.03));
    }
    assert!(
        (f32::from(clamped.x) + f32::from(max.width)).abs() < 0.5
            && (f32::from(clamped.y) + f32::from(max.height)).abs() < 0.5,
        "一直往左上推应停在可滚范围的两端 (-{:?}, -{:?})，实际 {clamped:?}",
        max.width,
        max.height
    );
    // 再推一次，值不该再动：到边就停住。
    let pushed_again = middle_drag(cx, inside(0.97, 0.97), inside(0.03, 0.03));
    assert_eq!(
        (f32::from(pushed_again.x), f32::from(pushed_again.y)),
        (f32::from(clamped.x), f32::from(clamped.y)),
        "已经到边之后继续推，偏移不该再变"
    );

    // 往右下推超界：偏移应正好停在 0，画布内容不会被推离左上角。
    reset_offset(cx);
    let clamped_back = middle_drag(cx, inside(0.03, 0.03), inside(0.97, 0.97));
    assert!(
        f32::from(clamped_back.x).abs() < 0.5 && f32::from(clamped_back.y).abs() < 0.5,
        "往右下推超界时应停在 0：{clamped_back:?}"
    );
    // 顶到 0 之后再推，画面也不该被推离左上角。
    let still_zero = middle_drag(cx, inside(0.03, 0.03), inside(0.97, 0.97));
    assert!(
        f32::from(still_zero.x).abs() < 0.5 && f32::from(still_zero.y).abs() < 0.5,
        "顶到 0 之后继续推仍应停在 0：{still_zero:?}"
    );
    let after_moves = cx.read_entity(&view, |root, _| root.window_move_requests);
    assert_eq!(
        after_moves, before_moves,
        "滚轮平移只动画布，绝不能请求移动窗口"
    );
    assert_eq!(
        cx.read_entity(&view, |root, _| root.project.items.len()),
        3,
        "滚轮平移不该改动作品内容"
    );
}

/// 需求 6：左下角预览区不悬停时是 30% 透明度。
///
/// 常量本身也要卡死：界面和测试都读同一个值，万一有人把常量调大，
/// 这条断言会立刻红，而不是两边一起悄悄改掉。
#[test]
fn navigator_idle_opacity_is_thirty_percent() {
    assert_eq!(tokens::NAVIGATOR_IDLE_OPACITY, 0.30, "闲置透明度常量应是 30%");
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    canvas_that_overflows(cx, &view);
    let idle = cx.read_entity(&view, |root, app| {
        root.navigator_opacity_at(app.background_executor().now())
    });
    assert!(
        (idle - 0.30).abs() < 1e-6,
        "没悬停时预览区应是 30% 透明，实际 {idle}"
    );
    // 导航器确实在，别把「压根没画」当成「透明度对」。
    assert!(cx.debug_bounds("navigator").is_some(), "作品溢出时应有预览区");
}

// ----------------------------------------------------------------------
// 需求 8：接缝死区按点验证；被作用的那张图有整圈亮边。
// ----------------------------------------------------------------------

/// 需求 8：两张图紧贴时，正按在接缝中线上什么边都不该拿到。
///
/// 这一条量的是「按在哪一点」，不是「热区画在哪」。用户反馈的现象是：
/// 想调第二张的顶边，手一抖按到接缝上，结果调成了第一张的底边，画面整个
/// 跳到第一张。所以必须拿真实坐标去按，而不是只看元素边框的位置。
///
/// 死区是 SEAM_DEADZONE(6 像素) 均分在接缝两侧，两侧热区各自再往里缩 3 像素，
/// 于是贴着接缝那 6 像素宽的带子里两边都不接鼠标。接缝中线正好落在这条带子
/// 正中间，是死区最核心的点。
///
/// 注意这条用例里「按不中」和「不是拖窗口」两段本身是不吃劲的：万一鼠标事件根本
/// 没人接收，这两段断言照样成立。真正证明死区确实生效的是最后那段正面证据 ——
/// 同一窗口里偏 4 像素和 6 像素按下去，必须分别拿到第一张的底边和第二张的顶边。
/// 两边的对照都成立，才说明中间按不中是死区在挡，而不是事件丢了。那段不要删。
#[test]
fn seam_deadzone_swallows_both_edges_when_gap_is_zero() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    cx.simulate_resize(gpui::size(gpui::px(1200.), gpui::px(1200.)));
    install_items(cx, &view, stacked_items());
    draw(cx);

    let first = cx.debug_bounds("canvas-block-1").expect("第一张应在画布上");
    let second = cx.debug_bounds("canvas-block-2").expect("第二张应在画布上");
    // 间距为 0 时两块图紧贴：第一张的底边就是第二张的顶边。
    assert!(
        (f32::from(first.bottom()) - f32::from(second.origin.y)).abs() < 1.0,
        "两张图应紧贴：第一张底 {:?} 第二张顶 {:?}",
        first.bottom(),
        second.origin.y
    );
    let seam_y = first.bottom();
    let seam_x = first.center().x;
    let deadzone = f32::from(tokens::SEAM_DEADZONE);
    assert!(deadzone >= 4.0, "死区常量应够宽，实际 {deadzone}");

    // 按在接缝中线上：既不该拿到第一张的底边，也不该拿到第二张的顶边，
    // 更不该退化成「按空白处拖窗口」。
    for offset in [0.0_f32, 1.0, -1.0, 2.0, -2.0] {
        let target = gpui::point(seam_x, seam_y + gpui::px(offset));
        let moves_before = cx.read_entity(&view, |root, _| root.window_move_requests);
        cx.simulate_mouse_move(target, None, gpui::Modifiers::default());
        cx.run_until_parked();
        draw(cx);
        cx.simulate_mouse_down(target, gpui::MouseButton::Left, gpui::Modifiers::default());
        cx.run_until_parked();
        let (drag, moves) = cx.read_entity(&view, |root, _| {
            (
                root.crop_drag
                    .as_ref()
                    .map(|state| (state.id, state.edge.name().to_string())),
                root.window_move_requests,
            )
        });
        assert_eq!(
            drag, None,
            "接缝中线偏 {offset} 像素处不该开始任何裁剪拖动，实际 {drag:?}"
        );
        assert_eq!(
            moves, moves_before,
            "接缝中线偏 {offset} 像素处不该被当成空白处去拖窗口"
        );
        cx.simulate_mouse_up(target, gpui::MouseButton::Left, gpui::Modifiers::default());
        cx.run_until_parked();
    }

    // 离开死区半宽 + 热区厚度之外，两条边就该各自认得出来：
    // 往上走落到第一张的底边，往下走落到第二张的顶边。
    // 死区半宽 3 像素、热区厚 16 像素，取 4 像素和 6 像素两档，
    // 4 像素这一档特意留在死区正外侧，验证「差一点点就能按到」。
    //
    // 按边之前必须先点一下要裁的那张图把它选中：现在的规则是只要有图被选中，
    // 没选中的图整个不接悬停和裁剪边。这个点击不是多余的，它让目标图拿到热区；
    // 少了它，按到的是没选中的图，拿到 None 是正确行为。
    for offset in [4.0_f32, 6.0] {
        let first_block = cx.debug_bounds("canvas-block-1").expect("第一张应在画布上");
        cx.simulate_click(first_block.center(), gpui::Modifiers::default());
        cx.run_until_parked();
        settle_selection(cx);
        draw(cx);
        let above = gpui::point(seam_x, seam_y - gpui::px(offset));
        let (got_above, _) = press_point(cx, &view, above);
        assert_eq!(
            got_above,
            Some((1, "bottom".to_string())),
            "先选中第一张，再按接缝上方 {offset} 像素，应拿到第一张的底边，实际 {got_above:?}"
        );

        let second_block = cx.debug_bounds("canvas-block-2").expect("第二张应在画布上");
        cx.simulate_click(second_block.center(), gpui::Modifiers::default());
        cx.run_until_parked();
        settle_selection(cx);
        draw(cx);
        let below = gpui::point(seam_x, seam_y + gpui::px(offset));
        let (got_below, _) = press_point(cx, &view, below);
        assert_eq!(
            got_below,
            Some((2, "top".to_string())),
            "先选中第二张，再按接缝下方 {offset} 像素，应拿到第二张的顶边，实际 {got_below:?}"
        );
    }
}

/// 需求 8：指针停在哪张图上，哪张图就整圈描一条细亮边。
///
/// 「整圈」要拿四条边分别核对，不能只看元素在不在：亮边如果只有一条线，
/// 用户还是分不清现在动的是哪一张。
#[test]
fn hovered_block_gets_a_full_focus_ring() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    cx.simulate_resize(gpui::size(gpui::px(1200.), gpui::px(1200.)));
    install_items(cx, &view, stacked_items());
    draw(cx);
    // 一眼没看之前不该有亮边：debug_bounds 只保留当前帧（每帧清空），
    // 这一帧指针还没移进去，所以读不到 block-focus；移进去重画一帧才会出现。
    assert!(
        cx.debug_bounds("block-focus-1").is_none() && cx.debug_bounds("block-focus-2").is_none(),
        "还没悬停时不该有亮边"
    );

    let first = cx.debug_bounds("canvas-block-1").expect("第一张应在画布上");
    let second = cx.debug_bounds("canvas-block-2").expect("第二张应在画布上");
    cx.simulate_mouse_move(first.center(), None, gpui::Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    let ring = cx.debug_bounds("block-focus-1").expect("第一张应有亮边");
    // 亮边按图块范围画；选中卡片取消浮起后和图块完全重合，两边应严丝合缝。
    // 这里仍按卡片对照，是为了守住亮边不向四周外扩。
    let card = cx
        .debug_bounds("selection-card-1")
        .expect("第一张应有抬起卡片");
    for (name, a, b, axis) in [
        ("左", f32::from(ring.origin.x), f32::from(card.origin.x), "x"),
        ("上", f32::from(ring.origin.y), f32::from(card.origin.y), "y"),
        ("右", f32::from(ring.right()), f32::from(card.right()), "x"),
        (
            "下",
            f32::from(ring.bottom()),
            f32::from(card.bottom()),
            "y",
        ),
    ] {
        assert!(
            (a - b).abs() < 1.0,
            "亮边的{name}沿应贴住图块（{axis} 轴）：亮边 {a} 图块 {b}"
        );
    }
    // 亮边只在被悬停的那一张上。第二张的亮边不在这个窗口里出现过，
    // 所以这里能直接断言它缺失。
    assert!(
        cx.debug_bounds("block-focus-2").is_none(),
        "只有指针压着的那张图该有亮边"
    );

    // 换到第二张：亮边跟着指针走。
    cx.simulate_mouse_move(second.center(), None, gpui::Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    let ring2 = cx.debug_bounds("block-focus-2").expect("第二张应有亮边");
    // 选中的卡片都抬起来了，第二张也有一张原尺寸上移 8px 的卡片，亮边贴卡片。
    let card2 = cx
        .debug_bounds("selection-card-2")
        .expect("第二张应有抬起卡片");
    assert!(
        (f32::from(ring2.origin.y) - f32::from(card2.origin.y)).abs() < 1.0
            && (f32::from(ring2.bottom()) - f32::from(card2.bottom())).abs() < 1.0,
        "亮边应跟着指针挪到第二张并贴住卡片：{ring2:?} 卡片 {card2:?}"
    );
}

/// 在画布上的某一点按下左键，返回那一刻的 `(图号, 边名)` 和「拖窗口」次数。
///
/// 和 `press_edge` 的区别是这里按的是坐标而不是元素中心，用来验证死区边界。
fn press_point(
    cx: &mut gpui::VisualTestContext,
    view: &gpui::Entity<AppRoot>,
    target: gpui::Point<gpui::Pixels>,
) -> (Option<(ItemId, String)>, usize) {
    let moves_before = cx.read_entity(view, |root, _| root.window_move_requests);
    cx.simulate_mouse_move(target, None, gpui::Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    cx.simulate_mouse_down(target, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    let (got, moves) = cx.read_entity(view, |root, _| {
        (
            root.crop_drag
                .as_ref()
                .map(|state| (state.id, state.edge.name().to_string())),
            root.window_move_requests,
        )
    });
    cx.simulate_mouse_up(target, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    (got, moves - moves_before)
}

// ----------------------------------------------------------------------
// 需求 2：已经完整显示的边，继续往外拖不该动画面。
// ----------------------------------------------------------------------

/// 需求 2：图已经完整显示，往外拖它的边，每一步都不该动画面。
///
/// 用户报的现象是「第三张已经完整显示，还能往上拉，整个画面跳到第一张」。
/// `dragging_third_top_edge_does_not_jump_to_the_first_image` 那条只看滚动位置
/// 和第 1 张的顶有没有跑；这里补齐另外三样：每一步的裁剪数据一个字都不变、
/// 每一步的自动滚动累加量都不变、松手后裁剪数据仍是原来的。
///
/// 先滚到第三张再拖，是因为只有「正在看第三张」时画面跳走的后果才明显。
#[test]
fn dragging_a_saturated_edge_outwards_keeps_scroll_and_bands_still() {
    // 正向控制：在另一个全新窗口里，先证明「拖这条边真的会改到裁剪数据」。
    //
    // 这一步不能省。下面往外拖那 12 步断的全是「没变」，如果裁剪拖动改数据的
    // 整条链路坏掉了，那 12 步照样全过，用例就成了空转。有了这个正向控制，
    // 「没变」才说明是饱和早退在挡，而不是根本没接上。
    //
    // 控制组单独开窗口，是因为它必须真的把第三张裁一刀，而裁完内容变矮、滚动
    // 偏移会被夹回去，接着在原窗口里恢复再按就会落在过期的坐标上。两个窗口各自
    // 独立，互不干扰。
    {
        let mut cx = TestAppContext::single();
        let (view, cx) = cx.add_window_view(AppRoot::new);
        cx.simulate_resize(gpui::size(gpui::px(960.), gpui::px(600.)));
        install_items(
            cx,
            &view,
            vec![
                test_item(1, 1000, 3000, vec![]),
                test_item(2, 1000, 3000, vec![]),
                test_item(3, 1000, 3000, vec![]),
            ],
        );
        draw(cx);
        draw(cx);
        // 和主体一样先滚到第三张，否则它在视口外，按不到。
        cx.update_entity(&view, |root, cx| {
            let layout = root.canvas_layout();
            let third = layout.entry(3).expect("应有第 3 张").rect.y;
            root.canvas_scroll
                .set_offset(gpui::point(gpui::px(0.), gpui::px(20.0 - third)));
            cx.notify();
        });
        draw(cx);
        let zone = cx
            .debug_bounds("crop-hotzone-3-top")
            .expect("正向控制窗口里第 3 张应有上边缘热区");
        let start = zone.center();
        cx.simulate_mouse_move(start, None, gpui::Modifiers::default());
        cx.run_until_parked();
        draw(cx);
        cx.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::default());
        cx.run_until_parked();
        let armed = cx.read_entity(&view, |root, _| {
            root.crop_drag
                .as_ref()
                .map(|state| (state.id, state.edge.name().to_string()))
        });
        assert_eq!(
            armed,
            Some((3, "top".to_string())),
            "正向控制：按第 3 张的上边缘应拖它自己的上边，实际 {armed:?}"
        );
        // 朝图片里面拖 90 像素：第一下用来起拖，第二下才真正派发。
        let inward_end = gpui::point(start.x, start.y + gpui::px(90.0));
        for offset in [60.0_f32, 90.0] {
            let inward = gpui::point(start.x, start.y + gpui::px(offset));
            cx.simulate_mouse_move(
                inward,
                Some(gpui::MouseButton::Left),
                gpui::Modifiers::default(),
            );
            cx.run_until_parked();
            draw(cx);
        }
        let moved = cx.read_entity(&view, |root, _| root.project.items[2].manual);
        assert!(
            moved.is_some_and(|band| band.top > 0.02),
            "正向控制：朝里拖 90 像素必须真的裁掉第 3 张的顶部，否则下面那 12 步证明不了任何东西，实际 {moved:?}"
        );
        cx.simulate_mouse_up(inward_end, gpui::MouseButton::Left, gpui::Modifiers::default());
        cx.run_until_parked();
    }

    // 主体：全新窗口，三张都完整显示，第三张的顶边朝外已经没有余量。
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    cx.simulate_resize(gpui::size(gpui::px(960.), gpui::px(600.)));
    install_items(
        cx,
        &view,
        vec![
            test_item(1, 1000, 3000, vec![]),
            test_item(2, 1000, 3000, vec![]),
            test_item(3, 1000, 3000, vec![]),
        ],
    );
    draw(cx);
    draw(cx);
    // 滚到「第三张的顶边在视口上方一点」，也就是用户正在看第三张的位置。
    cx.update_entity(&view, |root, cx| {
        let layout = root.canvas_layout();
        let third = layout.entry(3).expect("应有第 3 张").rect.y;
        root.canvas_scroll
            .set_offset(gpui::point(gpui::px(0.), gpui::px(20.0 - third)));
        cx.notify();
    });
    draw(cx);

    // 三张都是「完整显示」（没有手动裁剪），所以第三张的顶边朝外已经没有余量。
    let bands_before = cx.read_entity(&view, |root, _| {
        root.project
            .items
            .iter()
            .map(|item| item.manual)
            .collect::<Vec<_>>()
    });
    assert!(
        bands_before.iter().all(|band| band.is_none()),
        "起点应是三张都没有手动裁剪：{bands_before:?}"
    );
    let zone = cx
        .debug_bounds("crop-hotzone-3-top")
        .expect("第 3 张应有上边缘热区");
    let start = zone.center();
    cx.simulate_mouse_move(start, None, gpui::Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    let dragged = cx.read_entity(&view, |root, _| {
        root.crop_drag
            .as_ref()
            .map(|state| (state.id, state.edge.name().to_string()))
    });
    assert_eq!(
        dragged,
        Some((3, "top".to_string())),
        "按第 3 张的上边缘应拖它自己的上边，实际 {dragged:?}"
    );

    let snapshot = |cx: &mut gpui::VisualTestContext| {
        cx.read_entity(&view, |root, _| {
            let auto = root
                .crop_drag
                .as_ref()
                .map(|state| (state.auto_scroll_pixels.x, state.auto_scroll_pixels.y))
                .unwrap_or((0.0, 0.0));
            (
                f32::from(root.canvas_scroll.offset().y),
                auto,
                root.project
                    .items
                    .iter()
                    .map(|item| item.manual)
                    .collect::<Vec<_>>(),
            )
        })
    };
    let before = snapshot(cx);

    // 往上拖（朝图片外面）12 步，指针一直贴到视口上沿：每一步都该什么都没发生。
    let mut last = start;
    for step in 1..=12 {
        last = gpui::point(start.x, start.y - gpui::px(step as f32 * 60.0));
        cx.simulate_mouse_move(last, Some(gpui::MouseButton::Left), gpui::Modifiers::default());
        cx.run_until_parked();
        draw(cx);
        let now = snapshot(cx);
        assert!(
            (now.0 - before.0).abs() < 0.5,
            "第 {step} 步：滚动位置不该动（拖动前 {:.1}，现在 {:.1}）",
            before.0,
            now.0
        );
        assert!(
            (now.1.0 - before.1.0).abs() < 0.01 && (now.1.1 - before.1.1).abs() < 0.01,
            "第 {step} 步：自动滚动不该推进（拖动前 {:?}，现在 {:?}）",
            before.1,
            now.1
        );
        assert_eq!(
            now.2, before.2,
            "第 {step} 步：三张图的裁剪数据都不该变"
        );
    }
    cx.simulate_mouse_up(last, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    // 松手之后数据仍是原来的：顶住了就不该留下任何痕迹。
    let after = cx.read_entity(&view, |root, _| {
        root.project
            .items
            .iter()
            .map(|item| item.manual)
            .collect::<Vec<_>>()
    });
    assert_eq!(after, bands_before, "松手后裁剪数据也不该变");
}

/// 需求 2 + 整组裁剪：组里第 1 张已经裁过、被拖的那张已经顶住时，往外拖也不该推进自动滚动。
///
/// 场景来自用户报的「调第三张的边，画面突然跳到第一张」。条件是这三样同时成立：
///
/// 1. 排版模式 + 横排，拖上边时整组一起改（`group_follows` 为真），所以组里每张图
///    都被算进这次拖动的目标里。
/// 2. 第 1 张的上边已经手动裁到 0.3，也就是它「还有余量可裁」。
/// 3. 被拖的第 3 张和第 2 张都还是完整的一张，上边已经顶在 0.0，没有余量了。
///
/// 这种组合下两套判据会打架：吸附按「被拖的第 3 张」算，它已经贴住顶，吸附把这一步
/// 的位移钉成 0；而整组饱和判断按「全组」算，第 1 张还有余量，所以不算饱和。两套判据
/// 都不拦，代码就一路走到自动滚动，画面持续往作品开头跑。
///
/// 所以这里每一步都同时盯四件事：画面不动、自动滚动累加量不动、三张图的裁剪都没变、
/// 按下时确实抓到了第 3 张的上边。只看其中一件会空转：只看画面不动，如果拖动链路整个
/// 坏掉也照样过。
#[test]
fn mixed_crop_group_drag_outwards_keeps_scroll_and_bands_still() {
    // 正向控制：另开一个窗口，证明「按第 3 张的上边缘往里拖」真的会改到裁剪数据。
    // 没有这一步，下面那些「没变」证明不了是早退在挡，还是整条链路根本没接上。
    {
        let mut cx = TestAppContext::single();
        let (view, cx) = cx.add_window_view(AppRoot::new);
        cx.simulate_resize(gpui::size(gpui::px(960.), gpui::px(600.)));
        install_items(
            cx,
            &view,
            vec![
                test_item(1, 250, 900, vec![]),
                test_item(2, 250, 900, vec![]),
                test_item(3, 250, 900, vec![]),
            ],
        );
        cx.update_entity(&view, |root, cx| {
            root.project.settings.layout = crate::model::LayoutTarget::Row;
            root.set_mode(Mode::Layout, cx);
            let band = crate::model::ManualBand::clamped(0.0, 1.0, 0.3, 1.0).expect("裁过的带");
            root.apply_band_to_items(&[1], band);
            root.canvas_scroll
                .set_offset(gpui::point(gpui::px(0.), gpui::px(-20.0)));
            cx.notify();
        });
        draw(cx);
        draw(cx);
        let zone = cx
            .debug_bounds("crop-hotzone-3-top")
            .expect("正向控制窗口里第 3 张应有上边缘热区");
        let start = zone.center();
        cx.simulate_mouse_move(start, None, gpui::Modifiers::default());
        cx.run_until_parked();
        draw(cx);
        cx.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::default());
        cx.run_until_parked();
        let armed = cx.read_entity(&view, |root, _| {
            root.crop_drag
                .as_ref()
                .map(|state| (state.id, state.edge.name().to_string()))
        });
        assert_eq!(
            armed,
            Some((3, "top".to_string())),
            "正向控制：按第 3 张的上边缘应拖它自己的上边，实际 {armed:?}"
        );
        // 朝图片里面拖（往下）90 像素：第 3 张的顶会被真的裁掉。
        let end = gpui::point(start.x, start.y + gpui::px(90.0));
        for offset in [60.0_f32, 90.0] {
            cx.simulate_mouse_move(
                gpui::point(start.x, start.y + gpui::px(offset)),
                Some(gpui::MouseButton::Left),
                gpui::Modifiers::default(),
            );
            cx.run_until_parked();
            draw(cx);
        }
        let moved = cx.read_entity(&view, |root, _| root.current_manual_for(3));
        assert!(
            moved.is_some_and(|band| band.top > 0.02),
            "正向控制：朝里拖 90 像素必须真的裁掉第 3 张的顶部，否则下面证明不了任何东西，实际 {moved:?}"
        );
        cx.simulate_mouse_up(end, gpui::MouseButton::Left, gpui::Modifiers::default());
        cx.run_until_parked();
    }

    // 第二个正向控制：窗口、三张图的尺寸、滚动位置、按下去的坐标、往上拖的步长全都
    // 和主体一模一样，只把「被拖的第 3 张自己有没有余量」这一个条件换掉——给它也先裁
    // 到 0.3。这样这一步真的会改到数据，自动滚动就该照常推进。
    //
    // 这条控制说明主体里那 6 步的「没动」不是空转：自动滚动这套机器在同一种排版、
    // 同一个滚动位置、同一条指针轨迹下确实会触发，只是被「这一步什么都没改」挡掉了。
    // 如果哪天自动滚动整个失效，这里就会失败，主体那边也就不会再假装通过。
    {
        let mut cx = TestAppContext::single();
        let (view, cx) = cx.add_window_view(AppRoot::new);
        cx.simulate_resize(gpui::size(gpui::px(960.), gpui::px(600.)));
        install_items(
            cx,
            &view,
            vec![
                test_item(1, 250, 900, vec![]),
                test_item(2, 250, 900, vec![]),
                test_item(3, 250, 900, vec![]),
            ],
        );
        cx.update_entity(&view, |root, cx| {
            root.project.settings.layout = crate::model::LayoutTarget::Row;
            root.set_mode(Mode::Layout, cx);
            let band = crate::model::ManualBand::clamped(0.0, 1.0, 0.3, 1.0).expect("裁过的带");
            // 三张都裁到 0.3：被拖的第 3 张自己也贴着 0.3，往外拖还有余量可用。
            root.apply_band_to_items(&[1, 2, 3], band);
            root.canvas_scroll
                .set_offset(gpui::point(gpui::px(0.), gpui::px(-20.0)));
            cx.notify();
        });
        draw(cx);
        draw(cx);
        let zone = cx
            .debug_bounds("crop-hotzone-3-top")
            .expect("正向控制窗口里第 3 张应有上边缘热区");
        let start = zone.center();
        cx.simulate_mouse_move(start, None, gpui::Modifiers::default());
        cx.run_until_parked();
        draw(cx);
        cx.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::default());
        cx.run_until_parked();
        let mut last = start;
        for step in 1..=6 {
            let y = (f32::from(start.y) - step as f32 * 4.0).max(1.0);
            last = gpui::point(start.x, gpui::px(y));
            cx.simulate_mouse_move(last, Some(gpui::MouseButton::Left), gpui::Modifiers::default());
            cx.run_until_parked();
            draw(cx);
        }
        let advanced = cx.read_entity(&view, |root, _| {
            let auto = root
                .crop_drag
                .as_ref()
                .map(|state| state.auto_scroll_pixels.y)
                .unwrap_or(0.0);
            (f32::from(root.canvas_scroll.offset().y), auto)
        });
        assert!(
            advanced.1 > 0.0 && (advanced.0 + 20.0).abs() > 0.5,
            "正向控制：被拖的那张有余量时，往上拖必须真的触发自动滚动，实际偏移 {:.1}、累加量 {:.1}",
            advanced.0,
            advanced.1
        );
        cx.simulate_mouse_up(last, gpui::MouseButton::Left, gpui::Modifiers::default());
        cx.run_until_parked();
    }

    // 主体：全新窗口，第 1 张裁到 0.3，第 2、3 张保持完整。
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    cx.simulate_resize(gpui::size(gpui::px(960.), gpui::px(600.)));
    install_items(
        cx,
        &view,
        vec![
            test_item(1, 250, 900, vec![]),
            test_item(2, 250, 900, vec![]),
            test_item(3, 250, 900, vec![]),
        ],
    );
    cx.update_entity(&view, |root, cx| {
        root.project.settings.layout = crate::model::LayoutTarget::Row;
        root.set_mode(Mode::Layout, cx);
        let band = crate::model::ManualBand::clamped(0.0, 1.0, 0.3, 1.0).expect("裁过的带");
        root.apply_band_to_items(&[1], band);
        // 横排的三张图顶边都落在画布同一条线上（排版给的 y 都是 0），所以把画布往上
        // 推 20 像素，让第 3 张的顶边热区落在视口上沿附近：这样往外拖处在自动滚动的
        // 触发区里，同时画布上方还留着 20 像素的滚动余量，旧代码那种「悄悄滚一点」
        // 才看得见。推得更多热区就跑到窗口外面，按不到了。
        root.canvas_scroll
            .set_offset(gpui::point(gpui::px(0.), gpui::px(-20.0)));
        cx.notify();
    });
    draw(cx);
    draw(cx);

    let full = crate::model::ManualBand::clamped(0.0, 1.0, 0.0, 1.0).expect("整图带");
    let cropped = crate::model::ManualBand::clamped(0.0, 1.0, 0.3, 1.0).expect("裁过的带");
    // 用「此刻生效的保留带」比，不用 `item.manual` 字段本身：完整显示的图字段是
    // `None`，可它此刻生效的保留带就是整张图。按生效值比，才既覆盖「字段是 None」
    // 也覆盖「字段被写成整图带」这两种情况。
    let bands_now = |cx: &mut gpui::VisualTestContext| {
        cx.read_entity(&view, |root, _| {
            (
                root.current_manual_for(1),
                root.current_manual_for(2),
                root.current_manual_for(3),
            )
        })
    };
    assert_eq!(
        bands_now(cx),
        (Some(cropped), Some(full), Some(full)),
        "起点：第 1 张应是上边 0.3 的带，第 2、3 张应是整图带"
    );

    let snapshot = |cx: &mut gpui::VisualTestContext| {
        cx.read_entity(&view, |root, _| {
            let auto = root
                .crop_drag
                .as_ref()
                .map(|state| (state.auto_scroll_pixels.x, state.auto_scroll_pixels.y))
                .unwrap_or((0.0, 0.0));
            (f32::from(root.canvas_scroll.offset().y), auto)
        })
    };

    let zone = cx
        .debug_bounds("crop-hotzone-3-top")
        .expect("第 3 张应有上边缘热区");
    let start = zone.center();
    cx.simulate_mouse_move(start, None, gpui::Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    let armed = cx.read_entity(&view, |root, _| {
        root.crop_drag
            .as_ref()
            .map(|state| (state.id, state.edge.name().to_string()))
    });
    assert_eq!(
        armed,
        Some((3, "top".to_string())),
        "按第 3 张的上边缘应拖它自己的上边，实际 {armed:?}"
    );

    let before = snapshot(cx);
    assert!(
        (before.0 + 20.0).abs() < 1.0,
        "起点滚动位置应是 -20，实际 {:.1}",
        before.0
    );

    // 往上（朝图片外面）分步拖 6 次，指针一直留在视口上沿附近，每一步都落在自动滚动的
    // 触发区里。第 1 步用来起拖，之后每一步都会真正派发。
    let mut last = start;
    for step in 1..=6 {
        let y = (f32::from(start.y) - step as f32 * 4.0).max(1.0);
        last = gpui::point(start.x, gpui::px(y));
        cx.simulate_mouse_move(last, Some(gpui::MouseButton::Left), gpui::Modifiers::default());
        cx.run_until_parked();
        draw(cx);
        let now = snapshot(cx);
        assert!(
            (now.0 - before.0).abs() < 0.5,
            "第 {step} 步：画面不该动（拖动前 {:.1}，现在 {:.1}）",
            before.0,
            now.0
        );
        assert!(
            (now.1.0 - before.1.0).abs() < 0.01 && (now.1.1 - before.1.1).abs() < 0.01,
            "第 {step} 步：自动滚动不该累加（拖动前 {:?}，现在 {:?}）",
            before.1,
            now.1
        );
        assert_eq!(
            bands_now(cx),
            (Some(cropped), Some(full), Some(full)),
            "第 {step} 步：第 1 张该保持 0.3，第 2、3 张该保持整图带"
        );
    }
    cx.simulate_mouse_up(last, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    assert_eq!(
        bands_now(cx),
        (Some(cropped), Some(full), Some(full)),
        "松手后三张图的裁剪也不该变"
    );
}

// ----------------------------------------------------------------------
// 需求 4：缩放胶囊上只有当前生效的那一项带高亮标记。
// ----------------------------------------------------------------------

/// 需求 4：缩放胶囊的选中标记落在真正生效的那一项上。
///
/// 分两个窗口是为了让「默认适合画布」和「切到 1:1」两种状态各从一个干净的
/// 窗口开始，互不依赖上一轮操作留下的状态。`debug_bounds` 只保留当前帧
/// （每帧清空），所以每处断言读到的都是这一帧真实的标记，不是历史累积。
#[test]
fn zoom_chip_marks_only_the_active_item() {
    // 窗口一：停在默认的「适合画布」。
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(cx, &view, vec![test_item(1, 3200, 2400, vec![])]);
    draw(cx);
    assert!(
        cx.read_entity(&view, |root, _| root.view_scale().is_fit()),
        "默认应是适合画布"
    );
    let fit_item = cx
        .debug_bounds("zoom-item-zoom-fit")
        .expect("应有适合画布项");
    assert_eq!(
        cx.debug_bounds("zoom-active-fit"),
        Some(fit_item),
        "适合画布是当前项，标记的范围应等于这一项自己的范围"
    );
    assert!(
        cx.debug_bounds("zoom-active-one-to-one").is_none(),
        "还没切到 1:1，不该有它的标记"
    );

    // 窗口二：切到 1:1，标记应落到 1:1 那一项上。
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(cx, &view, vec![test_item(1, 3200, 2400, vec![])]);
    cx.update_entity(&view, |root, cx| root.action_zoom_one_to_one(cx));
    draw(cx);
    assert!(
        !cx.read_entity(&view, |root, _| root.view_scale().is_fit()),
        "点过 1:1 之后不该还是自动适应"
    );
    let one_item = cx
        .debug_bounds("zoom-item-zoom-one-to-one")
        .expect("应有 1:1 项");
    assert_eq!(
        cx.debug_bounds("zoom-active-one-to-one"),
        Some(one_item),
        "1:1 是当前项，标记的范围应等于这一项自己的范围"
    );
    let fit_item = cx
        .debug_bounds("zoom-item-zoom-fit")
        .expect("应有适合画布项");
    assert!(
        one_item.origin.x > fit_item.origin.x,
        "1:1 项应在适合画布项右边，实际 {:?} 对 {:?}",
        one_item.origin,
        fit_item.origin
    );
}

// ----------------------------------------------------------------------
// 需求 5：说明气泡贴在各自按钮正上方，画布中间不再重复提示。
// ----------------------------------------------------------------------

/// 需求 5：胶囊按钮的说明贴在按钮自己头上，位置对得上。
///
/// 除了「有没有」还要验「贴得对不对」：说明底边应比按钮顶边高出 HINT_GAP，
/// 横向要和按钮重叠。位置错了说明还是浮在别处，等于没修。
#[test]
fn hovering_a_capsule_button_has_no_centre_bubble() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(
        cx,
        &view,
        vec![test_item(1, 1000, 1000, vec![line(700.0, 760.0)])],
    );
    draw(cx);
    let canvas = cx.debug_bounds("canvas").expect("应有画布");
    let canvas_mid_y = f32::from(canvas.origin.y) + f32::from(canvas.size.height) / 2.0;
    for (button, hint, name) in [
        ("capsule-add", "hover-hint-capsule-add", "添加"),
        ("capsule-copy", "hover-hint-capsule-copy", "复制"),
        ("export-png", "hover-hint-export-png", "导出"),
    ] {
        let bounds = cx
            .debug_bounds(button)
            .unwrap_or_else(|| panic!("胶囊里应有{name}按钮"));
        cx.simulate_mouse_move(bounds.center(), None, gpui::Modifiers::default());
        cx.run_until_parked();
        draw(cx);
        let bubble = cx
            .debug_bounds(hint)
            .unwrap_or_else(|| panic!("指着{name}按钮时应有它的说明"));
        // 说明贴在按钮正上方：底边比按钮顶边高 HINT_GAP。
        let gap = f32::from(bounds.origin.y) - f32::from(bubble.bottom());
        assert!(
            (gap - f32::from(crate::ui::hint::HINT_GAP)).abs() < 1.5,
            "{name}的说明底边应比按钮顶边高 {}，实际差 {gap}（说明 {bubble:?} 按钮 {bounds:?}）",
            f32::from(crate::ui::hint::HINT_GAP)
        );
        // 横向要和按钮叠在一起，说明才是「贴着这个按钮」而不是飘在别处。
        assert!(
            f32::from(bubble.origin.x) < f32::from(bounds.right())
                && f32::from(bounds.origin.x) < f32::from(bubble.right()),
            "{name}的说明应和按钮横向重叠：说明 {bubble:?} 按钮 {bounds:?}"
        );
        // 需求 5：说明不再出现在画布垂直正中。
        let bubble_mid_y = f32::from(bubble.origin.y) + f32::from(bubble.size.height) / 2.0;
        assert!(
            (bubble_mid_y - canvas_mid_y).abs() > 20.0,
            "{name}的说明不该落在画布垂直正中（说明中点 {bubble_mid_y}，画布中点 {canvas_mid_y}）"
        );
    }
}

/// 需求 5：排版模式的横排、竖排图标预设，说明同样贴在按钮正上方。
#[test]
fn layout_preset_hints_sit_next_to_their_buttons() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(
        cx,
        &view,
        vec![test_item(1, 1000, 1000, vec![line(700.0, 760.0)])],
    );
    cx.update_entity(&view, |root, cx| root.set_mode(Mode::Layout, cx));
    draw(cx);
    for (selector, hint, name) in [
        ("layout-preset-4", "hover-hint-layout-row", "横排"),
        ("layout-preset-5", "hover-hint-layout-column", "竖排"),
    ] {
        let button = cx
            .debug_bounds(selector)
            .unwrap_or_else(|| panic!("排版模式应有{name}预设"));
        cx.simulate_mouse_move(button.center(), None, gpui::Modifiers::default());
        cx.run_until_parked();
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(700));
        cx.run_until_parked();
        draw(cx);
        let bubble = cx
            .debug_bounds(hint)
            .unwrap_or_else(|| panic!("指着{name}预设时应有它的说明"));
        let gap = f32::from(button.origin.y) - f32::from(bubble.bottom());
        assert!(
            (gap - f32::from(crate::ui::hint::HINT_GAP)).abs() < 1.5,
            "{name}的说明底边应比按钮顶边高 {}，实际差 {gap}（说明 {bubble:?} 按钮 {button:?}）",
            f32::from(crate::ui::hint::HINT_GAP)
        );
        assert!(
            f32::from(bubble.origin.x) < f32::from(button.right())
                && f32::from(button.origin.x) < f32::from(bubble.right()),
            "{name}的说明应和按钮横向重叠：说明 {bubble:?} 按钮 {button:?}"
        );
    }
}

// ----------------------------------------------------------------------
// 需求 7：不管横排还是竖排，「适合画布」显示出来一样大。
// ----------------------------------------------------------------------

/// 需求 7：同一批图换成横排、竖排，算出来的显示比例必须一样。
///
/// 用户报的是「竖向小一些横向大一些」。老做法拿整幅排版画布的最短边当基准，
/// 横排画布是「宽而矮」、竖排画布是「窄而高」，两种画布的短边差很多，比例自然
/// 一大一小。所以这里专门挑一组长宽比不同的图，让两种排版画布的外框明显不同，
/// 再要求比例相等：只有基准真的改成「看图自己」才可能通过。
///
/// 这里比的是显示比例。每张图自己的像素尺寸在两种排版下现在已经一致了
/// （`arrange_row` 和 `arrange_column` 都让每张图保留自己的长宽比），
/// 那一半由 `row_and_column_give_every_image_the_same_size` 单独盯着。
#[test]
fn row_and_column_fit_scales_match() {
    // 两种排版各用一个全新窗口，唯一变量只有排版方向。
    let mut measured: Vec<(f32, f32, f32)> = Vec::new();
    for target in [
        crate::model::LayoutTarget::Row,
        crate::model::LayoutTarget::Column,
    ] {
        let mut cx = TestAppContext::single();
        let (view, cx) = cx.add_window_view(AppRoot::new);
        cx.simulate_resize(gpui::size(gpui::px(900.), gpui::px(700.)));
        install_items(
            cx,
            &view,
            vec![
                test_item(1, 1920, 1080, vec![]),
                test_item(2, 1000, 1000, vec![]),
            ],
        );
        cx.update_entity(&view, |root, cx| {
            root.project.settings.layout = target;
            root.set_mode(Mode::Layout, cx);
            cx.notify();
        });
        draw(cx);
        let measured_here = cx.read_entity(&view, |root, _| {
            let layout = root.canvas_layout();
            assert_eq!(layout.entries.len(), 2, "{target:?} 下应有两张图");
            (layout.scale, layout.width, layout.height)
        });
        measured.push(measured_here);
    }
    let (row_scale, row_w, row_h) = measured[0];
    let (column_scale, column_w, column_h) = measured[1];
    // 先钉住「这个例子真的能区分新旧做法」：两种排版画布的外框差得足够远，
    // 老做法按画布短边算比例，这里必然算出一大一小。
    assert!(
        (row_w - column_w).abs() > 200.0 && (row_h - column_h).abs() > 200.0,
        "这组用例的横排画布 {row_w}×{row_h} 和竖排画布 {column_w}×{column_h} 应差得足够明显"
    );
    assert!(
        (row_scale - column_scale).abs() < 1e-4,
        "横排和竖排的显示比例应完全一样：横排 {row_scale}，竖排 {column_scale}"
    );
}

/// 导入过程中「适合画布」的比例不能变。
///
/// 图片解码完、字幕还没识别完的那段时间，图还画不出来（裁剪范围没定）。这段时间
/// 比例必须已经按图片尺寸定下来：否则先识别完的那张会按它自己的尺寸定比例，用户
/// 看到的就是先大后小地跳一下。
#[test]
fn fit_scale_is_stable_while_detection_runs() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    cx.simulate_resize(gpui::size(gpui::px(1381.), gpui::px(804.)));
    let mut items = vec![
        test_item(1, 800, 600, vec![line(500.0, 560.0)]),
        test_item(2, 3840, 2160, vec![line(1900.0, 1990.0)]),
        test_item(3, 1920, 1080, vec![line(900.0, 1000.0)]),
    ];
    for item in &mut items {
        item.status = ItemStatus::Pending;
        item.analysis = crate::model::AnalysisState::PendingAnalysis;
        item.keep_whole = false;
    }
    install_items(cx, &view, items);
    cx.update_entity(&view, |root, cx| {
        root.set_mode(Mode::Subtitle, cx);
        cx.notify();
    });
    draw(cx);
    // 短边基准是三张里最大的那张 3840×2160 的短边 2160，给出的比例是
    // 804 × 0.618 ÷ 2160 ≈ 0.230；宽度基准是最宽的 3840，给出的比例是
    // 1381 × 0.618 ÷ 3840 ≈ 0.222。宽度这条更紧，取它。
    let expected = 1381.0 * tokens::GOLDEN_FIT_FRACTION / 3840.0;
    let scale = cx.read_entity(&view, |root, _| root.canvas_layout().scale);
    assert!(
        (scale - expected).abs() < 1e-4,
        "一张都还没识别完时比例就该是 {expected}，实际 {scale}"
    );
    // 逐张识别完成，比例一点都不能动。
    for id in 1..=3 {
        cx.update_entity(&view, |root, cx| {
            let item = root
                .project
                .items
                .iter_mut()
                .find(|item| item.id == id)
                .expect("应有这张图");
            item.status = ItemStatus::Ready;
            item.analysis = crate::model::AnalysisState::Analyzed;
            cx.notify();
        });
        draw(cx);
        let scale = cx.read_entity(&view, |root, _| root.canvas_layout().scale);
        assert!(
            (scale - expected).abs() < 1e-4,
            "第 {id} 张识别完成后比例变了：{scale}，应为 {expected}"
        );
    }
}

/// 顺序导入尺寸差别很大的图片时，「适合画布」的比例从第一张起就是最终值。
///
/// 这是用户反馈的第二个现象：把整个文件夹一起导入，画面先呈现放大后的图，然后
/// 突然变成「适合画布」的缩放。原因是最开始只按「已经解码进项目的图」算基准：
/// 先到的 3840×540 把基准定成 540，比例 0.920；等 3840×2160 到齐，基准跳到
/// 2160，比例变成 0.230，整幅作品就当着用户的面缩了一下。
///
/// 现在入队时就读文件头，基准从第一帧起按整批图算。这里用真实文件走
/// `open_paths`，在第一张解码完、后面还没解码完的那一刻读比例。
#[test]
fn importing_files_one_by_one_never_jumps_the_fit_scale() {
    let dir = std::env::temp_dir().join(format!("gistpic-import-order-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("建临时目录失败");
    // 先宽后高：宽图先解码完，正是会触发「先放大再缩小」的顺序。
    let wide = dir.join("1-宽图.png");
    RgbImage::from_pixel(3840, 540, image::Rgb([20, 20, 24]))
        .save(&wide)
        .expect("写测试图片失败");
    let tall = dir.join("2-高图.png");
    RgbImage::from_pixel(3840, 2160, image::Rgb([30, 30, 34]))
        .save(&tall)
        .expect("写测试图片失败");

    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    cx.simulate_resize(gpui::size(gpui::px(1381.), gpui::px(804.)));
    // 两张一起交给导入入口，但解码 worker 是一张一张处理的。
    cx.update_entity(&view, |root, cx| {
        root.open_paths(vec![wide.clone(), tall.clone()], cx);
    });
    // 刚入队、一张都还没解码完：这里不能调 draw，draw 里的 run_until_parked 会把
    // 后台解码一口气跑完，就看不到中间状态了。直接读比例。
    // 短边基准 2160 给 804 × 0.618 ÷ 2160 ≈ 0.230，宽度基准 3840 给
    // 1381 × 0.618 ÷ 3840 ≈ 0.222。宽度这条更紧，整批的最终值就是 0.222。
    let expected = 1381.0 * tokens::GOLDEN_FIT_FRACTION / 3840.0;
    let first = cx.read_entity(&view, |root, _| {
        (root.project.items.len(), root.canvas_layout().scale)
    });
    assert!(
        (first.1 - expected).abs() < 1e-4,
        "入队那一刻比例就该是整批的最终值 {expected}，实际 {}（已有 {} 张）",
        first.1,
        first.0
    );

    // 一张一张解码完，每次读数都必须还是同一个值。
    let mut seen = Vec::new();
    for _ in 0..12 {
        cx.run_until_parked();
        draw(cx);
        let (count, scale) = cx.read_entity(&view, |root, _| {
            (root.project.items.len(), root.canvas_layout().scale)
        });
        if seen.last().map(|(c, _)| *c) != Some(count) {
            seen.push((count, scale));
        }
        assert!(
            (scale - expected).abs() < 1e-4,
            "导入过程中比例跳了：{count} 张时是 {scale}，应为 {expected}"
        );
        if count == 2 {
            break;
        }
    }
    assert_eq!(
        seen.last().map(|(count, _)| *count),
        Some(2),
        "两张图都应导入完成"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// 字幕模式切排版模式时，整张图的图块早就备好了，不用现成补。
///
/// 用户报的现象：4 张图在字幕模式下切到排版模式，先看到字幕模式那样拉伸的画面，
/// 过一会才变成完整的图。原因是字幕模式只给屏幕上露出来的那一截（裁剪带）切块，
/// 整张图其余部分没有位图；切到排版模式要显示整张图，只能先拿 256 宽的缩略图
/// 顶着，看着就是先糊一下。现在按整张原图切块，切模式不会多出一块要补的。
#[test]
fn switching_from_subtitle_to_layout_reuses_tiles() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    cx.simulate_resize(gpui::size(gpui::px(1381.), gpui::px(804.)));
    install_items(
        cx,
        &view,
        vec![
            test_item(1, 3840, 2160, vec![line(1900.0, 1990.0)]),
            test_item(2, 3840, 540, vec![line(400.0, 470.0)]),
            test_item(3, 3840, 540, vec![line(400.0, 470.0)]),
            test_item(4, 3840, 540, vec![line(400.0, 470.0)]),
        ],
    );
    cx.update_entity(&view, |root, _c| {
        root.project.settings.layout = crate::model::LayoutTarget::Column;
    });
    cx.update_entity(&view, |root, cx| {
        root.set_mode(Mode::Subtitle, cx);
        cx.notify();
    });
    assert!(
        wait_until(cx, |cx| !tile_keys(cx, &view).is_empty()),
        "字幕模式下应已生成整张图的图块"
    );
    // 按图记下代次：排版模式下一屏放不下三张，滚出屏幕的会被清缓存，
    // 所以只比较切完之后还在缓存里的那些图。
    let generations = |cx: &mut gpui::VisualTestContext| {
        cx.read_entity(&view, |root, _| {
            root
                .tile_cache
                .iter()
                .map(|(id, grid)| (*id, grid.generation()))
                .collect::<std::collections::HashMap<ItemId, u64>>()
        })
    };
    let before = generations(cx);
    assert!(!before.is_empty(), "字幕模式下每张图都该有图块缓存");

    cx.update_entity(&view, |root, cx| {
        root.set_mode(Mode::Layout, cx);
        cx.notify();
    });
    draw(cx);
    let after = generations(cx);
    assert!(!after.is_empty(), "排版模式下应有图块缓存");
    for (id, generation) in &after {
        if let Some(was) = before.get(id) {
            assert_eq!(
                generation, was,
                "第 {id} 张在切到排版模式时换了图块代次（换代要重新解码缩放，画面会先糊一下）"
            );
        }
    }
    // 切过去之后不该有「还要现成补」的块：缺块时只能拿缩略图顶着，那就是用户
    // 看到的先糊一下。
    let missing: Vec<(ItemId, usize)> = cx.read_entity(&view, |root, _| {
        let mut rows: Vec<(ItemId, usize)> = root
            .tile_cache
            .iter()
            .map(|(id, grid)| (*id, grid.missing().len()))
            .filter(|(_, count)| *count > 0)
            .collect();
        rows.sort();
        rows
    });
    assert!(
        missing.is_empty(),
        "切到排版模式后还有图块要现成补：{missing:?}；缺块时会先显示缩略图，看着就是糊一下"
    );
}

/// 单张宽图的比例由短边和宽度两条上限里更紧的那条决定。
///
/// 3840×540 这种字幕条截图在 1381×804 的窗口里：
///
/// - 短边上限：804 × 0.618 ÷ 540 ≈ 0.9202，图会显示成 3533 宽；
/// - 宽度上限：1381 × 0.618 ÷ 3840 ≈ 0.2223，图显示成 853 宽。
///
/// 宽度上限更紧，所以取 0.2223，图显示成 853 宽、120 高，左右各留约 264。
/// 用户选的正是这个结果：单独导入这张图，显示的缩放就是它。
///
/// 曾经只按短边算，单张宽图显示成 3533 宽，比 1381 的窗口宽两倍半，用户反馈
/// 「适合画布看着过大」。
#[test]
fn single_wide_image_fits_by_its_short_side() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    cx.simulate_resize(gpui::size(gpui::px(1381.), gpui::px(804.)));
    install_items(cx, &view, vec![test_item(1, 3840, 540, vec![])]);
    cx.update_entity(&view, |root, cx| {
        root.set_mode(Mode::Subtitle, cx);
        cx.notify();
    });
    draw(cx);
    let (scale, width, height) = cx.read_entity(&view, |root, _| {
        let layout = root.canvas_layout();
        (layout.scale, layout.width, layout.height)
    });
    // 短边上限给 804 × 0.618 / 540 ≈ 0.920（宽 3533），
    // 宽度上限给 1381 × 0.618 / 3840 ≈ 0.222（宽 853），取小的那个。
    let expected = 1381.0 * tokens::GOLDEN_FIT_FRACTION / 3840.0;
    assert!(
        (scale - expected).abs() < 1e-4,
        "比例应是窗口宽度的黄金分割除以图的宽度，即 {expected}，实际 {scale}"
    );
    assert!(
        (width - 3840.0 * expected).abs() < 0.5,
        "宽度按比例跟着缩，应为 {}，实际 {width}",
        3840.0 * expected
    );
    assert!(
        (height - 540.0 * expected).abs() < 0.5,
        "高度按比例跟着缩，应为 {}，实际 {height}",
        540.0 * expected
    );
    // 左右各留一半余量，图不会贴着窗口边。
    let margin = (1381.0 - width) / 2.0;
    assert!(
        margin > 200.0,
        "单张宽图两边应留出明显余量，实际每边 {margin}"
    );
}

/// 字幕模式和排版模式点「适合画布」之后，图必须显示成一样大。
///
/// 两种模式共用同一条基准：每张**原图**短边里最大的那个。排版模式按整张图摆，
/// 字幕模式按裁剪后的字幕带摆，但比例来自同一个基准，所以同一批图在两种模式下
/// 显示宽度相同。
///
/// 之前字幕模式拿裁剪后字幕带的短边当基准：字幕带又宽又矮（3840×2160 的截图，
/// 字幕带只有一两百像素高），短边太小，算出来的比例接近 100%，整条宽出窗口好几倍。
#[test]
fn subtitle_fit_matches_layout_mode() {
    let mut measured = Vec::new();
    for mode in [Mode::Subtitle, Mode::Layout] {
        let mut cx = TestAppContext::single();
        let (view, cx) = cx.add_window_view(AppRoot::new);
        cx.simulate_resize(gpui::size(gpui::px(1381.), gpui::px(804.)));
        install_items(
            cx,
            &view,
            vec![
                test_item(1, 3840, 2160, vec![line(1900.0, 1990.0)]),
                test_item(2, 3840, 2160, vec![line(1900.0, 1990.0)]),
                test_item(3, 3840, 2160, vec![line(1900.0, 1990.0)]),
            ],
        );
        cx.update_entity(&view, |root, cx| {
            root.project.settings.layout = crate::model::LayoutTarget::Column;
            root.set_mode(mode, cx);
            cx.notify();
        });
        draw(cx);
        draw(cx);
        measured.push(cx.read_entity(&view, |root, _| {
            let layout = root.canvas_layout();
            (layout.scale, layout.entries[0].display_width)
        }));
    }
    let (subtitle_scale, subtitle_width) = measured[0];
    let (layout_scale, layout_width) = measured[1];
    // 短边基准是原图短边 2160（804 × 0.618 ÷ 2160 ≈ 0.230），宽度基准是原图宽
    // 3840（1381 × 0.618 ÷ 3840 ≈ 0.222）。宽度这条更紧，取它。
    let expected = 1381.0 * tokens::GOLDEN_FIT_FRACTION / 3840.0;
    assert!(
        (subtitle_scale - expected).abs() < 1e-4,
        "字幕模式的比例应是 {expected}，实际 {subtitle_scale}"
    );
    assert!(
        (subtitle_scale - layout_scale).abs() < 1e-4,
        "两种模式的显示比例应完全一样：字幕 {subtitle_scale}，排版 {layout_scale}"
    );
    assert!(
        (subtitle_width - layout_width).abs() < 0.5,
        "每张图在两种模式下应显示成一样宽：字幕 {subtitle_width}，排版 {layout_width}"
    );
}

/// 需求 7：长宽比相同的两张图，横排竖排显示出来的大小完全一样。
///
/// 这是用户直观看得见的那一半：`arrange_row` 拉高、`arrange_column` 拉宽，长宽比
/// 相同的图在两种排版下摆出来的框子本来就一样大，所以「横排竖排显示不一样大」
/// 只可能来自比例，比例对了这里就必须逐一相等。
#[test]
fn row_and_column_show_equal_aspect_items_at_the_same_size() {
    let mut measured: Vec<Vec<(ItemId, f32, f32)>> = Vec::new();
    for target in [
        crate::model::LayoutTarget::Row,
        crate::model::LayoutTarget::Column,
    ] {
        let mut cx = TestAppContext::single();
        let (view, cx) = cx.add_window_view(AppRoot::new);
        cx.simulate_resize(gpui::size(gpui::px(900.), gpui::px(700.)));
        install_items(
            cx,
            &view,
            vec![
                test_item(1, 1000, 1000, vec![]),
                test_item(2, 1000, 1000, vec![]),
            ],
        );
        cx.update_entity(&view, |root, cx| {
            root.project.settings.layout = target;
            root.set_mode(Mode::Layout, cx);
            cx.notify();
        });
        draw(cx);
        let mut sizes = cx.read_entity(&view, |root, _| {
            root.canvas_layout()
                .entries
                .iter()
                .map(|entry| (entry.id, entry.display_width, entry.display_height))
                .collect::<Vec<_>>()
        });
        sizes.sort_by_key(|entry| entry.0);
        assert_eq!(sizes.len(), 2, "{target:?} 下应有两张图");
        measured.push(sizes);
    }
    for (index, (row, column)) in measured[0].iter().zip(measured[1].iter()).enumerate() {
        assert_eq!(row.0, column.0, "第 {} 张的图号应一致", index + 1);
        assert!(
            (row.1 - column.1).abs() < 0.5 && (row.2 - column.2).abs() < 0.5,
            "图 {} 在横排竖排下应一样大：横排 {:.1}×{:.1}，竖排 {:.1}×{:.1}",
            row.0,
            row.1,
            row.2,
            column.1,
            column.2
        );
    }
}

/// 用户给的三个具体答案：整幅作品都按「整张图最短边」这一个标准来，
/// 单张和多张完全同一套算法，不再有「单张另算」的例外。
///
/// 三组数都是 1381×804 窗口下算出来的，取短边上限和宽度上限里更紧的那条：
///
/// - 单张 3840×540：短边上限 804 × 0.618 ÷ 540 ≈ 0.920，宽度上限
///   1381 × 0.618 ÷ 3840 ≈ 0.222，取 0.222；
/// - 三张 3840×2160 竖排：短边上限 ≈ 0.230，宽度上限 ≈ 0.222，取 0.222；
/// - 两张 1920×1080 横排：短边上限 ≈ 0.460，宽度上限 ≈ 0.445，取 0.445。
#[test]
fn every_layout_fits_by_the_shortest_side_of_the_whole_artwork() {
    // 宽度上限给出的比例：窗口宽 1381 的黄金分割除以这批图里最宽的宽度。
    fn by_width(widest: f32) -> f32 {
        1381.0 * tokens::GOLDEN_FIT_FRACTION / widest
    }
    // (说明, 每张图的宽高, 排版, 期望比例, 期望画布宽, 期望画布高)
    type Case = (&'static str, Vec<(u32, u32)>, crate::model::LayoutTarget, f32, f32, f32);
    let cases: Vec<Case> = vec![
        // (说明, 图片尺寸, 排版, 期望比例, 期望画布宽, 期望画布高)
        (
            "单张 3840×540",
            vec![(3840, 540)],
            crate::model::LayoutTarget::Column,
            by_width(3840.0),
            3840.0 * by_width(3840.0),
            540.0 * by_width(3840.0),
        ),
        (
            "三张 3840×2160 竖排",
            vec![(3840, 2160), (3840, 2160), (3840, 2160)],
            crate::model::LayoutTarget::Column,
            by_width(3840.0),
            3840.0 * by_width(3840.0),
            3.0 * 2160.0 * by_width(3840.0),
        ),
        (
            "两张 1920×1080 横排",
            vec![(1920, 1080), (1920, 1080)],
            crate::model::LayoutTarget::Row,
            by_width(1920.0),
            2.0 * 1920.0 * by_width(1920.0),
            1080.0 * by_width(1920.0),
        ),
    ];
    for (name, sizes, layout, want_scale, want_width, want_height) in cases {
        let mut cx = TestAppContext::single();
        let (view, cx) = cx.add_window_view(AppRoot::new);
        cx.simulate_resize(gpui::size(gpui::px(1381.), gpui::px(804.)));
        let items = sizes
            .iter()
            .enumerate()
            .map(|(index, (width, height))| {
                test_item(index as u64 + 1, *width, *height, vec![])
            })
            .collect();
        install_items(cx, &view, items);
        cx.update_entity(&view, |root, cx| {
            root.project.settings.layout = layout;
            root.set_mode(Mode::Layout, cx);
            cx.notify();
        });
        draw(cx);
        let (scale, width, height) = cx.read_entity(&view, |root, _| {
            let layout = root.canvas_layout();
            (layout.scale, layout.width, layout.height)
        });
        assert!(
            (scale - want_scale).abs() < 1e-4,
            "{name} 的比例应是 {want_scale}，实际 {scale}"
        );
        assert!(
            (width - want_width).abs() < 0.5,
            "{name} 的画布宽应是 {want_width}，实际 {width}"
        );
        assert!(
            (height - want_height).abs() < 0.5,
            "{name} 的画布高应是 {want_height}，实际 {height}"
        );
    }
}

/// 同一张 1920×1080，单张和「两张横排里的每张」必须显示成一样大。
///
/// 这是用户直接提出的疑问：把整个文件夹一起导入时显示的缩放正合适，单独导入
/// 其中一张就明显偏大。原因是最开始只按短边压，单张 1920×1080 显示成
/// 1920 × 0.460 = 883 宽，比窗口窄不了多少；宽度这道上限补上之后，单张和
/// 多张都落在 1381 × 0.618 ≈ 853 宽，两条路径看到的大小一样。
#[test]
fn one_image_and_the_same_image_among_many_are_shown_at_the_same_size() {
    let mut measured = Vec::new();
    for count in [1usize, 2] {
        let mut cx = TestAppContext::single();
        let (view, cx) = cx.add_window_view(AppRoot::new);
        cx.simulate_resize(gpui::size(gpui::px(1381.), gpui::px(804.)));
        let items = (0..count)
            .map(|index| test_item(index as u64 + 1, 1920, 1080, vec![]))
            .collect();
        install_items(cx, &view, items);
        cx.update_entity(&view, |root, cx| {
            root.project.settings.layout = crate::model::LayoutTarget::Row;
            root.set_mode(Mode::Layout, cx);
            cx.notify();
        });
        draw(cx);
        measured.push(cx.read_entity(&view, |root, _| {
            let layout = root.canvas_layout();
            (layout.scale, layout.entries[0].display_width)
        }));
    }
    let (single_scale, single_width) = measured[0];
    let (multi_scale, multi_width) = measured[1];
    // 短边上限 804 × 0.618 ÷ 1080 ≈ 0.460，宽度上限 1381 × 0.618 ÷ 1920 ≈
    // 0.445。两种数量下都是宽度这条更紧，所以都是 0.445。
    let expected = 1381.0 * tokens::GOLDEN_FIT_FRACTION / 1920.0;
    for (label, scale, width) in [
        ("单张", single_scale, single_width),
        ("两张之一", multi_scale, multi_width),
    ] {
        assert!(
            (scale - expected).abs() < 1e-4,
            "{label} 的比例应是 {expected}，实际 {scale}"
        );
        assert!(
            (width - 1920.0 * expected).abs() < 0.5,
            "{label} 的显示宽应是 {}，实际 {width}",
            1920.0 * expected
        );
    }
    assert!(
        (single_width - multi_width).abs() < 0.5,
        "单张和两张之一应一样宽：单张 {single_width}，两张之一 {multi_width}"
    );
}

/// 点「适合画布」要把之前按过加减键留下的相对倍数一起清掉。
///
/// 只切模式不清倍数的话，画面保留原来的放大倍数，用户点了「适合画布」看到
/// 的还是偏大，像是这一下没生效。
#[test]
fn zoom_fit_clears_the_relative_zoom_left_by_the_step_keys() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    cx.simulate_resize(gpui::size(gpui::px(1381.), gpui::px(804.)));
    install_items(cx, &view, vec![test_item(1, 1920, 1080, vec![])]);
    draw(cx);
    let fit_scale = cx.read_entity(&view, |root, _| root.canvas_layout().scale);
    // 按两次加号：键盘和滚轮同一套算法，每次把当前显示比例乘 1.1。
    cx.simulate_keystrokes(&crate::platform::shortcut("+"));
    cx.run_until_parked();
    cx.simulate_keystrokes(&crate::platform::shortcut("+"));
    cx.run_until_parked();
    let enlarged = cx.read_entity(&view, |root, _| root.canvas_layout().scale);
    let doubled_step = fit_scale * (1.0 + tokens::ZOOM_STEP) * (1.0 + tokens::ZOOM_STEP);
    assert!(
        (enlarged - doubled_step).abs() < 1e-4,
        "按两次加号应放大到 {doubled_step}，实际 {enlarged}"
    );
    draw(cx);
    let fit = cx
        .debug_bounds("zoom-item-zoom-fit")
        .expect("应有适合画布项");
    cx.simulate_click(fit.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    let (is_fit, scale) =
        cx.read_entity(&view, |root, _| (root.view_scale().is_fit(), root.canvas_layout().scale));
    assert!(is_fit, "点「适合画布」应回到自动适应");
    // 1920×1080：宽度上限 1381 × 0.618 ÷ 1920 ≈ 0.445 比短边上限更紧。
    let expected = 1381.0 * tokens::GOLDEN_FIT_FRACTION / 1920.0;
    assert!(
        (scale - expected).abs() < 1e-4,
        "比例应回到纯适配值 {expected}，实际 {scale}"
    );
}

/// 键盘加减键和滚轮走同一套口径：都乘当前显示比例，范围都是 5%~400%。
///
/// 以前键盘改的是自动适应下的相对倍数，被夹在 0.5~2.0，和滚轮的 5%~400% 不一致；
/// 同一张图用键盘和用滚轮能到的最大倍数不一样，用户看不出规律。
#[test]
fn keyboard_zoom_and_wheel_zoom_share_one_range() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    cx.simulate_resize(gpui::size(gpui::px(500.), gpui::px(400.)));
    install_items(cx, &view, vec![test_item(1, 700, 525, vec![])]);
    draw(cx);
    let fit_scale = cx.read_entity(&view, |root, _| root.canvas_layout().scale);
    cx.simulate_keystrokes(&crate::platform::shortcut("+"));
    cx.run_until_parked();
    let after_one_plus = cx.read_entity(&view, |root, _| root.canvas_layout().scale);
    let wanted = (fit_scale * (1.0 + tokens::ZOOM_STEP)).min(tokens::MANUAL_SCALE_MAX);
    assert!(
        (after_one_plus - wanted).abs() < 1e-4,
        "按一次加号应等于当前比例乘 {}，期望 {wanted}，实际 {after_one_plus}",
        1.0 + tokens::ZOOM_STEP
    );
    let manual = cx.read_entity(&view, |root, _| root.view_scale().mode.manual());
    assert_eq!(
        manual,
        Some(wanted),
        "按加号应切进手动比例，并且值就是乘完的结果"
    );
    // 一路按到上限，确认能超过自动适应原来的 2.0 倍相对倍数上限。
    for _ in 0..60 {
        let previous = cx.read_entity(&view, |root, _| root.canvas_layout().scale);
        cx.simulate_keystrokes(&crate::platform::shortcut("+"));
        cx.run_until_parked();
        let now = cx.read_entity(&view, |root, _| root.canvas_layout().scale);
        if (now - previous).abs() < 1e-6 {
            break;
        }
    }
    let top = cx.read_entity(&view, |root, _| root.canvas_layout().scale);
    assert!(
        (top - tokens::MANUAL_SCALE_MAX).abs() < 1e-4,
        "键盘一直按加号应能到 {}，实际 {top}",
        tokens::MANUAL_SCALE_MAX
    );
}
/// 右键卡片里的「把当前裁剪同步到全部图片」：所有图都变成被右键那张的裁剪段。
///
/// 这条路径此前完全没有测试。这里同时盯着三件事：每张图都换成了第一张的裁剪段、
/// 输出尺寸跟着变、这一下能撤销回去。
#[test]
fn context_menu_sync_crop_to_all_copies_one_band_to_every_image() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    cx.simulate_resize(gpui::size(gpui::px(1000.), gpui::px(900.)));
    install_items(
        cx,
        &view,
        vec![
            test_item(1, 1000, 1000, vec![]),
            test_item(2, 1000, 2000, vec![]),
            test_item(3, 2000, 1000, vec![]),
        ],
    );
    // 第 1 张先手动裁一段，其余两张不裁；这样「同步」前后的差别是确定的。
    let band = crate::model::ManualBand::clamped(0.1, 0.9, 0.2, 0.8).expect("有效保留带");
    cx.update_entity(&view, |root, cx| {
        root.project.items[0].manual = Some(band);
        // 只选第 1 张，避免「多选一起改」那条分支把结果搅浑。
        root.selected_ids = [1].into_iter().collect();
        root.selected = Some(1);
        cx.notify();
    });
    draw(cx);
    let output_before = cx.read_entity(&view, |root, _| root.output_size());

    // 右键第 1 张，打开它的卡片。
    let block = cx.debug_bounds("canvas-block-1").expect("应有第一张图");
    cx.simulate_mouse_down(
        block.center(),
        gpui::MouseButton::Right,
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    draw(cx);
    assert!(cx.debug_bounds("context-menu").is_some(), "右键应出现卡片");
    let row = cx
        .debug_bounds("img-apply-crop")
        .expect("图片卡片应有「同步裁剪到全部」这一行");
    cx.simulate_click(row.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    draw(cx);

    // 每张图都变成第 1 张那一段，一张都不能漏。
    let manuals = cx.read_entity(&view, |root, _| {
        root.project
            .items
            .iter()
            .map(|item| (item.id, item.manual))
            .collect::<Vec<_>>()
    });
    for (id, manual) in &manuals {
        assert_eq!(
            *manual,
            Some(band),
            "第 {id} 张应同步成第一张的裁剪段，实际 {manual:?}"
        );
    }
    // 所有图都改成同一段，输出尺寸应当跟着变，不是原样不动。
    let output_after = cx.read_entity(&view, |root, _| root.output_size());
    assert_ne!(output_after, output_before, "同步裁剪后输出尺寸应变化");

    // 这一下要能撤销回去。
    cx.update_entity(&view, |root, cx| root.undo_edit(cx));
    cx.run_until_parked();
    let after_undo = cx.read_entity(&view, |root, _| {
        root.project
            .items
            .iter()
            .map(|item| (item.id, item.manual))
            .collect::<Vec<_>>()
    });
    assert_eq!(
        after_undo,
        vec![(1, Some(band)), (2, None), (3, None)],
        "撤销后应回到只有第一张被裁的状态"
    );
}

/// 没有手动裁剪时，「同步裁剪到全部」这一行是灰的，按了也不该有任何变化。
#[test]
fn sync_crop_to_all_row_is_disabled_without_a_manual_crop() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    cx.simulate_resize(gpui::size(gpui::px(1000.), gpui::px(900.)));
    install_items(
        cx,
        &view,
        vec![test_item(1, 1000, 1000, vec![]), test_item(2, 1000, 2000, vec![])],
    );
    cx.update_entity(&view, |root, cx| {
        root.selected_ids = [1].into_iter().collect();
        root.selected = Some(1);
        cx.notify();
    });
    draw(cx);
    let block = cx.debug_bounds("canvas-block-1").expect("应有第一张图");
    cx.simulate_mouse_down(
        block.center(),
        gpui::MouseButton::Right,
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    draw(cx);
    let row = cx
        .debug_bounds("img-apply-crop")
        .expect("图片卡片应有「同步裁剪到全部」这一行");
    cx.simulate_click(row.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    let manuals = cx.read_entity(&view, |root, _| {
        root.project
            .items
            .iter()
            .map(|item| item.manual)
            .collect::<Vec<_>>()
    });
    assert_eq!(
        manuals,
        vec![None, None],
        "没有手动裁剪时按这一行不该产生任何裁剪"
    );
}

/// 这次反馈的规则：横排所有图一样高，竖排所有图一样宽。
///
/// 上一版按「同一张图在横排竖排下尺寸完全相同」做，这次用户改成按最高/最宽那张
/// 统一；两张图长宽比不同时，同一张图在两种排法下会被缩放到不同大小。
#[test]
fn row_and_column_unify_the_facing_side() {
    let mut measured: Vec<Vec<(ItemId, f32, f32)>> = Vec::new();
    for target in [
        crate::model::LayoutTarget::Row,
        crate::model::LayoutTarget::Column,
    ] {
        // 每种排版用一个全新窗口，唯一变量只有排版方向。
        let mut cx = TestAppContext::single();
        let (view, cx) = cx.add_window_view(AppRoot::new);
        cx.simulate_resize(gpui::size(gpui::px(900.), gpui::px(700.)));
        install_items(
            cx,
            &view,
            vec![
                test_item(1, 1920, 1080, vec![]),
                test_item(2, 1000, 1000, vec![]),
            ],
        );
        cx.update_entity(&view, |root, cx| {
            root.project.settings.layout = target;
            root.set_mode(Mode::Layout, cx);
            cx.notify();
        });
        draw(cx);
        let mut sizes = cx.read_entity(&view, |root, _| {
            root.canvas_layout()
                .entries
                .iter()
                .map(|entry| (entry.id, entry.display_width, entry.display_height))
                .collect::<Vec<_>>()
        });
        sizes.sort_by_key(|entry| entry.0);
        assert_eq!(sizes.len(), 2, "{target:?} 下应有两张图");
        measured.push(sizes);
    }
    // 横排两张显示高度一样。
    for (index, row) in measured[0].iter().enumerate() {
        assert!(
            (row.2 - measured[0][0].2).abs() < 0.5,
            "横排第 {} 张应和其它图一样高：{:.1} 和 {:.1}",
            index + 1,
            row.2,
            measured[0][0].2
        );
    }
    // 竖排两张显示宽度一样。
    for (index, column) in measured[1].iter().enumerate() {
        assert!(
            (column.1 - measured[1][0].1).abs() < 0.5,
            "竖排第 {} 张应和其它图一样宽：{:.1} 和 {:.1}",
            index + 1,
            column.1,
            measured[1][0].1
        );
    }
    // 同一张方图在两种排法下大小不同：横排 1080×1080、竖排 1920×1920，再乘窗口缩放。
    let square_row = measured[0][1];
    let square_column = measured[1][1];
    assert_eq!(square_row.0, square_column.0, "两张的图号应一致");
    assert!(
        (square_row.1 - square_column.1).abs() > 100.0,
        "方图在横排竖排下应大小不同：横排 {:.1}×{:.1}，竖排 {:.1}×{:.1}",
        square_row.1,
        square_row.2,
        square_column.1,
        square_column.2
    );
}

/// 需求 7：新基准下小图仍然不放大。
///
/// 基准从「整幅排版画布」换成「每张图可见带的最短边」之后，400×300 这种小图
/// 依旧低于窗口短边的黄金分割，按实际像素显示，不能因为换了基准就被撑大。
#[test]
fn small_image_is_not_enlarged_under_new_basis() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    cx.simulate_resize(gpui::size(gpui::px(1400.), gpui::px(1400.)));
    install_items(
        cx,
        &view,
        vec![test_item(1, 400, 300, vec![]), test_item(2, 400, 300, vec![])],
    );
    for target in [
        crate::model::LayoutTarget::Row,
        crate::model::LayoutTarget::Column,
    ] {
        cx.update_entity(&view, |root, cx| {
            root.project.settings.layout = target;
            root.set_mode(Mode::Layout, cx);
            cx.notify();
        });
        draw(cx);
        let (scale, sizes) = cx.read_entity(&view, |root, _| {
            let layout = root.canvas_layout();
            let sizes = layout
                .entries
                .iter()
                .map(|entry| (entry.display_width, entry.display_height))
                .collect::<Vec<_>>();
            (layout.scale, sizes)
        });
        assert!(
            (scale - 1.0).abs() < 1e-6,
            "{target:?} 下 400×300 的小图不该放大，实际比例 {scale}"
        );
        assert!(
            (sizes[0].0 - 400.0).abs() < 1.0 && (sizes[0].1 - 300.0).abs() < 1.0,
            "{target:?} 下小图应显示成 400×300，实际 {:?}",
            sizes[0]
        );
        assert!(
            (sizes[1].0 - 400.0).abs() < 1.0 && (sizes[1].1 - 300.0).abs() < 1.0,
            "{target:?} 下第二张小图也应显示成 400×300，实际 {:?}",
            sizes[1]
        );
    }
}


// ----------------------------------------------------------------------
// 内存释放：图块只留可见的、清空释放全部、清空前在途的任务作废。
// ----------------------------------------------------------------------

fn encode_test_png(width: u32, height: u32, rgb: [u8; 3]) -> Vec<u8> {
    let mut bytes = Vec::new();
    image::DynamicImage::ImageRgb8(RgbImage::from_pixel(width, height, image::Rgb(rgb)))
        .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
        .expect("生成测试 PNG 失败");
    bytes
}

/// 清空作品要把图块、缩略图、待生成任务全部放掉，同时保留撤销快照。
#[test]
fn clear_all_releases_tiles_and_thumbnails() {
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
    assert!(
        wait_until(cx, |cx| {
            cx.read_entity(&view, |root, _| {
                root.tile_cache
                    .values()
                    .any(|grid| grid.counts().0 > 0)
            })
        }),
        "画完应有已经生成的图块"
    );
    assert!(
        wait_until(cx, |cx| cx.read_entity(&view, |root, _| !root.thumb_cache.is_empty())),
        "画完应有缩略图"
    );
    cx.dispatch_action(super::actions::ClearAll);
    cx.run_until_parked();
    let (items, tiles, thumbs, pending, jobs, undo) =
        cx.read_entity(&view, |root, _| {
            (
                root.project.items.len(),
                root.tile_cache.len(),
                root.thumb_cache.len(),
                root.thumb_pending.len(),
                root.tile_jobs.len(),
                root.undo.can_undo(),
            )
        });
    assert_eq!(items, 0, "清空后项目应为空");
    assert_eq!(tiles, 0, "清空后图块缓存应清空");
    assert_eq!(thumbs, 0, "清空后缩略图缓存应清空");
    assert_eq!(pending, 0, "清空后不该还有缩略图任务");
    assert_eq!(jobs, 0, "清空后不该还有图块任务");
    assert!(undo, "清空要能撤销");
    draw(cx);
    assert!(
        wait_until(cx, |cx| cx.read_entity(&view, |root, _| root.images_to_drop.is_empty())),
        "交出去的位图应在下一帧渲染时排空"
    );
    cx.dispatch_action(super::actions::UndoEdit);
    cx.run_until_parked();
    assert_eq!(
        cx.read_entity(&view, |root, _| root.project.items.len()),
        2,
        "撤销清空后图片应回来"
    );
}

/// 清空后第一次导入要清掉清空前的旧撤销历史，撤销一次只回到空作品。
#[test]
fn first_import_after_clear_starts_fresh_history() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(cx, &view, vec![test_item(1, 800, 600, vec![line(400.0, 460.0)])]);
    draw(cx);
    cx.dispatch_action(super::actions::ClearAll);
    cx.run_until_parked();
    let png = encode_test_png(8, 8, [200, 10, 10]);
    cx.update_entity(&view, |root, cx| {
        root.import_bytes("新图.png".to_string(), png, cx);
    });
    assert!(
        wait_until(cx, |cx| cx.read_entity(&view, |root, _| root.project.items.len() == 1)),
        "新图应导入项目"
    );
    cx.dispatch_action(super::actions::UndoEdit);
    cx.run_until_parked();
    assert_eq!(
        cx.read_entity(&view, |root, _| root.project.items.len()),
        0,
        "撤销一次应回到空作品，不能把清空前的图撤回来"
    );
}

/// 屏幕外的图不保留图块缓存；交出来的位图下一帧统一释放。
#[test]
fn offscreen_items_do_not_keep_tiles() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    let items: Vec<Item> = (1..=8)
        .map(|id| test_item(id, 1600, 1200, vec![line(900.0, 1000.0)]))
        .collect();
    install_items(cx, &view, items);
    // 字幕模式的「适合画布」会把整条压进窗口，作品不再溢出，八张图全都露在屏幕上。
    // 这里用 60% 把作品撑出可视区，才能测到「屏幕外的图不留图块缓存」。
    cx.update_entity(&view, |root, cx| {
        root.set_manual_scale(0.6, cx);
        cx.notify();
    });
    draw(cx);
    assert!(
        wait_until(cx, |cx| {
            cx.read_entity(&view, |root, _| {
                root.tile_cache
                    .values()
                    .any(|grid| grid.counts().0 > 0)
            })
        }),
        "可见图块应生成"
    );
    let (cached, visible) = cx.read_entity(&view, |root, _| {
        let cached: Vec<ItemId> = root.tile_cache.keys().copied().collect();
        let layout = root.canvas_layout();
        let visible: Vec<ItemId> = layout.entries.iter().map(|entry| entry.id).collect();
        (cached, visible)
    });
    assert!(
        cached.len() < visible.len(),
        "屏幕外不该保留图块缓存，实际缓存 {cached:?}"
    );
    cx.update_entity(&view, |root, _| root.release_offscreen_tiles(&[]));
    assert!(
        cx.read_entity(&view, |root, _| root.tile_cache.is_empty()),
        "要求释放后图块缓存应为空"
    );
    draw(cx);
    assert!(
        wait_until(cx, |cx| cx.read_entity(&view, |root, _| root.images_to_drop.is_empty())),
        "交出来的图块应在下一帧排空"
    );
}

/// 清空时还在后台解码的导入不能把图重新加回项目。
#[test]
fn import_in_flight_does_not_come_back_after_clear() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    let png = encode_test_png(1600, 1600, [30, 30, 40]);
    cx.update_entity(&view, |root, cx| {
        root.import_bytes("清空前导入.png".to_string(), png, cx);
    });
    cx.dispatch_action(super::actions::ClearAll);
    cx.run_until_parked();
    for _ in 0..10 {
        cx.run_until_parked();
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(20));
    }
    assert_eq!(
        cx.read_entity(&view, |root, _| root.project.items.len()),
        0,
        "清空前在途的解码不应把图加回来"
    );
    assert_eq!(cx.read_entity(&view, |root, _| root.decoding), 0, "解码计数应归零");
}

// ----------------------------------------------------------------------
// 选中视觉：抬起卡片、方勾、变淡遮罩、选中切换动画。
// ----------------------------------------------------------------------

/// 把选中动画推完：每次推进 20ms，共 320ms，足够盖住 tokens 里的总时长。
fn settle_selection(cx: &mut gpui::VisualTestContext) {
    for _ in 0..16 {
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(20));
        cx.run_until_parked();
    }
}

/// 装好图片后清掉 install_items 自动加的全选，再看指定图片的选中状态。
fn clear_selection_for_test(cx: &mut gpui::VisualTestContext, view: &gpui::Entity<AppRoot>) {
    cx.update_entity(view, |root, _| {
        root.selected = None;
        root.selected_ids.clear();
        root.selection_anim = None;
    });
}

fn roughly_equal(left: gpui::Pixels, right: f32) -> bool {
    (f32::from(left) - right).abs() < 0.75
}

/// 用户要求取消浮动，因为上移让裁剪边对不准。选中一张图后，卡片原点必须
/// 和图块原点完全一致，不能再有任何上移量。
#[test]
fn selected_card_does_not_move() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(cx, &view, vec![test_item(1, 1000, 800, vec![line(600.0, 680.0)])]);
    clear_selection_for_test(cx, &view);
    cx.update_entity(&view, |root, cx| root.select(1, cx));
    settle_selection(cx);
    draw(cx);

    let block = cx.debug_bounds("canvas-block-1").expect("应有图块");
    let card = cx.debug_bounds("selection-card-1").expect("选中后应有选中卡片");
    assert!(
        roughly_equal(card.origin.x, f32::from(block.origin.x))
            && roughly_equal(card.origin.y, f32::from(block.origin.y)),
        "选中卡片必须和图块完全重合、不能上移：卡片 {:?} 图块 {:?}",
        card.origin,
        block.origin
    );
}

/// 有选中项时，没选中的图块盖一层遮罩，选中的那张不盖。
#[test]
fn unselected_images_get_a_veil_and_selected_one_does_not() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(
        cx,
        &view,
        vec![
            test_item(1, 1000, 800, vec![line(600.0, 680.0)]),
            test_item(2, 1000, 800, vec![line(600.0, 680.0)]),
        ],
    );
    clear_selection_for_test(cx, &view);
    cx.update_entity(&view, |root, cx| root.select(1, cx));
    settle_selection(cx);
    draw(cx);

    let block = cx.debug_bounds("canvas-block-2").expect("应有第二张图");
    let veil = cx.debug_bounds("selection-veil-2").expect("未选中的图应有遮罩");
    assert!(
        roughly_equal(veil.origin.x, f32::from(block.origin.x))
            && roughly_equal(veil.origin.y, f32::from(block.origin.y)),
        "遮罩应贴住图块原点：遮罩 {:?} 图块 {:?}",
        veil.origin,
        block.origin
    );
    assert!(
        roughly_equal(veil.size.width, f32::from(block.size.width))
            && roughly_equal(veil.size.height, f32::from(block.size.height)),
        "遮罩应和图块一样大：遮罩 {:?} 图块 {:?}",
        veil.size,
        block.size
    );
    assert!(
        cx.debug_bounds("selection-veil-1").is_none(),
        "选中的那张不该盖遮罩"
    );
}

/// 第一次选中时，其他图的遮罩从 0 渐显到 1。
#[test]
fn first_selection_fades_every_other_veil() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(
        cx,
        &view,
        vec![
            test_item(1, 1000, 800, vec![line(600.0, 680.0)]),
            test_item(2, 1000, 800, vec![line(600.0, 680.0)]),
        ],
    );
    clear_selection_for_test(cx, &view);
    draw(cx);
    cx.update_entity(&view, |root, cx| root.select(1, cx));
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(60));
    cx.run_until_parked();
    let now = cx.executor().now();
    let dim_mid = cx.read_entity(&view, |root, _| root.selection_progress(2, now).1);
    assert!(
        (0.05..0.95).contains(&dim_mid),
        "选中 60ms 后另一张的遮罩应正在渐显，实际 {dim_mid}"
    );
    for _ in 0..10 {
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(20));
        cx.run_until_parked();
    }
    let now = cx.executor().now();
    let dim_done = cx.read_entity(&view, |root, _| root.selection_progress(2, now).1);
    assert!(
        dim_done > 0.99,
        "动画结束后另一张的遮罩应到 1，实际 {dim_done}"
    );
}

/// 点画布空白处：选中卡片和方勾渐隐，动画结束后状态归零。
#[test]
fn blank_canvas_click_animates_selection_out() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(cx, &view, vec![test_item(1, 1000, 800, vec![line(600.0, 680.0)])]);
    clear_selection_for_test(cx, &view);
    cx.update_entity(&view, |root, cx| root.select(1, cx));
    settle_selection(cx);
    draw(cx);
    assert!(
        cx.read_entity(&view, |root, _| root.selection_anim.is_none()),
        "选中动画应已经结束"
    );

    let canvas = cx.debug_bounds("canvas").expect("应有画布");
    let blank = gpui::point(
        canvas.origin.x + gpui::px(10.),
        canvas.origin.y + gpui::px(10.),
    );
    cx.simulate_mouse_down(blank, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    assert!(
        cx.read_entity(&view, |root, _| root.selection_anim.is_some()),
        "点空白处应开始选中淡出动画"
    );
    cx.simulate_mouse_up(blank, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(60));
    cx.run_until_parked();
    let now = cx.executor().now();
    let (lift, _, check) = cx.read_entity(&view, |root, _| root.selection_progress(1, now));
    assert!(
        lift > 0.0 && lift < 1.0,
        "60ms 时卡片应正在落回，抬起进度应在 0 和 1 之间，实际 {lift}"
    );
    assert!(check > 0.0, "60ms 时方勾应还在渐隐过程中，实际 {check}");
    for _ in 0..12 {
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(20));
        cx.run_until_parked();
    }
    draw(cx);
    assert!(
        cx.read_entity(&view, |root, _| root.selection_anim.is_none()),
        "动画结束后应清理选中动画"
    );
    let now = cx.executor().now();
    let (lift, _, check) = cx.read_entity(&view, |root, _| root.selection_progress(1, now));
    assert_eq!((lift, check), (0.0, 0.0), "动画结束后抬起和方勾应归零");
}

// 取消浮起后 `selection-ring-*` 外沿命中条随整套浮起机制删掉了：卡片不再
// 比图块高出一截，也没有「卡片外沿」需要接鼠标，原来测外沿点击的
// `selection_ring_click_keeps_selection` 不再适用。

// 取消浮起后卡片不再比图块高，也没有「卡片上方那条外沿带子」需要悬停；
// 原来测带子悬停保住裁剪线的 `hovering_lifted_ring_keeps_crop_line_lifted`
// 随这套机制一起不再适用。

/// 清空时队列里、在途的解码任务都要放下，计数归零，不能一直显示忙。
#[test]
fn clearing_while_decoding_keeps_busy_false() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    // 第一种：导入刚入队、worker 还没 pop，就在同一帧清空。
    let png = encode_test_png(4000, 4000, [60, 20, 20]);
    cx.update_entity(&view, |root, cx| {
        root.import_bytes("解码中导入.png".to_string(), png, cx);
        root.clear_all(cx);
    });
    cx.run_until_parked();
    assert_eq!(
        cx.read_entity(&view, |root, _| root.decoding),
        0,
        "清空后解码计数应立刻归零"
    );
    assert!(
        cx.read_entity(&view, |root, _| !root.is_busy()),
        "清空后不应一直处于忙状态"
    );
    assert_eq!(
        cx.read_entity(&view, |root, _| root.project.items.len()),
        0,
        "清空前排队中的导入不应把图加回来"
    );

    // 第二种：worker 已经把请求 pop 走、正在后台解码（这里直接把计数设成
    // 在途状态），再清空也必须在当场归零。
    cx.update_entity(&view, |root, _| {
        root.decoding = 1;
    });
    cx.dispatch_action(super::actions::ClearAll);
    cx.run_until_parked();
    assert_eq!(
        cx.read_entity(&view, |root, _| root.decoding),
        0,
        "在途解码时清空也应在当场把计数归零"
    );
    assert!(
        cx.read_entity(&view, |root, _| !root.is_busy()),
        "在途任务清空后不应一直处于忙状态"
    );
}

// ----------------------------------------------------------------------
// 选中和图块配合：拖裁剪线时方勾回到基础框、多选只抬主选中、切模式/快速切换收尾。
// ----------------------------------------------------------------------

/// 装图片但不选中、也不画第一帧：用来验证「某个元素从没出现过」。
fn install_items_unselected(
    cx: &mut gpui::VisualTestContext,
    view: &gpui::Entity<AppRoot>,
    items: Vec<Item>,
) {
    cx.update_entity(view, |root, cx| {
        root.project.items = items;
        root.selected = None;
        root.selected_ids.clear();
        root.selection_anim = None;
        sync_next_id_for_test(root);
        root.release_all_previews();
        cx.notify();
    });
    cx.run_until_parked();
}

/// 拖裁剪线时选中卡片留在图块原处，方勾也一直贴在卡片右上角内侧 8px。
///
/// 取消浮起后卡片本来就和图块重合，拖动裁剪线不再需要把卡片按回基础框；
/// 这里守住方勾位置不跟着裁剪线移动。
#[test]
fn crop_drag_keeps_check_on_base_frame() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(cx, &view, vec![test_item(1, 1000, 800, vec![line(600.0, 680.0)])]);
    draw(cx);
    let block = cx.debug_bounds("canvas-block-1").expect("应有图块");
    let card = cx.debug_bounds("selection-card-1").expect("选中后应有选中卡片");
    let block_right = f32::from(block.right());
    let block_top = f32::from(block.origin.y);
    assert!(
        (f32::from(card.right()) - block_right).abs() < 1.0
            && (f32::from(card.origin.y) - block_top).abs() < 1.0,
        "选中卡片应和图块完全重合：卡片 {:?} 图块 {:?}",
        card,
        block
    );

    let handle = cx
        .debug_bounds("crop-hotzone-1-bottom")
        .expect("应有下边界热区");
    let start = handle.center();
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    cx.simulate_mouse_move(
        gpui::point(start.x, start.y - gpui::px(20.)),
        Some(gpui::MouseButton::Left),
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
    draw(cx);
    let check = cx
        .debug_bounds("selection-check-1")
        .expect("拖动裁剪线时仍应画方勾");
    assert!(
        (f32::from(check.right()) - (block_right - 8.0)).abs() < 1.0,
        "拖动中方勾右边应贴图块右边内缩 8px：方勾 {} 图块右 {block_right}",
        f32::from(check.right())
    );
    assert!(
        (f32::from(check.origin.y) - (block_top + 8.0)).abs() < 1.0,
        "拖动中方勾上边应贴图块上边内缩 8px：方勾 {} 图块上 {block_top}",
        f32::from(check.origin.y)
    );
    cx.simulate_mouse_up(
        gpui::point(start.x, start.y - gpui::px(20.)),
        gpui::MouseButton::Left,
        gpui::Modifiers::default(),
    );
    cx.run_until_parked();
}

/// 多选时每一张选中的图都留在自己的图块上，主选中只决定右键菜单和裁剪同步。
#[test]
fn multi_select_keeps_every_selected_card_on_its_block() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(
        cx,
        &view,
        vec![
            test_item(1, 1000, 800, vec![line(600.0, 680.0)]),
            test_item(2, 1000, 800, vec![line(600.0, 680.0)]),
        ],
    );
    cx.update_entity(&view, |root, _| {
        root.selected = Some(1);
        root.selected_ids.clear();
        root.selected_ids.insert(1);
        root.selected_ids.insert(2);
        root.selection_anim = None;
    });
    draw(cx);
    let block1 = cx.debug_bounds("canvas-block-1").expect("应有第一张图");
    let block2 = cx.debug_bounds("canvas-block-2").expect("应有第二张图");
    for (selector, block) in [
        ("selection-card-1", block1),
        ("selection-card-2", block2),
    ] {
        let card = cx.debug_bounds(selector).expect("每张选中图都应有选中卡片");
        assert!(
            roughly_equal(card.origin.x, f32::from(block.origin.x))
                && roughly_equal(card.origin.y, f32::from(block.origin.y)),
            "选中卡片必须和图块完全重合、不能上移：{selector} 卡片 {:?} 图块 {:?}",
            card.origin,
            block.origin
        );
        assert!(
            roughly_equal(card.size.width, f32::from(block.size.width))
                && roughly_equal(card.size.height, f32::from(block.size.height)),
            "选中卡片宽高应严格等于图块：{selector} 卡片 {:?} 图块 {:?}",
            card.size,
            block.size
        );
    }
    let card2 = cx.debug_bounds("selection-card-2").expect("第二张应有卡片");
    let check2 = cx.debug_bounds("selection-check-2").expect("第二张应有方勾");
    assert!(
        roughly_equal(check2.right(), f32::from(card2.right()) - 8.0)
            && roughly_equal(check2.origin.y, f32::from(card2.origin.y) + 8.0),
        "方勾应贴在卡片右上角内侧 8px：方勾 {:?} 卡片 {:?}",
        check2.origin,
        card2.origin
    );
    for _ in 0..10 {
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(20));
        cx.run_until_parked();
    }
    draw(cx);
    assert!(
        cx.read_entity(&view, |root, _| root.selection_anim.is_none()),
        "没有切换动画时不该自己起动画"
    );
    assert!(
        cx.debug_bounds("selection-card-2").is_some(),
        "等一段时间后第二张仍应保持选中卡片"
    );
}

/// 切模式会清空选择，并且用一段淡出动画收尾。
#[test]
fn switching_mode_clears_selection_with_animation() {
    // 窗口一：先确认选中卡片画出来了，再切模式，确认动画和状态收尾。
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items_unselected(cx, &view, vec![test_item(1, 1000, 800, vec![line(600.0, 680.0)])]);
    draw(cx);
    cx.update_entity(&view, |root, cx| root.select(1, cx));
    settle_selection(cx);
    draw(cx);
    assert!(
        cx.debug_bounds("selection-card-1").is_some(),
        "切换前应有抬起卡片"
    );
    cx.update_entity(&view, |root, cx| root.set_mode(Mode::Layout, cx));
    assert!(
        cx.read_entity(&view, |root, _| root.selection_anim.is_some()),
        "切模式清空选择应起淡出动画"
    );
    assert!(
        cx.read_entity(&view, |root, _| root.selected_ids.is_empty()),
        "切模式后选择应清空"
    );
    for _ in 0..11 {
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(20));
        cx.run_until_parked();
    }
    assert!(
        cx.read_entity(&view, |root, _| root.selection_anim.is_none()),
        "淡出动画结束应清理"
    );

    // 窗口二：选中和切模式之间一帧都不画，确认清空后不会再画卡片和方勾。
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items_unselected(cx, &view, vec![test_item(1, 1000, 800, vec![line(600.0, 680.0)])]);
    draw(cx);
    cx.update_entity(&view, |root, cx| {
        root.select(1, cx);
        root.set_mode(Mode::Layout, cx);
    });
    // 先把时间推过淡出总时长再让窗口画第一帧，避免中间帧把卡片画进记录。
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(220));
    cx.run_until_parked();
    draw(cx);
    assert!(
        cx.read_entity(&view, |root, _| root.selection_anim.is_none()),
        "淡出动画结束应清理"
    );
    assert!(
        cx.debug_bounds("selection-card-1").is_none(),
        "切模式后不该留下抬起卡片"
    );
    assert!(
        cx.debug_bounds("selection-check-1").is_none(),
        "切模式后不该留下方勾"
    );
}

/// 快速来回切换选中：最后停在哪张就是哪张，旧方勾不能留下。
#[test]
fn quick_selection_switches_end_clean() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items_unselected(
        cx,
        &view,
        vec![
            test_item(1, 1000, 800, vec![line(600.0, 680.0)]),
            test_item(2, 1000, 800, vec![line(600.0, 680.0)]),
        ],
    );
    draw(cx);
    cx.update_entity(&view, |root, cx| root.select(1, cx));
    settle_selection(cx);
    draw(cx);
    cx.update_entity(&view, |root, cx| root.select(2, cx));
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(60));
    cx.update_entity(&view, |root, cx| root.select(1, cx));
    // 最后一次切换后先把时间推过淡出总时长，再让窗口画帧，避免第二张的
    // 方勾在淡出中间帧被画进记录里。
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(320));
    cx.run_until_parked();
    draw(cx);
    assert!(
        cx.read_entity(&view, |root, _| root.selection_anim.is_none()),
        "停止切换后动画应结束"
    );
    let (selected, ids) = cx.read_entity(&view, |root, _| {
        (root.selected, root.selected_ids.clone())
    });
    assert_eq!(selected, Some(1), "最终应停在第一张");
    let ids: Vec<ItemId> = ids.into_iter().collect();
    assert_eq!(ids, vec![1], "最终选择集合应只有第一张");
    assert!(
        cx.debug_bounds("selection-check-2").is_none(),
        "第二张的方勾不该留在画面上"
    );
}

/// 清空后先交出位图，等旧帧换掉、延迟到点才做 malloc_trim。
#[test]
fn clear_waits_for_frame_before_trim() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(cx, &view, vec![test_item(1, 1000, 800, vec![line(600.0, 680.0)])]);
    draw(cx);
    assert!(
        wait_until(cx, |cx| {
            cx.read_entity(&view, |root, _| {
                root.tile_cache
                    .values()
                    .any(|grid| grid.counts().0 > 0)
            })
        }),
        "画完应有已经生成的图块"
    );
    assert!(
        wait_until(cx, |cx| cx.read_entity(&view, |root, _| !root.thumb_cache.is_empty())),
        "画完应有缩略图"
    );
    // 清空当场就读待释放队列：测试窗口在 update 返回后可能已经画了一帧，
    // 那一帧会把队列交给窗口，所以必须在这个闭包里取。
    let queued = cx.update_entity(&view, |root, cx| {
        root.clear_all(cx);
        assert!(
            root.tile_cache.is_empty() && root.thumb_cache.is_empty(),
            "清空后缓存应立即为空"
        );
        root.images_to_drop.len()
    });
    assert!(queued > 0, "清空应把位图排进待释放队列");
    let (pending, due) = cx.read_entity(&view, |root, _| (root.trim_pending, root.trim_due.is_some()));
    assert!(pending && due, "清空应排一次延迟回收");

    // 第一帧只把位图交给窗口，50ms 还没到，不能 trim。
    draw(cx);
    let (pending_after, queued_after) =
        cx.read_entity(&view, |root, _| (root.trim_pending, root.images_to_drop.len()));
    assert!(queued_after == 0, "第一帧后释放队列应排空");
    assert!(pending_after, "延迟未到前不能提前 trim");

    // 时间到、再画一帧，回收收尾。
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(60));
    cx.run_until_parked();
    draw(cx);
    let still_pending = cx.read_entity(&view, |root, _| root.trim_pending);
    assert!(!still_pending, "延迟到点且队列排空后应完成回收");
}

/// 多选/全选时每一张卡片都保持自己图块的原尺寸、留在图块原处。
#[test]
fn all_selected_cards_keep_original_size_and_position() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(
        cx,
        &view,
        vec![
            test_item(1, 1000, 800, vec![line(600.0, 680.0)]),
            test_item(2, 700, 500, vec![line(400.0, 440.0)]),
            test_item(3, 1200, 900, vec![line(800.0, 840.0)]),
        ],
    );
    draw(cx);
    for id in 1..=3u64 {
        let block = cx
            .debug_bounds(match id {
                1 => "canvas-block-1",
                2 => "canvas-block-2",
                _ => "canvas-block-3",
            })
            .expect("三张图都应在画布上");
        let card = cx
            .debug_bounds(match id {
                1 => "selection-card-1",
                2 => "selection-card-2",
                _ => "selection-card-3",
            })
            .expect("全选时每张都应有抬起卡片");
        assert!(
            roughly_equal(card.size.width, f32::from(block.size.width))
                && roughly_equal(card.size.height, f32::from(block.size.height)),
            "第 {id} 张卡片宽高应等于图块、不能拉伸：卡片 {:?} 图块 {:?}",
            card.size,
            block.size
        );
        assert!(
            roughly_equal(card.origin.x, f32::from(block.origin.x))
                && roughly_equal(card.origin.y, f32::from(block.origin.y)),
            "第 {id} 张卡片应和图块完全重合、不能上移：卡片 {:?} 图块 {:?}",
            card.origin,
            block.origin
        );
    }
}

/// 导入新图不改变已有选择。
#[test]
fn import_does_not_select_new_image() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(
        cx,
        &view,
        vec![
            test_item(1, 1000, 800, vec![line(600.0, 680.0)]),
            test_item(2, 1000, 800, vec![line(600.0, 680.0)]),
        ],
    );
    clear_selection_for_test(cx, &view);
    cx.update_entity(&view, |root, cx| root.select(1, cx));
    settle_selection(cx);
    draw(cx);
    let selected_before = cx.read_entity(&view, |root, _| (root.selected, root.selected_ids.clone()));
    cx.update_entity(&view, |root, cx| {
        root.push_image_with("后来导入.png".into(), decoded_solid(800, 600, 120), cx);
    });
    // 导入后画一帧：id 同步正确时这里不会卡住。
    draw(cx);
    let (selected, ids, all_ids, next_id) = cx.read_entity(&view, |root, _| {
        (
            root.selected,
            root.selected_ids.clone(),
            root.project
                .items
                .iter()
                .map(|item| item.id)
                .collect::<Vec<_>>(),
            root.next_id,
        )
    });
    assert_eq!(all_ids.len(), 3, "导入后项目里应有三张图");
    let mut unique = all_ids.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(
        unique.len(),
        all_ids.len(),
        "导入后项目里的 id 不能重复：{all_ids:?}"
    );
    let max_id = *all_ids.iter().max().expect("至少有一张图");
    assert!(
        next_id > max_id,
        "next_id 必须大于项目里所有 id：next_id={next_id} ids={all_ids:?}"
    );
    assert_eq!(selected, selected_before.0, "导入不应改变主选中");
    let ids: Vec<ItemId> = ids.into_iter().collect();
    let before: Vec<ItemId> = selected_before.1.into_iter().collect();
    assert_eq!(ids, before, "导入不应改变选中集合");
    assert!(
        !cx.read_entity(&view, |root, _| root.selected_ids.contains(&3)),
        "新导入的图不该自动进选中集合"
    );
}

/// 卡片只换位置不换尺寸，图片和字幕不会被拉伸。
#[test]
fn selection_card_does_not_scale_content() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(cx, &view, vec![test_item(1, 1000, 800, vec![line(600.0, 680.0)])]);
    draw(cx);
    let block = cx.debug_bounds("canvas-block-1").expect("应有图块");
    let card = cx.debug_bounds("selection-card-1").expect("应有抬起卡片");
    assert!(
        roughly_equal(card.size.width, f32::from(block.size.width))
            && roughly_equal(card.size.height, f32::from(block.size.height)),
        "卡片尺寸必须等于图块：卡片 {:?} 图块 {:?}",
        card.size,
        block.size
    );
    assert!(
        roughly_equal(card.origin.x, f32::from(block.origin.x)),
        "卡片横坐标必须等于图块，不能左右扩：卡片 {:?} 图块 {:?}",
        card.origin,
        block.origin
    );
    assert!(
        roughly_equal(card.origin.y, f32::from(block.origin.y)),
        "卡片不能上移、必须和图块纵坐标一致：卡片 {:?} 图块 {:?}",
        card.origin,
        block.origin
    );
}

// 取消浮起后卡片不再上移，裁剪线和热区也没有「跟着卡片一起抬」这回事：
// 它们始终贴着图块边界。原来测这个跟随关系的
// `lifted_crop_line_and_hotzone_follow_card` 不再适用。




/// 导入后连画 3 帧都要能正常返回：安装测试图时 next_id 没同步会让同一 id
/// 的两张图共用图块缓存和识别结果，测试就会卡在无限重生成里。
#[test]
fn import_then_draw_three_frames_returns() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(cx, &view, vec![test_item(1, 1000, 800, vec![line(600.0, 680.0)])]);
    cx.update_entity(&view, |root, cx| {
        root.push_image_with("导入.png".into(), decoded_solid(800, 600, 120), cx);
    });
    for frame in 1..=3 {
        draw(cx);
        assert_eq!(
            cx.read_entity(&view, |root, _| root.project.items.len()),
            2,
            "第 {frame} 帧后项目里应还是两张图"
        );
    }
    let (all_ids, next_id) = cx.read_entity(&view, |root, _| {
        (
            root.project
                .items
                .iter()
                .map(|item| item.id)
                .collect::<Vec<_>>(),
            root.next_id,
        )
    });
    let mut unique = all_ids.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), all_ids.len(), "id 不能重复：{all_ids:?}");
    let max_id = *all_ids.iter().max().expect("至少有一张图");
    assert!(
        next_id > max_id,
        "next_id 必须大于所有 id：next_id={next_id} ids={all_ids:?}"
    );
}


/// 清空后第一次导入：导入不改变选择，空作品导入后选择仍是空的。
#[test]
fn import_into_empty_project_keeps_selection_empty() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items_unselected(cx, &view, vec![]);
    cx.update_entity(&view, |root, cx| {
        // 排版模式不需要识别，导入不会起后台任务，断言只看选择状态。
        root.project.mode = Mode::Layout;
        root.push_image_with("空作品导入.png".into(), decoded_solid(800, 600, 120), cx);
    });
    let (selected, selected_ids, count) = cx.read_entity(&view, |root, _| {
        (
            root.selected,
            root.selected_ids.iter().copied().collect::<Vec<ItemId>>(),
            root.project.items.len(),
        )
    });
    assert_eq!(count, 1, "导入后项目里应有一张图");
    assert_eq!(selected, None, "空作品导入不该自动选中新图");
    assert!(
        selected_ids.is_empty(),
        "空作品导入不该把新图加进选中集合：{selected_ids:?}"
    );
}

/// 连续两次触发释放：第一次的唤醒定时器到点后 trim_due 已被推后，还得再排
/// 一次唤醒，否则 trim_pending 永远挂着，内存页再也不还给系统。
#[test]
fn two_release_rounds_keep_trim_waking_until_done() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items(cx, &view, vec![test_item(1, 1000, 800, vec![line(600.0, 680.0)])]);

    // 第一次释放：清空作品。
    cx.update_entity(&view, |root, cx| root.clear_all(cx));
    let pending = cx.read_entity(&view, |root, _| root.trim_pending);
    assert!(pending, "清空后应挂着延迟回收");

    // 第二次释放：撤销回带图的作品，schedule_trim 把到期时间又推后 50ms，
    // 但第一次排的定时器不会重新计时。
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(20));
    cx.run_until_parked();
    cx.dispatch_action(super::actions::UndoEdit);

    // 让第一次的定时器到点醒一次：这时还没到新的 trim_due，Render 必须补排。
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(40));
    cx.run_until_parked();

    // 再推过新的到期时间；没有补排的话这里不会再有帧，pending 一直挂着。
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(200));
    cx.run_until_parked();

    let (pending, due) = cx.read_entity(&view, |root, _| (root.trim_pending, root.trim_due));
    assert!(!pending, "到期后 trim_pending 应变回 false");
    assert_eq!(due, None, "收尾后 trim_due 应清空");
}


/// 改法二：只要有图被选中，没选中的图整个不挂裁剪热区。
///
/// 选中的卡片顶部会向上抬 8 像素，和上面没选中的图叠在同一块地方；两张都接
/// 鼠标时那块地方会出现「亮的是上面那张、点中间选中的是下面那张、点左右开始拖
/// 的又是上面那张底边」三种结果。所以没选中的图整张退出悬停和裁剪。
#[test]
fn selected_image_locks_others_out_of_crop_hotzones() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    cx.simulate_resize(gpui::size(gpui::px(1200.), gpui::px(1200.)));
    install_items_unselected(cx, &view, stacked_items());
    draw(cx);
    let first = cx.debug_bounds("canvas-block-1").expect("第一张应在画布上");
    cx.simulate_click(first.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    settle_selection(cx);
    draw(cx);
    assert_eq!(
        cx.read_entity(&view, |root, _| root.selected_ids.iter().copied().collect::<Vec<ItemId>>()),
        vec![1],
        "点一下第一张应只选中它"
    );
    // 选中的第一张照常有上下裁剪热区，证明热区本身没被整体关掉。
    for selector in ["crop-hotzone-1-top", "crop-hotzone-1-bottom"] {
        assert!(
            cx.debug_bounds(selector).is_some(),
            "选中的第一张应保留裁剪热区 {selector}"
        );
    }
    // 没选中的第二张一条裁剪热区都不该有。
    for selector in [
        "crop-hotzone-2-top",
        "crop-hotzone-2-bottom",
        "crop-hotzone-2-left",
        "crop-hotzone-2-right",
    ] {
        assert!(
            cx.debug_bounds(selector).is_none(),
            "没选中的第二张不该有裁剪热区 {selector}"
        );
    }
}

/// 改法二：没选中的图也不该拿到焦点框，哪怕指针停在它上面。
#[test]
fn selected_image_locks_others_out_of_focus_ring() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    cx.simulate_resize(gpui::size(gpui::px(1200.), gpui::px(1200.)));
    install_items_unselected(cx, &view, stacked_items());
    draw(cx);
    let first = cx.debug_bounds("canvas-block-1").expect("第一张应在画布上");
    cx.simulate_click(first.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    settle_selection(cx);
    draw(cx);

    let second = cx.debug_bounds("canvas-block-2").expect("第二张应在画布上");
    cx.simulate_mouse_move(second.center(), None, gpui::Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert!(
        cx.debug_bounds("block-focus-2").is_none(),
        "没选中的第二张不该拿到焦点框"
    );

    // 正面控制：指针回到选中的第一张上，焦点框要正常出现，证明悬停链路本身活着。
    let first = cx.debug_bounds("canvas-block-1").expect("第一张应在画布上");
    cx.simulate_mouse_move(first.center(), None, gpui::Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert!(
        cx.debug_bounds("block-focus-1").is_some(),
        "选中的第一张悬停时应有焦点框"
    );
}

// 取消浮起后 `selection-ring-*` 顶部命中条已随浮起机制删除：卡片和图块
// 完全重合，不再有卡片顶边需要单独接鼠标，原来测命中条宽度铺满卡片的
// `selection_ring_top_strip_spans_the_whole_card` 不再适用。

/// 改法二：点一下没选中的图，它会被选中（并且只选中它）。
#[test]
fn clicking_an_unselected_image_selects_only_it() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    cx.simulate_resize(gpui::size(gpui::px(1200.), gpui::px(1200.)));
    install_items_unselected(cx, &view, stacked_items());
    draw(cx);
    let first = cx.debug_bounds("canvas-block-1").expect("第一张应在画布上");
    cx.simulate_click(first.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    settle_selection(cx);
    draw(cx);
    assert_eq!(
        cx.read_entity(&view, |root, _| root.selected_ids.iter().copied().collect::<Vec<ItemId>>()),
        vec![1],
        "先选中第一张"
    );

    let second = cx.debug_bounds("canvas-block-2").expect("第二张应在画布上");
    cx.simulate_click(second.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    settle_selection(cx);
    draw(cx);
    let (selected, ids) = cx.read_entity(&view, |root, _| {
        (
            root.selected,
            root.selected_ids.iter().copied().collect::<Vec<ItemId>>(),
        )
    });
    assert_eq!(selected, Some(2), "点没选中的第二张应把它设为主选中");
    assert_eq!(ids, vec![2], "点没选中的图应只选中它自己");
}

/// 改法二：没选中的图仍然能右键弹图片菜单、按住拖动排序。
///
/// 这两条入口不和抬起卡片顶部那 8 像素重叠，所以没选中的图继续接它们；
/// 只有悬停和裁剪边被摘掉。
#[test]
fn unselected_image_keeps_right_click_menu_and_drag_sort() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items_unselected(cx, &view, stacked_items());
    draw(cx);

    // 先选中第一张，让第二张变成「没选中的图」。
    let first = cx.debug_bounds("canvas-block-1").expect("第一张应在画布上");
    cx.simulate_click(first.center(), gpui::Modifiers::default());
    cx.run_until_parked();
    settle_selection(cx);
    draw(cx);
    assert_eq!(
        cx.read_entity(&view, |root, _| root.selected_ids.iter().copied().collect::<Vec<ItemId>>()),
        vec![1],
        "先选中第一张，让第二张变成没选中的图"
    );

    // 右键没选中的第二张：图片菜单照常弹出。
    // 菜单会顺手把第二张选中，这是右键卡片的原有行为。
    let second = cx.debug_bounds("canvas-block-2").expect("第二张应在画布上");
    let point = second.center();
    cx.simulate_mouse_down(point, gpui::MouseButton::Right, gpui::Modifiers::default());
    cx.run_until_parked();
    let menu_kind = cx.read_entity(&view, |root, _| root.menu.as_ref().map(|menu| menu.kind));
    assert!(
        matches!(menu_kind, Some(super::MenuKind::Image(2))),
        "右键没选中的第二张应弹出它的图片菜单，实际 {menu_kind:?}"
    );
    cx.simulate_mouse_up(point, gpui::MouseButton::Right, gpui::Modifiers::default());
    cx.run_until_parked();
    cx.update_entity(&view, |root, cx| root.close_menu(cx));
    draw(cx);
    assert_eq!(
        cx.read_entity(&view, |root, _| root.selected_ids.iter().copied().collect::<Vec<ItemId>>()),
        vec![2],
        "右键菜单应把第二张选中，这时第一张是没选中的图"
    );

    assert!(
        wait_until(cx, |cx| !tile_keys(cx, &view).is_empty()),
        "拖动前应已生成图块"
    );

    // 按住现在没选中的第一张往下拖 40 像素，松手后两者换位。
    // 位移和「把选中的图拖去排序」那条用例一致，区别只是这次拖的图没被选中。
    let first = cx.debug_bounds("canvas-block-1").expect("第一张应在画布上");
    let start = first.center();
    cx.simulate_mouse_down(start, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
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
    assert!(
        cx.read_entity(&view, |root, _| root.dragging.is_some()),
        "按住没选中的图拖动应进入排序拖动"
    );
    cx.simulate_mouse_up(cursor, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    let order: Vec<ItemId> = cx.read_entity(&view, |root, _| {
        root.project.items.iter().map(|item| item.id).collect()
    });
    assert_eq!(order, vec![2, 1], "把没选中的第一张拖到第二张处松手后顺序应互换");
}

/// 导航器盖在一张图块上时，悬停要归导航器，不能被下面的图抢走。
///
/// 坐标来历（760x600 视口、4 张 1000x1000、1:1 缩放、画两帧后实测）：
/// 导航器 rect = (20, 396, 160, 120)，第 1 张图块 rect = (48, 24, 1000, 775)。
/// 取点 (160, 450)：横坐标在导航器 20..180 和第 1 张 48..1048 内，纵坐标在
/// 导航器 396..516 和第 1 张 24..799 内，是两块区域真正重叠的位置。
/// 导航器用 block_mouse_except_scroll 接住鼠标，这里必须变成 Widget("navigator")，
/// 不能是 Item(1)。和 navigator_lets_the_wheel_through_to_the_canvas 是一对：
/// 那条管滚轮要能穿过去落到画布，这条管悬停要被导航器挡住。
///
/// 这里用 1:1 把作品撑出可视区：字幕模式的「适合画布」会把整条压进窗口，作品不再
/// 溢出，导航器就不显示了。导航器的显示条件只看有没有溢出，跟用哪种缩放方式无关。
#[test]
fn navigator_keeps_hover_when_it_covers_an_image() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    cx.simulate_resize(gpui::size(gpui::px(760.), gpui::px(600.)));
    let items = (1..=4)
        .map(|id| test_item(id, 1000, 1000, vec![line(700.0, 760.0)]))
        .collect();
    install_items(cx, &view, items);
    cx.update_entity(&view, |root, cx| {
        root.set_manual_scale(1.0, cx);
        root.selected = Some(3);
        root.selected_ids.clear();
        root.selected_ids.insert(3);
        root.selection_anim = None;
        cx.notify();
    });
    draw(cx);
    draw(cx);

    let navigator = cx.debug_bounds("navigator").expect("作品溢出时应显示导航器");
    let block = cx
        .debug_bounds("canvas-block-1")
        .expect("第 1 张应在画布上");
    // 先把实测几何钉住；坐标是下面这条重叠点断言的前提。
    assert!(
        roughly_equal(navigator.origin.x, 20.0)
            && roughly_equal(navigator.origin.y, 396.0)
            && roughly_equal(navigator.size.width, 160.0)
            && roughly_equal(navigator.size.height, 120.0),
        "导航器几何和实测不一致：{navigator:?}"
    );
    assert!(
        roughly_equal(block.origin.x, 48.0)
            && roughly_equal(block.origin.y, 24.0)
            && roughly_equal(block.size.width, 1000.0)
            && roughly_equal(block.size.height, 775.0),
        "第 1 张图块几何和实测不一致：{block:?}"
    );
    let point = gpui::point(gpui::px(160.), gpui::px(450.));
    assert!(
        f32::from(navigator.origin.x) <= 160.0
            && 160.0 <= f32::from(navigator.origin.x) + f32::from(navigator.size.width)
            && f32::from(navigator.origin.y) <= 450.0
            && 450.0 <= f32::from(navigator.origin.y) + f32::from(navigator.size.height),
        "取点应在导航器范围内"
    );
    assert!(
        f32::from(block.origin.x) <= 160.0
            && 160.0 <= f32::from(block.origin.x) + f32::from(block.size.width)
            && f32::from(block.origin.y) <= 450.0
            && 450.0 <= f32::from(block.origin.y) + f32::from(block.size.height),
        "取点应落在第 1 张图块上，才能证明悬停没被图抢走"
    );

    cx.simulate_mouse_move(point, None, gpui::Modifiers::default());
    cx.run_until_parked();
    let active = cx.read_entity(&view, |root, _| root.hover_gate.debug_active());
    assert!(
        !matches!(active, Some(HoverKey::Item(_))),
        "重叠点上悬停不该归图块，实际 {active:?}"
    );
    assert_eq!(
        active,
        Some(HoverKey::widget("navigator")),
        "重叠点上悬停应归导航器"
    );
}

#[test]
fn crop_line_shows_without_clicking_when_nothing_is_selected() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    install_items_unselected(
        cx,
        &view,
        vec![test_item(1, 1000, 1000, vec![line(700.0, 760.0)])],
    );
    draw(cx);
    let (selected, selected_ids) = cx.read_entity(&view, |root, _| {
        (
            root.selected,
            root.selected_ids.iter().copied().collect::<Vec<ItemId>>(),
        )
    });
    assert_eq!(selected, None, "这条测试的前提是没有选中的图");
    assert!(selected_ids.is_empty(), "这条测试的前提是选中集合为空：{selected_ids:?}");

    let block = cx.debug_bounds("canvas-block-1").expect("应画出一张图");
    cx.simulate_mouse_move(block.center(), None, gpui::Modifiers::default());
    cx.run_until_parked();
    draw(cx);
    assert_eq!(
        cx.read_entity(&view, |root, _| root.hover_gate.debug_active()),
        Some(HoverKey::Item(1)),
        "一张图都没选中时，指针停在图上应激活这张图的悬停"
    );
    assert!(
        cx.debug_bounds("crop-line-top").is_some(),
        "没有选中图时，悬停也就该画出上边界裁剪线"
    );
    let hotzone = cx
        .debug_bounds("crop-hotzone-1-top")
        .expect("没有选中图时，悬停也就该出现上边界热区");

    // 不点击图块本身，直接压在热区上按下：应立刻进入裁剪拖动。
    let press = gpui::point(hotzone.center().x, hotzone.origin.y + gpui::px(4.));
    cx.simulate_mouse_down(press, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    let drag = cx.read_entity(&view, |root, _| {
        root.crop_drag.as_ref().map(|drag| drag.id)
    });
    assert_eq!(
        drag,
        Some(1),
        "没先点图、直接按热区也应开始第 1 张的裁剪拖动"
    );
    cx.simulate_mouse_up(press, gpui::MouseButton::Left, gpui::Modifiers::default());
    cx.run_until_parked();
    assert!(
        cx.read_entity(&view, |root, _| root.crop_drag.is_none()),
        "松手后应结束裁剪拖动"
    );
}

/// 导航器挡住悬停的同时必须把滚轮放过去：它只盖住鼠标命中，不挖掉后面的滚动命中盒。
///
/// 和 `navigator_keeps_hover_when_it_covers_an_image` 是一对：
/// 那条管「悬停要被导航器挡住」，这条管「滚轮要能落到画布」。
/// 导航器只在作品超出可视区时出现，正是最需要滚的时候；换成 `.occlude()` 会
/// 把后面的滚动命中盒一起切成不可见，指针停在导航器上时滚轮就成了死的。
///
/// 这里用 1:1 把作品撑出可视区：字幕模式的「适合画布」会把整条压进窗口，作品不再
/// 溢出，导航器就不显示了。导航器的显示条件只看有没有溢出，跟用哪种缩放方式无关。
#[test]
fn navigator_lets_the_wheel_through_to_the_canvas() {
    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    cx.simulate_resize(gpui::size(gpui::px(760.), gpui::px(600.)));
    let items = (1..=4)
        .map(|id| test_item(id, 1000, 1000, vec![line(700.0, 760.0)]))
        .collect();
    install_items(cx, &view, items);
    cx.update_entity(&view, |root, cx| {
        root.set_manual_scale(1.0, cx);
        root.selected = Some(3);
        root.selected_ids.clear();
        root.selected_ids.insert(3);
        root.selection_anim = None;
        cx.notify();
    });
    draw(cx);
    draw(cx);

    let navigator = cx.debug_bounds("navigator").expect("作品溢出时应显示导航器");
    let center = navigator.center();
    cx.simulate_mouse_move(center, None, gpui::Modifiers::default());
    cx.run_until_parked();
    // 第一半：悬停归导航器，证明遮挡还在。
    assert_eq!(
        cx.read_entity(&view, |root, _| root.hover_gate.debug_active()),
        Some(HoverKey::widget("navigator")),
        "指针在导航器上时悬停应归导航器"
    );

    // 第二半：滚轮要穿过去滚动画布。
    let before = cx.read_entity(&view, |root, _| root.canvas_scroll.offset());
    let wheel = || gpui::ScrollWheelEvent {
        position: center,
        delta: gpui::ScrollDelta::Lines(gpui::point(0.0, -2.0)),
        modifiers: gpui::Modifiers::default(),
        touch_phase: gpui::TouchPhase::Moved,
    };
    for _ in 0..3 {
        cx.simulate_event(wheel());
        cx.run_until_parked();
    }
    let after = cx.read_entity(&view, |root, _| root.canvas_scroll.offset());
    assert_ne!(
        after, before,
        "指针停在导航器上滚轮应能滚动画布；被整个挡住时偏移不会变"
    );
}

/// 清空作品后要把内存还给系统：导入一张真实 4K 图推高堆水位，清空并走完
/// 延迟回收后，常驻内存必须明显降下来。
///
/// 这条必须先在进程里调用启动路径同一个 `limit_malloc_arenas`：不限 arena 时
/// glibc 给后台解码线程各开一个分配区，`malloc_trim` 收不回那些页，RSS 会停在
/// 高位。断言只比相对关系，不比绝对值（不同机器基线不同）。
#[cfg(all(target_os = "linux", target_env = "gnu"))]
fn measure_clear_all_memory() -> Option<(u64, u64, u64)> {
    assert!(
        super::limit_malloc_arenas(),
        "限制 glibc arena 应成功，否则这条测的是旧行为"
    );
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("示例图片/问题B示例_不能正确识别字幕/粘贴的图像 (2).png");
    if !path.is_file() {
        // 仓库里没有示例图片时直接跳过，不让别的环境跑不了。
        return None;
    }
    // /proc/self/statm 第二个字段是常驻页数，一页 4KB。
    let resident_bytes = || -> u64 {
        let statm = std::fs::read_to_string("/proc/self/statm").unwrap_or_default();
        let pages: u64 = statm
            .split_whitespace()
            .nth(1)
            .and_then(|value| value.parse().ok())
            .unwrap_or(0);
        pages * 4096
    };

    let mut cx = TestAppContext::single();
    let (view, cx) = cx.add_window_view(AppRoot::new);
    // 窗口建好、还没导入时的基线：下面判断「导入撑起来的内存还回去多少」。
    let baseline = resident_bytes();
    cx.update_entity(&view, |root, cx| root.open_paths(vec![path.clone()], cx));
    assert!(
        wait_until(cx, |cx| {
            cx.read_entity(&view, |root, _| {
                root.project.items.len() == 1 && root.decoding == 0
            })
        }),
        "4K 图应导入完成"
    );
    // 画几帧让图块/缩略图生成，把内存推到高水位，正是要复现的场景。
    for _ in 0..3 {
        draw(cx);
    }
    let after_import = resident_bytes();

    cx.update_entity(&view, |root, cx| root.clear_all(cx));
    for _ in 0..3 {
        draw(cx);
    }
    cx.executor()
        .advance_clock(std::time::Duration::from_millis(60));
    cx.run_until_parked();
    for _ in 0..3 {
        draw(cx);
    }
    let after_clear = resident_bytes();
    Some((baseline, after_import, after_clear))
}

/// 子进程里只跑这一条测试时用的环境变量，避免无限递归。
#[cfg(all(target_os = "linux", target_env = "gnu"))]
const MEMORY_MEASURE_CHILD_ENV: &str = "GISTPIC_MEMORY_MEASURE_CHILD";

/// 清空作品后 RSS 必须降回接近导入前的水平。
///
/// 为什么要在独立子进程里量：RSS 是整个进程的数字，cargo test 默认在同进程里
/// 并行跑别的测试，它们的占用会被算进每次采样。实测同进程并行时清空后是
/// 136MB，单独跑只要 34MB，比例阈值就判断不了。这里用当前测试程序重新起一个
/// 只跑这条测试的子进程，量到的才是这个程序自己清空后的占用。
#[cfg(all(target_os = "linux", target_env = "gnu"))]
#[test]
fn clear_all_returns_memory_to_the_system() {
    if std::env::var_os(MEMORY_MEASURE_CHILD_ENV).is_some() {
        match measure_clear_all_memory() {
            Some((baseline, after_import, after_clear)) => {
                println!("MEMORY_RSS {baseline} {after_import} {after_clear}");
            }
            // 仓库里没有示例图片（稀疏检出、源码打包、编译后搬家都可能）：
            // 明确告诉父进程跳过，不能让父进程误以为子进程挂了。
            None => println!("MEMORY_SKIP"),
        }
        return;
    }

    let exe = std::env::current_exe().expect("应能拿到测试程序路径");
    let mut child = std::process::Command::new(exe)
        .arg("--exact")
        .arg("app::ui_smoke::clear_all_returns_memory_to_the_system")
        .arg("--nocapture")
        .env(MEMORY_MEASURE_CHILD_ENV, "1")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("应能启动内存测量子进程");
    // 子进程最多等 120 秒：万一启动路径以后变成会卡住的东西，整轮测试不能
    // 一直挂在这里，超时就杀掉并给出明确原因。
    let timeout = std::time::Duration::from_secs(120);
    let deadline = std::time::Instant::now() + timeout;
    loop {
        match child.try_wait().expect("查询内存测量子进程状态失败") {
            Some(_) => break,
            None if std::time::Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("内存测量子进程超过 {} 秒没结束，可能卡住了", timeout.as_secs());
            }
            None => std::thread::sleep(std::time::Duration::from_millis(50)),
        }
    }
    let output = child.wait_with_output().expect("应能读取内存测量子进程输出");
    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.lines().any(|line| line.trim() == "MEMORY_SKIP") {
        // 子进程明确说没有示例图片，直接跳过这条。
        return;
    }
    let numbers = stdout.lines().find_map(|line| {
        let mut parts = line.strip_prefix("MEMORY_RSS ")?.split_whitespace();
        let baseline: u64 = parts.next()?.parse().ok()?;
        let after_import: u64 = parts.next()?.parse().ok()?;
        let after_clear: u64 = parts.next()?.parse().ok()?;
        Some((baseline, after_import, after_clear))
    });
    let Some((baseline, after_import, after_clear)) = numbers else {
        panic!(
            "内存测量子进程没有给出数字。stdout: {stdout}\nstderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    };
    assert!(
        output.status.success(),
        "内存测量子进程失败：stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // 前置条件：导入 4K 图必须真的把内存明显地撑起来，否则下面的
    // 「还回四分之三」比例断言没有意义（增量太小，比例怎么算都容易过）。
    // 如果以后导入路径优化到涨幅不到 100MB，是这条断言的阈值要跟着调，
    // 不是内存回退。
    assert!(
        after_import > baseline + 100 * 1024 * 1024,
        "导入 4K 图后 RSS 应比基线高至少 100MB（导入路径变省时调这个阈值，不是内存坏了）：基线 {baseline} 导入后 {after_import}"
    );
    // 阈值按「导入撑起来的增量至少还回去四分之三」定：
    // 清空后 < 基线 + (导入后 - 基线) / 4。
    // 本机实测：基线 26MB、导入后 195MB、正确实现清空后 34MB（增量剩约 8MB，
    // 占 169MB 增量的约 5%）；去掉 limit_malloc_arenas 时清空后 88MB
    // （增量剩约 61MB，占 36%），按这个阈值会红。
    let allowed = baseline + (after_import - baseline) / 4;
    assert!(
        after_clear < allowed,
        "清空后 RSS 应至少还回导入增量的四分之三：基线 {baseline} 导入后 {after_import} 清空后 {after_clear}，允许上限 {allowed}"
    );
}

/// 从 Rust 源码里取出某个函数的函数体：从签名后的第一个 `{` 开始按大括号配对，
/// 扫描时跳过 `//` 行注释和普通字符串，避免注释或字符串里的括号把配对搅乱。
///
/// 只给测试做源码断言用，不处理块注释、字符字面量和原始字符串：main.rs 里
/// `run()` 的函数体没有这些东西，真加了再补。
fn rust_function_body<'a>(source: &'a str, signature: &str) -> &'a str {
    let start = source
        .find(signature)
        .unwrap_or_else(|| panic!("源码里找不到 {signature}"));
    let open = start
        + source[start..]
            .find('{')
            .unwrap_or_else(|| panic!("{signature} 应有函数体"));
    let bytes = source.as_bytes();
    let mut depth = 0usize;
    let mut index = open;
    let mut in_string = false;
    let mut escaped = false;
    let mut in_line_comment = false;
    while index < bytes.len() {
        let byte = bytes[index];
        if in_line_comment {
            if byte == b'\n' {
                in_line_comment = false;
            }
        } else if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
        } else {
            match byte {
                b'/' if bytes.get(index + 1) == Some(&b'/') => {
                    in_line_comment = true;
                    index += 1;
                }
                b'"' => in_string = true,
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        return &source[open + 1..index];
                    }
                }
                _ => {}
            }
        }
        index += 1;
    }
    panic!("{signature} 的函数体没有闭合");
}

/// 启动路径必须真的调用 limit_malloc_arenas：删掉 src/main.rs 里那行，
/// 用户内存问题就会回来。
///
/// 为什么用源码断言：本仓库禁止有头测试，测试里没法真的跑 `run()` 起窗口。
/// 内存测试自己会调 limit_malloc_arenas，只能证明「这个函数有效」，证明不了
/// 「程序启动时真的调了它」；批评者实测删掉 main.rs 那行后 348 条测试仍然全绿。
/// 这里直接读 src/main.rs 的源码，盯住 `run()` 函数体里的调用，挡住这种回退。
#[test]
fn main_limits_malloc_arenas_on_startup() {
    let source = include_str!("../main.rs");
    let body = rust_function_body(source, "fn run()");
    let call = "limit_malloc_arenas(";
    let call_line = body
        .lines()
        .position(|line| {
            let code = line.trim_start();
            !code.starts_with("//") && code.contains(call)
        })
        .expect("run() 里必须调用 limit_malloc_arenas：这是启动时限制 glibc 分配区的唯一入口");
    let application = body
        .lines()
        .position(|line| line.contains("Application::new()"))
        .expect("run() 里应创建 Application");
    assert!(
        call_line < application,
        "limit_malloc_arenas 必须在创建 Application、初始化 GPUI 之前调用；实际情况是调用在第 {call_line} 行、Application::new 在第 {application} 行（相对 run 函数体开头数）"
    );
}
