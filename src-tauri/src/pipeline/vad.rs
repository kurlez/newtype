//! Voice activity detection. PRD §7.1. Not implemented.

use super::ModuleStatus;

pub fn status() -> ModuleStatus {
    ModuleStatus {
        name: "vad",
        status: "stub",
        note: "Silero VAD is not linked.",
    }
}
