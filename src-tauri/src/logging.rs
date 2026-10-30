//! File logging. Never log the API key, request/response bodies, or chat text
//! (chat text only when `debugLogChat` is on, which later milestones check).

use std::path::Path;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{Builder, Rotation};

pub fn init(logs_dir: &Path) -> Option<WorkerGuard> {
    std::fs::create_dir_all(logs_dir).ok()?;
    let appender = Builder::new()
        .rotation(Rotation::DAILY)
        .filename_prefix("app")
        .filename_suffix("log")
        .max_log_files(5)
        .build(logs_dir)
        .ok()?;
    let (writer, guard) = tracing_appender::non_blocking(appender);
    tracing_subscriber::fmt()
        .with_writer(writer)
        .with_ansi(false)
        .with_max_level(tracing::Level::INFO)
        .init();
    Some(guard)
}
