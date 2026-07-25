mod compat;
mod io;
mod paths;
mod types;

pub use io::{load_from_path, load_or_create, write_config};
pub use paths::AppPaths;
pub use types::*;

#[cfg(test)]
mod tests {
    use super::paths::{appdata_app_paths, portable_app_paths};
    use super::*;
    use std::path::{Path, PathBuf};

    #[test]
    fn default_config_has_expected_values() {
        let config = AppConfig::default();
        assert_eq!(config.general.hotkey, "Ctrl+Shift+A");
        assert!(config.general.auto_copy);
        assert!(config.general.auto_save);
        assert!(!config.general.launch_at_startup);
        assert!(config.general.save_dir.ends_with("OpenCapt"));
        assert_eq!(config.annotation_defaults.default_color_index, 4);
        assert_eq!(config.pin_defaults.opacity_percent, 100);
        assert!(!config.ocr.enabled);
        assert!(!config.ocr.auto_copy_full_text);
        assert!(config.ocr.auto_exit_after_copy);
        assert_eq!(config.ocr.request_timeout_ms, OCR_TIMEOUT_DEFAULT_MS);
        assert!(!config.translation.enabled);
        assert!(!config.translation.auto_copy_full_text);
        assert!(config.translation.auto_exit_after_copy);
        assert_eq!(
            config.translation.request_timeout_ms,
            TRANSLATION_TIMEOUT_DEFAULT_MS
        );
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
            secret_key: String::new(),
            model: "gpt-4.1-mini".to_string(),
            bbox_scale_mode: OcrBboxScaleMode::ZeroTo1000,
        });
        config.ocr.enabled = true;
        config.ocr.auto_copy_full_text = true;
        config.ocr.auto_exit_after_copy = false;
        config.ocr.default_profile_id = "default".to_string();

        config.translation.profiles.push(TranslationProfile {
            id: "tr_default".to_string(),
            display_name: "默认翻译".to_string(),
            provider_kind: TranslationProviderKind::OpenAiCompatible,
            base_url: "https://api.openai.com/v1".to_string(),
            api_key: "translate-key".to_string(),
            secret_key: String::new(),
            model: "gpt-4.1-mini".to_string(),
            prompt_template: "Translate the following text into Chinese. Return only the translated text without explanation.
{{text}}".to_string(),
            source_lang: "auto".to_string(),
            target_lang: "zh".to_string(),
            use_translated_image: false,
        });
        config.translation.enabled = true;
        config.translation.auto_copy_full_text = true;
        config.translation.auto_exit_after_copy = false;
        config.translation.default_profile_id = "tr_default".to_string();

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
        assert!(!parsed.general.launch_at_startup);
        assert_eq!(parsed.general.save_dir, PathBuf::from(r"C:\Shots"));
        assert_eq!(
            parsed.annotation_defaults.text_font_family,
            TextFontFamily::YaHei
        );
        assert!(!parsed.ocr.auto_copy_full_text);
        assert!(parsed.ocr.auto_exit_after_copy);
        assert!(!parsed.translation.auto_copy_full_text);
        assert!(parsed.translation.auto_exit_after_copy);
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
        assert!(!parsed.general.launch_at_startup);
        assert_eq!(
            parsed.annotation_defaults.stroke_width,
            DEFAULT_STROKE_WIDTH
        );
        assert!(!parsed.ocr.auto_copy_full_text);
        assert!(parsed.ocr.auto_exit_after_copy);
        assert!(!parsed.translation.auto_copy_full_text);
        assert!(parsed.translation.auto_exit_after_copy);
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
                secret_key: String::new(),
                model: "gpt-4.1-mini".to_string(),
                bbox_scale_mode: OcrBboxScaleMode::ZeroTo1000,
            },
            OcrProfile {
                id: "profile_1".to_string(),
                display_name: "B".to_string(),
                provider_kind: OcrProviderKind::OpenAiCompatible,
                base_url: "https://api.openai.com/v1".to_string(),
                api_key: "b".to_string(),
                secret_key: String::new(),
                model: "gpt-4.1-mini".to_string(),
                bbox_scale_mode: OcrBboxScaleMode::ZeroTo1000,
            },
            OcrProfile {
                id: "profile_1".to_string(),
                display_name: "C".to_string(),
                provider_kind: OcrProviderKind::OpenAiCompatible,
                base_url: "https://api.openai.com/v1".to_string(),
                api_key: "c".to_string(),
                secret_key: String::new(),
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
    fn missing_translation_prompt_is_rejected() {
        let parsed = toml::from_str::<AppConfig>(
            r#"
[translation]
enabled = true
default_profile_id = "t1"
request_timeout_ms = 20000

[[translation.profiles]]
id = "t1"
display_name = "Translate"
provider_kind = "openai_compatible"
base_url = "https://api.openai.com/v1"
api_key = "abc"
model = "gpt-4.1-mini"
"#,
        )
        .expect("parse translation config");
        assert!(parsed.validate().is_err());
    }

    #[test]
    fn missing_baidu_secret_key_is_rejected() {
        let mut config = AppConfig::default();
        config.ocr.enabled = true;
        config.ocr.default_profile_id = "baidu".to_string();
        config.ocr.profiles.push(OcrProfile {
            id: "baidu".to_string(),
            display_name: "百度".to_string(),
            provider_kind: OcrProviderKind::BaiduOcr,
            base_url: "https://aip.baidubce.com".to_string(),
            api_key: "api-key".to_string(),
            secret_key: String::new(),
            model: "general".to_string(),
            bbox_scale_mode: OcrBboxScaleMode::PixelAbsolute,
        });

        assert!(config.validate().is_err());
    }

    #[test]
    fn sanitize_translation_profile_ids_when_missing_or_duplicated() {
        let mut config = AppConfig::default();
        config.translation.profiles = vec![
            TranslationProfile {
                id: String::new(),
                display_name: "A".to_string(),
                provider_kind: TranslationProviderKind::OpenAiCompatible,
                base_url: "https://api.openai.com/v1".to_string(),
                api_key: "a".to_string(),
                model: "gpt-4.1-mini".to_string(),
                prompt_template: "{{texts}}".to_string(),
                secret_key: String::new(),
                source_lang: "auto".to_string(),
                target_lang: "zh".to_string(),
                use_translated_image: false,
            },
            TranslationProfile {
                id: "translate_profile_1".to_string(),
                display_name: "B".to_string(),
                provider_kind: TranslationProviderKind::OpenAiCompatible,
                base_url: "https://api.openai.com/v1".to_string(),
                api_key: "b".to_string(),
                model: "gpt-4.1-mini".to_string(),
                prompt_template: "{{texts}}".to_string(),
                secret_key: String::new(),
                source_lang: "auto".to_string(),
                target_lang: "zh".to_string(),
                use_translated_image: false,
            },
            TranslationProfile {
                id: "translate_profile_1".to_string(),
                display_name: "C".to_string(),
                provider_kind: TranslationProviderKind::OpenAiCompatible,
                base_url: "https://api.openai.com/v1".to_string(),
                api_key: "c".to_string(),
                model: "gpt-4.1-mini".to_string(),
                prompt_template: "{{texts}}".to_string(),
                secret_key: String::new(),
                source_lang: "auto".to_string(),
                target_lang: "zh".to_string(),
                use_translated_image: false,
            },
        ];
        config.translation.default_profile_id = "missing".to_string();

        let sanitized = config.sanitize();
        let ids: std::collections::HashSet<_> = sanitized
            .translation
            .profiles
            .iter()
            .map(|profile| profile.id.clone())
            .collect();
        assert_eq!(ids.len(), sanitized.translation.profiles.len());
        assert!(
            sanitized
                .translation
                .profiles
                .iter()
                .all(|profile| !profile.id.trim().is_empty())
        );
        assert_eq!(
            sanitized.translation.default_profile_id,
            sanitized.translation.profiles[0].id
        );
    }
    #[test]
    fn portable_paths_place_config_next_to_exe() {
        let exe_dir = Path::new(r"C:\Apps\OpenCapt");
        let paths = portable_app_paths(exe_dir);
        assert_eq!(paths.config_dir, exe_dir);
        assert_eq!(paths.config_file, exe_dir.join("config.toml"));
        assert_eq!(paths.log_dir, exe_dir.join("logs"));
    }

    #[test]
    fn appdata_paths_land_under_appdata() {
        let paths = appdata_app_paths().expect("resolve paths");
        assert!(paths.config_dir.ends_with("OpenCapt"));
        assert_eq!(
            paths.config_file.file_name().and_then(|name| name.to_str()),
            Some("config.toml")
        );
    }
}
