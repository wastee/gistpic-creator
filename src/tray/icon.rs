//! 托盘图标：直接使用仓库里的 `assets/tray-icon.png`。
//!
//! 那张图由 `assets/icon.png` 缩放到 64×64 得到，背景是透明的。
//! macOS 菜单栏要求单色模板图，这里按亮度把彩色图转成白色剪影：
//! 图标背景是深色，字幕条、闪电和图片框是浅色。

/// 托盘图标，64×64。
const TRAY_ICON_PNG: &[u8] = include_bytes!("../../assets/tray-icon.png");

/// 判断一个像素属于「图形」的亮度下限。
///
/// 图标背景的亮度在 20 到 50 之间，字幕条、闪电和图片框在 140 以上，
/// 取 100 能把两者分开。
const TEMPLATE_LUMINANCE_THRESHOLD: f32 = 100.0;

/// 返回托盘图标的 RGBA 数据和宽高。
///
/// `template` 为真时返回 macOS 菜单栏用的模板图：透明背景 + 单色图形，
/// 系统会按照当前明暗主题自动换色。
pub(crate) fn tray_rgba(template: bool) -> (Vec<u8>, u32, u32) {
    let image = image::load_from_memory(TRAY_ICON_PNG)
        .expect("assets/tray-icon.png 应该能解码")
        .to_rgba8();
    let (width, height) = image.dimensions();
    let mut rgba = image.into_raw();
    if template {
        to_template(&mut rgba);
    }
    (rgba, width, height)
}

/// 把彩色图标转成模板图：浅色图形变成白色，深色背景和透明区域变成全透明。
fn to_template(rgba: &mut [u8]) {
    for pixel in rgba.chunks_exact_mut(4) {
        if pixel[3] == 0 {
            continue;
        }
        let luminance =
            0.2126 * pixel[0] as f32 + 0.7152 * pixel[1] as f32 + 0.0722 * pixel[2] as f32;
        if luminance >= TEMPLATE_LUMINANCE_THRESHOLD {
            let alpha = pixel[3];
            pixel.copy_from_slice(&[255, 255, 255, alpha]);
        } else {
            pixel.copy_from_slice(&[0, 0, 0, 0]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colored_icon_comes_from_the_png() {
        let (rgba, width, height) = tray_rgba(false);
        assert_eq!((width, height), (64, 64));
        assert_eq!(rgba.len(), 64 * 64 * 4);

        let (pixels, rest) = rgba.as_chunks::<4>();
        assert!(rest.is_empty());
        let opaque = pixels.iter().filter(|pixel| pixel[3] > 200).count();
        let transparent = pixels.iter().filter(|pixel| pixel[3] == 0).count();
        assert!(opaque > 2000, "图标主体应该是不透明的，实际 {opaque}");
        assert!(
            transparent > 100,
            "圆角之外应该是透明的，实际 {transparent}"
        );
        assert_eq!(rgba[3], 0, "左上角在圆角之外，应该是透明的");
    }

    #[test]
    fn template_icon_is_white_on_transparent() {
        let (rgba, width, height) = tray_rgba(true);
        assert_eq!((width, height), (64, 64));

        let (pixels, rest) = rgba.as_chunks::<4>();
        assert!(rest.is_empty());
        let mut opaque = 0;
        let mut transparent = 0;
        for pixel in pixels {
            if pixel[3] == 0 {
                transparent += 1;
                continue;
            }
            if pixel[3] > 200 {
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
