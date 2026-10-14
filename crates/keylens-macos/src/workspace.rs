//! Frontmost app, frontmost-app change notifications, app icons.

use std::ptr::NonNull;
use std::sync::mpsc::Sender;

use block2::RcBlock;
use keylens_core::AppInfo;
use keylens_platform::ListenGuard;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2_app_kit::{
    NSBitmapImageFileType, NSBitmapImageRep, NSRunningApplication, NSWorkspace,
    NSWorkspaceDidActivateApplicationNotification,
};
use objc2_foundation::{
    NSDictionary, NSNotification, NSNotificationCenter, NSObjectProtocol, NSString, NSURL,
};

fn app_info(app: &NSRunningApplication) -> AppInfo {
    AppInfo {
        pid: app.processIdentifier(),
        name: app
            .localizedName()
            .map(|s| s.to_string())
            .unwrap_or_default(),
        ids: app
            .bundleIdentifier()
            .map(|s| s.to_string())
            .into_iter()
            .collect(),
    }
}

pub fn frontmost_app() -> Option<AppInfo> {
    NSWorkspace::sharedWorkspace()
        .frontmostApplication()
        .map(|app| app_info(&app))
}

/// A registered notification observer, unregistered on drop.
struct Observer {
    center: Retained<NSNotificationCenter>,
    token: Retained<ProtocolObject<dyn NSObjectProtocol>>,
}

// SAFETY: NSNotificationCenter is thread-safe; observers can be unregistered from any thread.
unsafe impl Send for Observer {}

impl Drop for Observer {
    fn drop(&mut self) {
        let token: &AnyObject = (*self.token).as_ref();
        // SAFETY: token is the observer object returned by addObserverForName.
        unsafe { self.center.removeObserver(token) };
    }
}

pub fn watch_frontmost(tx: Sender<AppInfo>) -> ListenGuard {
    let center = NSWorkspace::sharedWorkspace().notificationCenter();
    let block = RcBlock::new(move |_notification: NonNull<NSNotification>| {
        if let Some(app) = frontmost_app() {
            let _ = tx.send(app);
        }
    });
    // SAFETY: The notification name is a system constant; the block is retained by the notification center.
    let token = unsafe {
        center.addObserverForName_object_queue_usingBlock(
            Some(NSWorkspaceDidActivateApplicationNotification),
            None,
            None,
            &block,
        )
    };
    ListenGuard::new(Observer { center, token })
}

/// App icon, PNG-encoded. May be called from a background thread, so an autoreleasepool releases intermediate objects promptly.
pub fn app_icon(pid: i32) -> Option<Vec<u8>> {
    objc2::rc::autoreleasepool(|_| {
        let app = NSRunningApplication::runningApplicationWithProcessIdentifier(pid)?;
        let tiff = app.icon()?.TIFFRepresentation()?;
        let bitmap = NSBitmapImageRep::imageRepWithData(&tiff)?;
        // SAFETY: An empty dictionary is a valid properties argument.
        let png = unsafe {
            bitmap.representationUsingType_properties(
                NSBitmapImageFileType::PNG,
                &NSDictionary::new(),
            )
        }?;
        Some(png.to_vec())
    })
}

/// Opens "System Settings › Privacy & Security › Accessibility".
pub fn open_accessibility_settings() {
    let url = NSString::from_str(
        "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
    );
    if let Some(url) = NSURL::URLWithString(&url) {
        NSWorkspace::sharedWorkspace().openURL(&url);
    }
}
