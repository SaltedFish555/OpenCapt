use anyhow::{Result, anyhow};
use directories_next::{BaseDirs, UserDirs};
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::process;

const APP_NAME: &str = "OpenCapt";

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub config_dir: PathBuf,
    pub config_file: PathBuf,
    pub log_dir: PathBuf,
}

pub(super) fn app_paths() -> Result<AppPaths> {
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            let portable_paths = portable_app_paths(exe_dir);
            if path_is_writable(&portable_paths.config_dir) {
                return Ok(portable_paths);
            }
        }
    }

    appdata_app_paths()
}

pub(super) fn portable_app_paths(exe_dir: &Path) -> AppPaths {
    let config_dir = exe_dir.to_path_buf();
    let config_file = config_dir.join("config.toml");
    let log_dir = config_dir.join("logs");

    AppPaths {
        config_dir,
        config_file,
        log_dir,
    }
}

pub(super) fn appdata_app_paths() -> Result<AppPaths> {
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

pub(super) fn path_is_writable(dir: &Path) -> bool {
    if fs::create_dir_all(dir).is_err() {
        return false;
    }

    let probe_path = dir.join(format!(".opencapt_write_test_{}", process::id()));
    match OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&probe_path)
    {
        Ok(_) => {
            let _ = fs::remove_file(&probe_path);
            true
        }
        Err(_) => false,
    }
}

pub(super) fn default_save_dir() -> Option<PathBuf> {
    UserDirs::new().and_then(|dirs| dirs.picture_dir().map(|dir| dir.join(APP_NAME)))
}
