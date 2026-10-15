//! Native operations on the panel window. gpui has no API to hide or move a single window, so here the NSWindow
//! is obtained via raw-window-handle and manipulated directly. Does nothing when no native window is available (e.g. gpui's test platform).

use std::path::Path;

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{NSBeep, NSEvent, NSScreen, NSView, NSWindow, NSWorkspace};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString, NSURL};

use crate::ax;
use crate::geometry::{Rect, centered_origin, panel_size, quartz_to_cocoa, screen_containing};
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

/// Works out where the panel goes: the screen showing the app with `pid` (its focused window), or
/// the screen under the mouse if that is unknown. The size is recomputed for that screen so the
/// panel never overflows a smaller display (spec §6).
///
/// Returns the move instead of making it: AppKit reports a frame change to gpui synchronously from
/// `setFrame`, and gpui drops that report (logging "RefCell already borrowed") when the App is
/// already borrowed, as it is inside any window update. Call the result outside such an update.
pub fn place(window: &impl HasWindowHandle, pid: Option<i32>) -> Option<Box<dyn FnOnce()>> {
    let (Some(window), Some(mtm)) = (ns_window(window), MainThreadMarker::new()) else {
        return None;
    };
    let screens = NSScreen::screens(mtm);
    let frames: Vec<Rect> = screens.iter().map(|s| rect(s.frame())).collect();
    let primary_height = frames.first()?.height;
    let target = pid
        .and_then(ax::focused_window_frame)
        .map(|frame| quartz_to_cocoa(frame, primary_height).center())
        .unwrap_or_else(|| {
            let mouse = NSEvent::mouseLocation();
            (mouse.x, mouse.y)
        });
    let index = screen_containing(&frames, target).unwrap_or(0);
    let visible = rect(screens.objectAtIndex(index).visibleFrame());
    let size = panel_size(visible);
    let (x, y) = centered_origin(visible, size);
    let frame = NSRect::new(NSPoint::new(x, y), NSSize::new(size.0, size.1));
    Some(Box::new(move || window.setFrame_display(frame, true)))
}

fn rect(r: NSRect) -> Rect {
    Rect {
        x: r.origin.x,
        y: r.origin.y,
        width: r.size.width,
        height: r.size.height,
    }
}

pub fn beep() {
    NSBeep();
}

pub fn open_path(path: &Path) {
    let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
    NSWorkspace::sharedWorkspace().openURL(&url);
}
