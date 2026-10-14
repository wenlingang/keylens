//! 浮层窗口的原生操作。gpui 没有隐藏、移动单个窗口的接口，这里通过 raw-window-handle
//! 拿到 NSWindow 直接操作。拿不到原生窗口（例如 gpui 测试平台）时什么都不做。

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
    // SAFETY: AppKit 句柄里的 ns_view 指向一个有效的 NSView，生命周期由窗口管理。
    let view: &NSView = unsafe { &*(handle.ns_view.as_ptr() as *const NSView) };
    view.window()
}

pub fn hide(window: &impl HasWindowHandle) {
    if let Some(window) = ns_window(window) {
        window.orderOut(None);
    }
}

/// 把窗口移到鼠标所在屏幕的可视区域中央（略偏上），大小不变。
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
    // AppKit 坐标原点在左下角；放在中线略偏上，视觉上更居中。
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
