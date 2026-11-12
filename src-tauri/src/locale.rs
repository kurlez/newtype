//! UI language for strings that are chosen in Rust (tray, focus notice).

pub fn prefers_zh() -> bool {
    tag_is_zh(&system_tag())
}

pub fn tag_is_zh(tag: &str) -> bool {
    let tag = tag.trim().to_ascii_lowercase().replace('_', "-");
    let primary = tag.split(['-', '.']).next().unwrap_or("");
    primary == "zh"
}

pub fn text(en: &'static str, zh: &'static str) -> &'static str {
    if prefers_zh() { zh } else { en }
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
    fn chinese_tags_are_zh_and_english_tags_are_not() {
        assert!(tag_is_zh("zh-CN"));
        assert!(tag_is_zh("zh_Hans"));
        assert!(tag_is_zh("ZH"));
        assert!(!tag_is_zh("en-US"));
        assert!(!tag_is_zh("zulu"));
        assert!(!tag_is_zh(""));
    }
}
