//! macOS 原始数据与 keylens-core 类型之间的纯转换。不调用系统 API，可以在任何平台测试。

use keylens_core::{Chord, Key, Modifiers};

/// `AXMenuItemCmdModifiers`：bit0 = ⇧，bit1 = ⌥，bit2 = ⌃，bit3 = 不带 ⌘。
/// 编码见 `docs/spikes/2026-10-14-macos-probe.md`。
pub fn ax_modifiers(raw: i64) -> Modifiers {
    let mut modifiers = Modifiers::NONE;
    if raw & 8 == 0 {
        modifiers = modifiers | Modifiers::META;
    }
    if raw & 1 != 0 {
        modifiers = modifiers | Modifiers::SHIFT;
    }
    if raw & 2 != 0 {
        modifiers = modifiers | Modifiers::ALT;
    }
    if raw & 4 != 0 {
        modifiers = modifiers | Modifiers::CTRL;
    }
    modifiers
}

/// 有专门名称的按键和它们的虚拟键码（美式键盘）。
const SPECIAL_VKEYS: &[(u16, Key)] = &[
    (36, Key::Enter),
    (76, Key::Enter),
    (48, Key::Tab),
    (49, Key::Space),
    (51, Key::Backspace),
    (53, Key::Escape),
    (117, Key::Delete),
    (115, Key::Home),
    (119, Key::End),
    (116, Key::PageUp),
    (121, Key::PageDown),
    (123, Key::Left),
    (124, Key::Right),
    (125, Key::Down),
    (126, Key::Up),
    (122, Key::F(1)),
    (120, Key::F(2)),
    (99, Key::F(3)),
    (118, Key::F(4)),
    (96, Key::F(5)),
    (97, Key::F(6)),
    (98, Key::F(7)),
    (100, Key::F(8)),
    (101, Key::F(9)),
    (109, Key::F(10)),
    (103, Key::F(11)),
    (111, Key::F(12)),
    (105, Key::F(13)),
    (107, Key::F(14)),
    (113, Key::F(15)),
    (106, Key::F(16)),
    (64, Key::F(17)),
    (79, Key::F(18)),
    (80, Key::F(19)),
    (90, Key::F(20)),
];

/// 字母、数字和标点的虚拟键码（美式键盘）。
const CHAR_VKEYS: &[(char, u16)] = &[
    ('A', 0),
    ('S', 1),
    ('D', 2),
    ('F', 3),
    ('H', 4),
    ('G', 5),
    ('Z', 6),
    ('X', 7),
    ('C', 8),
    ('V', 9),
    ('B', 11),
    ('Q', 12),
    ('W', 13),
    ('E', 14),
    ('R', 15),
    ('Y', 16),
    ('T', 17),
    ('1', 18),
    ('2', 19),
    ('3', 20),
    ('4', 21),
    ('6', 22),
    ('5', 23),
    ('=', 24),
    ('9', 25),
    ('7', 26),
    ('-', 27),
    ('8', 28),
    ('0', 29),
    (']', 30),
    ('O', 31),
    ('U', 32),
    ('[', 33),
    ('I', 34),
    ('P', 35),
    ('L', 37),
    ('J', 38),
    ('\'', 39),
    ('K', 40),
    (';', 41),
    ('\\', 42),
    (',', 43),
    ('/', 44),
    ('N', 45),
    ('M', 46),
    ('.', 47),
    ('`', 50),
];

/// AppKit 用私有区字符和控制字符表示的特殊键。
fn special_char(c: char) -> Option<Key> {
    let key = match c {
        '\u{F700}' => Key::Up,
        '\u{F701}' => Key::Down,
        '\u{F702}' => Key::Left,
        '\u{F703}' => Key::Right,
        '\u{F704}'..='\u{F71B}' => Key::F((c as u32 - 0xF704 + 1) as u8),
        '\u{F728}' => Key::Delete,
        '\u{F729}' => Key::Home,
        '\u{F72B}' => Key::End,
        '\u{F72C}' => Key::PageUp,
        '\u{F72D}' => Key::PageDown,
        '\u{7f}' | '\u{8}' => Key::Backspace,
        '\r' | '\u{3}' => Key::Enter,
        '\t' => Key::Tab,
        '\u{1b}' => Key::Escape,
        ' ' => Key::Space,
        _ => return None,
    };
    Some(key)
}

/// 菜单项快捷键的主键。vkey 是有名称的特殊键时以 vkey 为准（此时 char 可能是 `⎋` 这类符号）；
/// 否则用 char。表示不了的键（如 🌐）返回 `None`，调用方跳过这个菜单项。
pub fn menu_key(cmd_char: Option<&str>, vkey: Option<i64>) -> Option<Key> {
    if let Some(code) = vkey.and_then(|v| u16::try_from(v).ok())
        && let Some(&(_, key)) = SPECIAL_VKEYS.iter().find(|(c, _)| *c == code)
    {
        return Some(key);
    }
    let mut chars = cmd_char?.chars();
    let c = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    if let Some(key) = special_char(c) {
        return Some(key);
    }
    (c.is_alphanumeric() || c.is_ascii_punctuation()).then(|| Key::from_char(c))
}

pub fn menu_chord(
    cmd_char: Option<&str>,
    vkey: Option<i64>,
    modifiers: Option<i64>,
) -> Option<Chord> {
    let key = menu_key(cmd_char, vkey)?;
    Some(Chord {
        modifiers: ax_modifiers(modifiers.unwrap_or(0)),
        key,
    })
}

const FLAG_SHIFT: u64 = 0x0002_0000;
const FLAG_CONTROL: u64 = 0x0004_0000;
const FLAG_ALTERNATE: u64 = 0x0008_0000;
const FLAG_COMMAND: u64 = 0x0010_0000;
const FLAG_FN: u64 = 0x0080_0000;

const FLAG_TABLE: [(u64, Modifiers); 5] = [
    (FLAG_COMMAND, Modifiers::META),
    (FLAG_SHIFT, Modifiers::SHIFT),
    (FLAG_ALTERNATE, Modifiers::ALT),
    (FLAG_CONTROL, Modifiers::CTRL),
    (FLAG_FN, Modifiers::FN),
];

/// CGEvent 的 flags 转成修饰键集合，忽略设备相关的位（如左右键标记 0x108）。
pub fn event_modifiers(flags: u64) -> Modifiers {
    FLAG_TABLE
        .iter()
        .filter(|(bit, _)| flags & bit != 0)
        .fold(Modifiers::NONE, |acc, (_, m)| acc | *m)
}

/// 修饰键集合转成 CGEvent flags，用于模拟按键。
pub fn modifier_flags(modifiers: Modifiers) -> u64 {
    FLAG_TABLE
        .iter()
        .filter(|(_, m)| modifiers.contains(*m))
        .fold(0, |acc, (bit, _)| acc | bit)
}

/// 按键对应的虚拟键码（美式键盘）。需要 Shift 才能打出的符号返回 `None`，
/// 调用方应先用 `Chord::canonical` 转成「⇧ + 基础键」。
pub fn keycode(key: Key) -> Option<u16> {
    if let Key::Char(c) = key {
        return CHAR_VKEYS
            .iter()
            .find(|(ch, _)| *ch == c)
            .map(|(_, code)| *code);
    }
    SPECIAL_VKEYS
        .iter()
        .find(|(code, k)| *k == key && *code != 76)
        .map(|(code, _)| *code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use keylens_core::Os;

    #[test]
    fn ax_modifier_bits_match_measured_encoding() {
        assert_eq!(ax_modifiers(0), Modifiers::META);
        assert_eq!(ax_modifiers(1), Modifiers::META | Modifiers::SHIFT);
        assert_eq!(ax_modifiers(2), Modifiers::META | Modifiers::ALT);
        assert_eq!(
            ax_modifiers(3),
            Modifiers::META | Modifiers::SHIFT | Modifiers::ALT
        );
        assert_eq!(ax_modifiers(4), Modifiers::META | Modifiers::CTRL);
        assert_eq!(ax_modifiers(8), Modifiers::NONE);
        assert_eq!(ax_modifiers(10), Modifiers::ALT);
        assert_eq!(ax_modifiers(12), Modifiers::CTRL);
    }

    #[test]
    fn special_keys_come_from_vkey_even_when_char_is_a_symbol() {
        assert_eq!(menu_key(Some("⎋"), Some(53)), Some(Key::Escape));
        assert_eq!(menu_key(Some("\t"), Some(48)), Some(Key::Tab));
        assert_eq!(menu_key(None, Some(126)), Some(Key::Up));
        assert_eq!(menu_key(None, Some(122)), Some(Key::F(1)));
        assert_eq!(menu_key(None, Some(111)), Some(Key::F(12)));
    }

    #[test]
    fn printable_chars_become_keys_and_letters_are_uppercased() {
        assert_eq!(menu_key(Some("s"), None), Some(Key::Char('S')));
        assert_eq!(menu_key(Some("+"), None), Some(Key::Char('+')));
        assert_eq!(menu_key(Some(","), None), Some(Key::Char(',')));
    }

    #[test]
    fn function_key_private_use_chars_are_recognized() {
        assert_eq!(menu_key(Some("\u{F700}"), None), Some(Key::Up));
        assert_eq!(menu_key(Some("\u{F703}"), None), Some(Key::Right));
        assert_eq!(menu_key(Some("\u{F704}"), None), Some(Key::F(1)));
        assert_eq!(menu_key(Some("\u{F728}"), None), Some(Key::Delete));
        assert_eq!(menu_key(Some("\u{7f}"), None), Some(Key::Backspace));
        assert_eq!(menu_key(Some("\r"), None), Some(Key::Enter));
    }

    #[test]
    fn unrepresentable_keys_are_skipped() {
        assert_eq!(menu_key(Some("🌐"), Some(128)), None);
        assert_eq!(menu_key(Some(""), None), None);
        assert_eq!(menu_key(None, None), None);
        assert_eq!(menu_key(Some("ab"), None), None);
    }

    #[test]
    fn menu_chord_combines_key_and_modifiers() {
        let chord = menu_chord(Some("S"), None, Some(1)).unwrap();
        assert_eq!(chord.display(Os::MacOS), "⇧⌘S");
        assert_eq!(
            menu_chord(Some("N"), None, None)
                .unwrap()
                .display(Os::MacOS),
            "⌘N"
        );
        assert!(menu_chord(None, None, Some(0)).is_none());
    }

    #[test]
    fn event_flags_ignore_device_bits() {
        assert_eq!(event_modifiers(0x0010_0108), Modifiers::META);
        assert_eq!(event_modifiers(0x0002_0102), Modifiers::SHIFT);
        assert_eq!(event_modifiers(0x0000_0100), Modifiers::NONE);
        assert_eq!(
            event_modifiers(0x0014_0000),
            Modifiers::META | Modifiers::CTRL
        );
        assert_eq!(event_modifiers(0x0080_0000), Modifiers::FN);
    }

    #[test]
    fn modifier_flags_round_trip() {
        for m in [
            Modifiers::NONE,
            Modifiers::META | Modifiers::SHIFT,
            Modifiers::CTRL | Modifiers::ALT | Modifiers::FN,
        ] {
            assert_eq!(event_modifiers(modifier_flags(m)), m);
        }
    }

    #[test]
    fn keycodes_follow_us_layout() {
        assert_eq!(keycode(Key::Char('A')), Some(0));
        assert_eq!(keycode(Key::Char('K')), Some(40));
        assert_eq!(keycode(Key::Char('1')), Some(18));
        assert_eq!(keycode(Key::Char('/')), Some(44));
        assert_eq!(keycode(Key::Up), Some(126));
        assert_eq!(keycode(Key::F(12)), Some(111));
        assert_eq!(keycode(Key::Escape), Some(53));
        assert_eq!(keycode(Key::Char('+')), None);
        assert_eq!(keycode(Key::F(24)), None);
    }

    #[test]
    fn every_special_vkey_round_trips() {
        for &(code, key) in SPECIAL_VKEYS {
            if code == 76 {
                continue; // 小键盘回车映射到 Enter，反查时用主键盘回车 36
            }
            assert_eq!(keycode(key), Some(code), "{key:?}");
        }
    }
}
