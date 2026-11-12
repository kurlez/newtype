#![cfg(windows)]

//! Windows low-level keyboard hook with a message pump that stays running.
//!
//! `rdev::listen` returns after a single `GetMessage`, which drops the hook
//! after the first message on the thread. This loop keeps the hook alive.

use crate::pipeline::hotkey;

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

pub fn run() -> Result<(), String> {
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
        if let Some(raw) = raw_key(wparam, lparam) {
            if should_swallow(raw) {
                return 1;
            }
        }
    }
    CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
}

struct RawKey {
    vk: u32,
    scan: u32,
    flags: u32,
    up: bool,
}

fn raw_key(wparam: WPARAM, lparam: LPARAM) -> Option<RawKey> {
    let up = match wparam {
        WM_KEYUP | WM_SYSKEYUP => true,
        WM_KEYDOWN | WM_SYSKEYDOWN => false,
        _ => return None,
    };
    let info = unsafe { *(lparam as *const KBDLLHOOKSTRUCT) };
    Some(RawKey {
        vk: info.vkCode,
        scan: info.scanCode,
        flags: info.flags,
        up: up || info.flags & LLKHF_UP != 0,
    })
}

fn should_swallow(raw: RawKey) -> bool {
    let capturing = hotkey::os_capture();
    let altgr = hotkey::is_altgr_synthetic_ctrl(raw.vk, raw.scan, raw.flags);
    if capturing {
        let dir = if raw.up { "up" } else { "down" };
        if altgr {
            crate::diag::record(&format!("capture key vk={} {dir} ignored=altgr", raw.vk));
        } else {
            crate::diag::record(&format!("capture key vk={} {dir}", raw.vk));
        }
    }
    if altgr {
        return capturing || hotkey::altgr_bound();
    }
    let Some(edge) = hotkey::edge_from_windows(raw.vk, raw.scan, raw.flags, raw.up) else {
        return capturing;
    };
    let suppress = crate::key_feed::push(edge, false);
    suppress || capturing
}

// Silence unused import if the compiler treats HHOOK as unused through the cast.
#[allow(dead_code)]
fn _hook_type(hook: HHOOK) -> HHOOK {
    hook
}
