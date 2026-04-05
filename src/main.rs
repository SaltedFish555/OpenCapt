#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod app;
mod capture;
mod config;
mod hotkey;
mod icons;
mod logging;
mod memory;
mod ocr;
mod output;
mod overlay;
mod pin;
mod settings;
mod startup;
mod translation;
mod tray;

use anyhow::Result;
use std::env;
use tracing::{info, warn};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupMode {
    Run,
    CaptureTest,
    OverlayTest,
    Settings,
}

fn main() -> Result<()> {
    let startup_mode = parse_startup_mode(env::args())?;
    let (config, paths) = config::load_or_create()?;
    let _logging_guard = logging::init(&paths.log_dir)?;

    if let Err(error) = startup::sync_launch_at_startup(config.general.launch_at_startup) {
        warn!(?error, "failed to sync launch-at-startup state");
    }

    info!(?startup_mode, "OpenCapt starting");

    match startup_mode {
        StartupMode::CaptureTest => {
            let image = capture::capture_current_monitor_region(None)?;
            let result = output::process_capture(image, &config)?;
            info!(path = ?result.saved_path, "capture-test completed");
            Ok(())
        }
        StartupMode::Settings => settings::run(config, paths),
        StartupMode::Run | StartupMode::OverlayTest => {
            app::run(config, paths, startup_mode);
        }
    }
}

fn parse_startup_mode(mut args: impl Iterator<Item = String>) -> Result<StartupMode> {
    let _program = args.next();
    Ok(match args.next().as_deref() {
        Some("capture-test") => StartupMode::CaptureTest,
        Some("overlay-test") => StartupMode::OverlayTest,
        Some("settings") => StartupMode::Settings,
        _ => StartupMode::Run,
    })
}
