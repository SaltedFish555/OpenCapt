use crate::config::{TranslationProfile, TranslationProviderKind};
use anyhow::{Context, Result, anyhow, bail};
use base64::{Engine as _, engine::general_purpose};
use reqwest::blocking::{Client, multipart};
use serde::Deserialize;
use serde_json::Value;
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};
use tracing::warn;

const SYSTEM_PROMPT: &str = "You are a translation engine. Translate only the provided text segment and return only the translated text. Do not return JSON, Markdown, numbering, labels, or explanations.";
pub const DEFAULT_PROMPT_TEMPLATE: &str = "Translate the following text into Chinese. Return only the translated text without explanation.\n{{text}}";
const MAX_PARALLEL_REQUESTS: usize = 4;

#[derive(Debug, Clone)]
pub struct ImageTranslateRequest {
    pub image_png: Vec<u8>,
    pub image_width: u32,
    pub image_height: u32,
    pub timeout_ms: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TranslationBlock {
    pub source_text: String,
    pub translated_text: String,
    pub bbox_norm: [f32; 4],
}

#[derive(Debug, Clone, PartialEq)]
pub struct ImageTranslationResult {
    pub source_full_text: String,
    pub translated_full_text: String,
    pub blocks: Vec<TranslationBlock>,
    pub pasted_image: Option<Vec<u8>>,
}

pub trait TranslationProvider {
    fn translate_one(
        &self,
        profile: &TranslationProfile,
        text: &str,
        timeout_ms: u64,
    ) -> Result<String>;

    fn translate_image(
        &self,
        _profile: &TranslationProfile,
        _request: &ImageTranslateRequest,
    ) -> Result<ImageTranslationResult> {
        bail!("当前翻译模型不支持图片翻译")
    }

    fn test_connection(&self, profile: &TranslationProfile, timeout_ms: u64) -> Result<()>;
}

#[derive(Default)]
pub struct OpenAiCompatibleTranslationProvider;

#[derive(Default)]
pub struct BaiduImageTranslationProvider;

impl TranslationProvider for OpenAiCompatibleTranslationProvider {
    fn translate_one(
        &self,
        profile: &TranslationProfile,
        text: &str,
        timeout_ms: u64,
    ) -> Result<String> {
        let prompt = build_single_prompt(&profile.prompt_template, text);
        let payload = serde_json::json!({
            "model": profile.model,
            "temperature": 0,
            "messages": [
                { "role": "system", "content": SYSTEM_PROMPT },
                { "role": "user", "content": prompt }
            ]
        });

        let content = send_chat_completion(profile, payload, timeout_ms)?;
        parse_single_translation_content(&content)
    }

    fn test_connection(&self, profile: &TranslationProfile, timeout_ms: u64) -> Result<()> {
        let client = Client::builder()
            .timeout(Duration::from_millis(timeout_ms.max(1_000)))
            .build()
            .context("failed to build http client")?;
        let url = endpoint_url(&profile.base_url, "models");
        let response = match client.get(url).bearer_auth(profile.api_key.trim()).send() {
            Ok(response) => response,
            Err(error) => {
                if error.is_timeout() {
                    bail!("翻译连接测试超时（{} ms）", timeout_ms.max(1_000));
                }
                return Err(anyhow!("翻译连接测试请求失败: {}", error));
            }
        };

        if !response.status().is_success() {
            let status = response.status();
            let body = response
                .text()
                .unwrap_or_else(|_| "<unreadable body>".to_string());
            match status.as_u16() {
                401 => bail!("翻译连接测试失败（401）：API Key 无效"),
                429 => bail!("翻译连接测试失败（429）：请求过于频繁"),
                _ => bail!("翻译连接测试失败（{}）: {}", status, body),
            }
        }
        Ok(())
    }
}

impl TranslationProvider for BaiduImageTranslationProvider {
    fn translate_one(
        &self,
        _profile: &TranslationProfile,
        _text: &str,
        _timeout_ms: u64,
    ) -> Result<String> {
        bail!("百度图片翻译不支持纯文本块翻译")
    }

    fn translate_image(
        &self,
        profile: &TranslationProfile,
        request: &ImageTranslateRequest,
    ) -> Result<ImageTranslationResult> {
        let client = Client::builder()
            .timeout(Duration::from_millis(request.timeout_ms.max(1_000)))
            .build()
            .context("failed to build http client")?;
        let access_token = fetch_baidu_access_token(&client, profile, request.timeout_ms)?;
        let url = baidu_pictrans_url(profile, &access_token);
        let image_part = multipart::Part::bytes(request.image_png.clone())
            .file_name("capture.png")
            .mime_str("image/png")
            .context("failed to prepare png upload")?;
        let paste_value = if profile.use_translated_image {
            "1"
        } else {
            "0"
        };
        let form = multipart::Form::new()
            .text("from", normalize_baidu_language(&profile.source_lang))
            .text("to", normalize_baidu_language(&profile.target_lang))
            .text("paste", paste_value.to_string())
            .part("image", image_part);

        let response = match client.post(url).multipart(form).send() {
            Ok(response) => response,
            Err(error) => {
                if error.is_timeout() {
                    bail!(
                        "百度图片翻译请求超时（{} ms）",
                        request.timeout_ms.max(1_000)
                    );
                }
                return Err(anyhow!("百度图片翻译请求失败: {}", error));
            }
        };

        let status = response.status();
        let body = response
            .text()
            .context("failed to read baidu image translation response body")?;
        if !status.is_success() {
            bail!(
                "百度图片翻译请求失败（{}）: {}",
                status,
                baidu_error_summary(&body)
            );
        }

        parse_baidu_image_translation_response(
            &body,
            request.image_width,
            request.image_height,
            profile.use_translated_image,
        )
    }

    fn test_connection(&self, profile: &TranslationProfile, timeout_ms: u64) -> Result<()> {
        let client = Client::builder()
            .timeout(Duration::from_millis(timeout_ms.max(1_000)))
            .build()
            .context("failed to build http client")?;
        fetch_baidu_access_token(&client, profile, timeout_ms).map(|_| ())
    }
}

pub fn translate_blocks_parallel(
    profile: &TranslationProfile,
    texts: &[String],
    timeout_ms: u64,
) -> Result<Vec<String>> {
    if texts.is_empty() {
        return Ok(Vec::new());
    }

    let worker_count = MAX_PARALLEL_REQUESTS.min(texts.len());
    let queue = Arc::new(Mutex::new((0..texts.len()).collect::<VecDeque<_>>()));
    let outputs = Arc::new(Mutex::new(vec![None::<String>; texts.len()]));
    let errors = Arc::new(Mutex::new(Vec::<String>::new()));
    let source_texts = Arc::new(texts.to_vec());

    let mut handles = Vec::with_capacity(worker_count);
    for _ in 0..worker_count {
        let queue = Arc::clone(&queue);
        let outputs = Arc::clone(&outputs);
        let errors = Arc::clone(&errors);
        let source_texts = Arc::clone(&source_texts);
        let profile = profile.clone();

        handles.push(thread::spawn(move || {
            let provider = provider_for(profile.provider_kind);
            loop {
                let next_index = {
                    let Ok(mut q) = queue.lock() else {
                        return;
                    };
                    q.pop_front()
                };
                let Some(index) = next_index else {
                    break;
                };

                let source = source_texts[index].clone();
                match provider.translate_one(&profile, &source, timeout_ms) {
                    Ok(translated) => {
                        let value = normalize_translated_output(&translated, &source);
                        if let Ok(mut out) = outputs.lock() {
                            out[index] = Some(value);
                        }
                    }
                    Err(error) => {
                        if let Ok(mut errs) = errors.lock() {
                            errs.push(format!("块 #{} 失败: {}", index, error));
                        }
                        if let Ok(mut out) = outputs.lock() {
                            out[index] = Some(source);
                        }
                    }
                }
            }
        }));
    }

    for handle in handles {
        if handle.join().is_err() {
            bail!("翻译线程异常退出");
        }
    }

    let error_messages = errors.lock().map(|errs| errs.clone()).unwrap_or_default();
    if error_messages.len() == texts.len() && !error_messages.is_empty() {
        bail!("所有文本块翻译失败；{}", error_messages[0]);
    }
    if !error_messages.is_empty() {
        warn!(
            failed = error_messages.len(),
            total = texts.len(),
            first_error = %error_messages[0],
            "partial translation failure"
        );
    }

    let result = outputs
        .lock()
        .map(|items| {
            items
                .iter()
                .enumerate()
                .map(|(index, item)| item.clone().unwrap_or_else(|| texts[index].clone()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_else(|_| texts.to_vec());
    Ok(result)
}

pub fn translate_image_with_profile(
    profile: &TranslationProfile,
    request: &ImageTranslateRequest,
) -> Result<ImageTranslationResult> {
    let provider = provider_for(profile.provider_kind);
    provider.translate_image(profile, request)
}

pub fn test_profile(profile: &TranslationProfile, timeout_ms: u64) -> Result<()> {
    let provider = provider_for(profile.provider_kind);
    provider.test_connection(profile, timeout_ms)
}

fn provider_for(kind: TranslationProviderKind) -> Box<dyn TranslationProvider + Send + Sync> {
    match kind {
        TranslationProviderKind::OpenAiCompatible => {
            Box::<OpenAiCompatibleTranslationProvider>::default()
        }
        TranslationProviderKind::BaiduImageTranslate => {
            Box::<BaiduImageTranslationProvider>::default()
        }
    }
}

fn send_chat_completion(
    profile: &TranslationProfile,
    payload: Value,
    timeout_ms: u64,
) -> Result<String> {
    let client = Client::builder()
        .timeout(Duration::from_millis(timeout_ms.max(1_000)))
        .build()
        .context("failed to build http client")?;

    let url = endpoint_url(&profile.base_url, "chat/completions");
    let response = match client
        .post(url)
        .bearer_auth(profile.api_key.trim())
        .json(&payload)
        .send()
    {
        Ok(response) => response,
        Err(error) => {
            if error.is_timeout() {
                bail!("翻译请求超时（{} ms）", timeout_ms.max(1_000));
            }
            return Err(anyhow!("翻译请求失败: {}", error));
        }
    };

    let status = response.status();
    let body = response
        .text()
        .context("failed to read translation response body")?;
    if !status.is_success() {
        match status.as_u16() {
            401 => bail!("翻译鉴权失败（401），请检查 API Key"),
            429 => bail!("翻译请求过于频繁（429），请稍后重试"),
            _ => bail!("翻译请求失败（{}）: {}", status, body),
        }
    }

    let value: Value = serde_json::from_str(&body).context("invalid json response")?;
    let content = extract_content_string(&value).unwrap_or_default();
    if content.trim().is_empty() {
        bail!("empty translation response content");
    }
    Ok(content)
}

fn fetch_baidu_access_token(
    client: &Client,
    profile: &TranslationProfile,
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
                bail!("百度图片翻译鉴权超时（{} ms）", timeout_ms.max(1_000));
            }
            return Err(anyhow!("百度图片翻译鉴权失败: {}", error));
        }
    };

    let status = response.status();
    let body = response
        .text()
        .context("failed to read baidu auth response body")?;
    if !status.is_success() {
        bail!(
            "百度图片翻译鉴权失败（{}）: {}",
            status,
            baidu_error_summary(&body)
        );
    }

    let parsed: BaiduAccessTokenResponse =
        serde_json::from_str(&body).context("invalid baidu auth response")?;
    if !parsed.access_token.trim().is_empty() {
        return Ok(parsed.access_token);
    }
    bail!("百度图片翻译鉴权失败: {}", baidu_error_summary(&body))
}

fn baidu_pictrans_url(profile: &TranslationProfile, access_token: &str) -> String {
    let path = profile.model.trim();
    let endpoint = if path.starts_with("http://") || path.starts_with("https://") {
        path.to_string()
    } else {
        endpoint_url(&profile.base_url, path)
    };

    if endpoint.contains('?') {
        format!("{}&access_token={}", endpoint, access_token)
    } else {
        format!("{}?access_token={}", endpoint, access_token)
    }
}

fn normalize_baidu_language(language: &str) -> String {
    let trimmed = language.trim();
    if trimmed.is_empty() {
        "auto".to_string()
    } else {
        trimmed.to_string()
    }
}

fn parse_baidu_image_translation_response(
    body: &str,
    image_width: u32,
    image_height: u32,
    prefer_pasted_image: bool,
) -> Result<ImageTranslationResult> {
    let value: Value =
        serde_json::from_str(body).context("invalid baidu image translation response")?;
    let data = value.get("data").unwrap_or(&value);

    let mut source_full_text = data
        .get("sumSrc")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let mut translated_full_text = data
        .get("sumDst")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();

    let mut blocks = Vec::new();
    if let Some(items) = data.get("content").and_then(Value::as_array) {
        for item in items {
            let source_text = item
                .get("src")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            let translated_text = item
                .get("dst")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            if source_text.is_empty() && translated_text.is_empty() {
                continue;
            }
            let Some(bbox_norm) = parse_baidu_bbox(
                item.get("rect"),
                item.get("points"),
                image_width,
                image_height,
            ) else {
                continue;
            };
            blocks.push(TranslationBlock {
                source_text,
                translated_text,
                bbox_norm,
            });
        }
    }

    if source_full_text.is_empty() {
        source_full_text = blocks
            .iter()
            .map(|block| block.source_text.trim())
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join(
                "
",
            );
    }
    if translated_full_text.is_empty() {
        translated_full_text = blocks
            .iter()
            .map(|block| block.translated_text.trim())
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join(
                "
",
            );
    }

    let pasted_image = if prefer_pasted_image {
        let payload = data
            .get("pasteImg")
            .and_then(Value::as_str)
            .or_else(|| value.get("pasteImg").and_then(Value::as_str))
            .unwrap_or("");
        decode_base64_image(payload).ok()
    } else {
        None
    };

    Ok(ImageTranslationResult {
        source_full_text,
        translated_full_text,
        blocks,
        pasted_image,
    })
}

fn parse_baidu_bbox(
    rect: Option<&Value>,
    points: Option<&Value>,
    image_width: u32,
    image_height: u32,
) -> Option<[f32; 4]> {
    parse_baidu_rect(rect)
        .or_else(|| parse_baidu_points(points))
        .map(|bbox| normalize_pixel_bbox(bbox, image_width, image_height))
        .filter(|bbox| (bbox[2] - bbox[0]) > 0.001 && (bbox[3] - bbox[1]) > 0.001)
}

fn parse_baidu_rect(rect: Option<&Value>) -> Option<[f32; 4]> {
    let rect = rect?;
    if let Some(text) = rect.as_str() {
        let numbers = parse_numeric_sequence(text);
        if numbers.len() >= 4 {
            return Some([
                numbers[0],
                numbers[1],
                numbers[0] + numbers[2],
                numbers[1] + numbers[3],
            ]);
        }
    }
    if let Some(items) = rect.as_array() {
        let numbers = items.iter().filter_map(value_to_f32).collect::<Vec<_>>();
        if numbers.len() >= 4 {
            return Some([
                numbers[0],
                numbers[1],
                numbers[0] + numbers[2],
                numbers[1] + numbers[3],
            ]);
        }
    }
    if let Some(obj) = rect.as_object() {
        let left = obj.get("left").and_then(value_to_f32)?;
        let top = obj.get("top").and_then(value_to_f32)?;
        let width = obj.get("width").and_then(value_to_f32)?;
        let height = obj.get("height").and_then(value_to_f32)?;
        return Some([left, top, left + width, top + height]);
    }
    None
}

fn parse_baidu_points(points: Option<&Value>) -> Option<[f32; 4]> {
    let points = points?;
    let mut xs = Vec::new();
    let mut ys = Vec::new();
    if let Some(items) = points.as_array() {
        for item in items {
            if let Some(pair) = item.as_array() {
                if pair.len() >= 2 {
                    if let (Some(x), Some(y)) = (value_to_f32(&pair[0]), value_to_f32(&pair[1])) {
                        xs.push(x);
                        ys.push(y);
                    }
                }
                continue;
            }
            if let Some(obj) = item.as_object() {
                let x = obj
                    .get("x")
                    .or_else(|| obj.get("left"))
                    .and_then(value_to_f32);
                let y = obj
                    .get("y")
                    .or_else(|| obj.get("top"))
                    .and_then(value_to_f32);
                if let (Some(x), Some(y)) = (x, y) {
                    xs.push(x);
                    ys.push(y);
                }
            }
        }
    }
    if xs.is_empty() || ys.is_empty() {
        return None;
    }
    Some([
        xs.iter().copied().fold(f32::INFINITY, f32::min),
        ys.iter().copied().fold(f32::INFINITY, f32::min),
        xs.iter().copied().fold(f32::NEG_INFINITY, f32::max),
        ys.iter().copied().fold(f32::NEG_INFINITY, f32::max),
    ])
}

fn value_to_f32(value: &Value) -> Option<f32> {
    value
        .as_f64()
        .map(|value| value as f32)
        .or_else(|| value.as_i64().map(|value| value as f32))
        .or_else(|| value.as_u64().map(|value| value as f32))
        .or_else(|| {
            value
                .as_str()
                .and_then(|text| text.trim().parse::<f32>().ok())
        })
}

fn parse_numeric_sequence(text: &str) -> Vec<f32> {
    text.split(|ch: char| !matches!(ch, '0'..='9' | '.' | '-'))
        .filter_map(|part| {
            let part = part.trim();
            if part.is_empty() {
                None
            } else {
                part.parse::<f32>().ok()
            }
        })
        .collect()
}

fn normalize_pixel_bbox(raw_bbox: [f32; 4], image_width: u32, image_height: u32) -> [f32; 4] {
    let width = image_width.max(1) as f32;
    let height = image_height.max(1) as f32;
    let mut x1 = raw_bbox[0] / width;
    let mut y1 = raw_bbox[1] / height;
    let mut x2 = raw_bbox[2] / width;
    let mut y2 = raw_bbox[3] / height;
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

fn decode_base64_image(payload: &str) -> Result<Vec<u8>> {
    let trimmed = payload.trim();
    if trimmed.is_empty() {
        bail!("empty pasted image payload");
    }
    let encoded = trimmed
        .split_once(',')
        .map(|(_, tail)| tail)
        .unwrap_or(trimmed)
        .trim();
    general_purpose::STANDARD
        .decode(encoded)
        .or_else(|_| general_purpose::STANDARD_NO_PAD.decode(encoded))
        .context("invalid base64 pasted image")
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

fn normalize_translated_output(translated: &str, source: &str) -> String {
    let trimmed = translated.trim();
    if trimmed.is_empty() {
        source.to_string()
    } else {
        trimmed.to_string()
    }
}

fn build_single_prompt(template: &str, text: &str) -> String {
    if should_force_plain_text_prompt(template) {
        return DEFAULT_PROMPT_TEMPLATE.replace("{{text}}", text);
    }
    if template.contains("{{text}}") {
        return template.replace("{{text}}", text);
    }
    if template.contains("{{texts}}") {
        return template.replace("{{texts}}", text);
    }
    format!("{}\n\n{}", template.trim(), text)
}

fn should_force_plain_text_prompt(template: &str) -> bool {
    let lower = template.to_ascii_lowercase();
    lower.contains("json")
        || lower.contains("\"translations\"")
        || (lower.contains("translations") && lower.contains("index"))
}

fn parse_single_translation_content(content: &str) -> Result<String> {
    if let Some(parsed) = parse_json_translation_content(content) {
        return Ok(parsed);
    }
    if let Some(parsed) = parse_loose_json_line(content) {
        return Ok(parsed);
    }

    let stripped = strip_code_fence(content).trim().to_string();
    if stripped.is_empty() {
        bail!("empty translation content");
    }

    if stripped.starts_with('"')
        && stripped.ends_with('"')
        && let Ok(decoded) = serde_json::from_str::<String>(&stripped)
    {
        let normalized = decoded.trim();
        if !normalized.is_empty() {
            return Ok(normalized.to_string());
        }
    }

    let normalized = strip_line_prefix(&stripped);
    if normalized.is_empty() {
        bail!("empty translation content");
    }
    Ok(normalized)
}

fn parse_json_translation_content(content: &str) -> Option<String> {
    let json_text = extract_json_payload(content)?;
    let value: Value = serde_json::from_str(&json_text).ok()?;
    extract_text_from_json_value(&value)
}

fn extract_text_from_json_value(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => normalized_non_empty(text),
        Value::Array(items) => {
            let texts = items
                .iter()
                .filter_map(extract_text_from_json_value)
                .collect::<Vec<_>>();
            if texts.is_empty() {
                None
            } else {
                Some(texts.join("\n"))
            }
        }
        Value::Object(map) => {
            if let Some(translations) = map.get("translations") {
                return extract_text_from_json_value(translations);
            }
            for key in [
                "text",
                "translation",
                "translated_text",
                "target_text",
                "value",
                "content",
                "output_text",
            ] {
                if let Some(text) = map.get(key).and_then(Value::as_str)
                    && let Some(normalized) = normalized_non_empty(text)
                {
                    return Some(normalized);
                }
            }
            None
        }
        _ => None,
    }
}

fn normalized_non_empty(text: &str) -> Option<String> {
    let normalized = strip_line_prefix(text.trim());
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn parse_loose_json_line(content: &str) -> Option<String> {
    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() {
            continue;
        }
        let is_text_line = line.contains("\"text\"")
            || line.contains("\"translation\"")
            || line.contains("\"translated_text\"")
            || line.contains("\"target_text\"")
            || line.contains("\"output_text\"");
        if !is_text_line {
            continue;
        }
        let raw_text = extract_quoted_field_value(line)?;
        let decoded = decode_json_like_escapes(raw_text.trim());
        if let Some(normalized) = normalized_non_empty(&decoded) {
            return Some(normalized);
        }
    }
    None
}

fn extract_quoted_field_value(line: &str) -> Option<String> {
    let colon = line.find(':')?;
    let tail = &line[colon + 1..];
    let first_quote = tail.find('"')?;
    let last_quote = tail.rfind('"')?;
    if last_quote <= first_quote {
        return None;
    }
    Some(tail[first_quote + 1..last_quote].to_string())
}

fn decode_json_like_escapes(value: &str) -> String {
    let mut decoded = String::with_capacity(value.len());
    let mut chars = value.chars();

    while let Some(ch) = chars.next() {
        if ch != '\\' {
            decoded.push(ch);
            continue;
        }

        match chars.next() {
            Some('n') => decoded.push('\n'),
            Some('r') => decoded.push('\r'),
            Some('t') => decoded.push('\t'),
            Some('"') => decoded.push('"'),
            Some('\\') => decoded.push('\\'),
            Some('/') => decoded.push('/'),
            Some('b') => decoded.push('\u{0008}'),
            Some('f') => decoded.push('\u{000C}'),
            Some('u') => {
                let hex: String = chars.by_ref().take(4).collect();
                if hex.len() == 4
                    && let Ok(code) = u32::from_str_radix(&hex, 16)
                    && let Some(unicode) = char::from_u32(code)
                {
                    decoded.push(unicode);
                    continue;
                }
                decoded.push_str("\\u");
                decoded.push_str(&hex);
            }
            Some(other) => decoded.push(other),
            None => decoded.push('\\'),
        }
    }

    decoded
}

fn strip_code_fence(content: &str) -> String {
    let trimmed = content.trim();
    if let Some(stripped) = trimmed.strip_prefix("```") {
        let stripped = stripped.trim();
        let stripped = stripped.strip_prefix("json").unwrap_or(stripped).trim();
        let stripped = stripped.strip_suffix("```").unwrap_or(stripped).trim();
        return stripped.to_string();
    }
    trimmed.to_string()
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

fn extract_json_payload(content: &str) -> Option<String> {
    let trimmed = strip_code_fence(content);
    let trimmed = trimmed.trim();
    if (trimmed.starts_with('{') && trimmed.ends_with('}'))
        || (trimmed.starts_with('[') && trimmed.ends_with(']'))
    {
        return Some(trimmed.to_string());
    }

    let object_span = trimmed
        .find('{')
        .and_then(|start| trimmed.rfind('}').map(|end| (start, end)))
        .filter(|(start, end)| end > start);
    let array_span = trimmed
        .find('[')
        .and_then(|start| trimmed.rfind(']').map(|end| (start, end)))
        .filter(|(start, end)| end > start);

    match (object_span, array_span) {
        (Some((os, oe)), Some((as_, ae))) => {
            if os <= as_ {
                Some(trimmed[os..=oe].to_string())
            } else {
                Some(trimmed[as_..=ae].to_string())
            }
        }
        (Some((start, end)), None) | (None, Some((start, end))) => {
            Some(trimmed[start..=end].to_string())
        }
        (None, None) => None,
    }
}

fn strip_line_prefix(line: &str) -> String {
    let trimmed = line.trim();
    let mut chars = trimmed.chars().peekable();
    let mut consumed = 0usize;
    while let Some(ch) = chars.peek().copied() {
        if ch.is_ascii_digit() || ch == '.' || ch == ':' || ch == ')' || ch == '-' {
            consumed += ch.len_utf8();
            chars.next();
        } else if ch.is_whitespace() {
            consumed += ch.len_utf8();
            chars.next();
        } else {
            break;
        }
    }
    trimmed[consumed..].trim().to_string()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_prompt_uses_text_placeholder() {
        let prompt = build_single_prompt("translate:\n{{text}}", "hello");
        assert_eq!(prompt, "translate:\nhello");
    }

    #[test]
    fn single_prompt_accepts_legacy_texts_placeholder() {
        let prompt = build_single_prompt("translate:\n{{texts}}", "hello");
        assert_eq!(prompt, "translate:\nhello");
    }

    #[test]
    fn legacy_json_prompt_is_replaced_with_plain_text_prompt() {
        let prompt = build_single_prompt(
            "Translate each line to Chinese. Return JSON only: {\"translations\":[{\"index\":0,\"text\":\"...\"}]}\n{{texts}}",
            "hello",
        );
        assert!(!prompt.contains("JSON only"));
        assert!(prompt.contains("hello"));
        assert!(!prompt.contains("{{text}}"));
    }

    #[test]
    fn parse_single_prefers_plain_text() {
        let text = parse_single_translation_content("你好世界").expect("single parse");
        assert_eq!(text, "你好世界");
    }

    #[test]
    fn parse_single_extracts_text_from_json_payload() {
        let text =
            parse_single_translation_content("{\"translation\":\"你好世界\"}").expect("json parse");
        assert_eq!(text, "你好世界");
    }

    #[test]
    fn parse_single_handles_invalid_json_line_with_inner_quotes() {
        let content = r#"```json
{
  "translation": "textual input such as category names (e.g., "the red car")"
}
```"#;
        let text = parse_single_translation_content(content).expect("loose parse");
        assert_eq!(
            text,
            "textual input such as category names (e.g., \"the red car\")"
        );
    }

    #[test]
    fn parse_baidu_rect_string_to_norm_bbox() {
        let value = Value::String("79 23 246 43".to_string());
        let bbox = parse_baidu_bbox(Some(&value), None, 400, 200).expect("bbox");
        assert_eq!(bbox, [0.1975, 0.115, 0.8125, 0.33]);
    }

    #[test]
    fn parse_baidu_translation_response_uses_blocks_and_paste_img() {
        let body = r#"{
  "data": {
    "sumSrc": "Grounding-DINO",
    "sumDst": "Grounding-DINO（译）",
    "pasteImg": "aGVsbG8=",
    "content": [
      {
        "src": "Overview",
        "dst": "概述",
        "rect": "10 20 30 40"
      }
    ]
  }
}"#;
        let result = parse_baidu_image_translation_response(body, 200, 100, true)
            .expect("parse baidu response");
        assert_eq!(result.source_full_text, "Grounding-DINO");
        assert_eq!(result.translated_full_text, "Grounding-DINO（译）");
        assert_eq!(result.blocks.len(), 1);
        assert_eq!(result.blocks[0].source_text, "Overview");
        assert_eq!(result.blocks[0].translated_text, "概述");
        assert_eq!(result.blocks[0].bbox_norm, [0.05, 0.2, 0.2, 0.6]);
        assert_eq!(result.pasted_image.as_deref(), Some(b"hello".as_slice()));
    }

    #[test]
    fn parse_baidu_translation_response_falls_back_to_joined_text() {
        let body = r#"{
  "data": {
    "content": [
      {
        "src": "A",
        "dst": "甲",
        "points": [[0, 0], [20, 0], [20, 10], [0, 10]]
      },
      {
        "src": "B",
        "dst": "乙",
        "rect": [30, 10, 20, 20]
      }
    ]
  }
}"#;
        let result = parse_baidu_image_translation_response(body, 100, 100, false)
            .expect("parse baidu response");
        assert_eq!(result.source_full_text, "A\nB");
        assert_eq!(result.translated_full_text, "甲\n乙");
        assert_eq!(result.blocks.len(), 2);
        assert!(result.pasted_image.is_none());
    }
}
