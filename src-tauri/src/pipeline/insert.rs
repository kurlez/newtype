//! Text insertion. PRD §3.12 INS-01. Not implemented.
//!
//! This build does not touch the clipboard or accessibility APIs.

use super::ModuleStatus;

pub fn status() -> ModuleStatus {
    ModuleStatus {
        name: "insert",
        status: "stub",
        note: "Cursor insertion and the clipboard fallback are not implemented.",
    }
}
