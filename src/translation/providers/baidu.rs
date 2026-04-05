use super::super::parse::parse_baidu_image_translation_response;
use super::super::{
    ImageTranslateRequest, ImageTranslationResult, TranslationProfile, TranslationProvider,
    endpoint_url,
};
use anyhow::{Context, Result, anyhow, bail};
use reqwest::blocking::{Client, multipart};
use serde::Deserialize;
use serde_json::Value;
use std::time::Duration;

#[derive(Default)]
pub(in crate::translation) struct BaiduImageTranslationProvider;

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
