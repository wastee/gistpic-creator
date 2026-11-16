//! 字幕检测：加载内嵌的文字检测模型，把图片里的文字行框出来，再交给 `classify` 挑选。
//!
//! 只加载检测模型（约 2.4 MB），不加载 9.7 MB 的识别模型，因为这里只需要“文字在哪”，
//! 不需要“写的是什么”。

use std::borrow::Cow;

use image::RgbImage;
use ocrs::{ImageSource, OcrEngine, OcrEngineParams};
use rten::Model;
use thiserror::Error;

use crate::i18n::Language;
use crate::model::{LineBox, SubtitleBlock};

use super::classify;

/// 内嵌的文字检测模型（ONNX，2,499,479 字节，来自 robertknight/ocrs-models，CC-BY-SA-4.0）。
const DETECTION_MODEL_BYTES: &[u8] = include_bytes!("../../assets/text-detection.onnx");

/// 检测模型要求的输入尺寸（宽, 高），取自上面这个模型的输入形状 [1, 1, 800, 600]。
///
/// ocrs 会把比这个尺寸小的图用纯黑补齐再缩放。纯黑补出来的是一条硬边，
/// 检测模型会贴着这条边误报一行“文字”，所以补边要自己先做掉，见
/// [pad_to_detection_input]。单元测试会核对这两个常量跟模型的实际输入形状一致。
const DETECTION_INPUT_WIDTH: u32 = 600;
const DETECTION_INPUT_HEIGHT: u32 = 800;

#[derive(Debug, Error)]
pub enum DetectError {
    #[error("加载文字检测模型失败: {0}")]
    Model(#[from] rten::LoadError),
    #[error("文字检测失败: {0}")]
    Ocr(#[from] ocrs::OcrError),
    #[error("图片格式不适合检测: {0}")]
    ImageSource(#[from] ocrs::ImageSourceError),
    #[error("{0}")]
    Source(String),
}

impl DetectError {
    /// 给界面显示用的错误说明。库自己给出的技术细节保持原样。
    pub fn localized(&self, language: Language) -> String {
        match (language, self) {
            (Language::ZhCn, DetectError::Model(err)) => format!("加载文字检测模型失败: {err}"),
            (Language::ZhCn, DetectError::Ocr(err)) => format!("文字检测失败: {err}"),
            (Language::ZhCn, DetectError::ImageSource(err)) => {
                format!("图片格式不适合检测: {err}")
            }
            (Language::ZhCn, DetectError::Source(message)) => message.clone(),
            (Language::EnUs, DetectError::Model(err)) => {
                format!("Failed to load the text detection model: {err}")
            }
            (Language::EnUs, DetectError::Ocr(err)) => {
                format!("Text detection failed: {err}")
            }
            (Language::EnUs, DetectError::ImageSource(err)) => {
                format!("Unsupported image format for detection: {err}")
            }
            (Language::EnUs, DetectError::Source(message)) => message.clone(),
        }
    }
}

/// 字幕检测器。构造一次，重复使用。
pub struct SubtitleDetector {
    engine: OcrEngine,
}

impl SubtitleDetector {
    /// 用内嵌的模型构造检测器。
    pub fn new_embedded() -> Result<Self, DetectError> {
        let model = Model::load_static_slice(DETECTION_MODEL_BYTES)?;
        let engine = OcrEngine::new(OcrEngineParams {
            detection_model: Some(model),
            recognition_model: None,
            debug: false,
            ..Default::default()
        })?;
        Ok(SubtitleDetector { engine })
    }

    /// 在一张 RGB 图片的整张画面里找字幕块。找不到返回 `Ok(None)`。
    pub fn detect(&self, rgb: &RgbImage) -> Result<Option<SubtitleBlock>, DetectError> {
        let (width, height) = rgb.dimensions();
        if width < 32 || height < 32 {
            return Ok(None);
        }
        let boxes = self.detect_line_boxes(rgb)?;
        Ok(classify::pick_subtitle(boxes, width as f32, height as f32))
    }

    /// 检测所有候选字幕块，不做尺寸过滤。用于训练数据导出。
    pub fn detect_candidate_blocks(
        &self,
        rgb: &RgbImage,
    ) -> Result<Vec<SubtitleBlock>, DetectError> {
        let (width, height) = rgb.dimensions();
        if width < 32 || height < 32 {
            return Ok(Vec::new());
        }
        let boxes = self.detect_line_boxes(rgb)?;
        Ok(classify::candidate_blocks(
            boxes,
            width as f32,
            height as f32,
        ))
    }

    fn detect_line_boxes(&self, rgb: &RgbImage) -> Result<Vec<LineBox>, DetectError> {
        // 先自己补边，ocrs 就不会再往里塞黑边。补边只加在右边和下边，
        // 原图的坐标不用平移；落在补边区域里的框会被 classify 的夹紧丢掉。
        let padded = pad_to_detection_input(rgb);
        let (width, height) = padded.dimensions();
        let source = ImageSource::from_bytes(padded.as_raw(), (width, height))?;
        let input = self.engine.prepare_input(source)?;
        let words = self.engine.detect_words(&input)?;
        let line_groups = self.engine.find_text_lines(&input, &words);

        let mut boxes = Vec::with_capacity(line_groups.len());
        for line in &line_groups {
            let mut min_x = f32::MAX;
            let mut min_y = f32::MAX;
            let mut max_x = f32::MIN;
            let mut max_y = f32::MIN;
            for word in line {
                for corner in word.corners() {
                    min_x = min_x.min(corner.x);
                    min_y = min_y.min(corner.y);
                    max_x = max_x.max(corner.x);
                    max_y = max_y.max(corner.y);
                }
            }
            if min_x.is_finite() && min_y.is_finite() && max_x > min_x && max_y > min_y {
                boxes.push(LineBox {
                    x0: min_x,
                    y0: min_y,
                    x1: max_x,
                    y1: max_y,
                });
            }
        }
        Ok(boxes)
    }
}

/// 把图片补到检测模型要求的输入尺寸，多出来的部分复制图片自己的边缘像素。
///
/// ocrs 遇到比模型输入小的图会用纯黑补齐。纯黑会在内容底边形成一条硬边，
/// 检测模型会把这条边当成一行文字报出来，位置正好贴底，很容易压过真正的字幕。
/// 这里先用图片自己的边缘像素补够尺寸，ocrs 就不会再补黑边。
///
/// 只在右边和下边补，左上角坐标不变，检测结果的坐标可以直接用。
fn pad_to_detection_input(rgb: &RgbImage) -> Cow<'_, RgbImage> {
    let (width, height) = rgb.dimensions();
    let padded_width = width.max(DETECTION_INPUT_WIDTH);
    let padded_height = height.max(DETECTION_INPUT_HEIGHT);
    if padded_width == width && padded_height == height {
        return Cow::Borrowed(rgb);
    }

    let mut padded = RgbImage::new(padded_width, padded_height);
    image::imageops::replace(&mut padded, rgb, 0, 0);
    // 右边：每一行重复最右一列的像素。
    for y in 0..height {
        let edge = *rgb.get_pixel(width - 1, y);
        for x in width..padded_width {
            padded.put_pixel(x, y, edge);
        }
    }
    // 下边：每一行重复最下一行的像素，右下角因此也是原图的右下角。
    for y in height..padded_height {
        for x in 0..padded_width {
            let pixel = *padded.get_pixel(x, height - 1);
            padded.put_pixel(x, y, pixel);
        }
    }
    Cow::Owned(padded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgb;

    /// 造一张每个像素颜色都不一样的图，补出来的像素复制了谁一眼能看出来。
    fn gradient(width: u32, height: u32) -> RgbImage {
        let mut image = RgbImage::new(width, height);
        for y in 0..height {
            for x in 0..width {
                image.put_pixel(x, y, Rgb([(x % 251) as u8, (y % 241) as u8, (x + y) as u8]));
            }
        }
        image
    }

    #[test]
    fn pads_short_images_with_edge_pixels() {
        let source = gradient(4, 2);
        let padded = pad_to_detection_input(&source);
        assert_eq!(
            padded.dimensions(),
            (DETECTION_INPUT_WIDTH, DETECTION_INPUT_HEIGHT)
        );

        // 原图区域一个像素都不改。
        for y in 0..2 {
            for x in 0..4 {
                assert_eq!(padded.get_pixel(x, y), source.get_pixel(x, y));
            }
        }
        // 右边重复最右一列，下边重复最下一行，右下角是原图的右下角。
        assert_eq!(
            *padded.get_pixel(5, 1),
            *source.get_pixel(3, 1),
            "右边应重复最右一列"
        );
        assert_eq!(
            *padded.get_pixel(0, 3),
            *source.get_pixel(0, 1),
            "下边应重复最下一行"
        );
        assert_eq!(
            *padded.get_pixel(DETECTION_INPUT_WIDTH - 1, DETECTION_INPUT_HEIGHT - 1),
            *source.get_pixel(3, 1),
            "右下角应等于原图右下角"
        );
    }

    #[test]
    fn leaves_large_images_untouched() {
        let source = gradient(
            DETECTION_INPUT_WIDTH + 100,
            DETECTION_INPUT_HEIGHT + 100,
        );
        let padded = pad_to_detection_input(&source);
        assert!(
            matches!(padded, Cow::Borrowed(_)),
            "尺寸够的图不该被复制"
        );
        assert_eq!(padded.dimensions(), source.dimensions());
    }

    #[test]
    fn detection_input_size_matches_constants() {
        let model = Model::load_static_slice(DETECTION_MODEL_BYTES).expect("内嵌模型应能加载");
        let shape = model.input_shape(0).expect("模型应给出输入形状");
        assert_eq!(shape.len(), 4, "模型输入应是 NCHW 四维");
        assert!(
            matches!(shape[2], rten::Dimension::Fixed(height) if height == DETECTION_INPUT_HEIGHT as usize),
            "DETECTION_INPUT_HEIGHT 必须等于模型输入高度"
        );
        assert!(
            matches!(shape[3], rten::Dimension::Fixed(width) if width == DETECTION_INPUT_WIDTH as usize),
            "DETECTION_INPUT_WIDTH 必须等于模型输入宽度"
        );
    }
}
