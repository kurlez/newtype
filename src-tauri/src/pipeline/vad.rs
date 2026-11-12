//! Voice activity detection. Not used on the cloud path.
//!
//! The cloud path sends the whole utterance. Silero stays a later local-path piece.

use super::ModuleStatus;

pub fn status() -> ModuleStatus {
    ModuleStatus {
        name: "vad",
        status: "stub",
        note: "Silero VAD is not linked. Cloud dictation sends the utterance captured between hotkey presses.",
    }
}
