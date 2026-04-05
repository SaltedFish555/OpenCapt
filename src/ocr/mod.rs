mod normalize;
mod parse;
mod providers;

use crate::config::{OcrProfile, OcrProviderKind};
use anyhow::Result;

use self::normalize::normalize_result;
use self::providers::{BaiduOcrProvider, OpenAiCompatibleProvider};

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

fn endpoint_url(base_url: &str, path: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    format!("{}/{}", trimmed, path.trim_start_matches('/'))
}
