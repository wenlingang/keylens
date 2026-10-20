//! Applies VS Code `keybindings.json` customizations on top of community shortcut entries.

use std::collections::HashMap;

use crate::keys::Shortcut;
use crate::model::{Entry, EntryId, EntrySource};
use crate::vscode::keybindings::Binding;

/// An entry being built, with its command and a sort key that keeps the original order.
struct Item {
    entry: Entry,
    command: Option<String>,
    /// Position of the entry (or of the command's first entry) in the merged layers; for custom
    /// entries, the order in which their command was first bound.
    anchor: usize,
    /// Whether the entry belongs to the custom group, which sorts after all other groups.
    custom: bool,
}

/// Applies VS Code `keybindings.json` semantics to community entries.
///
/// `layers` are ordered from highest to lowest priority (user toml, built-in toml); each entry
/// carries its VS Code command ID if it has one. A lower-layer entry is dropped when a higher
/// layer already has its command or its shortcut; like `merge`, its title and search terms are
/// then added to the winner's `search_terms`. Then, in file order: `{key, -C}`
/// removes C's binding on that key, `{-C}` removes all of C's bindings, and `{key, C}` adds a
/// binding for C, evicting whatever else is on that key. Commands without a community entry go
/// into `custom_group`, which is placed after all other groups. `when` clauses are ignored.
pub fn apply(
    layers: Vec<Vec<(Entry, Option<String>)>>,
    bindings: &[Binding],
    custom_group: &str,
) -> Vec<Entry> {
    let mut items: Vec<Item> = Vec::new();
    // Commands and shortcuts claimed by higher layers, mapped to the winning item.
    let mut command_owners: HashMap<String, usize> = HashMap::new();
    let mut key_owners: HashMap<Shortcut, usize> = HashMap::new();
    for layer in layers {
        let mut claimed = Vec::new();
        for (entry, command) in layer {
            let key = entry.shortcut.canonical();
            let winner = command
                .as_ref()
                .and_then(|c| command_owners.get(c))
                .or_else(|| key_owners.get(&key));
            if let Some(&winner) = winner {
                let winner = &mut items[winner].entry;
                for term in std::iter::once(entry.title).chain(entry.search_terms) {
                    if term != winner.title && !winner.search_terms.contains(&term) {
                        winner.search_terms.push(term);
                    }
                }
                continue;
            }
            let anchor = items.len();
            claimed.push((command.clone(), key, anchor));
            items.push(Item {
                entry,
                command,
                anchor,
                custom: false,
            });
        }
        for (command, key, index) in claimed {
            if let Some(command) = command {
                command_owners.entry(command).or_insert(index);
            }
            key_owners.entry(key).or_insert(index);
        }
    }

    // Group order and per-command templates come from the merged entries, before any removal.
    let mut group_rank: HashMap<String, usize> = HashMap::new();
    let mut templates: HashMap<String, (Entry, usize, bool)> = HashMap::new();
    for item in &items {
        let next = group_rank.len();
        group_rank.entry(item.entry.group.clone()).or_insert(next);
        if let Some(c) = &item.command {
            templates
                .entry(c.clone())
                .or_insert_with(|| (item.entry.clone(), item.anchor, false));
        }
    }

    let mut custom_index = 0u32;
    for binding in bindings {
        let command = binding.command.as_str();
        let key = binding.key.as_ref().map(Shortcut::canonical);
        if binding.remove {
            items.retain(|item| {
                item.command.as_deref() != Some(command)
                    || key
                        .as_ref()
                        .is_some_and(|k| *k != item.entry.shortcut.canonical())
            });
            continue;
        }
        let (Some(shortcut), Some(key)) = (&binding.key, key) else {
            continue;
        };
        items.retain(|item| item.entry.shortcut.canonical() != key);
        let (mut entry, anchor, custom) = match templates.get(command) {
            Some((template, anchor, custom)) => (template.clone(), *anchor, *custom),
            None => {
                let entry = Entry {
                    id: EntryId {
                        source: EntrySource::User,
                        snapshot: 0,
                        index: custom_index,
                    },
                    group: custom_group.to_string(),
                    path: Vec::new(),
                    title: title_from_command(command),
                    search_terms: vec![command.to_string()],
                    shortcut: shortcut.clone(),
                    enabled: true,
                };
                let anchor = custom_index as usize;
                custom_index += 1;
                templates.insert(command.to_string(), (entry.clone(), anchor, true));
                (entry, anchor, true)
            }
        };
        entry.shortcut = shortcut.clone();
        items.push(Item {
            entry,
            command: Some(command.to_string()),
            anchor,
            custom,
        });
    }

    // Stable sort: an added binding lands right after its command's first original entry.
    items.sort_by_key(|item| {
        let rank = if item.custom {
            usize::MAX
        } else {
            group_rank[&item.entry.group]
        };
        (rank, item.anchor)
    });
    items.into_iter().map(|item| item.entry).collect()
}

/// Derives a readable title from a command ID: the segment after the last `.`, split on camel
/// case, `-` and `_`, each word capitalized, e.g. `gitlens.showCommitSearch` → `Show Commit Search`.
pub fn title_from_command(command: &str) -> String {
    let last = command.rsplit('.').next().unwrap_or(command);
    let mut words: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut prev_lower = false;
    for c in last.chars() {
        if c == '-' || c == '_' {
            words.push(std::mem::take(&mut current));
            prev_lower = false;
            continue;
        }
        if c.is_uppercase() && prev_lower {
            words.push(std::mem::take(&mut current));
        }
        prev_lower = c.is_lowercase() || c.is_ascii_digit();
        current.push(c);
    }
    words.push(current);
    words
        .iter()
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut chars = w.chars();
            let first = chars.next().map(|c| c.to_uppercase().collect::<String>());
            first.unwrap_or_default() + chars.as_str()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::{Os, Shortcut};
    use crate::model::{EntryId, EntrySource};

    const CUSTOM: &str = "Custom";

    fn keys(text: &str) -> Shortcut {
        let parts: Vec<&str> = text.split_whitespace().collect();
        Shortcut::parse_seq(&parts, Os::Windows).unwrap()
    }

    fn entry(title: &str, keys_text: &str, source: EntrySource) -> Entry {
        Entry {
            id: EntryId {
                source,
                snapshot: 0,
                index: 0,
            },
            group: "General".into(),
            path: vec![],
            title: title.into(),
            search_terms: vec![],
            shortcut: keys(keys_text),
            enabled: true,
        }
    }

    fn with_cmd(e: Entry, command: &str) -> (Entry, Option<String>) {
        (e, Some(command.into()))
    }

    fn add(key: &str, command: &str) -> Binding {
        Binding {
            key: Some(keys(key)),
            command: command.into(),
            remove: false,
        }
    }

    fn remove(key: Option<&str>, command: &str) -> Binding {
        Binding {
            key: key.map(keys),
            command: command.into(),
            remove: true,
        }
    }

    fn summary(entries: &[Entry]) -> Vec<(String, String)> {
        entries
            .iter()
            .map(|e| (e.title.clone(), e.shortcut.display(Os::Windows)))
            .collect()
    }

    fn pairs(items: &[(&str, &str)]) -> Vec<(String, String)> {
        items
            .iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    }

    #[test]
    fn user_toml_wins_over_builtin_for_same_command() {
        let user = vec![with_cmd(entry("Mine", "Ctrl+1", EntrySource::User), "C")];
        let builtin = vec![with_cmd(
            entry("Builtin", "Ctrl+2", EntrySource::Community),
            "C",
        )];
        let out = apply(vec![user, builtin], &[], CUSTOM);
        assert_eq!(summary(&out), pairs(&[("Mine", "Ctrl+1")]));
        assert_eq!(out[0].source(), EntrySource::User);
    }

    #[test]
    fn added_binding_keeps_default_and_adds_new_key() {
        let builtin = vec![with_cmd(
            entry("Quick Open", "Ctrl+P", EntrySource::Community),
            "C",
        )];
        let out = apply(vec![builtin], &[add("ctrl+alt+p", "C")], CUSTOM);
        assert_eq!(
            summary(&out),
            pairs(&[("Quick Open", "Ctrl+P"), ("Quick Open", "Ctrl+Alt+P")])
        );
    }

    #[test]
    fn removal_with_key_drops_only_that_key() {
        let builtin = vec![
            with_cmd(entry("Add Next", "Ctrl+D", EntrySource::Community), "C"),
            with_cmd(entry("Other", "Ctrl+E", EntrySource::Community), "D"),
        ];
        let out = apply(vec![builtin], &[remove(Some("ctrl+d"), "C")], CUSTOM);
        assert_eq!(summary(&out), pairs(&[("Other", "Ctrl+E")]));
    }

    #[test]
    fn removal_with_key_keeps_other_keys_of_same_command() {
        let builtin = vec![
            with_cmd(entry("C", "Ctrl+D", EntrySource::Community), "C"),
            with_cmd(entry("C", "Ctrl+K", EntrySource::Community), "C"),
        ];
        let out = apply(vec![builtin], &[remove(Some("ctrl+d"), "C")], CUSTOM);
        assert_eq!(summary(&out), pairs(&[("C", "Ctrl+K")]));
    }

    #[test]
    fn removal_without_key_drops_all_defaults() {
        let builtin = || {
            vec![
                with_cmd(entry("Quick Open", "Ctrl+P", EntrySource::Community), "C"),
                with_cmd(entry("Quick Open", "Ctrl+E", EntrySource::Community), "C"),
            ]
        };
        let out = apply(vec![builtin()], &[remove(None, "C")], CUSTOM);
        assert!(out.is_empty());

        let out = apply(
            vec![builtin()],
            &[remove(None, "C"), add("ctrl+j", "C")],
            CUSTOM,
        );
        assert_eq!(summary(&out), pairs(&[("Quick Open", "Ctrl+J")]));
    }

    #[test]
    fn rebinding_a_key_evicts_the_previous_owner() {
        let builtin = vec![
            with_cmd(
                entry("Toggle Sidebar", "Ctrl+B", EntrySource::Community),
                "C",
            ),
            with_cmd(entry("Toggle Panel", "Ctrl+J", EntrySource::Community), "D"),
        ];
        let out = apply(vec![builtin], &[add("ctrl+b", "D")], CUSTOM);
        let titles: Vec<_> = out.iter().map(|e| e.title.as_str()).collect();
        assert!(!titles.contains(&"Toggle Sidebar"));
        let on_b: Vec<_> = out
            .iter()
            .filter(|e| e.shortcut.canonical() == keys("Ctrl+B").canonical())
            .map(|e| e.title.as_str())
            .collect();
        assert_eq!(on_b, ["Toggle Panel"]);
        assert_eq!(
            summary(&out),
            pairs(&[("Toggle Panel", "Ctrl+J"), ("Toggle Panel", "Ctrl+B")])
        );
    }

    #[test]
    fn unknown_command_goes_to_custom_group() {
        let builtin = vec![with_cmd(
            entry("Quick Open", "Ctrl+P", EntrySource::Community),
            "C",
        )];
        let out = apply(
            vec![builtin],
            &[add("ctrl+shift+l", "gitlens.showCommitSearch")],
            CUSTOM,
        );
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].title, "Quick Open");
        let custom = &out[1];
        assert_eq!(custom.group, CUSTOM);
        assert_eq!(custom.title, "Show Commit Search");
        assert_eq!(custom.source(), EntrySource::User);
        assert!(custom.path.is_empty());
        assert!(
            custom
                .search_terms
                .contains(&"gitlens.showCommitSearch".to_string())
        );
        assert_eq!(custom.shortcut.display(Os::Windows), "Ctrl+Shift+L");
    }

    #[test]
    fn custom_group_comes_last_and_group_order_is_kept() {
        let mut a = entry("A", "Ctrl+1", EntrySource::Community);
        a.group = "Edit".into();
        let mut b = entry("B", "Ctrl+2", EntrySource::Community);
        b.group = "View".into();
        let builtin = vec![with_cmd(a, "a"), with_cmd(b, "b")];
        let out = apply(
            vec![builtin],
            &[add("ctrl+9", "ext.foo"), add("ctrl+3", "a")],
            CUSTOM,
        );
        let groups: Vec<_> = out.iter().map(|e| e.group.as_str()).collect();
        assert_eq!(groups, ["Edit", "Edit", "View", CUSTOM]);
    }

    #[test]
    fn entries_without_command_dedupe_by_shortcut() {
        let user = vec![(entry("X", "Ctrl+Q", EntrySource::User), None)];
        let builtin = vec![(entry("Y", "Ctrl+Q", EntrySource::Community), None)];
        let out = apply(vec![user, builtin], &[], CUSTOM);
        assert_eq!(summary(&out), pairs(&[("X", "Ctrl+Q")]));
    }

    #[test]
    fn higher_layer_without_command_hides_lower_command_on_same_key() {
        let user = vec![(entry("X", "Ctrl+Q", EntrySource::User), None)];
        let builtin = vec![with_cmd(entry("C", "Ctrl+Q", EntrySource::Community), "C")];
        let out = apply(vec![user, builtin], &[], CUSTOM);
        assert_eq!(summary(&out), pairs(&[("X", "Ctrl+Q")]));
        assert_eq!(out[0].source(), EntrySource::User);
    }

    #[test]
    fn higher_layer_command_hides_lower_entry_without_command_on_same_key() {
        let user = vec![with_cmd(entry("C", "Ctrl+Q", EntrySource::User), "C")];
        let builtin = vec![(entry("Y", "Ctrl+Q", EntrySource::Community), None)];
        let out = apply(vec![user, builtin], &[], CUSTOM);
        assert_eq!(summary(&out), pairs(&[("C", "Ctrl+Q")]));
        assert_eq!(out[0].source(), EntrySource::User);
    }

    #[test]
    fn dropped_lower_layer_titles_stay_searchable() {
        let user = vec![with_cmd(entry("Mine", "Ctrl+1", EntrySource::User), "C")];
        let mut builtin_c = entry("Builtin", "Ctrl+2", EntrySource::Community);
        builtin_c.search_terms = vec!["内置".into(), "Mine".into()];
        let builtin = vec![
            with_cmd(builtin_c, "C"),
            (entry("Other", "Ctrl+1", EntrySource::Community), None),
        ];
        let out = apply(vec![user, builtin], &[], CUSTOM);
        assert_eq!(summary(&out), pairs(&[("Mine", "Ctrl+1")]));
        assert_eq!(out[0].search_terms, ["Builtin", "内置", "Other"]);
    }

    #[test]
    fn title_from_command_examples() {
        assert_eq!(
            title_from_command("editor.action.copyLinesDownAction"),
            "Copy Lines Down Action"
        );
        assert_eq!(title_from_command("workbench.action.files.save"), "Save");
        assert_eq!(
            title_from_command("gitlens.showCommitSearch"),
            "Show Commit Search"
        );
        assert_eq!(title_from_command("extension.my-cmd_name"), "My Cmd Name");
    }
}
