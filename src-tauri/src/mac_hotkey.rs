//! Keyboard event tap for the hotkey thread.
//!
//! When Accessibility is trusted the tap is active and swallows the non-modifier
//! key of a fully held chord. Otherwise it listens only, and the chord key still
//! reaches the focused app. Key identity comes from the hardware keycode and
//! modifier flags. This thread must not call TIS, TSM, `UCKeyTranslate`, AppKit,
//! or AVFoundation. macOS 26 asserts that text-input calls run on the main queue.

use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicU64, Ordering};
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
static RUN_LOOP: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());
static EVENTS: OnceLock<SyncSender<TapMsg>> = OnceLock::new();
static ACTIVE: AtomicBool = AtomicBool::new(false);
static RULE_COUNT: AtomicU32 = AtomicU32::new(0);
static RULES: [AtomicU64; 8] = [
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
    AtomicU64::new(0),
];
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

/// Whether the tap can remove events, or can only observe them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TapMode {
    Active,
    ListenOnly,
}

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
    fn CFRunLoopStop(run_loop: *mut std::ffi::c_void);
    fn CFRelease(item: *const std::ffi::c_void);
}

fn accessibility_trusted() -> bool {
    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> u8;
    }
    unsafe { AXIsProcessTrusted() != 0 }
}

fn create_tap(
    options: u32,
    mask: u64,
) -> *mut std::ffi::c_void {
    unsafe {
        CGEventTapCreate(
            0,
            0,
            options,
            mask,
            tap_callback,
            std::ptr::null_mut(),
        )
    }
}

pub fn publish_chords(chords: &[hotkey::Chord]) {
    let rules = hotkey::mac_swallow_rules(chords);
    for (index, rule) in rules.iter().take(RULES.len()).enumerate() {
        let packed = (u64::from(rule.mods) << 16) | u64::from(rule.keycode);
        RULES[index].store(packed, Ordering::Relaxed);
    }
    RULE_COUNT.store(rules.len().min(RULES.len()) as u32, Ordering::Release);
}

/// Keep trying until the tap exists, then run it. A timeout or a user
/// disable is turned back on inside the callback. This function does not
/// return while the process is running.
pub fn listen(mut report: impl FnMut(Option<TapMode>)) {
    start_side_threads();
    loop {
        match run_tap(&mut report) {
            Ok(()) => {
                crate::diag::record("hotkey event tap stopped");
                report(None);
            }
            Err(err) => {
                crate::diag::record(&format!("hotkey event tap failed: {err}"));
                report(None);
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
                crate::key_feed::resync(hotkey::mac_modifiers_physically_up(flags));
                crate::key_feed::tick(now_ms());
                if !ACTIVE.load(Ordering::Relaxed) && accessibility_trusted() {
                    let run_loop = RUN_LOOP.load(Ordering::Acquire);
                    if !run_loop.is_null() {
                        unsafe { CFRunLoopStop(run_loop) };
                    }
                }
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

fn run_tap(report: &mut impl FnMut(Option<TapMode>)) -> Result<(), &'static str> {
    let mask = (1u64 << KEY_DOWN) | (1u64 << KEY_UP) | (1u64 << FLAGS_CHANGED);
    let mut listen_only = true;
    let mut tap = std::ptr::null_mut();
    if accessibility_trusted() {
        tap = create_tap(0, mask);
        if !tap.is_null() {
            listen_only = false;
        }
    }
    if tap.is_null() {
        tap = create_tap(1, mask);
        listen_only = true;
    }
    if tap.is_null() {
        ACTIVE.store(false, Ordering::Relaxed);
        return Err("could not create the keyboard event tap");
    }
    ACTIVE.store(!listen_only, Ordering::Relaxed);
    TAP.store(tap, Ordering::Release);
    let source = unsafe { CFMachPortCreateRunLoopSource(std::ptr::null_mut(), tap, 0) };
    if source.is_null() {
        TAP.store(std::ptr::null_mut(), Ordering::Release);
        ACTIVE.store(false, Ordering::Relaxed);
        unsafe { CFRelease(tap) };
        return Err("could not create the keyboard run loop source");
    }
    unsafe {
        let run_loop = CFRunLoopGetCurrent();
        RUN_LOOP.store(run_loop, Ordering::Release);
        CFRunLoopAddSource(
            run_loop,
            source,
            kCFRunLoopCommonModes as *const std::ffi::c_void,
        );
        CGEventTapEnable(tap, 1);
    }
    if listen_only {
        crate::diag::record(
            "hotkey event tap is listen-only; Accessibility is required to swallow the chord key",
        );
        report(Some(TapMode::ListenOnly));
    } else {
        crate::diag::record("hotkey event tap is active");
        report(Some(TapMode::Active));
    }
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
    if swallow_raw(event_type, event) {
        return std::ptr::null_mut();
    }
    event
}

fn mark_swallowed(code: u16) {
    if code >= 256 {
        return;
    }
    let slot = &SWALLOW[(code / 32) as usize];
    slot.fetch_or(1u32 << (code % 32), Ordering::Relaxed);
}

fn take_swallowed(code: u16) -> bool {
    if code >= 256 {
        return false;
    }
    let slot = &SWALLOW[(code / 32) as usize];
    let bit = 1u32 << (code % 32);
    let previous = slot.fetch_and(!bit, Ordering::Relaxed);
    previous & bit != 0
}

fn rule_matches(mods: u32, keycode: u16) -> bool {
    let count = RULE_COUNT.load(Ordering::Acquire) as usize;
    for rule in RULES.iter().take(count) {
        let packed = rule.load(Ordering::Relaxed);
        let rule_code = packed as u16;
        let rule_mods = (packed >> 16) as u32;
        if rule_code == keycode && rule_mods == mods {
            return true;
        }
    }
    false
}

fn swallow_raw(event_type: u32, event: *mut std::ffi::c_void) -> bool {
    if !ACTIVE.load(Ordering::Relaxed) || event.is_null() || event_type == FLAGS_CHANGED {
        return false;
    }
    let raw = unsafe { CGEventGetIntegerValueField(event, KEYCODE_FIELD) };
    let Ok(code) = u16::try_from(raw) else {
        return false;
    };
    let key = hotkey::key_from_mac_keycode(code);
    if hotkey::is_modifier(key) {
        return false;
    }
    if event_type == KEY_UP {
        return take_swallowed(code);
    }
    if event_type != KEY_DOWN {
        return false;
    }
    let swallow = hotkey::os_capture()
        || (hotkey::session_keys() && matches!(key, hotkey::KeyId::Enter | hotkey::KeyId::Escape))
        || rule_matches(hotkey::mac_modifier_mask(unsafe { CGEventGetFlags(event) }), code);
    if swallow {
        mark_swallowed(code);
    }
    swallow
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
