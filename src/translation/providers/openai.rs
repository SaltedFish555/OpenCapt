use super::super::parse::{extract_content_string, parse_single_translation_content};
use super::super::{
    DEFAULT_PROMPT_TEMPLATE, TranslationProfile, TranslationProvider, endpoint_url,
};
use anyhow::{Context, Result, anyhow, bail};
use reqwest::blocking::Client;
use serde_json::Value;
use std::time::Duration;

const SYSTEM_PROMPT: &str = "You are a translation engine. Translate only the provided text segment and return only the translated text. Do not return JSON, Markdown, numbering, labels, or explanations.";

#[derive(Default)]
pub(in crate::translation) struct OpenAiCompatibleTranslationProvider;

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

pub(in crate::translation) fn build_single_prompt(template: &str, text: &str) -> String {
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
