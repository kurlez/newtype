//! UI language for strings that are chosen in Rust (tray, focus notice).

use std::sync::atomic::{AtomicU8, Ordering};

/// 0 = follow the system, 1 = English, 2 = Chinese.
static PREFERENCE: AtomicU8 = AtomicU8::new(0);

pub fn set_preference(value: &str) {
    let code = match value {
        "en" => 1,
        "zh" => 2,
        _ => 0,
    };
    PREFERENCE.store(code, Ordering::Relaxed);
}

pub fn prefers_zh() -> bool {
    match PREFERENCE.load(Ordering::Relaxed) {
        1 => false,
        2 => true,
        _ => tag_is_zh(&system_tag()),
    }
}

pub fn resolved_tag() -> &'static str {
    if prefers_zh() { "zh" } else { "en" }
}

pub fn tag_is_zh(tag: &str) -> bool {
    let tag = tag.trim().to_ascii_lowercase().replace('_', "-");
    let primary = tag.split(['-', '.']).next().unwrap_or("");
    primary == "zh"
}

pub fn text(en: &'static str, zh: &'static str) -> &'static str {
    if prefers_zh() { zh } else { en }
}

pub fn pick(en: String, zh: String) -> String {
    if prefers_zh() { zh } else { en }
}

/// A string is bilingual when it has Han characters and a Latin word that is
/// not a product token. "Local · manual · 本地 · 手动" matches. "DashScope" alone does not.
pub fn mixes_languages(value: &str) -> bool {
    let han = value.chars().any(|ch| ('\u{4e00}'..='\u{9fff}').contains(&ch));
    if !han {
        return false;
    }
    let mut word = String::new();
    let mut words = Vec::new();
    for ch in value.chars() {
        if ch.is_ascii_alphabetic() {
            word.push(ch.to_ascii_lowercase());
        } else if !word.is_empty() {
            words.push(std::mem::take(&mut word));
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    const ALLOW: &[&str] = &[
        "api", "gpt", "json", "url", "qwen", "llama", "cpp", "openai", "groq", "macos", "windows",
        "ctrl", "alt", "app", "id", "key", "mica", "acrylic",
    ];
    words.into_iter().any(|word| word.len() >= 3 && !ALLOW.contains(&word.as_str()))
}

fn system_tag() -> String {
    #[cfg(target_os = "windows")]
    {
        return windows_tag();
    }
    #[cfg(target_os = "macos")]
    {
        return macos_tag();
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        std::env::var("LC_ALL")
            .or_else(|_| std::env::var("LANG"))
            .unwrap_or_default()
    }
}

#[cfg(target_os = "windows")]
fn windows_tag() -> String {
    use windows_sys::Win32::Globalization::GetUserDefaultUILanguage;
    let lang = unsafe { GetUserDefaultUILanguage() };
    // LANGID primary language id is the low 10 bits. Chinese is 0x04.
    if lang & 0x3FF == 0x04 {
        "zh".to_string()
    } else {
        "en".to_string()
    }
}

#[cfg(target_os = "macos")]
fn macos_tag() -> String {
    unsafe {
        let locale = objc_class(c"NSLocale");
        if locale.is_null() {
            return String::new();
        }
        let preferred = msg0(locale, c"preferredLanguages");
        if preferred.is_null() {
            return String::new();
        }
        let first = msg0(preferred, c"firstObject");
        if first.is_null() {
            return String::new();
        }
        ns_string(first)
    }
}

#[cfg(target_os = "macos")]
fn objc_class(name: &std::ffi::CStr) -> *mut std::ffi::c_void {
    #[link(name = "objc", kind = "dylib")]
    extern "C" {
        fn objc_getClass(name: *const i8) -> *mut std::ffi::c_void;
    }
    unsafe { objc_getClass(name.as_ptr()) }
}

#[cfg(target_os = "macos")]
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

#[cfg(target_os = "macos")]
fn ns_string(value: *mut std::ffi::c_void) -> String {
    #[link(name = "objc", kind = "dylib")]
    extern "C" {
        fn sel_registerName(name: *const i8) -> *mut std::ffi::c_void;
        fn objc_msgSend();
    }
    unsafe {
        let sel = sel_registerName(c"UTF8String".as_ptr());
        let function: unsafe extern "C" fn(
            *mut std::ffi::c_void,
            *mut std::ffi::c_void,
        ) -> *const i8 = std::mem::transmute(objc_msgSend as *const ());
        let ptr = function(value, sel);
        if ptr.is_null() {
            return String::new();
        }
        std::ffi::CStr::from_ptr(ptr).to_string_lossy().into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_table_has_both_languages_and_no_bilingual_entry() {
        let table = [
            ("history", "History", "历史"),
            ("settings", "Settings", "设置"),
            ("copy", "Copy", "复制"),
            ("insert", "Insert again", "再次插入"),
            ("openHistory", "Open in History", "在历史中打开"),
            ("pathCloud", "Cloud", "云端"),
            ("pathLocalAuto", "Local, automatic", "本地自动"),
            ("pathLocal", "Local", "本地"),
            ("pathOffline", "Strict offline", "严格离线"),
            ("fallbackReady", "Local fallback: ready", "本地回落：就绪"),
            ("fallbackRewrite", "Local fallback: rewrite model missing", "本地回落：缺少润色模型"),
            ("fallbackMissing", "Local fallback: not ready", "本地回落：未就绪"),
            (
                "rulesOnly",
                "Local polish model is not installed, so only rule cleanup ran.",
                "本地润色模型未安装，只做了规则清理",
            ),
            ("recording", "Recording", "录音中"),
            ("processing", "Processing", "处理中"),
            ("inserted", "Inserted", "已插入"),
            ("idle", "Idle", "空闲"),
            ("cancel", "Cancel", "取消"),
        ];
        let mut keys = std::collections::BTreeSet::new();
        for (key, en, zh) in table {
            assert!(keys.insert(key), "duplicate key {key}");
            assert!(!en.is_empty() && !zh.is_empty(), "{key}");
            assert!(!mixes_languages(en), "english {key} mixes languages: {en}");
            assert!(!mixes_languages(zh), "chinese {key} mixes languages: {zh}");
        }
        assert!(mixes_languages("Local · manual · 本地 · 手动"));
    }

    fn chinese_tags_are_zh_and_english_tags_are_not() {
        assert!(tag_is_zh("zh-CN"));
        assert!(tag_is_zh("zh_Hans"));
        assert!(tag_is_zh("ZH"));
        assert!(!tag_is_zh("en-US"));
        assert!(!tag_is_zh("zulu"));
        assert!(!tag_is_zh(""));
    }
}
