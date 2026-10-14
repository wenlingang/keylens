//! macOS implementation of `Platform`.

use std::sync::Arc;
use std::sync::mpsc::Sender;

use keylens_core::{AppInfo, Entry, EntrySource, KeyEvent, Modifiers, ShortcutSource, Trigger};
use keylens_platform::{ListenGuard, PermissionState, Platform, PlatformError};

use crate::ax::{self, AxNode};
use crate::menu::AxMenuSource;
use crate::snapshots::Snapshots;
use crate::{exec, keytap, workspace};

/// Maximum number of apps whose menu snapshots are kept (spec §5).
const SNAPSHOT_CAPACITY: usize = 8;

pub struct MacPlatform {
    snapshots: Arc<Snapshots<AxNode>>,
}

impl Default for MacPlatform {
    fn default() -> Self {
        Self {
            snapshots: Arc::new(Snapshots::new(SNAPSHOT_CAPACITY)),
        }
    }
}

impl Platform for MacPlatform {
    fn permission(&self) -> PermissionState {
        if ax::is_trusted() {
            PermissionState::Granted
        } else {
            PermissionState::Denied
        }
    }

    fn request_permission(&self) {
        ax::request_trust();
        workspace::open_accessibility_settings();
    }

    fn frontmost_app(&self) -> Option<AppInfo> {
        workspace::frontmost_app()
    }

    fn menu_source(&self) -> Box<dyn ShortcutSource> {
        Box::new(AxMenuSource {
            snapshots: self.snapshots.clone(),
        })
    }

    fn execute(&self, entry: &Entry) -> Result<(), PlatformError> {
        if !entry.enabled {
            return Err(PlatformError::Disabled);
        }
        match entry.source() {
            EntrySource::Menu => exec::press_menu(&self.snapshots, entry),
            EntrySource::User | EntrySource::Community => exec::post_shortcut(&entry.shortcut),
        }
    }

    fn start_key_events(&self, tx: Sender<KeyEvent>) -> Result<ListenGuard, PlatformError> {
        if !ax::is_trusted() {
            return Err(PlatformError::PermissionDenied);
        }
        keytap::start(tx)
    }

    fn watch_frontmost(&self, tx: Sender<AppInfo>) -> Result<ListenGuard, PlatformError> {
        Ok(workspace::watch_frontmost(tx))
    }

    fn app_icon(&self, app: &AppInfo) -> Option<Vec<u8>> {
        workspace::app_icon(app.pid)
    }

    fn default_trigger(&self) -> Trigger {
        Trigger::HoldModifier {
            key: Modifiers::META,
            delay: Trigger::DEFAULT_HOLD_DELAY,
        }
    }
}
