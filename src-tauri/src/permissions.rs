//! Microphone and Accessibility prompts.
//!
//! macOS needs the microphone usage string in Info.plist, Accessibility for the
//! hotkey tap and for pasting, and on some versions Input Monitoring as well.
//! Windows asks for the microphone the first time it is opened.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionSnapshot {
    pub microphone: String,
    pub accessibility: String,
    pub platform: String,
    pub guidance: String,
    /// `granted`, `denied`, `restricted`, `notDetermined`, or `unknown`.
    pub microphone_state: String,
    /// `granted`, `required`, or `notApplicable`.
    pub accessibility_state: String,
    /// Empty when nothing is missing.
    pub prompt: String,
    /// `microphone`, `accessibility`, or `both`.
    pub prompt_action: String,
}

pub fn snapshot(microphone: &str) -> PermissionSnapshot {
    let microphone_state = microphone_state(microphone);
    let accessibility_state = accessibility_state();
    let (prompt, prompt_action) = access_prompt(std::env::consts::OS, &microphone_state, &accessibility_state);
    PermissionSnapshot {
        microphone: microphone.to_string(),
        accessibility: accessibility_label(),
        platform: std::env::consts::OS.to_string(),
        guidance: guidance(),
        microphone_state,
        accessibility_state,
        prompt,
        prompt_action,
    }
}

pub fn microphone_state(reported: &str) -> String {
    if reported.starts_with("unavailable") || reported.contains("denied") || reported.contains("blocked") {
        return "denied".into();
    }
    match std::env::consts::OS {
        "macos" => macos_microphone_state(),
        "windows" => windows_microphone_state(),
        _ => "unknown".into(),
    }
}

pub fn access_prompt(platform: &str, microphone: &str, accessibility: &str) -> (String, String) {
    let mic_missing = matches!(microphone, "denied" | "restricted" | "notDetermined");
    let ax_missing = platform == "macos" && accessibility != "granted";
    let zh = crate::locale::prefers_zh();
    match (mic_missing, ax_missing) {
        (true, true) => (
            if zh {
                "麦克风和辅助功能都关着。口授听不到声音，快捷键和粘贴也会失败。更新后系统常会再次询问，因为这个构建没有开发者签名。"
            } else {
                "Microphone and Accessibility are off. Dictation cannot hear you, and hotkeys or paste will fail. macOS often asks again after an update because this build is not Developer ID signed."
            }
            .into(),
            "both".into(),
        ),
        (true, false) if platform == "windows" => (
            if zh {
                "系统正在拦截麦克风。请打开麦克风隐私设置并允许 newtype。在此之前不会录音。"
            } else {
                "Windows is blocking the microphone. Open microphone privacy settings and allow newtype. Audio is not recorded until you do."
            }
            .into(),
            "microphone".into(),
        ),
        (true, false) => (
            if zh {
                "麦克风权限关着。newtype 听不到你。更新后系统可能再次询问，因为这个构建没有开发者签名。"
            } else {
                "Microphone access is off. newtype cannot hear you. After an update, macOS may ask again because this build is not Developer ID signed."
            }
            .into(),
            "microphone".into(),
        ),
        (false, true) => (
            if zh {
                "辅助功能关着。快捷键和粘贴需要它。更新后系统可能忘掉这项权限，因为这个构建没有开发者签名。"
            } else {
                "Accessibility is off. The hotkey and paste need it. After an update, macOS may forget this permission because the build is not Developer ID signed."
            }
            .into(),
            "accessibility".into(),
        ),
        _ => (String::new(), String::new()),
    }
}

pub fn av_status_name(code: i64) -> &'static str {
    match code {
        0 => "notDetermined",
        1 => "restricted",
        2 => "denied",
        3 => "granted",
        _ => "unknown",
    }
}

pub fn windows_consent(global: Option<&str>, app: Option<&str>) -> &'static str {
    let denied = |value: Option<&str>| value.is_some_and(|value| value.eq_ignore_ascii_case("Deny"));
    if denied(global) || denied(app) {
        return "denied";
    }
    let allow = |value: Option<&str>| value.is_some_and(|value| value.eq_ignore_ascii_case("Allow"));
    if allow(global) || allow(app) {
        "granted"
    } else {
        "unknown"
    }
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub fn accessibility_granted() -> bool {
    #[cfg(target_os = "macos")]
    {
        return mac::trusted();
    }
    #[cfg(not(target_os = "macos"))]
    {
        true
    }
}

pub fn prompt_accessibility() -> Result<bool, String> {
    #[cfg(target_os = "macos")]
    {
        return Ok(mac::prompt());
    }
    #[cfg(not(target_os = "macos"))]
    {
        Err("Accessibility is a macOS permission. Windows paste does not use it.".into())
    }
}

pub fn open_privacy(kind: &str) -> Result<(), String> {
    let url = match (std::env::consts::OS, kind) {
        ("macos", "microphone") => {
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone"
        }
        ("macos", "accessibility") => {
            "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
        }
        ("macos", "input") => {
            "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent"
        }
        ("windows", _) => "ms-settings:privacy-microphone",
        _ => {
            return Err(
                "System privacy settings are only opened on macOS and Windows.".into(),
            )
        }
    };
    open_url(url)
}

fn accessibility_state() -> String {
    match std::env::consts::OS {
        "macos" => {
            if accessibility_granted() {
                "granted".into()
            } else {
                "required".into()
            }
        }
        "windows" => "notApplicable".into(),
        _ => "notApplicable".into(),
    }
}

fn macos_microphone_state() -> String {
    #[cfg(target_os = "macos")]
    {
        return av_status_name(mac_av::authorization_status()).into();
    }
    #[cfg(not(target_os = "macos"))]
    {
        "unknown".into()
    }
}

fn windows_microphone_state() -> String {
    #[cfg(target_os = "windows")]
    {
        let global = win::consent(None);
        let app = win::consent(Some(&win::exe_key()));
        return windows_consent(global.as_deref(), app.as_deref()).into();
    }
    #[cfg(not(target_os = "windows"))]
    {
        "unknown".into()
    }
}

fn accessibility_label() -> String {
    #[cfg(target_os = "macos")]
    {
        if accessibility_granted() {
            "granted".into()
        } else {
            "required".into()
        }
    }
    #[cfg(target_os = "windows")]
    {
        "not required for paste".into()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        "not available on this machine".into()
    }
}

fn guidance() -> String {
    let zh = crate::locale::prefers_zh();
    match std::env::consts::OS {
        "macos" => {
            if zh {
                "请允许 newtype 使用麦克风和辅助功能。辅助功能用来监听右侧 Ctrl 快捷键，并粘贴到当前应用。有的系统版本还会要求输入监视；如果辅助功能已开但快捷键没反应，也请打开它。位置在系统设置的隐私与安全性。更新后可能会再次询问，因为这个构建没有开发者签名。"
            } else {
                "Allow Microphone and Accessibility for newtype. Accessibility is required to hear the Right Control hotkey and to paste into the focused app. Some macOS versions also ask for Input Monitoring; if the hotkey does nothing after Accessibility is on, enable that too. System Settings → Privacy & Security. After an update, macOS may ask again because this build is not Developer ID signed."
            }
            .into()
        }
        "windows" => {
            if zh {
                "如果系统询问，请允许麦克风。粘贴使用 Ctrl+V 写入当前应用。安装时如果出现安全提示，选择更多信息，然后仍要运行。"
            } else {
                "Allow microphone access if Windows asks. Paste uses Ctrl+V into the focused app. If SmartScreen appeared at install, More info → Run anyway."
            }
            .into()
        }
        _ => crate::locale::text(
            "Microphone capture, global hotkeys, and paste are implemented for macOS and Windows.",
            "麦克风采集、全局快捷键和粘贴只在 macOS 和 Windows 上实现。",
        )
        .into(),
    }
}

fn open_url(url: &str) -> Result<(), String> {
    let mut command = if cfg!(target_os = "windows") {
        let mut command = std::process::Command::new("cmd");
        command.args(["/C", "start", "", url]);
        command
    } else if cfg!(target_os = "macos") {
        let mut command = std::process::Command::new("open");
        command.arg(url);
        command
    } else {
        return Err("Cannot open system settings on this machine.".into());
    };
    let status = command
        .status()
        .map_err(|err| format!("Could not open system settings: {err}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("System settings did not open ({status})."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_microphone_and_accessibility_name_the_right_pane() {
        let (message, action) = access_prompt("macos", "denied", "required");
        assert_eq!(action, "both");
        assert!(message.contains("Microphone") || message.contains("麦克风"));
        assert!(message.contains("Accessibility") || message.contains("辅助功能"));
        assert!(message.contains("not Developer ID signed") || message.contains("开发者签名"));
        let (message, action) = access_prompt("macos", "granted", "required");
        assert_eq!(action, "accessibility");
        assert!(message.contains("Accessibility") || message.contains("辅助功能"));
        let (message, action) = access_prompt("windows", "denied", "notApplicable");
        assert_eq!(action, "microphone");
        assert!(message.contains("Windows") || message.contains("麦克风"));
        assert!(access_prompt("linux", "unknown", "notApplicable").0.is_empty());
    }

    #[test]
    fn windows_consent_treats_deny_as_a_block() {
        assert_eq!(windows_consent(Some("Allow"), Some("Deny")), "denied");
        assert_eq!(windows_consent(Some("Deny"), None), "denied");
        assert_eq!(windows_consent(Some("Allow"), None), "granted");
        assert_eq!(windows_consent(None, None), "unknown");
        assert_eq!(av_status_name(2), "denied");
        assert_eq!(av_status_name(3), "granted");
        assert_eq!(av_status_name(0), "notDetermined");
    }
}

#[cfg(target_os = "macos")]
mod mac_av {
    use core_foundation::base::TCFType;

    pub fn authorization_status() -> i64 {
        #[link(name = "AVFoundation", kind = "framework")]
        extern "C" {}
        #[link(name = "objc", kind = "dylib")]
        extern "C" {
            fn objc_getClass(name: *const i8) -> *mut std::ffi::c_void;
            fn sel_registerName(name: *const i8) -> *mut std::ffi::c_void;
            fn objc_msgSend();
        }
        unsafe {
            let class = objc_getClass(c"AVCaptureDevice".as_ptr());
            if class.is_null() {
                return -1;
            }
            let selector = sel_registerName(c"authorizationStatusForMediaType:".as_ptr());
            let media = core_foundation::string::CFString::new("soun");
            let function: unsafe extern "C" fn(
                *mut std::ffi::c_void,
                *mut std::ffi::c_void,
                *const std::ffi::c_void,
            ) -> isize = std::mem::transmute(objc_msgSend as *const ());
            function(class, selector, media.as_concrete_TypeRef() as *const std::ffi::c_void) as i64
        }
    }
}

#[cfg(target_os = "windows")]
mod win {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY_CURRENT_USER, KEY_READ, REG_SZ,
    };

    pub fn exe_key() -> String {
        std::env::current_exe()
            .map(|path| path.display().to_string().replace('\\', "#"))
            .unwrap_or_default()
    }

    pub fn consent(app: Option<&str>) -> Option<String> {
        let mut path = String::from(
            "Software\\Microsoft\\Windows\\CurrentVersion\\CapabilityAccessManager\\ConsentStore\\microphone",
        );
        if let Some(app) = app.filter(|value| !value.is_empty()) {
            path.push_str("\\NonPackaged\\");
            path.push_str(app);
        }
        let wide: Vec<u16> = path.encode_utf16().chain(std::iter::once(0)).collect();
        let mut key = std::ptr::null_mut();
        let opened = unsafe { RegOpenKeyExW(HKEY_CURRENT_USER, wide.as_ptr(), 0, KEY_READ, &mut key) };
        if opened != ERROR_SUCCESS {
            return None;
        }
        let name: Vec<u16> = "Value".encode_utf16().chain(std::iter::once(0)).collect();
        let mut kind = 0u32;
        let mut size = 0u32;
        let queried = unsafe {
            RegQueryValueExW(key, name.as_ptr(), std::ptr::null(), &mut kind, std::ptr::null_mut(), &mut size)
        };
        if queried != ERROR_SUCCESS || kind != REG_SZ || size < 2 {
            unsafe { RegCloseKey(key) };
            return None;
        }
        let mut buffer = vec![0u16; (size as usize / 2) + 1];
        let queried = unsafe {
            RegQueryValueExW(
                key,
                name.as_ptr(),
                std::ptr::null(),
                &mut kind,
                buffer.as_mut_ptr() as *mut u8,
                &mut size,
            )
        };
        unsafe { RegCloseKey(key) };
        if queried != ERROR_SUCCESS {
            return None;
        }
        let end = buffer.iter().position(|unit| *unit == 0).unwrap_or(buffer.len());
        Some(String::from_utf16_lossy(&buffer[..end]))
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use core_foundation::base::TCFType;
    use core_foundation::boolean::CFBoolean;
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::string::CFString;

    #[link(name = "ApplicationServices", kind = "framework")]
    extern "C" {
        fn AXIsProcessTrusted() -> u8;
        fn AXIsProcessTrustedWithOptions(
            options: core_foundation::dictionary::CFDictionaryRef,
        ) -> u8;
        static kAXTrustedCheckOptionPrompt: core_foundation::string::CFStringRef;
    }

    pub fn trusted() -> bool {
        unsafe { AXIsProcessTrusted() != 0 }
    }

    pub fn prompt() -> bool {
        unsafe {
            let key = CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt);
            let value = CFBoolean::true_value();
            let options =
                CFDictionary::from_CFType_pairs(&[(key.as_CFType(), value.as_CFType())]);
            AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef()) != 0
        }
    }
}
