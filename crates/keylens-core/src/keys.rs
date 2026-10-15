//! Shortcut representation, parsing and display.

use std::ops::BitOr;

/// The running platform; determines what `Mod` means and the display style.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Os {
    MacOS,
    Windows,
    Linux,
}

impl Os {
    pub const ALL: [Os; 3] = [Os::MacOS, Os::Windows, Os::Linux];

    pub fn current() -> Os {
        if cfg!(target_os = "macos") {
            Os::MacOS
        } else if cfg!(target_os = "windows") {
            Os::Windows
        } else {
            Os::Linux
        }
    }
}

/// Set of modifier keys. `META` is ⌘ on macOS, the Win key on Windows, and the Super key on Linux.
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

    /// The modifier that `Mod` maps to in data files: ⌘ on macOS, Ctrl on other platforms.
    pub const fn primary(os: Os) -> Self {
        match os {
            Os::MacOS => Self::META,
            Os::Windows | Os::Linux => Self::CTRL,
        }
    }

    /// The modifiers a trigger can use, in the order the settings window lists them.
    pub const TRIGGER_KEYS: [Modifiers; 4] = [
        Modifiers::META,
        Modifiers::ALT,
        Modifiers::CTRL,
        Modifiers::SHIFT,
    ];

    /// Parses the single modifier named in `config.toml` (`key = "cmd"`), accepting the same
    /// aliases as shortcut notation. `mod` and `fn` are rejected: one depends on the OS, the
    /// other cannot be detected as a trigger.
    pub fn from_trigger_name(name: &str) -> Option<Modifiers> {
        match name.to_ascii_lowercase().as_str() {
            "mod" | "fn" => None,
            other => parse_modifier(other, Os::MacOS),
        }
    }

    /// The name written back to `config.toml`, in the platform's own words.
    pub fn trigger_name(self, os: Os) -> &'static str {
        match (self, os) {
            (Modifiers::META, Os::MacOS) => "cmd",
            (Modifiers::META, Os::Windows) => "win",
            (Modifiers::META, Os::Linux) => "super",
            (Modifiers::ALT, Os::MacOS) => "option",
            (Modifiers::ALT, _) => "alt",
            (Modifiers::CTRL, _) => "ctrl",
            (Modifiers::SHIFT, _) => "shift",
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

    fn label(self, os: Os) -> String {
        let mac = os == Os::MacOS;
        let text = match self {
            Key::Char(c) => return c.to_string(),
            Key::F(n) => return format!("F{n}"),
            Key::Up => {
                if mac {
                    "↑"
                } else {
                    "Up"
                }
            }
            Key::Down => {
                if mac {
                    "↓"
                } else {
                    "Down"
                }
            }
            Key::Left => {
                if mac {
                    "←"
                } else {
                    "Left"
                }
            }
            Key::Right => {
                if mac {
                    "→"
                } else {
                    "Right"
                }
            }
            Key::Enter => {
                if mac {
                    "↩"
                } else {
                    "Enter"
                }
            }
            Key::Tab => {
                if mac {
                    "⇥"
                } else {
                    "Tab"
                }
            }
            Key::Space => "Space",
            Key::Backspace => {
                if mac {
                    "⌫"
                } else {
                    "Backspace"
                }
            }
            Key::Delete => {
                if mac {
                    "⌦"
                } else {
                    "Delete"
                }
            }
            Key::Escape => {
                if mac {
                    "⎋"
                } else {
                    "Esc"
                }
            }
            Key::Home => {
                if mac {
                    "↖"
                } else {
                    "Home"
                }
            }
            Key::End => {
                if mac {
                    "↘"
                } else {
                    "End"
                }
            }
            Key::PageUp => {
                if mac {
                    "⇞"
                } else {
                    "PgUp"
                }
            }
            Key::PageDown => {
                if mac {
                    "⇟"
                } else {
                    "PgDn"
                }
            }
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

pub(crate) fn parse_modifier(token: &str, os: Os) -> Option<Modifiers> {
    match token.to_ascii_lowercase().as_str() {
        "mod" => Some(Modifiers::primary(os)),
        "cmd" | "command" | "super" | "win" | "meta" => Some(Modifiers::META),
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
    pub fn parse(text: &str, os: Os) -> Result<Chord, KeyParseError> {
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
            if let Some(m) = parse_modifier(token, os) {
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

    /// Splits into keycaps in display order, e.g. ["⇧", "⌘", "P"] on macOS, ["Ctrl", "Shift", "P"] on Windows.
    pub fn caps(&self, os: Os) -> Vec<String> {
        let names: [(Modifiers, &str); 5] = match os {
            Os::MacOS => [
                (Modifiers::FN, "fn"),
                (Modifiers::CTRL, "⌃"),
                (Modifiers::ALT, "⌥"),
                (Modifiers::SHIFT, "⇧"),
                (Modifiers::META, "⌘"),
            ],
            Os::Windows => [
                (Modifiers::CTRL, "Ctrl"),
                (Modifiers::ALT, "Alt"),
                (Modifiers::SHIFT, "Shift"),
                (Modifiers::META, "Win"),
                (Modifiers::FN, "Fn"),
            ],
            Os::Linux => [
                (Modifiers::CTRL, "Ctrl"),
                (Modifiers::ALT, "Alt"),
                (Modifiers::SHIFT, "Shift"),
                (Modifiers::META, "Super"),
                (Modifiers::FN, "Fn"),
            ],
        };
        let mut caps: Vec<String> = names
            .iter()
            .filter(|(m, _)| self.modifiers.contains(*m))
            .map(|(_, name)| name.to_string())
            .collect();
        caps.push(self.key.label(os));
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

    pub fn display(&self, os: Os) -> String {
        let caps = self.caps(os);
        if os == Os::MacOS {
            caps.concat()
        } else {
            caps.join("+")
        }
    }
}

/// A shortcut: one or more combinations pressed in sequence, e.g. ⌘K ⌘S.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Shortcut(Vec<Chord>);

impl Shortcut {
    pub fn single(chord: Chord) -> Self {
        Self(vec![chord])
    }

    pub fn parse_seq<S: AsRef<str>>(parts: &[S], os: Os) -> Result<Shortcut, KeyParseError> {
        if parts.is_empty() {
            return Err(KeyParseError::Empty);
        }
        parts
            .iter()
            .map(|p| Chord::parse(p.as_ref(), os))
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

    pub fn display(&self, os: Os) -> String {
        self.0
            .iter()
            .map(|c| c.display(os))
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Lowercase text for search: the display form plus English names of modifiers and special keys.
    pub fn search_text(&self, os: Os) -> String {
        let mut out = self.display(os).to_lowercase();
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

    fn chord(s: &str, os: Os) -> Chord {
        Chord::parse(s, os).unwrap()
    }

    #[test]
    fn mod_is_command_on_macos_and_ctrl_elsewhere() {
        let mac = chord("Mod+Shift+P", Os::MacOS);
        assert_eq!(mac.modifiers, Modifiers::META | Modifiers::SHIFT);
        assert_eq!(mac.key, Key::Char('P'));
        let win = chord("Mod+Shift+P", Os::Windows);
        assert_eq!(win.modifiers, Modifiers::CTRL | Modifiers::SHIFT);
    }

    #[test]
    fn display_follows_platform_conventions() {
        assert_eq!(chord("Mod+Shift+P", Os::MacOS).display(Os::MacOS), "⇧⌘P");
        assert_eq!(
            chord("Mod+Shift+P", Os::Windows).display(Os::Windows),
            "Ctrl+Shift+P"
        );
        assert_eq!(
            chord("Ctrl+Alt+Shift+Cmd+K", Os::MacOS).display(Os::MacOS),
            "⌃⌥⇧⌘K"
        );
        assert_eq!(chord("Option+Up", Os::MacOS).display(Os::MacOS), "⌥↑");
        assert_eq!(
            chord("Super+Left", Os::Linux).display(Os::Linux),
            "Super+Left"
        );
        assert_eq!(chord("Win+E", Os::Windows).display(Os::Windows), "Win+E");
    }

    #[test]
    fn caps_split_modifiers_and_key() {
        assert_eq!(
            chord("Mod+Shift+P", Os::MacOS).caps(Os::MacOS),
            ["⇧", "⌘", "P"]
        );
        assert_eq!(
            chord("Mod+Enter", Os::Windows).caps(Os::Windows),
            ["Ctrl", "Enter"]
        );
    }

    #[test]
    fn parsing_is_case_insensitive_and_accepts_aliases() {
        assert_eq!(
            chord("cmd+shift+p", Os::MacOS),
            chord("Command+Shift+P", Os::MacOS)
        );
        assert_eq!(chord("Opt+Z", Os::MacOS), chord("Alt+Z", Os::MacOS));
        assert_eq!(chord("Esc", Os::MacOS).key, Key::Escape);
        assert_eq!(chord("Return", Os::MacOS).key, Key::Enter);
    }

    #[test]
    fn function_keys_and_punctuation() {
        assert_eq!(chord("F12", Os::MacOS).key, Key::F(12));
        assert_eq!(chord("Mod+/", Os::MacOS).key, Key::Char('/'));
        assert_eq!(chord("Ctrl+`", Os::Windows).display(Os::Windows), "Ctrl+`");
        assert_eq!(chord("F", Os::MacOS).key, Key::Char('F'));
    }

    #[test]
    fn from_char_uppercases_letters_and_keeps_symbols() {
        assert_eq!(Key::from_char('s'), Key::Char('S'));
        assert_eq!(Key::from_char('+'), Key::Char('+'));
        let plus = Chord {
            modifiers: Modifiers::META,
            key: Key::from_char('+'),
        };
        assert_eq!(plus.display(Os::MacOS), "⌘+");
    }

    #[test]
    fn invalid_combinations_are_rejected() {
        use KeyParseError::*;
        assert_eq!(Chord::parse("", Os::MacOS), Err(Empty));
        assert_eq!(
            Chord::parse("Mod+Shft+P", Os::MacOS),
            Err(Unknown("Shft".into()))
        );
        assert_eq!(
            Chord::parse("Mod+Shift", Os::MacOS),
            Err(NoKey("Mod+Shift".into()))
        );
        assert_eq!(
            Chord::parse("A+B", Os::MacOS),
            Err(MultipleKeys("A+B".into()))
        );
        assert_eq!(
            Chord::parse("Cmd+Cmd+A", Os::MacOS),
            Err(DuplicateModifier("Cmd+Cmd+A".into(), "Cmd".into()))
        );
        assert_eq!(
            Chord::parse("Mod++", Os::MacOS),
            Err(Unknown("Mod++".into()))
        );
        assert_eq!(Chord::parse("F25", Os::MacOS), Err(Unknown("F25".into())));
    }

    #[test]
    fn sequences_display_with_spaces() {
        let s = Shortcut::parse_seq(&["Mod+K", "Mod+S"], Os::MacOS).unwrap();
        assert_eq!(s.display(Os::MacOS), "⌘K ⌘S");
        assert_eq!(s.chords().len(), 2);
        assert_eq!(
            Shortcut::parse_seq::<&str>(&[], Os::MacOS),
            Err(KeyParseError::Empty)
        );
    }

    #[test]
    fn search_text_contains_symbols_and_words() {
        let s = Shortcut::parse_seq(&["Mod+Shift+Up"], Os::MacOS).unwrap();
        let text = s.search_text(Os::MacOS);
        for word in ["⇧⌘↑", "shift", "command", "cmd", "up"] {
            assert!(text.contains(word), "{text:?} is missing {word:?}");
        }
    }

    #[test]
    fn trigger_names_round_trip_and_accept_aliases() {
        for os in Os::ALL {
            for key in Modifiers::TRIGGER_KEYS {
                assert_eq!(
                    Modifiers::from_trigger_name(key.trigger_name(os)),
                    Some(key)
                );
            }
        }
        assert_eq!(Modifiers::META.trigger_name(Os::MacOS), "cmd");
        assert_eq!(Modifiers::META.trigger_name(Os::Linux), "super");
        assert_eq!(Modifiers::ALT.trigger_name(Os::MacOS), "option");
        assert_eq!(Modifiers::ALT.trigger_name(Os::Linux), "alt");
        assert_eq!(
            Modifiers::from_trigger_name("Command"),
            Some(Modifiers::META)
        );
        assert_eq!(Modifiers::from_trigger_name("win"), Some(Modifiers::META));
        assert_eq!(
            Modifiers::from_trigger_name("control"),
            Some(Modifiers::CTRL)
        );
        assert_eq!(Modifiers::from_trigger_name("opt"), Some(Modifiers::ALT));
        // `mod` depends on the OS and `fn` is not readable as a trigger; neither is accepted.
        assert_eq!(Modifiers::from_trigger_name("mod"), None);
        assert_eq!(Modifiers::from_trigger_name("fn"), None);
        assert_eq!(Modifiers::from_trigger_name("cmd+shift"), None);
        assert_eq!(Modifiers::from_trigger_name(""), None);
    }
}
