use crate::config::{TranslationProfile, TranslationProviderKind};
use anyhow::{Context, Result, anyhow, bail};
use reqwest::blocking::Client;
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

pub trait TranslationProvider {
    fn translate_one(
        &self,
        profile: &TranslationProfile,
        text: &str,
        timeout_ms: u64,
    ) -> Result<String>;

    fn test_connection(&self, profile: &TranslationProfile, timeout_ms: u64) -> Result<()>;
}

#[derive(Default)]
pub struct OpenAiCompatibleTranslationProvider;

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

pub fn test_profile(profile: &TranslationProfile, timeout_ms: u64) -> Result<()> {
    let provider = provider_for(profile.provider_kind);
    provider.test_connection(profile, timeout_ms)
}

fn provider_for(kind: TranslationProviderKind) -> Box<dyn TranslationProvider + Send + Sync> {
    match kind {
        TranslationProviderKind::OpenAiCompatible => {
            Box::<OpenAiCompatibleTranslationProvider>::default()
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
}
