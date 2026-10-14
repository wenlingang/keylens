//! Merges entries from multiple sources and organizes them into groups.

use std::collections::HashMap;

use crate::keys::Shortcut;
use crate::model::{Entry, EntrySource, Group};

/// `sources` is ordered from highest to lowest priority (menu, user data, built-in data). Shortcuts are compared by `Shortcut::canonical`.
/// If a shortcut is already taken by a higher-priority source, the lower-priority entry is dropped,
/// and its title and search terms are merged into the winning entry's `search_terms`. Duplicates within the same source are all kept.
pub fn merge(sources: Vec<Vec<Entry>>) -> Vec<Entry> {
    let mut out: Vec<Entry> = Vec::new();
    let mut owners: HashMap<Shortcut, usize> = HashMap::new();
    for source in sources {
        let mut claimed = Vec::new();
        for entry in source {
            if let Some(&winner) = owners.get(&entry.shortcut.canonical()) {
                let winner = &mut out[winner];
                for term in std::iter::once(entry.title).chain(entry.search_terms) {
                    if term != winner.title && !winner.search_terms.contains(&term) {
                        winner.search_terms.push(term);
                    }
                }
                continue;
            }
            claimed.push((entry.shortcut.canonical(), out.len()));
            out.push(entry);
        }
        for (shortcut, index) in claimed {
            owners.entry(shortcut).or_insert(index);
        }
    }
    out
}

/// Organizes entries into groups; group order follows first appearance. Menu groups and community groups stay separate even with the same name.
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
    use crate::keys::{Chord, Key, Modifiers, Os, Shortcut};
    use crate::model::{EntryId, EntrySource};

    fn entry(source: EntrySource, index: u32, group: &str, title: &str, keys: &str) -> Entry {
        Entry {
            id: EntryId {
                source,
                snapshot: 0,
                index,
            },
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

    fn menu_entry(index: u32, title: &str, modifiers: Modifiers, c: char) -> Entry {
        let mut e = entry(EntrySource::Menu, index, "显示", title, "Mod+A");
        e.shortcut = Shortcut::single(Chord {
            modifiers,
            key: Key::from_char(c),
        });
        e
    }

    #[test]
    fn shifted_symbols_from_menus_match_their_base_key_with_shift() {
        // In Chrome's menu "Zoom In" is ⌘+, while community data writes it as ⇧⌘=; they are the same shortcut
        let menu = vec![
            menu_entry(0, "放大", Modifiers::META, '+'),
            menu_entry(1, "选择下一个标签", Modifiers::META, '}'),
            menu_entry(2, "后退", Modifiers::META, '['),
        ];
        let community = vec![
            entry(EntrySource::Community, 0, "Page", "Zoom In", "Cmd+Shift+="),
            entry(EntrySource::Community, 1, "Tabs", "Next Tab", "Cmd+Shift+]"),
            entry(
                EntrySource::Community,
                2,
                "Tabs",
                "Previous Tab",
                "Cmd+Shift+[",
            ),
        ];
        let merged = merge(vec![menu, community]);
        let titles: Vec<_> = merged.iter().map(|e| e.title.as_str()).collect();
        // ⇧⌘[ and ⌘[ are different shortcuts and must not be merged
        assert_eq!(titles, ["放大", "选择下一个标签", "后退", "Previous Tab"]);
        assert_eq!(merged[0].search_terms, ["Zoom In"]);
        assert_eq!(merged[0].shortcut.display(Os::MacOS), "⌘+");
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
