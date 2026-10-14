//! Community shortcut data: format parsing, validation, catalog loading and data sources.

mod catalog;
mod data;
mod raw;
mod source;

pub use catalog::{Catalog, builtin, load_dir};
pub use data::{AppData, AppIds, DataError, DataGroup, DataShortcut, parse_app_data};
pub use source::CommunitySource;
