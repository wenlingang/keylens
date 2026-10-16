//! Native operations on the panel window. gpui has no API to hide or move a single window, so here the NSWindow
//! is obtained via raw-window-handle and manipulated directly. Does nothing when no native window is available (e.g. gpui's test platform).

use std::path::Path;

use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{
    NSAnimatablePropertyContainer, NSAnimationContext, NSBeep, NSEvent, NSNormalWindowLevel,
    NSPopUpMenuWindowLevel, NSScreen, NSView, NSWindow, NSWorkspace,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString, NSURL};

use crate::ax;
use crate::geometry::{
    Rect, centered_origin, panel_centered_origin, panel_size, quartz_to_cocoa, screen_containing,
};
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

/// Whether the user turned on Reduce Motion in the system accessibility settings.
pub fn reduce_motion() -> bool {
    NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()
}

/// How long the panel takes to fade in.
const FADE_IN_SECONDS: f64 = 0.16;

/// Fades the window in from transparent. Call right after ordering it on screen; skipped when the
/// user asked macOS to reduce motion.
pub fn fade_in(window: &impl HasWindowHandle) {
    let Some(window) = ns_window(window) else {
        return;
    };
    if reduce_motion() {
        window.setAlphaValue(1.0);
        return;
    }
    window.setAlphaValue(0.0);
    NSAnimationContext::beginGrouping();
    NSAnimationContext::currentContext().setDuration(FADE_IN_SECONDS);
    window.animator().setAlphaValue(1.0);
    NSAnimationContext::endGrouping();
}

/// Puts the panel at its pop-up level, above ordinary windows (`raised`), or at the normal level
/// so another Keylens window (settings) can come in front of it. gpui has no API for this.
pub fn set_raised(window: &impl HasWindowHandle, raised: bool) {
    if let Some(window) = ns_window(window) {
        window.setLevel(if raised {
            NSPopUpMenuWindowLevel
        } else {
            NSNormalWindowLevel
        });
    }
}

/// The visible area of the screen showing the app with `pid` (its focused window), or of the
/// screen under the mouse if that is unknown.
fn target_screen(mtm: MainThreadMarker, pid: Option<i32>) -> Option<Rect> {
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
    Some(rect(screens.objectAtIndex(index).visibleFrame()))
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
    let visible = target_screen(mtm, pid)?;
    let size = panel_size(visible);
    let (x, y) = centered_origin(visible, size);
    let frame = NSRect::new(NSPoint::new(x, y), NSSize::new(size.0, size.1));
    Some(Box::new(move || window.setFrame_display(frame, true)))
}

/// Shows the hold animation's window centred on where the panel will appear, above everything,
/// without taking focus or clicks: the user may still be on the way to an ordinary ⌘ shortcut,
/// and the app in front must keep the keyboard. Like [`place`], returns the move to make outside
/// a window update.
pub fn show_overlay(window: &impl HasWindowHandle, pid: Option<i32>) -> Option<Box<dyn FnOnce()>> {
    let (Some(window), Some(mtm)) = (ns_window(window), MainThreadMarker::new()) else {
        return None;
    };
    let visible = target_screen(mtm, pid)?;
    let size = window.frame().size;
    let (x, y) = panel_centered_origin(visible, (size.width, size.height));
    Some(Box::new(move || {
        window.setIgnoresMouseEvents(true);
        // One level above the panel, which sits at the pop-up menu level.
        window.setLevel(NSPopUpMenuWindowLevel + 1);
        window.setFrameOrigin(NSPoint::new(x, y));
        window.setAlphaValue(1.0);
        window.orderFrontRegardless();
    }))
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

/// The running app's bundle identifier; `None` outside an app bundle.
pub fn bundle_identifier() -> Option<String> {
    objc2_foundation::NSBundle::mainBundle()
        .bundleIdentifier()
        .map(|id| id.to_string())
}

pub fn open_path(path: &Path) {
    let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
    NSWorkspace::sharedWorkspace().openURL(&url);
}
