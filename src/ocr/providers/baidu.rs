use super::super::parse::parse_baidu_ocr_response;
use super::super::{OcrProfile, OcrProvider, OcrRecognizeRequest, OcrResultRaw, endpoint_url};
use anyhow::{Context, Result, anyhow, bail};
use base64::{Engine as _, engine::general_purpose};
use reqwest::blocking::Client;
use serde::Deserialize;
use serde_json::Value;
use std::time::Duration;

#[derive(Default)]
pub(in crate::ocr) struct BaiduOcrProvider;

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

#[derive(Debug, Deserialize)]
struct BaiduAccessTokenResponse {
    #[serde(default)]
    access_token: String,
}
