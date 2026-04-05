use super::paths::{AppPaths, app_paths};
use super::types::AppConfig;
use anyhow::{Context, Result};
use std::fs;
use std::path::Path;

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
