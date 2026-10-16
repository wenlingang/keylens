//! Filters entries by title, menu path, search terms and keys.

use std::ops::Range;

use crate::keys::{Chord, Modifiers, Os, parse_modifier};
use crate::model::Entry;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Match {
    /// Index in the input slice.
    pub index: usize,
    /// 3 = title starts with the query, 2 = title contains the query, 1 = other fields match, 0 = query is empty.
    pub score: u8,
    /// Byte range of the title to highlight.
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
                .all(|token| token_matches(entry, &haystack, token, os))
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

/// The keys a query asks for (`⌘⇧`, `shift`, `cmd+shift+p`…), so the panel can light up the
/// keycaps that made an entry match. Words that are not keys are ignored.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyQuery {
    modifiers: Modifiers,
    chords: Vec<Chord>,
}

impl KeyQuery {
    pub fn parse(query: &str, os: Os) -> Self {
        let mut parsed = KeyQuery {
            modifiers: Modifiers::NONE,
            chords: Vec::new(),
        };
        for token in query.trim().to_lowercase().split_whitespace() {
            if let Some(m) = modifier_token(token, os).or_else(|| parse_modifier(token, os)) {
                parsed.modifiers = parsed.modifiers | m;
            } else if let Some(chord) = chord_token(token, os) {
                parsed.chords.push(chord);
            }
        }
        parsed
    }

    pub fn is_empty(&self) -> bool {
        self.modifiers.is_empty() && self.chords.is_empty()
    }

    /// For each cap of `chord.labeled_caps(os)`, whether the query asked for it: every cap of a
    /// chord written out in full, and the caps of modifiers named on their own.
    pub fn lit_caps(&self, chord: &Chord, os: Os) -> Vec<bool> {
        let whole = self.chords.contains(chord);
        chord
            .labeled_caps(os)
            .iter()
            .map(|(m, _)| whole || m.is_some_and(|m| self.modifiers.contains(m)))
            .collect()
    }
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

/// A word made up only of modifiers, as symbols like `⌘⇧` or as `+`-joined names like `cmd+shift`
/// (a trailing `+` is allowed while the user is still typing); returns the matching modifier set.
fn modifier_token(token: &str, os: Os) -> Option<Modifiers> {
    if token.contains('+') {
        let mut modifiers = Modifiers::NONE;
        for word in token.split('+').filter(|w| !w.is_empty()) {
            modifiers = modifiers | parse_modifier(word, os)?;
        }
        return (!modifiers.is_empty()).then_some(modifiers);
    }
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

/// A fully written key combination: a symbol prefix plus main key like `⌘⇧P`, or `+` notation like `cmd+shift+p`.
fn chord_token(token: &str, os: Os) -> Option<Chord> {
    if token.contains('+') {
        return Chord::parse(token, os).ok();
    }
    let key_start = token.find(|c| !"⌘⇧⌥⌃".contains(c))?;
    let modifiers = modifier_token(&token[..key_start], os).filter(|m| !m.is_empty())?;
    let key = Chord::parse(&token[key_start..], os).ok()?;
    key.modifiers.is_empty().then_some(Chord {
        modifiers,
        key: key.key,
    })
}

fn token_matches(entry: &Entry, haystack: &str, token: &str, os: Os) -> bool {
    let chords = entry.shortcut.chords();
    if let Some(wanted) = modifier_token(token, os) {
        return chords.iter().any(|c| c.modifiers.contains(wanted));
    }
    if let Some(wanted) = chord_token(token, os) {
        return chords.contains(&wanted);
    }
    haystack.contains(token)
}

/// Finds `query` in `text` case-insensitively, returning a byte range that falls on character boundaries.
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
    fn modifier_words_joined_with_plus_match_like_symbols() {
        // Typing `cmd+shift` (or `cmd+shift+` on the way to a full chord) lists what ⌘⇧ lists.
        for query in ["cmd+shift", "Shift+Cmd", "mod+shift", "cmd+shift+"] {
            assert_eq!(
                indexes(&filter(&sample(), query, Os::MacOS)),
                [1, 2],
                "{query}"
            );
        }
        assert!(filter(&sample(), "ctrl+shift", Os::MacOS).is_empty());
    }

    #[test]
    fn full_shortcut_matches_regardless_of_modifier_order_or_spelling() {
        for query in [
            "⌘⇧P",
            "⇧⌘P",
            "⇧⌘p",
            "cmd+shift+p",
            "Shift+Cmd+P",
            "mod+shift+p",
        ] {
            assert_eq!(
                indexes(&filter(&sample(), query, Os::MacOS)),
                [2],
                "{query}"
            );
        }
        // Modifiers must match exactly: ⌘P should not match ⇧⌘P
        assert!(filter(&sample(), "⌘P", Os::MacOS).is_empty());
        assert!(filter(&sample(), "ctrl+shift+p", Os::MacOS).is_empty());
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
                        "range for query {query:?} is not on a char boundary"
                    );
                }
            }
        }
    }

    fn chord(text: &str) -> Chord {
        Chord::parse(text, Os::MacOS).unwrap()
    }

    #[test]
    fn key_query_lights_named_modifiers_only() {
        // ⇧⌘P shows as ⇧ ⌘ P.
        let q = KeyQuery::parse("⌘⇧", Os::MacOS);
        assert_eq!(
            q.lit_caps(&chord("Cmd+Shift+P"), Os::MacOS),
            [true, true, false]
        );
        let q = KeyQuery::parse("save shift", Os::MacOS);
        assert_eq!(
            q.lit_caps(&chord("Cmd+Shift+P"), Os::MacOS),
            [true, false, false]
        );
    }

    #[test]
    fn key_query_lights_every_cap_of_a_full_chord() {
        let q = KeyQuery::parse("cmd+shift+p", Os::MacOS);
        assert_eq!(
            q.lit_caps(&chord("Cmd+Shift+P"), Os::MacOS),
            [true, true, true]
        );
        assert_eq!(q.lit_caps(&chord("Cmd+P"), Os::MacOS), [false, false]);
    }

    #[test]
    fn key_query_ignores_plain_words() {
        assert!(KeyQuery::parse("save as", Os::MacOS).is_empty());
        assert!(!KeyQuery::parse("⌘", Os::MacOS).is_empty());
    }
}
