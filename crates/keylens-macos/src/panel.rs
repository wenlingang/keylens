//! Native operations on the panel window. gpui has no API to hide or move a single window, so here the NSWindow
//! is obtained via raw-window-handle and manipulated directly. Does nothing when no native window is available (e.g. gpui's test platform).

use std::path::Path;

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{NSBeep, NSEvent, NSScreen, NSView, NSWindow, NSWorkspace};
use objc2_foundation::{NSPoint, NSString, NSURL};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

fn ns_window(window: &impl HasWindowHandle) -> Option<Retained<NSWindow>> {
    let handle = HasWindowHandle::window_handle(window).ok()?;
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return None;
    };
    // SAFETY: The ns_view in the AppKit handle points to a valid NSView whose lifetime is managed by the window.
    let view: &NSView = unsafe { &*(handle.ns_view.as_ptr() as *const NSView) };
    view.window()
}

pub fn hide(window: &impl HasWindowHandle) {
    if let Some(window) = ns_window(window) {
        window.orderOut(None);
    }
}

/// Moves the window to the center (slightly above middle) of the visible area of the screen under the mouse; size is unchanged.
pub fn center_on_mouse_screen(window: &impl HasWindowHandle) {
    let (Some(window), Some(mtm)) = (ns_window(window), MainThreadMarker::new()) else {
        return;
    };
    let mouse = NSEvent::mouseLocation();
    let screens = NSScreen::screens(mtm);
    let screen = screens
        .iter()
        .find(|s| {
            let f = s.frame();
            mouse.x >= f.origin.x
                && mouse.x < f.origin.x + f.size.width
                && mouse.y >= f.origin.y
                && mouse.y < f.origin.y + f.size.height
        })
        .or_else(|| NSScreen::mainScreen(mtm));
    let Some(screen) = screen else { return };
    let visible = screen.visibleFrame();
    let size = window.frame().size;
    let x = visible.origin.x + (visible.size.width - size.width) / 2.0;
    // AppKit's coordinate origin is at the bottom left; placing it slightly above the midline looks more centered.
    let y =
        visible.origin.y + (visible.size.height - size.height) / 2.0 + visible.size.height * 0.04;
    window.setFrameOrigin(NSPoint::new(x.round(), y.round()));
}

pub fn beep() {
    NSBeep();
}

pub fn open_path(path: &Path) {
    let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
    NSWorkspace::sharedWorkspace().openURL(&url);
}
