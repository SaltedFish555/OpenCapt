mod app;
mod capture;
mod config;
mod hotkey;
mod logging;
mod output;
mod overlay;
mod tray;

use anyhow::Result;
use std::env;
use tracing::info;
#[cfg(windows)]
use windows::Win32::System::Console::FreeConsole;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupMode {
    Run,
    CaptureTest,
    OverlayTest,
}

fn main() -> Result<()> {
    let startup_mode = parse_startup_mode(env::args())?;
    let (config, paths) = config::load_or_create()?;
    let _logging_guard = logging::init(&paths.log_dir)?;

    info!(?startup_mode, "OpenCapt starting");
    detach_console_for_gui_mode(&startup_mode);

    match startup_mode {
        StartupMode::CaptureTest => {
            let image = capture::capture_current_monitor_region(None)?;
            let result = output::process_capture(image, &config)?;
            info!(path = ?result.saved_path, "capture-test completed");
            Ok(())
        }
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
        _ => StartupMode::Run,
    })
}

#[cfg(windows)]
fn detach_console_for_gui_mode(startup_mode: &StartupMode) {
    if !matches!(startup_mode, StartupMode::Run | StartupMode::OverlayTest) {
        return;
    }
    if env::var_os("OPENCAPT_KEEP_CONSOLE").is_some() {
        return;
    }
    unsafe {
        let _ = FreeConsole();
    }
}

#[cfg(not(windows))]
fn detach_console_for_gui_mode(_startup_mode: &StartupMode) {}
