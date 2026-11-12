#![allow(dead_code)]

//! ASR providers. PRD §7.2. Not implemented.

use super::ModuleStatus;

pub fn status() -> ModuleStatus {
    ModuleStatus {
        name: "asr",
        status: "stub",
        note: "Cloud ASR, sherpa-onnx 0.6B, and Mac MLX 1.7B are not linked.",
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsrProviderId {
    Cloud,
    /// Qwen3-ASR 0.6B int8 via sherpa-onnx. Windows default and Mac fallback.
    LocalQwen3Sherpa,
    /// Qwen3-ASR 1.7B via MLX. Mac only, after the M-S1 spike.
    LocalQwen3Mlx,
    LocalSenseVoice,
    LocalWhisperCpp,
    AppleSpeech,
}

#[derive(Debug, Clone)]
pub struct Transcript {
    pub text: String,
}

#[derive(Debug)]
pub struct AsrError;

impl std::fmt::Display for AsrError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("speech recognition is not implemented in this build")
    }
}

impl std::error::Error for AsrError {}

pub trait AsrProvider {
    fn id(&self) -> AsrProviderId;
    fn transcribe(&self, audio: &[u8]) -> Result<Transcript, AsrError>;
}
