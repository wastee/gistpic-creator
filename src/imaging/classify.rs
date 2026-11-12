//! 字幕检测结果的后处理：合并重复行、分组、打分。
//!
//! 这里的函数都是纯函数，不依赖模型和界面，方便单元测试。

use crate::model::{LineBox, SubtitleBlock};

/// 两行竖直重叠超过这个比例就认为是同一行的重复检测结果。
pub const MERGE_VERTICAL_OVERLAP: f32 = 0.5;
/// 合并重复行时，两行在水平方向的最小重叠比例，或最大允许的水平间隙（相对较窄一行的宽度）。
pub const MERGE_HORIZONTAL_OVERLAP: f32 = 0.20;
pub const MERGE_HORIZONTAL_GAP: f32 = 0.50;

/// 行高小于画面高度的这个比例，不认为是字幕（示例里最大弹幕 2.7%，字幕最小 4.5%）。
pub const MIN_LINE_HEIGHT_RATIO: f32 = 0.030;
/// 字幕块宽度小于画面宽度的这个比例，不认为是字幕。
pub const MIN_BLOCK_WIDTH_RATIO: f32 = 0.08;

/// 字幕块中心落在画面下半时得到的偏好分。
///
/// 只用来打破接近的分数：上半画面若有明显更大的文字，仍然可能胜出。
pub const BOTTOM_PREFERENCE_SCORE: f32 = 0.75;

/// 多行字幕的行间距上限（相对行高）。
pub const GROUP_GAP_RATIO: f32 = 0.45;
/// 多行字幕的水平重叠下限。
pub const GROUP_HORIZONTAL_OVERLAP: f32 = 0.30;
/// 同一字幕块内两行的高度比上限。
pub const GROUP_HEIGHT_RATIO_MAX: f32 = 2.0;
/// 同一字幕块内两行左右错开的上限（相对较窄一行的宽度）。
///
/// 只比水平重叠还不够：两行都很宽时，即使左右错开很多，重叠长度除以较窄行宽
/// 仍然可能过线，把字幕上方一条位置偏左或偏右的说明条并成第二行。
pub const GROUP_ALIGN_OFFSET_RATIO: f32 = 0.30;

/// 把检测框夹紧到图片范围内；越界或没有面积返回 `None`。
///
/// 文字检测模型给出的角点可能比图片边界多出几个像素，尤其是贴着画面底边的字幕。
/// 不先夹紧就直接合并和过滤，会把本来有效的字幕块整体判到图片外面。
pub fn clamp_line(line: LineBox, image_width: f32, image_height: f32) -> Option<LineBox> {
    let x0 = line.x0.clamp(0.0, image_width);
    let y0 = line.y0.clamp(0.0, image_height);
    let x1 = line.x1.clamp(0.0, image_width);
    let y1 = line.y1.clamp(0.0, image_height);
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    Some(LineBox { x0, y0, x1, y1 })
}

/// 批量夹紧文字框，丢弃完全落在图片外的框。
pub fn clamp_lines(lines: Vec<LineBox>, image_width: f32, image_height: f32) -> Vec<LineBox> {
    lines
        .into_iter()
        .filter_map(|line| clamp_line(line, image_width, image_height))
        .collect()
}

/// 把同一行被拆成多块的结果合并起来。
pub fn merge_duplicate_lines(lines: Vec<LineBox>) -> Vec<LineBox> {
    let mut kept: Vec<LineBox> = Vec::with_capacity(lines.len());

    for line in lines {
        let mut line = line;
        let mut index = 0;
        while index < kept.len() {
            if should_merge(kept[index], line) {
                line = kept[index].union(line);
                kept.remove(index);
            } else {
                index += 1;
            }
        }
        kept.push(line);
    }

    // 合并之后可能又和别的行重叠，反复合并直到没有变化。
    loop {
        let mut merged_any = false;
        let mut i = 0;
        'outer: while i < kept.len() {
            let mut j = i + 1;
            while j < kept.len() {
                if should_merge(kept[i], kept[j]) {
                    kept[i] = kept[i].union(kept[j]);
                    kept.remove(j);
                    merged_any = true;
                    continue 'outer;
                }
                j += 1;
            }
            i += 1;
        }
        if !merged_any {
            break;
        }
    }

    kept.sort_by(|a, b| a.y0.partial_cmp(&b.y0).unwrap_or(std::cmp::Ordering::Equal));
    kept
}

fn should_merge(a: LineBox, b: LineBox) -> bool {
    if a.vertical_overlap_ratio(b) < MERGE_VERTICAL_OVERLAP {
        return false;
    }
    // 同一行的不同碎片在水平方向一定靠近；两段并排的无关文字不算。
    let horizontal_overlap = a.horizontal_overlap_ratio(b);
    let gap = (a.x0.max(b.x0) - a.x1.min(b.x1)).max(0.0);
    let narrower = a.width().min(b.width());
    horizontal_overlap >= MERGE_HORIZONTAL_OVERLAP || gap <= narrower * MERGE_HORIZONTAL_GAP
}

/// 把检测到的文字行整理成所有候选字幕块。
///
/// 这个函数只做夹紧、去重和分组，不按尺寸过滤，供训练数据导出使用。
pub fn candidate_blocks(
    lines: Vec<LineBox>,
    image_width: f32,
    image_height: f32,
) -> Vec<SubtitleBlock> {
    let clamped = clamp_lines(lines, image_width, image_height);
    let merged = merge_duplicate_lines(clamped);
    group_lines(&merged)
}

/// 把相邻的行归成字幕块（多行字幕）。
pub fn group_lines(lines: &[LineBox]) -> Vec<SubtitleBlock> {
    let mut sorted: Vec<LineBox> = lines.to_vec();
    sorted.sort_by(|a, b| a.y0.partial_cmp(&b.y0).unwrap_or(std::cmp::Ordering::Equal));

    let mut blocks: Vec<SubtitleBlock> = Vec::new();
    for line in sorted {
        let mut placed = false;
        for block in blocks.iter_mut().rev() {
            let last = match block.lines.last() {
                Some(last) => *last,
                None => continue,
            };
            if !can_be_next_line(last, line) {
                continue;
            }
            block.lines.push(line);
            placed = true;
            break;
        }
        if !placed {
            blocks.push(SubtitleBlock { lines: vec![line] });
        }
    }

    for block in &mut blocks {
        block
            .lines
            .sort_by(|a, b| a.y0.partial_cmp(&b.y0).unwrap_or(std::cmp::Ordering::Equal));
    }
    blocks
}

/// 偏左偏右的程度：取中线差、左边差、右边差里最小的那个，再除以较窄一行的宽度。
///
/// 取最小值是为了只惩罚整体错开：两行宽度差很多、但有一侧对齐时，不算错开。
fn align_offset_ratio(upper: LineBox, lower: LineBox) -> f32 {
    let narrower = upper.width().min(lower.width()).max(1.0);
    let offset = (upper.center_x() - lower.center_x())
        .abs()
        .min((upper.x0 - lower.x0).abs())
        .min((upper.x1 - lower.x1).abs());
    offset / narrower
}

fn can_be_next_line(upper: LineBox, lower: LineBox) -> bool {
    let gap = lower.y0 - upper.y1;
    let max_height = upper.height().max(lower.height());
    if gap < -upper.height() * 0.2 {
        return false;
    }
    if gap > GROUP_GAP_RATIO * max_height {
        return false;
    }
    if upper.horizontal_overlap_ratio(lower) < GROUP_HORIZONTAL_OVERLAP {
        return false;
    }
    if align_offset_ratio(upper, lower) > GROUP_ALIGN_OFFSET_RATIO {
        return false;
    }
    let taller = upper.height().max(lower.height());
    let shorter = upper.height().min(lower.height());
    taller / shorter <= GROUP_HEIGHT_RATIO_MAX
}

/// 给字幕块打分，分越高越像字幕。
pub fn score_block(block: &SubtitleBlock, image_width: f32, image_height: f32) -> f32 {
    let bounds = block.bounds();
    let height_ratio = (block.max_line_height() / image_height).min(1.0);

    let half_width = (image_width * 0.5).max(1.0);
    let center_offset = ((bounds.center_x() - image_width * 0.5).abs() / half_width).min(1.0);
    let center_score = 1.0 - center_offset;

    let width_ratio = (bounds.width() / image_width).min(1.0);

    let edge_distance = bounds.y0.min(image_height - bounds.y1).max(0.0) / image_height;
    let edge_score = (1.0 - edge_distance / 0.30).clamp(0.0, 1.0);
    let bottom_score = if bounds.center_y() >= image_height * 0.5 {
        BOTTOM_PREFERENCE_SCORE
    } else {
        0.0
    };

    2.0 * height_ratio + 4.0 * center_score + width_ratio + 2.0 * edge_score + bottom_score
}

/// 从所有检测到的行里挑出最像字幕的一块。
///
/// 整张图都参与候选，下半画面只提高评分，不硬性排除上半画面。
pub fn pick_subtitle(
    lines: Vec<LineBox>,
    image_width: f32,
    image_height: f32,
) -> Option<SubtitleBlock> {
    // 先夹紧到图片范围，再合并和过滤。检测框越界几个像素是常见情况，
    // 必须在合并前处理，否则 union 会把越界带到整个字幕块上。
    let clamped = clamp_lines(lines, image_width, image_height);
    let merged = merge_duplicate_lines(clamped);
    if merged.is_empty() {
        return None;
    }

    let blocks = group_lines(&merged);
    let mut best: Option<(f32, SubtitleBlock)> = None;
    for block in blocks {
        if block.max_line_height() < image_height * MIN_LINE_HEIGHT_RATIO {
            continue;
        }
        if block.bounds().width() < image_width * MIN_BLOCK_WIDTH_RATIO {
            continue;
        }
        let score = score_block(&block, image_width, image_height);
        let better = match &best {
            Some((best_score, _)) => score > *best_score,
            None => true,
        };
        if better {
            best = Some((score, block));
        }
    }

    best.map(|(_, block)| block)
}
