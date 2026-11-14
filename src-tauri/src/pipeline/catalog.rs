//! Pinned local models. Weights are not in the installer.
//!
//! Hashes were read on 2026-11-12: Hugging Face LFS oids for the GGUF files,
//! and GitHub release asset digests for llama.cpp `b12292`, Silero VAD, and
//! the sherpa-onnx 0.6B archive. The macOS and Windows llama.cpp archives were
//! downloaded and hashed again in this workspace; both digests matched.

use crate::pipeline::hotkey::{host_os, OsKind};

pub const HIGH_RAM_MIN_BYTES: u64 = 15 << 30;

const ASR_17_SHA: &str = "58e22d0532d4eacaf034cfac17a6fed159f37c41390c710186783be439d1fc57";
const ASR_17_SIZE: u64 = 2_165_034_944;
const ASR_17_PROJ_SHA: &str = "46c1d533af3f354ceb37ce855dbceff7da7fa7cf1e6a523df3b13440bd164c0d";
const ASR_17_PROJ_SIZE: u64 = 355_709_344;
const ASR_06_SHA: &str = "393f8a14e2f5fb96746aaab342997a40641001fbd5bf9592a080a8329178ee96";
const ASR_06_SIZE: u64 = 878_702_423;
const LLM_4B_SHA: &str = "7485fe6f11af29433bc51cab58009521f205840f5b4ae3a32fa7f92e8534fdf5";
const LLM_4B_SIZE: u64 = 2_497_280_256;
const LLM_17_SHA: &str = "b139949c5bd74937ad8ed8c8cf3d9ffb1e99c866c823204dc42c0d91fa181897";
const LLM_17_SIZE: u64 = 1_107_409_472;
const SILERO_SHA: &str = "9e2449e1087496d8d4caba907f23e0bd3f78d91fa552479bb9c23ac09cbb1fd6";
const SILERO_SIZE: u64 = 643_854;
const LLAMA_MAC_SHA: &str = "94b860a529a64aecfe20c4b9ccb9ca6fb4cf9126b019f3380ee02d7951ca11ef";
const LLAMA_MAC_SIZE: u64 = 16_870_580;
const LLAMA_WIN_SHA: &str = "12017843d67f560ce1bacf771dccdb189ff651a2dde684cbb49634c29df5b751";
const LLAMA_WIN_SIZE: u64 = 25_057_882;
/// Official sherpa-onnx export of FunAudioLLM SenseVoice-Small, int8.
/// SHA-256 is the Hugging Face LFS oid, checked against the k2-fsa release
/// tarball `sherpa-onnx-sense-voice-zh-en-ja-ko-yue-int8-2024-07-17.tar.bz2`
/// on 2026-11-12. ModelScope does not publish these bytes.
const SENSE_MODEL_SHA: &str = "c71f0ce00bec95b07744e116345e33d8cbbe08cef896382cf907bf4b51a2cd51";
const SENSE_MODEL_SIZE: u64 = 239_233_841;
const SENSE_TOKENS_SHA: &str = "f449eb28dc567533d7fa59be34e2abca8784f771850c78a47fb731a31429a1dc";
const SENSE_TOKENS_SIZE: u64 = 315_894;
pub const SENSEVOICE_ID: &str = "sensevoice-small-int8";
const SENSE_REPO: &str = "csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mirror {
    Auto,
    HuggingFace,
    HfMirror,
    ModelScope,
    GitHub,
}

impl Mirror {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "auto" | "" => Some(Self::Auto),
            "huggingface" | "hf" => Some(Self::HuggingFace),
            "hf-mirror" | "hfMirror" => Some(Self::HfMirror),
            "modelscope" => Some(Self::ModelScope),
            "github" => Some(Self::GitHub),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::HuggingFace => "huggingface",
            Self::HfMirror => "hf-mirror",
            Self::ModelScope => "modelscope",
            Self::GitHub => "github",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::HuggingFace => "Hugging Face",
            Self::HfMirror => "hf-mirror",
            Self::ModelScope => "ModelScope",
            Self::GitHub => "GitHub",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Asr,
    Llm,
    Vad,
    Runtime,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Asr => "asr",
            Self::Llm => "llm",
            Self::Vad => "vad",
            Self::Runtime => "runtime",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Url {
    pub mirror: Mirror,
    pub url: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct RemoteFile {
    pub name: &'static str,
    pub sha256: &'static str,
    pub size: u64,
    pub urls: &'static [Url],
    pub extract: bool,
    pub required: &'static [&'static str],
}

#[derive(Debug, Clone, Copy)]
pub struct Card {
    pub id: &'static str,
    pub name: &'static str,
    pub role: Role,
    pub license: &'static str,
    pub languages: &'static str,
    pub runtime: &'static str,
    pub memory: &'static str,
    pub notes: &'static str,
    pub platforms: &'static [OsKind],
    pub depends_on: &'static [&'static str],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hardware {
    pub os: OsKind,
    pub ram_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recommendation {
    pub asr_id: String,
    pub llm_id: String,
    pub summary: String,
}

const MAC_WIN: &[OsKind] = &[OsKind::Mac, OsKind::Windows];
const MAC_ONLY: &[OsKind] = &[OsKind::Mac];

pub fn cards() -> &'static [Card] {
    &[
        Card {
            id: "qwen3-asr-1.7b-q8",
            name: "Qwen3-ASR 1.7B Q8_0",
            role: Role::Asr,
            license: "Apache-2.0",
            languages: "30 languages, Chinese dialects, Chinese-English mix",
            runtime: "llama.cpp llama-server (Metal sidecar)",
            memory: "About 2.4 GB on disk. The owner's M4 Pro peaked near 2.8 GB.",
            notes: "Mac default when this computer has 16 GB of RAM or more. The 0.6B model is the fallback if this server fails.",
            platforms: MAC_ONLY,
            depends_on: &["silero-vad", "llama-cpp-b12292"],
        },
        Card {
            id: "qwen3-asr-0.6b-int8",
            name: "Qwen3-ASR 0.6B int8",
            role: Role::Asr,
            license: "Apache-2.0 (weights and sherpa-onnx)",
            languages: "30 languages, Chinese dialects, Chinese-English mix",
            runtime: "sherpa-onnx, in process",
            memory: "About 0.9 GB on disk. The benchmark peaked near 2.6–3.3 GB.",
            notes: "Windows default. Also the Mac model below 16 GB, and the Mac fallback when 1.7B is missing or fails.",
            platforms: MAC_WIN,
            depends_on: &["silero-vad"],
        },
        Card {
            id: SENSEVOICE_ID,
            name: "SenseVoice-Small int8",
            role: Role::Asr,
            license: "FunASR Model License (attribution required)",
            languages: "Chinese, English, Japanese, Korean, Cantonese",
            runtime: "sherpa-onnx, in process",
            memory: "About 228 MB of weights. Lighter than Qwen3-ASR.",
            notes: "Optional lightweight speech model. Not the local default. Inverse text normalization stays on so punctuation is kept. Hugging Face and hf-mirror only; ModelScope does not host this export.",
            platforms: MAC_WIN,
            depends_on: &["silero-vad"],
        },
        Card {
            id: "qwen3-4b-q4",
            name: "Qwen3 4B Q4_K_M",
            role: Role::Llm,
            license: "Apache-2.0",
            languages: "Multilingual rewrite",
            runtime: "llama.cpp llama-server",
            memory: "About 2.5 GB on disk. Thinking is forced off.",
            notes: "Default local rewrite model on a Mac at 16 GB and above. Official Qwen GGUF.",
            platforms: MAC_WIN,
            depends_on: &["llama-cpp-b12292"],
        },
        Card {
            id: "qwen3-1.7b-q4",
            name: "Qwen3 1.7B Q4_K_M",
            role: Role::Llm,
            license: "Apache-2.0",
            languages: "Multilingual rewrite",
            runtime: "llama.cpp llama-server",
            memory: "About 1.1 GB on disk. Thinking is forced off.",
            notes: "Default local rewrite model on Windows, and on a Mac below 16 GB. The official Qwen GGUF repo publishes Q8_0 only, so this file is the Unsloth Q4_K_M quant of the same model.",
            platforms: MAC_WIN,
            depends_on: &["llama-cpp-b12292"],
        },
        Card {
            id: "silero-vad",
            name: "Silero VAD",
            role: Role::Vad,
            license: "MIT",
            languages: "Speech / silence",
            runtime: "sherpa-onnx",
            memory: "Under 1 MB.",
            notes: "Trims silence before local speech recognition. Installed with a speech model.",
            platforms: MAC_WIN,
            depends_on: &[],
        },
        Card {
            id: "llama-cpp-b12292",
            name: "llama.cpp b12292",
            role: Role::Runtime,
            license: "MIT",
            languages: "",
            runtime: "sidecar",
            memory: "About 17 MB on Mac, about 25 MB on Windows (CPU build).",
            notes: "Runs the 1.7B speech model on Mac and the rewrite model on both desktops. Not a weight file. Windows uses the CPU build so a machine without Vulkan or CUDA still rewrites.",
            platforms: MAC_WIN,
            depends_on: &[],
        },
    ]
}

pub fn card(id: &str) -> Option<&'static Card> {
    cards().iter().find(|card| card.id == id)
}

pub fn files_for(id: &str, os: OsKind) -> Result<&'static [RemoteFile], String> {
    match id {
        "qwen3-asr-1.7b-q8" => Ok(asr_17_files()),
        "qwen3-asr-0.6b-int8" => Ok(asr_06_files()),
        SENSEVOICE_ID => Ok(sensevoice_files()),
        "qwen3-4b-q4" => Ok(llm_4b_files()),
        "qwen3-1.7b-q4" => Ok(llm_17_files()),
        "silero-vad" => Ok(silero_files()),
        "llama-cpp-b12292" => match os {
            OsKind::Windows => Ok(llama_win_files()),
            OsKind::Mac => Ok(llama_mac_files()),
            OsKind::Other => Err(
                "The llama.cpp build is packaged for macOS Apple Silicon and Windows x64. It is not available on this OS yet.".into(),
            ),
        },
        other => Err(format!("Unknown model \"{other}\".")),
    }
}

pub fn listed_on(card: &Card, os: OsKind) -> bool {
    os == OsKind::Other || card.platforms.contains(&os)
}

pub fn runnable_on(card: &Card, os: OsKind) -> bool {
    card.platforms.contains(&os)
}

pub fn recommend(hardware: Hardware) -> Recommendation {
    let high = hardware.ram_bytes >= HIGH_RAM_MIN_BYTES;
    let (asr_id, llm_id) = match hardware.os {
        OsKind::Windows => ("qwen3-asr-0.6b-int8", "qwen3-1.7b-q4"),
        OsKind::Mac | OsKind::Other => {
            if high {
                ("qwen3-asr-1.7b-q8", "qwen3-4b-q4")
            } else {
                ("qwen3-asr-0.6b-int8", "qwen3-1.7b-q4")
            }
        }
    };
    let gib = hardware.ram_bytes as f64 / (1024.0 * 1024.0 * 1024.0);
    let asr_name = card(asr_id).map(|item| item.name).unwrap_or(asr_id);
    let llm_name = card(llm_id).map(|item| item.name).unwrap_or(llm_id);
    let summary = match hardware.os {
        OsKind::Windows => format!(
            "Windows with {gib:.1} GB RAM. Local speech is {asr_name}. Local rewrite is {llm_name}."
        ),
        OsKind::Mac => format!(
            "Mac with {gib:.1} GB RAM. Local speech is {asr_name}. Local rewrite is {llm_name}. 0.6B int8 stays available as the speech fallback."
        ),
        OsKind::Other => format!(
            "This OS is not a shipping target. The recommendation matches a Mac with {gib:.1} GB RAM: {asr_name} and {llm_name}."
        ),
    };
    Recommendation {
        asr_id: asr_id.to_string(),
        llm_id: llm_id.to_string(),
        summary,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalAsrBackend {
    QwenSherpa,
    SenseVoice,
    QwenLlama,
}

pub fn local_asr_backend(id: &str) -> Option<LocalAsrBackend> {
    match id {
        "qwen3-asr-0.6b-int8" => Some(LocalAsrBackend::QwenSherpa),
        SENSEVOICE_ID => Some(LocalAsrBackend::SenseVoice),
        "qwen3-asr-1.7b-q8" => Some(LocalAsrBackend::QwenLlama),
        _ => None,
    }
}

pub fn attribution(sensevoice_installed: bool) -> String {
    let mut text = String::from(
        "Cleanup prompts keep the MIT attribution for ckckck/OpenTypeless. No prompt text is taken from Open-Less/openless. Qwen3-ASR and Qwen3 weights are Apache-2.0. sherpa-onnx is Apache-2.0. llama.cpp is MIT. Silero VAD is MIT. newtype is not affiliated with or endorsed by Typeless.",
    );
    if sensevoice_installed {
        text.push_str(" SenseVoice-Small is installed. Its weights use the FunASR Model License and require attribution to FunAudioLLM / Alibaba.");
    }
    text
}

pub fn active_id(role: Role, selected: &str, hardware: Hardware) -> String {
    if let Some(card) = card(selected) {
        if card.role == role && listed_on(card, hardware.os) && (hardware.os == OsKind::Other || runnable_on(card, hardware.os)) {
            return card.id.to_string();
        }
    }
    let recommendation = recommend(hardware);
    match role {
        Role::Asr => recommendation.asr_id,
        Role::Llm => recommendation.llm_id,
        Role::Vad => "silero-vad".into(),
        Role::Runtime => "llama-cpp-b12292".into(),
    }
}

pub fn urls_for(file: &RemoteFile, preference: Mirror) -> Result<Vec<String>, String> {
    let chosen: Vec<String> = if preference == Mirror::Auto {
        file.urls.iter().map(|item| item.url.to_string()).collect()
    } else {
        file.urls
            .iter()
            .filter(|item| item.mirror == preference)
            .map(|item| item.url.to_string())
            .collect()
    };
    if chosen.is_empty() {
        let available = file
            .urls
            .iter()
            .map(|item| item.mirror.label())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(format!(
            "This file is not published on {}. Available sources: {available}.",
            preference.label()
        ));
    }
    Ok(chosen)
}

pub fn host_hardware() -> Hardware {
    Hardware {
        os: host_os(),
        ram_bytes: physical_ram_bytes(),
    }
}

fn physical_ram_bytes() -> u64 {
    #[cfg(target_os = "macos")]
    {
        if let Some(bytes) = sysctl_memsize() {
            return bytes;
        }
    }
    #[cfg(target_os = "windows")]
    {
        if let Some(bytes) = windows_memsize() {
            return bytes;
        }
    }
    #[cfg(target_os = "linux")]
    {
        if let Some(bytes) = linux_memsize() {
            return bytes;
        }
    }
    0
}

#[cfg(target_os = "macos")]
fn sysctl_memsize() -> Option<u64> {
    let output = std::process::Command::new("/usr/sbin/sysctl")
        .args(["-n", "hw.memsize"])
        .output()
        .ok()?;
    String::from_utf8(output.stdout).ok()?.trim().parse().ok()
}

#[cfg(target_os = "linux")]
fn linux_memsize() -> Option<u64> {
    let text = std::fs::read_to_string("/proc/meminfo").ok()?;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("MemTotal:") {
            let kb: u64 = rest.split_whitespace().next()?.parse().ok()?;
            return Some(kb.saturating_mul(1024));
        }
    }
    None
}

#[cfg(target_os = "windows")]
fn windows_memsize() -> Option<u64> {
    #[repr(C)]
    struct MemoryStatusEx {
        length: u32,
        memory_load: u32,
        total_phys: u64,
        avail_phys: u64,
        total_page_file: u64,
        avail_page_file: u64,
        total_virtual: u64,
        avail_virtual: u64,
        avail_extended_virtual: u64,
    }
    #[link(name = "kernel32")]
    extern "system" {
        fn GlobalMemoryStatusEx(buffer: *mut MemoryStatusEx) -> i32;
    }
    let mut status = MemoryStatusEx {
        length: std::mem::size_of::<MemoryStatusEx>() as u32,
        memory_load: 0,
        total_phys: 0,
        avail_phys: 0,
        total_page_file: 0,
        avail_page_file: 0,
        total_virtual: 0,
        avail_virtual: 0,
        avail_extended_virtual: 0,
    };
    let ok = unsafe { GlobalMemoryStatusEx(&mut status) };
    if ok == 0 {
        None
    } else {
        Some(status.total_phys)
    }
}

fn hf(repo: &str, file: &str) -> &'static [Url] {
    // The process keeps these URL strings. There are only a handful of model files.
    let hugging = format!("https://huggingface.co/{repo}/resolve/main/{file}");
    let mirror = format!("https://hf-mirror.com/{repo}/resolve/main/{file}");
    let scope = format!(
        "https://www.modelscope.cn/api/v1/models/{repo}/repo?Revision=master&FilePath={file}"
    );
    let urls = vec![
        Url {
            mirror: Mirror::HuggingFace,
            url: Box::leak(hugging.into_boxed_str()),
        },
        Url {
            mirror: Mirror::HfMirror,
            url: Box::leak(mirror.into_boxed_str()),
        },
        Url {
            mirror: Mirror::ModelScope,
            url: Box::leak(scope.into_boxed_str()),
        },
    ];
    Box::leak(urls.into_boxed_slice())
}

fn hosted(repo: &str, file: &str) -> &'static [Url] {
    let hugging = format!("https://huggingface.co/{repo}/resolve/main/{file}");
    let mirror = format!("https://hf-mirror.com/{repo}/resolve/main/{file}");
    let urls = vec![
        Url {
            mirror: Mirror::HuggingFace,
            url: Box::leak(hugging.into_boxed_str()),
        },
        Url {
            mirror: Mirror::HfMirror,
            url: Box::leak(mirror.into_boxed_str()),
        },
    ];
    Box::leak(urls.into_boxed_slice())
}

fn github(url: &'static str) -> &'static [Url] {
    Box::leak(
        vec![Url {
            mirror: Mirror::GitHub,
            url,
        }]
        .into_boxed_slice(),
    )
}

// The URL tables are built once. `hf()` leaks a few short strings for the process lifetime.
fn file_set(files: Vec<RemoteFile>) -> &'static [RemoteFile] {
    Box::leak(files.into_boxed_slice())
}

fn asr_17() -> &'static [RemoteFile] {
    file_set(vec![
        RemoteFile {
            name: "Qwen3-ASR-1.7B-Q8_0.gguf",
            sha256: ASR_17_SHA,
            size: ASR_17_SIZE,
            urls: hf("ggml-org/Qwen3-ASR-1.7B-GGUF", "Qwen3-ASR-1.7B-Q8_0.gguf"),
            extract: false,
            required: &["Qwen3-ASR-1.7B-Q8_0.gguf"],
        },
        RemoteFile {
            name: "mmproj-Qwen3-ASR-1.7B-Q8_0.gguf",
            sha256: ASR_17_PROJ_SHA,
            size: ASR_17_PROJ_SIZE,
            urls: hf("ggml-org/Qwen3-ASR-1.7B-GGUF", "mmproj-Qwen3-ASR-1.7B-Q8_0.gguf"),
            extract: false,
            required: &["mmproj-Qwen3-ASR-1.7B-Q8_0.gguf"],
        },
    ])
}

// Lazy statics via OnceLock so we don't rebuild on every call.
use std::sync::OnceLock;

fn once(
    cell: &OnceLock<&'static [RemoteFile]>,
    build: impl FnOnce() -> &'static [RemoteFile],
) -> &'static [RemoteFile] {
    *cell.get_or_init(build)
}

fn asr_17_files() -> &'static [RemoteFile] {
    once(&ASR_17_CELL, asr_17)
}

static ASR_17_CELL: OnceLock<&'static [RemoteFile]> = OnceLock::new();

fn asr_06_files() -> &'static [RemoteFile] {
    once(&ASR_06_CELL, || {
        file_set(vec![RemoteFile {
            name: "sherpa-onnx-qwen3-asr-0.6B-int8-2026-03-25.tar.bz2",
            sha256: ASR_06_SHA,
            size: ASR_06_SIZE,
            urls: github(
                "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-qwen3-asr-0.6B-int8-2026-03-25.tar.bz2",
            ),
            extract: true,
            required: &["conv_frontend.onnx", "encoder.int8.onnx", "decoder.int8.onnx", "tokenizer"],
        }])
    })
}
static ASR_06_CELL: OnceLock<&'static [RemoteFile]> = OnceLock::new();

fn sensevoice_files() -> &'static [RemoteFile] {
    once(&SENSE_CELL, || {
        file_set(vec![
            RemoteFile {
                name: "model.int8.onnx",
                sha256: SENSE_MODEL_SHA,
                size: SENSE_MODEL_SIZE,
                urls: hosted(SENSE_REPO, "model.int8.onnx"),
                extract: false,
                required: &["model.int8.onnx"],
            },
            RemoteFile {
                name: "tokens.txt",
                sha256: SENSE_TOKENS_SHA,
                size: SENSE_TOKENS_SIZE,
                urls: hosted(SENSE_REPO, "tokens.txt"),
                extract: false,
                required: &["tokens.txt"],
            },
        ])
    })
}
static SENSE_CELL: OnceLock<&'static [RemoteFile]> = OnceLock::new();

fn llm_4b_files() -> &'static [RemoteFile] {
    once(&LLM_4B_CELL, || {
        file_set(vec![RemoteFile {
            name: "Qwen3-4B-Q4_K_M.gguf",
            sha256: LLM_4B_SHA,
            size: LLM_4B_SIZE,
            urls: hf("Qwen/Qwen3-4B-GGUF", "Qwen3-4B-Q4_K_M.gguf"),
            extract: false,
            required: &["Qwen3-4B-Q4_K_M.gguf"],
        }])
    })
}
static LLM_4B_CELL: OnceLock<&'static [RemoteFile]> = OnceLock::new();

fn llm_17_files() -> &'static [RemoteFile] {
    once(&LLM_17_CELL, || {
        file_set(vec![RemoteFile {
            name: "Qwen3-1.7B-Q4_K_M.gguf",
            sha256: LLM_17_SHA,
            size: LLM_17_SIZE,
            urls: hf("unsloth/Qwen3-1.7B-GGUF", "Qwen3-1.7B-Q4_K_M.gguf"),
            extract: false,
            required: &["Qwen3-1.7B-Q4_K_M.gguf"],
        }])
    })
}
static LLM_17_CELL: OnceLock<&'static [RemoteFile]> = OnceLock::new();

fn silero_files() -> &'static [RemoteFile] {
    once(&SILERO_CELL, || {
        file_set(vec![RemoteFile {
            name: "silero_vad.onnx",
            sha256: SILERO_SHA,
            size: SILERO_SIZE,
            urls: github(
                "https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/silero_vad.onnx",
            ),
            extract: false,
            required: &["silero_vad.onnx"],
        }])
    })
}
static SILERO_CELL: OnceLock<&'static [RemoteFile]> = OnceLock::new();

fn llama_mac_files() -> &'static [RemoteFile] {
    once(&LLAMA_MAC_CELL, || {
        file_set(vec![RemoteFile {
            name: "llama-b12292-bin-macos-arm64.tar.gz",
            sha256: LLAMA_MAC_SHA,
            size: LLAMA_MAC_SIZE,
            urls: github(
                "https://github.com/ggml-org/llama.cpp/releases/download/b12292/llama-b12292-bin-macos-arm64.tar.gz",
            ),
            extract: true,
            required: &["llama-server", "libllama-server-impl.dylib"],
        }])
    })
}
static LLAMA_MAC_CELL: OnceLock<&'static [RemoteFile]> = OnceLock::new();

fn llama_win_files() -> &'static [RemoteFile] {
    once(&LLAMA_WIN_CELL, || {
        file_set(vec![RemoteFile {
            name: "llama-b12292-bin-win-cpu-x64.zip",
            sha256: LLAMA_WIN_SHA,
            size: LLAMA_WIN_SIZE,
            urls: github(
                "https://github.com/ggml-org/llama.cpp/releases/download/b12292/llama-b12292-bin-win-cpu-x64.zip",
            ),
            extract: true,
            required: &["llama-server.exe", "llama-server-impl.dll"],
        }])
    })
}
static LLAMA_WIN_CELL: OnceLock<&'static [RemoteFile]> = OnceLock::new();

#[cfg(test)]
mod tests {
    use super::*;

    fn hw(os: OsKind, gib: u64) -> Hardware {
        Hardware {
            os,
            ram_bytes: gib * (1 << 30),
        }
    }

    #[test]
    fn ram_and_os_pick_the_owner_defaults() {
        let mac_big = recommend(hw(OsKind::Mac, 24));
        assert_eq!(mac_big.asr_id, "qwen3-asr-1.7b-q8");
        assert_eq!(mac_big.llm_id, "qwen3-4b-q4");
        let mac_16 = recommend(hw(OsKind::Mac, 16));
        assert_eq!(mac_16.asr_id, "qwen3-asr-1.7b-q8");
        let mac_small = recommend(hw(OsKind::Mac, 8));
        assert_eq!(mac_small.asr_id, "qwen3-asr-0.6b-int8");
        assert_eq!(mac_small.llm_id, "qwen3-1.7b-q4");
        let just_under = recommend(Hardware {
            os: OsKind::Mac,
            ram_bytes: HIGH_RAM_MIN_BYTES - 1,
        });
        assert_eq!(just_under.asr_id, "qwen3-asr-0.6b-int8");
        let windows_big = recommend(hw(OsKind::Windows, 32));
        assert_eq!(windows_big.asr_id, "qwen3-asr-0.6b-int8");
        assert_eq!(windows_big.llm_id, "qwen3-1.7b-q4");
        let windows_small = recommend(hw(OsKind::Windows, 8));
        assert_eq!(windows_small.llm_id, "qwen3-1.7b-q4");
        assert!(card("qwen3-asr-1.7b-q8").unwrap().platforms == MAC_ONLY);
    }

    #[test]
    fn pinned_digests_are_the_ones_verified_for_this_milestone() {
        let asr = files_for("qwen3-asr-1.7b-q8", OsKind::Mac).unwrap();
        assert_eq!(asr[0].sha256, ASR_17_SHA);
        assert_eq!(asr[0].size, ASR_17_SIZE);
        assert_eq!(asr[1].sha256, ASR_17_PROJ_SHA);
        assert_eq!(files_for("qwen3-4b-q4", OsKind::Windows).unwrap()[0].sha256, LLM_4B_SHA);
        assert_eq!(files_for("qwen3-1.7b-q4", OsKind::Mac).unwrap()[0].sha256, LLM_17_SHA);
        assert_eq!(files_for("silero-vad", OsKind::Mac).unwrap()[0].sha256, SILERO_SHA);
        assert_eq!(files_for("qwen3-asr-0.6b-int8", OsKind::Windows).unwrap()[0].sha256, ASR_06_SHA);
        assert_eq!(files_for("llama-cpp-b12292", OsKind::Mac).unwrap()[0].sha256, LLAMA_MAC_SHA);
        assert_eq!(files_for("llama-cpp-b12292", OsKind::Windows).unwrap()[0].sha256, LLAMA_WIN_SHA);
        assert!(files_for("llama-cpp-b12292", OsKind::Other).is_err());
    }

    #[test]
    fn mirror_selection_stays_on_the_source_the_user_picked() {
        let file = &files_for("qwen3-4b-q4", OsKind::Mac).unwrap()[0];
        let auto = urls_for(file, Mirror::Auto).unwrap();
        assert!(auto[0].contains("huggingface.co"));
        assert!(auto.iter().any(|url| url.contains("hf-mirror.com")));
        assert!(auto.iter().any(|url| url.contains("modelscope.cn")));
        let scoped = urls_for(file, Mirror::ModelScope).unwrap();
        assert_eq!(scoped.len(), 1);
        assert!(scoped[0].contains("modelscope.cn"));
        let sherpa = &files_for("qwen3-asr-0.6b-int8", OsKind::Windows).unwrap()[0];
        let err = urls_for(sherpa, Mirror::ModelScope).unwrap_err();
        assert!(err.contains("ModelScope"), "{err}");
        assert!(err.contains("GitHub"), "{err}");
        let github = urls_for(sherpa, Mirror::GitHub).unwrap();
        assert!(github[0].contains("github.com"));
    }

    #[test]
    fn sensevoice_is_optional_and_is_not_the_local_default() {
        for hardware in [hw(OsKind::Mac, 24), hw(OsKind::Mac, 8), hw(OsKind::Windows, 32), hw(OsKind::Windows, 8)] {
            assert_ne!(recommend(hardware).asr_id, SENSEVOICE_ID);
            assert_eq!(active_id(Role::Asr, SENSEVOICE_ID, hardware), SENSEVOICE_ID);
        }
        assert_eq!(active_id(Role::Asr, "", hw(OsKind::Mac, 24)), "qwen3-asr-1.7b-q8");
        assert_eq!(local_asr_backend(SENSEVOICE_ID), Some(LocalAsrBackend::SenseVoice));
        assert_eq!(local_asr_backend("qwen3-asr-0.6b-int8"), Some(LocalAsrBackend::QwenSherpa));
        assert_eq!(local_asr_backend("qwen3-asr-1.7b-q8"), Some(LocalAsrBackend::QwenLlama));
        assert!(local_asr_backend("qwen3-4b-q4").is_none());
        let files = files_for(SENSEVOICE_ID, OsKind::Windows).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].sha256, SENSE_MODEL_SHA);
        assert_eq!(files[0].size, SENSE_MODEL_SIZE);
        assert_eq!(files[1].sha256, SENSE_TOKENS_SHA);
        assert_eq!(files[1].size, SENSE_TOKENS_SIZE);
        let auto = urls_for(&files[0], Mirror::Auto).unwrap();
        assert!(auto[0].contains("huggingface.co/csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17/resolve/main/model.int8.onnx"));
        assert!(auto.iter().any(|url| url.contains("hf-mirror.com")));
        assert!(auto.iter().all(|url| !url.contains("modelscope.cn")));
        let err = urls_for(&files[0], Mirror::ModelScope).unwrap_err();
        assert!(err.contains("ModelScope"), "{err}");
        assert!(err.contains("Hugging Face"), "{err}");
        assert!(!attribution(false).contains("FunASR"));
        let installed = attribution(true);
        assert!(installed.contains("FunASR"));
        assert!(installed.contains("FunAudioLLM"));
        assert!(card(SENSEVOICE_ID).unwrap().license.contains("FunASR"));
    }

    #[test]
    fn an_explicit_selection_overrides_the_recommendation_on_that_os() {
        let hardware = hw(OsKind::Mac, 24);
        assert_eq!(active_id(Role::Asr, "", hardware), "qwen3-asr-1.7b-q8");
        assert_eq!(
            active_id(Role::Asr, "qwen3-asr-0.6b-int8", hardware),
            "qwen3-asr-0.6b-int8"
        );
        let windows = hw(OsKind::Windows, 32);
        assert_eq!(active_id(Role::Asr, "qwen3-asr-1.7b-q8", windows), "qwen3-asr-0.6b-int8");
    }
}
