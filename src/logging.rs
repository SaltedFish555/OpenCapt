use anyhow::{Context, Result};
use chrono::Local;
use std::path::Path;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

pub fn init(log_dir: &Path) -> Result<WorkerGuard> {
    std::fs::create_dir_all(log_dir)
        .with_context(|| format!("failed to create log directory at {}", log_dir.display()))?;

    let file_name = format!("opencapt_{}.log", Local::now().format("%Y%m%d_%H%M%S"));
    let file_appender = tracing_appender::rolling::never(log_dir, file_name);
    let (writer, guard) = tracing_appender::non_blocking(file_appender);

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::registry()
        .with(filter)
        .with(
            fmt::layer()
                .with_writer(writer)
                .with_ansi(false)
                .with_target(true),
        )
        .try_init()
        .context("failed to initialize tracing subscriber")?;

    Ok(guard)
}
