//! Interactive hotkey capture for the settings CLI.
//!
//! Uses a low-level keyboard hook so Win/Ctrl/Alt/Shift combinations can be
//! recorded by pressing them, instead of typing `Win+Shift+G` as text.

use std::{
    cell::RefCell,
    sync::mpsc::{self, Sender},
    thread,
    time::Duration,
};
use windows_sys::Win32::{
    Foundation::{LPARAM, LRESULT, WPARAM},
    System::Threading::GetCurrentThreadId,
    UI::{
        Input::KeyboardAndMouse::{
            GetAsyncKeyState, VK_CONTROL, VK_ESCAPE, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
        },
        WindowsAndMessaging::{
            CallNextHookEx, DispatchMessageW, GetMessageW, PostThreadMessageW, SetWindowsHookExW,
            TranslateMessage, UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL,
            WM_KEYDOWN, WM_QUIT, WM_SYSKEYDOWN,
        },
    },
};

#[derive(Debug)]
pub enum CaptureError {
    Cancelled,
    Failed(String),
}

enum Event {
    Hotkey(String),
    Cancelled,
}

thread_local! {
    static HOOK_TX: RefCell<Option<Sender<Event>>> = const { RefCell::new(None) };
}

/// Block until the user presses a modifier+key chord, then return `Win+Shift+G` style text.
pub fn run() -> Result<String, CaptureError> {
    let (tx, rx) = mpsc::channel::<Event>();
    let (ready_tx, ready_rx) = mpsc::channel::<u32>();

    let hook_thread = thread::spawn(move || {
        let _ = ready_tx.send(unsafe { GetCurrentThreadId() });
        install_hook_and_pump(tx);
    });

    let thread_id = ready_rx
        .recv_timeout(Duration::from_secs(5))
        .map_err(|_| CaptureError::Failed("无法启动按键捕获".into()))?;

    let event = match rx.recv_timeout(Duration::from_secs(120)) {
        Ok(event) => event,
        Err(_) => {
            unsafe {
                PostThreadMessageW(thread_id, WM_QUIT, 0, 0);
            }
            let _ = hook_thread.join();
            return Err(CaptureError::Failed("等待按键超时".into()));
        }
    };

    unsafe {
        PostThreadMessageW(thread_id, WM_QUIT, 0, 0);
    }
    let _ = hook_thread.join();

    match event {
        Event::Hotkey(value) => Ok(value),
        Event::Cancelled => Err(CaptureError::Cancelled),
    }
}

fn install_hook_and_pump(tx: Sender<Event>) {
    HOOK_TX.with(|slot| {
        *slot.borrow_mut() = Some(tx);
    });

    unsafe {
        let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(low_level_proc), std::ptr::null_mut(), 0);
        if hook.is_null() {
            HOOK_TX.with(|slot| {
                if let Some(tx) = slot.borrow_mut().take() {
                    let _ = tx.send(Event::Cancelled);
                }
            });
            return;
        }

        let mut msg = std::mem::zeroed::<MSG>();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
        UnhookWindowsHookEx(hook);
    }

    HOOK_TX.with(|slot| {
        slot.borrow_mut().take();
    });
}

unsafe extern "system" fn low_level_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let kind = wparam as u32;
        if kind == WM_KEYDOWN || kind == WM_SYSKEYDOWN {
            let info = &*(lparam as *const KBDLLHOOKSTRUCT);
            if let Some(event) = interpret_key(info.vkCode) {
                HOOK_TX.with(|slot| {
                    if let Some(tx) = slot.borrow_mut().take() {
                        let _ = tx.send(event);
                        PostThreadMessageW(GetCurrentThreadId(), WM_QUIT, 0, 0);
                    }
                });
            }
        }
    }
    CallNextHookEx(std::ptr::null_mut::<HHOOK>() as HHOOK, code, wparam, lparam)
}

fn interpret_key(vk: u32) -> Option<Event> {
    if is_modifier(vk) {
        return None;
    }

    let win = key_down(VK_LWIN as u32) || key_down(VK_RWIN as u32);
    let ctrl = key_down(VK_CONTROL as u32);
    let alt = key_down(VK_MENU as u32);
    let shift = key_down(VK_SHIFT as u32);

    if vk == VK_ESCAPE as u32 && !win && !ctrl && !alt && !shift {
        return Some(Event::Cancelled);
    }

    let name = vk_name(vk)?;
    if !win && !ctrl && !alt && !shift {
        return None;
    }

    let mut parts = Vec::new();
    if win {
        parts.push("Win");
    }
    if ctrl {
        parts.push("Ctrl");
    }
    if alt {
        parts.push("Alt");
    }
    if shift {
        parts.push("Shift");
    }
    parts.push(name);
    Some(Event::Hotkey(parts.join("+")))
}

fn key_down(vk: u32) -> bool {
    unsafe { GetAsyncKeyState(vk as i32) as u16 & 0x8000 != 0 }
}

fn is_modifier(vk: u32) -> bool {
    vk == VK_SHIFT as u32
        || vk == VK_CONTROL as u32
        || vk == VK_MENU as u32
        || vk == VK_LWIN as u32
        || vk == VK_RWIN as u32
        || vk == 0xA0
        || vk == 0xA1
        || vk == 0xA2
        || vk == 0xA3
        || vk == 0xA4
        || vk == 0xA5
}

fn vk_name(vk: u32) -> Option<&'static str> {
    match vk {
        0x30 => Some("0"),
        0x31 => Some("1"),
        0x32 => Some("2"),
        0x33 => Some("3"),
        0x34 => Some("4"),
        0x35 => Some("5"),
        0x36 => Some("6"),
        0x37 => Some("7"),
        0x38 => Some("8"),
        0x39 => Some("9"),
        0x41 => Some("A"),
        0x42 => Some("B"),
        0x43 => Some("C"),
        0x44 => Some("D"),
        0x45 => Some("E"),
        0x46 => Some("F"),
        0x47 => Some("G"),
        0x48 => Some("H"),
        0x49 => Some("I"),
        0x4A => Some("J"),
        0x4B => Some("K"),
        0x4C => Some("L"),
        0x4D => Some("M"),
        0x4E => Some("N"),
        0x4F => Some("O"),
        0x50 => Some("P"),
        0x51 => Some("Q"),
        0x52 => Some("R"),
        0x53 => Some("S"),
        0x54 => Some("T"),
        0x55 => Some("U"),
        0x56 => Some("V"),
        0x57 => Some("W"),
        0x58 => Some("X"),
        0x59 => Some("Y"),
        0x5A => Some("Z"),
        0x70 => Some("F1"),
        0x71 => Some("F2"),
        0x72 => Some("F3"),
        0x73 => Some("F4"),
        0x74 => Some("F5"),
        0x75 => Some("F6"),
        0x76 => Some("F7"),
        0x77 => Some("F8"),
        0x78 => Some("F9"),
        0x79 => Some("F10"),
        0x7A => Some("F11"),
        0x7B => Some("F12"),
        0x7C => Some("F13"),
        0x7D => Some("F14"),
        0x7E => Some("F15"),
        0x7F => Some("F16"),
        0x80 => Some("F17"),
        0x81 => Some("F18"),
        0x82 => Some("F19"),
        0x83 => Some("F20"),
        0x84 => Some("F21"),
        0x85 => Some("F22"),
        0x86 => Some("F23"),
        0x87 => Some("F24"),
        _ => None,
    }
}
