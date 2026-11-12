//! Speech providers. Cloud adapters live in `cloud`. Local runtimes are not linked.

use super::ModuleStatus;

pub fn status() -> ModuleStatus {
    ModuleStatus {
        name: "asr",
        status: "ready",
        note: "Cloud speech is on for OpenAI, Groq, DashScope, and custom endpoints. Volcengine speech and local Qwen are not linked.",
    }
}

pub trait AsrProvider {
    fn id(&self) -> &'static str;
    fn transcribe(&self, wav: &[u8]) -> Result<String, String>;
}

pub struct LocalAsrUnavailable;

impl AsrProvider for LocalAsrUnavailable {
    fn id(&self) -> &'static str {
        "local"
    }

    fn transcribe(&self, _wav: &[u8]) -> Result<String, String> {
        Err("Local speech recognition is not in this build.".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_asr_refuses_instead_of_sending_audio() {
        let _ = LocalAsrUnavailable.id();
        let err = LocalAsrUnavailable.transcribe(b"wav").unwrap_err();
        assert!(err.contains("not in this build"));
    }
}
