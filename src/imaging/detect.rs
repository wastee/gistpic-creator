//! 字幕检测：加载内嵌的文字检测模型，把图片里的文字行框出来，再交给 `classify` 挑选。
//!
//! 只加载检测模型（约 2.4 MB），不加载 9.7 MB 的识别模型，因为这里只需要“文字在哪”，
//! 不需要“写的是什么”。

use image::RgbImage;
use ocrs::{ImageSource, OcrEngine, OcrEngineParams};
use rten::Model;
use thiserror::Error;

use crate::i18n::Language;
use crate::model::{LineBox, SubtitleBlock};

use super::classify;

/// 内嵌的文字检测模型（ONNX，2,499,479 字节，来自 robertknight/ocrs-models，CC-BY-SA-4.0）。
const DETECTION_MODEL_BYTES: &[u8] = include_bytes!("../../assets/text-detection.onnx");

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
        let (width, height) = rgb.dimensions();
        let source = ImageSource::from_bytes(rgb.as_raw(), (width, height))?;
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
