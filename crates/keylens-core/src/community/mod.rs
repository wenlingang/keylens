//! 社区快捷键数据：格式解析、校验、目录加载和数据来源。

mod catalog;
mod data;
mod raw;
mod source;

pub use catalog::{builtin, load_dir, Catalog};
pub use data::{parse_app_data, AppData, AppIds, DataError, DataGroup, DataShortcut};
pub use source::CommunitySource;
