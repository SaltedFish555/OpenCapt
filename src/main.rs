mod annotate;
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartupMode {
    Run,
    CaptureTest,
    OverlayTest,
    Annotate(annotate::AnnotateCli),
    AnnotateServer,
}

fn main() -> Result<()> {
    let startup_mode = parse_startup_mode(env::args())?;
    let (config, paths) = config::load_or_create()?;
    let _logging_guard = logging::init(&paths.log_dir)?;

    info!(?startup_mode, "OpenCapt starting");

    match startup_mode {
        StartupMode::CaptureTest => {
            let image = capture::capture_current_monitor_region(None)?;
            let result = output::process_capture(image, &config)?;
            info!(path = ?result.saved_path, "capture-test completed");
            Ok(())
        }
        StartupMode::Annotate(cli) => annotate::run(cli),
        StartupMode::AnnotateServer => annotate::run_server(),
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
        Some("annotate") => StartupMode::Annotate(annotate::AnnotateCli::parse(args)?),
        Some("annotate-server") => StartupMode::AnnotateServer,
        _ => StartupMode::Run,
    })
}
