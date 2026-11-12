//! Hot-word replacement before the LLM. PRD §3.7. Not implemented.

use super::ModuleStatus;

pub fn status() -> ModuleStatus {
    ModuleStatus {
        name: "hotwords",
        status: "stub",
        note: "Dictionary and phonetic replacement are not implemented.",
    }
}
