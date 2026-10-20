//! Low-level keyboard and mouse hooks on a dedicated thread, turned into keylens-core key events.
//! Injected events (including our own SendInput) are ignored so executing Ctrl+X never re-triggers.

use std::sync::OnceLock;
use std::sync::mpsc::Sender;

use keylens_core::{KeyEvent, Modifiers};
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::log;

struct State {
    tx: std::sync::Mutex<Sender<KeyEvent>>,
    /// Which of CTRL, SHIFT, ALT, META are held (Modifiers has no way to remove a bit).
    held: std::sync::Mutex<[bool; 4]>,
}

const ORDER: [Modifiers; 4] = [Modifiers::CTRL, Modifiers::SHIFT, Modifiers::ALT, Modifiers::META];

fn combine(held: &[bool; 4]) -> Modifiers {
    ORDER
        .iter()
        .zip(held)
        .filter(|(_, h)| **h)
        .fold(Modifiers::NONE, |acc, (m, _)| acc | *m)
}

static STATE: OnceLock<State> = OnceLock::new();

fn modifier_of(vk: u32) -> Option<Modifiers> {
    let vk = VIRTUAL_KEY(vk as u16);
    match vk {
        VK_CONTROL | VK_LCONTROL | VK_RCONTROL => Some(Modifiers::CTRL),
        VK_SHIFT | VK_LSHIFT | VK_RSHIFT => Some(Modifiers::SHIFT),
        VK_MENU | VK_LMENU | VK_RMENU => Some(Modifiers::ALT),
        VK_LWIN | VK_RWIN => Some(Modifiers::META),
        _ => None,
    }
}

fn send(event: KeyEvent) {
    if let Some(state) = STATE.get() {
        let _ = state.tx.lock().unwrap().send(event);
    }
}

unsafe extern "system" fn keyboard(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let kb = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
        let injected = kb.flags.0 & LLKHF_INJECTED.0 != 0;
        if !injected {
            let msg = wparam.0 as u32;
            let down = msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN;
            let up = msg == WM_KEYUP || msg == WM_SYSKEYUP;
            match modifier_of(kb.vkCode) {
                Some(m) => {
                    let state = STATE.get().unwrap();
                    let mut held = state.held.lock().unwrap();
                    let before = combine(&held);
                    let i = ORDER.iter().position(|o| *o == m).unwrap();
                    if down {
                        held[i] = true;
                    } else if up {
                        held[i] = false;
                    }
                    let next = combine(&held);
                    drop(held);
                    if next != before {
                        send(KeyEvent::ModifiersChanged(next));
                    }
                }
                None if down => send(KeyEvent::KeyDown),
                None => {}
            }
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

unsafe extern "system" fn mouse(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let ms = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };
        let injected = ms.flags & LLMHF_INJECTED != 0;
        let msg = wparam.0 as u32;
        if !injected && (msg == WM_LBUTTONDOWN || msg == WM_RBUTTONDOWN || msg == WM_MBUTTONDOWN)
        {
            send(KeyEvent::MouseDown);
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// Installs both hooks on a new thread that pumps messages for as long as the process lives.
pub fn start(tx: Sender<KeyEvent>) {
    let _ = STATE.set(State {
        tx: std::sync::Mutex::new(tx),
        held: std::sync::Mutex::new([false; 4]),
    });
    std::thread::Builder::new()
        .name("probe-hook".into())
        .spawn(|| unsafe {
            let kb = SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard), None, 0);
            let ms = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse), None, 0);
            log(&format!("hooks installed: keyboard={:?} mouse={:?}", kb.is_ok(), ms.is_ok()));
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            log("hook thread exited");
        })
        .expect("spawn hook thread");
}
