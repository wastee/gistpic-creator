//! 字幕后处理（合并、过滤、分组、打分）的单元测试，全部使用合成坐标，不依赖模型。

use video_screenshot::imaging::classify::{
    group_lines, merge_duplicate_lines, pick_subtitle, score_block,
};
use video_screenshot::model::{LineBox, SearchRegion, SubtitleBlock};

const W: f32 = 3840.0;
const H: f32 = 2160.0;

fn line(x0: f32, y0: f32, x1: f32, y1: f32) -> LineBox {
    LineBox { x0, y0, x1, y1 }
}

#[test]
fn merges_fragments_of_the_same_line() {
    // 来自 单行/粘贴的图像 的实测：同一行被拆成多块重叠的框
    let lines = vec![
        line(435.0, 1950.0, 1011.0, 1995.0),
        line(429.0, 1950.0, 3233.0, 2118.0),
        line(1386.0, 1963.0, 1938.0, 2009.0),
        line(432.0, 1988.0, 2934.0, 2045.0),
        line(1048.0, 2021.0, 1095.0, 2050.0),
    ];
    let merged = merge_duplicate_lines(lines);
    assert_eq!(merged.len(), 1, "同一行的碎片应合并成一行: {merged:?}");
    let merged = merged[0];
    assert!((merged.y0 - 1950.0).abs() < 1.0);
    assert!((merged.y1 - 2118.0).abs() < 1.0);
}

#[test]
fn does_not_merge_side_by_side_lines() {
    // 同一高度但水平分开的两段文字，不应合并
    let lines = vec![
        line(100.0, 100.0, 500.0, 160.0),
        line(2000.0, 100.0, 2400.0, 160.0),
    ];
    let merged = merge_duplicate_lines(lines);
    assert_eq!(merged.len(), 2);
}

#[test]
fn picks_subtitle_over_top_danmaku() {
    // 来自 不同位置/字幕在上方 (2)：字幕高 99 居中；弹幕高 48 偏右
    let lines = vec![
        line(2945.0, 15.0, 3331.0, 63.0),   // 弹幕
        line(1662.0, 99.0, 2192.0, 197.0),  // 字幕
    ];
    let block = pick_subtitle(lines, SearchRegion::Auto, W, H).expect("应找到字幕");
    assert!((block.lines[0].y0 - 99.0).abs() < 1.0);
}

#[test]
fn rejects_mid_frame_false_positive() {
    // 来自 不同位置/字幕在下方 (4)：画面中部有一块高 115 的误检，底部是真正的高 97 字幕
    let lines = vec![
        line(871.0, 1600.0, 1500.0, 1714.0),  // 中部误检，偏左
        line(1707.0, 1957.0, 2129.0, 2054.0), // 底部字幕，居中
    ];
    let block = pick_subtitle(lines, SearchRegion::Auto, W, H).expect("应找到字幕");
    assert!((block.lines[0].y0 - 1957.0).abs() < 1.0, "应选中底部字幕，而不是中部误检");
}

#[test]
fn groups_two_line_subtitle() {
    let lines = vec![
        line(1410.0, 1708.0, 2366.0, 1816.0), // 中文行
        line(1206.0, 1857.0, 2636.0, 1956.0), // 英文行
    ];
    let blocks = group_lines(&lines);
    assert_eq!(blocks.len(), 1, "中英两行应归为一个字幕块");
    assert_eq!(blocks[0].lines.len(), 2);
    assert!((blocks[0].lines[0].y0 - 1708.0).abs() < 1.0);
    assert!((blocks[0].lines[1].y0 - 1857.0).abs() < 1.0);
}

#[test]
fn does_not_group_far_apart_lines() {
    let lines = vec![
        line(100.0, 100.0, 900.0, 160.0),
        line(100.0, 600.0, 900.0, 660.0),
    ];
    let blocks = group_lines(&lines);
    assert_eq!(blocks.len(), 2);
}

#[test]
fn ignores_small_text() {
    // 弹幕高度 48（2.2%）低于 3% 门槛，全部应被丢弃
    let lines = vec![
        line(200.0, 20.0, 700.0, 68.0),
        line(1500.0, 20.0, 2100.0, 68.0),
    ];
    assert!(pick_subtitle(lines, SearchRegion::Auto, W, H).is_none());
}

#[test]
fn region_limits_filter() {
    let top_subtitle = vec![line(1662.0, 99.0, 2192.0, 197.0)];
    let bottom_subtitle = vec![line(1707.0, 1957.0, 2129.0, 2054.0)];

    assert!(pick_subtitle(top_subtitle.clone(), SearchRegion::Bottom, W, H).is_none());
    assert!(pick_subtitle(bottom_subtitle.clone(), SearchRegion::Top, W, H).is_none());
    assert!(pick_subtitle(top_subtitle, SearchRegion::Top, W, H).is_some());
    assert!(pick_subtitle(bottom_subtitle, SearchRegion::Bottom, W, H).is_some());
}

#[test]
fn score_prefers_centered_large_text() {
    let subtitle = SubtitleBlock {
        lines: vec![line(1707.0, 1957.0, 2129.0, 2054.0)],
    };
    let side_text = SubtitleBlock {
        lines: vec![line(3000.0, 100.0, 3600.0, 200.0)],
    };
    assert!(
        score_block(&subtitle, W, H) > score_block(&side_text, W, H),
        "居中字幕分数应高于靠边的文字"
    );
}
