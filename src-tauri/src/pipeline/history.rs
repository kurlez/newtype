//! On-device history. PRD §3.8. Not implemented.

use super::ModuleStatus;

pub fn status() -> ModuleStatus {
    ModuleStatus {
        name: "history",
        status: "stub",
        note: "Local history is not stored.",
    }
}
