//! The foreground window: who it is, giving focus back to it, and sending it a shortcut.

use std::time::{Duration, Instant};

use keylens_core::{Key, Modifiers, Shortcut};
use windows::Win32::Foundation::{CloseHandle, HWND, RECT};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow};
use windows::Win32::System::Threading::{
    AttachThreadInput, GetCurrentThreadId, OpenProcess, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::PWSTR;

use crate::log;

/// A window handle as an integer, so it can cross threads.
pub type Hwnd = isize;

fn hwnd(h: Hwnd) -> HWND {
    HWND(h as *mut _)
}

pub fn foreground() -> Hwnd {
    unsafe { GetForegroundWindow() }.0 as Hwnd
}

/// The foreground window, its process id and its executable's file name (e.g. "Code.exe").
pub fn current() -> Option<(Hwnd, u32, String)> {
    unsafe {
        let window = GetForegroundWindow();
        if window.0.is_null() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(window, Some(&mut pid));
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len);
        let _ = CloseHandle(process);
        ok.ok()?;
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        let exe = path.rsplit('\\').next().unwrap_or(&path).to_string();
        Some((window.0 as Hwnd, pid, exe))
    }
}

/// Brings `target` to the foreground and waits (up to 500ms) until it is. Tries a plain
/// SetForegroundWindow first, then again with our thread attached to the foreground thread's input.
pub fn restore(target: Hwnd) -> bool {
    let target_hwnd = hwnd(target);
    unsafe {
        if SetForegroundWindow(target_hwnd).as_bool() && wait_for(target) {
            log("restore: plain SetForegroundWindow ok");
            return true;
        }
        let fg = GetForegroundWindow();
        let fg_thread = GetWindowThreadProcessId(fg, None);
        let me = GetCurrentThreadId();
        let attached = fg_thread != me && AttachThreadInput(me, fg_thread, true).as_bool();
        let _ = BringWindowToTop(target_hwnd);
        let set = SetForegroundWindow(target_hwnd).as_bool();
        if attached {
            let _ = AttachThreadInput(me, fg_thread, false);
        }
        let ok = wait_for(target);
        log(&format!("restore: AttachThreadInput attached={attached} set={set} ok={ok}"));
        ok
    }
}

fn wait_for(target: Hwnd) -> bool {
    let start = Instant::now();
    while start.elapsed() < Duration::from_millis(500) {
        if foreground() == target {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

/// Makes our own window topmost, centred on the monitor of `near`, and tries to give it focus.
/// Returns whether it ended up in the foreground.
pub fn present(ours: Hwnd, near: Hwnd) -> bool {
    unsafe {
        let ours_hwnd = hwnd(ours);
        let monitor = MonitorFromWindow(hwnd(near), MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        let mut rect = RECT::default();
        if GetMonitorInfoW(monitor, &mut info).as_bool() && GetWindowRect(ours_hwnd, &mut rect).is_ok() {
            let work = info.rcWork;
            let (w, h) = (rect.right - rect.left, rect.bottom - rect.top);
            let x = work.left + (work.right - work.left - w) / 2;
            let y = work.top + (work.bottom - work.top - h) / 3;
            let _ = SetWindowPos(ours_hwnd, Some(HWND_TOPMOST), x, y, 0, 0, SWP_NOSIZE);
        }
        if foreground() == ours {
            log("present: gpui already gave us focus");
            return true;
        }
        log("present: gpui did not get focus, forcing");
        restore(ours)
    }
}

fn vk(key: Key) -> (VIRTUAL_KEY, bool) {
    let plain = |v: VIRTUAL_KEY| (v, false);
    let extended = |v: VIRTUAL_KEY| (v, true);
    match key {
        Key::Char(c) => plain(match c.to_ascii_uppercase() {
            c @ ('A'..='Z' | '0'..='9') => VIRTUAL_KEY(c as u16),
            '`' => VK_OEM_3,
            '-' => VK_OEM_MINUS,
            '=' => VK_OEM_PLUS,
            '[' => VK_OEM_4,
            ']' => VK_OEM_6,
            '\\' => VK_OEM_5,
            ';' => VK_OEM_1,
            '\'' => VK_OEM_7,
            ',' => VK_OEM_COMMA,
            '.' => VK_OEM_PERIOD,
            '/' => VK_OEM_2,
            _ => VK_SPACE,
        }),
        Key::F(n) => plain(VIRTUAL_KEY(VK_F1.0 + n as u16 - 1)),
        Key::Up => extended(VK_UP),
        Key::Down => extended(VK_DOWN),
        Key::Left => extended(VK_LEFT),
        Key::Right => extended(VK_RIGHT),
        Key::Enter => plain(VK_RETURN),
        Key::Tab => plain(VK_TAB),
        Key::Space => plain(VK_SPACE),
        Key::Backspace => plain(VK_BACK),
        Key::Delete => extended(VK_DELETE),
        Key::Escape => plain(VK_ESCAPE),
        Key::Home => extended(VK_HOME),
        Key::End => extended(VK_END),
        Key::PageUp => extended(VK_PRIOR),
        Key::PageDown => extended(VK_NEXT),
    }
}

fn input(key: VIRTUAL_KEY, extended: bool, up: bool) -> INPUT {
    let mut flags = KEYBD_EVENT_FLAGS(0);
    if extended {
        flags |= KEYEVENTF_EXTENDEDKEY;
    }
    if up {
        flags |= KEYEVENTF_KEYUP;
    }
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT { wVk: key, wScan: 0, dwFlags: flags, time: 0, dwExtraInfo: 0 },
        },
    }
}

/// Sends each chord of `shortcut` to the foreground window: modifiers down, key down/up, modifiers up.
pub fn send(shortcut: &Shortcut) -> bool {
    let mods = [
        (Modifiers::CTRL, VK_CONTROL),
        (Modifiers::SHIFT, VK_SHIFT),
        (Modifiers::ALT, VK_MENU),
        (Modifiers::META, VK_LWIN),
    ];
    let mut all_sent = true;
    for chord in shortcut.chords() {
        let held: Vec<VIRTUAL_KEY> =
            mods.iter().filter(|(m, _)| chord.modifiers.contains(*m)).map(|(_, v)| *v).collect();
        let (key, extended) = vk(chord.key);
        let mut inputs: Vec<INPUT> = held.iter().map(|v| input(*v, false, false)).collect();
        inputs.push(input(key, extended, false));
        inputs.push(input(key, extended, true));
        inputs.extend(held.iter().rev().map(|v| input(*v, false, true)));
        let sent = unsafe { SendInput(&inputs, size_of::<INPUT>() as i32) };
        all_sent &= sent as usize == inputs.len();
        std::thread::sleep(Duration::from_millis(30));
    }
    all_sent
}
