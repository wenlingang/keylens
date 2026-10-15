//! Tray (menu bar / status area) commands and labels shared by every platform's tray.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayCommand {
    Permission,
    /// Create or open the user data file for the frontmost app.
    EditCurrentApp,
    OpenDataFolder,
    Quit,
}

impl TrayCommand {
    pub const ALL: [TrayCommand; 4] = [
        TrayCommand::Permission,
        TrayCommand::EditCurrentApp,
        TrayCommand::OpenDataFolder,
        TrayCommand::Quit,
    ];
}

pub struct TrayLabels {
    /// Disabled first menu item, e.g. "Keylens 0.1.0".
    pub version: String,
    pub permission_ok: String,
    pub permission_missing: String,
    pub edit_current: String,
    pub open_data: String,
    pub quit: String,
}

/// An installed tray. Lives on the main thread; dropping it removes the tray.
pub trait TrayHandle {
    /// Updates the permission status: the icon and the permission menu item.
    fn set_permission(&self, granted: bool);
    /// Shows `text` (e.g. "2 data files have errors") in the menu, or hides the item for `None`.
    fn set_data_errors(&self, text: Option<&str>);
}
