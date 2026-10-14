//! One-off experiment: PopUp panel + event listening + synthesizing a key press after the panel closes.
//! Press Enter in the input box: closes the panel, then 300ms later synthesizes a press of the letter k (keycode 40).
use std::ffi::c_void;
use std::ptr::NonNull;
use std::time::{Duration, Instant};

use gpui_kit::component::{
    input::{Input, InputEvent, InputState},
    *,
};
use gpui_kit::*;
use objc2_core_foundation::{kCFRunLoopCommonModes, CFMachPort, CFRunLoop};
use objc2_core_graphics::{
    CGEvent, CGEventMask, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
    CGEventTapProxy, CGEventType,
};

struct Probe {
    input: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
}

impl Probe {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Type here; Enter = close the panel and simulate pressing k")
        });
        let subscription = cx.subscribe_in(&input, window, |_, _, ev: &InputEvent, window, _| {
            if let InputEvent::PressEnter { .. } = ev {
                window.remove_window();
                std::thread::spawn(|| {
                    std::thread::sleep(Duration::from_millis(300));
                    post_key(40);
                    println!("simulated pressing k");
                });
            }
        });
        Self { input, _subscriptions: vec![subscription] }
    }
}

impl Render for Probe {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        v_flex().p_4().size_full().child(Input::new(&self.input))
    }
}

fn post_key(code: u16) {
    for down in [true, false] {
        let event = CGEvent::new_keyboard_event(None, code, down).expect("keyboard event");
        CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&event));
    }
}

unsafe extern "C-unwind" fn on_event(
    _proxy: CGEventTapProxy,
    ty: CGEventType,
    event: NonNull<CGEvent>,
    _info: *mut c_void,
) -> *mut CGEvent {
    let flags = CGEvent::flags(Some(unsafe { event.as_ref() }));
    println!("[tap] {:?} type={} flags={:#010x}", Instant::now(), ty.0, flags.0);
    event.as_ptr()
}

fn run_tap() {
    let mask: CGEventMask = (1 << CGEventType::FlagsChanged.0) | (1 << CGEventType::KeyDown.0);
    let port = unsafe {
        CGEvent::tap_create(
            CGEventTapLocation::SessionEventTap,
            CGEventTapPlacement::HeadInsertEventTap,
            CGEventTapOptions::ListenOnly,
            mask,
            Some(on_event),
            std::ptr::null_mut(),
        )
    }
    .expect("tap_create failed: check the Accessibility permission");
    let source = CFMachPort::new_run_loop_source(None, Some(&port), 0).expect("run loop source");
    CFRunLoop::current()
        .expect("run loop")
        .add_source(Some(&source), unsafe { kCFRunLoopCommonModes });
    CGEvent::tap_enable(&port, true);
    CFRunLoop::run();
}

fn main() {
    std::thread::spawn(run_tap);
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(|cx| {
            gpui_kit::init(cx);
            cx.set_activation_policy(ActivationPolicy::Accessory);
            let options = WindowOptions {
                kind: WindowKind::PopUp,
                titlebar: None,
                focus: true,
                window_bounds: Some(WindowBounds::centered(size(px(520.), px(90.)), cx)),
                ..Default::default()
            };
            gpui_kit::open_window(options, cx, |window, cx| cx.new(|cx| Probe::new(window, cx)))
                .expect("open window");
        });
}
