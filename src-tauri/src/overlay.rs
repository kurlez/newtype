//! Indicator window that must not take keyboard focus while recording.

use tauri::WebviewWindow;

/// Show `window` without activating it. Does not change window style, so the
/// main window can use this while recording and stay on the taskbar.
pub fn show_without_activating(window: &WebviewWindow) {
    #[cfg(target_os = "windows")]
    {
        if let Some(hwnd) = raw_hwnd(window) {
            use windows_sys::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_SHOWNOACTIVATE};
            unsafe {
                ShowWindow(hwnd, SW_SHOWNOACTIVATE as i32);
            }
            return;
        }
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(ns) = ns_window(window) {
            unsafe {
                msg_void(ns, c"orderFrontRegardless");
            }
            return;
        }
    }
    let _ = window.show();
}

pub fn configure_indicator(window: &WebviewWindow) {
    let _ = window.set_focusable(false);
    #[cfg(target_os = "windows")]
    apply_windows_noactivate(window);
    #[cfg(target_os = "macos")]
    apply_macos_nonactivating(window);
}

#[cfg(target_os = "windows")]
fn raw_hwnd(window: &WebviewWindow) -> Option<*mut std::ffi::c_void> {
    let hwnd = window.hwnd().ok()?;
    Some(hwnd.0 as *mut std::ffi::c_void)
}

#[cfg(target_os = "windows")]
fn apply_windows_noactivate(window: &WebviewWindow) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE, SWP_FRAMECHANGED,
        SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    };
    let Some(hwnd) = raw_hwnd(window) else {
        return;
    };
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let next = style | (WS_EX_NOACTIVATE as isize) | (WS_EX_TOOLWINDOW as isize);
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

#[cfg(target_os = "macos")]
fn ns_window(window: &WebviewWindow) -> Option<*mut std::ffi::c_void> {
    window.ns_window().ok()
}

#[cfg(target_os = "macos")]
fn apply_macos_nonactivating(window: &WebviewWindow) {
    let Some(ns) = ns_window(window) else {
        return;
    };
    unsafe {
        let style = msg_usize(ns, c"styleMask");
        // NSWindowStyleMaskNonactivatingPanel
        msg_void_usize(ns, c"setStyleMask:", style | (1 << 7));
        // Join all spaces, stay out of the window cycle, work over full screen.
        let behavior = (1usize << 0) | (1 << 4) | (1 << 6) | (1 << 8);
        msg_void_usize(ns, c"setCollectionBehavior:", behavior);
        msg_void_u8(ns, c"setHidesOnDeactivate:", 0);
        install_nonactivating_class(ns);
    }
}

#[cfg(target_os = "macos")]
fn install_nonactivating_class(ns_window: *mut std::ffi::c_void) {
    use std::sync::atomic::{AtomicBool, Ordering};
    #[link(name = "objc", kind = "dylib")]
    extern "C" {
        fn objc_getClass(name: *const i8) -> *mut std::ffi::c_void;
        fn objc_allocateClassPair(
            superclass: *mut std::ffi::c_void,
            name: *const i8,
            extra: usize,
        ) -> *mut std::ffi::c_void;
        fn objc_registerClassPair(cls: *mut std::ffi::c_void);
        fn object_getClass(obj: *mut std::ffi::c_void) -> *mut std::ffi::c_void;
        fn object_setClass(
            obj: *mut std::ffi::c_void,
            cls: *mut std::ffi::c_void,
        ) -> *mut std::ffi::c_void;
        fn class_addMethod(
            cls: *mut std::ffi::c_void,
            sel: *mut std::ffi::c_void,
            imp: *const std::ffi::c_void,
            types: *const i8,
        ) -> u8;
        fn sel_registerName(name: *const i8) -> *mut std::ffi::c_void;
    }
    static READY: AtomicBool = AtomicBool::new(false);
    unsafe {
        if !READY.swap(true, Ordering::SeqCst) {
            let super_class = object_getClass(ns_window);
            let subclass = objc_allocateClassPair(super_class, c"NewtypeNonActivatingPanel".as_ptr(), 0);
            if !subclass.is_null() {
                let key = sel_registerName(c"canBecomeKeyWindow".as_ptr());
                let main = sel_registerName(c"canBecomeMainWindow".as_ptr());
                class_addMethod(
                    subclass,
                    key,
                    refuse_key as *const std::ffi::c_void,
                    c"c@:".as_ptr(),
                );
                class_addMethod(
                    subclass,
                    main,
                    refuse_key as *const std::ffi::c_void,
                    c"c@:".as_ptr(),
                );
                objc_registerClassPair(subclass);
            }
        }
        let subclass = objc_getClass(c"NewtypeNonActivatingPanel".as_ptr());
        if !subclass.is_null() {
            object_setClass(ns_window, subclass);
        }
    }
}

#[cfg(target_os = "macos")]
unsafe extern "C" fn refuse_key(_this: *mut std::ffi::c_void, _cmd: *mut std::ffi::c_void) -> u8 {
    0
}

#[cfg(target_os = "macos")]
fn msg_void(receiver: *mut std::ffi::c_void, name: &std::ffi::CStr) {
    #[link(name = "objc", kind = "dylib")]
    extern "C" {
        fn sel_registerName(name: *const i8) -> *mut std::ffi::c_void;
        fn objc_msgSend();
    }
    unsafe {
        let sel = sel_registerName(name.as_ptr());
        let function: unsafe extern "C" fn(*mut std::ffi::c_void, *mut std::ffi::c_void) =
            std::mem::transmute(objc_msgSend as *const ());
        function(receiver, sel);
    }
}

#[cfg(target_os = "macos")]
fn msg_void_usize(receiver: *mut std::ffi::c_void, name: &std::ffi::CStr, arg: usize) {
    #[link(name = "objc", kind = "dylib")]
    extern "C" {
        fn sel_registerName(name: *const i8) -> *mut std::ffi::c_void;
        fn objc_msgSend();
    }
    unsafe {
        let sel = sel_registerName(name.as_ptr());
        let function: unsafe extern "C" fn(*mut std::ffi::c_void, *mut std::ffi::c_void, usize) =
            std::mem::transmute(objc_msgSend as *const ());
        function(receiver, sel, arg);
    }
}

#[cfg(target_os = "macos")]
fn msg_void_u8(receiver: *mut std::ffi::c_void, name: &std::ffi::CStr, arg: u8) {
    #[link(name = "objc", kind = "dylib")]
    extern "C" {
        fn sel_registerName(name: *const i8) -> *mut std::ffi::c_void;
        fn objc_msgSend();
    }
    unsafe {
        let sel = sel_registerName(name.as_ptr());
        let function: unsafe extern "C" fn(*mut std::ffi::c_void, *mut std::ffi::c_void, u8) =
            std::mem::transmute(objc_msgSend as *const ());
        function(receiver, sel, arg);
    }
}

#[cfg(target_os = "macos")]
fn msg_usize(receiver: *mut std::ffi::c_void, name: &std::ffi::CStr) -> usize {
    #[link(name = "objc", kind = "dylib")]
    extern "C" {
        fn sel_registerName(name: *const i8) -> *mut std::ffi::c_void;
        fn objc_msgSend();
    }
    unsafe {
        let sel = sel_registerName(name.as_ptr());
        let function: unsafe extern "C" fn(*mut std::ffi::c_void, *mut std::ffi::c_void) -> usize =
            std::mem::transmute(objc_msgSend as *const ());
        function(receiver, sel)
    }
}
