//! Volcengine openspeech (豆包语音) file-style recognition.
//!
//! Official streaming input endpoint, checked 2026-11-12:
//! `wss://openspeech.bytedance.com/api/v3/sauc/bigmodel_nostream`
//! Resource `volc.seedasr.sauc.duration`, `model_name` = `bigmodel`.
//! `doubao-seed-asr-1-0` is not used.
//!
//! New-console callers send `X-Api-Key`. Old-console callers send
//! `X-Api-App-Key` and `X-Api-Access-Key`. The socket is opened only after
//! `egress` allows a cloud call.

use std::time::{SystemTime, UNIX_EPOCH};

use futures_util::{SinkExt, StreamExt};
use serde_json::{json, Value};

use super::egress::{self, Purpose};

pub const ENDPOINT: &str = "wss://openspeech.bytedance.com/api/v3/sauc/bigmodel_nostream";
pub const RESOURCE_ID: &str = "volc.seedasr.sauc.duration";
pub const MODEL_NAME: &str = "bigmodel";

const FULL_CLIENT: u8 = 0b0001;
const AUDIO_ONLY: u8 = 0b0010;
const SERVER_RESPONSE: u8 = 0b1001;
const SERVER_ERROR: u8 = 0b1111;
const FLAG_SEQUENCE: u8 = 0b0001;
const FLAG_LAST: u8 = 0b0010;
const JSON_SERIAL: u8 = 0b0001;
const GZIP: u8 = 0b0001;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VolcAuth {
    pub api_key: String,
    pub app_id: String,
    pub token: String,
}

pub fn validate_auth(auth: &VolcAuth) -> Result<(), String> {
    let key = !auth.api_key.trim().is_empty();
    let pair = !auth.app_id.trim().is_empty() && !auth.token.trim().is_empty();
    if key || pair {
        Ok(())
    } else {
        Err("Save a Volcengine speech API key, or an App ID and an access token.".into())
    }
}

pub fn header_pairs(auth: &VolcAuth) -> Result<Vec<(&'static str, String)>, String> {
    validate_auth(auth)?;
    let mut pairs = Vec::new();
    if !auth.api_key.trim().is_empty() {
        pairs.push(("X-Api-Key", auth.api_key.trim().to_string()));
    }
    if !auth.app_id.trim().is_empty() && !auth.token.trim().is_empty() {
        pairs.push(("X-Api-App-Key", auth.app_id.trim().to_string()));
        pairs.push(("X-Api-Access-Key", auth.token.trim().to_string()));
    }
    pairs.push(("X-Api-Resource-Id", RESOURCE_ID.to_string()));
    pairs.push(("X-Api-Request-Id", request_id()));
    pairs.push(("X-Api-Sequence", "-1".to_string()));
    Ok(pairs)
}

pub fn request_json(terms: &[String]) -> Value {
    let mut request = json!({
        "model_name": MODEL_NAME,
        "enable_itn": true,
        "enable_punc": true,
        "enable_nonstream": true,
        "result_type": "full",
    });
    let hotwords: Vec<Value> = terms
        .iter()
        .map(|word| word.trim())
        .filter(|word| !word.is_empty())
        .map(|word| json!({ "word": word }))
        .collect();
    if !hotwords.is_empty() {
        let context = serde_json::to_string(&json!({ "hotwords": hotwords })).unwrap_or_default();
        request["corpus"] = json!({ "context": context });
    }
    json!({
        "user": { "uid": "newtype" },
        "audio": {
            "format": "pcm",
            "codec": "raw",
            "rate": 16000,
            "bits": 16,
            "channel": 1
        },
        "request": request
    })
}

pub fn client_frames(terms: &[String], pcm: &[u8]) -> Result<Vec<Vec<u8>>, String> {
    let body = serde_json::to_vec(&request_json(terms)).map_err(|err| err.to_string())?;
    let mut frames = vec![frame(FULL_CLIENT, 0, JSON_SERIAL, GZIP, &gzip(&body)?)];
    let chunk = 6_400usize;
    if pcm.is_empty() {
        frames.push(frame(AUDIO_ONLY, FLAG_LAST, 0, GZIP, &gzip(&[])?));
        return Ok(frames);
    }
    let mut offset = 0;
    while offset < pcm.len() {
        let end = (offset + chunk).min(pcm.len());
        let last = end == pcm.len();
        let flags = if last { FLAG_LAST } else { 0 };
        frames.push(frame(AUDIO_ONLY, flags, 0, GZIP, &gzip(&pcm[offset..end])?));
        offset = end;
    }
    Ok(frames)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerEvent {
    pub text: String,
    pub last: bool,
    pub error: String,
}

pub fn parse_server_frame(bytes: &[u8]) -> Result<ServerEvent, String> {
    if bytes.len() < 8 {
        return Err("Volcengine frame is too short.".into());
    }
    let header_len = ((bytes[0] & 0x0f) as usize).saturating_mul(4).max(4);
    if bytes.len() < header_len {
        return Err("Volcengine frame header is truncated.".into());
    }
    let message = bytes[1] >> 4;
    let flags = bytes[1] & 0x0f;
    let compression = bytes[2] & 0x0f;
    let mut offset = header_len;
    if flags & FLAG_SEQUENCE != 0 {
        if bytes.len() < offset + 4 {
            return Err("Volcengine frame is missing its sequence.".into());
        }
        offset += 4;
    }
    if message == SERVER_ERROR {
        if bytes.len() < offset + 4 {
            return Err("Volcengine error frame is truncated.".into());
        }
        offset += 4;
    }
    if bytes.len() < offset + 4 {
        return Err("Volcengine frame is missing its payload size.".into());
    }
    let size = u32::from_be_bytes(bytes[offset..offset + 4].try_into().unwrap()) as usize;
    offset += 4;
    let end = offset + size;
    if bytes.len() < end {
        return Err("Volcengine payload is truncated.".into());
    }
    let raw = &bytes[offset..end];
    let payload = if compression == GZIP { gunzip(raw)? } else { raw.to_vec() };
    if message == SERVER_ERROR {
        let message = error_text(&payload);
        return Ok(ServerEvent {
            text: String::new(),
            last: true,
            error: message,
        });
    }
    if message != SERVER_RESPONSE {
        return Err(format!("Unexpected Volcengine message type {message}."));
    }
    Ok(ServerEvent {
        text: result_text(&payload).unwrap_or_default(),
        last: flags & FLAG_LAST != 0,
        error: String::new(),
    })
}

pub fn transcribe_wav(auth: &VolcAuth, terms: &[String], wav: &[u8]) -> Result<String, String> {
    egress::authorize(Purpose::Cloud)?;
    let pcm = pcm_from_wav(wav)?;
    let frames = client_frames(terms, &pcm)?;
    let pairs = header_pairs(auth)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|err| format!("Could not start the Volcengine client: {err}"))?;
    runtime.block_on(exchange(&pairs, &frames))
}

pub fn test_connection(auth: &VolcAuth) -> Result<String, String> {
    let _ = header_pairs(auth)?;
    let err = match transcribe_wav(auth, &[], &silent_wav()) {
        Ok(text) => {
            return Ok(format!(
                "Connected. Volcengine accepted the speech credentials{}.",
                if text.is_empty() { "" } else { " and returned text" }
            ));
        }
        Err(err) => err,
    };
    if err.contains("Strict offline") || err.contains("Local only") {
        return Err(err);
    }
    let folded = err.to_lowercase();
    if folded.contains("unauthor")
        || folded.contains("invalid")
        || folded.contains("401")
        || folded.contains("403")
        || err.contains("鉴权")
    {
        return Err(format!("Volcengine rejected the speech credentials. {err}"));
    }
    if folded.contains("could not") || folded.contains("connect") || folded.contains("timed") {
        return Err(err);
    }
    Ok(format!("Connected. Volcengine accepted the handshake ({err})."))
}

async fn exchange(pairs: &[(&'static str, String)], frames: &[Vec<u8>]) -> Result<String, String> {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    use tokio_tungstenite::tungstenite::http::{HeaderName, HeaderValue};
    let mut request = ENDPOINT
        .into_client_request()
        .map_err(|err| format!("Could not build the Volcengine request: {err}"))?;
    {
        let headers = request.headers_mut();
        for (name, value) in pairs {
            let name = HeaderName::from_bytes(name.as_bytes())
                .map_err(|_| format!("Bad header name {name}."))?;
            let value = HeaderValue::from_str(value).map_err(|_| {
                "A Volcengine credential contains a character the header cannot store.".to_string()
            })?;
            headers.insert(name, value);
        }
    }
    let (mut socket, _response) = tokio_tungstenite::connect_async(request)
        .await
        .map_err(|err| format!("Could not reach Volcengine speech: {err}"))?;
    for frame in frames {
        socket
            .send(tokio_tungstenite::tungstenite::Message::Binary(frame.clone().into()))
            .await
            .map_err(|err| format!("Could not send audio to Volcengine: {err}"))?;
    }
    let mut latest = String::new();
    while let Some(message) = socket.next().await {
        let message = message.map_err(|err| format!("Volcengine speech closed: {err}"))?;
        let bytes = match message {
            tokio_tungstenite::tungstenite::Message::Binary(bytes) => bytes.to_vec(),
            tokio_tungstenite::tungstenite::Message::Close(_) => break,
            _ => continue,
        };
        let event = parse_server_frame(&bytes)?;
        if !event.error.is_empty() {
            return Err(format!("Volcengine speech failed: {}", event.error));
        }
        if !event.text.is_empty() {
            latest = event.text;
        }
        if event.last && !latest.is_empty() {
            break;
        }
    }
    if latest.is_empty() {
        Err("Volcengine returned no transcript.".into())
    } else {
        Ok(latest)
    }
}

fn frame(message: u8, flags: u8, serial: u8, compression: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + payload.len());
    out.push(0x11);
    out.push((message << 4) | (flags & 0x0f));
    out.push((serial << 4) | (compression & 0x0f));
    out.push(0);
    out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    out.extend_from_slice(payload);
    out
}

fn gzip(bytes: &[u8]) -> Result<Vec<u8>, String> {
    use std::io::Write;
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(bytes).map_err(|err| err.to_string())?;
    encoder.finish().map_err(|err| err.to_string())
}

fn gunzip(bytes: &[u8]) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let mut decoder = flate2::read::GzDecoder::new(bytes);
    let mut out = Vec::new();
    decoder.read_to_end(&mut out).map_err(|err| format!("Could not read a Volcengine frame: {err}"))?;
    Ok(out)
}

fn result_text(payload: &[u8]) -> Option<String> {
    let value: Value = serde_json::from_slice(payload).ok()?;
    value
        .pointer("/result/text")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
        .or_else(|| {
            value
                .pointer("/payload_msg/result/text")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_string)
        })
}

fn error_text(payload: &[u8]) -> String {
    if let Ok(value) = serde_json::from_slice::<Value>(payload) {
        if let Some(message) = value.get("message").and_then(Value::as_str) {
            return message.to_string();
        }
        if let Some(message) = value.get("error").and_then(Value::as_str) {
            return message.to_string();
        }
    }
    String::from_utf8_lossy(payload).chars().take(180).collect()
}

pub fn pcm_from_wav(bytes: &[u8]) -> Result<Vec<u8>, String> {
    if bytes.len() < 44 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("Volcengine speech expected a WAV recording.".into());
    }
    let mut offset = 12usize;
    while offset + 8 <= bytes.len() {
        let id = &bytes[offset..offset + 4];
        let size = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap_or([0; 4])) as usize;
        let start = offset + 8;
        let end = start.saturating_add(size).min(bytes.len());
        if id == b"data" {
            return Ok(bytes[start..end].to_vec());
        }
        offset = end + (size % 2);
    }
    Err("The recording has no audio data.".into())
}

fn silent_wav() -> Vec<u8> {
    super::audio::encode_wav_16k(&super::audio::AudioClip {
        samples: vec![0.0; 1600],
        sample_rate: 16_000,
    })
}

fn request_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!(
        "{:08x}-{:04x}-4{:03x}-a{:03x}-{:012x}",
        (nanos >> 64) as u32,
        ((nanos >> 48) as u16),
        (nanos >> 36) as u16 & 0x0fff,
        (nanos >> 24) as u16 & 0x0fff,
        nanos & 0xffffffffffff
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::egress::{ModeGuard, Purpose};

    fn auth() -> VolcAuth {
        VolcAuth {
            api_key: "speech-key".into(),
            app_id: "123456".into(),
            token: "access-token".into(),
        }
    }

    #[test]
    fn headers_cover_both_consoles_and_the_seed_resource() {
        let pairs = header_pairs(&auth()).unwrap();
        let joined = pairs
            .iter()
            .map(|(name, value)| format!("{name}={value}"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(joined.contains("X-Api-Key=speech-key"));
        assert!(joined.contains("X-Api-App-Key=123456"));
        assert!(joined.contains("X-Api-Access-Key=access-token"));
        assert!(joined.contains("X-Api-Resource-Id=volc.seedasr.sauc.duration"));
        assert!(joined.contains("X-Api-Sequence=-1"));
        assert!(!joined.contains("doubao-seed-asr"));
        assert!(validate_auth(&VolcAuth {
            api_key: String::new(),
            app_id: String::new(),
            token: "only-token".into(),
        })
        .is_err());
    }

    #[test]
    fn the_request_uses_bigmodel_and_dictionary_terms() {
        let value = request_json(&["Zorblax Prime".into(), "青岚计划".into()]);
        assert_eq!(value["request"]["model_name"], "bigmodel");
        let context = value["request"]["corpus"]["context"].as_str().unwrap();
        let parsed: Value = serde_json::from_str(context).unwrap();
        let words = parsed["hotwords"].as_array().unwrap();
        assert_eq!(words.len(), 2);
        assert_eq!(words[0]["word"], "Zorblax Prime");
        assert_eq!(words[1]["word"], "青岚计划");
        assert!(!context.contains("doubao-seed-asr"));
        let frames = client_frames(&["Fnordix".into()], &[0u8; 100]).unwrap();
        assert!(frames.len() >= 2);
        assert_eq!(frames[0][0], 0x11);
        assert_eq!(frames[0][1] >> 4, FULL_CLIENT);
        assert_eq!(frames.last().unwrap()[1] & 0x0f, FLAG_LAST);
    }

    #[test]
    fn a_server_frame_round_trips_the_transcript() {
        let payload = gzip(r#"{"result":{"text":"今天把这个发给 Emma"}}"#.as_bytes()).unwrap();
        let mut bytes = frame(SERVER_RESPONSE, FLAG_LAST | FLAG_SEQUENCE, JSON_SERIAL, GZIP, &[]);
        bytes.truncate(4);
        bytes.extend_from_slice(&3i32.to_be_bytes());
        bytes.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&payload);
        let event = parse_server_frame(&bytes).unwrap();
        assert!(event.last);
        assert_eq!(event.text, "今天把这个发给 Emma");
        assert!(event.error.is_empty());
    }

    #[test]
    fn strict_offline_refuses_before_any_socket() {
        let _guard = ModeGuard::strict();
        let started = std::time::Instant::now();
        let err = transcribe_wav(&auth(), &[], &silent_wav()).unwrap_err();
        assert!(err.contains("Strict offline"), "{err}");
        assert!(!err.contains("speech-key"));
        assert!(!err.contains("access-token"));
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        assert!(egress::authorize(Purpose::Cloud).is_err());
    }
}
