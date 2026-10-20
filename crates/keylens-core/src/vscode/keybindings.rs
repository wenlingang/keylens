//! Parses VS Code's user `keybindings.json` (JSONC) into shortcut bindings.

use crate::keys::{Os, Shortcut};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    pub key: Option<Shortcut>,
    pub command: String,
    pub remove: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Parsed {
    pub bindings: Vec<Binding>,
    pub skipped: usize,
}

pub fn parse_keybindings(text: &str, os: Os) -> Result<Parsed, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let clean = strip_trailing_commas(&strip_comments(text));
    if clean.trim().is_empty() {
        return Ok(Parsed::default());
    }
    let entries: Vec<serde_json::Value> =
        serde_json::from_str(&clean).map_err(|e| format!("invalid keybindings.json: {e}"))?;

    let mut parsed = Parsed::default();
    for entry in &entries {
        let Some(command) = entry.get("command").and_then(|v| v.as_str()) else {
            continue;
        };
        let (command, remove) = match command.strip_prefix('-') {
            Some(rest) => (rest.to_string(), true),
            None => (command.to_string(), false),
        };
        let key = match entry.get("key").and_then(|v| v.as_str()) {
            Some(raw) => {
                let parts: Vec<&str> = raw.split_whitespace().collect();
                match Shortcut::parse_seq(&parts, os) {
                    Ok(shortcut) => Some(shortcut),
                    Err(_) => {
                        parsed.skipped += 1;
                        continue;
                    }
                }
            }
            None if remove => None,
            None => continue,
        };
        parsed.bindings.push(Binding {
            key,
            command,
            remove,
        });
    }
    Ok(parsed)
}

/// Removes `//` and `/* */` comments outside of string literals.
fn strip_comments(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    let mut in_string = false;
    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            if c == '\\' {
                if let Some(n) = chars.next() {
                    out.push(n);
                }
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match (c, chars.peek()) {
            ('"', _) => {
                in_string = true;
                out.push(c);
            }
            ('/', Some('/')) => {
                for n in chars.by_ref() {
                    if n == '\n' {
                        out.push('\n');
                        break;
                    }
                }
            }
            ('/', Some('*')) => {
                chars.next();
                let mut prev = '\0';
                for n in chars.by_ref() {
                    if prev == '*' && n == '/' {
                        break;
                    }
                    prev = n;
                }
                out.push(' ');
            }
            _ => out.push(c),
        }
    }
    out
}

/// Removes commas that are directly followed (ignoring whitespace) by `]` or `}`, outside strings.
fn strip_trailing_commas(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut in_string = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if in_string {
            out.push(c);
            if c == '\\' {
                if let Some(&n) = chars.get(i + 1) {
                    out.push(n);
                    i += 1;
                }
            } else if c == '"' {
                in_string = false;
            }
        } else if c == '"' {
            in_string = true;
            out.push(c);
        } else if c == ',' {
            let next = chars[i + 1..].iter().find(|n| !n.is_whitespace());
            if !matches!(next, Some(']') | Some('}')) {
                out.push(c);
            }
        } else {
            out.push(c);
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_jsonc_with_comments_and_trailing_commas() {
        let text = r#"// Place your key bindings in this file
[
  /* block */ { "key": "ctrl+shift+alt+p", "command": "workbench.action.showCommands" },
  { "key": "ctrl+k ctrl+s", "command": "workbench.action.openGlobalKeybindings", },
]"#;
        let p = parse_keybindings(text, Os::Windows).unwrap();
        assert_eq!(p.bindings.len(), 2);
        assert_eq!(
            p.bindings[0].key.as_ref().unwrap().display(Os::Windows),
            "Ctrl+Alt+Shift+P"
        );
        assert_eq!(p.bindings[1].key.as_ref().unwrap().chords().len(), 2);
    }

    #[test]
    fn comment_markers_inside_strings_are_kept() {
        let text = r#"[{ "key": "ctrl+u", "command": "type", "args": { "text": "http://a/*b*/ \" // x" } }]"#;
        assert_eq!(
            parse_keybindings(text, Os::Windows).unwrap().bindings.len(),
            1
        );
    }

    #[test]
    fn removal_with_and_without_key() {
        let text = r#"[{ "key": "ctrl+d", "command": "-editor.action.addSelectionToNextFindMatch" },
                  { "command": "-workbench.action.quickOpen" }]"#;
        let p = parse_keybindings(text, Os::Windows).unwrap();
        assert!(p.bindings.iter().all(|b| b.remove));
        assert_eq!(
            p.bindings[0].command,
            "editor.action.addSelectionToNextFindMatch"
        );
        assert!(p.bindings[1].key.is_none());
    }

    #[test]
    fn unknown_keys_are_skipped_not_fatal() {
        let text = r#"[{ "key": "numpad1", "command": "a" }, { "key": "[KeyA]", "command": "b" }, { "key": "cmd+s", "command": "c" }]"#;
        let p = parse_keybindings(text, Os::MacOS).unwrap();
        assert_eq!((p.bindings.len(), p.skipped), (1, 2));
    }

    #[test]
    fn non_array_is_an_error() {
        assert!(parse_keybindings("{}", Os::MacOS).is_err());
        assert!(parse_keybindings("", Os::MacOS).is_ok());
        assert!(
            parse_keybindings("  \n", Os::MacOS)
                .unwrap()
                .bindings
                .is_empty()
        );
    }

    #[test]
    fn leading_bom_is_ignored() {
        let text = "\u{feff}[{ \"key\": \"cmd+s\", \"command\": \"a\" }]";
        let p = parse_keybindings(text, Os::MacOS).unwrap();
        assert_eq!(p.bindings.len(), 1);
        assert_eq!(p.bindings[0].command, "a");
    }
}
