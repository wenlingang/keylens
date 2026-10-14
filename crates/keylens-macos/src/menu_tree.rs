//! 菜单树遍历。通过 `MenuNode` trait 和具体的 AX 实现解耦，可以用假数据测试。

use keylens_core::{Entry, EntryId, EntrySource, Shortcut};

use crate::convert::menu_chord;

/// 一个菜单节点的属性，对应 AX 的同名属性。
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ItemInfo {
    pub role: String,
    pub title: String,
    pub enabled: bool,
    pub cmd_char: Option<String>,
    pub cmd_vkey: Option<i64>,
    pub cmd_modifiers: Option<i64>,
}

pub trait MenuNode: Sized {
    fn info(&self) -> ItemInfo;
    fn children(&self) -> Vec<Self>;
}

/// 一次菜单读取的结果。`handles[i]` 是 `entries[i]` 对应的菜单元素。
pub struct MenuRead<N> {
    pub entries: Vec<Entry>,
    pub handles: Vec<N>,
}

/// 遍历菜单栏，收集有快捷键的菜单项。第一个顶层菜单（苹果菜单）跳过。
pub fn collect<N: MenuNode>(menu_bar: &N, snapshot: u64) -> MenuRead<N> {
    let mut read = MenuRead {
        entries: Vec::new(),
        handles: Vec::new(),
    };
    for top in menu_bar.children().into_iter().skip(1) {
        let group = top.info().title;
        let mut path = Vec::new();
        for child in top.children() {
            walk(child, &group, &mut path, snapshot, &mut read);
        }
    }
    read
}

fn walk<N: MenuNode>(
    node: N,
    group: &str,
    path: &mut Vec<String>,
    snapshot: u64,
    read: &mut MenuRead<N>,
) {
    let info = node.info();
    if info.role != "AXMenuItem" {
        for child in node.children() {
            walk(child, group, path, snapshot, read);
        }
        return;
    }
    let children = node.children();
    if !children.is_empty() {
        path.push(info.title.clone());
        for child in children {
            walk(child, group, path, snapshot, read);
        }
        path.pop();
    }
    if info.title.is_empty() {
        return;
    }
    let Some(chord) = menu_chord(info.cmd_char.as_deref(), info.cmd_vkey, info.cmd_modifiers)
    else {
        return;
    };
    read.entries.push(Entry {
        id: EntryId {
            source: EntrySource::Menu,
            snapshot,
            index: read.entries.len() as u32,
        },
        group: group.to_string(),
        path: path.clone(),
        title: info.title,
        search_terms: Vec::new(),
        shortcut: Shortcut::single(chord),
        enabled: info.enabled,
    });
    read.handles.push(node);
}

#[cfg(test)]
mod tests {
    use super::*;
    use keylens_core::Os;

    #[derive(Clone, Debug, PartialEq)]
    struct Node {
        info: ItemInfo,
        children: Vec<Node>,
    }

    impl MenuNode for Node {
        fn info(&self) -> ItemInfo {
            self.info.clone()
        }
        fn children(&self) -> Vec<Node> {
            self.children.clone()
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
        let read = collect(&sample(), 7);
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
        let read = collect(&sample(), 7);
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
        let read = collect(&sample(), 1);
        let copy = read.entries.iter().find(|e| e.title == "拷贝").unwrap();
        assert!(!copy.enabled);
    }

    #[test]
    fn empty_menu_bar_yields_nothing() {
        let read = collect(&node("AXMenuBar", "", vec![]), 1);
        assert!(read.entries.is_empty());
    }
}
