use crate::config::{OcrBboxScaleMode, OcrProfile, OcrProviderKind};
use anyhow::{Context, Result, anyhow, bail};
use base64::{Engine as _, engine::general_purpose};
use reqwest::blocking::Client;
use serde::Deserialize;
use serde_json::Value;
use std::time::Duration;

const SYSTEM_PROMPT: &str = "You are an OCR engine. Return strict JSON only.";
const USER_PROMPT_JSON: &str = "Recognize all text from the image. Return JSON with shape: {\"full_text\": string, \"blocks\": [{\"text\": string, \"bbox\": [x1,y1,x2,y2], \"confidence\": number|null}]}. bbox must be in your native coordinate scale.";
const USER_PROMPT_DEEPSEEK_GROUNDING: &str = "<image>\\n<|grounding|>OCR this image.";

#[derive(Debug, Clone)]
pub struct OcrRecognizeRequest {
    pub image_png: Vec<u8>,
    pub timeout_ms: u64,
    pub language_hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OcrTextBlockRaw {
    pub text: String,
    pub bbox_raw: [f32; 4],
    pub confidence: Option<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OcrResultRaw {
    pub full_text: String,
    pub blocks: Vec<OcrTextBlockRaw>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OcrTextBlock {
    pub text: String,
    pub bbox_norm: [f32; 4],
    pub confidence: Option<f32>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OcrResult {
    pub full_text: String,
    pub blocks: Vec<OcrTextBlock>,
}

pub trait OcrProvider {
    fn recognize_raw(
        &self,
        profile: &OcrProfile,
        request: &OcrRecognizeRequest,
    ) -> Result<OcrResultRaw>;
    fn test_connection(&self, profile: &OcrProfile, timeout_ms: u64) -> Result<()>;
}

#[derive(Default)]
pub struct OpenAiCompatibleProvider;

#[derive(Default)]
pub struct BaiduOcrProvider;

impl OcrProvider for OpenAiCompatibleProvider {
    fn recognize_raw(
        &self,
        profile: &OcrProfile,
        request: &OcrRecognizeRequest,
    ) -> Result<OcrResultRaw> {
        let client = Client::builder()
            .timeout(Duration::from_millis(request.timeout_ms.max(1_000)))
            .build()
            .context("failed to build http client")?;

        let url = endpoint_url(&profile.base_url, "chat/completions");
        let image_base64 = general_purpose::STANDARD.encode(&request.image_png);
        let payload = build_chat_payload(profile, request, &image_base64);

        let response = match client
            .post(url)
            .bearer_auth(profile.api_key.trim())
            .json(&payload)
            .send()
        {
            Ok(response) => response,
            Err(error) => {
                if error.is_timeout() {
                    bail!("OCR 请求超时（{} ms）", request.timeout_ms.max(1_000));
                }
                return Err(anyhow!("OCR 请求失败: {}", error));
            }
        };

        let status = response.status();
        let body = response
            .text()
            .context("failed to read ocr response body")?;
        if !status.is_success() {
            match status.as_u16() {
                401 => bail!("OCR 鉴权失败（401），请检查 API Key"),
                429 => bail!("OCR 请求过于频繁（429），请稍后重试"),
                _ => bail!("OCR 请求失败（{}）: {}", status, body),
            }
        }

        parse_chat_completion_ocr_response(&body)
    }

    fn test_connection(&self, profile: &OcrProfile, timeout_ms: u64) -> Result<()> {
        let client = Client::builder()
            .timeout(Duration::from_millis(timeout_ms.max(1_000)))
            .build()
            .context("failed to build http client")?;
        let url = endpoint_url(&profile.base_url, "models");
        let response = match client.get(url).bearer_auth(profile.api_key.trim()).send() {
            Ok(response) => response,
            Err(error) => {
                if error.is_timeout() {
                    bail!("连接测试超时（{} ms）", timeout_ms.max(1_000));
                }
                return Err(anyhow!("连接测试请求失败: {}", error));
            }
        };
        if !response.status().is_success() {
            let status = response.status();
            let body = response
                .text()
                .unwrap_or_else(|_| "<unreadable body>".to_string());
            match status.as_u16() {
                401 => bail!("连接测试失败（401）：API Key 无效"),
                429 => bail!("连接测试失败（429）：请求过于频繁"),
                _ => bail!("连接测试失败（{}）: {}", status, body),
            }
        }
        Ok(())
    }
}
impl OcrProvider for BaiduOcrProvider {
    fn recognize_raw(
        &self,
        profile: &OcrProfile,
        request: &OcrRecognizeRequest,
    ) -> Result<OcrResultRaw> {
        let client = Client::builder()
            .timeout(Duration::from_millis(request.timeout_ms.max(1_000)))
            .build()
            .context("failed to build http client")?;

        let access_token = fetch_baidu_access_token(&client, profile, request.timeout_ms)?;
        let url = baidu_ocr_url(profile, &access_token);
        let image_base64 = general_purpose::STANDARD.encode(&request.image_png);
        let mut form_fields = vec![
            ("image".to_string(), image_base64),
            ("detect_direction".to_string(), "true".to_string()),
            ("probability".to_string(), "true".to_string()),
        ];
        if let Some(language_hint) = request.language_hint.as_deref() {
            let language_hint = language_hint.trim();
            if !language_hint.is_empty() {
                form_fields.push(("language_type".to_string(), language_hint.to_string()));
            }
        }

        let response = match client.post(url).form(&form_fields).send() {
            Ok(response) => response,
            Err(error) => {
                if error.is_timeout() {
                    bail!("百度 OCR 请求超时（{} ms）", request.timeout_ms.max(1_000));
                }
                return Err(anyhow!("百度 OCR 请求失败: {}", error));
            }
        };

        let status = response.status();
        let body = response
            .text()
            .context("failed to read baidu ocr response body")?;
        if !status.is_success() {
            bail!(
                "百度 OCR 请求失败（{}）: {}",
                status,
                baidu_error_summary(&body)
            );
        }

        parse_baidu_ocr_response(&body)
    }

    fn test_connection(&self, profile: &OcrProfile, timeout_ms: u64) -> Result<()> {
        let client = Client::builder()
            .timeout(Duration::from_millis(timeout_ms.max(1_000)))
            .build()
            .context("failed to build http client")?;
        fetch_baidu_access_token(&client, profile, timeout_ms).map(|_| ())
    }
}

fn build_chat_payload(
    profile: &OcrProfile,
    request: &OcrRecognizeRequest,
    image_base64: &str,
) -> Value {
    if is_deepseek_ocr_model(profile) {
        serde_json::json!({
            "model": profile.model,
            "temperature": 0,
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "image_url",
                            "image_url": {
                                "url": format!("data:image/png;base64,{}", image_base64),
                            }
                        },
                        {
                            "type": "text",
                            "text": USER_PROMPT_DEEPSEEK_GROUNDING,
                        }
                    ]
                }
            ]
        })
    } else {
        let lang_hint = request
            .language_hint
            .as_deref()
            .filter(|hint| !hint.trim().is_empty())
            .unwrap_or("auto");
        let user_prompt = format!("{} Language hint: {}.", USER_PROMPT_JSON, lang_hint);
        serde_json::json!({
            "model": profile.model,
            "temperature": 0,
            "messages": [
                {
                    "role": "system",
                    "content": SYSTEM_PROMPT,
                },
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "text",
                            "text": user_prompt,
                        },
                        {
                            "type": "image_url",
                            "image_url": {
                                "url": format!("data:image/png;base64,{}", image_base64),
                            }
                        }
                    ]
                }
            ]
        })
    }
}

fn is_deepseek_ocr_model(profile: &OcrProfile) -> bool {
    let model = profile.model.to_ascii_lowercase();
    if model.contains("deepseek") && model.contains("ocr") {
        return true;
    }
    let base = profile.base_url.to_ascii_lowercase();
    base.contains("deepseek") && model.contains("ocr")
}
pub fn recognize_with_profile(
    profile: &OcrProfile,
    request: &OcrRecognizeRequest,
    image_width: u32,
    image_height: u32,
) -> Result<OcrResult> {
    let provider = provider_for(profile.provider_kind);
    let raw = provider.recognize_raw(profile, request)?;
    Ok(normalize_result(
        raw,
        profile.bbox_scale_mode,
        image_width,
        image_height,
    ))
}

pub fn test_profile(profile: &OcrProfile, timeout_ms: u64) -> Result<()> {
    let provider = provider_for(profile.provider_kind);
    provider.test_connection(profile, timeout_ms)
}

fn provider_for(kind: OcrProviderKind) -> Box<dyn OcrProvider + Send + Sync> {
    match kind {
        OcrProviderKind::OpenAiCompatible => Box::<OpenAiCompatibleProvider>::default(),
        OcrProviderKind::BaiduOcr => Box::<BaiduOcrProvider>::default(),
    }
}

fn fetch_baidu_access_token(
    client: &Client,
    profile: &OcrProfile,
    timeout_ms: u64,
) -> Result<String> {
    let url = endpoint_url(&profile.base_url, "oauth/2.0/token");
    let response = match client
        .post(url)
        .query(&[
            ("grant_type", "client_credentials"),
            ("client_id", profile.api_key.trim()),
            ("client_secret", profile.secret_key.trim()),
        ])
        .send()
    {
        Ok(response) => response,
        Err(error) => {
            if error.is_timeout() {
                bail!("百度 OCR 鉴权超时（{} ms）", timeout_ms.max(1_000));
            }
            return Err(anyhow!("百度 OCR 鉴权失败: {}", error));
        }
    };

    let status = response.status();
    let body = response
        .text()
        .context("failed to read baidu auth response body")?;
    if !status.is_success() {
        bail!(
            "百度 OCR 鉴权失败（{}）: {}",
            status,
            baidu_error_summary(&body)
        );
    }

    let parsed: BaiduAccessTokenResponse =
        serde_json::from_str(&body).context("invalid baidu auth response")?;
    if !parsed.access_token.trim().is_empty() {
        return Ok(parsed.access_token);
    }
    bail!("百度 OCR 鉴权失败: {}", baidu_error_summary(&body))
}

fn baidu_ocr_url(profile: &OcrProfile, access_token: &str) -> String {
    let path = profile.model.trim();
    let endpoint = if path.starts_with("http://") || path.starts_with("https://") {
        path.to_string()
    } else if path.contains("rest/2.0/ocr/") {
        endpoint_url(&profile.base_url, path)
    } else {
        endpoint_url(&profile.base_url, &format!("rest/2.0/ocr/v1/{}", path))
    };

    if endpoint.contains('?') {
        format!("{}&access_token={}", endpoint, access_token)
    } else {
        format!("{}?access_token={}", endpoint, access_token)
    }
}

fn baidu_error_summary(body: &str) -> String {
    let Ok(value) = serde_json::from_str::<Value>(body) else {
        return body.trim().to_string();
    };

    let code = value
        .get("error_code")
        .and_then(Value::as_i64)
        .map(|code| code.to_string())
        .or_else(|| {
            value
                .get("error")
                .and_then(Value::as_str)
                .map(str::to_string)
        });
    let message = value
        .get("error_msg")
        .and_then(Value::as_str)
        .or_else(|| value.get("error_description").and_then(Value::as_str))
        .unwrap_or(body.trim());

    match code {
        Some(code) if !code.is_empty() => format!("{}: {}", code, message),
        _ => message.to_string(),
    }
}
fn normalize_result(
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

pub fn normalize_bbox(
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

fn endpoint_url(base_url: &str, path: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    format!("{}/{}", trimmed, path.trim_start_matches('/'))
}

#[derive(Debug, Deserialize)]
struct BaiduAccessTokenResponse {
    #[serde(default)]
    access_token: String,
}

#[derive(Debug, Deserialize)]
struct BaiduOcrResponse {
    #[serde(default)]
    words_result: Vec<BaiduOcrWord>,
    error_code: Option<i64>,
    error_msg: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BaiduOcrWord {
    #[serde(default)]
    words: String,
    location: Option<BaiduOcrLocation>,
    probability: Option<BaiduOcrProbability>,
}

#[derive(Debug, Deserialize)]
struct BaiduOcrLocation {
    left: f32,
    top: f32,
    width: f32,
    height: f32,
}

#[derive(Debug, Deserialize)]
struct BaiduOcrProbability {
    average: Option<f32>,
}

fn parse_baidu_ocr_response(body: &str) -> Result<OcrResultRaw> {
    let response: BaiduOcrResponse =
        serde_json::from_str(body).context("invalid baidu ocr response")?;
    if let Some(code) = response.error_code {
        let message = response.error_msg.unwrap_or_else(|| "未知错误".to_string());
        bail!("百度 OCR 识别失败（{}）：{}", code, message);
    }

    let mut full_text_parts = Vec::new();
    let mut blocks = Vec::new();
    for item in response.words_result {
        let text = item.words.trim().to_string();
        if text.is_empty() {
            continue;
        }
        full_text_parts.push(text.clone());
        if let Some(location) = item.location {
            blocks.push(OcrTextBlockRaw {
                text,
                bbox_raw: [
                    location.left,
                    location.top,
                    location.left + location.width,
                    location.top + location.height,
                ],
                confidence: item.probability.and_then(|probability| probability.average),
            });
        }
    }

    Ok(OcrResultRaw {
        full_text: full_text_parts.join("\n"),
        blocks,
    })
}
#[derive(Debug, Deserialize)]
struct ParsedOcrPayload {
    #[serde(default)]
    full_text: String,
    #[serde(default)]
    blocks: Vec<ParsedOcrBlock>,
}

#[derive(Debug, Deserialize)]
struct ParsedOcrBlock {
    #[serde(default)]
    text: String,
    #[serde(default)]
    bbox: Value,
    confidence: Option<f32>,
}

fn parse_chat_completion_ocr_response(body: &str) -> Result<OcrResultRaw> {
    let value: Value = serde_json::from_str(body).context("invalid json response")?;
    let content = extract_content_string(&value).unwrap_or_default();
    if content.trim().is_empty() {
        return Err(anyhow!("empty OCR response content"));
    }

    let parsed_payload = match extract_json_object(&content) {
        Some(json_text) => serde_json::from_str::<ParsedOcrPayload>(&json_text).ok(),
        None => None,
    };

    if let Some(payload) = parsed_payload {
        let blocks = payload
            .blocks
            .into_iter()
            .filter_map(|block| {
                let bbox_raw = parse_bbox_value(&block.bbox)?;
                Some(OcrTextBlockRaw {
                    text: block.text,
                    bbox_raw,
                    confidence: block.confidence,
                })
            })
            .collect::<Vec<_>>();

        let full_text = if payload.full_text.trim().is_empty() {
            content.trim().to_string()
        } else {
            payload.full_text
        };

        return Ok(OcrResultRaw { full_text, blocks });
    }

    let tagged_blocks = parse_tagged_ocr_blocks(&content);
    if !tagged_blocks.is_empty() {
        let full_text = tagged_blocks
            .iter()
            .map(|block| block.text.trim())
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        return Ok(OcrResultRaw {
            full_text: if full_text.trim().is_empty() {
                content.trim().to_string()
            } else {
                full_text
            },
            blocks: tagged_blocks,
        });
    }

    Ok(OcrResultRaw {
        full_text: content.trim().to_string(),
        blocks: Vec::new(),
    })
}

fn extract_content_string(value: &Value) -> Option<String> {
    let content = value
        .get("choices")?
        .as_array()?
        .first()?
        .get("message")?
        .get("content")?;

    if let Some(text) = content.as_str() {
        return Some(text.to_string());
    }

    if let Some(parts) = content.as_array() {
        let mut combined = String::new();
        for part in parts {
            if let Some(text) = part.get("text").and_then(Value::as_str) {
                if !combined.is_empty() {
                    combined.push('\n');
                }
                combined.push_str(text);
            }
        }
        if !combined.is_empty() {
            return Some(combined);
        }
    }

    None
}

fn extract_json_object(content: &str) -> Option<String> {
    if let Some(stripped) = content.strip_prefix("```") {
        let stripped = stripped.trim();
        let stripped = stripped.strip_prefix("json").unwrap_or(stripped).trim();
        let stripped = stripped.strip_suffix("```").unwrap_or(stripped).trim();
        if stripped.starts_with('{') && stripped.ends_with('}') {
            return Some(stripped.to_string());
        }
    }

    let start = content.find('{')?;
    let end = content.rfind('}')?;
    if end <= start {
        return None;
    }
    Some(content[start..=end].to_string())
}

fn parse_bbox_value(value: &Value) -> Option<[f32; 4]> {
    if let Some(array) = value.as_array() {
        if array.len() >= 4 {
            return Some([
                array[0].as_f64()? as f32,
                array[1].as_f64()? as f32,
                array[2].as_f64()? as f32,
                array[3].as_f64()? as f32,
            ]);
        }
    }

    if let Some(object) = value.as_object() {
        let x1 = object.get("x1").and_then(Value::as_f64)? as f32;
        let y1 = object.get("y1").and_then(Value::as_f64)? as f32;
        let x2 = object.get("x2").and_then(Value::as_f64)? as f32;
        let y2 = object.get("y2").and_then(Value::as_f64)? as f32;
        return Some([x1, y1, x2, y2]);
    }

    None
}

fn parse_tagged_ocr_blocks(content: &str) -> Vec<OcrTextBlockRaw> {
    let patterns = [
        ("<|ref|>", "<|/ref|>", "<|det|>", "<|/det|>"),
        ("<ref>", "</ref>", "<box>", "</box>"),
        ("<ref>", "</ref>", "<det>", "</det>"),
    ];

    for (ref_start, ref_end, det_start, det_end) in patterns {
        let blocks = extract_tagged_block_pairs(content, ref_start, ref_end, det_start, det_end);
        if !blocks.is_empty() {
            return blocks;
        }
    }

    Vec::new()
}

fn extract_tagged_block_pairs(
    content: &str,
    ref_start: &str,
    ref_end: &str,
    det_start: &str,
    det_end: &str,
) -> Vec<OcrTextBlockRaw> {
    let mut blocks = Vec::new();
    let mut cursor = 0usize;

    while let Some(ref_pos_rel) = content[cursor..].find(ref_start) {
        let ref_start_index = cursor + ref_pos_rel + ref_start.len();
        let Some(ref_end_rel) = content[ref_start_index..].find(ref_end) else {
            break;
        };
        let ref_end_index = ref_start_index + ref_end_rel;
        let text = content[ref_start_index..ref_end_index].trim();
        let after_ref = ref_end_index + ref_end.len();

        let det_rel_opt = content[after_ref..].find(det_start);
        let next_ref_rel = content[after_ref..].find(ref_start);
        let Some(det_rel) = det_rel_opt else {
            cursor = after_ref;
            continue;
        };
        if let Some(next_ref_rel) = next_ref_rel {
            if det_rel > next_ref_rel {
                cursor = after_ref;
                continue;
            }
        }

        let det_start_index = after_ref + det_rel + det_start.len();
        let Some(det_end_rel) = content[det_start_index..].find(det_end) else {
            break;
        };
        let det_end_index = det_start_index + det_end_rel;

        if !text.is_empty() {
            if let Some(bbox_raw) = parse_bbox_from_text(&content[det_start_index..det_end_index]) {
                blocks.push(OcrTextBlockRaw {
                    text: text.to_string(),
                    bbox_raw,
                    confidence: None,
                });
            }
        }

        cursor = det_end_index + det_end.len();
    }

    blocks
}

fn parse_bbox_from_text(input: &str) -> Option<[f32; 4]> {
    let numbers = parse_numbers_from_text(input);
    if numbers.len() < 4 {
        return None;
    }

    if numbers.len() >= 8 && numbers.len() % 2 == 0 {
        let mut min_x = f32::INFINITY;
        let mut min_y = f32::INFINITY;
        let mut max_x = f32::NEG_INFINITY;
        let mut max_y = f32::NEG_INFINITY;
        for pair in numbers.chunks_exact(2) {
            min_x = min_x.min(pair[0]);
            min_y = min_y.min(pair[1]);
            max_x = max_x.max(pair[0]);
            max_y = max_y.max(pair[1]);
        }
        if min_x.is_finite() && min_y.is_finite() && max_x.is_finite() && max_y.is_finite() {
            return Some([min_x, min_y, max_x, max_y]);
        }
    }

    Some([numbers[0], numbers[1], numbers[2], numbers[3]])
}

fn parse_numbers_from_text(input: &str) -> Vec<f32> {
    let mut numbers = Vec::new();
    let mut token = String::new();

    for ch in input.chars() {
        if ch.is_ascii_digit() || matches!(ch, '.' | '-' | '+' | 'e' | 'E') {
            token.push(ch);
        } else if !token.is_empty() {
            if let Ok(value) = token.parse::<f32>() {
                numbers.push(value);
            }
            token.clear();
        }
    }

    if !token.is_empty() {
        if let Ok(value) = token.parse::<f32>() {
            numbers.push(value);
        }
    }

    numbers
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_zero_to_one_with_clamp_and_swap() {
        let bbox = normalize_bbox([0.8, -0.2, 0.1, 1.4], OcrBboxScaleMode::ZeroToOne, 100, 100);
        assert_eq!(bbox, [0.1, 0.0, 0.8, 1.0]);
    }

    #[test]
    fn normalizes_zero_to_999() {
        let bbox = normalize_bbox(
            [100.0, 200.0, 500.0, 700.0],
            OcrBboxScaleMode::ZeroTo999,
            100,
            100,
        );
        assert!((bbox[0] - 100.0 / 999.0).abs() < 0.0001);
        assert!((bbox[3] - 700.0 / 999.0).abs() < 0.0001);
    }

    #[test]
    fn normalizes_zero_to_1000() {
        let bbox = normalize_bbox(
            [100.0, 200.0, 500.0, 700.0],
            OcrBboxScaleMode::ZeroTo1000,
            100,
            100,
        );
        assert!((bbox[1] - 0.2).abs() < 0.0001);
        assert!((bbox[2] - 0.5).abs() < 0.0001);
    }

    #[test]
    fn normalizes_pixel_absolute() {
        let bbox = normalize_bbox(
            [50.0, 25.0, 150.0, 75.0],
            OcrBboxScaleMode::PixelAbsolute,
            200,
            100,
        );
        assert_eq!(bbox, [0.25, 0.25, 0.75, 0.75]);
    }

    #[test]
    fn parses_deepseek_grounding_blocks() {
        let body = serde_json::json!({
            "choices": [
                {
                    "message": {
                        "content": "<|ref|>标题<|/ref|><|det|>[[100,200,300,400]]<|/det|>\n<|ref|>正文<|/ref|><|det|>[[500,600],[700,800]]<|/det|>"
                    }
                }
            ]
        })
        .to_string();

        let parsed = parse_chat_completion_ocr_response(&body).expect("parse grounding response");
        assert_eq!(parsed.blocks.len(), 2);
        assert_eq!(parsed.blocks[0].text, "标题");
        assert_eq!(parsed.blocks[0].bbox_raw, [100.0, 200.0, 300.0, 400.0]);
        assert_eq!(parsed.blocks[1].text, "正文");
        assert_eq!(parsed.blocks[1].bbox_raw, [500.0, 600.0, 700.0, 800.0]);
        assert_eq!(parsed.full_text, "标题\n正文");
    }

    #[test]
    fn parses_polygon_bbox_text_as_bounds() {
        let bbox =
            parse_bbox_from_text("[[10,20],[30,20],[30,40],[10,40]]").expect("parse polygon");
        assert_eq!(bbox, [10.0, 20.0, 30.0, 40.0]);
    }

    #[test]
    fn detects_deepseek_ocr_model_name() {
        let profile = OcrProfile {
            id: "p".to_string(),
            display_name: "deepseek".to_string(),
            provider_kind: OcrProviderKind::OpenAiCompatible,
            base_url: "https://api.deepseek.com/v1".to_string(),
            api_key: "k".to_string(),
            secret_key: String::new(),
            model: "deepseek-ocr".to_string(),
            bbox_scale_mode: OcrBboxScaleMode::ZeroTo1000,
        };
        assert!(is_deepseek_ocr_model(&profile));
    }

    #[test]
    fn parses_baidu_ocr_response_blocks() {
        let body = serde_json::json!({
            "words_result": [
                {
                    "words": "OpenCapt",
                    "location": { "left": 10.0, "top": 20.0, "width": 80.0, "height": 18.0 },
                    "probability": { "average": 0.98 }
                },
                {
                    "words": "百度OCR",
                    "location": { "left": 12.0, "top": 50.0, "width": 60.0, "height": 16.0 }
                }
            ]
        })
        .to_string();

        let parsed = parse_baidu_ocr_response(&body).expect("parse baidu ocr response");
        assert_eq!(parsed.full_text, "OpenCapt\n百度OCR");
        assert_eq!(parsed.blocks.len(), 2);
        assert_eq!(parsed.blocks[0].bbox_raw, [10.0, 20.0, 90.0, 38.0]);
        assert_eq!(parsed.blocks[0].confidence, Some(0.98));
        assert_eq!(parsed.blocks[1].bbox_raw, [12.0, 50.0, 72.0, 66.0]);
    }
}
