//! Keylens 的平台无关核心：快捷键模型、合并、搜索、触发判断、语言和社区数据。
#![forbid(unsafe_code)]

pub mod keys;

pub use keys::{Chord, Key, KeyParseError, Modifiers, Os, Shortcut};
