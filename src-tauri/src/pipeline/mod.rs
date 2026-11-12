//! Dictate pipeline.
//!
//! Auto prefers the configured cloud provider and falls back to local models
//! when the probe says it is unreachable. Strict offline is enforced in
//! `egress` for cloud calls, probes, model downloads, and update checks.

pub(crate) mod asr;
pub(crate) mod asr_text;
pub(crate) mod audio;
pub(crate) mod catalog;
pub(crate) mod checksum;
pub(crate) mod codesign;
pub(crate) mod cloud;
pub(crate) mod download;
pub(crate) mod egress;
pub(crate) mod engines;
pub(crate) mod extract;
pub(crate) mod front;
pub(crate) mod history;
pub(crate) mod hotkey;
pub(crate) mod hotwords;
pub(crate) mod insert;
pub(crate) mod library;
pub(crate) mod llm;
pub(crate) mod local_run;
pub(crate) mod netwatch;
pub(crate) mod polish;
pub(crate) mod probe;
pub(crate) mod rewrite;
pub(crate) mod router;
pub(crate) mod selection;
pub(crate) mod session;
pub(crate) mod task;
pub(crate) mod vad;
pub(crate) mod volcengine;

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
        library_status(),
    ]
}

fn library_status() -> ModuleStatus {
    ModuleStatus {
        name: "library",
        status: "ready",
        note: "Local Qwen3-ASR and Qwen3 rewrite models download on demand. Checksums are verified before a model is marked installed.",
    }
}
