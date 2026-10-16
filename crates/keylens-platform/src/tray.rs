//! Tray (menu bar / status area) commands and labels shared by every platform's tray.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayCommand {
    Permission,
    /// Create or open the user data file for the frontmost app.
    EditCurrentApp,
    OpenDataFolder,
    /// Open Keylens' settings window.
    Settings,
    /// Check for a new version now (and offer to install it).
    CheckForUpdates,
    Quit,
}

impl TrayCommand {
    pub const ALL: [TrayCommand; 6] = [
        TrayCommand::Permission,
        TrayCommand::EditCurrentApp,
        TrayCommand::OpenDataFolder,
        TrayCommand::Settings,
        TrayCommand::CheckForUpdates,
        TrayCommand::Quit,
    ];
}

pub struct TrayLabels {
    /// Menu bar title; "!" is appended while the permission is missing.
    pub icon: String,
    /// Disabled first menu item, e.g. "Keylens 0.1.0".
    pub version: String,
    pub permission_ok: String,
    pub permission_missing: String,
    pub edit_current: String,
    pub open_data: String,
    pub settings: String,
    pub check_updates: String,
    /// Replaces `check_updates` (disabled) while an update is being checked or installed.
    pub updating: String,
    pub quit: String,
}

/// An installed tray. Lives on the main thread; dropping it removes the tray.
pub trait TrayHandle {
    /// Updates the permission status: the icon and the permission menu item.
    fn set_permission(&self, granted: bool);
    /// Shows `text` (e.g. "2 data files have errors") in the menu, or hides the item for `None`.
    fn set_data_errors(&self, text: Option<&str>);
    /// Replaces every label (after the UI language changed), keeping the current state.
    fn set_labels(&self, labels: TrayLabels);
    /// Shows that an update check or install is running: the check-for-updates item reads
    /// `updating` and is disabled until `false`.
    fn set_updating(&self, updating: bool);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_lists_every_command_once() {
        assert_eq!(TrayCommand::ALL.len(), 6);
        assert!(TrayCommand::ALL.contains(&TrayCommand::Settings));
    }
}
