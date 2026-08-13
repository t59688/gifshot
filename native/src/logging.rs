//! File-backed diagnostics. Logs intentionally contain no captured pixel data.

use crate::paths;
use std::time::Duration;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};

const LOG_RETENTION: Duration = Duration::from_secs(14 * 24 * 60 * 60);

pub fn init() -> Option<WorkerGuard> {
    let dir = paths::log_dir();
    std::fs::create_dir_all(&dir).ok()?;
    cleanup_old_logs(&dir);

    let file = tracing_appender::rolling::daily(dir, "gifshot.log");
    let (writer, guard) = tracing_appender::non_blocking(file);
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_writer(writer).with_ansi(false).compact())
        .try_init()
        .ok()?;

    Some(guard)
}

fn cleanup_old_logs(dir: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        if !name.to_string_lossy().starts_with("gifshot.log.") {
            continue;
        }
        let Ok(metadata) = entry.metadata() else {
            continue;
        };
        let Ok(modified) = metadata.modified() else {
            continue;
        };
        if modified.elapsed().is_ok_and(|age| age > LOG_RETENTION) {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}
