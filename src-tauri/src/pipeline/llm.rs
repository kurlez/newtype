//! Language-model status. The polish module owns the prompt and the fallback.

use super::ModuleStatus;

pub fn status() -> ModuleStatus {
    ModuleStatus {
        name: "llm",
        status: "ready",
        note: "Cloud rewrite is on. Local rewrite uses llama.cpp with thinking off. temperature is 0 on both paths.",
    }
}
