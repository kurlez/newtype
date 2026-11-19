//! Listen-only keyboard tap for the hotkey thread.
//!
//! Key identity comes from the hardware keycode and modifier flags. This
//! thread must not call TIS, TSM, `UCKeyTranslate`, AppKit, or AVFoundation.
//! macOS 26 asserts that text-input calls run on the main queue.

use std::sync::atomic::{AtomicPtr, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::OnceLock;
use std::thread;
use std::time::Duration;

use core_foundation::runloop::kCFRunLoopCommonModes;

use crate::pipeline::hotkey::{self, KeyEdge};

const RETRY: Duration = hotkey::EVENT_TAP_RETRY;
const KEY_DOWN: u32 = 10;
const KEY_UP: u32 = 11;
const FLAGS_CHANGED: u32 = 12;
const KEYCODE_FIELD: u32 = 9;

static TAP: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());
static EVENTS: OnceLock<SyncSender<TapMsg>> = OnceLock::new();

enum TapMsg {
    Edge(KeyEdge),
    Reenabled,
    Tick,
}

#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventTapCreate(
        tap: u32,
        place: u32,
        options: u32,
        events_of_interest: u64,
        callback: unsafe extern "C" fn(
            *mut std::ffi::c_void,
            u32,
            *mut std::ffi::c_void,
            *mut std::ffi::c_void,
        ) -> *mut std::ffi::c_void,
        user_info: *mut std::ffi::c_void,
    ) -> *mut std::ffi::c_void;
    fn CGEventTapEnable(tap: *mut std::ffi::c_void, enable: u8);
    fn CGEventGetIntegerValueField(event: *mut std::ffi::c_void, field: u32) -> i64;
    fn CGEventGetFlags(event: *mut std::ffi::c_void) -> u64;
    fn CGEventSourceFlagsState(state: i32) -> u64;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFMachPortCreateRunLoopSource(
        allocator: *mut std::ffi::c_void,
        port: *mut std::ffi::c_void,
        order: i64,
    ) -> *mut std::ffi::c_void;
    fn CFRunLoopAddSource(
        run_loop: *mut std::ffi::c_void,
        source: *mut std::ffi::c_void,
        mode: *const std::ffi::c_void,
    );
    fn CFRunLoopGetCurrent() -> *mut std::ffi::c_void;
    fn CFRunLoopRun();
    fn CFRelease(item: *const std::ffi::c_void);
}

/// Keep trying until the tap exists, then run it. A timeout or a user
/// disable is turned back on inside the callback. This function does not
/// return while the process is running.
pub fn listen(mut report: impl FnMut(bool)) {
    start_side_threads();
    loop {
        match run_tap(&mut report) {
            Ok(()) => {
                crate::diag::record("hotkey event tap stopped");
                report(false);
            }
            Err(err) => {
                crate::diag::record(&format!("hotkey event tap failed: {err}"));
                report(false);
            }
        }
        thread::sleep(RETRY);
    }
}

fn start_side_threads() {
    if EVENTS.get().is_some() {
        return;
    }
    let (tx, rx) = sync_channel::<TapMsg>(512);
    if EVENTS.set(tx.clone()).is_err() {
        return;
    }
    thread::Builder::new()
        .name("newtype-keys".into())
        .spawn(move || dispatch(rx))
        .ok();
    thread::Builder::new()
        .name("newtype-key-sync".into())
        .spawn(move || loop {
            thread::sleep(Duration::from_millis(200));
            let _ = tx.try_send(TapMsg::Tick);
        })
        .ok();
}

fn dispatch(rx: Receiver<TapMsg>) {
    while let Ok(message) = rx.recv() {
        match message {
            TapMsg::Edge(edge) => {
                let _ = crate::key_feed::push(edge, false);
            }
            TapMsg::Reenabled => crate::diag::record("hotkey event tap re-enabled"),
            TapMsg::Tick => {
                let flags = unsafe { CGEventSourceFlagsState(1) };
                for key in hotkey::mac_modifiers_physically_up(flags) {
                    let _ = crate::key_feed::push(KeyEdge::Up(key), false);
                }
                crate::key_feed::tick(now_ms());
            }
        }
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn enqueue(message: TapMsg) {
    let Some(tx) = EVENTS.get() else {
        return;
    };
    let _ = tx.try_send(message);
}

fn run_tap(report: &mut impl FnMut(bool)) -> Result<(), &'static str> {
    let mask = (1u64 << KEY_DOWN) | (1u64 << KEY_UP) | (1u64 << FLAGS_CHANGED);
    let tap = unsafe {
        CGEventTapCreate(
            0, // HID
            0, // head insert
            1, // listen only
            mask,
            tap_callback,
            std::ptr::null_mut(),
        )
    };
    if tap.is_null() {
        return Err("could not create the keyboard event tap");
    }
    TAP.store(tap, Ordering::Release);
    let source = unsafe { CFMachPortCreateRunLoopSource(std::ptr::null_mut(), tap, 0) };
    if source.is_null() {
        TAP.store(std::ptr::null_mut(), Ordering::Release);
        unsafe { CFRelease(tap) };
        return Err("could not create the keyboard run loop source");
    }
    unsafe {
        let run_loop = CFRunLoopGetCurrent();
        CFRunLoopAddSource(
            run_loop,
            source,
            kCFRunLoopCommonModes as *const std::ffi::c_void,
        );
        CGEventTapEnable(tap, 1);
    }
    crate::diag::record("hotkey event tap is listening");
    report(true);
    // The run loop retains `source` for this thread. The pointer stays live
    // because this function does not return until the loop stops.
    let _keep_source = source;
    unsafe { CFRunLoopRun() };
    Ok(())
}

unsafe extern "C" fn tap_callback(
    _proxy: *mut std::ffi::c_void,
    event_type: u32,
    event: *mut std::ffi::c_void,
    _user: *mut std::ffi::c_void,
) -> *mut std::ffi::c_void {
    if hotkey::event_tap_needs_reenable(event_type) {
        let tap = TAP.load(Ordering::Acquire);
        if !tap.is_null() {
            unsafe { CGEventTapEnable(tap, 1) };
            enqueue(TapMsg::Reenabled);
        }
        return event;
    }
    if let Some(edge) = edge_from_raw(event_type, event) {
        enqueue(TapMsg::Edge(edge));
    }
    event
}

fn edge_from_raw(event_type: u32, event: *mut std::ffi::c_void) -> Option<KeyEdge> {
    if event.is_null() {
        return None;
    }
    let raw = unsafe { CGEventGetIntegerValueField(event, KEYCODE_FIELD) };
    let code = u16::try_from(raw).ok()?;
    match event_type {
        KEY_DOWN => Some(KeyEdge::Down(hotkey::key_from_mac_keycode(code))),
        KEY_UP => Some(KeyEdge::Up(hotkey::key_from_mac_keycode(code))),
        FLAGS_CHANGED => {
            let flags = unsafe { CGEventGetFlags(event) };
            hotkey::mac_flags_changed(code, flags)
        }
        _ => None,
    }
}
