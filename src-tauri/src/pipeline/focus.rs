//! Remember the window the user was typing in, and put that focus back before paste.
//!
//! The log line names the process and window ids. It never includes dictated text.

use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use super::insert;

pub fn front_pid() -> Option<u32> {
    #[cfg(target_os = "macos")]
    {
        return macos::front_pid();
    }
    #[cfg(target_os = "windows")]
    {
        return windows::front_pid();
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        None
    }
}

pub fn field_text(target: &FocusTarget) -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        return macos::field_text(target);
    }
    #[cfg(target_os = "windows")]
    {
        return windows::field_text(target);
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = target;
        None
    }
}

pub fn notice() -> &'static str {
    if cfg!(target_os = "macos") {
        crate::locale::text("Copied. Press \u{2318}V to paste.", "已复制，按 \u{2318}V 粘贴")
    } else {
        crate::locale::text("Copied. Press Ctrl+V to paste.", "已复制，按 Ctrl+V 粘贴")
    }
}

#[derive(Debug)]
pub struct FocusTarget {
    pub pid: u32,
    pub window: u64,
    pub control: u64,
    pub thread_id: u32,
    pub app: String,
    /// macOS: retained AXUIElement. Zero everywhere else.
    ax: usize,
}

impl FocusTarget {
    pub fn from_parts(pid: u32, window: u64, control: u64, thread_id: u32, app: impl Into<String>) -> Self {
        Self {
            pid,
            window,
            control,
            thread_id,
            app: safe_app(&app.into()),
            ax: 0,
        }
    }

    fn own(&self) -> bool {
        self.pid != 0 && self.pid == std::process::id()
    }
}

impl Clone for FocusTarget {
    fn clone(&self) -> Self {
        #[cfg(target_os = "macos")]
        if self.ax != 0 {
            unsafe { CFRetain(self.ax as *const std::ffi::c_void) };
        }
        Self {
            pid: self.pid,
            window: self.window,
            control: self.control,
            thread_id: self.thread_id,
            app: self.app.clone(),
            ax: self.ax,
        }
    }
}

impl Drop for FocusTarget {
    fn drop(&mut self) {
        #[cfg(target_os = "macos")]
        if self.ax != 0 {
            unsafe { CFRelease(self.ax as *const std::ffi::c_void) };
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertPlan {
    /// Foreground already matches the captured target.
    Paste,
    /// Foreground moved. Put the captured window back, then paste.
    Restore,
    /// Restoring failed, or the only window is ours. Leave the text on the clipboard.
    CopyOnly,
}

pub fn foreground_matches(target: &FocusTarget, current: &FocusTarget) -> bool {
    if target.pid != 0 && current.pid != 0 && target.pid != current.pid {
        return false;
    }
    if target.window != 0 && current.window != 0 && target.window != current.window {
        return false;
    }
    if target.control != 0 && current.control != 0 && target.control != current.control {
        return false;
    }
    (target.pid != 0 && target.pid == current.pid) || (target.window != 0 && target.window == current.window)
}

pub fn plan(
    target: Option<&FocusTarget>,
    current: Option<&FocusTarget>,
    own_pid: u32,
    return_to_field: bool,
) -> InsertPlan {
    let current_is_ours = current.is_some_and(|current| own_pid != 0 && current.pid == own_pid);
    if current_is_ours {
        return InsertPlan::CopyOnly;
    }
    if !return_to_field {
        return if current.is_some() {
            InsertPlan::Paste
        } else {
            InsertPlan::CopyOnly
        };
    }
    let Some(target) = target else {
        return if current.is_some() {
            InsertPlan::Paste
        } else {
            InsertPlan::CopyOnly
        };
    };
    if own_pid != 0 && target.pid == own_pid {
        return InsertPlan::CopyOnly;
    }
    match current {
        Some(current) if foreground_matches(target, current) => InsertPlan::Paste,
        _ => InsertPlan::Restore,
    }
}

pub fn log_line(kind: &str, target: &FocusTarget) -> String {
    format!(
        "focus {kind} pid={} window={:#x} control={:#x} thread={} app={}",
        target.pid, target.window, target.control, target.thread_id, target.app
    )
}

fn safe_app(name: &str) -> String {
    let clean: String = name
        .chars()
        .filter(|ch| !ch.is_control() && *ch != '\n' && *ch != '\r')
        .take(80)
        .collect();
    let clean = clean.trim();
    if clean.is_empty() {
        "unknown".to_string()
    } else {
        clean.to_string()
    }
}

/// Store the foreground app when it is not this process.
/// `prefer_existing` keeps a target already captured at the hotkey.
pub fn remember(slot: &Mutex<Option<FocusTarget>>, prefer_existing: bool) {
    let Some(current) = capture() else {
        return;
    };
    let mut guard = slot.lock().expect("focus target");
    if current.own() {
        return;
    }
    if prefer_existing && guard.as_ref().is_some_and(|saved| !saved.own()) {
        return;
    }
    *guard = Some(current);
}

pub fn log_saved(slot: &Mutex<Option<FocusTarget>>) {
    let guard = slot.lock().expect("focus target");
    match guard.as_ref() {
        Some(target) => crate::diag::record(&log_line("capture", target)),
        None => crate::diag::record("focus capture empty"),
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum InsertOutcome {
    Pasted,
    Copied,
    Failed(String),
}

pub fn insert_with_target(
    text: &str,
    target: Option<&FocusTarget>,
    return_to_field: bool,
) -> InsertOutcome {
    if text.is_empty() {
        return InsertOutcome::Pasted;
    }
    let own = std::process::id();
    let current = capture();
    if target.is_none() && current.as_ref().is_some_and(|item| item.pid != own) {
        crate::diag::record("focus capture-missing");
    }
    match plan(target, current.as_ref(), own, return_to_field) {
        InsertPlan::CopyOnly => copy_only(text, target),
        InsertPlan::Restore => {
            let Some(target) = target else {
                return copy_only(text, None);
            };
            if !restore_and_wait(target) {
                return copy_only(text, Some(target));
            }
            paste(text, Some(target))
        }
        InsertPlan::Paste => {
            if let Some(target) = target {
                focus_control(target);
            }
            paste(text, target)
        }
    }
}

fn paste(text: &str, target: Option<&FocusTarget>) -> InsertOutcome {
    match insert::insert_text(text) {
        Ok(()) => InsertOutcome::Pasted,
        Err(err) => {
            if let Some(target) = target {
                if write_accessible_verified(target, text) {
                    return InsertOutcome::Pasted;
                }
            }
            InsertOutcome::Failed(err)
        }
    }
}

fn write_accessible_verified(target: &FocusTarget, text: &str) -> bool {
    let before = field_text(target);
    if !write_accessible(target, text) {
        return false;
    }
    let after = field_text(target);
    after.is_some() && after != before
}

fn write_accessible(target: &FocusTarget, text: &str) -> bool {
    #[cfg(target_os = "macos")]
    {
        return macos::set_selected_text(target, text);
    }
    #[cfg(target_os = "windows")]
    {
        return windows::replace_selection(target, text);
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (target, text);
        false
    }
}

fn copy_only(text: &str, target: Option<&FocusTarget>) -> InsertOutcome {
    if let Some(target) = target {
        crate::diag::record(&log_line("restore-failed", target));
    } else {
        crate::diag::record("focus restore-failed no-target");
    }
    match insert::copy_text(text) {
        Ok(()) => InsertOutcome::Copied,
        Err(err) => InsertOutcome::Failed(err),
    }
}

fn restore_and_wait(target: &FocusTarget) -> bool {
    restore_once(target);
    let deadline = Instant::now() + Duration::from_millis(400);
    let mut retried = false;
    loop {
        if let Some(current) = capture() {
            if foreground_matches(target, &current) && !current.own() {
                focus_control(target);
                crate::diag::record(&log_line("restored", target));
                return true;
            }
        }
        if Instant::now() >= deadline {
            break;
        }
        if !retried && deadline.saturating_duration_since(Instant::now()) < Duration::from_millis(250) {
            restore_once(target);
            retried = true;
        }
        thread::sleep(Duration::from_millis(20));
    }
    false
}

pub(crate) fn capture() -> Option<FocusTarget> {
    #[cfg(target_os = "windows")]
    {
        return windows::capture();
    }
    #[cfg(target_os = "macos")]
    {
        return macos::capture();
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        None
    }
}

fn restore_once(target: &FocusTarget) {
    #[cfg(target_os = "windows")]
    windows::restore(target);
    #[cfg(target_os = "macos")]
    macos::restore(target);
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = target;
    }
}

fn focus_control(target: &FocusTarget) {
    #[cfg(target_os = "windows")]
    windows::focus_control(target);
    #[cfg(target_os = "macos")]
    macos::focus_control(target);
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = target;
    }
}

#[cfg(target_os = "windows")]
mod windows {
    use super::FocusTarget;
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        AttachThreadInput, GetCurrentThreadId, OpenProcess, QueryFullProcessImageNameW,
    };
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::SetFocus;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AllowSetForegroundWindow, BringWindowToTop, GetClassNameW, GetForegroundWindow,
        GetGUIThreadInfo, GetWindowThreadProcessId, IsWindow, SetForegroundWindow, ShowWindow,
        GUITHREADINFO, ASFW_ANY, SW_SHOW,
    };

    const PROCESS_QUERY_LIMITED_INFORMATION: u32 = 0x1000;

    pub fn front_pid() -> Option<u32> {
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.is_null() {
                return None;
            }
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, &mut pid);
            (pid != 0).then_some(pid)
        }
    }

    pub fn capture() -> Option<FocusTarget> {
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.is_null() {
                return None;
            }
            let mut pid = 0u32;
            let thread = GetWindowThreadProcessId(hwnd, &mut pid);
            let mut info: GUITHREADINFO = std::mem::zeroed();
            info.cbSize = std::mem::size_of::<GUITHREADINFO>() as u32;
            let focused = if GetGUIThreadInfo(thread, &mut info) != 0 && !info.hwndFocus.is_null() {
                info.hwndFocus
            } else {
                hwnd
            };
            let app = process_name(pid).or_else(|| class_name(hwnd));
            Some(FocusTarget::from_parts(
                pid,
                hwnd as usize as u64,
                focused as usize as u64,
                thread,
                app.unwrap_or_else(|| "unknown".to_string()),
            ))
        }
    }

    pub fn restore(target: &FocusTarget) {
        unsafe {
            let hwnd = target.window as usize as *mut std::ffi::c_void;
            if hwnd.is_null() || IsWindow(hwnd) == 0 {
                return;
            }
            AllowSetForegroundWindow(ASFW_ANY);
            let foreground = GetForegroundWindow();
            let fg_thread = GetWindowThreadProcessId(foreground, std::ptr::null_mut());
            let target_thread = if target.thread_id != 0 {
                target.thread_id
            } else {
                GetWindowThreadProcessId(hwnd, std::ptr::null_mut())
            };
            let current = GetCurrentThreadId();
            let attached_fg = fg_thread != 0
                && fg_thread != current
                && AttachThreadInput(current, fg_thread, 1) != 0;
            let attached_target = target_thread != 0
                && target_thread != current
                && target_thread != fg_thread
                && AttachThreadInput(current, target_thread, 1) != 0;
            ShowWindow(hwnd, SW_SHOW as i32);
            SetForegroundWindow(hwnd);
            BringWindowToTop(hwnd);
            focus_control(target);
            if attached_target {
                AttachThreadInput(current, target_thread, 0);
            }
            if attached_fg {
                AttachThreadInput(current, fg_thread, 0);
            }
        }
    }

    pub fn focus_control(target: &FocusTarget) {
        unsafe {
            let control = target.control as usize as *mut std::ffi::c_void;
            let window = target.window as usize as *mut std::ffi::c_void;
            let hwnd = if !control.is_null() && IsWindow(control) != 0 {
                control
            } else {
                window
            };
            if !hwnd.is_null() && IsWindow(hwnd) != 0 {
                SetFocus(hwnd);
            }
        }
    }

    pub fn replace_selection(target: &FocusTarget, text: &str) -> bool {
        unsafe {
            let control = target.control as usize as *mut std::ffi::c_void;
            if control.is_null() || IsWindow(control) == 0 {
                return false;
            }
            let class = class_name(control).unwrap_or_default();
            let editable = class.eq_ignore_ascii_case("Edit")
                || class.to_ascii_lowercase().contains("richedit");
            if !editable {
                return false;
            }
            let style = windows_sys::Win32::UI::WindowsAndMessaging::GetWindowLongW(control, -16);
            if style & 0x0800 != 0 {
                return false;
            }
            let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
            const EM_REPLACESEL: u32 = 0x00C2;
            windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW(
                control,
                EM_REPLACESEL,
                1,
                wide.as_ptr() as isize,
            );
            true
        }
    }

    pub fn field_text(target: &FocusTarget) -> Option<String> {
        if let Some(text) = uia_focused_text() {
            return Some(text);
        }
        unsafe {
            let control = target.control as usize as *mut std::ffi::c_void;
            if control.is_null() || IsWindow(control) == 0 {
                return None;
            }
            let style = windows_sys::Win32::UI::WindowsAndMessaging::GetWindowLongW(control, -16);
            if style & 0x20 != 0 || style & 0x0800 != 0 {
                return None;
            }
            let class = class_name(control).unwrap_or_default();
            if !class.eq_ignore_ascii_case("Edit") && !class.to_ascii_lowercase().contains("richedit") {
                return None;
            }
            let mut buffer = vec![0u16; 2048];
            let count = windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW(
                control,
                0x000D,
                buffer.len(),
                buffer.as_mut_ptr() as isize,
            );
            if count <= 0 {
                return None;
            }
            let end = (count as usize).min(buffer.len());
            Some(String::from_utf16_lossy(&buffer[..end]))
        }
    }

    /// Focused control via UI Automation ValuePattern. Password fields return nothing.
    /// A failure falls through to the Edit/RichEdit reader.
    fn uia_focused_text() -> Option<String> {
        #[repr(C)]
        struct Guid {
            data1: u32,
            data2: u16,
            data3: u16,
            data4: [u8; 8],
        }
        #[repr(C)]
        struct Variant {
            vt: u16,
            reserved1: u16,
            reserved2: u16,
            reserved3: u16,
            data: u64,
        }
        const CLSID_CUIAUTOMATION: Guid = Guid {
            data1: 0xff48dba4,
            data2: 0x60ef,
            data3: 0x4201,
            data4: [0xaa, 0x87, 0x54, 0x10, 0x3e, 0xef, 0x59, 0x4e],
        };
        const IID_IUIAUTOMATION: Guid = Guid {
            data1: 0x30cbe57d,
            data2: 0xd9d0,
            data3: 0x452a,
            data4: [0xab, 0x13, 0x7a, 0xc5, 0xac, 0x48, 0x25, 0xee],
        };
        const UIA_VALUE_PATTERN: i32 = 10002;
        const UIA_IS_PASSWORD: i32 = 30019;
        const VT_BOOL: u16 = 11;

        #[link(name = "ole32")]
        extern "system" {
            fn CoInitializeEx(reserved: *mut std::ffi::c_void, model: u32) -> i32;
            fn CoCreateInstance(
                clsid: *const Guid,
                outer: *mut std::ffi::c_void,
                context: u32,
                iid: *const Guid,
                out: *mut *mut std::ffi::c_void,
            ) -> i32;
            fn CoUninitialize();
        }
        #[link(name = "oleaut32")]
        extern "system" {
            fn SysStringLen(bstr: *const u16) -> u32;
            fn SysFreeString(bstr: *const u16);
        }

        unsafe fn release(ptr: *mut std::ffi::c_void) {
            if ptr.is_null() {
                return;
            }
            let table = *(ptr as *const *const usize);
            let release: unsafe extern "system" fn(*mut std::ffi::c_void) -> u32 =
                std::mem::transmute(*table.add(2));
            release(ptr);
        }

        unsafe {
            let started = CoInitializeEx(std::ptr::null_mut(), 2);
            let mut automation = std::ptr::null_mut();
            let hr = CoCreateInstance(
                &CLSID_CUIAUTOMATION,
                std::ptr::null_mut(),
                1,
                &IID_IUIAUTOMATION,
                &mut automation,
            );
            if hr < 0 || automation.is_null() {
                if started == 0 {
                    CoUninitialize();
                }
                return None;
            }
            let table = *(automation as *const *const usize);
            let get_focused: unsafe extern "system" fn(
                *mut std::ffi::c_void,
                *mut *mut std::ffi::c_void,
            ) -> i32 = std::mem::transmute(*table.add(8));
            let mut element = std::ptr::null_mut();
            let hr = get_focused(automation, &mut element);
            release(automation);
            if hr < 0 || element.is_null() {
                if started == 0 {
                    CoUninitialize();
                }
                return None;
            }
            let element_table = *(element as *const *const usize);
            let get_property: unsafe extern "system" fn(*mut std::ffi::c_void, i32, *mut Variant) -> i32 =
                std::mem::transmute(*element_table.add(10));
            let mut password = Variant {
                vt: 0,
                reserved1: 0,
                reserved2: 0,
                reserved3: 0,
                data: 0,
            };
            let hr = get_property(element, UIA_IS_PASSWORD, &mut password);
            if hr >= 0 && password.vt == VT_BOOL && (password.data as u16) != 0 {
                release(element);
                if started == 0 {
                    CoUninitialize();
                }
                return None;
            }
            let get_pattern: unsafe extern "system" fn(
                *mut std::ffi::c_void,
                i32,
                *mut *mut std::ffi::c_void,
            ) -> i32 = std::mem::transmute(*element_table.add(16));
            let mut pattern = std::ptr::null_mut();
            let hr = get_pattern(element, UIA_VALUE_PATTERN, &mut pattern);
            release(element);
            if hr < 0 || pattern.is_null() {
                if started == 0 {
                    CoUninitialize();
                }
                return None;
            }
            let pattern_table = *(pattern as *const *const usize);
            let current_value: unsafe extern "system" fn(*mut std::ffi::c_void, *mut *mut u16) -> i32 =
                std::mem::transmute(*pattern_table.add(4));
            let mut bstr = std::ptr::null_mut();
            let hr = current_value(pattern, &mut bstr);
            release(pattern);
            if started == 0 {
                CoUninitialize();
            }
            if hr < 0 || bstr.is_null() {
                return None;
            }
            let len = SysStringLen(bstr) as usize;
            let text = String::from_utf16_lossy(std::slice::from_raw_parts(bstr, len));
            SysFreeString(bstr);
            if text.is_empty() { None } else { Some(text) }
        }
    }

    fn process_name(pid: u32) -> Option<String> {
        if pid == 0 {
            return None;
        }
        unsafe {
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if handle.is_null() {
                return None;
            }
            let mut buffer = [0u16; 512];
            let mut length = buffer.len() as u32;
            let ok = QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut length);
            CloseHandle(handle);
            if ok == 0 {
                return None;
            }
            let path = String::from_utf16_lossy(&buffer[..length as usize]);
            Some(
                path.rsplit(['\\', '/'])
                    .next()
                    .unwrap_or("")
                    .trim_end_matches(".exe")
                    .trim_end_matches(".EXE")
                    .to_string(),
            )
        }
    }

    fn class_name(hwnd: *mut std::ffi::c_void) -> Option<String> {
        unsafe {
            let mut buffer = [0u16; 128];
            let length = GetClassNameW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32);
            if length <= 0 {
                return None;
            }
            Some(String::from_utf16_lossy(&buffer[..length as usize]))
        }
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use core_foundation::base::TCFType;
    use core_foundation::boolean::CFBoolean;
    use core_foundation::string::CFString;

    use super::FocusTarget;

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXUIElementCreateApplication(pid: i32) -> *mut std::ffi::c_void;
        fn AXUIElementCreateSystemWide() -> *mut std::ffi::c_void;
        fn AXUIElementCopyAttributeValue(
            element: *mut std::ffi::c_void,
            attribute: *const std::ffi::c_void,
            value: *mut *mut std::ffi::c_void,
        ) -> i32;
        fn AXUIElementSetAttributeValue(
            element: *mut std::ffi::c_void,
            attribute: *const std::ffi::c_void,
            value: *const std::ffi::c_void,
        ) -> i32;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(cf: *const std::ffi::c_void);
    }

    pub fn front_pid() -> Option<u32> {
        unsafe {
            let system = AXUIElementCreateSystemWide();
            if system.is_null() {
                return None;
            }
            let attribute = CFString::new("AXFocusedApplication");
            let mut value = std::ptr::null_mut();
            let status = AXUIElementCopyAttributeValue(
                system,
                attribute.as_concrete_TypeRef() as *const std::ffi::c_void,
                &mut value,
            );
            CFRelease(system);
            if status != 0 || value.is_null() {
                return None;
            }
            #[link(name = "ApplicationServices", kind = "framework")]
            extern "C" {
                fn AXUIElementGetPid(element: *mut std::ffi::c_void, pid: *mut i32) -> i32;
            }
            let mut pid = 0i32;
            let status = AXUIElementGetPid(value, &mut pid);
            CFRelease(value);
            if status != 0 || pid <= 0 {
                None
            } else {
                Some(pid as u32)
            }
        }
    }

    pub fn capture() -> Option<FocusTarget> {
        let pid = front_pid()?;
        let ax = focused_element(pid as i32);
        let mut target = FocusTarget::from_parts(pid, 0, 0, 0, format!("pid-{pid}"));
        target.ax = ax as usize;
        Some(target)
    }

    pub fn restore(target: &FocusTarget) {
        if target.pid == 0 {
            return;
        }
        unsafe {
            let running = objc_class(c"NSRunningApplication");
            if running.is_null() {
                return;
            }
            let app = msg_usize(running, c"runningApplicationWithProcessIdentifier:", target.pid as usize);
            if app.is_null() {
                return;
            }
            // NSApplicationActivateIgnoringOtherApps
            msg_void_usize(app, c"activateWithOptions:", 2);
        }
        focus_control(target);
    }

    pub fn focus_control(target: &FocusTarget) {
        if target.ax == 0 {
            return;
        }
        raise_window(target.ax as *mut std::ffi::c_void);
        set_bool(target.ax as *mut std::ffi::c_void, "AXFocused");
    }

    pub fn field_text(target: &FocusTarget) -> Option<String> {
        if target.ax == 0 {
            return None;
        }
        let role = copy_ax_string(target.ax, "AXRole").unwrap_or_default();
        let subrole = copy_ax_string(target.ax, "AXSubrole").unwrap_or_default();
        if crate::pipeline::corrections::is_secure_role(&role)
            || crate::pipeline::corrections::is_secure_role(&subrole)
            || subrole == "AXSecureTextField"
        {
            return None;
        }
        copy_ax_string(target.ax, "AXValue")
    }

    fn copy_ax_string(element: usize, name: &str) -> Option<String> {
        let attribute = CFString::new(name);
        unsafe {
            let mut value = std::ptr::null_mut();
            let status = AXUIElementCopyAttributeValue(
                element as *mut std::ffi::c_void,
                attribute.as_concrete_TypeRef() as *const std::ffi::c_void,
                &mut value,
            );
            if status != 0 || value.is_null() {
                return None;
            }
            #[link(name = "CoreFoundation", kind = "framework")]
            extern "C" {
                fn CFGetTypeID(cf: *const std::ffi::c_void) -> usize;
                fn CFStringGetTypeID() -> usize;
                fn CFRelease(cf: *const std::ffi::c_void);
            }
            if CFGetTypeID(value) != CFStringGetTypeID() {
                CFRelease(value);
                return None;
            }
            Some(CFString::wrap_under_create_rule(value as _).to_string())
        }
    }

    pub fn set_selected_text(target: &FocusTarget, text: &str) -> bool {
        if target.ax == 0 || text.is_empty() {
            return false;
        }
        let attribute = CFString::new("AXSelectedText");
        let value = CFString::new(text);
        unsafe {
            AXUIElementSetAttributeValue(
                target.ax as *mut std::ffi::c_void,
                attribute.as_concrete_TypeRef() as *const std::ffi::c_void,
                value.as_concrete_TypeRef() as *const std::ffi::c_void,
            ) == 0
        }
    }

    fn raise_window(element: *mut std::ffi::c_void) {
        if element.is_null() {
            return;
        }
        let attribute = CFString::new("AXWindow");
        unsafe {
            let mut value = std::ptr::null_mut();
            let status = AXUIElementCopyAttributeValue(
                element,
                attribute.as_concrete_TypeRef() as *const std::ffi::c_void,
                &mut value,
            );
            if status == 0 && !value.is_null() {
                set_bool(value, "AXMain");
                CFRelease(value);
            }
        }
    }

    fn set_bool(element: *mut std::ffi::c_void, name: &str) {
        let attribute = CFString::new(name);
        let focused = CFBoolean::true_value();
        unsafe {
            AXUIElementSetAttributeValue(
                element,
                attribute.as_concrete_TypeRef() as *const std::ffi::c_void,
                focused.as_concrete_TypeRef() as *const std::ffi::c_void,
            );
        }
    }

    fn focused_element(pid: i32) -> *mut std::ffi::c_void {
        unsafe {
            let app = AXUIElementCreateApplication(pid);
            let copied = copy_focused(app);
            if !app.is_null() {
                CFRelease(app);
            }
            if !copied.is_null() {
                return copied;
            }
            let system = AXUIElementCreateSystemWide();
            let copied = copy_focused(system);
            if !system.is_null() {
                CFRelease(system);
            }
            copied
        }
    }

    fn copy_focused(element: *mut std::ffi::c_void) -> *mut std::ffi::c_void {
        if element.is_null() {
            return std::ptr::null_mut();
        }
        let attribute = CFString::new("AXFocusedUIElement");
        unsafe {
            let mut value = std::ptr::null_mut();
            let status = AXUIElementCopyAttributeValue(
                element,
                attribute.as_concrete_TypeRef() as *const std::ffi::c_void,
                &mut value,
            );
            if status != 0 {
                std::ptr::null_mut()
            } else {
                value
            }
        }
    }

    fn objc_class(name: &std::ffi::CStr) -> *mut std::ffi::c_void {
        #[link(name = "objc", kind = "dylib")]
        extern "C" {
            fn objc_getClass(name: *const i8) -> *mut std::ffi::c_void;
        }
        unsafe { objc_getClass(name.as_ptr()) }
    }

    fn msg0(receiver: *mut std::ffi::c_void, name: &std::ffi::CStr) -> *mut std::ffi::c_void {
        #[link(name = "objc", kind = "dylib")]
        extern "C" {
            fn sel_registerName(name: *const i8) -> *mut std::ffi::c_void;
            fn objc_msgSend();
        }
        unsafe {
            let sel = sel_registerName(name.as_ptr());
            let function: unsafe extern "C" fn(
                *mut std::ffi::c_void,
                *mut std::ffi::c_void,
            ) -> *mut std::ffi::c_void = std::mem::transmute(objc_msgSend as *const ());
            function(receiver, sel)
        }
    }

    fn msg_usize(
        receiver: *mut std::ffi::c_void,
        name: &std::ffi::CStr,
        arg: usize,
    ) -> *mut std::ffi::c_void {
        #[link(name = "objc", kind = "dylib")]
        extern "C" {
            fn sel_registerName(name: *const i8) -> *mut std::ffi::c_void;
            fn objc_msgSend();
        }
        unsafe {
            let sel = sel_registerName(name.as_ptr());
            let function: unsafe extern "C" fn(
                *mut std::ffi::c_void,
                *mut std::ffi::c_void,
                usize,
            ) -> *mut std::ffi::c_void = std::mem::transmute(objc_msgSend as *const ());
            function(receiver, sel, arg)
        }
    }

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
}

#[cfg(target_os = "macos")]
#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRetain(cf: *const std::ffi::c_void) -> *const std::ffi::c_void;
    fn CFRelease(cf: *const std::ffi::c_void);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(pid: u32, window: u64, control: u64) -> FocusTarget {
        FocusTarget::from_parts(pid, window, control, 9, "notepad")
    }

    #[test]
    fn same_window_and_control_pastes_without_a_restore() {
        let saved = target(40, 0x10, 0x11);
        let current = target(40, 0x10, 0x11);
        assert!(foreground_matches(&saved, &current));
        assert_eq!(plan(Some(&saved), Some(&current), 7, true), InsertPlan::Paste);
    }

    #[test]
    fn a_different_foreground_window_needs_a_restore() {
        let saved = target(40, 0x10, 0x11);
        let current = target(41, 0x20, 0x21);
        assert!(!foreground_matches(&saved, &current));
        assert_eq!(plan(Some(&saved), Some(&current), 7, true), InsertPlan::Restore);
    }

    #[test]
    fn the_same_window_with_a_different_control_needs_a_restore() {
        let saved = target(40, 0x10, 0x11);
        let current = target(40, 0x10, 0x99);
        assert_eq!(plan(Some(&saved), Some(&current), 7, true), InsertPlan::Restore);
    }

    #[test]
    fn our_own_window_is_not_a_paste_target() {
        let saved = target(7, 0x10, 0x11);
        let current = target(8, 0x20, 0x21);
        assert_eq!(plan(Some(&saved), Some(&current), 7, true), InsertPlan::CopyOnly);
        assert_eq!(plan(None, Some(&target(7, 1, 1)), 7, true), InsertPlan::CopyOnly);
        assert_eq!(plan(None, Some(&target(8, 1, 1)), 7, true), InsertPlan::Paste);
        assert_eq!(plan(None, Some(&target(8, 1, 1)), 7, false), InsertPlan::Paste);
    }

    #[test]
    fn the_log_line_names_the_window_and_not_typed_text() {
        let saved = FocusTarget::from_parts(42, 0x1000, 0x1001, 7, "notepad");
        let line = log_line("capture", &saved);
        assert!(line.contains("pid=42"));
        assert!(line.contains("window=0x1000"));
        assert!(line.contains("control=0x1001"));
        assert!(line.contains("thread=7"));
        assert!(line.contains("app=notepad"));
        assert!(!line.contains("secret phrase"));
        assert_eq!(notice().contains('\n'), false);
    }
}
