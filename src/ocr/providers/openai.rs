use super::super::parse::parse_chat_completion_ocr_response;
use super::super::{OcrProfile, OcrProvider, OcrRecognizeRequest, OcrResultRaw, endpoint_url};
use anyhow::{Context, Result, anyhow, bail};
use base64::{Engine as _, engine::general_purpose};
use reqwest::blocking::Client;
use serde_json::Value;
use std::time::Duration;

const SYSTEM_PROMPT: &str = "You are an OCR engine. Return strict JSON only.";
const USER_PROMPT_JSON: &str = "Recognize all text from the image. Return JSON with shape: {\"full_text\": string, \"blocks\": [{\"text\": string, \"bbox\": [x1,y1,x2,y2], \"confidence\": number|null}]}. bbox must be in your native coordinate scale.";
const USER_PROMPT_DEEPSEEK_GROUNDING: &str = "<image>\\n<|grounding|>OCR this image.";

#[derive(Default)]
pub(in crate::ocr) struct OpenAiCompatibleProvider;

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
#[cfg(test)]
mod tests {
    use super::is_deepseek_ocr_model;
    use crate::config::{OcrBboxScaleMode, OcrProfile, OcrProviderKind};

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
}
