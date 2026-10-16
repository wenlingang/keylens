//! File logging (spec §9): daily files, kept for 7 days, under ~/Library/Logs/Keylens on macOS
//! and $XDG_STATE_HOME/keylens/logs (default ~/.local/state/keylens/logs) elsewhere.

use std::path::{Path, PathBuf};

use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};

/// The log folder; development builds log next to it, in `<folder>-dev`, so their logs never mix
/// with an installed release's.
pub fn log_dir(dev: bool) -> PathBuf {
    let dir = log_dir_in(
        &crate::config::home_dir(),
        std::env::var_os("XDG_STATE_HOME").map(PathBuf::from),
    );
    if dev { dev_variant(&dir) } else { dir }
}

fn dev_variant(dir: &Path) -> PathBuf {
    let mut name = dir.file_name().unwrap_or_default().to_os_string();
    name.push("-dev");
    dir.with_file_name(name)
}

fn log_dir_in(home: &Path, xdg_state_home: Option<PathBuf>) -> PathBuf {
    if cfg!(target_os = "macos") {
        return home.join("Library").join("Logs").join("Keylens");
    }
    xdg_state_home
        .filter(|dir| dir.is_absolute())
        .unwrap_or_else(|| home.join(".local").join("state"))
        .join("keylens")
        .join("logs")
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
    fn log_dir_follows_the_platform_convention() {
        let home = Path::new("/home/u");
        if cfg!(target_os = "macos") {
            assert_eq!(
                log_dir_in(home, Some("/xdg".into())),
                Path::new("/home/u/Library/Logs/Keylens")
            );
        } else {
            assert_eq!(
                log_dir_in(home, None),
                Path::new("/home/u/.local/state/keylens/logs")
            );
            assert_eq!(
                log_dir_in(home, Some("/xdg".into())),
                Path::new("/xdg/keylens/logs")
            );
        }
    }

    #[test]
    fn dev_builds_log_next_to_the_release_folder() {
        assert_eq!(
            dev_variant(Path::new("/home/u/Library/Logs/Keylens")),
            Path::new("/home/u/Library/Logs/Keylens-dev")
        );
    }
}
