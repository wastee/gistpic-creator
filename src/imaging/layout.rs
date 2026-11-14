//! 排版：把若干张图片按目标比例或横排/竖排摆放，不裁剪任何图片。

use crate::model::LayoutTarget;

/// 一张图片在画布里的位置和大小。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Placement {
    pub item: usize,
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// 排版结果：画布大小 + 每张图的位置。
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LayoutResult {
    pub canvas_w: u32,
    pub canvas_h: u32,
    pub placements: Vec<Placement>,
}

impl LayoutResult {
    pub fn aspect(&self) -> f32 {
        if self.canvas_h == 0 {
            return 1.0;
        }
        self.canvas_w as f32 / self.canvas_h as f32
    }
}

/// 计算排版。
///
/// - `sizes`：每张图的原始宽高，顺序就是当前排列顺序
/// - `target`：目标比例、横排或竖排。比例模式会按图片宽高自动分组，不要求按粘贴顺序
/// - `gap`：图片之间、列与列之间的缝隙像素
pub fn arrange(sizes: &[(u32, u32)], target: LayoutTarget, gap: u32) -> LayoutResult {
    match target {
        LayoutTarget::Row => arrange_row(sizes, gap),
        LayoutTarget::Column => arrange_column(sizes, gap),
        LayoutTarget::Aspect(ratio) => arrange_aspect(sizes, ratio, gap),
    }
}

fn clamp_dim(value: f64) -> u32 {
    if !value.is_finite() {
        return 1;
    }
    (value.round() as i64).clamp(1, i32::MAX as i64) as u32
}

fn aspect_of(size: (u32, u32)) -> f64 {
    let (w, h) = size;
    (w.max(1) as f64) / (h.max(1) as f64)
}

fn arrange_single(sizes: &[(u32, u32)]) -> LayoutResult {
    let (w, h) = sizes[0];
    LayoutResult {
        canvas_w: w.max(1),
        canvas_h: h.max(1),
        placements: vec![Placement {
            item: 0,
            x: 0,
            y: 0,
            w: w.max(1),
            h: h.max(1),
        }],
    }
}

/// 横向一排：所有图统一高度，高度取现有最高的那张，其它图按原比例缩放。
///
/// 横坐标依次累加，纵坐标一律 0，画布上下边缘都贴着图片，不留空白。
/// 高度不够的图放大到同高，比最高的那张还高的不存在，所以不会有图被缩小。
fn arrange_row(sizes: &[(u32, u32)], gap: u32) -> LayoutResult {
    if sizes.is_empty() {
        return LayoutResult::default();
    }
    if sizes.len() == 1 {
        return arrange_single(sizes);
    }

    let height = sizes.iter().map(|(_, h)| (*h).max(1)).max().unwrap_or(1);
    let mut placements = Vec::with_capacity(sizes.len());
    let mut x = 0u32;
    for (index, size) in sizes.iter().enumerate() {
        let w = clamp_dim(height as f64 * aspect_of(*size));
        placements.push(Placement {
            item: index,
            x,
            y: 0,
            w,
            h: height,
        });
        x = x.saturating_add(w).saturating_add(gap);
    }
    LayoutResult {
        canvas_w: x.saturating_sub(gap).max(1),
        canvas_h: height,
        placements,
    }
}

/// 竖向一列：所有图统一宽度，宽度取现有最宽的那张，其它图按原比例缩放。
///
/// 纵坐标依次累加，横坐标一律 0，画布左右边缘都贴着图片，不留空白。
/// 宽度不够的图放大到同宽，比最宽的那张还宽的不存在，所以不会有图被缩小。
fn arrange_column(sizes: &[(u32, u32)], gap: u32) -> LayoutResult {
    if sizes.is_empty() {
        return LayoutResult::default();
    }
    if sizes.len() == 1 {
        return arrange_single(sizes);
    }

    let width = sizes.iter().map(|(w, _)| (*w).max(1)).max().unwrap_or(1);
    let mut placements = Vec::with_capacity(sizes.len());
    let mut y = 0u32;
    for (index, size) in sizes.iter().enumerate() {
        let h = clamp_dim(width as f64 / aspect_of(*size));
        placements.push(Placement {
            item: index,
            x: 0,
            y,
            w: width,
            h,
        });
        y = y.saturating_add(h).saturating_add(gap);
    }
    LayoutResult {
        canvas_w: width,
        canvas_h: y.saturating_sub(gap).max(1),
        placements,
    }
}

/// 按目标比例自动排版：先按宽高比把图片分组，再把每组排成一列、每列等高，
/// 挑最接近目标比例的一种切法。
///
/// 分组前会按宽高比排序，所以不需要按粘贴顺序：横图和横图会自动排到一起，
/// 竖图和竖图排到一起。列内、列间的位置仍然参考当前图片顺序，拖动图片可以调整上下、左右次序。
fn arrange_aspect(sizes: &[(u32, u32)], ratio: f32, gap: u32) -> LayoutResult {
    if sizes.is_empty() {
        return LayoutResult::default();
    }
    if sizes.len() == 1 {
        return arrange_single(sizes);
    }

    let aspects: Vec<f64> = sizes.iter().map(|size| aspect_of(*size)).collect();
    let target = (ratio.max(1e-3)) as f64;

    // 按宽高比从横到竖排序，宽高比相同的保持原来的先后顺序。
    let mut order: Vec<usize> = (0..sizes.len()).collect();
    order.sort_by(|&a, &b| {
        aspects[b]
            .partial_cmp(&aspects[a])
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.cmp(&b))
    });
    let sorted_aspects: Vec<f64> = order.iter().map(|&index| aspects[index]).collect();

    let n = sizes.len();
    let max_columns = n.min(4);
    // 前缀和：单次评价里每列的和变成 O(1)，整体只与列数 k（≤4）有关。
    // 原来每个切法都要把整列重扫一遍，配合 ΣC(n-1,k) 个切法整体是 O(n⁴)。
    let mut prefix_inv: Vec<f64> = Vec::with_capacity(n + 1);
    prefix_inv.push(0.0);
    for aspect in &sorted_aspects {
        let next = prefix_inv.last().copied().unwrap_or(0.0) + 1.0 / aspect.max(1e-9);
        prefix_inv.push(next);
    }

    let mut best: Option<(f64, Vec<usize>)> = None;
    let mut evaluated = 0usize;
    for columns in 1..=max_columns {
        for_each_partition(n, columns, &mut |cuts| {
            evaluated += 1;
            if evaluated > PARTITION_BUDGET {
                return false;
            }
            let aspect = canvas_aspect(&prefix_inv, cuts, n);
            if !aspect.is_finite() || aspect <= 0.0 {
                return true;
            }
            let error = (aspect / target).ln().abs();
            // 同一列里混入横竖差别很大的图片时，列会又窄又怪，这种切法加一点惩罚。
            let score = error + column_spread(&sorted_aspects, cuts, n) * 0.6;
            let better = match &best {
                Some((best_score, best_cuts)) => {
                    if (score - best_score).abs() < 1e-9 {
                        cuts.len() < best_cuts.len()
                    } else {
                        score < *best_score
                    }
                }
                None => true,
            };
            if better {
                best = Some((score, cuts.to_vec()));
            }
            true
        });
        if evaluated > PARTITION_BUDGET {
            break;
        }
    }

    let cuts = match best {
        Some((_, cuts)) => cuts,
        None => return arrange_column(sizes, gap),
    };

    // 把切法展开成列（元素是原始下标）。
    let mut columns: Vec<Vec<usize>> = Vec::new();
    let mut start = 0usize;
    for cut in cuts.iter().copied().chain(std::iter::once(sizes.len())) {
        if cut > start {
            columns.push(order[start..cut].to_vec());
        }
        start = cut;
    }
    if columns.is_empty() {
        return arrange_column(sizes, gap);
    }
    // 列内按原来的先后顺序排，列间按每列最靠前的图片排，这样拖动图片仍然能调整上下、左右位置。
    for column in &mut columns {
        column.sort_unstable();
    }
    columns.sort_by_key(|column| column.first().copied().unwrap_or(usize::MAX));

    // 每列的相对宽度：让所有列的相对高度都等于 1。
    let mut column_rel_width: Vec<f64> = Vec::with_capacity(columns.len());
    for column in &columns {
        let sum: f64 = column.iter().map(|&i| 1.0 / aspects[i]).sum();
        column_rel_width.push(1.0 / sum.max(1e-9));
    }

    let mut total_rel_area = 0.0f64;
    for (column, rel_width) in columns.iter().zip(column_rel_width.iter()) {
        for &i in column {
            let rel_height = rel_width / aspects[i];
            total_rel_area += rel_width * rel_height;
        }
    }
    let source_area: f64 = sizes
        .iter()
        .map(|(w, h)| (*w).max(1) as f64 * (*h).max(1) as f64)
        .sum();
    let scale = if total_rel_area > 0.0 {
        (source_area / total_rel_area).sqrt()
    } else {
        1.0
    };

    // 每列的像素宽度、每张图的像素高度。
    let column_px_width: Vec<u32> = column_rel_width
        .iter()
        .map(|rel| clamp_dim(rel * scale))
        .collect();

    let mut column_px_height: Vec<u32> = Vec::with_capacity(columns.len());
    for (column_index, column) in columns.iter().enumerate() {
        let width = column_px_width[column_index];
        let mut height = 0u32;
        for (position, &i) in column.iter().enumerate() {
            let image_h = clamp_dim(width as f64 / aspects[i]);
            height = height.saturating_add(image_h);
            if position + 1 < column.len() {
                height = height.saturating_add(gap);
            }
        }
        column_px_height.push(height);
    }

    let canvas_w = column_px_width
        .iter()
        .copied()
        .fold(0u32, |acc, w| acc.saturating_add(w))
        .saturating_add(gap.saturating_mul(columns.len() as u32 - 1))
        .max(1);
    let canvas_h = column_px_height.iter().copied().max().unwrap_or(1).max(1);

    let mut placements = Vec::with_capacity(sizes.len());
    let mut x = 0u32;
    for (column_index, column) in columns.iter().enumerate() {
        let width = column_px_width[column_index];
        let mut y = 0u32;
        for (position, &item) in column.iter().enumerate() {
            let h = clamp_dim(width as f64 / aspects[item]);
            placements.push(Placement {
                item,
                x,
                y,
                w: width,
                h,
            });
            y = y.saturating_add(h);
            if position + 1 < column.len() {
                y = y.saturating_add(gap);
            }
        }
        x = x.saturating_add(width).saturating_add(gap);
    }

    placements.sort_by_key(|placement| placement.item);

    LayoutResult {
        canvas_w,
        canvas_h,
        placements,
    }
}

/// 评价切法的次数上限：正常图片数量（远小于 200）根本碰不到，
/// 只是防止病态输入让枚举失控。
const PARTITION_BUDGET: usize = 1_000_000;

/// 画布宽高比：每列等高时，总宽 / 总高 = 各列（相对宽度的倒数）之和。
///
/// 用前缀和把每列的和降到 O(1)，单次评价只与列数 k（≤4）有关。
fn canvas_aspect(prefix_inv: &[f64], cuts: &[usize], n: usize) -> f64 {
    let mut total = 0.0;
    let mut start = 0usize;
    for cut in cuts.iter().copied().chain(std::iter::once(n)) {
        if cut <= start {
            continue;
        }
        let sum = prefix_inv[cut] - prefix_inv[start];
        total += 1.0 / sum.max(1e-9);
        start = cut;
    }
    total
}

/// 每一列里图片宽高比的最大差距（取对数后再求平均），用来避免把横图和竖图塞进同一列。
///
/// 宽高比已按降序排好，一段里的最大/最小就是首尾两项，单次评价 O(列数)。
fn column_spread(aspects: &[f64], cuts: &[usize], n: usize) -> f64 {
    let mut total = 0.0;
    let mut columns = 0.0;
    let mut start = 0usize;
    for cut in cuts.iter().copied().chain(std::iter::once(n)) {
        if cut <= start {
            continue;
        }
        let max = aspects[start];
        let min = aspects[cut - 1];
        total += (max / min.max(1e-9)).ln().abs();
        columns += 1.0;
        start = cut;
    }
    if columns > 0.0 { total / columns } else { 0.0 }
}

/// 枚举把 n 张图按顺序切成 columns 段的切点（切点是每段第一个元素的下标）。
///
/// 用回调直接消费每个切法，不再为每种切法分配一个临时 Vec —— n=100、4 列时
/// 原本要分配 ΣC(n-1,k) ≈ 16 万个。回调返回 false 表示提前停止。
fn for_each_partition(n: usize, columns: usize, f: &mut impl FnMut(&[usize]) -> bool) {
    if columns == 0 || columns > n {
        return;
    }
    let mut current = Vec::with_capacity(columns.saturating_sub(1));
    recurse_partition(1, columns, n, &mut current, f);
}

fn recurse_partition(
    start: usize,
    remaining: usize,
    n: usize,
    current: &mut Vec<usize>,
    f: &mut impl FnMut(&[usize]) -> bool,
) -> bool {
    if remaining == 1 {
        return f(current);
    }
    for cut in start..n {
        if n - cut < remaining - 1 {
            break;
        }
        current.push(cut);
        let keep_going = recurse_partition(cut + 1, remaining - 1, n, current, f);
        current.pop();
        if !keep_going {
            return false;
        }
    }
    true
}
