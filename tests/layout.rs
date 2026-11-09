//! 排版算法单元测试：目标比例贴近程度、横竖排、不裁剪、顺序保持。

use video_screenshot::imaging::layout::arrange;
use video_screenshot::model::LayoutTarget;

const LANDSCAPE: (u32, u32) = (1920, 1080);
const PORTRAIT: (u32, u32) = (1080, 1920);

fn aspect_close(actual_w: u32, actual_h: u32, source: (u32, u32)) -> bool {
    let actual = actual_w as f32 / actual_h.max(1) as f32;
    let expected = source.0 as f32 / source.1.max(1) as f32;
    (actual - expected).abs() / expected < 0.02
}

#[test]
fn three_images_two_landscape_one_portrait_near_16_9() {
    let sizes = [LANDSCAPE, LANDSCAPE, PORTRAIT];
    let result = arrange(&sizes, LayoutTarget::Aspect(16.0 / 9.0), 0);

    assert_eq!(result.placements.len(), 3);
    assert!(
        (result.aspect() - 16.0 / 9.0).abs() < 0.35,
        "三张图排版后比例 {:.3} 应接近 16:9",
        result.aspect()
    );

    // 两张横图在同一列，竖图单独一列
    let landscape_x = result.placements[0].x;
    assert_eq!(result.placements[1].x, landscape_x, "前两张横图应在同一列");
    assert_ne!(result.placements[2].x, landscape_x, "竖图应在另一列");
    assert_eq!(result.placements[2].y, 0, "竖图应从列顶开始");

    for placement in &result.placements {
        let source = sizes[placement.item];
        assert!(
            aspect_close(placement.w, placement.h, source),
            "第 {} 张被拉伸了：{}x{}，原图 {:?}",
            placement.item,
            placement.w,
            placement.h,
            source
        );
    }
}

#[test]
fn four_landscape_images_hit_16_9() {
    let sizes = [LANDSCAPE; 4];
    let result = arrange(&sizes, LayoutTarget::Aspect(16.0 / 9.0), 0);

    assert_eq!(result.placements.len(), 4);
    assert!(
        (result.aspect() - 16.0 / 9.0).abs() < 0.05,
        "四张横图应正好排成 16:9，实际 {:.3}",
        result.aspect()
    );

    // 两列，每列两张
    let mut columns: Vec<u32> = result.placements.iter().map(|p| p.x).collect();
    columns.sort_unstable();
    columns.dedup();
    assert_eq!(columns.len(), 2, "四张横图应排成两列");
}

#[test]
fn three_portrait_images_pick_closest_columns() {
    // 三张 9:16 的竖图无法在不裁剪的前提下凑成 9:16 画布：
    // 一列是 0.1875，两列是 0.84375。两列离目标更近，应该选两列。
    let sizes = [PORTRAIT, PORTRAIT, PORTRAIT];
    let result = arrange(&sizes, LayoutTarget::Aspect(9.0 / 16.0), 0);
    assert!(
        (result.aspect() - 0.84375).abs() < 0.03,
        "三张竖图应选两列，实际比例 {:.3}",
        result.aspect()
    );
    let mut columns: Vec<u32> = result.placements.iter().map(|p| p.x).collect();
    columns.sort_unstable();
    columns.dedup();
    assert_eq!(columns.len(), 2);
}

#[test]
fn row_layout_uses_uniform_height() {
    let sizes = [LANDSCAPE, (1000, 1000)];
    let result = arrange(&sizes, LayoutTarget::Row, 0);

    let height = result.canvas_h;
    assert_eq!(result.placements[0].h, height);
    assert_eq!(result.placements[1].h, height);
    assert_eq!(result.canvas_h, 1080, "横排统一高度取原图最大高度");
    assert_eq!(result.canvas_w, 1920 + 1080);
}

#[test]
fn column_layout_uses_uniform_width() {
    let sizes = [LANDSCAPE, (1000, 1000)];
    let result = arrange(&sizes, LayoutTarget::Column, 0);

    let width = result.canvas_w;
    assert_eq!(result.placements[0].w, width);
    assert_eq!(result.placements[1].w, width);
    assert_eq!(result.canvas_w, 1920, "竖排统一宽度取原图最大宽度");
    assert_eq!(result.canvas_h, 1080 + 1920);
}

#[test]
fn gap_increases_canvas() {
    let sizes = [LANDSCAPE, LANDSCAPE];
    let no_gap = arrange(&sizes, LayoutTarget::Row, 0);
    let with_gap = arrange(&sizes, LayoutTarget::Row, 20);
    assert_eq!(with_gap.canvas_w, no_gap.canvas_w + 20);
}

#[test]
fn preserves_order_and_indices() {
    let sizes = [LANDSCAPE, PORTRAIT, LANDSCAPE];
    let result = arrange(&sizes, LayoutTarget::Aspect(16.0 / 9.0), 0);
    for (position, placement) in result.placements.iter().enumerate() {
        assert_eq!(placement.item, position, "位置计算结果应按原顺序返回");
    }
}

#[test]
fn never_crops_images() {
    let sizes = [LANDSCAPE, PORTRAIT, LANDSCAPE, (800, 600)];
    for target in [
        LayoutTarget::Aspect(16.0 / 9.0),
        LayoutTarget::Aspect(9.0 / 16.0),
        LayoutTarget::Aspect(4.0 / 3.0),
        LayoutTarget::Aspect(1.0),
        LayoutTarget::Row,
        LayoutTarget::Column,
    ] {
        let result = arrange(&sizes, target, 12);
        for placement in &result.placements {
            let source = sizes[placement.item];
            assert!(
                aspect_close(placement.w, placement.h, source),
                "目标 {:?} 下标 {} 的图片比例被改变：{}x{} vs {:?}",
                target,
                placement.item,
                placement.w,
                placement.h,
                source
            );
        }
    }
}

#[test]
fn single_image_keeps_original_size() {
    let sizes = [(1234, 567)];
    let result = arrange(&sizes, LayoutTarget::Aspect(16.0 / 9.0), 10);
    assert_eq!(result.canvas_w, 1234);
    assert_eq!(result.canvas_h, 567);
    assert_eq!(result.placements[0].w, 1234);
    assert_eq!(result.placements[0].h, 567);
}


#[test]
fn aspect_mode_reorders_mixed_images_by_orientation() {
    // 粘贴顺序是 横、竖、横、竖，自动排版按宽高比分列：两张横图叠一列，两张竖图各占一列
    let sizes = [LANDSCAPE, PORTRAIT, LANDSCAPE, PORTRAIT];
    let result = arrange(&sizes, LayoutTarget::Aspect(16.0 / 9.0), 0);

    assert_eq!(result.placements.len(), 4);
    let landscape_x = result.placements[0].x;
    assert_eq!(
        result.placements[2].x, landscape_x,
        "两张横图应该排在同一列"
    );
    assert_ne!(
        result.placements[1].x, landscape_x,
        "竖图不应和横图挤在同一列"
    );
    assert_ne!(
        result.placements[3].x, result.placements[1].x,
        "两张竖图各自占一列，避免把画面拉得太窄"
    );

    // 画布紧贴内容：自动分组后比例约 2.01
    assert!(
        (result.aspect() - 2.014).abs() < 0.03,
        "两横两竖自动分组后的比例应约 2.01，实际 {:.3}",
        result.aspect()
    );
}

#[test]
fn aspect_mode_canvas_hugs_content_without_padding() {
    for sizes in [
        vec![LANDSCAPE, LANDSCAPE, PORTRAIT],
        vec![LANDSCAPE, PORTRAIT, LANDSCAPE, PORTRAIT],
        vec![LANDSCAPE; 4],
        vec![PORTRAIT, PORTRAIT, PORTRAIT],
    ] {
        for target in [
            LayoutTarget::Aspect(16.0 / 9.0),
            LayoutTarget::Aspect(9.0 / 16.0),
            LayoutTarget::Row,
            LayoutTarget::Column,
        ] {
            let result = arrange(&sizes, target, 6);
            assert_eq!(result.placements.len(), sizes.len());
            // 画布左下角贴着内容，右下角也贴着内容：没有额外补边
            let min_x = result.placements.iter().map(|p| p.x).min().unwrap_or(0);
            let min_y = result.placements.iter().map(|p| p.y).min().unwrap_or(0);
            let max_x = result
                .placements
                .iter()
                .map(|p| p.x + p.w)
                .max()
                .unwrap_or(0);
            let max_y = result
                .placements
                .iter()
                .map(|p| p.y + p.h)
                .max()
                .unwrap_or(0);
            assert_eq!(min_x, 0, "目标 {target:?} 左侧有多余空白");
            assert_eq!(min_y, 0, "目标 {target:?} 上方有多余空白");
            assert_eq!(max_x, result.canvas_w, "目标 {target:?} 右侧有多余空白");
            assert_eq!(max_y, result.canvas_h, "目标 {target:?} 下方有多余空白");
        }
    }
}
