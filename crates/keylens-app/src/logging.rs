//! File logging (spec §9): daily files under ~/Library/Logs/Keylens, kept for 7 days.

use std::path::{Path, PathBuf};

use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};

pub fn log_dir() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join("Library").join("Logs").join("Keylens")
}

/// A daily-rolling appender in `dir`, or `None` if the directory cannot be created.
fn appender(dir: &Path) -> Option<RollingFileAppender> {
    std::fs::create_dir_all(dir).ok()?;
    RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix("keylens")
        .filename_suffix("log")
        .max_log_files(7)
        .build(dir)
        .ok()
}

/// Installs the global logger. Keep the returned guard alive for the whole process; dropping
/// it flushes and stops the background writer. Returns `None` (no logging) if `dir` is unusable.
pub fn init(dir: &Path) -> Option<WorkerGuard> {
    let (writer, guard) = tracing_appender::non_blocking(appender(dir)?);
    tracing_subscriber::fmt()
        .with_writer(writer)
        .with_ansi(false)
        .with_max_level(tracing::Level::INFO)
        .try_init()
        .ok()?;
    Some(guard)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_a_daily_log_file_and_survives_an_unwritable_dir() {
        assert!(appender(Path::new("/dev/null/keylens")).is_none());

        let dir = std::env::temp_dir().join(format!("keylens-log-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let appender = appender(&dir).expect("appender");
        let (writer, guard) = tracing_appender::non_blocking(appender);
        let subscriber = tracing_subscriber::fmt()
            .with_writer(writer)
            .with_ansi(false)
            .finish();
        tracing::subscriber::with_default(subscriber, || tracing::warn!("hello from the test"));
        drop(guard);
        let contents: String = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| std::fs::read_to_string(e.unwrap().path()).unwrap())
            .collect();
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(contents.contains("hello from the test"), "{contents}");
    }

    #[test]
    fn log_dir_is_under_library_logs() {
        assert!(log_dir().ends_with("Library/Logs/Keylens"));
    }
}
