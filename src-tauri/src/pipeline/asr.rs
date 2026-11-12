//! Speech providers. Cloud adapters live in `cloud`. Local Qwen3-ASR lives in `engines`.

use super::ModuleStatus;

pub fn status() -> ModuleStatus {
    ModuleStatus {
        name: "asr",
        status: "ready",
        note: "Cloud speech is on for OpenAI, Groq, DashScope, and custom endpoints. Local speech is Qwen3-ASR 1.7B on 16 GB Macs and 0.6B int8 elsewhere. Volcengine speech is not linked.",
    }
}

pub trait AsrProvider {
    #[allow(dead_code)]
    fn id(&self) -> &'static str;
    fn transcribe(&self, wav: &[u8]) -> Result<String, String>;
}
