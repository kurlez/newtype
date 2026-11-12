//! Resident local engines. Weights load on the first local dictation, not when
//! Settings opens. One speech engine is resident at a time. The rewrite model
//! is a second llama-server, because one process cannot hold both GGUFs.
//!
//! llama-server binds 127.0.0.1 only. The HTTP client disables proxies so a
//! system proxy cannot carry the audio off the machine.

use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sherpa_onnx::{OfflineQwen3ASRModelConfig, OfflineRecognizer, OfflineRecognizerConfig};

use super::asr_text;
use super::catalog::{self, Mirror};
use super::extract;
use super::hotkey::OsKind;
use super::library::{self, InstallEvent};
use super::polish::LlmRequest;

const ASR_17: &str = "qwen3-asr-1.7b-q8";
const ASR_06: &str = "qwen3-asr-0.6b-int8";
const LOAD_TIMEOUT: Duration = Duration::from_secs(180);
const ASR_TIMEOUT: Duration = Duration::from_secs(45);
const LLM_TIMEOUT: Duration = Duration::from_secs(30);

pub struct Engines {
    root: PathBuf,
    asr: Mutex<Option<LoadedAsr>>,
    llm: Mutex<Option<LoadedLlm>>,
    job: Mutex<Option<DownloadJob>>,
}

struct DownloadJob {
    id: String,
    cancel: Arc<AtomicBool>,
}

enum LoadedAsr {
    Sherpa {
        id: String,
        recognizer: OfflineRecognizer,
        last: Instant,
    },
    Llama {
        id: String,
        sidecar: Sidecar,
        last: Instant,
    },
}

struct LoadedLlm {
    id: String,
    sidecar: Sidecar,
    last: Instant,
}

struct Sidecar {
    child: Child,
    port: u16,
    alias: String,
}

impl Drop for Sidecar {
    fn drop(&mut self) {
        kill_child(&mut self.child);
    }
}

impl Engines {
    pub fn new(root: PathBuf) -> Self {
        let _ = fs::create_dir_all(&root);
        Self {
            root,
            asr: Mutex::new(None),
            llm: Mutex::new(None),
            job: Mutex::new(None),
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn downloading(&self) -> String {
        self.job
            .lock()
            .expect("download job")
            .as_ref()
            .map(|job| job.id.clone())
            .unwrap_or_default()
    }

    pub fn cancel_download(&self) {
        if let Some(job) = self.job.lock().expect("download job").as_ref() {
            job.cancel.store(true, Ordering::Relaxed);
        }
    }

    pub fn shutdown(&self) {
        self.cancel_download();
        *self.asr.lock().expect("asr engine") = None;
        *self.llm.lock().expect("llm engine") = None;
    }

    pub fn reap(&self, idle: Duration) {
        {
            let mut slot = self.asr.lock().expect("asr engine");
            if slot.as_ref().is_some_and(|loaded| loaded.idle_for() >= idle) {
                *slot = None;
                crate::diag::record("unloaded local speech model after idle");
            }
        }
        let mut slot = self.llm.lock().expect("llm engine");
        if slot.as_ref().is_some_and(|loaded| loaded.last.elapsed() >= idle) {
            *slot = None;
            crate::diag::record("unloaded local rewrite model after idle");
        }
    }

    pub fn retain(&self, asr_id: &str, llm_id: &str) {
        {
            let mut slot = self.asr.lock().expect("asr engine");
            if slot.as_ref().is_some_and(|loaded| loaded.id() != asr_id) {
                *slot = None;
            }
        }
        let mut slot = self.llm.lock().expect("llm engine");
        if slot.as_ref().is_some_and(|loaded| loaded.id != llm_id) {
            *slot = None;
        }
    }

    pub fn spawn_download(
        self: &Arc<Self>,
        app: tauri::AppHandle,
        ids: Vec<String>,
        os: OsKind,
        mirror: Mirror,
    ) -> Result<(), String> {
        if ids.is_empty() {
            return Err("Choose a model to download.".into());
        }
        for id in &ids {
            if catalog::card(id).is_none() {
                return Err(format!("Unknown model \"{id}\"."));
            }
        }
        let label = ids.join(", ");
        let mut job = self.job.lock().expect("download job");
        if let Some(active) = job.as_ref() {
            return Err(format!("A download is already running ({}).", active.id));
        }
        let cancel = Arc::new(AtomicBool::new(false));
        *job = Some(DownloadJob {
            id: label.clone(),
            cancel: Arc::clone(&cancel),
        });
        drop(job);
        let hub = Arc::clone(self);
        std::thread::Builder::new()
            .name("newtype-download".into())
            .spawn(move || {
                for id in &ids {
                    if cancel.load(Ordering::Relaxed) {
                        emit_progress(
                            &app,
                            id,
                            &InstallEvent {
                                phase: "cancelled".into(),
                                received: 0,
                                total: 0,
                                message: "Download cancelled.".into(),
                            },
                        );
                        break;
                    }
                    let mut last_phase = String::new();
                    let mut last_emit = Instant::now() - Duration::from_secs(1);
                    let outcome = library::install(&hub.root, id, os, mirror, &cancel, |event| {
                        let phase_changed = event.phase != last_phase;
                        if phase_changed {
                            crate::diag::record(&format!("model {id} {}", event.phase));
                            last_phase.clone_from(&event.phase);
                        }
                        if phase_changed || last_emit.elapsed() >= Duration::from_millis(250) {
                            last_emit = Instant::now();
                            emit_progress(&app, id, &event);
                        }
                    });
                    if let Err(err) = outcome {
                        let _ = library::write_error(&library::model_dir(&hub.root, id), &err);
                        crate::diag::record(&format!("model {id} failed"));
                        emit_progress(
                            &app,
                            id,
                            &InstallEvent {
                                phase: "failed".into(),
                                received: 0,
                                total: 0,
                                message: err,
                            },
                        );
                        break;
                    }
                }
                let mut job = hub.job.lock().expect("download job");
                if job.as_ref().is_some_and(|active| active.id == label) {
                    *job = None;
                }
            })
            .map_err(|err| format!("Could not start the download: {err}"))?;
        Ok(())
    }

    pub fn transcribe(&self, id: &str, samples: &[f32], wav: &[u8], report: &dyn Fn(&str)) -> Result<String, String> {
        let text = if id == ASR_06 {
            self.transcribe_sherpa(samples, report)?
        } else if id == ASR_17 {
            self.transcribe_llama(wav, report)?
        } else {
            return Err(format!("\"{id}\" is not a local speech model."));
        };
        let text = asr_text::split_asr_text(&text);
        if text.trim().is_empty() {
            return Err("Local speech recognition returned no text.".into());
        }
        crate::diag::record(&format!("local speech ok engine={id} chars={}", text.chars().count()));
        Ok(text)
    }

    pub fn complete(&self, id: &str, request: &LlmRequest, report: &dyn Fn(&str)) -> Result<String, String> {
        report("Loading the local rewrite model…");
        let mut slot = self.llm.lock().expect("llm engine");
        let needs_start = slot.as_ref().is_none_or(|loaded| loaded.id != id);
        if needs_start {
            *slot = None;
            let sidecar = start_llm(&self.root, id)?;
            *slot = Some(LoadedLlm {
                id: id.to_string(),
                sidecar,
                last: Instant::now(),
            });
        }
        let loaded = slot.as_mut().expect("llm just started");
        loaded.last = Instant::now();
        let port = loaded.sidecar.port;
        let alias = loaded.sidecar.alias.clone();
        drop(slot);
        report("Rewriting on this computer…");
        chat_complete(port, &alias, request)
    }

    fn transcribe_sherpa(&self, samples: &[f32], report: &dyn Fn(&str)) -> Result<String, String> {
        report("Loading the local speech model…");
        let mut slot = self.asr.lock().expect("asr engine");
        let ready = matches!(&*slot, Some(LoadedAsr::Sherpa { id, .. }) if id == ASR_06);
        if !ready {
            *slot = None;
            let recognizer = load_sherpa(&self.root)?;
            *slot = Some(LoadedAsr::Sherpa {
                id: ASR_06.to_string(),
                recognizer,
                last: Instant::now(),
            });
        }
        let Some(LoadedAsr::Sherpa { recognizer, last, .. }) = slot.as_mut() else {
            return Err("The local speech model is not loaded.".into());
        };
        *last = Instant::now();
        report("Recognizing on this computer…");
        let stream = recognizer.create_stream();
        stream.accept_waveform(16_000, samples);
        recognizer.decode(&stream);
        Ok(stream.get_result().map(|result| result.text).unwrap_or_default())
    }

    fn transcribe_llama(&self, wav: &[u8], report: &dyn Fn(&str)) -> Result<String, String> {
        report("Loading the local speech model…");
        let mut slot = self.asr.lock().expect("asr engine");
        let ready = matches!(&*slot, Some(LoadedAsr::Llama { id, .. }) if id == ASR_17);
        if !ready {
            *slot = None;
            let sidecar = start_asr(&self.root)?;
            *slot = Some(LoadedAsr::Llama {
                id: ASR_17.to_string(),
                sidecar,
                last: Instant::now(),
            });
        }
        let Some(LoadedAsr::Llama { sidecar, last, .. }) = slot.as_mut() else {
            return Err("The local speech model is not loaded.".into());
        };
        *last = Instant::now();
        let port = sidecar.port;
        drop(slot);
        report("Recognizing on this computer…");
        transcribe_http(port, wav)
    }
}

impl LoadedAsr {
    fn id(&self) -> &str {
        match self {
            Self::Sherpa { id, .. } | Self::Llama { id, .. } => id,
        }
    }

    fn idle_for(&self) -> Duration {
        match self {
            Self::Sherpa { last, .. } | Self::Llama { last, .. } => last.elapsed(),
        }
    }
}

fn load_sherpa(root: &Path) -> Result<OfflineRecognizer, String> {
    let dir = library::model_dir(root, ASR_06);
    let conv = extract::find_named(&dir, "conv_frontend.onnx")
        .ok_or_else(|| "Qwen3-ASR 0.6B is not installed yet.".to_string())?;
    let encoder = extract::find_named(&dir, "encoder.int8.onnx")
        .ok_or_else(|| "Qwen3-ASR 0.6B is missing encoder.int8.onnx.".to_string())?;
    let decoder = extract::find_named(&dir, "decoder.int8.onnx")
        .ok_or_else(|| "Qwen3-ASR 0.6B is missing decoder.int8.onnx.".to_string())?;
    let tokenizer = extract::find_named(&dir, "tokenizer")
        .ok_or_else(|| "Qwen3-ASR 0.6B is missing its tokenizer.".to_string())?;
    let mut config = OfflineRecognizerConfig::default();
    config.model_config.qwen3_asr = OfflineQwen3ASRModelConfig {
        conv_frontend: Some(conv.display().to_string()),
        encoder: Some(encoder.display().to_string()),
        decoder: Some(decoder.display().to_string()),
        tokenizer: Some(tokenizer.display().to_string()),
        max_total_len: 8192,
        max_new_tokens: 2048,
        ..OfflineQwen3ASRModelConfig::default()
    };
    config.model_config.tokens = Some(String::new());
    config.model_config.provider = Some("cpu".into());
    config.model_config.num_threads = 4;
    config.model_config.debug = false;
    OfflineRecognizer::create(&config).ok_or_else(|| "Could not load Qwen3-ASR 0.6B.".to_string())
}

fn start_asr(root: &Path) -> Result<Sidecar, String> {
    let dir = library::model_dir(root, ASR_17);
    let model = dir.join("Qwen3-ASR-1.7B-Q8_0.gguf");
    let mmproj = dir.join("mmproj-Qwen3-ASR-1.7B-Q8_0.gguf");
    if !model.is_file() || !mmproj.is_file() {
        return Err("Qwen3-ASR 1.7B is not installed yet. The decoder and mmproj are one model.".into());
    }
    start_server(
        root,
        ServerSpec {
            model,
            mmproj: Some(mmproj),
            context: 2048,
            alias: "qwen3-asr".into(),
        },
    )
}

fn start_llm(root: &Path, id: &str) -> Result<Sidecar, String> {
    let file = match id {
        "qwen3-4b-q4" => "Qwen3-4B-Q4_K_M.gguf",
        "qwen3-1.7b-q4" => "Qwen3-1.7B-Q4_K_M.gguf",
        other => return Err(format!("\"{other}\" is not a local rewrite model.")),
    };
    let model = library::model_dir(root, id).join(file);
    if !model.is_file() {
        return Err("The local rewrite model is not installed yet.".into());
    }
    start_server(
        root,
        ServerSpec {
            model,
            mmproj: None,
            context: 4096,
            alias: "local-llm".into(),
        },
    )
}

struct ServerSpec {
    model: PathBuf,
    mmproj: Option<PathBuf>,
    context: u32,
    alias: String,
}

fn start_server(root: &Path, spec: ServerSpec) -> Result<Sidecar, String> {
    let binary = llama_binary(root)?;
    let port = free_port()?;
    let log_path = library::model_dir(root, "llama-cpp-b12292").join("llama-server.log");
    let _ = fs::create_dir_all(log_path.parent().unwrap_or(Path::new(".")));
    let log_file = File::create(&log_path).map_err(|err| format!("Could not open the llama-server log: {err}"))?;
    let stderr = Stdio::from(log_file);
    let mut command = Command::new(&binary);
    command
        .arg("-m")
        .arg(&spec.model)
        .arg("-c")
        .arg(spec.context.to_string())
        .arg("-ngl")
        .arg(gpu_layers().to_string())
        .arg("--parallel")
        .arg("1")
        .arg("--cache-ram")
        .arg("0")
        .arg("--no-warmup")
        .arg("--reasoning")
        .arg("off")
        .arg("--alias")
        .arg(&spec.alias)
        .arg("--host")
        .arg("127.0.0.1")
        .arg("--port")
        .arg(port.to_string())
        .current_dir(binary.parent().unwrap_or(Path::new(".")))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(stderr);
    if let Some(mmproj) = &spec.mmproj {
        command.arg("--mmproj").arg(mmproj);
    }
    prepare_library_path(&mut command, &binary);
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let child = command.spawn().map_err(|err| format!("Could not start llama-server: {err}"))?;
    crate::diag::record(&format!("llama-server starting alias={} port={port}", spec.alias));
    let mut sidecar = Sidecar {
        child,
        port,
        alias: spec.alias,
    };
    if let Err(err) = wait_healthy(&mut sidecar, &log_path) {
        return Err(err);
    }
    crate::diag::record(&format!("llama-server ready port={port}"));
    Ok(sidecar)
}

fn llama_binary(root: &Path) -> Result<PathBuf, String> {
    let dir = library::model_dir(root, "llama-cpp-b12292");
    extract::find_named(&dir, "llama-server.exe")
        .or_else(|| extract::find_named(&dir, "llama-server"))
        .ok_or_else(|| "llama.cpp is not installed yet. Download a local model to install the runtime.".to_string())
}

fn gpu_layers() -> u32 {
    if cfg!(target_os = "macos") {
        99
    } else {
        0
    }
}

fn prepare_library_path(command: &mut Command, binary: &Path) {
    let Some(parent) = binary.parent() else {
        return;
    };
    let parent = parent.display().to_string();
    #[cfg(windows)]
    {
        let path = std::env::var("PATH").unwrap_or_default();
        command.env("PATH", format!("{parent};{path}"));
    }
    #[cfg(unix)]
    {
        let dyld = std::env::var("DYLD_LIBRARY_PATH").unwrap_or_default();
        command.env("DYLD_LIBRARY_PATH", format!("{parent}:{dyld}"));
        let ld = std::env::var("LD_LIBRARY_PATH").unwrap_or_default();
        command.env("LD_LIBRARY_PATH", format!("{parent}:{ld}"));
    }
}

fn free_port() -> Result<u16, String> {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").map_err(|err| format!("Could not reserve a local port: {err}"))?;
    let port = listener.local_addr().map_err(|err| err.to_string())?.port();
    drop(listener);
    Ok(port)
}

fn wait_healthy(sidecar: &mut Sidecar, log_path: &Path) -> Result<(), String> {
    let client = loopback_client(Duration::from_secs(2))?;
    let deadline = Instant::now() + LOAD_TIMEOUT;
    loop {
        if sidecar.child.try_wait().map_err(|err| err.to_string())?.is_some() {
            return Err(format!(
                "llama-server exited while loading. {}",
                log_tail(log_path)
            ));
        }
        if client
            .get(format!("http://127.0.0.1:{}/health", sidecar.port))
            .send()
            .ok()
            .is_some_and(|response| response.status().is_success())
        {
            return Ok(());
        }
        if Instant::now() >= deadline {
            kill_child(&mut sidecar.child);
            return Err(format!(
                "llama-server did not become ready. {}",
                log_tail(log_path)
            ));
        }
        std::thread::sleep(Duration::from_millis(400));
    }
}

fn transcribe_http(port: u16, wav: &[u8]) -> Result<String, String> {
    let part = reqwest::blocking::multipart::Part::bytes(wav.to_vec())
        .file_name("utterance.wav")
        .mime_str("audio/wav")
        .map_err(|err| err.to_string())?;
    let form = reqwest::blocking::multipart::Form::new()
        .part("file", part)
        .text("response_format", "json")
        .text("temperature", "0");
    let response = loopback_client(ASR_TIMEOUT)?
        .post(format!("http://127.0.0.1:{port}/v1/audio/transcriptions"))
        .multipart(form)
        .send()
        .map_err(|err| format!("Local speech recognition failed: {err}"))?;
    let status = response.status();
    let body = response.text().unwrap_or_default();
    if !status.is_success() {
        return Err(format!("Local speech recognition returned HTTP {}. {}", status.as_u16(), clip(&body)));
    }
    let value: serde_json::Value = serde_json::from_str(&body).map_err(|_| format!("Local speech recognition returned an unexpected response. {}", clip(&body)))?;
    if let Some(error) = value.get("error") {
        return Err(format!("Local speech recognition failed: {}", clip(&error.to_string())));
    }
    value
        .get("text")
        .and_then(|text| text.as_str())
        .map(|text| text.to_string())
        .ok_or_else(|| "Local speech recognition returned no text.".to_string())
}

fn chat_complete(port: u16, alias: &str, request: &LlmRequest) -> Result<String, String> {
    let user = format!("/no_think\n{}", request.user);
    let with_flag = chat_body(alias, request, &user, true);
    let response = post_chat(port, &with_flag)?;
    if response.status == 400 {
        let plain = chat_body(alias, request, &user, false);
        let retry = post_chat(port, &plain)?;
        return chat_text(retry.status, &retry.body);
    }
    chat_text(response.status, &response.body)
}

struct ChatResponse {
    status: u16,
    body: String,
}

fn post_chat(port: u16, body: &serde_json::Value) -> Result<ChatResponse, String> {
    let response = loopback_client(LLM_TIMEOUT)?
        .post(format!("http://127.0.0.1:{port}/v1/chat/completions"))
        .json(body)
        .send()
        .map_err(|err| format!("Local rewrite failed: {err}"))?;
    let status = response.status().as_u16();
    let body = response.text().unwrap_or_default();
    Ok(ChatResponse { status, body })
}

fn chat_body(alias: &str, request: &LlmRequest, user: &str, thinking_flag: bool) -> serde_json::Value {
    let mut body = serde_json::json!({
        "model": alias,
        "temperature": 0,
        "max_tokens": request.max_tokens,
        "messages": [
            {"role": "system", "content": request.system},
            {"role": "user", "content": user}
        ]
    });
    if thinking_flag {
        body["chat_template_kwargs"] = serde_json::json!({"enable_thinking": false});
    }
    body
}

fn chat_text(status: u16, body: &str) -> Result<String, String> {
    if status >= 400 {
        return Err(format!("Local rewrite returned HTTP {status}. {}", clip(body)));
    }
    let value: serde_json::Value = serde_json::from_str(body).map_err(|_| format!("Local rewrite returned an unexpected response. {}", clip(body)))?;
    if let Some(error) = value.get("error") {
        return Err(format!("Local rewrite failed: {}", clip(&error.to_string())));
    }
    let content = value["choices"][0]["message"]["content"].as_str().unwrap_or("").trim();
    let content = content.strip_prefix("/no_think").unwrap_or(content).trim();
    if content.is_empty() {
        return Err("The local rewrite model returned no text.".into());
    }
    Ok(content.to_string())
}

/// On-device llama-server only. This client is not the outbound gate: strict
/// offline still has to reach `127.0.0.1`, and `no_proxy` keeps a system proxy
/// from forwarding it.
fn loopback_client(timeout: Duration) -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .timeout(timeout)
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|err| err.to_string())
}

fn kill_child(child: &mut Child) {
    #[cfg(unix)]
    {
        let pid = child.id();
        let _ = Command::new("kill").args(["-KILL", &format!("-{pid}")]).status();
    }
    let _ = child.kill();
    let _ = child.wait();
}

fn log_tail(path: &Path) -> String {
    let Ok(text) = fs::read_to_string(path) else {
        return "No llama-server log was written.".into();
    };
    let tail: String = text.chars().rev().take(180).collect::<String>().chars().rev().collect();
    let one_line = tail.replace('\n', " ");
    clip(&one_line)
}

fn clip(text: &str) -> String {
    let mut out = text.trim().to_string();
    if out.len() > 180 {
        out.truncate(180);
    }
    out
}

fn emit_progress(app: &tauri::AppHandle, id: &str, event: &InstallEvent) {
    use tauri::Emitter;
    let _ = app.emit(
        "local://progress",
        serde_json::json!({
            "id": id,
            "phase": event.phase,
            "received": event.received,
            "total": event.total,
            "message": event.message,
        }),
    );
}
