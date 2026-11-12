//! Dictate pipeline boundaries from PRD §7.
//!
//! Milestone 0 does not record, transcribe, rewrite, or insert text.
//! These modules exist so later work has a place to land.

mod asr;
mod history;
mod hotkey;
mod hotwords;
mod insert;
mod llm;
mod router;
mod vad;

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
    ]
}
