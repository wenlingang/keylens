//! 前台应用、前台应用切换通知、应用图标。

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

/// 注册的通知观察者，drop 时注销。
struct Observer {
    center: Retained<NSNotificationCenter>,
    token: Retained<ProtocolObject<dyn NSObjectProtocol>>,
}

// SAFETY: NSNotificationCenter 是线程安全的，可以在任意线程注销观察者。
unsafe impl Send for Observer {}

impl Drop for Observer {
    fn drop(&mut self) {
        let token: &AnyObject = (*self.token).as_ref();
        // SAFETY: token 是 addObserverForName 返回的观察者对象。
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
    // SAFETY: 通知名是系统常量；block 会被通知中心 retain。
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

/// 应用图标，PNG 编码。可能在后台线程调用，所以用 autoreleasepool 及时释放中间对象。
pub fn app_icon(pid: i32) -> Option<Vec<u8>> {
    objc2::rc::autoreleasepool(|_| {
        let app = NSRunningApplication::runningApplicationWithProcessIdentifier(pid)?;
        let tiff = app.icon()?.TIFFRepresentation()?;
        let bitmap = NSBitmapImageRep::imageRepWithData(&tiff)?;
        // SAFETY: 空字典是合法的 properties 参数。
        let png = unsafe {
            bitmap.representationUsingType_properties(
                NSBitmapImageFileType::PNG,
                &NSDictionary::new(),
            )
        }?;
        Some(png.to_vec())
    })
}

/// 打开「系统设置 › 隐私与安全性 › 辅助功能」。
pub fn open_accessibility_settings() {
    let url = NSString::from_str(
        "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility",
    );
    if let Some(url) = NSURL::URLWithString(&url) {
        NSWorkspace::sharedWorkspace().openURL(&url);
    }
}
