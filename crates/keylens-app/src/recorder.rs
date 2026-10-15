//! Turns a key press in the shortcut editor into the notation data files use (`Mod+Shift+P`).

use std::time::Duration;

use keylens_core::{Modifiers, Os, Shortcut};

/// After the first chord, a second chord within this window makes a two-step sequence.
pub const SEQUENCE_WINDOW: Duration = Duration::from_secs(1);

/// The modifier state of one key press, mirroring gpui's `Modifiers`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pressed {
    pub control: bool,
    pub alt: bool,
    pub shift: bool,
    /// ⌘ on macOS, the Win key on Windows, Super on Linux.
    pub platform: bool,
    pub function: bool,
}

pub fn chord_text(pressed: Pressed, key: &str, os: Os) -> Option<String> {
    let key = key_name(key)?;
    let primary = Modifiers::primary(os);
    let mut parts: Vec<&str> = Vec::new();
    let is_primary = |m: Modifiers| m == primary;
    if (pressed.platform && is_primary(Modifiers::META))
        || (pressed.control && is_primary(Modifiers::CTRL))
    {
        parts.push("Mod");
    }
    if pressed.platform && !is_primary(Modifiers::META) {
        parts.push("Cmd");
    }
    if pressed.control && !is_primary(Modifiers::CTRL) {
        parts.push("Ctrl");
    }
    if pressed.alt {
        parts.push("Alt");
    }
    if pressed.shift {
        parts.push("Shift");
    }
    if pressed.function {
        parts.push("Fn");
    }
    let text = parts
        .into_iter()
        .chain(std::iter::once(key.as_str()))
        .collect::<Vec<_>>()
        .join("+");
    Shortcut::parse_seq(&[&text], os).ok().map(|_| text)
}

/// gpui's key name (lowercase: "p", "enter", "f5", ",") in keylens notation, or `None` for a
/// modifier on its own or an unknown key.
fn key_name(key: &str) -> Option<String> {
    const NAMED: [(&str, &str); 14] = [
        ("up", "Up"),
        ("down", "Down"),
        ("left", "Left"),
        ("right", "Right"),
        ("enter", "Enter"),
        ("tab", "Tab"),
        ("space", "Space"),
        ("backspace", "Backspace"),
        ("delete", "Delete"),
        ("escape", "Escape"),
        ("home", "Home"),
        ("end", "End"),
        ("pageup", "PageUp"),
        ("pagedown", "PageDown"),
    ];
    if let Some((_, name)) = NAMED.iter().find(|(gpui, _)| *gpui == key) {
        return Some((*name).to_string());
    }
    if let Some(n) = key.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()) {
        return (1..=24).contains(&n).then(|| format!("F{n}"));
    }
    let mut chars = key.chars();
    match (chars.next(), chars.next()) {
        (Some(c), None) if c.is_ascii_alphanumeric() || "`-=[]\\;',./".contains(c) => {
            Some(c.to_ascii_uppercase().to_string())
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd() -> Pressed {
        Pressed {
            platform: true,
            ..Default::default()
        }
    }

    #[test]
    fn primary_modifier_becomes_mod_per_platform() {
        let ctrl = Pressed {
            control: true,
            ..Default::default()
        };
        assert_eq!(chord_text(cmd(), "s", Os::MacOS).as_deref(), Some("Mod+S"));
        assert_eq!(chord_text(ctrl, "s", Os::MacOS).as_deref(), Some("Ctrl+S"));
        assert_eq!(chord_text(ctrl, "s", Os::Linux).as_deref(), Some("Mod+S"));
        assert_eq!(chord_text(cmd(), "s", Os::Linux).as_deref(), Some("Cmd+S"));
    }

    #[test]
    fn modifiers_are_written_in_a_fixed_order() {
        let all = Pressed {
            control: true,
            alt: true,
            shift: true,
            platform: true,
            function: false,
        };
        assert_eq!(
            chord_text(all, "p", Os::MacOS).as_deref(),
            Some("Mod+Ctrl+Alt+Shift+P")
        );
    }

    #[test]
    fn named_keys_use_keylens_names() {
        assert_eq!(
            chord_text(cmd(), "enter", Os::MacOS).as_deref(),
            Some("Mod+Enter")
        );
        assert_eq!(
            chord_text(cmd(), "up", Os::MacOS).as_deref(),
            Some("Mod+Up")
        );
        assert_eq!(
            chord_text(Pressed::default(), "f5", Os::MacOS).as_deref(),
            Some("F5")
        );
        assert_eq!(
            chord_text(cmd(), "pageup", Os::MacOS).as_deref(),
            Some("Mod+PageUp")
        );
        assert_eq!(
            chord_text(cmd(), "space", Os::MacOS).as_deref(),
            Some("Mod+Space")
        );
        assert_eq!(chord_text(cmd(), ",", Os::MacOS).as_deref(), Some("Mod+,"));
    }

    #[test]
    fn shifted_digits_keep_their_base_key() {
        let shift = Pressed {
            shift: true,
            ..Default::default()
        };
        let text = chord_text(
            Pressed {
                platform: true,
                ..shift
            },
            "1",
            Os::MacOS,
        )
        .unwrap();
        assert_eq!(text, "Mod+Shift+1");
        assert!(Shortcut::parse_seq(&[text], Os::MacOS).is_ok());
    }

    #[test]
    fn modifier_only_or_unknown_keys_are_not_recorded() {
        assert_eq!(chord_text(cmd(), "", Os::MacOS), None);
        assert_eq!(chord_text(cmd(), "shift", Os::MacOS), None);
        assert_eq!(chord_text(cmd(), "cmd", Os::MacOS), None);
        assert_eq!(chord_text(cmd(), "unknownkey", Os::MacOS), None);
    }

    #[test]
    fn every_result_parses() {
        for key in [
            "a",
            "z",
            "0",
            "9",
            "enter",
            "tab",
            "escape",
            "backspace",
            "delete",
            "home",
            "end",
            "pagedown",
            "left",
            "right",
            "down",
            "f1",
            "f12",
            "[",
            "]",
            ";",
            "'",
            "/",
            "\\",
            "`",
            "-",
            "=",
        ] {
            let text = chord_text(cmd(), key, Os::MacOS).unwrap_or_else(|| panic!("{key}"));
            assert!(Shortcut::parse_seq(&[&text], Os::MacOS).is_ok(), "{text}");
        }
    }
}
