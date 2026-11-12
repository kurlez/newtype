//! Wake the probe when the OS reports a network change.
//!
//! The periodic probe still runs. This only asks it to take a sample sooner.
//! macOS watches configd's network-change notification. Windows blocks in
//! `NotifyAddrChange` (IPv4 address table). Other systems keep the timer.

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::sync::atomic::Ordering;
#[cfg(any(target_os = "macos", target_os = "windows"))]
use std::time::Duration;

pub fn spawn(wake: Arc<AtomicBool>) {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        let _ = std::thread::Builder::new()
            .name("newtype-netwatch".into())
            .spawn(move || platform(wake));
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = wake;
    }
}

#[cfg(target_os = "macos")]
fn platform(wake: Arc<AtomicBool>) {
    unsafe extern "C" {
        fn notify_register_check(name: *const i8, out_token: *mut i32) -> u32;
        fn notify_check(token: i32, check: *mut i32) -> u32;
    }
    let name = b"com.apple.system.config.network_change\0";
    let mut token = 0i32;
    let status = unsafe { notify_register_check(name.as_ptr().cast(), &mut token) };
    if status != 0 {
        crate::diag::record("network-change watch is unavailable; probes stay on the timer");
        return;
    }
    loop {
        std::thread::sleep(Duration::from_secs(2));
        let mut changed = 0i32;
        let check = unsafe { notify_check(token, &mut changed) };
        if check == 0 && changed != 0 {
            wake.store(true, Ordering::Relaxed);
        }
    }
}

#[cfg(target_os = "windows")]
fn platform(wake: Arc<AtomicBool>) {
    loop {
        let status = unsafe {
            windows_sys::Win32::NetworkManagement::IpHelper::NotifyAddrChange(
                std::ptr::null_mut(),
                std::ptr::null(),
            )
        };
        wake.store(true, Ordering::Relaxed);
        if status != 0 {
            crate::diag::record("network-change watch returned an error; waiting before retrying");
            std::thread::sleep(Duration::from_secs(5));
        }
    }
}
