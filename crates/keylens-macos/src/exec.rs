//! Executes entries: menu entries are pressed with AXPress, community entries synthesize key events.

use std::thread::sleep;
use std::time::Duration;

use keylens_core::{Entry, Shortcut};
use keylens_platform::PlatformError;
use objc2_core_foundation::{CFPreferencesCopyAppValue, CFString};
use objc2_core_graphics::{CGEvent, CGEventFlags, CGEventTapLocation};

use crate::ax::{self, AxNode};
use crate::convert::{keycode, modifier_flags, us_compatible_layout};
use crate::snapshots::Snapshots;

/// After the panel closes, wait for focus to return to the target app before sending keys (300ms was enough in experiments; a smaller value is used here).
const KEY_POST_DELAY: Duration = Duration::from_millis(80);
/// Interval between consecutive key presses (e.g. ⌘K ⌘S).
const CHORD_GAP: Duration = Duration::from_millis(50);

/// Current keyboard layout ID (e.g. `com.apple.keylayout.US`). While an input method is active this is still the underlying layout.
/// Reads system preferences instead of using TIS functions, since calling TIS from a background thread may trigger a main-thread assertion.
fn current_layout() -> Option<String> {
    let value = CFPreferencesCopyAppValue(
        &CFString::from_str("AppleCurrentKeyboardLayoutInputSourceID"),
        &CFString::from_str("com.apple.HIToolbox"),
    )?;
    value.downcast::<CFString>().ok().map(|s| s.to_string())
}

pub fn press_menu(snapshots: &Snapshots<AxNode>, entry: &Entry) -> Result<(), PlatformError> {
    let node = snapshots.handle(entry)?;
    if ax::title(&node).as_deref() != Some(entry.title.as_str()) {
        return Err(PlatformError::Stale);
    }
    ax::press(&node)
}

pub fn post_shortcut(shortcut: &Shortcut) -> Result<(), PlatformError> {
    if let Some(layout) = current_layout()
        && !us_compatible_layout(&layout)
    {
        return Err(PlatformError::Other(format!(
            "keyboard layout {layout} is not supported for synthesized shortcuts"
        )));
    }
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
