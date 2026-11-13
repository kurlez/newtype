//! Indicator window that must not take keyboard focus while recording.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;
use tauri::WebviewWindow;

const INDICATOR_LOGICAL_W: f64 = 256.0;
const INDICATOR_LOGICAL_H: f64 = 44.0;

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
    let _ = window.set_shadow(false);
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
        clear_dwm_border(hwnd);
    }
}

#[cfg(target_os = "windows")]
fn clear_dwm_border(hwnd: *mut std::ffi::c_void) {
    use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
    // DWMWA_BORDER_COLOR. DWMWA_COLOR_NONE keeps Windows 11 from drawing the 1px frame.
    const DWMWA_BORDER_COLOR: u32 = 34;
    const DWMWA_COLOR_NONE: u32 = 0xFFFF_FFFE;
    let color = DWMWA_COLOR_NONE;
    unsafe {
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR,
            &color as *const u32 as *const _,
            std::mem::size_of::<u32>() as u32,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WorkArea {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// Keep the window's top-left inside `area`. A window larger than the area
/// stays at the area origin.
pub fn clamp_origin(x: i32, y: i32, win_w: i32, win_h: i32, area: WorkArea) -> (i32, i32) {
    let win_w = win_w.max(1);
    let win_h = win_h.max(1);
    let max_x = area.x.saturating_add(area.width).saturating_sub(win_w);
    let max_y = area.y.saturating_add(area.height).saturating_sub(win_h);
    let x = if max_x < area.x { area.x } else { x.clamp(area.x, max_x) };
    let y = if max_y < area.y { area.y } else { y.clamp(area.y, max_y) };
    (x, y)
}

#[derive(Serialize, Deserialize, Default)]
struct Places {
    monitors: BTreeMap<String, SavedPoint>,
}

#[derive(Serialize, Deserialize, Clone, Copy)]
struct SavedPoint {
    x: i32,
    y: i32,
}

fn places_path(dir: &Path) -> std::path::PathBuf {
    dir.join("indicator-places.json")
}

pub fn load_place(dir: &Path, monitor: &str) -> Option<(i32, i32)> {
    let text = std::fs::read_to_string(places_path(dir)).ok()?;
    let places: Places = serde_json::from_str(&text).ok()?;
    places.monitors.get(monitor).map(|point| (point.x, point.y))
}

pub fn store_place(dir: &Path, monitor: &str, x: i32, y: i32) -> Result<(), String> {
    let path = places_path(dir);
    let mut places = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str::<Places>(&text).ok())
        .unwrap_or_default();
    places.monitors.insert(monitor.to_string(), SavedPoint { x, y });
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let body = serde_json::to_string(&places).map_err(|err| err.to_string())?;
    std::fs::write(path, body).map_err(|err| err.to_string())
}

pub fn place_indicator(window: &WebviewWindow, dir: Option<&Path>) {
    let Ok(Some(monitor)) = window.current_monitor() else {
        return;
    };
    let scale = monitor.scale_factor();
    let width = (INDICATOR_LOGICAL_W * scale).round() as i32;
    let height = (INDICATOR_LOGICAL_H * scale).round() as i32;
    let _ = window.set_size(tauri::Size::Physical(tauri::PhysicalSize {
        width: width.max(1) as u32,
        height: height.max(1) as u32,
    }));
    let area = work_area_of(&monitor);
    let name = monitor_key(&monitor);
    let saved = dir.and_then(|dir| load_place(dir, &name));
    let (x, y) = if let Some((x, y)) = saved {
        clamp_origin(x, y, width, height, area)
    } else {
        let x = area.x + (area.width - width) / 2;
        let y = area.y + area.height - height - (64.0 * scale).round() as i32;
        clamp_origin(x, y, width, height, area)
    };
    let _ = window.set_position(tauri::Position::Physical(tauri::PhysicalPosition { x, y }));
}

pub fn clamp_indicator(window: &WebviewWindow, _dir: Option<&Path>) {
    let Ok(pos) = window.outer_position() else {
        return;
    };
    let Ok(size) = window.outer_size() else {
        return;
    };
    let Ok(Some(monitor)) = window.current_monitor() else {
        return;
    };
    let area = work_area_of(&monitor);
    let (x, y) = clamp_origin(pos.x, pos.y, size.width as i32, size.height as i32, area);
    if x != pos.x || y != pos.y {
        let _ = window.set_position(tauri::Position::Physical(tauri::PhysicalPosition { x, y }));
    }
}

pub fn remember_indicator(window: &WebviewWindow, dir: &Path) -> Result<(), String> {
    let pos = window.outer_position().map_err(|err| err.to_string())?;
    let size = window.outer_size().map_err(|err| err.to_string())?;
    let monitor = window
        .current_monitor()
        .map_err(|err| err.to_string())?
        .ok_or_else(|| "no monitor".to_string())?;
    let area = work_area_of(&monitor);
    let (x, y) = clamp_origin(pos.x, pos.y, size.width as i32, size.height as i32, area);
    store_place(dir, &monitor_key(&monitor), x, y)?;
    if x != pos.x || y != pos.y {
        window
            .set_position(tauri::Position::Physical(tauri::PhysicalPosition { x, y }))
            .map_err(|err| err.to_string())?;
    }
    Ok(())
}

fn monitor_key(monitor: &tauri::Monitor) -> String {
    monitor
        .name()
        .cloned()
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| {
            let pos = monitor.position();
            format!("monitor-{}x{}", pos.x, pos.y)
        })
}

fn work_area_of(monitor: &tauri::Monitor) -> WorkArea {
    let work = monitor.work_area();
    WorkArea {
        x: work.position.x,
        y: work.position.y,
        width: work.size.width as i32,
        height: work.size.height as i32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_indicator_covers_about_a_quarter_of_the_old_panel() {
        let previous = 480.0 * 132.0;
        let now = INDICATOR_LOGICAL_W * INDICATOR_LOGICAL_H;
        assert!(INDICATOR_LOGICAL_W <= 260.0);
        assert!(INDICATOR_LOGICAL_H <= 64.0);
        assert!(now <= previous / 4.0);
    }

    #[test]
    fn a_smaller_window_stays_inside_a_saved_corner() {
        let area = WorkArea { x: 0, y: 0, width: 1920, height: 1080 };
        let (x, y) = clamp_origin(1600, 1000, INDICATOR_LOGICAL_W as i32, INDICATOR_LOGICAL_H as i32, area);
        assert!(x >= area.x && y >= area.y);
        assert!(x + INDICATOR_LOGICAL_W as i32 <= area.x + area.width);
        assert!(y + INDICATOR_LOGICAL_H as i32 <= area.y + area.height);
    }

    #[test]
    fn clamp_keeps_a_saved_point_inside_the_work_area() {
        let area = WorkArea { x: 100, y: 50, width: 800, height: 600 };
        assert_eq!(clamp_origin(120, 80, 200, 100, area), (120, 80));
        assert_eq!(clamp_origin(-40, 900, 200, 100, area), (100, 550));
        assert_eq!(clamp_origin(2000, -10, 200, 100, area), (700, 50));
        let tiny = WorkArea { x: 10, y: 20, width: 50, height: 40 };
        assert_eq!(clamp_origin(500, 500, 200, 100, tiny), (10, 20));
    }

    #[test]
    fn a_position_is_remembered_per_monitor() {
        let dir = std::env::temp_dir().join(format!("newtype-places-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        store_place(&dir, "display-a", 12, 34).unwrap();
        store_place(&dir, "display-b", 56, 78).unwrap();
        assert_eq!(load_place(&dir, "display-a"), Some((12, 34)));
        assert_eq!(load_place(&dir, "display-b"), Some((56, 78)));
        assert_eq!(load_place(&dir, "missing"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
