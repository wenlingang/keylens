//! 执行条目：菜单条目用 AXPress 点按，社区条目模拟按键。

use std::thread::sleep;
use std::time::Duration;

use keylens_core::{Entry, Shortcut};
use keylens_platform::PlatformError;
use objc2_core_foundation::{CFPreferencesCopyAppValue, CFString};
use objc2_core_graphics::{CGEvent, CGEventFlags, CGEventTapLocation};

use crate::ax::{self, AxNode};
use crate::convert::{keycode, modifier_flags, us_compatible_layout};
use crate::snapshots::Snapshots;

/// 浮层关闭后等焦点回到目标应用，再发送按键（实验中 300ms 足够，这里取较小值）。
const KEY_POST_DELAY: Duration = Duration::from_millis(80);
/// 连续按键（如 ⌘K ⌘S）之间的间隔。
const CHORD_GAP: Duration = Duration::from_millis(50);

/// 当前键盘布局 ID（如 `com.apple.keylayout.US`）。输入法激活时仍是底层布局。
/// 读系统偏好而不用 TIS 函数，因为 TIS 在后台线程调用可能触发主线程断言。
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
