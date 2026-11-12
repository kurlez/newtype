//! Language-model status. The polish module owns the prompt and the fallback.

use super::ModuleStatus;

pub fn status() -> ModuleStatus {
    ModuleStatus {
        name: "llm",
        status: "ready",
        note: "Cloud rewrite is on. temperature 0, and thinking is turned off where the model allows it. Local llama.cpp is not linked.",
    }
}
