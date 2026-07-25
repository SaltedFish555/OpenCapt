use super::types::*;
use anyhow::Result;
use serde::{Deserialize, Deserializer};
use std::path::PathBuf;

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
    translation: Option<TranslationConfigCompat>,
    hotkey: Option<String>,
    auto_copy: Option<bool>,
    auto_save: Option<bool>,
    launch_at_startup: Option<bool>,
    save_dir: Option<PathBuf>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct GeneralConfigCompat {
    hotkey: Option<String>,
    auto_copy: Option<bool>,
    auto_save: Option<bool>,
    launch_at_startup: Option<bool>,
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
    auto_copy_full_text: Option<bool>,
    auto_exit_after_copy: Option<bool>,
    default_profile_id: Option<String>,
    request_timeout_ms: Option<u64>,
    profiles: Option<Vec<OcrProfile>>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct TranslationConfigCompat {
    enabled: Option<bool>,
    auto_copy_full_text: Option<bool>,
    auto_exit_after_copy: Option<bool>,
    default_profile_id: Option<String>,
    request_timeout_ms: Option<u64>,
    profiles: Option<Vec<TranslationProfile>>,
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
        if let Some(translation) = self.translation {
            apply_translation(&mut config.translation, translation);
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
        if let Some(launch_at_startup) = self.launch_at_startup {
            config.general.launch_at_startup = launch_at_startup;
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
    if let Some(launch_at_startup) = value.launch_at_startup {
        target.launch_at_startup = launch_at_startup;
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
    if let Some(auto_copy_full_text) = value.auto_copy_full_text {
        target.auto_copy_full_text = auto_copy_full_text;
    }
    if let Some(auto_exit_after_copy) = value.auto_exit_after_copy {
        target.auto_exit_after_copy = auto_exit_after_copy;
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

fn apply_translation(target: &mut TranslationConfig, value: TranslationConfigCompat) {
    if let Some(enabled) = value.enabled {
        target.enabled = enabled;
    }
    if let Some(auto_copy_full_text) = value.auto_copy_full_text {
        target.auto_copy_full_text = auto_copy_full_text;
    }
    if let Some(auto_exit_after_copy) = value.auto_exit_after_copy {
        target.auto_exit_after_copy = auto_exit_after_copy;
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
