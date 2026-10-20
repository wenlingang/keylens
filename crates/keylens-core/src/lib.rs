//! Keylens' platform-independent core: shortcut model, merging, search, trigger detection, languages and community data.
#![forbid(unsafe_code)]

pub mod community;
pub mod keys;
pub mod locale;
pub mod merge;
pub mod model;
pub mod search;
pub mod source;
pub mod trigger;
pub mod vscode;

pub use keys::{Chord, Key, KeyParseError, Modifiers, Os, Shortcut};
pub use locale::{Lang, Locale, Localized, resolve, resolve_preferred};
pub use merge::{group_entries, merge};
pub use model::{AppInfo, Entry, EntryId, EntrySource, Group};
pub use search::{KeyQuery, Match, filter};
pub use source::{AppShortcuts, ShortcutSource, SourceError, collect};
pub use trigger::{KeyEvent, Trigger, TriggerAction, TriggerDetector};
