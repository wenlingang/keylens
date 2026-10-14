//! 快捷键数据来源的接口，以及把多个来源汇总成界面数据。

use crate::merge::{group_entries, merge};
use crate::model::{AppInfo, Entry, Group};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SourceError {
    #[error("permission denied")]
    PermissionDenied,
    #[error("the app did not respond in time")]
    Timeout,
    #[error("{0}")]
    Other(String),
}

pub trait ShortcutSource: Send + Sync {
    fn load(&self, app: &AppInfo) -> Result<Vec<Entry>, SourceError>;
}

/// 一个应用的全部快捷键，已经合并去重并分组。
#[derive(Clone, Debug, PartialEq)]
pub struct AppShortcuts {
    pub app: AppInfo,
    pub groups: Vec<Group>,
    pub warnings: Vec<SourceError>,
}

/// `sources` 按优先级从高到低排列。某个来源出错时记进 `warnings`，不影响其他来源。
pub fn collect(app: AppInfo, sources: &[&dyn ShortcutSource]) -> AppShortcuts {
    let mut loaded = Vec::new();
    let mut warnings = Vec::new();
    for source in sources {
        match source.load(&app) {
            Ok(entries) => loaded.push(entries),
            Err(error) => warnings.push(error),
        }
    }
    AppShortcuts { app, groups: group_entries(merge(loaded)), warnings }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::{Os, Shortcut};
    use crate::model::{EntryId, EntrySource};

    struct Fixed(Result<Vec<Entry>, SourceError>);

    impl ShortcutSource for Fixed {
        fn load(&self, _: &AppInfo) -> Result<Vec<Entry>, SourceError> {
            self.0.clone()
        }
    }

    fn app() -> AppInfo {
        AppInfo { pid: 1, name: "Demo".into(), ids: vec!["com.example.demo".into()] }
    }

    fn entry(source: EntrySource, title: &str, keys: &str) -> Entry {
        Entry {
            id: EntryId { source, index: 0 },
            group: "G".into(),
            path: vec![],
            title: title.into(),
            search_terms: vec![],
            shortcut: Shortcut::parse_seq(&[keys], Os::MacOS).unwrap(),
            enabled: true,
        }
    }

    #[test]
    fn failing_source_becomes_a_warning() {
        let menu = Fixed(Err(SourceError::Timeout));
        let community = Fixed(Ok(vec![entry(EntrySource::Community, "Save", "Mod+S")]));
        let result = collect(app(), &[&menu, &community]);
        assert_eq!(result.warnings, [SourceError::Timeout]);
        assert_eq!(result.groups.len(), 1);
        assert!(result.groups[0].community);
    }

    #[test]
    fn sources_are_merged_by_priority() {
        let menu = Fixed(Ok(vec![entry(EntrySource::Menu, "保存", "Mod+S")]));
        let community = Fixed(Ok(vec![entry(EntrySource::Community, "Save", "Mod+S")]));
        let result = collect(app(), &[&menu, &community]);
        assert_eq!(result.groups.len(), 1);
        assert_eq!(result.groups[0].entries[0].title, "保存");
        assert_eq!(result.groups[0].entries[0].search_terms, ["Save"]);
    }
}
