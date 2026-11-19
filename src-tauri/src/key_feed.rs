//! One keyboard path for the low-level hook and the settings-window fallback.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use crate::pipeline::hotkey::KeyEdge;

pub enum Feed {
    Key(KeyEdge),
    /// Periodic resync. The hook and the event tap must not do this work.
    #[allow(dead_code)]
    Tick(u64),
}

static CALLBACK: Mutex<Option<Box<dyn FnMut(Feed) -> bool + Send>>> = Mutex::new(None);
static HOOK_ALIVE: AtomicBool = AtomicBool::new(false);
static FROM_WEBVIEW: AtomicBool = AtomicBool::new(false);

pub fn install(callback: impl FnMut(Feed) -> bool + Send + 'static) {
    *CALLBACK.lock().expect("key feed") = Some(Box::new(callback));
}

pub fn from_webview() -> bool {
    FROM_WEBVIEW.load(Ordering::SeqCst)
}

/// `from_webview` is ignored once the native hook has delivered any key.
/// The bool is true when the key must not reach the foreground app.
pub fn push(edge: KeyEdge, from_webview: bool) -> bool {
    deliver(Feed::Key(edge), from_webview)
}

#[allow(dead_code)]
pub fn tick(now_ms: u64) {
    let mut slot = CALLBACK.lock().expect("key feed");
    if let Some(callback) = slot.as_mut() {
        let _ = callback(Feed::Tick(now_ms));
    }
}

fn deliver(feed: Feed, from_webview: bool) -> bool {
    if from_webview && HOOK_ALIVE.load(Ordering::SeqCst) {
        return false;
    }
    if !from_webview {
        if let Feed::Key(KeyEdge::Down(_)) = &feed {
            HOOK_ALIVE.store(true, Ordering::SeqCst);
        }
    }
    let mut slot = CALLBACK.lock().expect("key feed");
    FROM_WEBVIEW.store(from_webview, Ordering::SeqCst);
    let suppress = slot.as_mut().is_some_and(|callback| callback(feed));
    FROM_WEBVIEW.store(false, Ordering::SeqCst);
    suppress
}
