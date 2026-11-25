//! 字幕检测结果的后处理：合并重复行、分组、打分。
//!
//! 这里的函数都是纯函数，不依赖模型和界面，方便单元测试。

use crate::model::{LineBox, SubtitleBlock};

/// 两行竖直重叠超过这个比例就认为是同一行的重复检测结果。
pub const MERGE_VERTICAL_OVERLAP: f32 = 0.5;
/// 合并重复行时，两行在水平方向的最小重叠比例，或最大允许的水平间隙（相对较窄一行的宽度）。
pub const MERGE_HORIZONTAL_OVERLAP: f32 = 0.20;
pub const MERGE_HORIZONTAL_GAP: f32 = 0.50;

/// 行高小于画面高度的这个比例，不认为是字幕。
///
/// 实测样本里弹幕行高最大约 3.5%，字幕行高最小约 4.5%；这条只是粗筛，
/// 真正区分弹幕和字幕靠下面的打分。
pub const MIN_LINE_HEIGHT_RATIO: f32 = 0.030;
/// 字幕块宽度小于画面宽度的这个比例时，只有明显居中才算字幕。
///
/// 宽度粗筛用来丢掉画面角落的说明文字和贴边水印。但字幕长度跟着台词走：
/// 实测 3840 宽的截图里，两个字的字幕（“台独”）只占画面宽度 5.3%，按画面宽度
/// 一刀切会把真字幕删掉，剩下的唯一候选就变成左上角台标。字幕按约定水平居中，
/// 所以窄块只要中心离画面中线不超过 [NARROW_BLOCK_CENTER_TOLERANCE]，仍然当候选。
pub const MIN_BLOCK_WIDTH_RATIO: f32 = 0.08;
/// 窄字幕块中心允许偏离画面中线的比例（相对画面宽度）。
///
/// 实测样本里真字幕的中心偏离都在 1% 以内；画面左侧的竖排误检、左上角台标
/// 偏离都在 35% 以上，用 10% 能分开。
pub const NARROW_BLOCK_CENTER_TOLERANCE: f32 = 0.10;

/// 字幕块中心落在画面下半时得到的偏好分。
///
/// 只用来打破接近的分数：上半画面若有明显更大的文字，仍然可能胜出。
pub const BOTTOM_PREFERENCE_SCORE: f32 = 0.75;

/// 行高（也就是字号）的权重。字号是区分字幕和弹幕最可靠的信号：
/// 弹幕要避开画面中心，字号普遍比字幕小很多，所以这一项权重最高。
pub const HEIGHT_SCORE_WEIGHT: f32 = 6.0;
/// 居中程度的权重。字幕基本都水平居中，这一项比位置可靠。
pub const CENTER_SCORE_WEIGHT: f32 = 4.0;
/// 宽度的权重。字幕通常较宽，但多行弹幕并在一起也能铺满整个画面宽度，
/// 所以只作为辅助信号，权重低于字号和居中。
pub const WIDTH_SCORE_WEIGHT: f32 = 1.0;
/// 「贴近画面上下边」的权重。
///
/// 弹幕默认就从画面最上沿往下排，同样贴着顶边。贴边本身不能说明是字幕，
/// 只能说明它不在画面中间，所以这里权重压低，位置偏好主要交给下半画面加分。
pub const EDGE_SCORE_WEIGHT: f32 = 1.0;

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
///
/// 用并查集收口：先按上边缘排序，每块只跟纵向还重叠的后续块比，连边后一次成组。
/// 原来的写法在 `Vec` 中间反复 `remove` 再整体重扫，最坏是三次方；
/// 现在一趟扫描加一次合并，重叠块多的时候也不会退化。
pub fn merge_duplicate_lines(lines: Vec<LineBox>) -> Vec<LineBox> {
    let mut current = lines;
    loop {
        let count = current.len();
        let mut order: Vec<usize> = (0..count).collect();
        order.sort_by(|a, b| {
            current[*a]
                .y0
                .partial_cmp(&current[*b].y0)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let mut parent: Vec<usize> = (0..count).collect();
        let mut merged_any = false;
        for (position, &i) in order.iter().enumerate() {
            for &j in &order[position + 1..] {
                // 按上边缘排好序了，一旦下一块的顶边落到这一块的底边之下，
                // 后面的只会更靠下，不可能再有纵向重叠。
                if current[j].y0 >= current[i].y1 {
                    break;
                }
                if should_merge(current[i], current[j]) && union(&mut parent, i, j) {
                    merged_any = true;
                }
            }
        }
        if !merged_any {
            break;
        }

        // 合并后的框可能又和别的块挨上，收拢成组再跑一轮，直到没有新的合并。
        let mut groups: Vec<Option<LineBox>> = vec![None; count];
        for (index, line) in current.into_iter().enumerate() {
            let root = find(&mut parent, index);
            groups[root] = Some(match groups[root] {
                Some(merged) => merged.union(line),
                None => line,
            });
        }
        current = groups.into_iter().flatten().collect();
    }

    current.sort_by(|a, b| a.y0.partial_cmp(&b.y0).unwrap_or(std::cmp::Ordering::Equal));
    current
}

fn find(parent: &mut [usize], index: usize) -> usize {
    let mut root = index;
    while parent[root] != root {
        root = parent[root];
    }
    let mut cursor = index;
    while parent[cursor] != root {
        let next = parent[cursor];
        parent[cursor] = root;
        cursor = next;
    }
    root
}

/// 合并两个组，返回是否真的合了（同一组返回 `false`）。
fn union(parent: &mut [usize], a: usize, b: usize) -> bool {
    let root_a = find(parent, a);
    let root_b = find(parent, b);
    if root_a == root_b {
        return false;
    }
    parent[root_b] = root_a;
    true
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
///
/// 权重的来历：用户提供的 3836×2160 实拍帧（同一视频的 4 张连拍）里，有一张
/// 顶部弹幕连成 3 行、行高 70~76 像素（画面高度 3.2%~3.5%）、横向铺满 97%、
/// 上边距只有 14 像素；真正的字幕在底部，行高 208 像素（9.6%）、宽 45%。
/// 旧权重是「行高 2、宽度 1、贴边 2、下半画面 0.75」，弹幕那块靠贴顶边和
/// 铺满宽度拿到 6.98 分，真正字幕只有 6.81 分，于是把弹幕当成了字幕。
/// 把字号提到最高权重（6）、贴边降到 1 之后，同一批候选里字幕 6.46 分、
/// 弹幕 6.15 分，真正字幕胜出；「上半画面文字明显更大更宽时仍可能胜出」这条
/// 既有行为也保留（见 tests/classify.rs 的 clearly_stronger_top_candidate_still_wins）。
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

    HEIGHT_SCORE_WEIGHT * height_ratio
        + CENTER_SCORE_WEIGHT * center_score
        + WIDTH_SCORE_WEIGHT * width_ratio
        + EDGE_SCORE_WEIGHT * edge_score
        + bottom_score
}

/// 宽度是否够格当字幕块。
///
/// 够宽直接算合格；不够宽时只放行水平居中的窄块，用来接住“台独”这种两个字
/// 的短字幕，同时把画面角落的窄条文字挡在外面。
fn plausible_subtitle_width(block: &SubtitleBlock, image_width: f32) -> bool {
    let image_width = image_width.max(1.0);
    let bounds = block.bounds();
    if bounds.width() >= image_width * MIN_BLOCK_WIDTH_RATIO {
        return true;
    }
    let center_offset = (bounds.center_x() - image_width * 0.5).abs() / image_width;
    center_offset <= NARROW_BLOCK_CENTER_TOLERANCE
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
        if !plausible_subtitle_width(&block, image_width) {
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
