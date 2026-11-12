#![cfg(windows)]

//! Windows low-level keyboard hook with a message pump that stays running.
//!
//! `rdev::listen` returns after a single `GetMessage`, which drops the hook
//! after the first message on the thread. This loop keeps the hook alive.

use crate::pipeline::hotkey::{self, KeyEdge, KeyId};

use std::sync::Mutex;
use windows_sys::Win32::Foundation::{GetLastError, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, SetWindowsHookExW, TranslateMessage,
    UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL,
};

const WM_KEYDOWN: usize = 0x0100;
const WM_KEYUP: usize = 0x0101;
const WM_SYSKEYDOWN: usize = 0x0104;
const WM_SYSKEYUP: usize = 0x0105;
const LLKHF_UP: u32 = 0x80;

static HOOK: Mutex<Option<isize>> = Mutex::new(None);
static ON_EDGE: Mutex<Option<Box<dyn FnMut(KeyEdge) + Send>>> = Mutex::new(None);

pub fn run(on_edge: impl FnMut(KeyEdge) + Send + 'static) -> Result<(), String> {
    *ON_EDGE.lock().expect("hotkey callback") = Some(Box::new(on_edge));
    unsafe {
        let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), std::ptr::null_mut(), 0);
        if hook.is_null() {
            let code = GetLastError();
            return Err(format!("SetWindowsHookExW failed ({code})"));
        }
        *HOOK.lock().expect("hook") = Some(hook as isize);
        crate::diag::record("Windows hotkey hook installed");
        loop {
            let mut message = MSG::default();
            let status = GetMessageW(&mut message, std::ptr::null_mut(), 0, 0);
            if status == 0 || status == -1 {
                break;
            }
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        UnhookWindowsHookEx(hook);
    }
    Ok(())
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        if let Some(edge) = edge_from(wparam, lparam) {
            if let Some(callback) = ON_EDGE.lock().expect("hotkey callback").as_mut() {
                callback(edge);
            }
        }
    }
    CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
}

fn edge_from(wparam: WPARAM, lparam: LPARAM) -> Option<KeyEdge> {
    let up = match wparam {
        WM_KEYUP | WM_SYSKEYUP => true,
        WM_KEYDOWN | WM_SYSKEYDOWN => false,
        _ => return None,
    };
    let info = unsafe { *(lparam as *const KBDLLHOOKSTRUCT) };
    let released = up || info.flags & LLKHF_UP != 0;
    let key = hotkey::key_from_windows_vk(info.vkCode);
    if key == KeyId::Other && info.vkCode == 0 {
        return None;
    }
    Some(if released {
        KeyEdge::Up(key)
    } else {
        KeyEdge::Down(key)
    })
}

// Silence unused import if the compiler treats HHOOK as unused through the cast.
#[allow(dead_code)]
fn _hook_type(hook: HHOOK) -> HHOOK {
    hook
}
