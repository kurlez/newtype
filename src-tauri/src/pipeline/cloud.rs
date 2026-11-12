//! Cloud ASR and LLM calls.
//!
//! Model ids below were checked against vendor docs on 2026-11-12.
//! They are suggestions. The settings screen can type another id or load
//! `GET /v1/models` for the saved key. Nothing here is the only accepted id.

use serde_json::{json, Value};

use super::asr::AsrProvider;
use super::polish::{LlmComplete, LlmRequest};

pub const VERIFIED_ON: &str = "2026-11-12";

const LLM_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(8);
const ASR_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);
const PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(12);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AsrWire {
    Transcriptions,
    DashscopeAudio,
    Volcengine,
    Unfinished,
}

impl AsrWire {
    pub fn parse(value: &str) -> Self {
        match value {
            "dashscope" | "dashscopeAudio" => Self::DashscopeAudio,
            "volcengine" | "openspeech" => Self::Volcengine,
            "unfinished" => Self::Unfinished,
            _ => Self::Transcriptions,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Transcriptions => "transcriptions",
            Self::DashscopeAudio => "dashscope",
            Self::Volcengine => "volcengine",
            Self::Unfinished => "unfinished",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Thinking {
    /// `reasoning_effort: none` plus temperature 0. GPT-6 Luna, GPT-6 Sol, GPT-6.1 Terra.
    OpenAiNone,
    /// Lowest effort those models accept. GPT-6 Astra and GPT-6.1 Sol reject `none`.
    OpenAiLow,
    /// GPT-OSS on Groq rejects `none`. `low` is the smallest accepted effort.
    GroqLow,
    /// Qwen on Groq accepts `none`.
    GroqNone,
    /// DashScope hybrid models. `enable_thinking: false`, with a `reasoning_effort: none` retry.
    Dashscope,
    /// Volcengine Ark. `thinking.type: disabled`.
    Volcengine,
    /// Temperature 0 and no thinking field.
    Off,
}

#[derive(Debug, Clone, Copy)]
pub struct Preset {
    pub id: &'static str,
    pub label: &'static str,
    pub asr_ready: bool,
    pub llm_ready: bool,
    pub base_editable: bool,
    pub default_base: &'static str,
    pub default_asr: &'static str,
    pub default_llm: &'static str,
    pub asr_models: &'static [&'static str],
    pub llm_models: &'static [&'static str],
    pub asr_wire: AsrWire,
    pub note: &'static str,
    pub verified: &'static str,
}

pub fn presets() -> &'static [Preset] {
    &[
        Preset {
            id: "openai",
            label: "OpenAI",
            asr_ready: true,
            llm_ready: true,
            base_editable: false,
            default_base: "https://api.openai.com/v1",
            default_asr: "gpt-transcribe",
            default_llm: "gpt-6-luna",
            asr_models: &["gpt-transcribe"],
            llm_models: &["gpt-6-luna", "gpt-6.1-sol", "gpt-6-astra"],
            asr_wire: AsrWire::Transcriptions,
            note: "Default. File transcription plus a chat rewrite. gpt-6-luna can turn thinking off. gpt-6-astra and gpt-6.1-sol cannot; they use the low effort setting.",
            verified: "developers.openai.com model pages for gpt-transcribe, gpt-6-luna, gpt-6.1-sol, and gpt-6-astra, checked 2026-11-12. whisper-1 and gpt-4o-transcribe are deprecated and are not suggested.",
        },
        Preset {
            id: "groq",
            label: "Groq",
            asr_ready: true,
            llm_ready: true,
            base_editable: false,
            default_base: "https://api.groq.com/openai/v1",
            default_asr: "whisper-large-v3",
            default_llm: "openai/gpt-oss-20b",
            asr_models: &["whisper-large-v3", "whisper-large-v3-turbo"],
            llm_models: &[
                "openai/gpt-oss-20b",
                "openai/gpt-oss-120b",
                "qwen/qwen3.8-27b",
            ],
            asr_wire: AsrWire::Transcriptions,
            note: "OpenAI-compatible. Whisper's Chinese error rate is weaker than Qwen or Doubao; whisper-large-v3 is the default, turbo is the faster option. gpt-oss cannot fully disable reasoning (low is the minimum). qwen/qwen3.8-27b is optional and can disable it.",
            verified: "console.groq.com/docs/models and the speech-to-text and reasoning pages, checked 2026-11-12.",
        },
        Preset {
            id: "dashscope",
            label: "DashScope · 通义",
            asr_ready: true,
            llm_ready: true,
            base_editable: true,
            default_base: "",
            default_asr: "qwen3-asr-flash",
            default_llm: "qwen3.8-flash",
            asr_models: &["qwen3-asr-flash"],
            llm_models: &["qwen3.8-flash", "qwen3.8-max"],
            asr_wire: AsrWire::DashscopeAudio,
            note: "Partial. Speech uses the OpenAI-compatible chat endpoint with input audio, not a WebSocket. Paste the workspace base URL, for example https://{WorkspaceId}.cn-beijing.maas.aliyuncs.com/compatible-mode/v1. The one-shot message WebSocket model is not implemented. Thinking is requested off.",
            verified: "Aliyun Model Studio ASR and chat-completions docs, checked 2026-11-12. qwen3-asr-flash and qwen3.8-flash are the documented ids. qwen-audio-3.1-asr-flash-message and qwen3.7-plus were not in that model list, so they are not suggested.",
        },
        Preset {
            id: "volcengine",
            label: "Volcengine · 火山",
            asr_ready: true,
            llm_ready: true,
            base_editable: true,
            default_base: "https://ark.cn-beijing.volces.com/api/v3",
            default_asr: "bigmodel",
            default_llm: "doubao-seed-2-1-lite-260915",
            asr_models: &["bigmodel"],
            llm_models: &[
                "doubao-seed-2-1-lite-260915",
                "doubao-seed-2-1-pro-260915",
            ],
            asr_wire: AsrWire::Volcengine,
            note: "Speech uses openspeech (volc.seedasr.sauc.duration, model name bigmodel). New console: speech API key. Old console: App ID plus access token. The rewrite model uses the Ark key with thinking disabled. doubao-seed-asr-1-0 is not used.",
            verified: "Volcengine openspeech streaming ASR page and the Ark model list, checked 2026-11-12. Lite and pro snapshot ids both support thinking.type=disabled.",
        },
        Preset {
            id: "custom",
            label: "OpenAI-compatible",
            asr_ready: true,
            llm_ready: true,
            base_editable: true,
            default_base: "",
            default_asr: "",
            default_llm: "",
            asr_models: &[],
            llm_models: &[],
            asr_wire: AsrWire::Transcriptions,
            note: "Your base URL, speech model, and rewrite model. Speech can be /audio/transcriptions or the DashScope audio chat shape. localhost may use http. Thinking follows the style you pick.",
            verified: "No model id is assumed.",
        },
    ]
}

pub fn preset(id: &str) -> Option<&'static Preset> {
    presets().iter().find(|preset| preset.id == id)
}

pub fn thinking_for(provider: &str, model: &str, preference: &str, custom_style: &str) -> Thinking {
    let preference = if provider == "custom" && preference == "auto" {
        custom_style
    } else {
        preference
    };
    match preference {
        "off" => match provider {
            "openai" => Thinking::OpenAiNone,
            "groq" if model.starts_with("qwen/") => Thinking::GroqNone,
            "groq" => Thinking::GroqLow,
            "dashscope" => Thinking::Dashscope,
            "volcengine" => Thinking::Volcengine,
            _ => match custom_style {
                "openai-low" => Thinking::OpenAiLow,
                "dashscope" => Thinking::Dashscope,
                "volcengine" => Thinking::Volcengine,
                "off" => Thinking::Off,
                _ => Thinking::OpenAiNone,
            },
        },
        "low" | "openai-low" => match provider {
            "groq" => Thinking::GroqLow,
            "dashscope" => Thinking::Dashscope,
            "volcengine" => Thinking::Volcengine,
            _ => Thinking::OpenAiLow,
        },
        "dashscope" => Thinking::Dashscope,
        "volcengine" => Thinking::Volcengine,
        "openai-none" => Thinking::OpenAiNone,
        "none-field" | "off-field" => Thinking::Off,
        _ => auto_thinking(provider, model, custom_style),
    }
}

fn auto_thinking(provider: &str, model: &str, custom_style: &str) -> Thinking {
    match provider {
        "openai" => match model {
            "gpt-6-astra" | "gpt-6.1-sol" => Thinking::OpenAiLow,
            _ => Thinking::OpenAiNone,
        },
        "groq" => {
            if model.starts_with("qwen/") {
                Thinking::GroqNone
            } else {
                Thinking::GroqLow
            }
        }
        "dashscope" => Thinking::Dashscope,
        "volcengine" => Thinking::Volcengine,
        _ => match custom_style {
            "openai-low" => Thinking::OpenAiLow,
            "dashscope" => Thinking::Dashscope,
            "volcengine" => Thinking::Volcengine,
            "off" => Thinking::Off,
            _ => Thinking::OpenAiNone,
        },
    }
}

pub fn chat_body(
    model: &str,
    system: &str,
    user: &str,
    max_tokens: u32,
    thinking: Thinking,
) -> Value {
    let mut body = json!({
        "model": model,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user}
        ]
    });
    let object = body.as_object_mut().expect("object");
    match thinking {
        Thinking::OpenAiNone => {
            object.insert("temperature".into(), json!(0));
            object.insert("max_completion_tokens".into(), json!(max_tokens));
            object.insert("reasoning_effort".into(), json!("none"));
        }
        Thinking::OpenAiLow => {
            object.insert("max_completion_tokens".into(), json!(max_tokens));
            object.insert("reasoning_effort".into(), json!("low"));
        }
        Thinking::GroqLow => {
            object.insert("temperature".into(), json!(0));
            object.insert("max_tokens".into(), json!(max_tokens));
            object.insert("reasoning_effort".into(), json!("low"));
            object.insert("include_reasoning".into(), json!(false));
        }
        Thinking::GroqNone => {
            object.insert("temperature".into(), json!(0));
            object.insert("max_tokens".into(), json!(max_tokens));
            object.insert("reasoning_effort".into(), json!("none"));
        }
        Thinking::Dashscope => {
            object.insert("temperature".into(), json!(0));
            object.insert("max_tokens".into(), json!(max_tokens));
            object.insert("enable_thinking".into(), json!(false));
        }
        Thinking::Volcengine => {
            object.insert("temperature".into(), json!(0));
            object.insert("max_tokens".into(), json!(max_tokens));
            object.insert("thinking".into(), json!({"type": "disabled"}));
        }
        Thinking::Off => {
            object.insert("temperature".into(), json!(0));
            object.insert("max_tokens".into(), json!(max_tokens));
        }
    }
    body
}

pub fn normalize_base(url: &str) -> Result<String, String> {
    let url = url.trim().trim_end_matches('/').to_string();
    if url.is_empty() {
        return Err("Base URL is empty.".into());
    }
    if url.starts_with("https://")
        || url.starts_with("http://127.0.0.1")
        || url.starts_with("http://localhost")
    {
        return Ok(url);
    }
    Err("Base URL must start with https://, or http://localhost for a service on this machine.".into())
}

pub struct CloudAsr {
    pub base: String,
    pub key: String,
    pub model: String,
    pub wire: AsrWire,
    pub prompt: String,
}

impl AsrProvider for CloudAsr {
    fn id(&self) -> &'static str {
        "cloud"
    }

    fn transcribe(&self, wav: &[u8]) -> Result<String, String> {
        transcribe(&self.base, &self.key, &self.model, self.wire, &self.prompt, wav)
    }
}

pub struct CloudLlm {
    pub base: String,
    pub key: String,
    pub model: String,
    pub thinking: Thinking,
}

impl LlmComplete for CloudLlm {
    fn complete(&self, request: &LlmRequest) -> Result<String, String> {
        complete(
            &self.base,
            &self.key,
            &self.model,
            self.thinking,
            &request.system,
            &request.user,
            request.max_tokens,
        )
    }
}

pub fn transcribe(
    base: &str,
    key: &str,
    model: &str,
    wire: AsrWire,
    prompt: &str,
    wav: &[u8],
) -> Result<String, String> {
    let base = normalize_base(base)?;
    if model.trim().is_empty() {
        return Err("Speech model id is empty.".into());
    }
    match wire {
        AsrWire::Unfinished => Err(
            "This provider's speech API is not implemented. Pick another speech provider.".into(),
        ),
        AsrWire::Transcriptions => transcribe_file(&base, key, model, prompt, wav),
        AsrWire::DashscopeAudio => transcribe_dashscope(&base, key, model, prompt, wav),
        AsrWire::Volcengine => Err(
            "Volcengine speech is sent by the openspeech adapter, not this HTTP client.".into(),
        ),
    }
}

pub fn complete(
    base: &str,
    key: &str,
    model: &str,
    thinking: Thinking,
    system: &str,
    user: &str,
    max_tokens: u32,
) -> Result<String, String> {
    let base = normalize_base(base)?;
    if model.trim().is_empty() {
        return Err("Rewrite model id is empty.".into());
    }
    let body = chat_body(model, system, user, max_tokens, thinking);
    match post_chat(&base, key, &body, LLM_TIMEOUT) {
        Ok(text) => Ok(text),
        Err(err) if err.status == 400 => {
            if let Some(retry) = retry_body(&body, thinking) {
                return post_chat(&base, key, &retry, LLM_TIMEOUT)
                    .map(|text| text)
                    .map_err(|retry_err| redact(&retry_err.message, key));
            }
            Err(redact(&err.message, key))
        }
        Err(err) => Err(redact(&err.message, key)),
    }
}

pub fn test_connection(base: &str, key: &str) -> Result<String, String> {
    let base = normalize_base(base)?;
    let url = format!("{base}/models");
    let response = client(PROBE_TIMEOUT)?
        .get(url)
        .bearer_auth(key)
        .header("user-agent", user_agent())
        .send()
        .map_err(|err| redact(&format!("Could not reach the provider: {err}"), key))?;
    let status = response.status();
    if status.is_success() {
        Ok(format!("Connected. The provider accepted the key ({status})."))
    } else {
        let body = response.text().unwrap_or_default();
        Err(redact(
            &format!("The provider rejected the check ({status}). {}", clip(&body)),
            key,
        ))
    }
}

pub fn list_model_ids(base: &str, key: &str) -> Result<Vec<String>, String> {
    let base = normalize_base(base)?;
    let response = client(PROBE_TIMEOUT)?
        .get(format!("{base}/models"))
        .bearer_auth(key)
        .header("user-agent", user_agent())
        .send()
        .map_err(|err| redact(&format!("Could not list models: {err}"), key))?;
    let status = response.status();
    let body = response
        .text()
        .map_err(|err| redact(&format!("Could not read the model list: {err}"), key))?;
    if !status.is_success() {
        return Err(redact(
            &format!("Model list failed ({status}). {}", clip(&body)),
            key,
        ));
    }
    let parsed: Value = serde_json::from_str(&body)
        .map_err(|_| "The model list was not JSON.".to_string())?;
    let mut ids = Vec::new();
    if let Some(data) = parsed.get("data").and_then(Value::as_array) {
        for item in data {
            if let Some(id) = item.get("id").and_then(Value::as_str) {
                ids.push(id.to_string());
            }
        }
    }
    ids.sort();
    ids.dedup();
    ids.truncate(300);
    Ok(ids)
}

struct HttpFailure {
    status: u16,
    message: String,
}

fn transcribe_file(base: &str, key: &str, model: &str, prompt: &str, wav: &[u8]) -> Result<String, String> {
    let part = reqwest::blocking::multipart::Part::bytes(wav.to_vec())
        .file_name("speech.wav")
        .mime_str("audio/wav")
        .map_err(|err| err.to_string())?;
    let mut form = reqwest::blocking::multipart::Form::new()
        .text("model", model.to_string())
        .text("response_format", "json")
        .part("file", part);
    if !prompt.trim().is_empty() {
        form = form.text("prompt", prompt.to_string());
    }
    let response = client(ASR_TIMEOUT)?
        .post(format!("{base}/audio/transcriptions"))
        .bearer_auth(key)
        .header("user-agent", user_agent())
        .multipart(form)
        .send()
        .map_err(|err| redact(&format!("Speech request failed: {err}"), key))?;
    let status = response.status();
    let body = response
        .text()
        .map_err(|err| redact(&format!("Could not read the transcript: {err}"), key))?;
    if !status.is_success() {
        return Err(redact(
            &format!("Speech request failed ({status}). {}", clip(&body)),
            key,
        ));
    }
    let parsed: Value = serde_json::from_str(&body)
        .map_err(|_| "The transcript response was not JSON.".to_string())?;
    parsed
        .get("text")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
        .ok_or_else(|| "The transcript response had no text.".to_string())
}

fn transcribe_dashscope(base: &str, key: &str, model: &str, prompt: &str, wav: &[u8]) -> Result<String, String> {
    let encoded = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, wav);
    let mut content = Vec::new();
    if !prompt.trim().is_empty() {
        content.push(json!({"type": "text", "text": format!("Terms to spell exactly: {prompt}")}));
    }
    content.push(json!({
        "type": "input_audio",
        "input_audio": {
            "data": format!("data:audio/wav;base64,{encoded}"),
            "format": "wav"
        }
    }));
    let body = json!({
        "model": model,
        "messages": [{
            "role": "user",
            "content": content
        }],
        "stream": false,
        "asr_options": {"enable_itn": false}
    });
    post_chat(base, key, &body, ASR_TIMEOUT).map_err(|err| redact(&err.message, key))
}

fn post_chat(base: &str, key: &str, body: &Value, timeout: std::time::Duration) -> Result<String, HttpFailure> {
    let response = client(timeout)
        .map_err(|err| HttpFailure {
            status: 0,
            message: err,
        })?
        .post(format!("{base}/chat/completions"))
        .bearer_auth(key)
        .header("user-agent", user_agent())
        .json(body)
        .send()
        .map_err(|err| HttpFailure {
            status: 0,
            message: format!("Rewrite request failed: {err}"),
        })?;
    let status = response.status();
    let text = response.text().map_err(|err| HttpFailure {
        status: status.as_u16(),
        message: format!("Could not read the rewrite: {err}"),
    })?;
    if !status.is_success() {
        return Err(HttpFailure {
            status: status.as_u16(),
            message: format!("Rewrite request failed ({}). {}", status, clip(&text)),
        });
    }
    let parsed: Value = serde_json::from_str(&text).map_err(|_| HttpFailure {
        status: status.as_u16(),
        message: "The rewrite response was not JSON.".into(),
    })?;
    message_text(&parsed).ok_or_else(|| HttpFailure {
        status: status.as_u16(),
        message: "The rewrite response had no text.".into(),
    })
}

fn retry_body(body: &Value, thinking: Thinking) -> Option<Value> {
    let mut retry = body.clone();
    let object = retry.as_object_mut()?;
    match thinking {
        Thinking::Dashscope => {
            object.remove("enable_thinking");
            object.insert("reasoning_effort".into(), json!("none"));
            Some(retry)
        }
        Thinking::Volcengine => {
            object.remove("temperature");
            Some(retry)
        }
        Thinking::OpenAiNone => {
            object.remove("temperature");
            object.insert("reasoning_effort".into(), json!("low"));
            Some(retry)
        }
        _ => None,
    }
}

fn message_text(value: &Value) -> Option<String> {
    let content = value.get("choices")?.get(0)?.get("message")?.get("content")?;
    let text = match content {
        Value::String(text) => text.clone(),
        Value::Array(parts) => {
            let mut out = String::new();
            for part in parts {
                if let Some(text) = part.get("text").and_then(Value::as_str) {
                    out.push_str(text);
                } else if let Some(text) = part.as_str() {
                    out.push_str(text);
                }
            }
            out
        }
        _ => return None,
    };
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn client(timeout: std::time::Duration) -> Result<reqwest::blocking::Client, String> {
    super::egress::blocking_client(super::egress::Purpose::Cloud, timeout)
}

fn user_agent() -> String {
    format!("newtype/{}", env!("CARGO_PKG_VERSION"))
}

fn clip(body: &str) -> String {
    let compact = body.split_whitespace().collect::<Vec<_>>().join(" ");
    compact.chars().take(180).collect()
}

pub fn redact(message: &str, key: &str) -> String {
    if key.is_empty() {
        return message.to_string();
    }
    message.replace(key, "[redacted]")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggested_models_match_the_verified_defaults() {
        let openai = preset("openai").unwrap();
        assert_eq!(openai.default_asr, "gpt-transcribe");
        assert_eq!(openai.default_llm, "gpt-6-luna");
        assert!(openai.asr_ready && openai.llm_ready);
        let volc = preset("volcengine").unwrap();
        assert!(volc.asr_ready && volc.llm_ready);
        assert_eq!(volc.default_asr, "bigmodel");
        assert_eq!(volc.asr_wire, AsrWire::Volcengine);
        assert_eq!(volc.default_llm, "doubao-seed-2-1-lite-260915");
        assert!(!volc.note.contains("doubao-seed-asr-1-0 is the default"));
        assert!(volc.note.contains("bigmodel"));
        assert!(preset("dashscope").unwrap().note.contains("Partial"));
    }

    #[test]
    fn thinking_is_off_where_the_model_allows_it() {
        let luna = chat_body("gpt-6-luna", "sys", "user", 64, Thinking::OpenAiNone);
        assert_eq!(luna["temperature"], 0);
        assert_eq!(luna["reasoning_effort"], "none");
        assert!(luna.get("max_completion_tokens").is_some());

        let astra = chat_body("gpt-6-astra", "sys", "user", 64, Thinking::OpenAiLow);
        assert!(astra.get("temperature").is_none());
        assert_eq!(astra["reasoning_effort"], "low");

        assert_eq!(
            thinking_for("openai", "gpt-6-luna", "auto", ""),
            Thinking::OpenAiNone
        );
        assert_eq!(
            thinking_for("openai", "gpt-6-astra", "auto", ""),
            Thinking::OpenAiLow
        );
        assert_eq!(
            thinking_for("groq", "openai/gpt-oss-20b", "auto", ""),
            Thinking::GroqLow
        );
        let dash = chat_body("qwen3.8-flash", "sys", "user", 64, Thinking::Dashscope);
        assert_eq!(dash["enable_thinking"], false);
        assert_eq!(dash["temperature"], 0);
        let volc = chat_body(
            "doubao-seed-2-1-lite-260915",
            "sys",
            "user",
            64,
            Thinking::Volcengine,
        );
        assert_eq!(volc["thinking"]["type"], "disabled");
    }

    #[test]
    fn bases_reject_cleartext_except_localhost_and_keys_are_redacted() {
        assert!(normalize_base("https://api.openai.com/v1/").is_ok());
        assert!(normalize_base("http://127.0.0.1:11434/v1").is_ok());
        assert!(normalize_base("http://example.com/v1").is_err());
        assert_eq!(
            redact("bearer sk-test failed", "sk-test"),
            "bearer [redacted] failed"
        );
    }

    #[test]
    fn strict_offline_and_local_only_do_not_open_a_cloud_request() {
        let started = std::time::Instant::now();
        let guard = crate::pipeline::egress::ModeGuard::strict();
        let err = transcribe(
            "http://127.0.0.1:9/v1",
            "secret-key",
            "gpt-transcribe",
            AsrWire::Transcriptions,
            "",
            b"RIFF",
        )
        .unwrap_err();
        assert!(err.contains("Strict offline"), "{err}");
        assert!(!err.contains("secret-key"));
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        drop(guard);

        let _local = crate::pipeline::egress::ModeGuard::local_only();
        let err = complete(
            "http://127.0.0.1:9/v1",
            "secret-key",
            "gpt-6-luna",
            Thinking::Off,
            "system",
            "user",
            32,
        )
        .unwrap_err();
        assert!(err.contains("Local only"), "{err}");
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
    }
}
