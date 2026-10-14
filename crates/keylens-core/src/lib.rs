//! Keylens 的平台无关核心：快捷键模型、合并、搜索、触发判断、语言和社区数据。
#![forbid(unsafe_code)]

pub mod keys;
pub mod merge;
pub mod model;
pub mod search;

pub use keys::{Chord, Key, KeyParseError, Modifiers, Os, Shortcut};
pub use merge::{group_entries, merge};
pub use model::{AppInfo, Entry, EntryId, EntrySource, Group};
pub use search::{filter, Match};
