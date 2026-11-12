//! Global hotkeys. PRD §3.4 KEY-05. Not implemented.
//!
//! No hotkey is registered in this build. The factory defaults, when they
//! arrive, are Right Option on macOS and Right Ctrl on Windows, not Typeless's keys.

use super::ModuleStatus;

pub fn status() -> ModuleStatus {
    ModuleStatus {
        name: "hotkey",
        status: "stub",
        note: "No global hotkey is registered.",
    }
}
