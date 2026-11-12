//! Dictate pipeline.
//!
//! Cloud speech, rewrite, hotkeys, paste, and history run in this build.
//! Local models, Silero, and offline auto-switch stay behind the router.

pub(crate) mod asr;
pub(crate) mod audio;
pub(crate) mod cloud;
pub(crate) mod front;
pub(crate) mod history;
pub(crate) mod hotkey;
pub(crate) mod hotwords;
pub(crate) mod insert;
pub(crate) mod llm;
pub(crate) mod polish;
pub(crate) mod router;
pub(crate) mod vad;

#[cfg(test)]
mod harness;

pub struct ModuleStatus {
    pub name: &'static str,
    pub status: &'static str,
    pub note: &'static str,
}

pub fn inventory() -> Vec<ModuleStatus> {
    vec![
        asr::status(),
        vad::status(),
        hotwords::status(),
        llm::status(),
        router::status(),
        hotkey::status(),
        insert::status(),
        history::status(),
        audio::status(),
    ]
}
