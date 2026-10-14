//! 合并多个来源的条目，并按分组整理。

use std::collections::HashMap;

use crate::keys::Shortcut;
use crate::model::{Entry, EntrySource, Group};

/// `sources` 按优先级从高到低排列（菜单、用户数据、内置数据）。
/// 如果某个快捷键已经被更高优先级的来源占用，低优先级的条目会被丢弃，
/// 它的标题和搜索词会并入胜出条目的 `search_terms`。同一来源内部的重复条目都保留。
pub fn merge(sources: Vec<Vec<Entry>>) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::new();
    let mut owners: HashMap<Shortcut, usize> = HashMap::new();
    for source in sources {
        let mut claimed = Vec::new();
        for entry in source {
            if let Some(&winner) = owners.get(&entry.shortcut) {
                let winner = &mut out[winner];
                for term in std::iter::once(entry.title).chain(entry.search_terms) {
                    if term != winner.title && !winner.search_terms.contains(&term) {
                        winner.search_terms.push(term);
                    }
                }
                continue;
            }
            claimed.push((entry.shortcut.clone(), out.len()));
            out.push(entry);
        }
        for (shortcut, index) in claimed {
            owners.entry(shortcut).or_insert(index);
        }
    }
    out
}

/// 按分组整理，分组顺序以第一次出现为准。菜单分组和社区分组即使同名也分开。
pub fn group_entries(entries: Vec<Entry>) -> Vec<Group> {
    let mut groups: Vec<Group> = Vec::new();
    for entry in entries {
        let community = entry.source() != EntrySource::Menu;
        match groups
            .iter_mut()
            .find(|g| g.title == entry.group && g.community == community)
        {
            Some(group) => group.entries.push(entry),
            None => groups.push(Group {
                title: entry.group.clone(),
                community,
                entries: vec![entry],
            }),
        }
    }
    groups
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::{Os, Shortcut};
    use crate::model::{EntryId, EntrySource};

    fn entry(source: EntrySource, index: u32, group: &str, title: &str, keys: &str) -> Entry {
        Entry {
            id: EntryId { source, index },
            group: group.into(),
            path: vec![],
            title: title.into(),
            search_terms: vec![],
            shortcut: Shortcut::parse_seq(&[keys], Os::MacOS).unwrap(),
            enabled: true,
        }
    }

    #[test]
    fn higher_priority_source_wins_and_absorbs_titles() {
        let menu = vec![entry(EntrySource::Menu, 0, "文件", "保存", "Mod+S")];
        let mut community = entry(EntrySource::Community, 0, "General", "Save", "Mod+S");
        community.search_terms = vec!["存储".into()];
        let merged = merge(vec![menu, vec![], vec![community]]);
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].title, "保存");
        assert_eq!(merged[0].search_terms, ["Save", "存储"]);
    }

    #[test]
    fn duplicates_inside_one_source_are_kept() {
        let menu = vec![
            entry(EntrySource::Menu, 0, "文件", "关闭", "Mod+W"),
            entry(EntrySource::Menu, 1, "窗口", "关闭窗口", "Mod+W"),
        ];
        assert_eq!(merge(vec![menu]).len(), 2);
    }

    #[test]
    fn user_data_overrides_builtin_community_data() {
        let user = vec![entry(
            EntrySource::User,
            0,
            "Mine",
            "My Palette",
            "Mod+Shift+P",
        )];
        let builtin = vec![
            entry(
                EntrySource::Community,
                0,
                "General",
                "Command Palette",
                "Mod+Shift+P",
            ),
            entry(EntrySource::Community, 1, "General", "Quick Open", "Mod+P"),
        ];
        let merged = merge(vec![vec![], user, builtin]);
        let titles: Vec<_> = merged.iter().map(|e| e.title.as_str()).collect();
        assert_eq!(titles, ["My Palette", "Quick Open"]);
    }

    #[test]
    fn search_terms_are_not_duplicated() {
        let mut menu = entry(EntrySource::Menu, 0, "文件", "保存", "Mod+S");
        menu.search_terms = vec!["Save".into()];
        let community = entry(EntrySource::Community, 0, "General", "Save", "Mod+S");
        let merged = merge(vec![vec![menu], vec![community]]);
        assert_eq!(merged[0].search_terms, ["Save"]);
    }

    #[test]
    fn groups_keep_first_seen_order_and_separate_community() {
        let entries = vec![
            entry(EntrySource::Menu, 0, "文件", "保存", "Mod+S"),
            entry(EntrySource::Menu, 1, "编辑", "拷贝", "Mod+C"),
            entry(EntrySource::Menu, 2, "文件", "打开", "Mod+O"),
            entry(EntrySource::Community, 0, "文件", "Zen", "Mod+K"),
        ];
        let groups = group_entries(entries);
        let summary: Vec<_> = groups
            .iter()
            .map(|g| (g.title.as_str(), g.community, g.entries.len()))
            .collect();
        assert_eq!(
            summary,
            [("文件", false, 2), ("编辑", false, 1), ("文件", true, 1)]
        );
    }
}
