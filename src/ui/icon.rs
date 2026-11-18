//! 界面图标。全部用路径画，不依赖图标字体或图片资源。
//!
//! 每个图标在 16×16 的方格内绘制，调用方给多大区域就按比例缩放。

use gpui::prelude::*;
use gpui::{App, Bounds, Hsla, IntoElement, Pixels, Point, Window, canvas, point, px};

use super::theme::Theme;

/// 一个可画的图标。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    /// 撤销：一个向左的弯箭头
    Undo,
    /// 重做：一个向右的弯箭头
    Redo,
    /// 更多：竖着的三个点
    More,
    /// 图钉
    Pin,
    /// 关闭：叉
    Close,
    /// 字幕：一个矩形，下半部分两条横线
    Subtitle,
    /// 排版：四宫格
    Layout,
    /// 缝隙：两个矩形中间夹一条线
    Gap,
    /// 加减号里的加号
    Plus,
    /// 加号里减号
    Minus,
    /// 勾
    Check,
    /// 向下箭头
    ChevronDown,
    /// 右向三角：展开叠放预览
    ChevronRight,
    /// 复制：两张叠在一起的纸
    Copy,
    /// 导出：从托盘里向上的箭头
    Export,
    /// 删除：垃圾桶
    Trash,
    /// 大加号，空状态用
    PlusLarge,
    /// 横向排列：三个竖条左右并排
    ArrangeRow,
    /// 竖向排列：三个横条上下堆叠
    ArrangeColumn,
    /// 循环排版：一个简洁的循环箭头
    ArrangeShuffle,
    /// 全选：四角括号围住中间一块
    SelectAll,
}

/// 在指定区域里画一个图标。
pub fn icon(icon: Icon, size: Pixels, color: Hsla) -> impl IntoElement {
    // 外面套一个定尺寸的容器，canvas 自己撑满它。
    gpui::div().w(size).h(size).child(canvas(
        move |_bounds, _window, _cx| (),
        move |bounds: Bounds<Pixels>, _state, window: &mut Window, _cx: &mut App| {
            paint(icon, bounds, color, window);
        },
    ))
}

/// 把一个图标画在给定区域里，坐标系按 16×16 换算。
fn paint(icon: Icon, bounds: Bounds<Pixels>, color: Hsla, window: &mut Window) {
    let scale = f32::from(bounds.size.width) / 16.0;
    let origin = point(bounds.origin.x, bounds.origin.y);

    // 把 16×16 方格里的坐标换到屏幕坐标。
    let at = |x: f32, y: f32| point(origin.x + px(x * scale), origin.y + px(y * scale));

    let stroke = |width: f32| gpui::PathBuilder::stroke(px(width * scale));

    match icon {
        Icon::Undo => {
            let mut path = stroke(1.5);
            path.move_to(at(6.5, 4.5));
            path.line_to(at(3.0, 8.0));
            path.line_to(at(6.5, 11.5));
            path.move_to(at(3.0, 8.0));
            path.line_to(at(9.5, 8.0));
            path.cubic_bezier_to(at(13.0, 8.0), at(11.8, 8.0), at(13.0, 8.0));
            path.cubic_bezier_to(at(13.0, 12.0), at(13.0, 10.6), at(13.0, 12.0));
            if let Ok(path) = path.build() {
                window.paint_path(path, color);
            }
        }
        Icon::Redo => {
            let mut path = stroke(1.5);
            path.move_to(at(9.5, 4.5));
            path.line_to(at(13.0, 8.0));
            path.line_to(at(9.5, 11.5));
            path.move_to(at(13.0, 8.0));
            path.line_to(at(6.5, 8.0));
            path.cubic_bezier_to(at(3.0, 8.0), at(4.2, 8.0), at(3.0, 8.0));
            path.cubic_bezier_to(at(3.0, 12.0), at(3.0, 10.6), at(3.0, 12.0));
            if let Ok(path) = path.build() {
                window.paint_path(path, color);
            }
        }
        Icon::More => {
            for y in [4.0, 8.0, 12.0] {
                let mut path = gpui::PathBuilder::fill();
                path.add_polygon(&circle_points(at(8.0, y), px(1.1 * scale), 12), true);
                if let Ok(path) = path.build() {
                    window.paint_path(path, color);
                }
            }
        }
        Icon::Pin => {
            let mut path = stroke(1.5);
            // 图钉的针
            path.move_to(at(8.0, 9.0));
            path.line_to(at(8.0, 14.0));
            // 图钉的帽
            path.move_to(at(4.5, 9.5));
            path.line_to(at(11.5, 9.5));
            path.move_to(at(6.0, 9.5));
            path.line_to(at(6.0, 3.5));
            path.line_to(at(10.0, 3.5));
            path.line_to(at(10.0, 9.5));
            if let Ok(path) = path.build() {
                window.paint_path(path, color);
            }
        }
        Icon::Close => {
            let mut path = stroke(1.5);
            path.move_to(at(4.0, 4.0));
            path.line_to(at(12.0, 12.0));
            path.move_to(at(12.0, 4.0));
            path.line_to(at(4.0, 12.0));
            if let Ok(path) = path.build() {
                window.paint_path(path, color);
            }
        }
        Icon::Subtitle => {
            let mut path = stroke(1.4);
            // 外框
            path.move_to(at(2.5, 3.5));
            path.line_to(at(13.5, 3.5));
            path.line_to(at(13.5, 12.5));
            path.line_to(at(2.5, 12.5));
            path.close();
            // 下部两条横线，代表字幕
            path.move_to(at(5.0, 9.0));
            path.line_to(at(11.0, 9.0));
            path.move_to(at(5.0, 11.0));
            path.line_to(at(11.0, 11.0));
            if let Ok(path) = path.build() {
                window.paint_path(path, color);
            }
        }
        Icon::Layout => {
            let mut path = stroke(1.4);
            // 四宫格
            path.move_to(at(2.5, 2.5));
            path.line_to(at(13.5, 2.5));
            path.line_to(at(13.5, 13.5));
            path.line_to(at(2.5, 13.5));
            path.close();
            path.move_to(at(8.0, 2.5));
            path.line_to(at(8.0, 13.5));
            path.move_to(at(2.5, 8.0));
            path.line_to(at(13.5, 8.0));
            if let Ok(path) = path.build() {
                window.paint_path(path, color);
            }
        }
        Icon::ArrangeRow => {
            // 三块等宽的竖条左右并排，块与块之间留一点缝。
            let mut path = stroke(1.4);
            for x in [2.0f32, 6.75, 11.5] {
                path.move_to(at(x, 3.0));
                path.line_to(at(x + 2.5, 3.0));
                path.line_to(at(x + 2.5, 13.0));
                path.line_to(at(x, 13.0));
                path.close();
            }
            if let Ok(path) = path.build() {
                window.paint_path(path, color);
            }
        }
        Icon::ArrangeColumn => {
            // 三块等高的横条上下堆叠。
            let mut path = stroke(1.4);
            for y in [2.0f32, 6.75, 11.5] {
                path.move_to(at(3.0, y));
                path.line_to(at(13.0, y));
                path.line_to(at(13.0, y + 2.5));
                path.line_to(at(3.0, y + 2.5));
                path.close();
            }
            if let Ok(path) = path.build() {
                window.paint_path(path, color);
            }
        }
        Icon::ArrangeShuffle => {
            // 一段近似圆环的弧，加上末端的箭头头部，表达「循环 / 换一种」。
            // 四段三次贝塞尔沿顺时针方向拼出约 290 度的圆弧，顶部右侧留出缺口。
            // 坐标落在 2.8~13.2 之间，加上 1.4 的线宽，实际占 2.1~13.9，不会超出 16×16。
            let mut path = stroke(1.4);
            path.move_to(at(11.93, 10.75));
            path.cubic_bezier_to(at(6.56, 12.58), at(10.73, 12.47), at(8.55, 13.21));
            path.cubic_bezier_to(at(3.2, 8.0), at(4.56, 11.95), at(3.2, 10.09));
            path.cubic_bezier_to(at(6.56, 3.42), at(3.2, 5.91), at(4.56, 4.05));
            path.cubic_bezier_to(at(11.93, 5.25), at(8.55, 2.79), at(10.73, 3.53));
            // 箭头头部：顺着弧线末端的切线方向。
            path.move_to(at(10.14, 4.79));
            path.line_to(at(11.93, 5.25));
            path.line_to(at(12.11, 3.41));
            if let Ok(path) = path.build() {
                window.paint_path(path, color);
            }
        }
        Icon::SelectAll => {
            // 四个角括号围住中间一个小方块，和「全选」的通常画法一致。
            let mut path = stroke(1.5);
            for (cx, cy, sx, sy) in [
                (2.5f32, 2.5f32, 1.0f32, 1.0f32),
                (13.5, 2.5, -1.0, 1.0),
                (2.5, 13.5, 1.0, -1.0),
                (13.5, 13.5, -1.0, -1.0),
            ] {
                path.move_to(at(cx + sx * 3.0, cy));
                path.line_to(at(cx, cy));
                path.line_to(at(cx, cy + sy * 3.0));
            }
            path.move_to(at(6.5, 6.5));
            path.line_to(at(9.5, 6.5));
            path.line_to(at(9.5, 9.5));
            path.line_to(at(6.5, 9.5));
            path.close();
            if let Ok(path) = path.build() {
                window.paint_path(path, color);
            }
        }
        Icon::Gap => {
            let mut path = stroke(1.4);
            // 上块
            path.move_to(at(2.5, 2.5));
            path.line_to(at(13.5, 2.5));
            path.line_to(at(13.5, 6.0));
            path.line_to(at(2.5, 6.0));
            path.close();
            // 下块
            path.move_to(at(2.5, 10.0));
            path.line_to(at(13.5, 10.0));
            path.line_to(at(13.5, 13.5));
            path.line_to(at(2.5, 13.5));
            path.close();
            // 中间那条缝
            path.move_to(at(2.5, 8.0));
            path.line_to(at(13.5, 8.0));
            if let Ok(path) = path.build() {
                window.paint_path(path, color);
            }
        }
        Icon::Plus => {
            let mut path = stroke(1.5);
            path.move_to(at(8.0, 3.5));
            path.line_to(at(8.0, 12.5));
            path.move_to(at(3.5, 8.0));
            path.line_to(at(12.5, 8.0));
            if let Ok(path) = path.build() {
                window.paint_path(path, color);
            }
        }
        Icon::Minus => {
            let mut path = stroke(1.5);
            path.move_to(at(3.5, 8.0));
            path.line_to(at(12.5, 8.0));
            if let Ok(path) = path.build() {
                window.paint_path(path, color);
            }
        }
        Icon::Check => {
            let mut path = stroke(1.8);
            path.move_to(at(3.5, 8.5));
            path.line_to(at(6.5, 11.5));
            path.line_to(at(12.5, 4.5));
            if let Ok(path) = path.build() {
                window.paint_path(path, color);
            }
        }
        Icon::ChevronDown => {
            let mut path = stroke(1.5);
            path.move_to(at(4.5, 6.5));
            path.line_to(at(8.0, 10.0));
            path.line_to(at(11.5, 6.5));
            if let Ok(path) = path.build() {
                window.paint_path(path, color);
            }
        }
        Icon::ChevronRight => {
            let mut path = stroke(1.5);
            path.move_to(at(6.5, 4.5));
            path.line_to(at(10.0, 8.0));
            path.line_to(at(6.5, 11.5));
            if let Ok(path) = path.build() {
                window.paint_path(path, color);
            }
        }
        Icon::Copy => {
            let mut path = stroke(1.4);
            // 后面那张纸
            path.move_to(at(5.0, 2.5));
            path.line_to(at(13.5, 2.5));
            path.line_to(at(13.5, 10.5));
            // 前面那张纸
            path.move_to(at(2.5, 5.0));
            path.line_to(at(2.5, 13.5));
            path.line_to(at(10.5, 13.5));
            path.line_to(at(10.5, 10.5));
            if let Ok(path) = path.build() {
                window.paint_path(path, color);
            }
        }
        Icon::Export => {
            let mut path = stroke(1.5);
            // 托盘
            path.move_to(at(2.5, 9.5));
            path.line_to(at(2.5, 13.5));
            path.line_to(at(13.5, 13.5));
            path.line_to(at(13.5, 9.5));
            // 向上箭头
            path.move_to(at(8.0, 10.5));
            path.line_to(at(8.0, 2.5));
            path.move_to(at(4.5, 6.0));
            path.line_to(at(8.0, 2.5));
            path.line_to(at(11.5, 6.0));
            if let Ok(path) = path.build() {
                window.paint_path(path, color);
            }
        }
        Icon::Trash => {
            let mut path = stroke(1.4);
            // 盖子
            path.move_to(at(2.5, 4.5));
            path.line_to(at(13.5, 4.5));
            path.move_to(at(6.0, 4.5));
            path.line_to(at(6.0, 2.5));
            path.line_to(at(10.0, 2.5));
            path.line_to(at(10.0, 4.5));
            // 桶身
            path.move_to(at(4.0, 4.5));
            path.line_to(at(4.8, 13.5));
            path.line_to(at(11.2, 13.5));
            path.line_to(at(12.0, 4.5));
            // 两道竖纹
            path.move_to(at(6.8, 6.5));
            path.line_to(at(7.1, 11.5));
            path.move_to(at(9.2, 6.5));
            path.line_to(at(8.9, 11.5));
            if let Ok(path) = path.build() {
                window.paint_path(path, color);
            }
        }
        Icon::PlusLarge => {
            let mut path = stroke(1.2);
            path.move_to(at(8.0, 2.0));
            path.line_to(at(8.0, 14.0));
            path.move_to(at(2.0, 8.0));
            path.line_to(at(14.0, 8.0));
            if let Ok(path) = path.build() {
                window.paint_path(path, color);
            }
        }
    }
}

/// 生成一个近似圆的点序列。
fn circle_points(center: Point<Pixels>, radius: Pixels, segments: usize) -> Vec<Point<Pixels>> {
    let mut points = Vec::with_capacity(segments);
    for index in 0..segments {
        let angle = std::f32::consts::TAU * index as f32 / segments as f32;
        let radius = f32::from(radius);
        points.push(point(
            center.x + px(radius * angle.cos()),
            center.y + px(radius * angle.sin()),
        ));
    }
    points
}

/// 图标在禁用状态下的颜色。
pub fn disabled_color(theme: &Theme) -> Hsla {
    theme.faint
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_icon_has_a_stable_debug_name() {
        // 图标类型只增不减；这条测试防止以后改动时不小心删掉某个分支。
        let all = [
            Icon::Undo,
            Icon::Redo,
            Icon::More,
            Icon::Pin,
            Icon::Close,
            Icon::Subtitle,
            Icon::Layout,
            Icon::Gap,
            Icon::Plus,
            Icon::Minus,
            Icon::Check,
            Icon::ChevronDown,
            Icon::ChevronRight,
            Icon::Copy,
            Icon::Export,
            Icon::Trash,
            Icon::PlusLarge,
            Icon::ArrangeRow,
            Icon::ArrangeColumn,
            Icon::ArrangeShuffle,
            Icon::SelectAll,
        ];
        for icon in all {
            assert!(!format!("{icon:?}").is_empty());
        }
    }

    #[test]
    fn circle_points_stay_on_the_radius() {
        let center = point(px(0.), px(0.));
        let points = circle_points(center, px(10.), 8);
        assert_eq!(points.len(), 8);
        for p in points {
            let x = f32::from(p.x);
            let y = f32::from(p.y);
            let distance = (x * x + y * y).sqrt();
            assert!((distance - 10.0).abs() < 0.01, "点要落在半径上");
        }
    }
}
