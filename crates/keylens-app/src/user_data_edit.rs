//! Edits a user shortcut data file (`apps/<id>.toml`) in place with `toml_edit`: comments, i18n,
//! per-platform keys and unknown keys survive. Positions are `(group, index)` and checked against
//! the title the editor showed, so a file changed on disk is never edited at the wrong entry.

use keylens_core::{Os, Shortcut};
use toml_edit::{Array, ArrayOfTables, DocumentMut, Item, Table, value};

/// A shortcut's position: group index, then index inside the group.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct At {
    pub group: usize,
    pub index: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ShortcutView {
    pub title: String,
    /// The chords as written, e.g. ["Mod+K", "Mod+S"]; `None` when keys differ per platform.
    pub keys: Option<Vec<String>>,
    /// For display, e.g. "⌘K ⌘S"; the raw text when it does not parse.
    pub display: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GroupView {
    pub title: String,
    pub shortcuts: Vec<ShortcutView>,
}

/// What `remove` took out, so `restore` can put it back.
#[derive(Clone, Debug)]
pub struct Removed {
    pub at: At,
    pub title: String,
    group_title: String,
    table: Table,
    /// The group's own table (title, comments, extra keys) without its shortcuts, set when the
    /// group went with its last entry.
    group_removed: Option<Table>,
}

const GROUP_LAYOUT: &str = "`group` is not written as [[group]] tables; edit this file by hand";
const SHORTCUT_LAYOUT: &str = "`shortcut` is not written as [[group.shortcut]] tables";

fn parse(text: &str) -> Result<DocumentMut, String> {
    text.parse::<DocumentMut>().map_err(|e| e.to_string())
}

fn groups_mut(doc: &mut DocumentMut) -> Result<&mut ArrayOfTables, String> {
    if doc.get("group").is_none() {
        doc.insert("group", Item::ArrayOfTables(ArrayOfTables::new()));
    }
    doc.get_mut("group")
        .and_then(Item::as_array_of_tables_mut)
        .ok_or_else(|| {
            "`group` is not written as [[group]] tables; edit this file by hand".to_string()
        })
}

fn title_of(table: &Table) -> &str {
    table
        .get("title")
        .and_then(Item::as_str)
        .unwrap_or_default()
}

fn shortcuts_mut(group: &mut Table) -> Result<&mut ArrayOfTables, String> {
    if group.get("shortcut").is_none() {
        group.insert("shortcut", Item::ArrayOfTables(ArrayOfTables::new()));
    }
    group
        .get_mut("shortcut")
        .and_then(Item::as_array_of_tables_mut)
        .ok_or_else(|| SHORTCUT_LAYOUT.to_string())
}

fn keys_item(keys: &[String]) -> Item {
    match keys {
        [one] => value(one.as_str()),
        many => value(many.iter().map(String::as_str).collect::<Array>()),
    }
}

/// The chords of a `keys` value written as a string or an array; `None` for a per-platform table.
fn chords(item: &Item) -> Option<Vec<String>> {
    if let Some(s) = item.as_str() {
        return Some(vec![s.to_string()]);
    }
    let array = item.as_array()?;
    array
        .iter()
        .map(|v| v.as_str().map(str::to_string))
        .collect()
}

fn os_key(os: Os) -> &'static str {
    match os {
        Os::MacOS => "macos",
        Os::Windows => "windows",
        Os::Linux => "linux",
    }
}

pub fn list(text: &str, os: Os) -> Result<Vec<GroupView>, String> {
    let doc = parse(text)?;
    let Some(item) = doc.get("group") else {
        return Ok(Vec::new());
    };
    let groups = item.as_array_of_tables().ok_or(GROUP_LAYOUT)?;
    groups
        .iter()
        .map(|group| {
            let shortcuts = match group.get("shortcut") {
                None => Vec::new(),
                Some(item) => item
                    .as_array_of_tables()
                    .ok_or(SHORTCUT_LAYOUT)?
                    .iter()
                    .map(|entry| shortcut_view(entry, os))
                    .collect(),
            };
            Ok(GroupView {
                title: title_of(group).to_string(),
                shortcuts,
            })
        })
        .collect()
}

fn shortcut_view(item: &Table, os: Os) -> ShortcutView {
    let keys = item.get("keys").and_then(chords);
    let display = match &keys {
        Some(parts) => Shortcut::parse_seq(parts, os)
            .map(|s| s.display(os))
            .unwrap_or_else(|_| parts.join(" ")),
        None => item
            .get("keys")
            .and_then(Item::as_table_like)
            .and_then(|t| t.get(os_key(os)).or_else(|| t.get("default")))
            .and_then(chords)
            .and_then(|p| Shortcut::parse_seq(&p, os).ok())
            .map(|s| s.display(os))
            .unwrap_or_default(),
    };
    ShortcutView {
        title: title_of(item).to_string(),
        keys,
        display,
    }
}

/// Index of the group titled `title`, creating it at the end if missing.
fn group_index(groups: &mut ArrayOfTables, title: &str) -> usize {
    if let Some(i) = groups.iter().position(|g| title_of(g) == title) {
        return i;
    }
    let mut group = Table::new();
    group.insert("title", value(title));
    groups.push(group);
    groups.len() - 1
}

fn new_entry(title: &str, keys: &[String]) -> Table {
    let mut entry = Table::new();
    entry.insert("title", value(title));
    entry.insert("keys", keys_item(keys));
    entry
}

pub fn add(text: &str, group: &str, title: &str, keys: &[String]) -> Result<String, String> {
    let mut doc = parse(text)?;
    let groups = groups_mut(&mut doc)?;
    let g = group_index(groups, group);
    shortcuts_mut(groups.get_mut(g).expect("index from group_index"))?.push(new_entry(title, keys));
    Ok(finish(doc))
}

/// The entry at `at`, after checking it is still the one titled `expected`.
fn entry_mut<'a>(
    groups: &'a mut ArrayOfTables,
    at: At,
    expected: &str,
) -> Result<&'a mut Table, String> {
    let stale = || format!("the file changed: no shortcut “{expected}” at that position");
    let group = groups.get_mut(at.group).ok_or_else(stale)?;
    let entry = shortcuts_mut(group)?.get_mut(at.index).ok_or_else(stale)?;
    if title_of(entry) != expected {
        return Err(stale());
    }
    Ok(entry)
}

/// Replaces `key`'s value keeping its formatting (trailing comment, spacing); inserts if absent.
fn set_value(table: &mut Table, key: &str, new: Item) {
    let old = table.get_mut(key).and_then(Item::as_value_mut);
    match (old, new.into_value()) {
        (Some(old), Ok(mut new)) => {
            *new.decor_mut() = old.decor().clone();
            *old = new;
        }
        (None, Ok(new)) => {
            table.insert(key, Item::Value(new));
        }
        (_, Err(_)) => unreachable!("callers pass values"),
    }
}

pub fn update(
    text: &str,
    at: At,
    expected_title: &str,
    title: &str,
    keys: Option<&[String]>,
    group: &str,
) -> Result<String, String> {
    let mut doc = parse(text)?;
    let groups = groups_mut(&mut doc)?;
    let current_group = title_of(groups.get(at.group).ok_or("the file changed")?).to_string();
    let entry = entry_mut(groups, at, expected_title)?;
    if let Some(keys) = keys {
        if entry.get("keys").is_some_and(|k| chords(k).is_none()) {
            return Err("the file changed: this shortcut has per-platform keys".to_string());
        }
        set_value(entry, "keys", keys_item(keys));
    }
    set_value(entry, "title", value(title));
    if current_group != group {
        let (moved, _) = take(groups, at);
        let g = group_index(groups, group);
        shortcuts_mut(groups.get_mut(g).expect("index from group_index"))?.push(moved);
    }
    Ok(finish(doc))
}

/// Removes the entry at `at` (already validated) and, if its group is now empty, the group.
/// Returns the entry and, if the group went too, the group without its shortcuts.
fn take(groups: &mut ArrayOfTables, at: At) -> (Table, Option<Table>) {
    let group = groups.get_mut(at.group).expect("validated");
    let items = shortcuts_mut(group).expect("validated");
    let entry = items.remove(at.index);
    if !items.is_empty() {
        return (entry, None);
    }
    let mut shell = groups.remove(at.group);
    shell.remove("shortcut");
    (entry, Some(shell))
}

pub fn remove(text: &str, at: At, expected_title: &str) -> Result<(String, Removed), String> {
    let mut doc = parse(text)?;
    let groups = groups_mut(&mut doc)?;
    let group_title = title_of(groups.get(at.group).ok_or("the file changed")?).to_string();
    entry_mut(groups, at, expected_title)?;
    let (table, group_removed) = take(groups, at);
    let no_groups_left = groups.is_empty();
    if no_groups_left {
        doc.remove("group");
    }
    let removed = Removed {
        at,
        title: expected_title.to_string(),
        group_title,
        table,
        group_removed,
    };
    Ok((finish(doc), removed))
}

pub fn restore(text: &str, removed: &Removed) -> Result<String, String> {
    let mut doc = parse(text)?;
    let groups = groups_mut(&mut doc)?;
    let g = if let Some(shell) = &removed.group_removed {
        let index = removed.at.group.min(groups.len());
        groups.insert(index, shell.clone());
        index
    } else {
        let own = groups
            .get(removed.at.group)
            .is_some_and(|g| title_of(g) == removed.group_title);
        if own {
            removed.at.group
        } else {
            groups
                .iter()
                .position(|g| title_of(g) == removed.group_title)
                .ok_or("the group is gone")?
        }
    };
    let items = shortcuts_mut(groups.get_mut(g).expect("found"))?;
    items.insert(removed.at.index.min(items.len()), removed.table.clone());
    Ok(finish(doc))
}

/// Serializes `doc` with every table's position renumbered in tree order. `toml_edit` emits
/// tables sorted by position, and new tables have none, so without this a `[[group.shortcut]]`
/// added to the first group would be written after the last group and parse as part of it.
fn finish(mut doc: DocumentMut) -> String {
    let mut next = 0;
    renumber(doc.as_table_mut(), &mut next);
    doc.to_string()
}

fn renumber(table: &mut Table, next: &mut isize) {
    for (_, item) in table.iter_mut() {
        match item {
            Item::Table(t) => {
                t.set_position(Some(*next));
                *next += 1;
                renumber(t, next);
            }
            Item::ArrayOfTables(array) => {
                for t in array.iter_mut() {
                    t.set_position(Some(*next));
                    *next += 1;
                    renumber(t, next);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use keylens_core::community::parse_app_data;
    use keylens_core::{Os, Shortcut};

    use super::*;

    const TWO_GROUPS: &str = r#"# my shortcuts
schema = 1

[app]
name = "Demo"

[app.match]
macos = ["com.demo"]

[[group]]
title = "Files" # file stuff

  [[group.shortcut]]
  title = "Open"
  i18n = { zh-Hans = "打开" }
  keys = "Mod+O"

[[group]]
title = "View"

  [[group.shortcut]]
  title = "Zoom"
  keys = { default = "Mod+=", windows = "Ctrl+=" }
"#;

    const EMPTY: &str =
        "schema = 1\n\n[app]\nname = \"Demo\"\n\n[app.match]\nmacos = [\"com.demo\"]\n";

    fn keys(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|s| s.to_string()).collect()
    }

    /// Group titles with their shortcut titles, as the real parser sees them.
    fn parsed(text: &str) -> Vec<(String, Vec<String>)> {
        let data = parse_app_data("demo.toml", text).expect("the result parses");
        data.groups
            .iter()
            .map(|g| {
                (
                    g.title.en.clone(),
                    g.shortcuts.iter().map(|s| s.title.en.clone()).collect(),
                )
            })
            .collect()
    }

    #[test]
    fn list_shows_groups_titles_and_keys() {
        let groups = list(TWO_GROUPS, Os::MacOS).unwrap();
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].title, "Files");
        assert_eq!(groups[0].shortcuts[0].title, "Open");
        assert_eq!(groups[0].shortcuts[0].keys, Some(keys(&["Mod+O"])));
        assert_eq!(
            groups[0].shortcuts[0].display,
            Shortcut::parse_seq(&["Mod+O"], Os::MacOS)
                .unwrap()
                .display(Os::MacOS)
        );
        assert_eq!(
            groups[1].shortcuts[0].keys, None,
            "per-platform keys are not editable"
        );
        assert!(list(EMPTY, Os::MacOS).unwrap().is_empty());
    }

    #[test]
    fn adding_to_the_first_of_two_groups_stays_in_it() {
        let text = add(TWO_GROUPS, "Files", "Save", &keys(&["Mod+S"])).unwrap();
        assert_eq!(
            parsed(&text),
            vec![
                ("Files".into(), vec!["Open".into(), "Save".into()]),
                ("View".into(), vec!["Zoom".into()]),
            ]
        );
        assert!(text.contains("# my shortcuts") && text.contains("# file stuff"));
        assert!(text.contains("zh-Hans = \"打开\""));
        assert!(text.contains("windows = \"Ctrl+=\""));
    }

    #[test]
    fn adding_to_a_new_group_appends_it() {
        let text = add(TWO_GROUPS, "Edit", "Undo", &keys(&["Mod+Z"])).unwrap();
        assert_eq!(
            parsed(&text).last().unwrap(),
            &("Edit".to_string(), vec!["Undo".to_string()])
        );
    }

    #[test]
    fn adding_to_an_empty_file_creates_the_group() {
        let text = add(EMPTY, "General", "Find", &keys(&["Mod+K", "Mod+F"])).unwrap();
        assert_eq!(parsed(&text), vec![("General".into(), vec!["Find".into()])]);
        assert!(text.contains(r#"keys = ["Mod+K", "Mod+F"]"#), "{text}");
    }

    #[test]
    fn update_changes_title_and_keys_in_place() {
        let at = At { group: 0, index: 0 };
        let text = update(
            TWO_GROUPS,
            at,
            "Open",
            "Open File",
            Some(&keys(&["Mod+Shift+O"])),
            "Files",
        )
        .unwrap();
        let groups = list(&text, Os::MacOS).unwrap();
        assert_eq!(groups[0].shortcuts[0].title, "Open File");
        assert_eq!(groups[0].shortcuts[0].keys, Some(keys(&["Mod+Shift+O"])));
        assert!(text.contains("zh-Hans = \"打开\""), "other fields stay");
    }

    #[test]
    fn update_can_move_an_entry_to_another_group() {
        let at = At { group: 0, index: 0 };
        let text = update(TWO_GROUPS, at, "Open", "Open", None, "View").unwrap();
        assert_eq!(
            parsed(&text),
            vec![("View".into(), vec!["Zoom".into(), "Open".into()])]
        );
    }

    #[test]
    fn per_platform_keys_survive_a_title_edit() {
        let at = At { group: 1, index: 0 };
        let text = update(TWO_GROUPS, at, "Zoom", "Zoom In", None, "View").unwrap();
        assert!(text.contains("windows = \"Ctrl+=\""));
        assert_eq!(
            list(&text, Os::MacOS).unwrap()[1].shortcuts[0].title,
            "Zoom In"
        );
    }

    #[test]
    fn a_stale_position_is_refused() {
        let at = At { group: 0, index: 0 };
        assert!(update(TWO_GROUPS, at, "Something else", "X", None, "Files").is_err());
        assert!(remove(TWO_GROUPS, at, "Something else").is_err());
        assert!(remove(TWO_GROUPS, At { group: 5, index: 0 }, "Open").is_err());
    }

    #[test]
    fn undo_restores_a_removed_last_entry_and_its_group() {
        let (text, removed) = remove(TWO_GROUPS, At { group: 0, index: 0 }, "Open").unwrap();
        assert_eq!(parsed(&text), vec![("View".into(), vec!["Zoom".into()])]);
        let back = restore(&text, &removed).unwrap();
        assert_eq!(parsed(&back), parsed(TWO_GROUPS));
        assert!(back.contains("zh-Hans = \"打开\""));
        assert!(
            back.contains("# file stuff"),
            "the group's own comment comes back too"
        );
    }

    #[test]
    fn removing_the_only_entry_leaves_a_valid_file() {
        let text = add(EMPTY, "General", "Find", &keys(&["Mod+F"])).unwrap();
        let (text, _) = remove(&text, At { group: 0, index: 0 }, "Find").unwrap();
        assert!(list(&text, Os::MacOS).unwrap().is_empty());
        assert!(parse_app_data("demo.toml", &text).is_ok());
    }

    #[test]
    fn invalid_text_is_an_error() {
        assert!(list("schema = ", Os::MacOS).is_err());
        assert!(add("schema = ", "G", "T", &keys(&["Mod+T"])).is_err());
    }

    #[test]
    fn update_keeps_trailing_comments_and_indentation() {
        let src = "schema = 1\n\n[[group]]\ntitle = \"G\"\n\n  [[group.shortcut]]\n  title = \"Open\" # keep me\n  keys = \"Mod+O\" # keys note\n";
        let at = At { group: 0, index: 0 };
        let text = update(src, at, "Open", "Open File", Some(&keys(&["Mod+P"])), "G").unwrap();
        assert!(
            text.contains("# keep me") && text.contains("# keys note"),
            "{text}"
        );
        assert!(
            text.contains("  title = \"Open File\"") && text.contains("  keys = \"Mod+P\""),
            "{text}"
        );
        let groups = list(&text, Os::MacOS).unwrap();
        assert_eq!(groups[0].shortcuts[0].title, "Open File");
        assert_eq!(groups[0].shortcuts[0].keys, Some(keys(&["Mod+P"])));
    }

    #[test]
    fn new_keys_are_refused_for_per_platform_entries() {
        let at = At { group: 1, index: 0 };
        assert!(
            update(
                TWO_GROUPS,
                at,
                "Zoom",
                "Zoom",
                Some(&keys(&["Mod+P"])),
                "View"
            )
            .is_err()
        );
    }

    #[test]
    fn layouts_the_editor_does_not_write_are_errors() {
        let inline_groups = "schema = 1\ngroup = [{ title = \"G\", shortcut = [{ title = \"A\", keys = \"Mod+A\" }] }]\n";
        let inline_shortcuts = "schema = 1\n\n[[group]]\ntitle = \"G\"\nshortcut = [{ title = \"A\", keys = \"Mod+A\" }]\n";
        for src in [inline_groups, inline_shortcuts] {
            assert!(list(src, Os::MacOS).is_err(), "{src}");
            assert!(add(src, "G", "B", &keys(&["Mod+B"])).is_err(), "{src}");
        }
    }

    #[test]
    fn undo_goes_back_to_its_own_group_when_titles_repeat() {
        let src = "schema = 1\n\n[app]\nname = \"Demo\"\n\n[app.match]\nmacos = [\"com.demo\"]\n\n[[group]]\ntitle = \"G\"\n\n[[group.shortcut]]\ntitle = \"A\"\nkeys = \"Mod+A\"\n\n[[group]]\ntitle = \"G\"\n\n[[group.shortcut]]\ntitle = \"B\"\nkeys = \"Mod+B\"\n\n[[group.shortcut]]\ntitle = \"C\"\nkeys = \"Mod+C\"\n";
        let (text, removed) = remove(src, At { group: 1, index: 1 }, "C").unwrap();
        let back = restore(&text, &removed).unwrap();
        assert_eq!(
            parsed(&back),
            vec![
                ("G".into(), vec!["A".into()]),
                ("G".into(), vec!["B".into(), "C".into()])
            ]
        );
    }
}
