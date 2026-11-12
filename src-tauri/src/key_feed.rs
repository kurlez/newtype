//! One keyboard path for the low-level hook and the settings-window fallback.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use crate::pipeline::hotkey::KeyEdge;

static CALLBACK: Mutex<Option<Box<dyn FnMut(KeyEdge) + Send>>> = Mutex::new(None);
static HOOK_ALIVE: AtomicBool = AtomicBool::new(false);
static FROM_WEBVIEW: AtomicBool = AtomicBool::new(false);

pub fn install(callback: impl FnMut(KeyEdge) + Send + 'static) {
    *CALLBACK.lock().expect("key feed") = Some(Box::new(callback));
}

pub fn from_webview() -> bool {
    FROM_WEBVIEW.load(Ordering::SeqCst)
}

/// `from_webview` is ignored once the native hook has delivered any key.
pub fn push(edge: KeyEdge, from_webview: bool) {
    if from_webview && HOOK_ALIVE.load(Ordering::SeqCst) {
        return;
    }
    if !from_webview {
        HOOK_ALIVE.store(true, Ordering::SeqCst);
    }
    let mut slot = CALLBACK.lock().expect("key feed");
    FROM_WEBVIEW.store(from_webview, Ordering::SeqCst);
    if let Some(callback) = slot.as_mut() {
        callback(edge);
    }
    FROM_WEBVIEW.store(false, Ordering::SeqCst);
}
