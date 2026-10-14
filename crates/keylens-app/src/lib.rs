//! Keylens 应用：浮层界面与控制逻辑。
#![forbid(unsafe_code)]

rust_i18n::i18n!("locales", fallback = "en");

pub mod config;
pub mod loader;
pub mod model;
pub mod strings;
