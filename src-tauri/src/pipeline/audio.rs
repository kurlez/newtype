//! Microphone capture and 16 kHz mono WAV encoding.
//!
//! The whole utterance is kept until the hotkey stops it. There is no
//! streaming transcript in this build.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};

use super::ModuleStatus;

pub fn status() -> ModuleStatus {
    ModuleStatus {
        name: "audio",
        status: "ready",
        note: "The default input device records until the hotkey stops. Silero VAD is not linked.",
    }
}

const MAX_SECONDS: u32 = 9 * 60;
const TARGET_RATE: u32 = 16_000;

#[derive(Debug, Clone)]
pub struct AudioClip {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

struct SharedAudio {
    samples: Mutex<Vec<f32>>,
    sample_rate: u32,
    level: AtomicU32,
    capped: AtomicBool,
}

pub struct MicSession {
    _stream: cpal::Stream,
    shared: Arc<SharedAudio>,
}

#[derive(Clone)]
pub struct LevelWatch {
    shared: Arc<SharedAudio>,
}

impl LevelWatch {
    pub fn level(&self) -> f32 {
        self.shared.level.load(Ordering::Relaxed) as f32 / 1000.0
    }

    pub fn capped(&self) -> bool {
        self.shared.capped.load(Ordering::Relaxed)
    }
}

pub fn input_devices() -> Result<Vec<String>, String> {
    let host = cpal::default_host();
    let devices = host
        .input_devices()
        .map_err(|err| format!("Could not list microphones: {err}"))?;
    let mut names = Vec::new();
    for device in devices {
        if let Ok(name) = device.name() {
            if !name.is_empty() {
                names.push(name);
            }
        }
    }
    Ok(names)
}

impl MicSession {
    pub fn start(preferred_name: &str) -> Result<Self, String> {
        let host = cpal::default_host();
        let device = if preferred_name.trim().is_empty() {
            host.default_input_device()
        } else {
            host.input_devices()
                .map_err(|err| format!("Could not list microphones: {err}"))?
                .find(|device| device.name().ok().as_deref() == Some(preferred_name.trim()))
                .or_else(|| host.default_input_device())
        }
        .ok_or_else(|| "No microphone was found.".to_string())?;
        let supported = device
            .default_input_config()
            .map_err(|err| format!("Could not open the microphone: {err}"))?;
        let sample_rate = supported.sample_rate().0;
        let channels = supported.channels() as usize;
        let shared = Arc::new(SharedAudio {
            samples: Mutex::new(Vec::new()),
            sample_rate,
            level: AtomicU32::new(0),
            capped: AtomicBool::new(false),
        });
        let stream = build_stream(&device, &supported, channels, Arc::clone(&shared))?;
        stream
            .play()
            .map_err(|err| format!("Could not start the microphone: {err}"))?;
        Ok(Self {
            _stream: stream,
            shared,
        })
    }

    pub fn watch(&self) -> LevelWatch {
        LevelWatch {
            shared: Arc::clone(&self.shared),
        }
    }

    pub fn level(&self) -> f32 {
        self.watch().level()
    }

    pub fn stop(self) -> AudioClip {
        drop(self._stream);
        let samples = std::mem::take(&mut *self.shared.samples.lock().expect("audio buffer"));
        AudioClip {
            samples,
            sample_rate: self.shared.sample_rate,
        }
    }
}

fn build_stream(
    device: &cpal::Device,
    supported: &cpal::SupportedStreamConfig,
    channels: usize,
    shared: Arc<SharedAudio>,
) -> Result<cpal::Stream, String> {
    let config: cpal::StreamConfig = supported.clone().into();
    let stream = match supported.sample_format() {
        cpal::SampleFormat::F32 => {
            let shared = Arc::clone(&shared);
            device.build_input_stream(
                &config,
                move |data: &[f32], _| push_frames(data, channels, &shared),
                log_mic_error,
                None,
            )
        }
        cpal::SampleFormat::I16 => {
            let shared = Arc::clone(&shared);
            device.build_input_stream(
                &config,
                move |data: &[i16], _| {
                    let converted: Vec<f32> = data
                        .iter()
                        .map(|sample| *sample as f32 / i16::MAX as f32)
                        .collect();
                    push_frames(&converted, channels, &shared);
                },
                log_mic_error,
                None,
            )
        }
        cpal::SampleFormat::U16 => {
            let shared = Arc::clone(&shared);
            device.build_input_stream(
                &config,
                move |data: &[u16], _| {
                    let converted: Vec<f32> = data
                        .iter()
                        .map(|sample| (*sample as f32 / u16::MAX as f32) * 2.0 - 1.0)
                        .collect();
                    push_frames(&converted, channels, &shared);
                },
                log_mic_error,
                None,
            )
        }
        other => {
            return Err(format!("This microphone format is not supported ({other:?})."));
        }
    };
    stream.map_err(|err| format!("Could not start the microphone: {err}"))
}

fn log_mic_error(error: cpal::StreamError) {
    eprintln!("newtype microphone error: {error}");
}

fn push_frames(data: &[f32], channels: usize, shared: &SharedAudio) {
    let channels = channels.max(1);
    let mut guard = match shared.samples.lock() {
        Ok(guard) => guard,
        Err(_) => return,
    };
    let cap = shared.sample_rate.saturating_mul(MAX_SECONDS) as usize;
    if guard.len() >= cap {
        shared.capped.store(true, Ordering::Relaxed);
        return;
    }
    let mut sum_sq = 0.0f32;
    let mut count = 0usize;
    for frame in data.chunks(channels) {
        if guard.len() >= cap {
            shared.capped.store(true, Ordering::Relaxed);
            break;
        }
        let sample = frame.iter().sum::<f32>() / channels as f32;
        sum_sq += sample * sample;
        count += 1;
        guard.push(sample);
    }
    if count > 0 {
        let rms = (sum_sq / count as f32).sqrt().clamp(0.0, 1.0);
        shared.level.store((rms * 1000.0) as u32, Ordering::Relaxed);
    }
}

pub fn is_silent(samples: &[f32]) -> bool {
    samples
        .iter()
        .map(|sample| sample.abs())
        .fold(0.0f32, f32::max)
        < 0.012
}

pub fn encode_wav_16k(clip: &AudioClip) -> Vec<u8> {
    let mono = resample_16k(&clip.samples, clip.sample_rate);
    let mut body = Vec::with_capacity(44 + mono.len() * 2);
    let data_len = (mono.len() * 2) as u32;
    body.extend_from_slice(b"RIFF");
    body.extend_from_slice(&(36 + data_len).to_le_bytes());
    body.extend_from_slice(b"WAVE");
    body.extend_from_slice(b"fmt ");
    body.extend_from_slice(&16u32.to_le_bytes());
    body.extend_from_slice(&1u16.to_le_bytes());
    body.extend_from_slice(&1u16.to_le_bytes());
    body.extend_from_slice(&TARGET_RATE.to_le_bytes());
    body.extend_from_slice(&(TARGET_RATE * 2).to_le_bytes());
    body.extend_from_slice(&2u16.to_le_bytes());
    body.extend_from_slice(&16u16.to_le_bytes());
    body.extend_from_slice(b"data");
    body.extend_from_slice(&data_len.to_le_bytes());
    for sample in mono {
        let scaled = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        body.extend_from_slice(&scaled.to_le_bytes());
    }
    body
}

fn resample_16k(samples: &[f32], rate: u32) -> Vec<f32> {
    if samples.is_empty() || rate == 0 {
        return Vec::new();
    }
    if rate == TARGET_RATE {
        return samples.to_vec();
    }
    let step = rate as f64 / TARGET_RATE as f64;
    let mut out = Vec::new();
    let mut position = 0.0f64;
    while position + 1.0 < samples.len() as f64 {
        let index = position.floor() as usize;
        let fraction = (position - index as f64) as f32;
        let left = samples[index];
        let right = samples.get(index + 1).copied().unwrap_or(left);
        out.push(left + (right - left) * fraction);
        position += step;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn silence_and_wav_header() {
        assert!(is_silent(&[0.0, 0.001, -0.002]));
        assert!(!is_silent(&[0.0, 0.2]));
        let clip = AudioClip {
            samples: vec![0.0, 0.5, -0.5, 0.25],
            sample_rate: 16_000,
        };
        let wav = encode_wav_16k(&clip);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(u32::from_le_bytes(wav[24..28].try_into().unwrap()), 16_000);
        assert_eq!(wav.len(), 44 + 8);
    }

    #[test]
    fn downsample_shrinks_48k_audio() {
        let clip = AudioClip {
            samples: vec![0.1; 48_000],
            sample_rate: 48_000,
        };
        let wav = encode_wav_16k(&clip);
        let data_bytes = wav.len() - 44;
        assert!(data_bytes < 48_000 * 2);
        assert!(data_bytes > 15_000 * 2);
    }
}
