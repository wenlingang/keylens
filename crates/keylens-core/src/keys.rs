//! Shortcut representation, parsing and display.

use std::ops::BitOr;

/// Set of modifier keys. `META` is ⌘.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Modifiers(u8);

impl Modifiers {
    pub const NONE: Self = Self(0);
    pub const CTRL: Self = Self(1 << 0);
    pub const ALT: Self = Self(1 << 1);
    pub const SHIFT: Self = Self(1 << 2);
    pub const META: Self = Self(1 << 3);
    pub const FN: Self = Self(1 << 4);

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }

    /// The modifiers a trigger can use, in the order the settings window lists them.
    pub const TRIGGER_KEYS: [Modifiers; 4] = [
        Modifiers::META,
        Modifiers::ALT,
        Modifiers::CTRL,
        Modifiers::SHIFT,
    ];

    /// Parses the single modifier named in `config.toml` (`key = "cmd"`), accepting the same
    /// aliases as shortcut notation. `mod` and `fn` are rejected: `mod` is data-file notation,
    /// and `fn` cannot be detected as a trigger.
    pub fn from_trigger_name(name: &str) -> Option<Modifiers> {
        match name.to_ascii_lowercase().as_str() {
            "mod" | "fn" => None,
            other => parse_modifier(other),
        }
    }

    /// The name written back to `config.toml`, in macOS terms.
    pub fn trigger_name(self) -> &'static str {
        match self {
            Modifiers::ALT => "option",
            Modifiers::CTRL => "ctrl",
            Modifiers::SHIFT => "shift",
            _ => "cmd",
        }
    }
}

impl BitOr for Modifiers {
    type Output = Self;

    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

/// The main key. Letters are always stored uppercase.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    Char(char),
    F(u8),
    Up,
    Down,
    Left,
    Right,
    Enter,
    Tab,
    Space,
    Backspace,
    Delete,
    Escape,
    Home,
    End,
    PageUp,
    PageDown,
}

const PUNCTUATION: &str = "`-=[]\\;',./";

/// Symbols that need Shift on a US keyboard, with their base keys.
const SHIFTED_SYMBOLS: [(char, char); 21] = [
    ('~', '`'),
    ('!', '1'),
    ('@', '2'),
    ('#', '3'),
    ('$', '4'),
    ('%', '5'),
    ('^', '6'),
    ('&', '7'),
    ('*', '8'),
    ('(', '9'),
    (')', '0'),
    ('_', '-'),
    ('+', '='),
    ('{', '['),
    ('}', ']'),
    ('|', '\\'),
    (':', ';'),
    ('"', '\''),
    ('<', ','),
    ('>', '.'),
    ('?', '/'),
];

impl Key {
    /// Used when the platform layer converts a menu character into a `Key`: letters are uppercased, other characters kept as-is.
    pub fn from_char(c: char) -> Key {
        Key::Char(c.to_ascii_uppercase())
    }

    fn parse(token: &str) -> Option<Key> {
        let lower = token.to_ascii_lowercase();
        let named = match lower.as_str() {
            "up" => Some(Key::Up),
            "down" => Some(Key::Down),
            "left" => Some(Key::Left),
            "right" => Some(Key::Right),
            "enter" | "return" => Some(Key::Enter),
            "tab" => Some(Key::Tab),
            "space" => Some(Key::Space),
            "backspace" => Some(Key::Backspace),
            "delete" | "del" => Some(Key::Delete),
            "escape" | "esc" => Some(Key::Escape),
            "home" => Some(Key::Home),
            "end" => Some(Key::End),
            "pageup" => Some(Key::PageUp),
            "pagedown" => Some(Key::PageDown),
            _ => None,
        };
        if named.is_some() {
            return named;
        }
        if let Some(n) = lower.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()) {
            return (1..=24).contains(&n).then_some(Key::F(n));
        }
        let mut chars = token.chars();
        match (chars.next(), chars.next()) {
            (Some(c), None) if c.is_ascii_alphanumeric() => Some(Key::Char(c.to_ascii_uppercase())),
            (Some(c), None) if PUNCTUATION.contains(c) => Some(Key::Char(c)),
            _ => None,
        }
    }

    fn label(self) -> String {
        let text = match self {
            Key::Char(c) => return c.to_string(),
            Key::F(n) => return format!("F{n}"),
            Key::Up => "↑",
            Key::Down => "↓",
            Key::Left => "←",
            Key::Right => "→",
            Key::Enter => "↩",
            Key::Tab => "⇥",
            Key::Space => "Space",
            Key::Backspace => "⌫",
            Key::Delete => "⌦",
            Key::Escape => "⎋",
            Key::Home => "↖",
            Key::End => "↘",
            Key::PageUp => "⇞",
            Key::PageDown => "⇟",
        };
        text.to_string()
    }

    /// Key names (lowercase) a user might type, used for search.
    fn search_words(self) -> &'static str {
        match self {
            Key::Char(_) | Key::F(_) => "",
            Key::Up => "up arrow",
            Key::Down => "down arrow",
            Key::Left => "left arrow",
            Key::Right => "right arrow",
            Key::Enter => "enter return",
            Key::Tab => "tab",
            Key::Space => "space",
            Key::Backspace => "backspace",
            Key::Delete => "delete",
            Key::Escape => "escape esc",
            Key::Home => "home",
            Key::End => "end",
            Key::PageUp => "pageup page up",
            Key::PageDown => "pagedown page down",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KeyParseError {
    #[error("empty key combination")]
    Empty,
    #[error("unknown key or modifier `{0}`")]
    Unknown(String),
    #[error("`{0}` has no main key")]
    NoKey(String),
    #[error("`{0}` has more than one main key")]
    MultipleKeys(String),
    #[error("modifier `{1}` is repeated in `{0}`")]
    DuplicateModifier(String, String),
}

/// Parses a modifier name. `Mod` is ⌘; `super`/`win` are kept as aliases of ⌘ so older
/// data and config files keep parsing.
pub(crate) fn parse_modifier(token: &str) -> Option<Modifiers> {
    match token.to_ascii_lowercase().as_str() {
        "mod" | "cmd" | "command" | "super" | "win" | "meta" => Some(Modifiers::META),
        "ctrl" | "control" => Some(Modifiers::CTRL),
        "alt" | "option" | "opt" => Some(Modifiers::ALT),
        "shift" => Some(Modifiers::SHIFT),
        "fn" => Some(Modifiers::FN),
        _ => None,
    }
}

/// A single key combination, e.g. ⇧⌘P.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Chord {
    pub modifiers: Modifiers,
    pub key: Key,
}

impl Chord {
    /// Parses notation like `Mod+Shift+P`, case-insensitively.
    pub fn parse(text: &str) -> Result<Chord, KeyParseError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(KeyParseError::Empty);
        }
        let mut modifiers = Modifiers::NONE;
        let mut key = None;
        for token in text.split('+').map(str::trim) {
            if token.is_empty() {
                return Err(KeyParseError::Unknown(text.to_string()));
            }
            if let Some(m) = parse_modifier(token) {
                if modifiers.contains(m) {
                    return Err(KeyParseError::DuplicateModifier(text.into(), token.into()));
                }
                modifiers = modifiers | m;
            } else if let Some(k) = Key::parse(token) {
                if key.is_some() {
                    return Err(KeyParseError::MultipleKeys(text.to_string()));
                }
                key = Some(k);
            } else {
                return Err(KeyParseError::Unknown(token.to_string()));
            }
        }
        let key = key.ok_or_else(|| KeyParseError::NoKey(text.to_string()))?;
        Ok(Chord { modifiers, key })
    }

    /// Splits into keycaps in display order, e.g. ["⇧", "⌘", "P"].
    pub fn caps(&self) -> Vec<String> {
        self.labeled_caps()
            .into_iter()
            .map(|(_, label)| label)
            .collect()
    }

    /// Like [`Chord::caps`], with the modifier each cap stands for (`None` for the main key).
    pub fn labeled_caps(&self) -> Vec<(Option<Modifiers>, String)> {
        const NAMES: [(Modifiers, &str); 5] = [
            (Modifiers::FN, "fn"),
            (Modifiers::CTRL, "⌃"),
            (Modifiers::ALT, "⌥"),
            (Modifiers::SHIFT, "⇧"),
            (Modifiers::META, "⌘"),
        ];
        let mut caps: Vec<(Option<Modifiers>, String)> = NAMES
            .iter()
            .filter(|(m, _)| self.modifiers.contains(*m))
            .map(|(m, name)| (Some(*m), name.to_string()))
            .collect();
        caps.push((None, self.key.label()));
        caps
    }

    /// Canonical form for comparison: symbols that need Shift are split into "⇧ + base key" (per US keyboard),
    /// so ⌘+ in a menu and ⇧⌘= in the data count as the same shortcut. Display still uses the original notation.
    pub fn canonical(self) -> Chord {
        if let Key::Char(c) = self.key
            && let Some(&(_, base)) = SHIFTED_SYMBOLS.iter().find(|(shifted, _)| *shifted == c)
        {
            return Chord {
                modifiers: self.modifiers | Modifiers::SHIFT,
                key: Key::Char(base),
            };
        }
        self
    }

    pub fn display(&self) -> String {
        self.caps().concat()
    }
}

/// A shortcut: one or more combinations pressed in sequence, e.g. ⌘K ⌘S.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Shortcut(Vec<Chord>);

impl Shortcut {
    pub fn single(chord: Chord) -> Self {
        Self(vec![chord])
    }

    pub fn parse_seq<S: AsRef<str>>(parts: &[S]) -> Result<Shortcut, KeyParseError> {
        if parts.is_empty() {
            return Err(KeyParseError::Empty);
        }
        parts
            .iter()
            .map(|p| Chord::parse(p.as_ref()))
            .collect::<Result<Vec<_>, _>>()
            .map(Shortcut)
    }

    /// Takes `Chord::canonical` of each combination, used for merging and deduplication.
    pub fn canonical(&self) -> Shortcut {
        Shortcut(self.0.iter().map(|c| c.canonical()).collect())
    }

    pub fn chords(&self) -> &[Chord] {
        &self.0
    }

    pub fn display(&self) -> String {
        self.0
            .iter()
            .map(|c| c.display())
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Lowercase text for search: the display form plus English names of modifiers and special keys.
    pub fn search_text(&self) -> String {
        let mut out = self.display().to_lowercase();
        for chord in &self.0 {
            for (m, words) in [
                (Modifiers::META, "cmd command meta super win"),
                (Modifiers::CTRL, "ctrl control"),
                (Modifiers::ALT, "alt option opt"),
                (Modifiers::SHIFT, "shift"),
                (Modifiers::FN, "fn"),
            ] {
                if chord.modifiers.contains(m) {
                    out.push(' ');
                    out.push_str(words);
                }
            }
            let words = chord.key.search_words();
            if !words.is_empty() {
                out.push(' ');
                out.push_str(words);
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chord(s: &str) -> Chord {
        Chord::parse(s).unwrap()
    }

    #[test]
    fn mod_is_command() {
        let c = chord("Mod+Shift+P");
        assert_eq!(c.modifiers, Modifiers::META | Modifiers::SHIFT);
        assert_eq!(c.key, Key::Char('P'));
        assert_eq!(chord("Super+P"), chord("Cmd+P"));
        assert_eq!(chord("Win+P"), chord("Mod+P"));
    }

    #[test]
    fn display_uses_mac_symbols() {
        assert_eq!(chord("Mod+Shift+P").display(), "⇧⌘P");
        assert_eq!(chord("Ctrl+Alt+Shift+Cmd+K").display(), "⌃⌥⇧⌘K");
        assert_eq!(chord("Option+Up").display(), "⌥↑");
        assert_eq!(chord("Super+Left").display(), "⌘←");
        assert_eq!(chord("Ctrl+`").display(), "⌃`");
    }

    #[test]
    fn caps_split_modifiers_and_key() {
        assert_eq!(chord("Mod+Shift+P").caps(), ["⇧", "⌘", "P"]);
        assert_eq!(chord("Mod+Enter").caps(), ["⌘", "↩"]);
    }

    #[test]
    fn parsing_is_case_insensitive_and_accepts_aliases() {
        assert_eq!(chord("cmd+shift+p"), chord("Command+Shift+P"));
        assert_eq!(chord("Opt+Z"), chord("Alt+Z"));
        assert_eq!(chord("Control+Z"), chord("Ctrl+Z"));
        assert_eq!(chord("Esc").key, Key::Escape);
        assert_eq!(chord("Return").key, Key::Enter);
    }

    #[test]
    fn function_keys_and_punctuation() {
        assert_eq!(chord("F12").key, Key::F(12));
        assert_eq!(chord("Mod+/").key, Key::Char('/'));
        assert_eq!(chord("F").key, Key::Char('F'));
    }

    #[test]
    fn from_char_uppercases_letters_and_keeps_symbols() {
        assert_eq!(Key::from_char('s'), Key::Char('S'));
        assert_eq!(Key::from_char('+'), Key::Char('+'));
        let plus = Chord {
            modifiers: Modifiers::META,
            key: Key::from_char('+'),
        };
        assert_eq!(plus.display(), "⌘+");
    }

    #[test]
    fn invalid_combinations_are_rejected() {
        use KeyParseError::*;
        assert_eq!(Chord::parse(""), Err(Empty));
        assert_eq!(Chord::parse("Mod+Shft+P"), Err(Unknown("Shft".into())));
        assert_eq!(Chord::parse("Mod+Shift"), Err(NoKey("Mod+Shift".into())));
        assert_eq!(Chord::parse("A+B"), Err(MultipleKeys("A+B".into())));
        assert_eq!(
            Chord::parse("Cmd+Cmd+A"),
            Err(DuplicateModifier("Cmd+Cmd+A".into(), "Cmd".into()))
        );
        assert_eq!(Chord::parse("Mod++"), Err(Unknown("Mod++".into())));
        assert_eq!(Chord::parse("F25"), Err(Unknown("F25".into())));
    }

    #[test]
    fn sequences_display_with_spaces() {
        let s = Shortcut::parse_seq(&["Mod+K", "Mod+S"]).unwrap();
        assert_eq!(s.display(), "⌘K ⌘S");
        assert_eq!(s.chords().len(), 2);
        assert_eq!(Shortcut::parse_seq::<&str>(&[]), Err(KeyParseError::Empty));
    }

    #[test]
    fn search_text_contains_symbols_and_words() {
        let s = Shortcut::parse_seq(&["Mod+Shift+Up"]).unwrap();
        let text = s.search_text();
        for word in ["⇧⌘↑", "shift", "command", "cmd", "up"] {
            assert!(text.contains(word), "{text:?} is missing {word:?}");
        }
    }

    #[test]
    fn trigger_names_round_trip_and_accept_aliases() {
        for key in Modifiers::TRIGGER_KEYS {
            assert_eq!(Modifiers::from_trigger_name(key.trigger_name()), Some(key));
        }
        assert_eq!(Modifiers::META.trigger_name(), "cmd");
        assert_eq!(Modifiers::ALT.trigger_name(), "option");
        assert_eq!(
            Modifiers::from_trigger_name("Command"),
            Some(Modifiers::META)
        );
        assert_eq!(Modifiers::from_trigger_name("win"), Some(Modifiers::META));
        assert_eq!(Modifiers::from_trigger_name("super"), Some(Modifiers::META));
        assert_eq!(
            Modifiers::from_trigger_name("control"),
            Some(Modifiers::CTRL)
        );
        assert_eq!(Modifiers::from_trigger_name("alt"), Some(Modifiers::ALT));
        assert_eq!(Modifiers::from_trigger_name("opt"), Some(Modifiers::ALT));
        assert_eq!(Modifiers::from_trigger_name("mod"), None);
        assert_eq!(Modifiers::from_trigger_name("fn"), None);
        assert_eq!(Modifiers::from_trigger_name("cmd+shift"), None);
        assert_eq!(Modifiers::from_trigger_name(""), None);
    }
}
