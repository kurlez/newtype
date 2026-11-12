//! Windows main-window translucency.
//!
//! Windows 11 uses Mica, then Acrylic if Mica is unavailable. Windows 10 keeps
//! a high alpha so the text stays readable. Other systems leave the window opaque.

use tauri::{AppHandle, Manager};

#[cfg(windows)]
const WIN11_BUILD: u32 = 22000;
#[cfg(windows)]
const WIN10_ALPHA: u8 = 230;

pub fn apply(app: &AppHandle, enabled: bool) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    #[cfg(windows)]
    apply_windows(&window, enabled);
    #[cfg(not(windows))]
    {
        let _ = (&window, enabled);
    }
}

#[cfg(windows)]
fn apply_windows(window: &tauri::WebviewWindow, enabled: bool) {
    clear_layered(window);
    let _ = window_vibrancy::clear_mica(window);
    let _ = window_vibrancy::clear_acrylic(window);
    let _ = window_vibrancy::clear_blur(window);
    if !enabled {
        crate::diag::record("window material off");
        return;
    }
    if windows_build() >= WIN11_BUILD {
        if window_vibrancy::apply_mica(window, None).is_ok() {
            crate::diag::record("window material mica");
            return;
        }
        if window_vibrancy::apply_acrylic(window, Some((244, 241, 234, 180))).is_ok() {
            crate::diag::record("window material acrylic");
            return;
        }
    }
    set_alpha(window, WIN10_ALPHA);
    crate::diag::record(&format!("window material alpha={WIN10_ALPHA}"));
}

#[cfg(windows)]
fn windows_build() -> u32 {
    #[repr(C)]
    struct OsVersion {
        size: u32,
        major: u32,
        minor: u32,
        build: u32,
        platform: u32,
        csd: [u16; 128],
    }
    #[link(name = "ntdll")]
    extern "system" {
        fn RtlGetVersion(info: *mut OsVersion) -> i32;
    }
    unsafe {
        let mut info = OsVersion {
            size: std::mem::size_of::<OsVersion>() as u32,
            major: 0,
            minor: 0,
            build: 0,
            platform: 0,
            csd: [0; 128],
        };
        if RtlGetVersion(&mut info) == 0 {
            info.build
        } else {
            0
        }
    }
}

#[cfg(windows)]
fn hwnd(window: &tauri::WebviewWindow) -> Option<*mut std::ffi::c_void> {
    let handle = window.hwnd().ok()?;
    Some(handle.0 as *mut std::ffi::c_void)
}

#[cfg(windows)]
fn clear_layered(window: &tauri::WebviewWindow) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE, SWP_FRAMECHANGED,
        SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, WS_EX_LAYERED,
    };
    let Some(hwnd) = hwnd(window) else {
        return;
    };
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let next = style & !(WS_EX_LAYERED as isize);
        if next != style {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, next);
            SetWindowPos(
                hwnd,
                std::ptr::null_mut(),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
            );
        }
    }
}

#[cfg(windows)]
fn set_alpha(window: &tauri::WebviewWindow, alpha: u8) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetLayeredWindowAttributes, SetWindowLongPtrW, SetWindowPos,
        GWL_EXSTYLE, LWA_ALPHA, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
        SWP_NOZORDER, WS_EX_LAYERED,
    };
    let Some(hwnd) = hwnd(window) else {
        return;
    };
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style | (WS_EX_LAYERED as isize));
        SetWindowPos(
            hwnd,
            std::ptr::null_mut(),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
        SetLayeredWindowAttributes(hwnd, 0u32, alpha, LWA_ALPHA);
    }
}
