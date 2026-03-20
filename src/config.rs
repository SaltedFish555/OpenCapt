use anyhow::{Context, Result, anyhow};
use directories_next::{BaseDirs, UserDirs};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

const APP_NAME: &str = "OpenCapt";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppConfig {
    pub hotkey: String,
    pub auto_copy: bool,
    pub auto_save: bool,
    pub save_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub config_dir: PathBuf,
    pub config_file: PathBuf,
    pub log_dir: PathBuf,
}

impl Default for AppConfig {
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
        let content = fs::read_to_string(&paths.config_file).with_context(|| {
            format!(
                "failed to read config file at {}",
                paths.config_file.display()
            )
        })?;
        toml::from_str::<AppConfig>(&content).with_context(|| {
            format!(
                "failed to parse config file at {}",
                paths.config_file.display()
            )
        })?
    } else {
        let config = AppConfig::default();
        write_config(&paths.config_file, &config)?;
        config
    };

    fs::create_dir_all(&config.save_dir).with_context(|| {
        format!(
            "failed to create save directory at {}",
            config.save_dir.display()
        )
    })?;

    Ok((config, paths))
}

pub fn write_config(path: &Path, config: &AppConfig) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!("failed to create parent directory at {}", parent.display())
        })?;
    }

    let serialized = toml::to_string_pretty(config).context("failed to serialize config")?;
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
        assert_eq!(config.hotkey, "Ctrl+Shift+A");
        assert!(config.auto_copy);
        assert!(config.auto_save);
        assert!(config.save_dir.ends_with("OpenCapt"));
    }

    #[test]
    fn write_config_round_trips() {
        let config = AppConfig::default();
        let serialized = toml::to_string_pretty(&config).expect("serialize config");
        let parsed: AppConfig = toml::from_str(&serialized).expect("parse config");
        assert_eq!(config, parsed);
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
