//! 全局按键监听：只监听、不拦截的 CGEventTap，运行在独立线程上。

use std::cell::OnceCell;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::mpsc::{Sender, channel};

use keylens_core::KeyEvent;
use keylens_platform::{ListenGuard, PlatformError};
use objc2_core_foundation::{CFMachPort, CFRetained, CFRunLoop, kCFRunLoopCommonModes};
use objc2_core_graphics::{
    CGEvent, CGEventMask, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
    CGEventTapProxy, CGEventType,
};

use crate::convert::event_modifiers;

/// 回调的上下文，只在监听线程上访问。
struct Context {
    tx: Sender<KeyEvent>,
    port: OnceCell<CFRetained<CFMachPort>>,
}

/// 监听线程的 run loop。
struct RunLoopHandle(CFRetained<CFRunLoop>);

// SAFETY: 只用来调用 CFRunLoopStop，Apple 文档说明它可以从任意线程调用。
unsafe impl Send for RunLoopHandle {}

impl Drop for RunLoopHandle {
    fn drop(&mut self) {
        self.0.stop();
    }
}

unsafe extern "C-unwind" fn on_event(
    _proxy: CGEventTapProxy,
    ty: CGEventType,
    event: NonNull<CGEvent>,
    info: *mut c_void,
) -> *mut CGEvent {
    // SAFETY: `info` 是 `start` 里传入的 Box<Context> 指针，在监听线程结束前一直有效。
    let context = unsafe { &*(info as *const Context) };
    if ty == CGEventType::TapDisabledByTimeout || ty == CGEventType::TapDisabledByUserInput {
        if let Some(port) = context.port.get() {
            CGEvent::tap_enable(port, true);
        }
        return event.as_ptr();
    }
    let key_event = if ty == CGEventType::FlagsChanged {
        // SAFETY: 回调期间事件有效。
        let flags = CGEvent::flags(Some(unsafe { event.as_ref() }));
        Some(KeyEvent::ModifiersChanged(event_modifiers(flags.0)))
    } else if ty == CGEventType::KeyDown {
        Some(KeyEvent::KeyDown)
    } else if ty == CGEventType::LeftMouseDown || ty == CGEventType::RightMouseDown {
        Some(KeyEvent::MouseDown)
    } else {
        None
    };
    if let Some(key_event) = key_event {
        let _ = context.tx.send(key_event);
    }
    event.as_ptr()
}

pub fn start(tx: Sender<KeyEvent>) -> Result<ListenGuard, PlatformError> {
    let (ready_tx, ready_rx) = channel::<Result<RunLoopHandle, PlatformError>>();
    std::thread::Builder::new()
        .name("keylens-keytap".into())
        .spawn(move || {
            let context = Box::into_raw(Box::new(Context {
                tx,
                port: OnceCell::new(),
            }));
            let mask: CGEventMask = (1 << CGEventType::FlagsChanged.0)
                | (1 << CGEventType::KeyDown.0)
                | (1 << CGEventType::LeftMouseDown.0)
                | (1 << CGEventType::RightMouseDown.0);
            // SAFETY: 回调签名符合要求；`context` 在本线程结束前不会被释放。
            let port = unsafe {
                CGEvent::tap_create(
                    CGEventTapLocation::SessionEventTap,
                    CGEventTapPlacement::HeadInsertEventTap,
                    CGEventTapOptions::ListenOnly,
                    mask,
                    Some(on_event),
                    context.cast(),
                )
            };
            let Some(port) = port else {
                // SAFETY: tap 创建失败，回调不会被调用，可以立即释放上下文。
                drop(unsafe { Box::from_raw(context) });
                let _ = ready_tx.send(Err(PlatformError::PermissionDenied));
                return;
            };
            let source = CFMachPort::new_run_loop_source(None, Some(&port), 0);
            let run_loop = CFRunLoop::current();
            let (Some(source), Some(run_loop)) = (source, run_loop) else {
                // SAFETY: 还没有进入 run loop，回调不会被调用。
                drop(unsafe { Box::from_raw(context) });
                let _ = ready_tx.send(Err(PlatformError::Other(
                    "cannot create run loop source".into(),
                )));
                return;
            };
            // SAFETY: kCFRunLoopCommonModes 是系统常量。
            run_loop.add_source(Some(&source), unsafe { kCFRunLoopCommonModes });
            CGEvent::tap_enable(&port, true);
            // SAFETY: `context` 仍然有效；只在本线程访问。
            let _ = unsafe { &*context }.port.set(port.clone());
            let _ = ready_tx.send(Ok(RunLoopHandle(run_loop)));
            CFRunLoop::run();
            CGEvent::tap_enable(&port, false);
            // SAFETY: run loop 已经退出，回调不会再被调用。
            drop(unsafe { Box::from_raw(context) });
        })
        .map_err(|e| PlatformError::Other(e.to_string()))?;
    let handle = ready_rx
        .recv()
        .map_err(|_| PlatformError::Other("key tap thread exited".into()))??;
    Ok(ListenGuard::new(handle))
}
