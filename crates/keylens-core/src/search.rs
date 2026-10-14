//! 按标题、菜单路径、搜索词和按键过滤条目。

use std::ops::Range;

use crate::keys::{Modifiers, Os};
use crate::model::Entry;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Match {
    /// 在输入切片中的下标。
    pub index: usize,
    /// 3 = 标题以查询开头，2 = 标题包含查询，1 = 其他字段匹配，0 = 查询为空。
    pub score: u8,
    /// 标题中要高亮的字节范围。
    pub highlight: Option<Range<usize>>,
}

pub fn filter(entries: &[Entry], query: &str, os: Os) -> Vec<Match> {
    let query = query.trim();
    if query.is_empty() {
        return (0..entries.len())
            .map(|index| Match {
                index,
                score: 0,
                highlight: None,
            })
            .collect();
    }
    let lowered = query.to_lowercase();
    let tokens: Vec<String> = lowered.split_whitespace().map(str::to_string).collect();
    let mut matches: Vec<Match> = entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| {
            let haystack = haystack(entry, os);
            tokens
                .iter()
                .all(|token| token_matches(entry, &haystack, token))
        })
        .map(|(index, entry)| {
            let title = entry.title.to_lowercase();
            let score = if title.starts_with(&lowered) {
                3
            } else if title.contains(&lowered) {
                2
            } else {
                1
            };
            Match {
                index,
                score,
                highlight: find_ignore_case(&entry.title, query),
            }
        })
        .collect();
    matches.sort_by_key(|m| std::cmp::Reverse(m.score));
    matches
}

fn haystack(entry: &Entry, os: Os) -> String {
    let mut parts: Vec<&str> = vec![&entry.group];
    parts.extend(entry.path.iter().map(String::as_str));
    parts.push(&entry.title);
    parts.extend(entry.search_terms.iter().map(String::as_str));
    let mut text = parts.join(" ").to_lowercase();
    text.push(' ');
    text.push_str(&entry.shortcut.search_text(os));
    text
}

/// 只由修饰键符号组成的词，返回对应的修饰键集合。
fn modifier_token(token: &str) -> Option<Modifiers> {
    let mut modifiers = Modifiers::NONE;
    for c in token.chars() {
        modifiers = modifiers
            | match c {
                '⌘' => Modifiers::META,
                '⇧' => Modifiers::SHIFT,
                '⌥' => Modifiers::ALT,
                '⌃' => Modifiers::CTRL,
                _ => return None,
            };
    }
    Some(modifiers)
}

fn token_matches(entry: &Entry, haystack: &str, token: &str) -> bool {
    match modifier_token(token) {
        Some(wanted) => entry
            .shortcut
            .chords()
            .iter()
            .any(|c| c.modifiers.contains(wanted)),
        None => haystack.contains(token),
    }
}

/// 在 `text` 中不区分大小写地查找 `query`，返回落在字符边界上的字节范围。
fn find_ignore_case(text: &str, query: &str) -> Option<Range<usize>> {
    let needle: Vec<char> = query.chars().flat_map(char::to_lowercase).collect();
    if needle.is_empty() {
        return None;
    }
    for (start, _) in text.char_indices() {
        let mut lowered = text[start..]
            .char_indices()
            .flat_map(|(offset, c)| c.to_lowercase().map(move |l| (offset + c.len_utf8(), l)));
        let mut end = start;
        let mut matched = 0;
        for &want in &needle {
            match lowered.next() {
                Some((char_end, got)) if got == want => {
                    end = start + char_end;
                    matched += 1;
                }
                _ => break,
            }
        }
        if matched == needle.len() && text.is_char_boundary(end) {
            return Some(start..end);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::{Os, Shortcut};
    use crate::model::{EntryId, EntrySource};

    fn entry(index: u32, group: &str, title: &str, keys: &str) -> Entry {
        Entry {
            id: EntryId {
                source: EntrySource::Menu,
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

    fn sample() -> Vec<Entry> {
        let mut save = entry(0, "文件", "保存", "Mod+S");
        save.search_terms = vec!["Save".into()];
        vec![
            save,
            entry(1, "文件", "另存为…", "Mod+Shift+S"),
            entry(2, "查看", "命令面板…", "Mod+Shift+P"),
            entry(3, "编辑", "自动保存设置", "Mod+Alt+S"),
        ]
    }

    fn indexes(matches: &[Match]) -> Vec<usize> {
        matches.iter().map(|m| m.index).collect()
    }

    #[test]
    fn empty_query_returns_everything_in_order() {
        assert_eq!(indexes(&filter(&sample(), "  ", Os::MacOS)), [0, 1, 2, 3]);
    }

    #[test]
    fn title_prefix_ranks_above_contains() {
        let matches = filter(&sample(), "保存", Os::MacOS);
        assert_eq!(indexes(&matches), [0, 3]);
        assert_eq!(matches[0].score, 3);
        assert_eq!(matches[1].score, 2);
    }

    #[test]
    fn search_terms_match_case_insensitively() {
        assert_eq!(indexes(&filter(&sample(), "SAVE", Os::MacOS)), [0]);
    }

    #[test]
    fn modifier_symbols_match_in_any_order() {
        assert_eq!(indexes(&filter(&sample(), "⌘⇧", Os::MacOS)), [1, 2]);
        assert_eq!(indexes(&filter(&sample(), "⇧⌘", Os::MacOS)), [1, 2]);
    }

    #[test]
    fn modifier_words_and_multiple_tokens() {
        assert_eq!(indexes(&filter(&sample(), "shift", Os::MacOS)), [1, 2]);
        assert_eq!(indexes(&filter(&sample(), "文件 shift", Os::MacOS)), [1]);
        assert!(filter(&sample(), "文件 不存在", Os::MacOS).is_empty());
    }

    #[test]
    fn highlight_covers_matched_title_text() {
        let entries = sample();
        let m = &filter(&entries, "面板", Os::MacOS)[0];
        let range = m.highlight.clone().unwrap();
        assert_eq!(&entries[m.index].title[range], "面板");
    }

    #[test]
    fn highlight_never_splits_characters() {
        let entries = vec![entry(0, "編集", "İstanbul を開く", "Mod+O")];
        for query in ["i̇stanbul", "開く", "o", "İ"] {
            for m in filter(&entries, query, Os::MacOS) {
                if let Some(range) = m.highlight {
                    assert!(
                        entries[0].title.get(range).is_some(),
                        "查询 {query:?} 的范围不在字符边界上"
                    );
                }
            }
        }
    }
}
