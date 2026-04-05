mod parallel;
mod parse;
mod providers;

use crate::config::{TranslationProfile, TranslationProviderKind};
use anyhow::{Result, bail};

use self::providers::{BaiduImageTranslationProvider, OpenAiCompatibleTranslationProvider};

pub use crate::config::DEFAULT_TRANSLATION_PROMPT_TEMPLATE as DEFAULT_PROMPT_TEMPLATE;

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

pub fn translate_blocks_parallel(
    profile: &TranslationProfile,
    texts: &[String],
    timeout_ms: u64,
) -> Result<Vec<String>> {
    parallel::translate_blocks_parallel(profile, texts, timeout_ms)
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

fn endpoint_url(base_url: &str, path: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');
    format!("{}/{}", trimmed, path.trim_start_matches('/'))
}
