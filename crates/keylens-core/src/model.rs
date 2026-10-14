//! Shortcut entries and app info.

use crate::keys::Shortcut;

/// Entry source; the declaration order is the merge priority (menu is highest).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum EntrySource {
    Menu,
    User,
    Community,
}

/// Unique entry ID; the platform layer uses it to find the menu element again.
///
/// `snapshot` identifies the menu read that produced this entry. The UI may still show an older read
/// while the background has already re-read, so the same `index` may be a different menu item in the new result.
/// Hence the platform layer must check the snapshot when executing a menu entry: on mismatch return `PlatformError::Stale`,
/// or re-confirm by group, path and title before executing. Community entries need no snapshot and use 0.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EntryId {
    pub source: EntrySource,
    pub snapshot: u64,
    pub index: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub id: EntryId,
    /// Top-level menu name or community data group name (localized).
    pub group: String,
    /// Submenu path between the top-level menu and the entry.
    pub path: Vec<String>,
    /// Localized display title.
    pub title: String,
    /// Text searched in addition to the title, e.g. titles in other languages.
    pub search_terms: Vec<String>,
    pub shortcut: Shortcut,
    pub enabled: bool,
}

impl Entry {
    pub fn source(&self) -> EntrySource {
        self.id.source
    }
}

/// The frontmost app. `ids` are the identifiers that identify the app on the current platform; on macOS, the bundle ID.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppInfo {
    pub pid: i32,
    pub name: String,
    pub ids: Vec<String>,
}

/// A group in the UI. When `community` is true, ◇ is shown next to the title.
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub title: String,
    pub community: bool,
    pub entries: Vec<Entry>,
}
