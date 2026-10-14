//! Menu tree traversal. Decoupled from the concrete AX implementation via the `MenuNode` trait, so it can be tested with fake data.

use std::time::Instant;

use keylens_core::{Entry, EntryId, EntrySource, Shortcut, SourceError};

use crate::convert::menu_chord;

/// Attributes of a menu node, corresponding to the AX attributes of the same names.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ItemInfo {
    pub role: String,
    pub title: String,
    pub enabled: bool,
    pub cmd_char: Option<String>,
    pub cmd_vkey: Option<i64>,
    pub cmd_modifiers: Option<i64>,
}

/// A menu node. When a read fails (especially a timeout) it returns an error and the traversal aborts immediately,
/// rather than treating the failed node as empty and continuing, which could make one read drag on for tens of seconds with no sign of failure.
pub trait MenuNode: Sized {
    fn info(&self) -> Result<ItemInfo, SourceError>;
    fn children(&self) -> Result<Vec<Self>, SourceError>;
}

/// Result of one menu read. `handles[i]` is the menu element for `entries[i]`.
pub struct MenuRead<N> {
    pub entries: Vec<Entry>,
    pub handles: Vec<N>,
}

/// Traverses the menu bar, collecting menu items that have shortcuts. The first top-level menu (the Apple menu) is skipped.
/// If any node errors or `deadline` is exceeded, the whole read fails.
pub fn collect<N: MenuNode>(
    menu_bar: &N,
    snapshot: u64,
    deadline: Instant,
) -> Result<MenuRead<N>, SourceError> {
    let mut walker = Walker {
        snapshot,
        deadline,
        read: MenuRead {
            entries: Vec::new(),
            handles: Vec::new(),
        },
    };
    for top in walker.check(menu_bar.children())?.into_iter().skip(1) {
        let group = walker.check(top.info())?.title;
        let mut path = Vec::new();
        for child in walker.check(top.children())? {
            walker.walk(child, &group, &mut path)?;
        }
    }
    Ok(walker.read)
}

struct Walker<N> {
    snapshot: u64,
    deadline: Instant,
    read: MenuRead<N>,
}

impl<N: MenuNode> Walker<N> {
    /// Passes the result through, and returns a timeout once the total time limit is exceeded.
    fn check<T>(&self, result: Result<T, SourceError>) -> Result<T, SourceError> {
        let value = result?;
        if Instant::now() >= self.deadline {
            return Err(SourceError::Timeout);
        }
        Ok(value)
    }

    fn walk(&mut self, node: N, group: &str, path: &mut Vec<String>) -> Result<(), SourceError> {
        let info = self.check(node.info())?;
        let children = self.check(node.children())?;
        if info.role != "AXMenuItem" {
            for child in children {
                self.walk(child, group, path)?;
            }
            return Ok(());
        }
        if !children.is_empty() {
            path.push(info.title.clone());
            for child in children {
                self.walk(child, group, path)?;
            }
            path.pop();
        }
        if info.title.is_empty() {
            return Ok(());
        }
        let Some(chord) = menu_chord(info.cmd_char.as_deref(), info.cmd_vkey, info.cmd_modifiers)
        else {
            return Ok(());
        };
        let index = self.read.entries.len() as u32;
        self.read.entries.push(Entry {
            id: EntryId {
                source: EntrySource::Menu,
                snapshot: self.snapshot,
                index,
            },
            group: group.to_string(),
            path: path.clone(),
            title: info.title,
            search_terms: Vec::new(),
            shortcut: Shortcut::single(chord),
            enabled: info.enabled,
        });
        self.read.handles.push(node);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use keylens_core::{Os, SourceError};
    use std::time::{Duration, Instant};

    fn later() -> Instant {
        Instant::now() + Duration::from_secs(60)
    }

    #[derive(Clone, Debug, PartialEq)]
    struct Node {
        info: ItemInfo,
        children: Vec<Node>,
        error: Option<SourceError>,
    }

    impl MenuNode for Node {
        fn info(&self) -> Result<ItemInfo, SourceError> {
            match &self.error {
                Some(e) => Err(e.clone()),
                None => Ok(self.info.clone()),
            }
        }
        fn children(&self) -> Result<Vec<Node>, SourceError> {
            Ok(self.children.clone())
        }
    }

    fn node(role: &str, title: &str, children: Vec<Node>) -> Node {
        Node {
            info: ItemInfo {
                role: role.into(),
                title: title.into(),
                enabled: true,
                ..ItemInfo::default()
            },
            children,
            error: None,
        }
    }

    fn item(title: &str, key: &str, mods: i64) -> Node {
        let mut n = node("AXMenuItem", title, vec![]);
        n.info.cmd_char = Some(key.into());
        n.info.cmd_modifiers = Some(mods);
        n
    }

    fn menu(items: Vec<Node>) -> Node {
        node("AXMenu", "", items)
    }

    fn bar_item(title: &str, items: Vec<Node>) -> Node {
        node("AXMenuBarItem", title, vec![menu(items)])
    }

    fn sample() -> Node {
        let mut disabled = item("拷贝", "C", 0);
        disabled.info.enabled = false;
        let mut font = node(
            "AXMenuItem",
            "字体",
            vec![menu(vec![item("显示字体", "T", 0), item("粗体", "B", 0)])],
        );
        font.info.cmd_char = None;
        node(
            "AXMenuBar",
            "",
            vec![
                bar_item("Apple", vec![item("锁定屏幕", "Q", 4)]),
                bar_item(
                    "文件",
                    vec![
                        item("新建", "N", 0),
                        node("AXMenuItem", "", vec![]),
                        item("存储为…", "S", 3),
                    ],
                ),
                bar_item(
                    "编辑",
                    vec![disabled, node("AXMenuItem", "没有快捷键", vec![])],
                ),
                bar_item("格式", vec![font]),
            ],
        )
    }

    #[test]
    fn collects_items_with_shortcuts_and_skips_the_apple_menu() {
        let read = collect(&sample(), 7, later()).unwrap();
        let summary: Vec<_> = read
            .entries
            .iter()
            .map(|e| {
                (
                    e.group.as_str(),
                    e.path.join("›"),
                    e.title.as_str(),
                    e.shortcut.display(Os::MacOS),
                )
            })
            .collect();
        assert_eq!(
            summary,
            [
                ("文件", String::new(), "新建", "⌘N".to_string()),
                ("文件", String::new(), "存储为…", "⌥⇧⌘S".to_string()),
                ("编辑", String::new(), "拷贝", "⌘C".to_string()),
                ("格式", "字体".to_string(), "显示字体", "⌘T".to_string()),
                ("格式", "字体".to_string(), "粗体", "⌘B".to_string()),
            ]
        );
    }

    #[test]
    fn ids_carry_snapshot_and_match_handles() {
        let read = collect(&sample(), 7, later()).unwrap();
        assert_eq!(read.entries.len(), read.handles.len());
        for (i, entry) in read.entries.iter().enumerate() {
            assert_eq!(
                entry.id,
                EntryId {
                    source: EntrySource::Menu,
                    snapshot: 7,
                    index: i as u32
                }
            );
            assert_eq!(read.handles[i].info.title, entry.title);
        }
    }

    #[test]
    fn disabled_state_is_kept() {
        let read = collect(&sample(), 1, later()).unwrap();
        let copy = read.entries.iter().find(|e| e.title == "拷贝").unwrap();
        assert!(!copy.enabled);
    }

    #[test]
    fn empty_menu_bar_yields_nothing() {
        let read = collect(&node("AXMenuBar", "", vec![]), 1, later()).unwrap();
        assert!(read.entries.is_empty());
    }

    #[test]
    fn a_timeout_anywhere_aborts_the_whole_read() {
        let mut stuck = item("卡住", "K", 0);
        stuck.error = Some(SourceError::Timeout);
        let bar = node(
            "AXMenuBar",
            "",
            vec![
                bar_item("Apple", vec![]),
                bar_item("文件", vec![item("新建", "N", 0), stuck]),
            ],
        );
        assert_eq!(collect(&bar, 1, later()).err(), Some(SourceError::Timeout));
    }

    #[test]
    fn exceeding_the_overall_deadline_is_a_timeout() {
        let past = Instant::now() - Duration::from_millis(1);
        assert_eq!(
            collect(&sample(), 1, past).err(),
            Some(SourceError::Timeout)
        );
    }
}
