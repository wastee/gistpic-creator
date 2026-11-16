//! 字幕后处理（合并、过滤、分组、打分）的单元测试，全部使用合成坐标，不依赖模型。

use gistpic_creator::imaging::classify::{
    clamp_line, group_lines, merge_duplicate_lines, pick_subtitle, score_block,
};
use gistpic_creator::model::{LineBox, SubtitleBlock};

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
        line(2945.0, 15.0, 3331.0, 63.0),  // 弹幕
        line(1662.0, 99.0, 2192.0, 197.0), // 字幕
    ];
    let block = pick_subtitle(lines, W, H).expect("应找到字幕");
    assert!((block.lines[0].y0 - 99.0).abs() < 1.0);
}

#[test]
fn rejects_mid_frame_false_positive() {
    // 来自 不同位置/字幕在下方 (4)：画面中部有一块高 115 的误检，底部是真正的高 97 字幕
    let lines = vec![
        line(871.0, 1600.0, 1500.0, 1714.0),  // 中部误检，偏左
        line(1707.0, 1957.0, 2129.0, 2054.0), // 底部字幕，居中
    ];
    let block = pick_subtitle(lines, W, H).expect("应找到字幕");
    assert!(
        (block.lines[0].y0 - 1957.0).abs() < 1.0,
        "应选中底部字幕，而不是中部误检"
    );
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
fn does_not_group_a_caption_above_the_subtitle() {
    // 来自 问题B示例/粘贴的图像 (5)：上方是位置偏左的说明条，下方才是真字幕。
    // 旧的水平重叠比（414/952≈0.43）过线，把说明条并成了字幕第二行。
    let lines = vec![
        line(681.0, 1641.0, 1863.0, 1760.0),  // 说明条
        line(1449.0, 1792.0, 2401.0, 1950.0), // 真字幕
    ];
    let blocks = group_lines(&lines);
    assert_eq!(blocks.len(), 2, "说明条不应并进字幕: {blocks:?}");
    assert_eq!(blocks[0].lines.len(), 1);
    assert_eq!(blocks[1].lines.len(), 1);
    assert!((blocks[1].lines[0].y0 - 1792.0).abs() < 1.0);

    // 问题B示例/粘贴的图像 (6)：同样的结构，两条都宽 1183，中线差 654。
    let lines = vec![
        line(680.0, 1640.0, 1863.0, 1759.0),  // 说明条
        line(1334.0, 1802.0, 2517.0, 1953.0), // 真字幕
    ];
    let blocks = group_lines(&lines);
    assert_eq!(blocks.len(), 2, "说明条不应并进字幕: {blocks:?}");
    assert_eq!(blocks[0].lines.len(), 1);
    assert_eq!(blocks[1].lines.len(), 1);
    assert!((blocks[1].lines[0].y0 - 1802.0).abs() < 1.0);
}

#[test]
fn groups_lines_with_a_small_horizontal_offset() {
    // 正例：两行左右只错开较窄行宽的 0.2，仍应归为同一个字幕块。
    let lines = vec![
        line(1000.0, 1700.0, 2000.0, 1800.0),
        line(1200.0, 1840.0, 2200.0, 1940.0),
    ];
    let blocks = group_lines(&lines);
    assert_eq!(blocks.len(), 1, "小幅左右错开的两行应归为一块: {blocks:?}");
    assert_eq!(blocks[0].lines.len(), 2);
}

#[test]
fn ignores_small_text() {
    // 弹幕高度 48（2.2%）低于 3% 门槛，全部应被丢弃
    let lines = vec![
        line(200.0, 20.0, 700.0, 68.0),
        line(1500.0, 20.0, 2100.0, 68.0),
    ];
    assert!(pick_subtitle(lines, W, H).is_none());
}

#[test]
fn lower_half_preference_breaks_close_scores() {
    // 上面一块略宽的候选，下面一块略窄但位于下半。下半偏好应让下面这块胜出。
    let lines = vec![
        line(420.0, 200.0, 3420.0, 500.0),
        line(420.0, 1600.0, 3420.0, 1900.0),
    ];
    let block = pick_subtitle(lines, W, H).expect("应找到字幕");
    assert!(
        (block.lines[0].y0 - 1600.0).abs() < 1.0,
        "下半候选应优先，实际 y0={}",
        block.lines[0].y0
    );
}

#[test]
fn clearly_stronger_top_candidate_still_wins() {
    // 上面的文字明显更大、更宽，下半只有一小块时，不能因为在下半就硬选小字。
    let lines = vec![
        line(200.0, 200.0, 3600.0, 500.0),
        line(1500.0, 1700.0, 2500.0, 1900.0),
    ];
    let block = pick_subtitle(lines, W, H).expect("应找到字幕");
    assert!(
        (block.lines[0].y0 - 200.0).abs() < 1.0,
        "明显更强的顶部候选仍应胜出，实际 y0={}",
        block.lines[0].y0
    );
}

/// 用户实测的失败样本：4 张连拍视频帧里，有一帧顶部弹幕字号偏大，
/// 旧打分把顶部弹幕块当成字幕，真正字幕在底部却被丢掉。
///
/// 坐标取自 3836×2160 原图的实测检测框：顶部 3 行弹幕行高 70~76 像素、
/// 横向铺满 97%、上边距 14 像素；底部真字幕行高 208 像素、宽 45%。
/// 下面还带着左下方一块 2 行的误检，保证候选里不止两项。
#[test]
fn prefers_bottom_subtitle_over_large_top_danmaku() {
    const IMAGE_WIDTH: f32 = 3836.0;
    const IMAGE_HEIGHT: f32 = 2160.0;
    let lines = vec![
        line(88.0, 14.0, 3567.42, 77.41),
        line(145.61, 70.22, 3783.53, 140.02),
        line(65.7, 135.06, 3755.7, 211.35),
        line(1023.0, 1775.0, 2770.0, 1982.48),
        line(583.0, 1873.0, 780.0, 1966.0),
        line(209.0, 1990.0, 829.0, 2136.0),
    ];
    let block = pick_subtitle(lines, IMAGE_WIDTH, IMAGE_HEIGHT).expect("应找到字幕");
    assert!(
        (block.lines[0].y0 - 1775.0).abs() < 1.0,
        "应选中底部字幕而不是顶部弹幕，实际 y0={}",
        block.lines[0].y0
    );
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

#[test]
fn clamps_text_box_to_image_bounds() {
    let clamped = clamp_line(line(991.0, 1790.0, 2843.0, 1955.0), W, 1950.0).expect("字幕框应保留");
    assert_eq!(clamped.y1, 1950.0, "超出图片底边的部分应被夹紧");
    assert_eq!(clamped.y0, 1790.0);
    assert_eq!(clamped.x0, 991.0);
    assert_eq!(clamped.x1, 2843.0);
}

#[test]
fn clamps_before_merging_so_bottom_subtitle_is_not_lost() {
    // 问题 A 的实测框：两个重复框合并后 y1 会到 1955，图片高度只有 1950。
    let lines = vec![
        line(991.0, 1790.0, 2843.0, 1943.0),
        line(1347.0, 1787.0, 2224.0, 1955.0),
        line(3233.0, 83.0, 3764.0, 191.0), // 右上角水印
    ];
    let block = pick_subtitle(lines, W, 1950.0).expect("底部字幕贴边时仍应识别");
    assert_eq!(block.lines[0].y0, 1787.0);
    assert_eq!(block.lines[0].y1, 1950.0);

    let block = pick_subtitle(
        vec![
            line(991.0, 1790.0, 2843.0, 1943.0),
            line(1347.0, 1787.0, 2224.0, 1955.0),
            line(3233.0, 83.0, 3764.0, 191.0),
        ],
        W,
        1950.0,
    )
    .expect("自动模式应找到底部字幕");
    assert!(
        block.lines[0].y0 > 1700.0,
        "自动模式不应选中顶部水印，实际 y0={}",
        block.lines[0].y0
    );
}

#[test]
fn clamps_top_box_to_image_top() {
    let clamped = clamp_line(line(1000.0, -20.0, 2800.0, 300.0), W, H).expect("字幕框应保留");
    assert_eq!(clamped.y0, 0.0, "超出图片顶边的部分应被夹紧");
    assert_eq!(clamped.y1, 300.0);
}
