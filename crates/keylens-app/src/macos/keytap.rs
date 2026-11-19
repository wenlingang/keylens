//! Global key listener: a listen-only, non-intercepting CGEventTap running on its own thread.

use std::cell::{Cell, OnceCell};
use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Sender, channel};
use std::time::{Duration, Instant};

use crate::platform::{ListenGuard, PlatformError};
use keylens_core::{KeyEvent, Modifiers};
use objc2_core_foundation::{
    CFMachPort, CFRetained, CFRunLoop, kCFRunLoopCommonModes, kCFRunLoopDefaultMode,
};
use objc2_core_graphics::{
    CGEvent, CGEventField, CGEventMask, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
    CGEventTapProxy, CGEventType,
};

use crate::macos::convert::event_modifiers;
use crate::macos::synthetic;

/// Callback context, accessed only on the listener thread.
struct Context {
    tx: Sender<KeyEvent>,
    port: OnceCell<CFRetained<CFMachPort>>,
    /// The modifiers held after the last FlagsChanged, and when it came.
    modifiers: Cell<(Modifiers, Option<Instant>)>,
}

/// How long after the last modifier change a key or mouse press still matters to a trigger: it
/// covers the longest double-tap interval (600ms).
const PRESS_RELEVANCE: Duration = Duration::from_secs(1);

/// Whether a key or mouse press is worth sending on. Triggers only care about presses while a
/// modifier is held or shortly after one was released (between the taps of a double-tap), so
/// ordinary typing does not wake the app up for every key.
fn press_matters(held: Modifiers, last_change: Option<Instant>, now: Instant) -> bool {
    !held.is_empty()
        || last_change.is_some_and(|at| now.saturating_duration_since(at) < PRESS_RELEVANCE)
}

/// The listener thread's run loop and stop flag.
///
/// Calling only `CFRunLoopStop` is not enough: if it happens before the listener thread enters the run loop,
/// the next run resets the stop state, and the thread and the global tap would live forever. So the listener thread
/// runs a loop with a timeout and checks the stop flag every round.
struct RunLoopHandle {
    run_loop: CFRetained<CFRunLoop>,
    stopped: Arc<AtomicBool>,
}

// SAFETY: Only used to call CFRunLoopStop, which Apple's documentation says may be called from any thread.
unsafe impl Send for RunLoopHandle {}

impl Drop for RunLoopHandle {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        self.run_loop.stop();
        self.run_loop.wake_up();
    }
}

/// Each round the listener thread runs at most this long, then checks the stop flag. Dropping the
/// handle stops the run loop at once, so this only bounds the rare stop that lands before the
/// thread enters it; a long round keeps the idle thread from waking up.
const STOP_CHECK_INTERVAL_SECS: f64 = 5.0;

unsafe extern "C-unwind" fn on_event(
    _proxy: CGEventTapProxy,
    ty: CGEventType,
    event: NonNull<CGEvent>,
    info: *mut c_void,
) -> *mut CGEvent {
    // SAFETY: `info` is the Box<Context> pointer passed in `start`, valid until the listener thread ends.
    let context = unsafe { &*(info as *const Context) };
    if ty == CGEventType::TapDisabledByTimeout || ty == CGEventType::TapDisabledByUserInput {
        if let Some(port) = context.port.get() {
            CGEvent::tap_enable(port, true);
        }
        return event.as_ptr();
    }
    // SAFETY: the event is valid for the duration of the callback.
    let user_data = CGEvent::integer_value_field(
        Some(unsafe { event.as_ref() }),
        CGEventField::EventSourceUserData,
    );
    if synthetic::is_ours(user_data) {
        return event.as_ptr();
    }
    let key_event = if ty == CGEventType::FlagsChanged {
        // SAFETY: The event is valid for the duration of the callback.
        let flags = CGEvent::flags(Some(unsafe { event.as_ref() }));
        let modifiers = event_modifiers(flags.0);
        context.modifiers.set((modifiers, Some(Instant::now())));
        Some(KeyEvent::ModifiersChanged(modifiers))
    } else {
        let press = if ty == CGEventType::KeyDown {
            Some(KeyEvent::KeyDown)
        } else if ty == CGEventType::LeftMouseDown || ty == CGEventType::RightMouseDown {
            Some(KeyEvent::MouseDown)
        } else {
            None
        };
        let (held, last_change) = context.modifiers.get();
        press.filter(|_| press_matters(held, last_change, Instant::now()))
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
                modifiers: Cell::new((Modifiers::NONE, None)),
            }));
            let mask: CGEventMask = (1 << CGEventType::FlagsChanged.0)
                | (1 << CGEventType::KeyDown.0)
                | (1 << CGEventType::LeftMouseDown.0)
                | (1 << CGEventType::RightMouseDown.0);
            // SAFETY: The callback signature is as required; `context` is not freed before this thread ends.
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
                // SAFETY: tap creation failed so the callback will never be called; the context can be freed immediately.
                drop(unsafe { Box::from_raw(context) });
                let _ = ready_tx.send(Err(PlatformError::PermissionDenied));
                return;
            };
            let source = CFMachPort::new_run_loop_source(None, Some(&port), 0);
            let run_loop = CFRunLoop::current();
            let (Some(source), Some(run_loop)) = (source, run_loop) else {
                // SAFETY: The run loop has not been entered yet, so the callback will not be called.
                drop(unsafe { Box::from_raw(context) });
                let _ = ready_tx.send(Err(PlatformError::Other(
                    "cannot create run loop source".into(),
                )));
                return;
            };
            // SAFETY: kCFRunLoopCommonModes is a system constant.
            run_loop.add_source(Some(&source), unsafe { kCFRunLoopCommonModes });
            CGEvent::tap_enable(&port, true);
            // SAFETY: `context` is still valid; accessed only on this thread.
            let _ = unsafe { &*context }.port.set(port.clone());
            let stopped = Arc::new(AtomicBool::new(false));
            let _ = ready_tx.send(Ok(RunLoopHandle {
                run_loop,
                stopped: stopped.clone(),
            }));
            while !stopped.load(Ordering::Acquire) {
                // SAFETY: kCFRunLoopDefaultMode is a system constant; the tap's source is added to the common modes, which include the default mode.
                CFRunLoop::run_in_mode(
                    unsafe { kCFRunLoopDefaultMode },
                    STOP_CHECK_INTERVAL_SECS,
                    false,
                );
            }
            CGEvent::tap_enable(&port, false);
            // SAFETY: The run loop has exited, so the callback will not be called again.
            drop(unsafe { Box::from_raw(context) });
        })
        .map_err(|e| PlatformError::Other(e.to_string()))?;
    let handle = ready_rx
        .recv()
        .map_err(|_| PlatformError::Other("key tap thread exited".into()))??;
    Ok(ListenGuard::new(handle))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presses_matter_only_near_modifier_use() {
        let t0 = Instant::now();
        let ms = Duration::from_millis;
        // Plain typing, nothing pressed since launch or long ago.
        assert!(!press_matters(Modifiers::NONE, None, t0));
        assert!(!press_matters(Modifiers::NONE, Some(t0), t0 + ms(1500)));
        // A modifier is held: an ordinary shortcut (⌘C) must cancel a hold.
        assert!(press_matters(Modifiers::META, Some(t0), t0 + ms(5000)));
        // Between the taps of a double-tap: typing must cancel it.
        assert!(press_matters(Modifiers::NONE, Some(t0), t0 + ms(600)));
    }
}
