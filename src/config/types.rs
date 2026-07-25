use crate::hotkey;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const ANNOTATION_COLOR_PRESETS: [u32; 5] = [0xF14C4C, 0xFF8C00, 0xF2C94C, 0x2ECC71, 0x4F8CFF];
pub const MIN_STROKE_WIDTH: u32 = 1;
pub const MAX_STROKE_WIDTH: u32 = 16;
pub const DEFAULT_STROKE_WIDTH: u32 = 2;
pub const MIN_TEXT_SIZE: u32 = 14;
pub const MAX_TEXT_SIZE: u32 = 54;
pub const DEFAULT_TEXT_SIZE: u32 = 24;
pub const MIN_NUMBER_SIZE: u32 = 18;
pub const MAX_NUMBER_SIZE: u32 = 52;
pub const DEFAULT_NUMBER_SIZE: u32 = 28;
pub const MIN_MOSAIC_SIZE: u32 = 6;
pub const MAX_MOSAIC_SIZE: u32 = 30;
pub const DEFAULT_MOSAIC_SIZE: u32 = 12;
pub const PIN_OPACITY_OPTIONS: [u8; 4] = [100, 80, 60, 40];
pub const OCR_TIMEOUT_MIN_MS: u64 = 2_000;
pub const OCR_TIMEOUT_MAX_MS: u64 = 120_000;
pub const OCR_TIMEOUT_DEFAULT_MS: u64 = 20_000;
pub const TRANSLATION_TIMEOUT_MIN_MS: u64 = 2_000;
pub const TRANSLATION_TIMEOUT_MAX_MS: u64 = 120_000;
pub const TRANSLATION_TIMEOUT_DEFAULT_MS: u64 = 20_000;
pub const DEFAULT_TRANSLATION_PROMPT_TEMPLATE: &str = "Translate the following text into Chinese. Return only the translated text without explanation.\n{{text}}";

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum TextFontFamily {
    #[default]
    #[serde(rename = "yahei")]
    YaHei,
    #[serde(rename = "dengxian")]
    DengXian,
    #[serde(rename = "kaiti")]
    KaiTi,
}

impl TextFontFamily {
    pub const ALL: [Self; 3] = [Self::YaHei, Self::DengXian, Self::KaiTi];

    pub fn label(self) -> &'static str {
        match self {
            Self::YaHei => "微软雅黑",
            Self::DengXian => "等线",
            Self::KaiTi => "楷体",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum OcrProviderKind {
    #[default]
    #[serde(rename = "openai_compatible")]
    OpenAiCompatible,
    #[serde(rename = "baidu_ocr")]
    BaiduOcr,
}

impl OcrProviderKind {
    pub const ALL: [Self; 2] = [Self::OpenAiCompatible, Self::BaiduOcr];

    pub fn label(self) -> &'static str {
        match self {
            Self::OpenAiCompatible => "OpenAI Compatible",
            Self::BaiduOcr => "百度 OCR",
        }
    }

    pub fn default_base_url(self) -> &'static str {
        match self {
            Self::OpenAiCompatible => "https://api.openai.com/v1",
            Self::BaiduOcr => "https://aip.baidubce.com",
        }
    }

    pub fn default_model(self) -> &'static str {
        match self {
            Self::OpenAiCompatible => "gpt-4.1-mini",
            Self::BaiduOcr => "general",
        }
    }

    pub fn default_bbox_scale_mode(self) -> OcrBboxScaleMode {
        match self {
            Self::OpenAiCompatible => OcrBboxScaleMode::ZeroTo1000,
            Self::BaiduOcr => OcrBboxScaleMode::PixelAbsolute,
        }
    }

    pub fn model_field_label(self) -> &'static str {
        match self {
            Self::OpenAiCompatible => "模型名称",
            Self::BaiduOcr => "接口名称",
        }
    }

    pub fn model_field_hint(self) -> &'static str {
        match self {
            Self::OpenAiCompatible => "例如 gpt-4.1-mini / deepseek-ocr",
            Self::BaiduOcr => "例如 general / accurate / general_basic",
        }
    }

    pub fn uses_secret_key(self) -> bool {
        matches!(self, Self::BaiduOcr)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum TranslationProviderKind {
    #[default]
    #[serde(rename = "openai_compatible")]
    OpenAiCompatible,
    #[serde(rename = "baidu_image_translate")]
    BaiduImageTranslate,
}

impl TranslationProviderKind {
    pub const ALL: [Self; 2] = [Self::OpenAiCompatible, Self::BaiduImageTranslate];

    pub fn label(self) -> &'static str {
        match self {
            Self::OpenAiCompatible => "OpenAI Compatible",
            Self::BaiduImageTranslate => "百度图片翻译",
        }
    }

    pub fn default_base_url(self) -> &'static str {
        match self {
            Self::OpenAiCompatible => "https://api.openai.com/v1",
            Self::BaiduImageTranslate => "https://aip.baidubce.com",
        }
    }

    pub fn default_model(self) -> &'static str {
        match self {
            Self::OpenAiCompatible => "gpt-4.1-mini",
            Self::BaiduImageTranslate => "file/2.0/mt/pictrans/v1",
        }
    }

    pub fn model_field_label(self) -> &'static str {
        match self {
            Self::OpenAiCompatible => "模型名称",
            Self::BaiduImageTranslate => "接口路径",
        }
    }

    pub fn model_field_hint(self) -> &'static str {
        match self {
            Self::OpenAiCompatible => "例如 gpt-4.1-mini",
            Self::BaiduImageTranslate => "例如 file/2.0/mt/pictrans/v1",
        }
    }

    pub fn uses_secret_key(self) -> bool {
        matches!(self, Self::BaiduImageTranslate)
    }

    pub fn uses_prompt_template(self) -> bool {
        matches!(self, Self::OpenAiCompatible)
    }

    pub fn supports_image_output(self) -> bool {
        matches!(self, Self::BaiduImageTranslate)
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum OcrBboxScaleMode {
    #[default]
    #[serde(rename = "0_1")]
    ZeroToOne,
    #[serde(rename = "0_999")]
    ZeroTo999,
    #[serde(rename = "0_1000")]
    ZeroTo1000,
    #[serde(rename = "pixel")]
    PixelAbsolute,
}

impl OcrBboxScaleMode {
    pub const ALL: [Self; 4] = [
        Self::ZeroToOne,
        Self::ZeroTo999,
        Self::ZeroTo1000,
        Self::PixelAbsolute,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::ZeroToOne => "0~1",
            Self::ZeroTo999 => "0~999",
            Self::ZeroTo1000 => "0~1000",
            Self::PixelAbsolute => "像素坐标",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct GeneralConfig {
    pub hotkey: String,
    pub auto_copy: bool,
    pub auto_save: bool,
    pub launch_at_startup: bool,
    pub save_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct AnnotationDefaults {
    pub default_color_index: usize,
    pub stroke_width: u32,
    pub text_size: u32,
    pub number_size: u32,
    pub mosaic_size: u32,
    pub text_bold: bool,
    pub text_italic: bool,
    pub text_background: bool,
    pub text_font_family: TextFontFamily,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct PinDefaults {
    pub always_on_top: bool,
    pub show_decoration: bool,
    pub opacity_percent: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OcrProfile {
    pub id: String,
    pub display_name: String,
    pub provider_kind: OcrProviderKind,
    pub base_url: String,
    pub api_key: String,
    #[serde(default)]
    pub secret_key: String,
    pub model: String,
    pub bbox_scale_mode: OcrBboxScaleMode,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct OcrConfig {
    pub enabled: bool,
    pub auto_copy_full_text: bool,
    pub auto_exit_after_copy: bool,
    pub default_profile_id: String,
    pub request_timeout_ms: u64,
    pub profiles: Vec<OcrProfile>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TranslationProfile {
    pub id: String,
    pub display_name: String,
    pub provider_kind: TranslationProviderKind,
    pub base_url: String,
    pub api_key: String,
    #[serde(default)]
    pub secret_key: String,
    pub model: String,
    #[serde(default)]
    pub prompt_template: String,
    #[serde(default)]
    pub source_lang: String,
    #[serde(default)]
    pub target_lang: String,
    #[serde(default)]
    pub use_translated_image: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct TranslationConfig {
    pub enabled: bool,
    pub auto_copy_full_text: bool,
    pub auto_exit_after_copy: bool,
    pub default_profile_id: String,
    pub request_timeout_ms: u64,
    pub profiles: Vec<TranslationProfile>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AppConfig {
    pub general: GeneralConfig,
    pub annotation_defaults: AnnotationDefaults,
    pub pin_defaults: PinDefaults,
    pub ocr: OcrConfig,
    pub translation: TranslationConfig,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            hotkey: "Ctrl+Shift+A".to_string(),
            auto_copy: true,
            auto_save: true,
            launch_at_startup: false,
            save_dir: super::paths::default_save_dir()
                .unwrap_or_else(|| PathBuf::from(r"C:\Users\Public\Pictures\OpenCapt")),
        }
    }
}

impl Default for AnnotationDefaults {
    fn default() -> Self {
        Self {
            default_color_index: ANNOTATION_COLOR_PRESETS.len().saturating_sub(1),
            stroke_width: DEFAULT_STROKE_WIDTH,
            text_size: DEFAULT_TEXT_SIZE,
            number_size: DEFAULT_NUMBER_SIZE,
            mosaic_size: DEFAULT_MOSAIC_SIZE,
            text_bold: false,
            text_italic: false,
            text_background: false,
            text_font_family: TextFontFamily::YaHei,
        }
    }
}

impl Default for PinDefaults {
    fn default() -> Self {
        Self {
            always_on_top: true,
            show_decoration: true,
            opacity_percent: 100,
        }
    }
}

impl Default for OcrConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            auto_copy_full_text: false,
            auto_exit_after_copy: true,
            default_profile_id: String::new(),
            request_timeout_ms: OCR_TIMEOUT_DEFAULT_MS,
            profiles: Vec::new(),
        }
    }
}

impl Default for TranslationConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            auto_copy_full_text: false,
            auto_exit_after_copy: true,
            default_profile_id: String::new(),
            request_timeout_ms: TRANSLATION_TIMEOUT_DEFAULT_MS,
            profiles: Vec::new(),
        }
    }
}

pub fn default_ocr_profile(index: usize, id: String) -> OcrProfile {
    let provider = OcrProviderKind::OpenAiCompatible;
    OcrProfile {
        id,
        display_name: format!("模型{}", index),
        provider_kind: provider,
        base_url: provider.default_base_url().to_string(),
        api_key: String::new(),
        secret_key: String::new(),
        model: provider.default_model().to_string(),
        bbox_scale_mode: provider.default_bbox_scale_mode(),
    }
}

pub fn default_translation_profile(index: usize, id: String) -> TranslationProfile {
    let provider = TranslationProviderKind::OpenAiCompatible;
    TranslationProfile {
        id,
        display_name: format!("翻译模型{}", index),
        provider_kind: provider,
        base_url: provider.default_base_url().to_string(),
        api_key: String::new(),
        secret_key: String::new(),
        model: provider.default_model().to_string(),
        prompt_template: DEFAULT_TRANSLATION_PROMPT_TEMPLATE.to_string(),
        source_lang: "auto".to_string(),
        target_lang: "zh".to_string(),
        use_translated_image: false,
    }
}
impl Default for AppConfig {
    fn default() -> Self {
        Self {
            general: GeneralConfig::default(),
            annotation_defaults: AnnotationDefaults::default(),
            pin_defaults: PinDefaults::default(),
            ocr: OcrConfig::default(),
            translation: TranslationConfig::default(),
        }
    }
}

impl AppConfig {
    pub fn validate(&self) -> Result<()> {
        hotkey::parse_hotkey(&self.general.hotkey)
            .with_context(|| format!("invalid hotkey: {}", self.general.hotkey))?;
        if self.general.save_dir.as_os_str().is_empty() {
            bail!("save directory may not be empty");
        }
        if !PIN_OPACITY_OPTIONS.contains(&self.pin_defaults.opacity_percent) {
            bail!(
                "unsupported pin opacity percent: {}",
                self.pin_defaults.opacity_percent
            );
        }
        if !(OCR_TIMEOUT_MIN_MS..=OCR_TIMEOUT_MAX_MS).contains(&self.ocr.request_timeout_ms) {
            bail!(
                "ocr timeout must be between {} and {} ms",
                OCR_TIMEOUT_MIN_MS,
                OCR_TIMEOUT_MAX_MS
            );
        }

        let mut ids = std::collections::HashSet::new();
        for profile in &self.ocr.profiles {
            if profile.id.trim().is_empty() {
                bail!("ocr profile id may not be empty");
            }
            if !ids.insert(profile.id.trim().to_string()) {
                bail!("duplicate ocr profile id: {}", profile.id);
            }
            if profile.display_name.trim().is_empty() {
                bail!("ocr profile display_name may not be empty");
            }
            if profile.base_url.trim().is_empty() {
                bail!("ocr profile base_url may not be empty");
            }
            match profile.provider_kind {
                OcrProviderKind::OpenAiCompatible => {
                    if profile.api_key.trim().is_empty() {
                        bail!("ocr profile api_key may not be empty");
                    }
                    if profile.model.trim().is_empty() {
                        bail!("ocr profile model may not be empty");
                    }
                }
                OcrProviderKind::BaiduOcr => {
                    if profile.api_key.trim().is_empty() {
                        bail!("baidu ocr profile api_key may not be empty");
                    }
                    if profile.secret_key.trim().is_empty() {
                        bail!("baidu ocr profile secret_key may not be empty");
                    }
                    if profile.model.trim().is_empty() {
                        bail!("baidu ocr profile api path may not be empty");
                    }
                }
            }
        }

        if self.ocr.enabled && self.ocr.profiles.is_empty() {
            bail!("ocr enabled but no profile configured");
        }

        if !self.ocr.profiles.is_empty() {
            if self.ocr.default_profile_id.trim().is_empty() {
                bail!("ocr default profile may not be empty when profiles are configured");
            }
            if !self
                .ocr
                .profiles
                .iter()
                .any(|profile| profile.id == self.ocr.default_profile_id)
            {
                bail!(
                    "ocr default profile not found: {}",
                    self.ocr.default_profile_id
                );
            }
        }

        if !(TRANSLATION_TIMEOUT_MIN_MS..=TRANSLATION_TIMEOUT_MAX_MS)
            .contains(&self.translation.request_timeout_ms)
        {
            bail!(
                "translation timeout must be between {} and {} ms",
                TRANSLATION_TIMEOUT_MIN_MS,
                TRANSLATION_TIMEOUT_MAX_MS
            );
        }

        let mut translation_ids = std::collections::HashSet::new();
        for profile in &self.translation.profiles {
            if profile.id.trim().is_empty() {
                bail!("translation profile id may not be empty");
            }
            if !translation_ids.insert(profile.id.trim().to_string()) {
                bail!("duplicate translation profile id: {}", profile.id);
            }
            if profile.display_name.trim().is_empty() {
                bail!("translation profile display_name may not be empty");
            }
            if profile.base_url.trim().is_empty() {
                bail!("translation profile base_url may not be empty");
            }
            match profile.provider_kind {
                TranslationProviderKind::OpenAiCompatible => {
                    if profile.api_key.trim().is_empty() {
                        bail!("translation profile api_key may not be empty");
                    }
                    if profile.model.trim().is_empty() {
                        bail!("translation profile model may not be empty");
                    }
                    if profile.prompt_template.trim().is_empty() {
                        bail!("translation profile prompt_template may not be empty");
                    }
                }
                TranslationProviderKind::BaiduImageTranslate => {
                    if profile.api_key.trim().is_empty() {
                        bail!("baidu translation profile api_key may not be empty");
                    }
                    if profile.secret_key.trim().is_empty() {
                        bail!("baidu translation profile secret_key may not be empty");
                    }
                    if profile.model.trim().is_empty() {
                        bail!("baidu translation profile api path may not be empty");
                    }
                    if profile.source_lang.trim().is_empty() {
                        bail!("baidu translation profile source_lang may not be empty");
                    }
                    if profile.target_lang.trim().is_empty() {
                        bail!("baidu translation profile target_lang may not be empty");
                    }
                }
            }
        }

        if self.translation.enabled && self.translation.profiles.is_empty() {
            bail!("translation enabled but no profile configured");
        }

        if !self.translation.profiles.is_empty() {
            if self.translation.default_profile_id.trim().is_empty() {
                bail!("translation default profile may not be empty when profiles are configured");
            }
            if !self
                .translation
                .profiles
                .iter()
                .any(|profile| profile.id == self.translation.default_profile_id)
            {
                bail!(
                    "translation default profile not found: {}",
                    self.translation.default_profile_id
                );
            }
        }

        Ok(())
    }

    pub fn sanitize(mut self) -> Self {
        self.annotation_defaults.default_color_index = self
            .annotation_defaults
            .default_color_index
            .min(ANNOTATION_COLOR_PRESETS.len().saturating_sub(1));
        self.annotation_defaults.stroke_width = self
            .annotation_defaults
            .stroke_width
            .clamp(MIN_STROKE_WIDTH, MAX_STROKE_WIDTH);
        self.annotation_defaults.text_size = self
            .annotation_defaults
            .text_size
            .clamp(MIN_TEXT_SIZE, MAX_TEXT_SIZE);
        self.annotation_defaults.number_size = self
            .annotation_defaults
            .number_size
            .clamp(MIN_NUMBER_SIZE, MAX_NUMBER_SIZE);
        self.annotation_defaults.mosaic_size = self
            .annotation_defaults
            .mosaic_size
            .clamp(MIN_MOSAIC_SIZE, MAX_MOSAIC_SIZE);
        if !PIN_OPACITY_OPTIONS.contains(&self.pin_defaults.opacity_percent) {
            self.pin_defaults.opacity_percent = PinDefaults::default().opacity_percent;
        }
        self.ocr.request_timeout_ms = self
            .ocr
            .request_timeout_ms
            .clamp(OCR_TIMEOUT_MIN_MS, OCR_TIMEOUT_MAX_MS);
        normalize_ocr_profile_ids(&mut self.ocr.profiles);

        if self.ocr.profiles.is_empty() {
            self.ocr.enabled = false;
            self.ocr.default_profile_id.clear();
        } else if !self
            .ocr
            .profiles
            .iter()
            .any(|profile| profile.id == self.ocr.default_profile_id)
        {
            self.ocr.default_profile_id = self.ocr.profiles[0].id.clone();
        }

        self.translation.request_timeout_ms = self
            .translation
            .request_timeout_ms
            .clamp(TRANSLATION_TIMEOUT_MIN_MS, TRANSLATION_TIMEOUT_MAX_MS);
        normalize_translation_profile_ids(&mut self.translation.profiles);
        for profile in &mut self.translation.profiles {
            if matches!(
                profile.provider_kind,
                TranslationProviderKind::BaiduImageTranslate
            ) {
                if profile.source_lang.trim().is_empty() {
                    profile.source_lang = "auto".to_string();
                }
                if profile.target_lang.trim().is_empty() {
                    profile.target_lang = "zh".to_string();
                }
                if profile.model.trim().is_empty() {
                    profile.model = TranslationProviderKind::BaiduImageTranslate
                        .default_model()
                        .to_string();
                }
                if profile.base_url.trim().is_empty() {
                    profile.base_url = TranslationProviderKind::BaiduImageTranslate
                        .default_base_url()
                        .to_string();
                }
            }
        }

        if self.translation.profiles.is_empty() {
            self.translation.enabled = false;
            self.translation.default_profile_id.clear();
        } else if !self
            .translation
            .profiles
            .iter()
            .any(|profile| profile.id == self.translation.default_profile_id)
        {
            self.translation.default_profile_id = self.translation.profiles[0].id.clone();
        }

        self
    }
}

fn normalize_ocr_profile_ids(profiles: &mut [OcrProfile]) {
    let mut used = std::collections::HashSet::new();
    let mut next_index = 1usize;

    for profile in profiles {
        let trimmed = profile.id.trim();
        if !trimmed.is_empty() && used.insert(trimmed.to_string()) {
            if trimmed != profile.id {
                profile.id = trimmed.to_string();
            }
            continue;
        }

        loop {
            let candidate = format!("profile_{}", next_index);
            next_index += 1;
            if used.insert(candidate.clone()) {
                profile.id = candidate;
                break;
            }
        }
    }
}

fn normalize_translation_profile_ids(profiles: &mut [TranslationProfile]) {
    let mut used = std::collections::HashSet::new();
    let mut next_index = 1usize;

    for profile in profiles {
        let trimmed = profile.id.trim();
        if !trimmed.is_empty() && used.insert(trimmed.to_string()) {
            if trimmed != profile.id {
                profile.id = trimmed.to_string();
            }
            continue;
        }

        loop {
            let candidate = format!("translate_profile_{}", next_index);
            next_index += 1;
            if used.insert(candidate.clone()) {
                profile.id = candidate;
                break;
            }
        }
    }
}
