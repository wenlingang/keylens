//! Keylens 的平台无关核心：快捷键模型、合并、搜索、触发判断、语言和社区数据。
#![forbid(unsafe_code)]

pub mod community;
pub mod keys;
pub mod locale;
pub mod merge;
pub mod model;
pub mod search;
pub mod trigger;

pub use keys::{Chord, Key, KeyParseError, Modifiers, Os, Shortcut};
pub use locale::{resolve, resolve_preferred, Lang, Locale, Localized};
pub use merge::{group_entries, merge};
pub use model::{AppInfo, Entry, EntryId, EntrySource, Group};
pub use search::{filter, Match};
pub use trigger::{KeyEvent, Trigger, TriggerAction, TriggerDetector};
