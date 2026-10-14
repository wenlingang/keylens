//! 一次性实验：读菜单树并计时、打印修饰键原始编码、执行 AXPress、监听按键事件。
//! 用法：
//!   ax-probe trust
//!   ax-probe menu <pid>              打印带快捷键的菜单项和耗时
//!   ax-probe press <pid> <标题>...    按菜单路径执行 AXPress，例如 press 123 格式 字体 显示字体
//!   ax-probe tap                     打印 flagsChanged / keyDown / mouseDown 事件，Ctrl+C 退出
use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::OnceLock;
use std::time::Instant;

use objc2_application_services::{AXError, AXIsProcessTrusted, AXUIElement};
use objc2_core_foundation::{
    kCFRunLoopCommonModes, CFArray, CFBoolean, CFMachPort, CFNumber, CFRetained, CFRunLoop,
    CFString, CFType,
};
use objc2_core_graphics::{
    CGEvent, CGEventMask, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement,
    CGEventTapProxy, CGEventType,
};

static START: OnceLock<Instant> = OnceLock::new();

fn attr(el: &AXUIElement, name: &str) -> Option<CFRetained<CFType>> {
    let name = CFString::from_str(name);
    let mut value: *const CFType = std::ptr::null();
    let err = unsafe { el.copy_attribute_value(&name, NonNull::from(&mut value)) };
    if err != AXError::Success {
        return None;
    }
    NonNull::new(value as *mut CFType).map(|p| unsafe { CFRetained::from_raw(p) })
}

fn string_attr(el: &AXUIElement, name: &str) -> Option<String> {
    attr(el, name)?.downcast::<CFString>().ok().map(|s| s.to_string())
}

fn number_attr(el: &AXUIElement, name: &str) -> Option<i64> {
    attr(el, name)?.downcast::<CFNumber>().ok()?.as_i64()
}

fn bool_attr(el: &AXUIElement, name: &str) -> Option<bool> {
    attr(el, name)?.downcast::<CFBoolean>().ok().map(|b| b.as_bool())
}

fn children(el: &AXUIElement) -> Vec<CFRetained<AXUIElement>> {
    let Some(value) = attr(el, "AXChildren") else { return vec![] };
    let Ok(array) = value.downcast::<CFArray>() else { return vec![] };
    unsafe { array.cast_unchecked::<AXUIElement>() }.to_vec()
}

fn menu_bar(pid: i32) -> CFRetained<AXUIElement> {
    let app = unsafe { AXUIElement::new_application(pid) };
    unsafe { app.set_messaging_timeout(1.0) };
    attr(&app, "AXMenuBar")
        .and_then(|v| v.downcast::<AXUIElement>().ok())
        .expect("没有拿到 AXMenuBar：检查 pid 和辅助功能权限")
}

fn walk(el: &AXUIElement, depth: usize, count: &mut usize, print: bool) {
    for child in children(el) {
        let role = string_attr(&child, "AXRole").unwrap_or_default();
        let title = string_attr(&child, "AXTitle").unwrap_or_default();
        if role == "AXMenuItem" {
            *count += 1;
            let ch = string_attr(&child, "AXMenuItemCmdChar").filter(|c| !c.is_empty());
            let vk = number_attr(&child, "AXMenuItemCmdVirtualKey");
            if print && (ch.is_some() || vk.is_some()) {
                println!(
                    "{}{title} | char={ch:?} vkey={vk:?} mods={:?} enabled={:?}",
                    "  ".repeat(depth),
                    number_attr(&child, "AXMenuItemCmdModifiers"),
                    bool_attr(&child, "AXEnabled"),
                );
            }
        } else if print && role == "AXMenuBarItem" {
            println!("== {title} ==");
        }
        walk(&child, depth + 1, count, print);
    }
}

fn find(el: &AXUIElement, titles: &[String]) -> Option<CFRetained<AXUIElement>> {
    for child in children(el) {
        let role = string_attr(&child, "AXRole").unwrap_or_default();
        if role == "AXMenu" {
            if let Some(found) = find(&child, titles) {
                return Some(found);
            }
            continue;
        }
        if string_attr(&child, "AXTitle").as_deref() == Some(titles[0].as_str()) {
            if titles.len() == 1 {
                return Some(child);
            }
            if let Some(found) = find(&child, &titles[1..]) {
                return Some(found);
            }
        }
    }
    None
}

unsafe extern "C-unwind" fn on_event(
    _proxy: CGEventTapProxy,
    ty: CGEventType,
    event: NonNull<CGEvent>,
    _info: *mut c_void,
) -> *mut CGEvent {
    let flags = CGEvent::flags(Some(unsafe { event.as_ref() }));
    let t = START.get().map(|s| s.elapsed().as_secs_f64()).unwrap_or_default();
    println!("{t:>8.3}s type={:>3} flags={:#010x}", ty.0, flags.0);
    event.as_ptr()
}

fn run_tap() {
    START.get_or_init(Instant::now);
    let mask: CGEventMask = (1 << CGEventType::FlagsChanged.0)
        | (1 << CGEventType::KeyDown.0)
        | (1 << CGEventType::LeftMouseDown.0);
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
    .expect("tap_create 失败：检查辅助功能权限");
    let source = CFMachPort::new_run_loop_source(None, Some(&port), 0).expect("run loop source");
    CFRunLoop::current()
        .expect("run loop")
        .add_source(Some(&source), unsafe { kCFRunLoopCommonModes });
    CGEvent::tap_enable(&port, true);
    println!("监听中，Ctrl+C 退出");
    CFRunLoop::run();
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("trust") => println!("AXIsProcessTrusted = {}", unsafe { AXIsProcessTrusted() }),
        Some("menu") => {
            let pid: i32 = args[1].parse().expect("pid");
            for round in ["冷启动", "第二次"] {
                let started = Instant::now();
                let bar = menu_bar(pid);
                let mut count = 0;
                walk(&bar, 0, &mut count, round == "冷启动");
                println!("[{round}] {count} 个菜单项，耗时 {:?}", started.elapsed());
            }
        }
        Some("press") => {
            let pid: i32 = args[1].parse().expect("pid");
            let titles = args[2..].to_vec();
            let item = find(&menu_bar(pid), &titles).expect("没找到菜单项");
            let started = Instant::now();
            let err = unsafe { item.perform_action(&CFString::from_str("AXPress")) };
            println!("AXPress -> {:?}，耗时 {:?}", err, started.elapsed());
        }
        Some("windows") => {
            let pid: i32 = args[1].parse().expect("pid");
            let app = unsafe { AXUIElement::new_application(pid) };
            let windows = attr(&app, "AXWindows")
                .and_then(|v| v.downcast::<CFArray>().ok())
                .map(|a| unsafe { a.cast_unchecked::<AXUIElement>() }.to_vec())
                .unwrap_or_default();
            for w in windows {
                println!("{:?} | {:?}", string_attr(&w, "AXTitle"), string_attr(&w, "AXSubrole"));
            }
        }
        Some("tap") => run_tap(),
        _ => eprintln!("用法：ax-probe trust | menu <pid> | press <pid> <标题>... | tap"),
    }
}
