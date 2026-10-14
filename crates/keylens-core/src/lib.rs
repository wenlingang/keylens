//! Keylens 的平台无关核心：快捷键模型、合并、搜索、触发判断、语言和社区数据。
#![forbid(unsafe_code)]

pub mod community;
pub mod keys;
pub mod locale;
pub mod merge;
pub mod model;
pub mod search;
pub mod source;
pub mod trigger;

pub use keys::{Chord, Key, KeyParseError, Modifiers, Os, Shortcut};
pub use locale::{Lang, Locale, Localized, resolve, resolve_preferred};
pub use merge::{group_entries, merge};
pub use model::{AppInfo, Entry, EntryId, EntrySource, Group};
pub use search::{Match, filter};
pub use source::{AppShortcuts, ShortcutSource, SourceError, collect};
pub use trigger::{KeyEvent, Trigger, TriggerAction, TriggerDetector};
