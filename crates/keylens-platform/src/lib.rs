//! Platform layer interface. The macOS implementation is in `keylens-macos`; tests use `fake::FakePlatform`.

use std::any::Any;
use std::sync::mpsc::Sender;

use keylens_core::{AppInfo, Entry, KeyEvent, ShortcutSource, Trigger};

#[cfg(any(test, feature = "fake"))]
pub mod fake;
pub mod tray;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PermissionState {
    Granted,
    Denied,
}

#[derive(Debug, thiserror::Error)]
pub enum PlatformError {
    #[error("accessibility permission is not granted")]
    PermissionDenied,
    #[error("the menu item no longer exists")]
    Stale,
    #[error("the menu item is disabled")]
    Disabled,
    #[error("{0}")]
    Other(String),
}

/// Lifetime of one listener (key events, frontmost app changes). Dropping it stops listening.
pub struct ListenGuard(#[allow(dead_code)] Box<dyn Any + Send>);

impl ListenGuard {
    pub fn new(inner: impl Any + Send) -> Self {
        Self(Box::new(inner))
    }
}

pub trait Platform: Send + Sync {
    fn permission(&self) -> PermissionState;
    /// Opens the system's authorization UI.
    fn request_permission(&self);
    fn frontmost_app(&self) -> Option<AppInfo>;
    /// Data source that reads the frontmost app's menu.
    fn menu_source(&self) -> Box<dyn ShortcutSource>;
    /// Executes an entry: menu entries press the menu item, community entries synthesize key events.
    /// When a menu entry comes from an earlier read (`EntryId::snapshot` is not the latest),
    /// it must return `PlatformError::Stale`, or re-confirm by group, path and title before executing;
    /// it must never execute directly by `index`, which could press a different menu item.
    fn execute(&self, entry: &Entry) -> Result<(), PlatformError>;
    /// Starts sending global key events to `tx` until the returned guard is dropped.
    fn start_key_events(&self, tx: Sender<KeyEvent>) -> Result<ListenGuard, PlatformError>;
    /// Sends the new app to `tx` when the frontmost app changes, until the returned guard is dropped.
    /// Lets the UI layer prefetch menus early: an Electron app's first read takes about 900ms, longer than the long-press time.
    fn watch_frontmost(&self, tx: Sender<AppInfo>) -> Result<ListenGuard, PlatformError>;
    /// App icon (PNG-encoded). Returns `None` if unavailable; the UI shows a placeholder icon.
    fn app_icon(&self, app: &AppInfo) -> Option<Vec<u8>>;
    fn default_trigger(&self) -> Trigger;
}
