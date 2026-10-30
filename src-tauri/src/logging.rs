//! File logging. Never log the API key, request/response bodies, or chat text
//! (chat text only when `debugLogChat` is on, which later milestones check).

use std::path::Path;

use tracing_appender::rolling::{Builder, Rotation};

/// Synchronous writes: log lines are tiny and rare, and a non-blocking writer only flushes on a
/// clean exit, which loses lines when the app is killed.
pub fn init(logs_dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(logs_dir).map_err(|e| format!("create dir: {e}"))?;
    let appender = Builder::new()
        .rotation(Rotation::DAILY)
        .filename_prefix("app")
        .filename_suffix("log")
        .max_log_files(5)
        .build(logs_dir)
        .map_err(|e| format!("appender: {e}"))?;
    tracing_subscriber::fmt()
        .with_writer(std::sync::Mutex::new(appender))
        .with_ansi(false)
        .with_max_level(tracing::Level::INFO)
        .try_init()
        .map_err(|e| format!("subscriber: {e}"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn lines_reach_the_file_immediately() {
        let dir = std::env::temp_dir().join(format!("wch-log-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        super::init(&dir).unwrap();
        tracing::info!("probe line");
        let text: String = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| std::fs::read_to_string(e.unwrap().path()).unwrap())
            .collect();
        assert!(text.contains("probe line"), "log was: {text:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
