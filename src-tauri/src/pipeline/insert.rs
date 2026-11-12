//! Insert text into the focused app by pasting, then restore the clipboard.

use std::thread;
use std::time::Duration;

use super::ModuleStatus;

pub fn status() -> ModuleStatus {
    ModuleStatus {
        name: "insert",
        status: "ready",
        note: "Text is pasted into the focused app and the previous clipboard text is put back.",
    }
}

pub fn copy_text(text: &str) -> Result<(), String> {
    clipboard_set(text)
}

pub fn insert_text(text: &str) -> Result<(), String> {
    if text.is_empty() {
        return Ok(());
    }
    let previous = clipboard_get();
    clipboard_set(text)?;
    thread::sleep(Duration::from_millis(30));
    paste_shortcut()?;
    thread::sleep(Duration::from_millis(140));
    if let Some(previous) = previous {
        let _ = clipboard_set(&previous);
    }
    Ok(())
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn clipboard_get() -> Option<String> {
    arboard::Clipboard::new()
        .and_then(|mut clipboard| clipboard.get_text())
        .ok()
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn clipboard_set(text: &str) -> Result<(), String> {
    let mut clipboard =
        arboard::Clipboard::new().map_err(|err| format!("Clipboard is unavailable: {err}"))?;
    clipboard
        .set_text(text)
        .map_err(|err| format!("Could not copy the text: {err}"))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn clipboard_get() -> Option<String> {
    None
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn clipboard_set(_text: &str) -> Result<(), String> {
    Err("Clipboard paste is implemented for macOS and Windows.".into())
}

fn paste_shortcut() -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let modifier = rdev::Key::MetaLeft;
    #[cfg(not(target_os = "macos"))]
    let modifier = rdev::Key::ControlLeft;
    press(modifier)?;
    press(rdev::Key::KeyV)?;
    release(rdev::Key::KeyV)?;
    release(modifier)?;
    Ok(())
}

fn press(key: rdev::Key) -> Result<(), String> {
    rdev::simulate(&rdev::EventType::KeyPress(key)).map_err(|err| format!("Could not press {key:?}: {err:?}"))
}

fn release(key: rdev::Key) -> Result<(), String> {
    rdev::simulate(&rdev::EventType::KeyRelease(key))
        .map_err(|err| format!("Could not release {key:?}: {err:?}"))
}
