//! 程序内生成的托盘图标：不新增图片文件，避免打包时再处理各平台图标格式。

/// 图标边长（像素）。
const SIZE: u32 = 64;

/// 超采样倍数：先画 4 倍大的图，再缩小，边缘更顺滑。
const SUPERSAMPLE: u32 = 4;

const BLUE: [u8; 4] = [47, 111, 235, 255];
const WHITE: [u8; 4] = [255, 255, 255, 255];

/// 返回托盘图标的 RGBA 数据和宽高。
///
/// `template` 为真时画 macOS 菜单栏用的模板图：透明背景 + 单色图形，
/// 系统会按照当前明暗主题自动换色。
pub(crate) fn tray_rgba(template: bool) -> (Vec<u8>, u32, u32) {
    let big = SIZE * SUPERSAMPLE;
    let mut canvas = vec![0u8; (big * big * 4) as usize];

    if template {
        // 模板图必须单色：只画图形，背景保持透明。
        fill_triangle(
            &mut canvas,
            big,
            (12.0, 18.0),
            (12.0, 46.0),
            (35.0, 32.0),
            WHITE,
        );
        fill_rounded_rect(
            &mut canvas,
            big,
            &RoundedRect::new(40.0, 23.0, 56.0, 29.0, 3.0),
            WHITE,
        );
        fill_rounded_rect(
            &mut canvas,
            big,
            &RoundedRect::new(40.0, 35.0, 56.0, 41.0, 3.0),
            WHITE,
        );
    } else {
        // 彩色图标：蓝色圆角底 + 白色播放键和两条字幕横条。
        fill_rounded_rect(
            &mut canvas,
            big,
            &RoundedRect::new(2.0, 2.0, 62.0, 62.0, 13.0),
            BLUE,
        );
        fill_triangle(
            &mut canvas,
            big,
            (17.0, 20.0),
            (17.0, 44.0),
            (36.0, 32.0),
            WHITE,
        );
        fill_rounded_rect(
            &mut canvas,
            big,
            &RoundedRect::new(41.0, 23.0, 55.0, 28.0, 2.5),
            WHITE,
        );
        fill_rounded_rect(
            &mut canvas,
            big,
            &RoundedRect::new(41.0, 36.0, 55.0, 41.0, 2.5),
            WHITE,
        );
    }

    (downsample(&canvas, big), SIZE, SIZE)
}

/// 一个圆角矩形：左上角、右下角坐标和圆角半径。
struct RoundedRect {
    x0: f32,
    y0: f32,
    x1: f32,
    y1: f32,
    radius: f32,
}

impl RoundedRect {
    fn new(x0: f32, y0: f32, x1: f32, y1: f32, radius: f32) -> Self {
        Self {
            x0,
            y0,
            x1,
            y1,
            radius,
        }
    }

    /// 一个点是否落在这个圆角矩形内。
    fn contains(&self, x: f32, y: f32) -> bool {
        if x < self.x0 || x > self.x1 || y < self.y0 || y > self.y1 {
            return false;
        }
        let center_x = x.clamp(self.x0 + self.radius, self.x1 - self.radius);
        let center_y = y.clamp(self.y0 + self.radius, self.y1 - self.radius);
        let dx = x - center_x;
        let dy = y - center_y;
        dx * dx + dy * dy <= self.radius * self.radius
    }
}

fn fill_rounded_rect(canvas: &mut [u8], big: u32, rect: &RoundedRect, color: [u8; 4]) {
    let scale = SUPERSAMPLE as f32;
    for py in 0..big {
        for px in 0..big {
            let x = (px as f32 + 0.5) / scale;
            let y = (py as f32 + 0.5) / scale;
            if rect.contains(x, y) {
                put_pixel(canvas, big, px, py, color);
            }
        }
    }
}

fn fill_triangle(
    canvas: &mut [u8],
    big: u32,
    a: (f32, f32),
    b: (f32, f32),
    c: (f32, f32),
    color: [u8; 4],
) {
    let scale = SUPERSAMPLE as f32;
    for py in 0..big {
        for px in 0..big {
            let point = ((px as f32 + 0.5) / scale, (py as f32 + 0.5) / scale);
            if inside_triangle(point, a, b, c) {
                put_pixel(canvas, big, px, py, color);
            }
        }
    }
}

fn inside_triangle(point: (f32, f32), a: (f32, f32), b: (f32, f32), c: (f32, f32)) -> bool {
    let d1 = edge(point, a, b);
    let d2 = edge(point, b, c);
    let d3 = edge(point, c, a);
    let has_negative = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let has_positive = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(has_negative && has_positive)
}

fn edge(point: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    (point.0 - a.0) * (b.1 - a.1) - (point.1 - a.1) * (b.0 - a.0)
}

fn put_pixel(canvas: &mut [u8], big: u32, x: u32, y: u32, color: [u8; 4]) {
    let offset = ((y * big + x) * 4) as usize;
    canvas[offset..offset + 4].copy_from_slice(&color);
}

/// 把超采样画布按 alpha 加权缩小到目标尺寸。
fn downsample(canvas: &[u8], big: u32) -> Vec<u8> {
    let samples = SUPERSAMPLE * SUPERSAMPLE;
    let mut out = vec![0u8; (SIZE * SIZE * 4) as usize];
    for y in 0..SIZE {
        for x in 0..SIZE {
            let mut alpha_sum = 0u32;
            let mut red_sum = 0u32;
            let mut green_sum = 0u32;
            let mut blue_sum = 0u32;
            for dy in 0..SUPERSAMPLE {
                for dx in 0..SUPERSAMPLE {
                    let px = x * SUPERSAMPLE + dx;
                    let py = y * SUPERSAMPLE + dy;
                    let offset = ((py * big + px) * 4) as usize;
                    let alpha = canvas[offset + 3] as u32;
                    alpha_sum += alpha;
                    red_sum += canvas[offset] as u32 * alpha;
                    green_sum += canvas[offset + 1] as u32 * alpha;
                    blue_sum += canvas[offset + 2] as u32 * alpha;
                }
            }
            let offset = ((y * SIZE + x) * 4) as usize;
            let alpha = (alpha_sum / samples) as u8;
            out[offset + 3] = alpha;
            out[offset] = red_sum.checked_div(alpha_sum).unwrap_or(0) as u8;
            out[offset + 1] = green_sum.checked_div(alpha_sum).unwrap_or(0) as u8;
            out[offset + 2] = blue_sum.checked_div(alpha_sum).unwrap_or(0) as u8;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colored_icon_has_blue_background_and_white_glyph() {
        let (rgba, width, height) = tray_rgba(false);
        assert_eq!((width, height), (SIZE, SIZE));
        assert_eq!(rgba.len(), (SIZE * SIZE * 4) as usize);

        let (pixels, rest) = rgba.as_chunks::<4>();
        assert!(rest.is_empty());
        let has_blue = pixels
            .iter()
            .any(|pixel| pixel[0] < 100 && pixel[2] > 200 && pixel[3] == 255);
        let has_white = pixels
            .iter()
            .any(|pixel| pixel[0] > 240 && pixel[1] > 240 && pixel[2] > 240);
        assert!(has_blue, "彩色图标应该有蓝色底色");
        assert!(has_white, "彩色图标应该有白色图形");
        assert_eq!(rgba[3], 0, "圆角之外的角落应该是透明的");
    }

    #[test]
    fn template_icon_is_single_color_on_transparent() {
        let (rgba, width, height) = tray_rgba(true);
        assert_eq!((width, height), (SIZE, SIZE));

        let mut opaque = 0;
        let mut transparent = 0;
        let (pixels, rest) = rgba.as_chunks::<4>();
        assert!(rest.is_empty());
        for pixel in pixels {
            if pixel[3] == 0 {
                transparent += 1;
                continue;
            }
            if pixel[3] == 255 {
                opaque += 1;
                assert_eq!(
                    (pixel[0], pixel[1], pixel[2]),
                    (255, 255, 255),
                    "模板图里不透明的像素必须是白色"
                );
            }
        }
        assert!(opaque > 100, "模板图应该有不透明的图形，实际 {opaque}");
        assert!(transparent > 1000, "模板图背景应该透明，实际 {transparent}");
    }
}
