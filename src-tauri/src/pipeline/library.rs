//! On-disk model library. Opening this state reads files only. It does not
//! load weights and it does not contact a network.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

use super::catalog::{self, Card, Hardware, Mirror, RemoteFile, Role};
use super::hotkey::OsKind;
use super::download::{self, FetchProgress};
use super::extract;

const MARKER: &str = "installed.json";
const ERROR_NAME: &str = "error.txt";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModelState {
    Missing,
    Partial { bytes: u64 },
    Incomplete,
    Failed { message: String },
    Installed { bytes: u64 },
}

impl ModelState {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::Partial { .. } => "partial",
            Self::Incomplete => "incomplete",
            Self::Failed { .. } => "failed",
            Self::Installed { .. } => "installed",
        }
    }

    pub fn bytes(&self) -> u64 {
        match self {
            Self::Partial { bytes } | Self::Installed { bytes } => *bytes,
            _ => 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct MarkerFile {
    name: String,
    bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
struct MarkerDoc {
    id: String,
    source_sha256: String,
    files: Vec<MarkerFile>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Readiness {
    pub asr: bool,
    pub llm: bool,
}

impl Readiness {
    pub fn fallback_ready(&self) -> bool {
        self.asr && self.llm
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelRow {
    pub id: String,
    pub name: String,
    pub role: String,
    pub license: String,
    pub languages: String,
    pub runtime: String,
    pub memory: String,
    pub notes: String,
    pub size_bytes: u64,
    pub state: String,
    pub received_bytes: u64,
    pub message: String,
    pub recommended: bool,
    pub active: bool,
    pub available: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibraryView {
    pub mirror: String,
    pub disk_bytes: u64,
    pub asr_ready: bool,
    pub llm_ready: bool,
    pub fallback_ready: bool,
    pub reminder: String,
    pub skipped: bool,
    pub hardware_summary: String,
    pub recommended_asr: String,
    pub recommended_llm: String,
    pub active_asr: String,
    pub active_llm: String,
    pub downloading: String,
    pub attribution: String,
    /// Shown before a skip is recorded. Both local speech and local rewrite are required.
    pub skip_warning: String,
    pub models: Vec<ModelRow>,
}

#[derive(Debug, Clone)]
pub struct InstallEvent {
    pub phase: String,
    pub received: u64,
    pub total: u64,
    pub message: String,
}

pub fn model_dir(root: &Path, id: &str) -> PathBuf {
    root.join(id)
}

pub fn source_sha(files: &[RemoteFile]) -> String {
    files.iter().map(|file| file.sha256).collect::<Vec<_>>().join("+")
}

pub fn view(
    root: &Path,
    hardware: Hardware,
    mirror: &str,
    skipped: bool,
    selected_asr: &str,
    selected_llm: &str,
    downloading: &str,
) -> LibraryView {
    let recommendation = catalog::recommend(hardware);
    let active_asr = catalog::active_id(Role::Asr, selected_asr, hardware);
    let active_llm = catalog::active_id(Role::Llm, selected_llm, hardware);
    let ready = readiness(root, hardware, selected_asr, selected_llm);
    let sensevoice_installed = catalog::card(catalog::SENSEVOICE_ID)
        .is_some_and(|item| installed(root, item, hardware.os));
    let mut models = Vec::new();
    for card in catalog::cards() {
        if !catalog::listed_on(card, hardware.os) {
            continue;
        }
        let available = catalog::runnable_on(card, hardware.os);
        let (state, message, size) = describe(root, card, hardware.os);
        let active = card.id == active_asr || card.id == active_llm;
        let recommended = card.id == recommendation.asr_id || card.id == recommendation.llm_id;
        models.push(ModelRow {
            id: card.id.to_string(),
            name: card.name.to_string(),
            role: card.role.as_str().to_string(),
            license: card.license.to_string(),
            languages: card.languages.to_string(),
            runtime: card.runtime.to_string(),
            memory: card.memory.to_string(),
            notes: card.notes.to_string(),
            size_bytes: size,
            state: state.as_str().to_string(),
            received_bytes: state.bytes(),
            message,
            recommended,
            active,
            available,
        });
    }
    LibraryView {
        mirror: mirror.to_string(),
        disk_bytes: disk_usage(root),
        asr_ready: ready.asr,
        llm_ready: ready.llm,
        fallback_ready: ready.fallback_ready(),
        reminder: reminder(ready.asr, ready.llm, skipped),
        skipped,
        hardware_summary: recommendation.summary,
        recommended_asr: recommendation.asr_id,
        recommended_llm: recommendation.llm_id,
        active_asr,
        active_llm,
        downloading: downloading.to_string(),
        attribution: catalog::attribution(sensevoice_installed),
        skip_warning: skip_warning().to_string(),
        models,
    }
}

pub fn skip_warning() -> &'static str {
    "Skip the local download? Offline fallback needs both the local speech model and the local rewrite model. 断网后无法自动改用本地模型，直到这两个都安装并校验完成。 This reminder stays until both are installed."
}

pub fn fallback_menu(asr_ready: bool, llm_ready: bool) -> &'static str {
    if asr_ready && llm_ready {
        "Local fallback: ready"
    } else if !llm_ready {
        "Local fallback: rewrite model missing"
    } else {
        "Local fallback: not ready"
    }
}

pub fn reminder(asr_ready: bool, llm_ready: bool, skipped: bool) -> String {
    if asr_ready && llm_ready {
        return String::new();
    }
    let mut text = String::from(
        "Offline fallback is not ready. Download both the local speech model and the local rewrite model. 断网后无法自动改用本地模型，直到这两个都安装并校验完成。",
    );
    if skipped {
        text.push_str(" You skipped the download. This reminder stays until both are installed.");
    }
    if !asr_ready && !llm_ready {
        text.push_str(" Neither model is installed.");
    } else if !asr_ready {
        text.push_str(" The speech model is still missing.");
    } else {
        text.push_str(" The rewrite model is still missing.");
    }
    text
}

pub fn readiness(root: &Path, hardware: Hardware, selected_asr: &str, selected_llm: &str) -> Readiness {
    let asr = catalog::active_id(Role::Asr, selected_asr, hardware);
    let llm = catalog::active_id(Role::Llm, selected_llm, hardware);
    Readiness {
        asr: engine_ready(root, &asr, hardware.os),
        llm: engine_ready(root, &llm, hardware.os),
    }
}

pub fn engine_ready(root: &Path, id: &str, os: OsKind) -> bool {
    let Some(card) = catalog::card(id) else {
        return false;
    };
    if !installed(root, card, os) {
        return false;
    }
    card.depends_on.iter().all(|dep| engine_ready(root, dep, os))
}

pub fn installed(root: &Path, card: &Card, os: OsKind) -> bool {
    let Ok(files) = catalog::files_for(card.id, os) else {
        return false;
    };
    matches!(state_of(&model_dir(root, card.id), files), ModelState::Installed { .. })
}

pub fn state_of(dir: &Path, files: &[RemoteFile]) -> ModelState {
    let expected = source_sha(files);
    if let Some(marker) = read_marker(dir) {
        if marker.source_sha256 == expected && sizes_match(dir, &marker.files) && required_present(dir, files) {
            let bytes = marker.files.iter().map(|file| file.bytes).sum();
            return ModelState::Installed { bytes };
        }
    }
    let partial = partial_bytes(dir);
    if partial > 0 {
        return ModelState::Partial { bytes: partial };
    }
    if let Some(message) = read_error(dir) {
        return ModelState::Failed { message };
    }
    if dir_has_payload(dir) {
        ModelState::Incomplete
    } else {
        ModelState::Missing
    }
}

pub fn install(
    root: &Path,
    id: &str,
    os: OsKind,
    preference: Mirror,
    cancel: &AtomicBool,
    mut progress: impl FnMut(InstallEvent),
) -> Result<(), String> {
    // The dependency walk must not be generic over the callback. Each `&mut`
    // layer would be a new type and the compiler recurses until it gives up.
    install_with(root, id, os, preference, cancel, &mut progress)
}

fn install_with(
    root: &Path,
    id: &str,
    os: OsKind,
    preference: Mirror,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(InstallEvent),
) -> Result<(), String> {
    let card = catalog::card(id).ok_or_else(|| format!("Unknown model \"{id}\"."))?;
    if os != OsKind::Other && !catalog::runnable_on(card, os) {
        return Err(format!("{} is not available on this operating system.", card.name));
    }
    for dep in card.depends_on {
        if !engine_ready(root, dep, os) {
            install_with(root, dep, os, preference, cancel, progress)?;
        }
    }
    let files = catalog::files_for(id, os)?;
    let dir = model_dir(root, id);
    fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    for file in files {
        if cancel.load(Ordering::Relaxed) {
            return Err("Download cancelled.".into());
        }
        let urls = catalog::urls_for(file, preference)?;
        let dest = dir.join(file.name);
        let label = format!("{} ({})", card.name, file.name);
        progress(InstallEvent {
            phase: "downloading".into(),
            received: 0,
            total: file.size,
            message: format!("Downloading {label}"),
        });
        let file_size = file.size;
        download::download_verified(&urls, &dest, file.sha256, file.size, cancel, |fetch: FetchProgress| {
            progress(InstallEvent {
                phase: "downloading".into(),
                received: fetch.received,
                total: file_size.max(fetch.total),
                message: format!("Downloading {label}"),
            });
        })?;
        if file.extract {
            progress(InstallEvent {
                phase: "extracting".into(),
                received: file.size,
                total: file.size,
                message: format!("Unpacking {label}"),
            });
            extract::extract_archive(&dest, &dir)?;
            let _ = fs::remove_file(&dest);
            for required in file.required {
                if extract::find_named(&dir, required).is_none() {
                    let message = format!("The archive did not contain {required}.");
                    let _ = write_error(&dir, &message);
                    return Err(message);
                }
            }
        }
    }
    if id == "llama-cpp-b12292" {
        relax_runtime(&dir);
    }
    let records = inventory(&dir);
    write_marker(&dir, id, &source_sha(files), &records)?;
    let _ = fs::remove_file(dir.join(ERROR_NAME));
    progress(InstallEvent {
        phase: "installed".into(),
        received: records.iter().map(|file| file.bytes).sum(),
        total: files.iter().map(|file| file.size).sum(),
        message: format!("{} is installed.", card.name),
    });
    Ok(())
}

pub fn delete_model(root: &Path, id: &str) -> Result<(), String> {
    if catalog::card(id).is_none() {
        return Err(format!("Unknown model \"{id}\"."));
    }
    let dir = model_dir(root, id);
    if dir.exists() {
        fs::remove_dir_all(&dir).map_err(|err| format!("Could not delete {}: {err}", dir.display()))?;
    }
    Ok(())
}

pub fn disk_usage(root: &Path) -> u64 {
    let mut total = 0u64;
    walk(root, &mut |path| {
        if let Ok(meta) = path.metadata() {
            total = total.saturating_add(meta.len());
        }
    });
    total
}

pub fn write_error(dir: &Path, message: &str) -> Result<(), String> {
    let _ = fs::create_dir_all(dir);
    let mut text = message.trim().to_string();
    if text.len() > 500 {
        text.truncate(500);
    }
    fs::write(dir.join(ERROR_NAME), text).map_err(|err| err.to_string())
}

fn describe(root: &Path, card: &Card, os: OsKind) -> (ModelState, String, u64) {
    let Ok(files) = catalog::files_for(card.id, os) else {
        return (
            ModelState::Missing,
            "Not available on this operating system yet.".into(),
            0,
        );
    };
    let size = files.iter().map(|file| file.size).sum();
    let state = state_of(&model_dir(root, card.id), files);
    let message = match &state {
        ModelState::Installed { .. } => "Installed and checksum verified.".into(),
        ModelState::Partial { .. } => "Download paused. It will resume.".into(),
        ModelState::Failed { message } => message.clone(),
        ModelState::Incomplete => "Files are incomplete. Download again to verify them.".into(),
        ModelState::Missing => String::new(),
    };
    (state, message, size)
}

fn write_marker(dir: &Path, id: &str, source: &str, files: &[MarkerFile]) -> Result<(), String> {
    let doc = MarkerDoc {
        id: id.to_string(),
        source_sha256: source.to_string(),
        files: files.to_vec(),
    };
    let body = serde_json::to_vec_pretty(&doc).map_err(|err| err.to_string())?;
    let path = dir.join(MARKER);
    let mut file = File::create(&path).map_err(|err| err.to_string())?;
    file.write_all(&body).map_err(|err| err.to_string())?;
    Ok(())
}

fn read_marker(dir: &Path) -> Option<MarkerDoc> {
    let bytes = fs::read(dir.join(MARKER)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn read_error(dir: &Path) -> Option<String> {
    let text = fs::read_to_string(dir.join(ERROR_NAME)).ok()?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn sizes_match(dir: &Path, files: &[MarkerFile]) -> bool {
    !files.is_empty()
        && files.iter().all(|file| {
            let path = dir.join(&file.name);
            path.is_file() && path.metadata().map(|meta| meta.len() == file.bytes).unwrap_or(false)
        })
}

fn required_present(dir: &Path, files: &[RemoteFile]) -> bool {
    files.iter().all(|file| {
        if file.extract {
            file.required.iter().all(|name| extract::find_named(dir, name).is_some())
        } else {
            let path = dir.join(file.name);
            path.is_file() && path.metadata().map(|meta| meta.len() == file.size).unwrap_or(false)
        }
    })
}

fn inventory(dir: &Path) -> Vec<MarkerFile> {
    let mut files = Vec::new();
    walk(dir, &mut |path| {
        let name = path.file_name().and_then(|value| value.to_str()).unwrap_or("");
        if name == MARKER || name == ERROR_NAME || name.ends_with(".partial") {
            return;
        }
        let Ok(relative) = path.strip_prefix(dir) else {
            return;
        };
        let relative = relative.to_string_lossy().replace('\\', "/");
        let bytes = path.metadata().map(|meta| meta.len()).unwrap_or(0);
        files.push(MarkerFile { name: relative, bytes });
    });
    files.sort_by(|left, right| left.name.cmp(&right.name));
    files
}

fn partial_bytes(dir: &Path) -> u64 {
    let mut total = 0u64;
    walk(dir, &mut |path| {
        let name = path.file_name().and_then(|value| value.to_str()).unwrap_or("");
        if name.ends_with(".partial") {
            total = total.saturating_add(path.metadata().map(|meta| meta.len()).unwrap_or(0));
        }
    });
    total
}

fn dir_has_payload(dir: &Path) -> bool {
    let mut found = false;
    walk(dir, &mut |path| {
        let name = path.file_name().and_then(|value| value.to_str()).unwrap_or("");
        if name != MARKER && name != ERROR_NAME {
            found = true;
        }
    });
    found
}

fn walk(root: &Path, visit: &mut dyn FnMut(&Path)) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_symlink() {
            continue;
        }
        let path = entry.path();
        if kind.is_dir() {
            walk(&path, visit);
        } else if kind.is_file() {
            visit(&path);
        }
    }
}

fn relax_runtime(dir: &Path) {
    let Some(binary) = extract::find_named(dir, "llama-server")
        .or_else(|| extract::find_named(dir, "llama-server.exe"))
    else {
        return;
    };
    make_executable(&binary);
    // macOS: one identity for llama-server and every sibling Mach-O.
    // Windows: the executable bit is a no-op and there is nothing to sign.
    super::codesign::sign_runtime_tree(dir);
}

fn make_executable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = path.metadata() {
            let mut permissions = meta.permissions();
            permissions.set_mode(permissions.mode() | 0o755);
            let _ = fs::set_permissions(path, permissions);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("newtype-lib-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn tiny() -> RemoteFile {
        RemoteFile {
            name: "tiny.bin",
            sha256: "abc",
            size: 4,
            urls: &[],
            extract: false,
            required: &["tiny.bin"],
        }
    }

    #[test]
    fn marker_is_installed_only_when_the_source_sha_and_sizes_match() {
        let dir = scratch("state");
        let model = dir.join("demo");
        fs::create_dir_all(&model).unwrap();
        let files = [tiny()];
        assert!(matches!(state_of(&model, &files), ModelState::Missing));
        fs::write(model.join("tiny.bin"), b"test").unwrap();
        assert!(matches!(state_of(&model, &files), ModelState::Incomplete));
        write_marker(&model, "demo", "wrong", &inventory(&model)).unwrap();
        assert!(matches!(state_of(&model, &files), ModelState::Incomplete));
        write_marker(&model, "demo", &source_sha(&files), &inventory(&model)).unwrap();
        assert!(matches!(state_of(&model, &files), ModelState::Installed { bytes: 4 }));
        fs::write(model.join("tiny.bin"), b"te").unwrap();
        assert!(!matches!(state_of(&model, &files), ModelState::Installed { .. }));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_failed_checksum_record_is_not_installed_and_a_partial_can_resume() {
        let dir = scratch("fail");
        let model = dir.join("demo");
        fs::create_dir_all(&model).unwrap();
        let files = [tiny()];
        write_error(&model, "SHA-256 did not match").unwrap();
        match state_of(&model, &files) {
            ModelState::Failed { message } => assert!(message.contains("SHA-256")),
            other => panic!("expected failure, got {other:?}"),
        }
        fs::write(model.join("tiny.bin.partial"), b"te").unwrap();
        assert!(matches!(state_of(&model, &files), ModelState::Partial { bytes: 2 }));
        let kept = scratch("delete");
        let silero = model_dir(&kept, "silero-vad");
        fs::create_dir_all(&silero).unwrap();
        fs::write(silero.join("silero_vad.onnx"), b"x").unwrap();
        delete_model(&kept, "silero-vad").unwrap();
        assert!(!silero.exists());
        assert!(delete_model(&kept, "missing-model").is_err());
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&kept);
    }

    #[test]
    fn an_empty_library_is_not_a_ready_fallback() {
        let dir = scratch("empty");
        let hardware = Hardware {
            os: OsKind::Mac,
            ram_bytes: 24 << 30,
        };
        let ready = readiness(&dir, hardware, "", "");
        assert!(!ready.asr);
        assert!(!ready.llm);
        assert!(!ready.fallback_ready());
        let shown = view(&dir, hardware, "auto", true, "", "", "");
        assert!(!shown.fallback_ready);
        assert!(shown.reminder.contains("skipped") || shown.reminder.contains("Skipped") || shown.reminder.contains("skip"));
        assert!(shown.reminder.contains("断网"));
        assert!(shown.skip_warning.contains("断网"));
        assert!(shown.skip_warning.contains("speech") && shown.skip_warning.contains("rewrite"));
        assert_eq!(shown.recommended_asr, "qwen3-asr-1.7b-q8");
        assert!(shown.disk_bytes == 0);
        assert!(shown.models.iter().any(|model| model.id == "qwen3-asr-0.6b-int8" && model.state == "missing"));
        let sense = shown.models.iter().find(|model| model.id == catalog::SENSEVOICE_ID).expect("sensevoice card");
        assert!(!sense.recommended);
        assert_eq!(sense.state, "missing");
        assert!(!shown.attribution.contains("FunASR"));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn reminder_clears_only_when_both_roles_are_ready() {
        assert!(!reminder(true, false, false).is_empty());
        assert!(!reminder(false, true, true).is_empty());
        assert!(reminder(true, true, true).is_empty());
        let warning = skip_warning();
        assert!(warning.contains("speech"));
        assert!(warning.contains("rewrite"));
        assert!(warning.contains("断网"));
        assert_eq!(fallback_menu(true, true), "Local fallback: ready");
        assert_eq!(fallback_menu(true, false), "Local fallback: rewrite model missing");
        assert_eq!(fallback_menu(false, false), "Local fallback: rewrite model missing");
        assert_eq!(fallback_menu(false, true), "Local fallback: not ready");
    }
}
