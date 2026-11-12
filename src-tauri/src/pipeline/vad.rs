//! Silero VAD, used on the local path to drop leading, trailing, and long internal silence.
//!
//! The cloud path still sends the whole utterance. This module does not open a socket.

use std::path::Path;

use sherpa_onnx::{SileroVadModelConfig, VadModelConfig, VoiceActivityDetector};

use super::ModuleStatus;

pub fn status() -> ModuleStatus {
    ModuleStatus {
        name: "vad",
        status: "ready",
        note: "Silero VAD trims silence before local speech recognition. The model file is downloaded, not bundled.",
    }
}

pub fn trim_silence(model: &Path, samples: &[f32]) -> Result<Vec<f32>, String> {
    if samples.is_empty() {
        return Ok(Vec::new());
    }
    if !model.is_file() {
        return Err("Silero VAD is not installed yet. Download a local speech model to install it.".into());
    }
    let mut silero = SileroVadModelConfig::default();
    silero.model = Some(model.display().to_string());
    silero.threshold = 0.5;
    silero.min_silence_duration = 0.25;
    silero.min_speech_duration = 0.25;
    silero.window_size = 512;
    silero.max_speech_duration = 30.0;
    let config = VadModelConfig {
        silero_vad: silero,
        ten_vad: Default::default(),
        sample_rate: 16_000,
        num_threads: 1,
        provider: Some("cpu".into()),
        debug: false,
    };
    let buffer = samples.len() as f32 / 16_000.0 + 5.0;
    let vad = VoiceActivityDetector::create(&config, buffer)
        .ok_or_else(|| "Silero VAD could not be loaded.".to_string())?;
    const WINDOW: usize = 512;
    let mut speech = Vec::new();
    for chunk in samples.chunks(WINDOW) {
        vad.accept_waveform(chunk);
        while let Some(segment) = vad.front() {
            speech.extend_from_slice(segment.samples());
            vad.pop();
        }
    }
    vad.flush();
    while let Some(segment) = vad.front() {
        speech.extend_from_slice(segment.samples());
        vad.pop();
    }
    Ok(speech)
}

/// Keep only the given sample spans. Spans outside the buffer are clipped.
pub fn keep_spans(samples: &[f32], spans: &[(usize, usize)]) -> Vec<f32> {
    let mut kept = Vec::new();
    for (start, end) in spans {
        let start = (*start).min(samples.len());
        let end = (*end).min(samples.len()).max(start);
        kept.extend_from_slice(&samples[start..end]);
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_outside_speech_spans_is_dropped() {
        let samples = vec![0.0, 0.0, 0.8, -0.4, 0.0, 0.2];
        let kept = keep_spans(&samples, &[(2, 4), (5, 6)]);
        assert_eq!(kept, vec![0.8, -0.4, 0.2]);
        assert!(keep_spans(&samples, &[]).is_empty());
        assert_eq!(keep_spans(&samples, &[(10, 12)]), Vec::<f32>::new());
    }

    #[test]
    fn missing_model_file_is_an_error() {
        let err = trim_silence(Path::new("/no/such/silero_vad.onnx"), &[0.1, 0.2]).unwrap_err();
        assert!(err.contains("not installed"));
    }
}
