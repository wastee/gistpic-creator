//! 排版算法单元测试：目标比例贴近程度、横竖排、不裁剪、顺序保持。

use gistpic_creator::imaging::layout::arrange;
use gistpic_creator::model::LayoutTarget;

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

/// 横排按每张图自己的原始尺寸摆，不再拉成一样高。
#[test]
fn row_layout_keeps_each_image_at_its_own_size() {
    let sizes = [LANDSCAPE, (1000, 1000)];
    let result = arrange(&sizes, LayoutTarget::Row, 0);

    // 每张图的框就是它自己的宽高，长宽比不同的图不再被撑大。
    assert_eq!((result.placements[0].w, result.placements[0].h), LANDSCAPE);
    assert_eq!((result.placements[1].w, result.placements[1].h), (1000, 1000));
    // 横坐标依次累加，纵坐标一律 0（按顶边对齐）。
    assert_eq!(result.placements[0].x, 0);
    assert_eq!(result.placements[1].x, 1920);
    assert_eq!(result.placements[0].y, 0);
    assert_eq!(result.placements[1].y, 0);
    // 画布宽是各图宽度之和，画布高是最高的那张。
    assert_eq!(result.canvas_w, 1920 + 1000);
    assert_eq!(result.canvas_h, 1080, "画布高取最高的那张图");
}

/// 竖排同样按每张图自己的原始尺寸摆，不再拉成一样宽。
#[test]
fn column_layout_keeps_each_image_at_its_own_size() {
    let sizes = [LANDSCAPE, (1000, 1000)];
    let result = arrange(&sizes, LayoutTarget::Column, 0);

    assert_eq!((result.placements[0].w, result.placements[0].h), LANDSCAPE);
    assert_eq!((result.placements[1].w, result.placements[1].h), (1000, 1000));
    // 纵坐标依次累加，横坐标一律 0（按左边对齐）。
    assert_eq!(result.placements[0].y, 0);
    assert_eq!(result.placements[1].y, 1080);
    assert_eq!(result.placements[0].x, 0);
    assert_eq!(result.placements[1].x, 0);
    // 画布高是各图高度之和，画布宽是最宽的那张。
    assert_eq!(result.canvas_h, 1080 + 1000);
    assert_eq!(result.canvas_w, 1920, "画布宽取最宽的那张图");
}

/// 用户第 7 条：同一批图，横排和竖排给出的每张图像素尺寸必须完全相同。
///
/// 这是这次改造要保证的核心。以前横排把所有图拉成统一高度、竖排拉成统一宽度，
/// 长宽比不同的图在两种排法下会被撑成不同大小（1000×1000 的方图横排 1080×1080、
/// 竖排 1920×1920）。现在两种排法都按原始尺寸摆，所以逐张比对必须一模一样。
#[test]
fn row_and_column_give_every_image_the_same_size() {
    let sizes = [LANDSCAPE, (1000, 1000), PORTRAIT, (800, 600)];
    for gap in [0u32, 16] {
        let row = arrange(&sizes, LayoutTarget::Row, gap);
        let column = arrange(&sizes, LayoutTarget::Column, gap);

        assert_eq!(row.placements.len(), sizes.len());
        assert_eq!(column.placements.len(), sizes.len());
        for (index, source) in sizes.iter().enumerate() {
            let r = &row.placements[index];
            let c = &column.placements[index];
            assert_eq!(r.item, index);
            assert_eq!(c.item, index);
            assert_eq!(
                (r.w, r.h),
                (c.w, c.h),
                "缝隙 {gap} 时第 {} 张在横排竖排下大小应相同：横排 {}×{}，竖排 {}×{}",
                index + 1,
                r.w,
                r.h,
                c.w,
                c.h
            );
            // 顺带钉住尺寸确实来自原图，没被任何一边撑大。
            assert_eq!(
                (r.w, r.h),
                (source.0.max(1), source.1.max(1)),
                "第 {} 张应按原图尺寸摆放",
                index + 1
            );
        }

        // 两种排法的画布外框不同（一个横长一个竖长），但里面每张图一样大。
        assert_eq!(row.canvas_w, sizes.iter().map(|s| s.0).sum::<u32>() + gap * 3);
        assert_eq!(row.canvas_h, sizes.iter().map(|s| s.1).max().unwrap());
        assert_eq!(column.canvas_h, sizes.iter().map(|s| s.1).sum::<u32>() + gap * 3);
        assert_eq!(column.canvas_w, sizes.iter().map(|s| s.0).max().unwrap());
    }
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

/// 需求 12：100 张图的排版不能失控（分区枚举优化前是 O(n⁴) + 十几万次分配）。
/// 这里只断言结果正确且能瞬间完成；一旦算法退化，测试会明显变慢甚至超时。
#[test]
fn arrange_handles_100_items_within_budget() {
    let sizes: Vec<(u32, u32)> = (0..100)
        .map(|i| if i % 3 == 0 { PORTRAIT } else { LANDSCAPE })
        .collect();
    let result = arrange(&sizes, LayoutTarget::Aspect(16.0 / 9.0), 8);

    assert_eq!(result.placements.len(), 100, "每张图都应被摆放");
    assert!(result.canvas_w > 0 && result.canvas_h > 0);

    // 顺序保持：placements 按下标排好。
    for (index, placement) in result.placements.iter().enumerate() {
        assert_eq!(placement.item, index, "placements 应按下标排序");
        assert!(placement.w > 0 && placement.h > 0);
        assert!(placement.x + placement.w <= result.canvas_w);
        assert!(placement.y + placement.h <= result.canvas_h);
    }
}
