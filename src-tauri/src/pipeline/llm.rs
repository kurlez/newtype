#![allow(dead_code)]

//! LLM providers. PRD §7.2. Not implemented.

use super::ModuleStatus;

pub fn status() -> ModuleStatus {
    ModuleStatus {
        name: "llm",
        status: "stub",
        note: "Cloud polish and local llama.cpp are not linked.",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmProviderId {
    Cloud,
    LocalLlamaCpp,
    LocalMlx,
    OpenAiCompatLocal,
    None,
}

#[derive(Debug)]
pub struct LlmError;

impl std::fmt::Display for LlmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("language cleanup is not implemented in this build")
    }
}

impl std::error::Error for LlmError {}

pub trait LlmProvider {
    fn id(&self) -> LlmProviderId;
    fn complete(&self, prompt: &str) -> Result<String, LlmError>;
}
