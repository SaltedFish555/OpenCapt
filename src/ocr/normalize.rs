use super::{OcrResult, OcrResultRaw, OcrTextBlock};
use crate::config::OcrBboxScaleMode;

pub(in crate::ocr) fn normalize_result(
    raw: OcrResultRaw,
    scale_mode: OcrBboxScaleMode,
    image_width: u32,
    image_height: u32,
) -> OcrResult {
    let blocks = raw
        .blocks
        .into_iter()
        .map(|block| OcrTextBlock {
            text: block.text,
            bbox_norm: normalize_bbox(block.bbox_raw, scale_mode, image_width, image_height),
            confidence: block.confidence,
        })
        .filter(|block| {
            let [x1, y1, x2, y2] = block.bbox_norm;
            (x2 - x1) > 0.001 && (y2 - y1) > 0.001 && !block.text.trim().is_empty()
        })
        .collect();

    OcrResult {
        full_text: raw.full_text,
        blocks,
    }
}

pub(in crate::ocr) fn normalize_bbox(
    raw_bbox: [f32; 4],
    scale_mode: OcrBboxScaleMode,
    image_width: u32,
    image_height: u32,
) -> [f32; 4] {
    let width = image_width.max(1) as f32;
    let height = image_height.max(1) as f32;

    let mut x1 = scale_value(raw_bbox[0], scale_mode, true, width);
    let mut y1 = scale_value(raw_bbox[1], scale_mode, false, height);
    let mut x2 = scale_value(raw_bbox[2], scale_mode, true, width);
    let mut y2 = scale_value(raw_bbox[3], scale_mode, false, height);

    if x1 > x2 {
        std::mem::swap(&mut x1, &mut x2);
    }
    if y1 > y2 {
        std::mem::swap(&mut y1, &mut y2);
    }

    [
        x1.clamp(0.0, 1.0),
        y1.clamp(0.0, 1.0),
        x2.clamp(0.0, 1.0),
        y2.clamp(0.0, 1.0),
    ]
}

fn scale_value(value: f32, scale_mode: OcrBboxScaleMode, is_x: bool, dim: f32) -> f32 {
    if !value.is_finite() {
        return 0.0;
    }
    match scale_mode {
        OcrBboxScaleMode::ZeroToOne => value,
        OcrBboxScaleMode::ZeroTo999 => value / 999.0,
        OcrBboxScaleMode::ZeroTo1000 => value / 1000.0,
        OcrBboxScaleMode::PixelAbsolute => {
            let denom = if is_x { dim } else { dim };
            if denom <= 0.0 { 0.0 } else { value / denom }
        }
    }
}
