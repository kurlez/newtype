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
}

pub fn snapshot(microphone: &str) -> PermissionSnapshot {
    PermissionSnapshot {
        microphone: microphone.to_string(),
        accessibility: accessibility_label(),
        platform: std::env::consts::OS.to_string(),
        guidance: guidance(),
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
    match std::env::consts::OS {
        "macos" => concat!(
            "Allow Microphone and Accessibility for newtype. ",
            "Accessibility is required to hear the Right Control hotkey and to paste into the focused app. ",
            "Some macOS versions also ask for Input Monitoring; if the hotkey does nothing after Accessibility is on, enable that too. ",
            "System Settings → Privacy & Security. After an update, macOS may ask again because this build is not Developer ID signed."
        )
        .into(),
        "windows" => concat!(
            "Allow microphone access if Windows asks. Paste uses Ctrl+V into the focused app. ",
            "If SmartScreen appeared at install, More info → Run anyway."
        )
        .into(),
        _ => "Microphone capture, global hotkeys, and paste are implemented for macOS and Windows.".into(),
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
