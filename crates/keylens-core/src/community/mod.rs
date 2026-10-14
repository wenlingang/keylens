//! 社区快捷键数据：格式解析、校验、目录加载和数据来源。

mod catalog;
mod data;
mod raw;
mod source;

pub use catalog::{Catalog, builtin, load_dir};
pub use data::{AppData, AppIds, DataError, DataGroup, DataShortcut, parse_app_data};
pub use source::CommunitySource;
