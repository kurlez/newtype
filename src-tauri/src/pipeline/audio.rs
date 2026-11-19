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
        note: "The default input device records until the hotkey stops. Local recognition resamples to 16 kHz and trims silence.",
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
    /// False while the device stays open between dictations.
    capturing: AtomicBool,
    /// Set when a capturing callback has stored samples.
    heard: AtomicBool,
    /// The device reported an error. The next open starts a new stream.
    dead: AtomicBool,
}

struct WarmInput {
    name: String,
    /// Owning the stream keeps the device open between dictations.
    #[allow(dead_code)]
    stream: cpal::Stream,
    shared: Arc<SharedAudio>,
}

enum MicCmd {
    Begin(String, std::sync::mpsc::Sender<Result<Arc<SharedAudio>, String>>),
    Idle,
    Reset,
}

const IDLE_KEEP: std::time::Duration = std::time::Duration::from_secs(30);

static MIC_TX: std::sync::OnceLock<std::sync::mpsc::Sender<MicCmd>> = std::sync::OnceLock::new();
static LIVE: AtomicBool = AtomicBool::new(false);

pub struct MicSession {
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

    pub fn heard(&self) -> bool {
        self.shared.heard.load(Ordering::Relaxed)
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

fn ensure_mic_thread() -> Result<&'static std::sync::mpsc::Sender<MicCmd>, String> {
    if let Some(tx) = MIC_TX.get() {
        return Ok(tx);
    }
    let (tx, rx) = std::sync::mpsc::channel::<MicCmd>();
    std::thread::Builder::new()
        .name("newtype-mic".into())
        .spawn(move || mic_thread(rx))
        .map_err(|err| format!("Could not start the microphone thread: {err}"))?;
    let _ = MIC_TX.set(tx);
    MIC_TX.get().ok_or_else(|| "Microphone thread is unavailable.".to_string())
}

fn mic_thread(rx: std::sync::mpsc::Receiver<MicCmd>) {
    let mut warm: Option<WarmInput> = None;
    let mut idle_since: Option<std::time::Instant> = None;
    loop {
        let cmd = rx.recv_timeout(std::time::Duration::from_secs(1));
        if !LIVE.load(Ordering::SeqCst) {
            if let Some(since) = idle_since {
                if since.elapsed() >= IDLE_KEEP {
                    warm = None;
                    idle_since = None;
                }
            }
        }
        match cmd {
            Ok(MicCmd::Reset) => {
                warm = None;
                idle_since = None;
                LIVE.store(false, Ordering::SeqCst);
            }
            Ok(MicCmd::Idle) => {
                if !LIVE.load(Ordering::SeqCst) {
                    idle_since = Some(std::time::Instant::now());
                }
            }
            Ok(MicCmd::Begin(name, reply)) => {
                let wanted = resolved_input_name(&name);
                let reopen = match warm.as_ref() {
                    Some(open) => open.shared.dead.load(Ordering::SeqCst) || open.name != wanted,
                    None => true,
                };
                if reopen {
                    match open_input(&name) {
                        Ok(opened) => warm = Some(opened),
                        Err(err) => {
                            let _ = reply.send(Err(err));
                            continue;
                        }
                    }
                }
                let Some(opened) = warm.as_ref() else {
                    let _ = reply.send(Err("No microphone was found.".into()));
                    continue;
                };
                {
                    let mut samples = opened.shared.samples.lock().expect("audio buffer");
                    samples.clear();
                }
                opened.shared.dead.store(false, Ordering::SeqCst);
                opened.shared.capped.store(false, Ordering::SeqCst);
                opened.shared.heard.store(false, Ordering::SeqCst);
                opened.shared.capturing.store(true, Ordering::SeqCst);
                LIVE.store(true, Ordering::SeqCst);
                idle_since = None;
                let _ = reply.send(Ok(Arc::clone(&opened.shared)));
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
}

pub fn reset() {
    if let Ok(tx) = ensure_mic_thread() {
        let _ = tx.send(MicCmd::Reset);
    }
}

fn note_idle() {
    if let Ok(tx) = ensure_mic_thread() {
        let _ = tx.send(MicCmd::Idle);
    }
}

impl MicSession {
    pub fn start(preferred_name: &str) -> Result<Self, String> {
        let tx = ensure_mic_thread()?;
        let (reply_tx, reply_rx) = std::sync::mpsc::channel();
        tx.send(MicCmd::Begin(preferred_name.trim().to_string(), reply_tx))
            .map_err(|_| "Microphone thread stopped.".to_string())?;
        let shared = reply_rx
            .recv()
            .map_err(|_| "Microphone thread stopped.".to_string())??;
        Ok(Self { shared })
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
        self.finish()
    }

    fn finish(&self) -> AudioClip {
        self.shared.capturing.store(false, Ordering::SeqCst);
        LIVE.store(false, Ordering::SeqCst);
        note_idle();
        let samples = std::mem::take(&mut *self.shared.samples.lock().expect("audio buffer"));
        AudioClip {
            samples,
            sample_rate: self.shared.sample_rate,
        }
    }
}

impl Drop for MicSession {
    fn drop(&mut self) {
        self.shared.capturing.store(false, Ordering::SeqCst);
        if LIVE.swap(false, Ordering::SeqCst) {
            note_idle();
        }
    }
}

fn open_input(preferred_name: &str) -> Result<WarmInput, String> {
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
        capturing: AtomicBool::new(false),
        heard: AtomicBool::new(false),
        dead: AtomicBool::new(false),
    });
    let stream = build_stream(&device, &supported, channels, Arc::clone(&shared))?;
    stream
        .play()
        .map_err(|err| format!("Could not start the microphone: {err}"))?;
    Ok(WarmInput {
        name: resolved_input_name(preferred_name),
        stream,
        shared,
    })
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
            let errors = Arc::clone(&shared);
            device.build_input_stream(
                &config,
                move |data: &[f32], _| push_frames(data, channels, &shared),
                move |_error| mark_stream_dead(&errors),
                None,
            )
        }
        cpal::SampleFormat::I16 => {
            let shared = Arc::clone(&shared);
            let errors = Arc::clone(&shared);
            device.build_input_stream(
                &config,
                move |data: &[i16], _| {
                    let converted: Vec<f32> = data
                        .iter()
                        .map(|sample| *sample as f32 / i16::MAX as f32)
                        .collect();
                    push_frames(&converted, channels, &shared);
                },
                move |_error| mark_stream_dead(&errors),
                None,
            )
        }
        cpal::SampleFormat::U16 => {
            let shared = Arc::clone(&shared);
            let errors = Arc::clone(&shared);
            device.build_input_stream(
                &config,
                move |data: &[u16], _| {
                    let converted: Vec<f32> = data
                        .iter()
                        .map(|sample| (*sample as f32 / u16::MAX as f32) * 2.0 - 1.0)
                        .collect();
                    push_frames(&converted, channels, &shared);
                },
                move |_error| mark_stream_dead(&errors),
                None,
            )
        }
        other => {
            return Err(format!("This microphone format is not supported ({other:?})."));
        }
    };
    stream.map_err(|err| format!("Could not start the microphone: {err}"))
}

fn mark_stream_dead(shared: &SharedAudio) {
    shared.dead.store(true, Ordering::SeqCst);
    crate::diag::record("microphone stream stopped");
}

fn resolved_input_name(preferred_name: &str) -> String {
    let host = cpal::default_host();
    if preferred_name.trim().is_empty() {
        return host
            .default_input_device()
            .and_then(|device| device.name().ok())
            .unwrap_or_default();
    }
    preferred_name.trim().to_string()
}

pub fn start_tone_samples(sample_rate: u32, millis: u32) -> Vec<f32> {
    let count = (sample_rate as u64 * u64::from(millis) / 1000) as usize;
    (0..count)
        .map(|index| {
            let phase = index as f32 * 880.0 * std::f32::consts::TAU / sample_rate as f32;
            (phase.sin() * 0.2).clamp(-1.0, 1.0)
        })
        .collect()
}

pub fn play_start_tone() {
    let host = cpal::default_host();
    let Some(device) = host.default_output_device() else {
        crate::diag::record("start sound skipped");
        return;
    };
    let Ok(supported) = device.default_output_config() else {
        crate::diag::record("start sound skipped");
        return;
    };
    let rate = supported.sample_rate().0;
    let channels = supported.channels() as usize;
    let tone = start_tone_samples(rate, 90);
    let config: cpal::StreamConfig = supported.into();
    let cursor = Arc::new(AtomicU32::new(0));
    let samples = Arc::new(tone);
    let total = samples.len() as u32;
    let stream = device.build_output_stream(
        &config,
        move |data: &mut [f32], _| {
            let mut index = cursor.load(Ordering::Relaxed) as usize;
            for frame in data.chunks_mut(channels.max(1)) {
                let sample = samples.get(index).copied().unwrap_or(0.0);
                for slot in frame.iter_mut() {
                    *slot = sample;
                }
                index = index.saturating_add(1);
            }
            cursor.store(index as u32, Ordering::Relaxed);
        },
        |_| {},
        None,
    );
    if let Ok(stream) = stream {
        let _ = stream.play();
        std::thread::sleep(std::time::Duration::from_millis(120));
        let _ = total;
    }
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
    let capturing = shared.capturing.load(Ordering::Relaxed);
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
        if capturing {
            guard.push(sample);
            shared.heard.store(true, Ordering::Relaxed);
        }
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

pub fn pcm_16k(clip: &AudioClip) -> Vec<f32> {
    resample_16k(&clip.samples, clip.sample_rate)
}

pub fn encode_wav_16k(clip: &AudioClip) -> Vec<u8> {
    let mono = pcm_16k(clip);
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

pub fn decode_wav(bytes: &[u8]) -> Result<AudioClip, String> {
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("The saved recording is not a WAV file.".into());
    }
    let mut rate = TARGET_RATE;
    let mut offset = 12usize;
    let mut data: Option<&[u8]> = None;
    while offset + 8 <= bytes.len() {
        let id = &bytes[offset..offset + 4];
        let size = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap_or([0; 4])) as usize;
        let start = offset + 8;
        let end = start.saturating_add(size).min(bytes.len());
        if id == b"fmt " && end >= start + 16 {
            rate = u32::from_le_bytes(bytes[start + 4..start + 8].try_into().unwrap_or([0; 4]));
        }
        if id == b"data" {
            data = Some(&bytes[start..end]);
            break;
        }
        offset = end + (size % 2);
    }
    let data = data.ok_or_else(|| "The saved recording has no audio data.".to_string())?;
    let mut samples = Vec::with_capacity(data.len() / 2);
    let mut index = 0;
    while index + 1 < data.len() {
        let value = i16::from_le_bytes([data[index], data[index + 1]]);
        samples.push(value as f32 / i16::MAX as f32);
        index += 2;
    }
    if rate == 0 {
        rate = TARGET_RATE;
    }
    Ok(AudioClip {
        samples,
        sample_rate: rate,
    })
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
    fn an_idle_device_does_not_keep_samples_until_capture_starts() {
        let shared = Arc::new(SharedAudio {
            samples: Mutex::new(Vec::new()),
            sample_rate: 16_000,
            level: AtomicU32::new(0),
            capped: AtomicBool::new(false),
            capturing: AtomicBool::new(false),
            heard: AtomicBool::new(false),
            dead: AtomicBool::new(false),
        });
        push_frames(&[0.4, 0.4], 1, &shared);
        assert!(shared.samples.lock().expect("audio").is_empty());
        shared.capturing.store(true, Ordering::Relaxed);
        push_frames(&[0.4, 0.2], 1, &shared);
        assert_eq!(shared.samples.lock().expect("audio").len(), 2);
        assert!(shared.heard.load(Ordering::Relaxed));
        assert!(!start_tone_samples(16_000, 90).is_empty());
    }

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
        let decoded = decode_wav(&wav).unwrap();
        assert_eq!(decoded.sample_rate, 16_000);
        assert_eq!(decoded.samples.len(), 4);
        assert!((decoded.samples[1] - 0.5).abs() < 0.01);
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
