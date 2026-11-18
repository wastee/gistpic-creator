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

/// 横排统一高度：高度取现有最高的那张，其它图按原比例放大到同高。
///
/// 1920×1080 和 1000×1000 两张：高度都是 1080，方图放大成 1080×1080，
/// 横图保持 1920×1080，画布 3000×1080，上下都不留空白。
#[test]
fn row_layout_scales_images_to_a_shared_height() {
    let sizes = [LANDSCAPE, (1000, 1000)];
    let result = arrange(&sizes, LayoutTarget::Row, 0);

    assert_eq!((result.placements[0].w, result.placements[0].h), LANDSCAPE);
    assert_eq!(
        (result.placements[1].w, result.placements[1].h),
        (1080, 1080),
        "方图应放大到和横图一样高"
    );
    // 横坐标依次累加，纵坐标一律 0（按顶边对齐）。
    assert_eq!(result.placements[0].x, 0);
    assert_eq!(result.placements[1].x, 1920);
    assert_eq!(result.placements[0].y, 0);
    assert_eq!(result.placements[1].y, 0);
    // 画布宽是各图宽度之和，画布高是统一后的高度。
    assert_eq!(result.canvas_w, 1920 + 1080);
    assert_eq!(result.canvas_h, 1080, "画布高取最高的那张图");
}

/// 竖排统一宽度：宽度取现有最宽的那张，其它图按原比例放大到同宽。
///
/// 1920×1080 和 1000×1000 两张：宽度都是 1920，方图放大成 1920×1920，
/// 横图保持 1920×1080，画布 1920×3000，左右都不留空白。
#[test]
fn column_layout_scales_images_to_a_shared_width() {
    let sizes = [LANDSCAPE, (1000, 1000)];
    let result = arrange(&sizes, LayoutTarget::Column, 0);

    assert_eq!((result.placements[0].w, result.placements[0].h), LANDSCAPE);
    assert_eq!(
        (result.placements[1].w, result.placements[1].h),
        (1920, 1920),
        "方图应放大到和横图一样宽"
    );
    // 纵坐标依次累加，横坐标一律 0（按左边对齐）。
    assert_eq!(result.placements[0].y, 0);
    assert_eq!(result.placements[1].y, 1080);
    assert_eq!(result.placements[0].x, 0);
    assert_eq!(result.placements[1].x, 0);
    // 画布高是各图高度之和，画布宽是统一后的宽度。
    assert_eq!(result.canvas_h, 1080 + 1920);
    assert_eq!(result.canvas_w, 1920, "画布宽取最宽的那张图");
}

/// 横排每张一样高、竖排每张一样宽，每张图的长宽比不变。
///
/// 同一张图在横排、竖排下尺寸会不同（方图横排 1080×1080、竖排 1920×1920），
/// 原来的「横排竖排每张图像素尺寸完全相同」不再成立，这是这次定下的取舍。
#[test]
fn row_and_column_unify_one_side_without_stretching() {
    let sizes = [LANDSCAPE, (1000, 1000), PORTRAIT, (800, 600)];
    for gap in [0u32, 16] {
        let row = arrange(&sizes, LayoutTarget::Row, gap);
        let column = arrange(&sizes, LayoutTarget::Column, gap);
        let row_height = sizes.iter().map(|(_, h)| *h).max().unwrap();
        let column_width = sizes.iter().map(|(w, _)| *w).max().unwrap();

        assert_eq!(row.placements.len(), sizes.len());
        assert_eq!(column.placements.len(), sizes.len());
        for (index, source) in sizes.iter().enumerate() {
            let r = &row.placements[index];
            let c = &column.placements[index];
            assert_eq!(r.item, index);
            assert_eq!(c.item, index);
            assert_eq!(r.h, row_height, "缝隙 {gap} 时第 {} 张应和最高的那张同高", index + 1);
            assert_eq!(
                c.w, column_width,
                "缝隙 {gap} 时第 {} 张应和最宽的那张同宽",
                index + 1
            );
            assert!(
                aspect_close(r.w, r.h, *source),
                "横排第 {} 张比例变了：{}x{}",
                index + 1,
                r.w,
                r.h
            );
            assert!(
                aspect_close(c.w, c.h, *source),
                "竖排第 {} 张比例变了：{}x{}",
                index + 1,
                c.w,
                c.h
            );
        }

        // 画布贴着图片，没有空白：宽是每张缩放后宽度之和，高是每张缩放后高度之和。
        let expected_row_width: u32 = sizes
            .iter()
            .map(|(w, h)| ((*w as f64) * row_height as f64 / (*h as f64)).round() as u32)
            .sum();
        assert_eq!(row.canvas_w, expected_row_width + gap * 3);
        assert_eq!(row.canvas_h, row_height);
        let expected_column_height: u32 = sizes
            .iter()
            .map(|(w, h)| ((*h as f64) * column_width as f64 / (*w as f64)).round() as u32)
            .sum();
        assert_eq!(column.canvas_h, expected_column_height + gap * 3);
        assert_eq!(column.canvas_w, column_width);
    }

    // 旧规则确实被去掉：同一张方图在两种排法下大小不同。
    // 用 1920×1080 和 1000×1000 两张，最高 1080、最宽 1920，两种排法的统一边不一样。
    let contrast = [LANDSCAPE, (1000, 1000)];
    let row = arrange(&contrast, LayoutTarget::Row, 0);
    let column = arrange(&contrast, LayoutTarget::Column, 0);
    let square_row = (row.placements[1].w, row.placements[1].h);
    let square_column = (column.placements[1].w, column.placements[1].h);
    assert_eq!(square_row, (1080, 1080));
    assert_eq!(square_column, (1920, 1920));
    assert_ne!(square_row, square_column);
}

/// 问题 C 示例：3840×3426 和 1896×352 两张。
///
/// 横排统一到最高 3426：两张都 3426 高，宽度分别是 3840 和 18454，画布 22294×3426。
/// 竖排统一到最宽 3840：两张都 3840 宽，高度分别是 3426 和 713，画布 3840×4139。
#[test]
fn problem_c_example_unifies_the_facing_side() {
    let sizes = [(3840, 3426), (1896, 352)];

    let row = arrange(&sizes, LayoutTarget::Row, 0);
    assert_eq!((row.placements[0].w, row.placements[0].h), (3840, 3426));
    assert_eq!((row.placements[1].w, row.placements[1].h), (18454, 3426));
    assert_eq!((row.canvas_w, row.canvas_h), (22294, 3426));

    let column = arrange(&sizes, LayoutTarget::Column, 0);
    assert_eq!(
        (column.placements[0].w, column.placements[0].h),
        (3840, 3426)
    );
    assert_eq!((column.placements[1].w, column.placements[1].h), (3840, 713));
    assert_eq!((column.canvas_w, column.canvas_h), (3840, 4139));
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

// ----------------------------------------------------------------------
// 循环排版：点击依次换拼法，三张图的顺序由版式目录固定。
// ----------------------------------------------------------------------

use gistpic_creator::imaging::layout::shuffle_pattern_count;

/// 三张同样尺寸的图，前三次点循环排版依次得到用户要求的三种拼法。
#[test]
fn shuffle_cycles_through_three_arrangements_for_three_images() {
    let sizes = [(1000u32, 1000u32); 3];
    let a = arrange(&sizes, LayoutTarget::Shuffle(0), 0);
    let b = arrange(&sizes, LayoutTarget::Shuffle(1), 0);
    let c = arrange(&sizes, LayoutTarget::Shuffle(2), 0);
    assert_eq!(a.placements.len(), 3);
    assert_eq!(b.placements.len(), 3);
    assert_eq!(c.placements.len(), 3);

    // 第 0 种：图片 0 单独在左列，图片 1、2 在右列上下。
    assert!(a.placements[0].x < a.placements[1].x);
    assert_eq!(a.placements[1].x, a.placements[2].x);
    assert_ne!(a.placements[1].y, a.placements[2].y);
    assert_eq!(a.placements[0].y, 0);

    // 第 1 种：图片 0、1 在左列上下，图片 2 在右列。
    assert_eq!(b.placements[0].x, b.placements[1].x);
    assert_ne!(b.placements[0].y, b.placements[1].y);
    assert!(b.placements[2].x > b.placements[0].x);

    // 第 2 种：图片 0 在上方整行，图片 1、2 在下方左右。
    assert!(c.placements[0].y < c.placements[1].y);
    assert_eq!(c.placements[1].y, c.placements[2].y);
    assert_ne!(c.placements[1].x, c.placements[2].x);
    assert_eq!(c.placements[0].x, 0);

    // 三种拼法的坐标确实不同，点一下就会换。
    assert_ne!(a.placements, b.placements);
    assert_ne!(b.placements, c.placements);

    // 每张图的宽高比与原图一致，没有裁剪也没有拉伸。
    for result in [&a, &b, &c] {
        for placement in &result.placements {
            assert!(
                aspect_close(placement.w, placement.h, sizes[placement.item]),
                "循环排版下第 {} 张比例变了：{}x{}",
                placement.item, placement.w, placement.h
            );
        }
    }
    // 画布紧贴内容。
    for result in [&a, &b, &c] {
        let min_x = result.placements.iter().map(|p| p.x).min().unwrap();
        let min_y = result.placements.iter().map(|p| p.y).min().unwrap();
        let max_x = result.placements.iter().map(|p| p.x + p.w).max().unwrap();
        let max_y = result.placements.iter().map(|p| p.y + p.h).max().unwrap();
        assert_eq!(min_x, 0);
        assert_eq!(min_y, 0);
        assert_eq!(max_x, result.canvas_w);
        assert_eq!(max_y, result.canvas_h);
    }
}

/// 循环排版里每张图的显示尺寸，必须和横排或竖排算出来的一致。
/// 不能像面积守恒那样把部分图放大、部分图缩小。
#[test]
fn shuffle_uses_the_same_image_size_as_row_or_column() {
    // 三张尺寸各不相同的图，避免「恰好一样」掩盖尺寸算法错误。
    let sizes = [(1920u32, 1080u32), (1000, 1500), (800, 600)];
    let row = arrange(&sizes, LayoutTarget::Row, 0);
    let column = arrange(&sizes, LayoutTarget::Column, 0);
    let row_size: Vec<(u32, u32)> = row.placements.iter().map(|p| (p.w, p.h)).collect();
    let column_size: Vec<(u32, u32)> = column.placements.iter().map(|p| (p.w, p.h)).collect();

    for index in 0..shuffle_pattern_count(sizes.len()) {
        let shuffled = arrange(&sizes, LayoutTarget::Shuffle(index), 0);
        let mut matched_row = true;
        let mut matched_column = true;
        for placement in &shuffled.placements {
            let size = (placement.w, placement.h);
            if size != row_size[placement.item] {
                matched_row = false;
            }
            if size != column_size[placement.item] {
                matched_column = false;
            }
        }
        assert!(
            matched_row || matched_column,
            "循环排版第 {index} 种里每张图的宽高，应与横排或竖排一致；行版式一致={matched_row}，列版式一致={matched_column}"
        );
    }
}

#[test]
fn shuffle_gap_and_determinism() {
    let sizes = [(1920u32, 1080u32), (1000, 1000), (1080, 1920)];
    let no_gap = arrange(&sizes, LayoutTarget::Shuffle(0), 0);
    let with_gap = arrange(&sizes, LayoutTarget::Shuffle(0), 20);
    assert!(
        with_gap.canvas_w > no_gap.canvas_w && with_gap.canvas_h > no_gap.canvas_h,
        "第 0 种是两列，缝隙应让画布变宽也变高"
    );
    // 同参数重复调用结果完全一样。
    let again = arrange(&sizes, LayoutTarget::Shuffle(0), 20);
    assert_eq!(with_gap, again);
    // 每张图比例仍与原图一致。
    for placement in &with_gap.placements {
        assert!(aspect_close(placement.w, placement.h, sizes[placement.item]));
    }
    // placements 按原图下标排序。
    for (index, placement) in with_gap.placements.iter().enumerate() {
        assert_eq!(placement.item, index);
    }
}

#[test]
fn shuffle_pattern_counts_are_deterministic() {
    assert_eq!(shuffle_pattern_count(0), 1);
    assert_eq!(shuffle_pattern_count(1), 1);
    assert_eq!(shuffle_pattern_count(2), 2);
    assert_eq!(shuffle_pattern_count(3), 4);
    assert_eq!(shuffle_pattern_count(4), 6);
    assert_eq!(shuffle_pattern_count(7), 12);
    assert_eq!(shuffle_pattern_count(100), 12);
}

#[test]
fn shuffle_handles_edge_counts_and_out_of_range_index() {
    // 0、1、2、4、100 张图都不能 panic。
    for n in [0usize, 1, 2, 4, 100] {
        let sizes: Vec<(u32, u32)> = (0..n).map(|i| (1000 + i as u32, 800)).collect();
        for index in [0usize, 1, 3, 100, usize::MAX] {
            let result = arrange(&sizes, LayoutTarget::Shuffle(index), 4);
            assert_eq!(result.placements.len(), n);
            for placement in &result.placements {
                assert!(placement.w > 0 && placement.h > 0);
                assert!(placement.x + placement.w <= result.canvas_w);
                assert!(placement.y + placement.h <= result.canvas_h);
                assert!(aspect_close(placement.w, placement.h, sizes[placement.item]));
            }
        }
    }
    // 超出范围要取模：Shuffle(100) 与 Shuffle(100 % count) 结果一致。
    let sizes = [(1000u32, 1000u32); 3];
    let count = shuffle_pattern_count(3);
    assert_eq!(
        arrange(&sizes, LayoutTarget::Shuffle(100), 0),
        arrange(&sizes, LayoutTarget::Shuffle(100 % count), 0)
    );
}
