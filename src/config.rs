use crate::hotkey;
use anyhow::{Context, Result, anyhow, bail};
use directories_next::{BaseDirs, UserDirs};
use serde::{Deserialize, Deserializer, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

const APP_NAME: &str = "OpenCapt";
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
    pub model: String,
    pub bbox_scale_mode: OcrBboxScaleMode,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct OcrConfig {
    pub enabled: bool,
    pub default_profile_id: String,
    pub request_timeout_ms: u64,
    pub profiles: Vec<OcrProfile>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AppConfig {
    pub general: GeneralConfig,
    pub annotation_defaults: AnnotationDefaults,
    pub pin_defaults: PinDefaults,
    pub ocr: OcrConfig,
}

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub config_dir: PathBuf,
    pub config_file: PathBuf,
    pub log_dir: PathBuf,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            hotkey: "Ctrl+Shift+A".to_string(),
            auto_copy: true,
            auto_save: true,
            save_dir: default_save_dir()
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
            default_profile_id: String::new(),
            request_timeout_ms: OCR_TIMEOUT_DEFAULT_MS,
            profiles: Vec::new(),
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            general: GeneralConfig::default(),
            annotation_defaults: AnnotationDefaults::default(),
            pin_defaults: PinDefaults::default(),
            ocr: OcrConfig::default(),
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
            if profile.api_key.trim().is_empty() {
                bail!("ocr profile api_key may not be empty");
            }
            if profile.model.trim().is_empty() {
                bail!("ocr profile model may not be empty");
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

impl<'de> Deserialize<'de> for AppConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let compat = AppConfigCompat::deserialize(deserializer)?;
        Ok(compat.into_config().sanitize())
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct AppConfigCompat {
    general: Option<GeneralConfigCompat>,
    annotation_defaults: Option<AnnotationDefaultsCompat>,
    pin_defaults: Option<PinDefaultsCompat>,
    ocr: Option<OcrConfigCompat>,
    hotkey: Option<String>,
    auto_copy: Option<bool>,
    auto_save: Option<bool>,
    save_dir: Option<PathBuf>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct GeneralConfigCompat {
    hotkey: Option<String>,
    auto_copy: Option<bool>,
    auto_save: Option<bool>,
    save_dir: Option<PathBuf>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct AnnotationDefaultsCompat {
    default_color_index: Option<usize>,
    stroke_width: Option<u32>,
    text_size: Option<u32>,
    number_size: Option<u32>,
    mosaic_size: Option<u32>,
    text_bold: Option<bool>,
    text_italic: Option<bool>,
    text_background: Option<bool>,
    text_font_family: Option<TextFontFamily>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct PinDefaultsCompat {
    always_on_top: Option<bool>,
    show_decoration: Option<bool>,
    opacity_percent: Option<u8>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct OcrConfigCompat {
    enabled: Option<bool>,
    default_profile_id: Option<String>,
    request_timeout_ms: Option<u64>,
    profiles: Option<Vec<OcrProfile>>,
}

impl AppConfigCompat {
    fn into_config(self) -> AppConfig {
        let mut config = AppConfig::default();

        if let Some(general) = self.general {
            apply_general(&mut config.general, general);
        }
        if let Some(annotation) = self.annotation_defaults {
            apply_annotation(&mut config.annotation_defaults, annotation);
        }
        if let Some(pin) = self.pin_defaults {
            apply_pin(&mut config.pin_defaults, pin);
        }
        if let Some(ocr) = self.ocr {
            apply_ocr(&mut config.ocr, ocr);
        }

        if let Some(hotkey) = self.hotkey {
            config.general.hotkey = hotkey;
        }
        if let Some(auto_copy) = self.auto_copy {
            config.general.auto_copy = auto_copy;
        }
        if let Some(auto_save) = self.auto_save {
            config.general.auto_save = auto_save;
        }
        if let Some(save_dir) = self.save_dir {
            config.general.save_dir = save_dir;
        }

        config
    }
}

fn apply_general(target: &mut GeneralConfig, value: GeneralConfigCompat) {
    if let Some(hotkey) = value.hotkey {
        target.hotkey = hotkey;
    }
    if let Some(auto_copy) = value.auto_copy {
        target.auto_copy = auto_copy;
    }
    if let Some(auto_save) = value.auto_save {
        target.auto_save = auto_save;
    }
    if let Some(save_dir) = value.save_dir {
        target.save_dir = save_dir;
    }
}

fn apply_annotation(target: &mut AnnotationDefaults, value: AnnotationDefaultsCompat) {
    if let Some(default_color_index) = value.default_color_index {
        target.default_color_index = default_color_index;
    }
    if let Some(stroke_width) = value.stroke_width {
        target.stroke_width = stroke_width;
    }
    if let Some(text_size) = value.text_size {
        target.text_size = text_size;
    }
    if let Some(number_size) = value.number_size {
        target.number_size = number_size;
    }
    if let Some(mosaic_size) = value.mosaic_size {
        target.mosaic_size = mosaic_size;
    }
    if let Some(text_bold) = value.text_bold {
        target.text_bold = text_bold;
    }
    if let Some(text_italic) = value.text_italic {
        target.text_italic = text_italic;
    }
    if let Some(text_background) = value.text_background {
        target.text_background = text_background;
    }
    if let Some(text_font_family) = value.text_font_family {
        target.text_font_family = text_font_family;
    }
}

fn apply_pin(target: &mut PinDefaults, value: PinDefaultsCompat) {
    if let Some(always_on_top) = value.always_on_top {
        target.always_on_top = always_on_top;
    }
    if let Some(show_decoration) = value.show_decoration {
        target.show_decoration = show_decoration;
    }
    if let Some(opacity_percent) = value.opacity_percent {
        target.opacity_percent = opacity_percent;
    }
}

fn apply_ocr(target: &mut OcrConfig, value: OcrConfigCompat) {
    if let Some(enabled) = value.enabled {
        target.enabled = enabled;
    }
    if let Some(default_profile_id) = value.default_profile_id {
        target.default_profile_id = default_profile_id;
    }
    if let Some(request_timeout_ms) = value.request_timeout_ms {
        target.request_timeout_ms = request_timeout_ms;
    }
    if let Some(profiles) = value.profiles {
        target.profiles = profiles;
    }
}

pub fn load_or_create() -> Result<(AppConfig, AppPaths)> {
    let paths = app_paths()?;
    fs::create_dir_all(&paths.config_dir).with_context(|| {
        format!(
            "failed to create config directory at {}",
            paths.config_dir.display()
        )
    })?;
    fs::create_dir_all(&paths.log_dir).with_context(|| {
        format!(
            "failed to create log directory at {}",
            paths.log_dir.display()
        )
    })?;

    let config = if paths.config_file.exists() {
        load_from_path(&paths.config_file)?
    } else {
        let config = AppConfig::default();
        write_config(&paths.config_file, &config)?;
        config
    };

    fs::create_dir_all(&config.general.save_dir).with_context(|| {
        format!(
            "failed to create save directory at {}",
            config.general.save_dir.display()
        )
    })?;

    Ok((config, paths))
}

pub fn load_from_path(path: &Path) -> Result<AppConfig> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("failed to read config file at {}", path.display()))?;
    let config = toml::from_str::<AppConfig>(&content)
        .with_context(|| format!("failed to parse config file at {}", path.display()))?;
    config.validate()?;
    Ok(config)
}

pub fn write_config(path: &Path, config: &AppConfig) -> Result<()> {
    config.validate()?;

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!("failed to create parent directory at {}", parent.display())
        })?;
    }

    let serialized =
        toml::to_string_pretty(&config.clone().sanitize()).context("failed to serialize config")?;
    fs::write(path, serialized)
        .with_context(|| format!("failed to write config file at {}", path.display()))?;
    Ok(())
}

fn app_paths() -> Result<AppPaths> {
    let base_dirs =
        BaseDirs::new().ok_or_else(|| anyhow!("failed to discover base directories"))?;
    let config_dir = base_dirs.config_dir().join(APP_NAME);
    let config_file = config_dir.join("config.toml");
    let log_dir = config_dir.join("logs");

    Ok(AppPaths {
        config_dir,
        config_file,
        log_dir,
    })
}

fn default_save_dir() -> Option<PathBuf> {
    UserDirs::new().and_then(|dirs| dirs.picture_dir().map(|dir| dir.join(APP_NAME)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_expected_values() {
        let config = AppConfig::default();
        assert_eq!(config.general.hotkey, "Ctrl+Shift+A");
        assert!(config.general.auto_copy);
        assert!(config.general.auto_save);
        assert!(config.general.save_dir.ends_with("OpenCapt"));
        assert_eq!(config.annotation_defaults.default_color_index, 4);
        assert_eq!(config.pin_defaults.opacity_percent, 100);
        assert!(!config.ocr.enabled);
        assert_eq!(config.ocr.request_timeout_ms, OCR_TIMEOUT_DEFAULT_MS);
    }

    #[test]
    fn write_config_round_trips() {
        let mut config = AppConfig::default();
        config.ocr.profiles.push(OcrProfile {
            id: "default".to_string(),
            display_name: "默认模型".to_string(),
            provider_kind: OcrProviderKind::OpenAiCompatible,
            base_url: "https://api.openai.com/v1".to_string(),
            api_key: "test-key".to_string(),
            model: "gpt-4.1-mini".to_string(),
            bbox_scale_mode: OcrBboxScaleMode::ZeroTo1000,
        });
        config.ocr.enabled = true;
        config.ocr.default_profile_id = "default".to_string();

        let serialized = toml::to_string_pretty(&config).expect("serialize config");
        let parsed: AppConfig = toml::from_str(&serialized).expect("parse config");
        assert_eq!(config, parsed);
    }

    #[test]
    fn old_flat_general_config_still_loads() {
        let parsed: AppConfig = toml::from_str(
            r#"
hotkey = "Alt+Shift+Z"
auto_copy = false
auto_save = true
save_dir = "C:\\Shots"
"#,
        )
        .expect("parse old flat config");

        assert_eq!(parsed.general.hotkey, "Alt+Shift+Z");
        assert!(!parsed.general.auto_copy);
        assert!(parsed.general.auto_save);
        assert_eq!(parsed.general.save_dir, PathBuf::from(r"C:\Shots"));
        assert_eq!(
            parsed.annotation_defaults.text_font_family,
            TextFontFamily::YaHei
        );
    }

    #[test]
    fn nested_partial_config_uses_defaults() {
        let parsed: AppConfig = toml::from_str(
            r#"
[general]
hotkey = "Alt+Shift+S"
"#,
        )
        .expect("parse nested partial config");

        assert_eq!(parsed.general.hotkey, "Alt+Shift+S");
        assert!(parsed.general.auto_copy);
        assert_eq!(
            parsed.annotation_defaults.stroke_width,
            DEFAULT_STROKE_WIDTH
        );
    }

    #[test]
    fn invalid_hotkey_is_rejected() {
        let mut config = AppConfig::default();
        config.general.hotkey = "Ctrl+A+B".to_string();
        assert!(config.validate().is_err());
    }

    #[test]
    fn missing_bbox_scale_mode_is_rejected() {
        let parsed = toml::from_str::<AppConfig>(
            r#"
[ocr]
enabled = true
default_profile_id = "demo"
request_timeout_ms = 20000

[[ocr.profiles]]
id = "demo"
display_name = "Demo"
provider_kind = "openai_compatible"
base_url = "https://api.openai.com/v1"
api_key = "abc"
model = "gpt-4.1-mini"
"#,
        );
        assert!(parsed.is_err());
    }
    #[test]
    fn sanitize_ocr_profile_ids_when_missing_or_duplicated() {
        let mut config = AppConfig::default();
        config.ocr.profiles = vec![
            OcrProfile {
                id: String::new(),
                display_name: "A".to_string(),
                provider_kind: OcrProviderKind::OpenAiCompatible,
                base_url: "https://api.openai.com/v1".to_string(),
                api_key: "a".to_string(),
                model: "gpt-4.1-mini".to_string(),
                bbox_scale_mode: OcrBboxScaleMode::ZeroTo1000,
            },
            OcrProfile {
                id: "profile_1".to_string(),
                display_name: "B".to_string(),
                provider_kind: OcrProviderKind::OpenAiCompatible,
                base_url: "https://api.openai.com/v1".to_string(),
                api_key: "b".to_string(),
                model: "gpt-4.1-mini".to_string(),
                bbox_scale_mode: OcrBboxScaleMode::ZeroTo1000,
            },
            OcrProfile {
                id: "profile_1".to_string(),
                display_name: "C".to_string(),
                provider_kind: OcrProviderKind::OpenAiCompatible,
                base_url: "https://api.openai.com/v1".to_string(),
                api_key: "c".to_string(),
                model: "gpt-4.1-mini".to_string(),
                bbox_scale_mode: OcrBboxScaleMode::ZeroTo1000,
            },
        ];
        config.ocr.default_profile_id = "missing".to_string();

        let sanitized = config.sanitize();
        let ids: std::collections::HashSet<_> = sanitized
            .ocr
            .profiles
            .iter()
            .map(|profile| profile.id.clone())
            .collect();
        assert_eq!(ids.len(), sanitized.ocr.profiles.len());
        assert!(
            sanitized
                .ocr
                .profiles
                .iter()
                .all(|profile| !profile.id.trim().is_empty())
        );
        assert_eq!(
            sanitized.ocr.default_profile_id,
            sanitized.ocr.profiles[0].id
        );
    }

    #[test]
    fn config_paths_land_under_appdata() {
        let paths = app_paths().expect("resolve paths");
        assert!(paths.config_dir.ends_with("OpenCapt"));
        assert_eq!(
            paths.config_file.file_name().and_then(|name| name.to_str()),
            Some("config.toml")
        );
    }
}
