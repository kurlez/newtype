#![cfg(windows)]

//! Windows low-level keyboard hook on its own message pump.
//!
//! `WH_KEYBOARD_LL` is removed without a log line when the installing thread
//! stops pumping, or when the hook procedure blocks, for longer than
//! `LowLevelHooksTimeout`. This thread only runs `GetMessage`. The procedure
//! posts the key and returns. A timer on the same thread reinstalls the hook
//! when a key is physically down and no callback has arrived.

use crate::pipeline::hotkey::{self, HookWatch};

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, SyncSender, TrySendError};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use windows_sys::Win32::Foundation::{GetLastError, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, PeekMessageW, SetTimer, SetWindowsHookExW,
    TranslateMessage, UnhookWindowsHookEx, HHOOK, KBDLLHOOKSTRUCT, MSG, PM_NOREMOVE,
    WH_KEYBOARD_LL,
};

const WM_KEYDOWN: usize = 0x0100;
const WM_KEYUP: usize = 0x0101;
const WM_SYSKEYDOWN: usize = 0x0104;
const WM_SYSKEYUP: usize = 0x0105;
const WM_TIMER: u32 = 0x0113;
const LLKHF_UP: u32 = 0x80;
const WATCH_VKS: [i32; 8] = [0xA5, 0xA3, 0xA1, 0xA2, 0xA4, 0x14, 0x1B, 0x70];

static TX: OnceLock<SyncSender<HookEvent>> = OnceLock::new();
static LAST_CALLBACK_MS: AtomicU64 = AtomicU64::new(0);
static DROPPED: AtomicU32 = AtomicU32::new(0);
static SWALLOW: [AtomicU32; 8] = [
    AtomicU32::new(0),
    AtomicU32::new(0),
    AtomicU32::new(0),
    AtomicU32::new(0),
    AtomicU32::new(0),
    AtomicU32::new(0),
    AtomicU32::new(0),
    AtomicU32::new(0),
];

#[derive(Clone, Copy)]
struct HookEvent {
    vk: u32,
    scan: u32,
    flags: u32,
    up: bool,
}

/// Keys in the saved chords are swallowed without taking a lock in the hook.
pub fn set_swallowed_vks(vks: &[u32]) {
    let mut words = [0u32; 8];
    for vk in vks {
        let vk = *vk;
        if vk >= 256 {
            continue;
        }
        words[(vk / 32) as usize] |= 1u32 << (vk % 32);
    }
    for (slot, word) in SWALLOW.iter().zip(words) {
        slot.store(word, Ordering::Relaxed);
    }
}

fn vk_is_swallowed(vk: u32) -> bool {
    if vk >= 256 {
        return false;
    }
    let word = SWALLOW[(vk / 32) as usize].load(Ordering::Relaxed);
    word & (1u32 << (vk % 32)) != 0
}

pub fn run() -> Result<(), String> {
    let (tx, rx) = sync_channel::<HookEvent>(256);
    TX.set(tx).map_err(|_| "hotkey hook already started".to_string())?;
    std::thread::Builder::new()
        .name("newtype-hook".into())
        .spawn(hook_thread)
        .map_err(|err| format!("could not start the hotkey hook thread: {err}"))?;
    while let Ok(event) = rx.recv() {
        deliver(event);
    }
    crate::diag::record("hotkey hook queue closed");
    Ok(())
}

fn deliver(event: HookEvent) {
    if hotkey::is_altgr_synthetic_ctrl(event.vk, event.scan, event.flags) {
        return;
    }
    let Some(edge) = hotkey::edge_from_windows(event.vk, event.scan, event.flags, event.up) else {
        return;
    };
    let _ = crate::key_feed::push(edge, false);
}

fn hook_thread() {
    unsafe {
        let mut ready = MSG::default();
        PeekMessageW(&mut ready, std::ptr::null_mut(), 0, 0, PM_NOREMOVE);
        let mut hook = match install_hook() {
            Ok(hook) => hook,
            Err(err) => {
                crate::diag::record(&format!("hotkey hook stopped: {err}"));
                return;
            }
        };
        // Clear the sticky "pressed since last query" bit before watching.
        for vk in WATCH_VKS {
            let _ = GetAsyncKeyState(vk);
        }
        if SetTimer(std::ptr::null_mut(), 1, 250, None) == 0 {
            crate::diag::record("hotkey hook watchdog timer failed");
        }
        let mut watch = HookWatch::default();
        let mut last_reinstall_ms = 0u64;
        loop {
            let mut message = MSG::default();
            let status = GetMessageW(&mut message, std::ptr::null_mut(), 0, 0);
            if status == 0 {
                crate::diag::record("hotkey hook loop ended");
                break;
            }
            if status == -1 {
                crate::diag::record("hotkey hook GetMessage failed");
                continue;
            }
            if message.message == WM_TIMER {
                let dropped = DROPPED.swap(0, Ordering::Relaxed);
                if dropped > 0 {
                    crate::diag::record(&format!("hotkey hook queue dropped {dropped}"));
                }
                let now = now_ms();
                if watch.observe(LAST_CALLBACK_MS.load(Ordering::Relaxed), now, key_activity())
                    && now.saturating_sub(last_reinstall_ms) > 1_000
                {
                    crate::diag::record("hotkey hook dead; reinstalling");
                    if !hook.is_null() {
                        UnhookWindowsHookEx(hook);
                    }
                    match install_hook() {
                        Ok(next) => {
                            hook = next;
                            last_reinstall_ms = now;
                            watch = HookWatch::default();
                        }
                        Err(err) => {
                            hook = std::ptr::null_mut();
                            crate::diag::record(&format!("hotkey hook reinstall failed: {err}"));
                        }
                    }
                }
                continue;
            }
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        if !hook.is_null() {
            UnhookWindowsHookEx(hook);
        }
    }
}

fn install_hook() -> Result<HHOOK, String> {
    unsafe {
        let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook_proc), std::ptr::null_mut(), 0);
        if hook.is_null() {
            let code = GetLastError();
            return Err(format!("SetWindowsHookExW failed ({code})"));
        }
        crate::diag::record("Windows hotkey hook installed");
        Ok(hook)
    }
}

fn key_activity() -> bool {
    unsafe {
        WATCH_VKS.iter().any(|vk| {
            let state = GetAsyncKeyState(*vk);
            state < 0 || (state & 1) != 0
        })
    }
}

unsafe extern "system" fn hook_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        if let Some(event) = raw_event(wparam, lparam) {
            LAST_CALLBACK_MS.store(now_ms(), Ordering::Relaxed);
            if let Some(tx) = TX.get() {
                if let Err(TrySendError::Full(_)) = tx.try_send(event) {
                    DROPPED.fetch_add(1, Ordering::Relaxed);
                }
            }
            if fast_swallow(event) {
                return 1;
            }
        }
    }
    CallNextHookEx(std::ptr::null_mut(), code, wparam, lparam)
}

fn raw_event(wparam: WPARAM, lparam: LPARAM) -> Option<HookEvent> {
    let up = match wparam {
        WM_KEYUP | WM_SYSKEYUP => true,
        WM_KEYDOWN | WM_SYSKEYDOWN => false,
        _ => return None,
    };
    let info = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
    Some(HookEvent {
        vk: info.vkCode,
        scan: info.scanCode,
        flags: info.flags,
        up: up || info.flags & LLKHF_UP != 0,
    })
}

fn fast_swallow(event: HookEvent) -> bool {
    if hotkey::is_altgr_synthetic_ctrl(event.vk, event.scan, event.flags) {
        return hotkey::os_capture() || hotkey::altgr_bound();
    }
    hotkey::os_capture() || vk_is_swallowed(event.vk)
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}
