//! 执行条目：菜单条目用 AXPress 点按，社区条目模拟按键。

use std::thread::sleep;
use std::time::Duration;

use keylens_core::{Entry, Shortcut};
use keylens_platform::PlatformError;
use objc2_core_graphics::{CGEvent, CGEventFlags, CGEventTapLocation};

use crate::ax::{self, AxNode};
use crate::convert::{keycode, modifier_flags};
use crate::snapshots::Snapshots;

/// 浮层关闭后等焦点回到目标应用，再发送按键（实验中 300ms 足够，这里取较小值）。
const KEY_POST_DELAY: Duration = Duration::from_millis(80);
/// 连续按键（如 ⌘K ⌘S）之间的间隔。
const CHORD_GAP: Duration = Duration::from_millis(50);

pub fn press_menu(snapshots: &Snapshots<AxNode>, entry: &Entry) -> Result<(), PlatformError> {
    let node = snapshots.handle(entry)?;
    if ax::title(&node).as_deref() != Some(entry.title.as_str()) {
        return Err(PlatformError::Stale);
    }
    ax::press(&node)
}

pub fn post_shortcut(shortcut: &Shortcut) -> Result<(), PlatformError> {
    let chords: Vec<_> = shortcut.chords().iter().map(|c| c.canonical()).collect();
    let codes = chords
        .iter()
        .map(|c| {
            keycode(c.key)
                .ok_or_else(|| PlatformError::Other(format!("no key code for {:?}", c.key)))
        })
        .collect::<Result<Vec<_>, _>>()?;
    sleep(KEY_POST_DELAY);
    for (i, (chord, code)) in chords.iter().zip(codes).enumerate() {
        if i > 0 {
            sleep(CHORD_GAP);
        }
        let flags = CGEventFlags(modifier_flags(chord.modifiers));
        for down in [true, false] {
            let event = CGEvent::new_keyboard_event(None, code, down)
                .ok_or_else(|| PlatformError::Other("cannot create keyboard event".into()))?;
            CGEvent::set_flags(Some(&event), flags);
            CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&event));
        }
    }
    Ok(())
}
