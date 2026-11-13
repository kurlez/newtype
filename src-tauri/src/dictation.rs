//! Dictation session: hotkey, microphone, cloud speech, polish, paste, history.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use std::sync::mpsc::{self, Receiver, Sender};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::build_info::{commit, full_version};
use crate::dictate_config::{self, DictationSettings};
use crate::permissions::{self, PermissionSnapshot};
use crate::pipeline::asr::AsrProvider;
use crate::pipeline::audio::{self, AudioClip, MicSession};
use crate::pipeline::catalog::{self, Mirror};
use crate::pipeline::cloud::{self, AsrWire, CloudAsr, Preset, Thinking};
use crate::pipeline::egress::{self, NetMode};
use crate::pipeline::engines::Engines;
use crate::pipeline::failover::{self, ProviderError};
use crate::pipeline::focus::{self, InsertOutcome};
use crate::pipeline::front;
use crate::pipeline::history::{self, HistoryEntry};
use crate::pipeline::hotkey::{
    self, EngineEvent, HotkeyEngine, HotkeyMode, KeyEdge, TalkController, TalkEffect, TalkMode,
};
use crate::pipeline::insert;
use crate::pipeline::library::{self, LibraryView};
use crate::pipeline::local_run;
use crate::pipeline::polish::{FinishedText, LlmComplete, LlmRequest, PolishLevel};
use crate::pipeline::probe;
use crate::pipeline::rewrite::{self, Audience};
use crate::pipeline::router::{
    self, Block, PathLabel, Reachability, RouteDecision, RouteInput, RoutePreference, Slot,
};
use crate::pipeline::selection;
use crate::pipeline::session::{self, Outcome, Via};
use crate::pipeline::task::{self, Job};
use crate::pipeline::volcengine::{self, VolcAuth};
use crate::secrets;

enum Control {
    Engine(EngineEvent),
    Toggle,
    Cancel,
    AutoStop,
}

struct HotkeyShare {
    generation: u64,
    bindings: Vec<(HotkeyMode, hotkey::Chord)>,
    capture: bool,
    capture_mode: String,
}

pub struct Dictation {
    settings_path: std::path::PathBuf,
    history_path: std::path::PathBuf,
    dev_secrets: std::path::PathBuf,
    engines: Arc<Engines>,
    settings: Mutex<DictationSettings>,
    history: Mutex<Vec<HistoryEntry>>,
    hotkeys: Arc<Mutex<HotkeyShare>>,
    listener: Arc<Mutex<String>>,
    secure_store: Mutex<String>,
    log_path: String,
    mic_status: Arc<Mutex<String>>,
    phase: Mutex<String>,
    session_label: Mutex<String>,
    reachability: Mutex<Reachability>,
    notice: Mutex<String>,
    probe_wake: Arc<AtomicBool>,
    probe_epoch: AtomicU64,
    locked: Mutex<Option<LockedSession>>,
    control: Sender<Control>,
    inbox: Mutex<Option<Receiver<Control>>>,
    cancel: Arc<AtomicBool>,
    generation: Arc<AtomicU64>,
    focus_target: Mutex<Option<focus::FocusTarget>>,
    /// Groq Retry-After, in unix millis. Zero means try Groq on the next request.
    groq_cooldown_until_ms: Arc<AtomicU64>,
}

struct LockedSession {
    decision: RouteDecision,
    preference: RoutePreference,
    job: Job,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RouteEvent {
    route_label: String,
    route_detail: String,
    route_notice: String,
    connectivity: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct PhaseEvent {
    phase: String,
    route: String,
    message: String,
    elapsed_ms: u64,
    level: f32,
    text: String,
    mode: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CaptureEvent {
    mode: String,
    hotkey: String,
    label: String,
    /// `pending` shows the chord and waits for Set. `idle` means capture was cancelled.
    phase: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PresetDto {
    id: String,
    label: String,
    asr_ready: bool,
    llm_ready: bool,
    base_editable: bool,
    default_base: String,
    default_asr: String,
    default_llm: String,
    asr_models: Vec<String>,
    llm_models: Vec<String>,
    asr_wire: String,
    note: String,
    verified: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleDto {
    id: String,
    label: String,
    asr_provider: String,
    llm_provider: String,
    asr_model: String,
    llm_model: String,
    monthly_usd: String,
    note: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub settings: DictationSettings,
    pub hotkey_labels: Vec<String>,
    pub hotkey_chords: Vec<String>,
    pub warnings: Vec<String>,
    pub verified_on: String,
    pub keys: std::collections::BTreeMap<String, bool>,
    pub presets: Vec<PresetDto>,
    pub bundles: Vec<BundleDto>,
    pub permissions: PermissionSnapshot,
    pub listener: String,
    pub secure_store: String,
    pub log_path: String,
    pub route_label: String,
    pub route_detail: String,
    pub route_notice: String,
    pub connectivity: String,
    pub phase: String,
    pub version: String,
    pub dev_key_store: bool,
    pub local: LibraryView,
    pub language: String,
}

impl Dictation {
    pub fn load(app: &AppHandle) -> Result<Self, String> {
        let config_dir = app.path().app_config_dir().map_err(|err| err.to_string())?;
        let data_dir = app.path().app_data_dir().map_err(|err| err.to_string())?;
        std::fs::create_dir_all(&config_dir).map_err(|err| err.to_string())?;
        std::fs::create_dir_all(&data_dir).map_err(|err| err.to_string())?;
        let log_dir = app
            .path()
            .app_log_dir()
            .unwrap_or_else(|_| config_dir.clone());
        crate::diag::init(&log_dir);
        let secure_store = match secrets::probe(&config_dir.join("dev-secrets.json")) {
            Ok(()) => {
                crate::diag::record(&format!(
                    "credential store ready ({})",
                    secrets::store_name()
                ));
                "ready".to_string()
            }
            Err(err) => {
                crate::diag::record(&format!("credential store unavailable: {err}"));
                err
            }
        };
        let settings_path = config_dir.join("dictation.json");
        let history_path = data_dir.join("history.json");
        let dev_secrets = config_dir.join("dev-secrets.json");
        let engines = Arc::new(Engines::new(data_dir.join("models")));
        let mut settings = dictate_config::load(&settings_path);
        dictate_config::normalize(&mut settings);
        crate::locale::set_preference(&settings.ui_language);
        let effective =
            dictate_config::effective(&settings, hotkey::host_os()).unwrap_or_else(|_| {
                dictate_config::effective(&DictationSettings::default(), hotkey::host_os())
                    .expect("defaults")
            });
        let (control, inbox) = mpsc::channel();
        let mut history = history::load(&history_path);
        let removed = history::prune(
            &mut history,
            history::Retention::parse(&settings.history_retention),
            now_ms(),
        );
        for id in &removed {
            let _ = std::fs::remove_file(audio_file(&history_path, id));
        }
        if !removed.is_empty() {
            let _ = history::save(&history_path, &history);
        }
        Ok(Self {
            settings_path,
            history_path: history_path.clone(),
            dev_secrets,
            engines,
            settings: Mutex::new(settings),
            history: Mutex::new(history),
            hotkeys: Arc::new(Mutex::new(HotkeyShare {
                generation: 1,
                bindings: effective.bindings,
                capture: false,
                capture_mode: String::new(),
            })),
            listener: Arc::new(Mutex::new("Starting the hotkey listener.".into())),
            secure_store: Mutex::new(secure_store),
            log_path: crate::diag::path().unwrap_or_default(),
            mic_status: Arc::new(Mutex::new("unknown".into())),
            phase: Mutex::new("idle".into()),
            session_label: Mutex::new(crate::locale::text("Dictate", "口授").into()),
            reachability: Mutex::new(Reachability::Unknown),
            notice: Mutex::new(String::new()),
            probe_wake: Arc::new(AtomicBool::new(true)),
            probe_epoch: AtomicU64::new(0),
            locked: Mutex::new(None),
            control,
            inbox: Mutex::new(Some(inbox)),
            cancel: Arc::new(AtomicBool::new(false)),
            generation: Arc::new(AtomicU64::new(1)),
            focus_target: Mutex::new(None),
            groq_cooldown_until_ms: Arc::new(AtomicU64::new(0)),
        })
    }

    pub fn launch(app: AppHandle) {
        open_indicator(&app);
        show_indicator(&app);
        #[cfg(windows)]
        watch_taskbar(app.clone());
        {
            let state = app.state::<Dictation>();
            apply_route_mode(&state, true);
        }
        spawn_probe(app.clone());
        let state = app.state::<Dictation>();
        let rx = state.inbox.lock().expect("inbox").take();
        let control = state.control.clone();
        let hotkeys = Arc::clone(&state.hotkeys);
        let listener = Arc::clone(&state.listener);
        let engines = Arc::clone(&state.engines);
        drop(state);
        let idle_app = app.clone();
        std::thread::Builder::new()
            .name("newtype-idle".into())
            .spawn(move || loop {
                std::thread::sleep(Duration::from_secs(30));
                let minutes = idle_app
                    .state::<Dictation>()
                    .settings
                    .lock()
                    .expect("settings")
                    .local_idle_minutes
                    .clamp(1, 120);
                engines.reap(Duration::from_secs(u64::from(minutes) * 60));
            })
            .expect("idle thread");
        let translucent = app
            .state::<Dictation>()
            .settings
            .lock()
            .expect("settings")
            .window_translucent;
        crate::material::apply(&app, translucent);
        spawn_listener(app.clone(), control.clone(), hotkeys, listener);
        if let Some(rx) = rx {
            std::thread::Builder::new()
                .name("newtype-session".into())
                .spawn(move || session(app, rx, control))
                .expect("session thread");
        }
    }

    pub fn snapshot(&self, app: &AppHandle) -> Snapshot {
        let mut settings = self.settings.lock().expect("settings").clone();
        dictate_config::normalize(&mut settings);
        let effective = dictate_config::effective(&settings, hotkey::host_os());
        let notice = self.notice.lock().expect("notice").clone();
        let connectivity = connectivity_line(self);
        let (labels, chords, warnings, route_label, route_detail) = match &effective {
            Ok(effective) => {
                let ready = readiness_of(self);
                let decision = route_for(self, effective, ready);
                let detail = route_detail(effective.preference, decision, ready, &notice);
                (
                    effective.hotkey_labels.clone(),
                    effective
                        .bindings
                        .iter()
                        .map(|(_, chord)| hotkey::canonical(chord))
                        .collect(),
                    effective.warnings.clone(),
                    decision.label.text().to_string(),
                    detail,
                )
            }
            Err(err) => (
                Vec::new(),
                Vec::new(),
                vec![err.clone()],
                PathLabel::Cloud.text().to_string(),
                err.clone(),
            ),
        };
        let mut keys = std::collections::BTreeMap::new();
        for preset in cloud::presets() {
            keys.insert(
                preset.id.to_string(),
                secrets::has_key(preset.id, &self.dev_secrets),
            );
        }
        keys.insert(
            "volcengine-speech".into(),
            secrets::has_key("volcengine-speech", &self.dev_secrets),
        );
        keys.insert(
            "volcengine-token".into(),
            secrets::has_key("volcengine-token", &self.dev_secrets),
        );
        Snapshot {
            settings,
            hotkey_labels: labels,
            hotkey_chords: chords,
            warnings,
            verified_on: cloud::VERIFIED_ON.into(),
            keys,
            presets: cloud::presets().iter().map(preset_dto).collect(),
            bundles: cloud::bundles().iter().map(bundle_dto).collect(),
            permissions: permissions::snapshot(&self.mic_status.lock().expect("mic")),
            listener: self.listener.lock().expect("listener").clone(),
            secure_store: self.secure_store.lock().expect("store").clone(),
            log_path: self.log_path.clone(),
            route_label,
            route_detail,
            route_notice: notice,
            connectivity,
            phase: self.phase.lock().expect("phase").clone(),
            version: version_of(app),
            dev_key_store: cfg!(not(any(target_os = "macos", target_os = "windows"))),
            local: self.library_view(),
            language: crate::locale::resolved_tag().into(),
        }
    }

    pub fn route_label(&self) -> String {
        self.snapshot_route().0
    }

    fn snapshot_route(&self) -> (String, String) {
        let settings = self.settings.lock().expect("settings").clone();
        match dictate_config::effective(&settings, hotkey::host_os()) {
            Ok(effective) => {
                let ready = readiness_of(self);
                let decision = route_for(self, &effective, ready);
                let notice = self.notice.lock().expect("notice").clone();
                (
                    decision.label.text().to_string(),
                    route_detail(effective.preference, decision, ready, &notice),
                )
            }
            Err(err) => (PathLabel::Cloud.text().to_string(), err),
        }
    }
}

pub fn toggle(app: &AppHandle) {
    let _ = app.state::<Dictation>().control.send(Control::Toggle);
}

pub fn cancel(app: &AppHandle) {
    app.state::<Dictation>()
        .cancel
        .store(true, Ordering::Relaxed);
    let _ = app.state::<Dictation>().control.send(Control::Cancel);
}

pub fn route_label(app: &AppHandle) -> String {
    app.state::<Dictation>().route_label()
}

fn bundle_dto(bundle: &cloud::Bundle) -> BundleDto {
    BundleDto {
        id: bundle.id.into(),
        label: bundle.label.into(),
        asr_provider: bundle.asr_provider.into(),
        llm_provider: bundle.llm_provider.into(),
        asr_model: bundle.asr_model.into(),
        llm_model: bundle.llm_model.into(),
        monthly_usd: bundle.monthly_usd.into(),
        note: bundle.note.into(),
    }
}

fn preset_label(id: &str, fallback: &str) -> String {
    match id {
        "dashscope" => crate::locale::text("DashScope", "通义").into(),
        "volcengine" => crate::locale::text("Volcengine", "火山").into(),
        _ => fallback.to_string(),
    }
}

fn preset_dto(preset: &Preset) -> PresetDto {
    PresetDto {
        id: preset.id.into(),
        label: preset_label(preset.id, preset.label),
        asr_ready: preset.asr_ready,
        llm_ready: preset.llm_ready,
        base_editable: preset.base_editable,
        default_base: preset.default_base.into(),
        default_asr: preset.default_asr.into(),
        default_llm: preset.default_llm.into(),
        asr_models: preset
            .asr_models
            .iter()
            .map(|id| (*id).to_string())
            .collect(),
        llm_models: preset
            .llm_models
            .iter()
            .map(|id| (*id).to_string())
            .collect(),
        asr_wire: preset.asr_wire.as_str().into(),
        note: preset.note.into(),
        verified: preset.verified.into(),
    }
}

fn route_for(
    state: &Dictation,
    effective: &dictate_config::Effective,
    ready: library::Readiness,
) -> router::RouteDecision {
    let reachability = *state.reachability.lock().expect("reachability");
    router::decide(RouteInput {
        preference: effective.preference,
        reachability,
        cloud_configured: effective.asr_implemented,
        asr_implemented: effective.asr_implemented,
        local_asr_ready: ready.asr,
        local_llm_ready: ready.llm,
    })
}

fn readiness_of(state: &Dictation) -> library::Readiness {
    let settings = state.settings.lock().expect("settings").clone();
    library::readiness(
        state.engines.root(),
        catalog::host_hardware(),
        &settings.local_asr_id,
        &settings.local_llm_id,
    )
}

fn route_detail(
    preference: RoutePreference,
    decision: router::RouteDecision,
    ready: library::Readiness,
    notice: &str,
) -> String {
    let mut text = if let Some(message) = router::block_message_for(decision.block, preference) {
        message.to_string()
    } else {
        let zh = crate::locale::prefers_zh();
        match decision.label {
            PathLabel::Cloud if preference == RoutePreference::CloudOnly => {
                if zh {
                    "仅云端。失败时不会改用本地模型。".into()
                } else {
                    "Cloud only. A failure does not switch to local models.".into()
                }
            }
            PathLabel::Cloud if !ready.fallback_ready() => {
                if zh {
                    "语音发往你配置的供应商。离线回落需要两个本地模型都已安装。".into()
                } else {
                    "Speech goes to the provider you configured. Offline fallback needs both local models installed.".into()
                }
            }
            PathLabel::Cloud => {
                if zh {
                    "语音发往你配置的供应商。如果一直连不上，下一次口授改用本地模型。".into()
                } else {
                    "Speech goes to the provider you configured. If it stays unreachable, the next dictation uses local models.".into()
                }
            }
            PathLabel::LocalAutoFallback if decision.llm == Slot::None => {
                if zh {
                    "云端连不上。音频留在这台电脑上。润色模型未安装，所以口授插入规则整理后的文本。"
                        .into()
                } else {
                    "Cloud is unreachable. Audio stays on this computer. The rewrite model is not installed, so dictation inserts the ruled transcript.".into()
                }
            }
            PathLabel::LocalAutoFallback => {
                if zh {
                    "云端连不上。音频留在这台电脑上，并使用本地模型。".into()
                } else {
                    "Cloud is unreachable. Audio stays on this computer and uses the local models."
                        .into()
                }
            }
            PathLabel::LocalManual if decision.llm == Slot::None => {
                if zh {
                    "仅本地。音频留在这台电脑上。润色模型未安装，所以口授插入规则整理后的文本。"
                        .into()
                } else {
                    "Local only. Audio stays on this computer. The rewrite model is not installed, so dictation inserts the ruled transcript.".into()
                }
            }
            PathLabel::LocalManual => {
                if zh {
                    "仅本地。音频留在这台电脑上，即使云端可以连通。".into()
                } else {
                    "Local only. Audio stays on this computer, even when the cloud is reachable."
                        .into()
                }
            }
            PathLabel::StrictOffline if decision.llm == Slot::None => {
                if zh {
                    "严格离线。音频留在这台电脑上。润色模型未安装，所以口授插入规则整理后的文本。"
                        .into()
                } else {
                    "Strict offline. Audio stays on this computer. The rewrite model is not installed, so dictation inserts the ruled transcript.".into()
                }
            }
            PathLabel::StrictOffline => {
                if zh {
                    "严格离线。音频留在这台电脑上。不会发起云端调用、探测、更新检查或模型下载。"
                        .into()
                } else {
                    "Strict offline. Audio stays on this computer. No cloud call, probe, update check, or model download runs.".into()
                }
            }
        }
    };
    if decision.block == Block::None && !notice.is_empty() {
        text.push(' ');
        text.push_str(notice);
    }
    text
}

impl Dictation {
    fn library_view(&self) -> LibraryView {
        let settings = self.settings.lock().expect("settings").clone();
        library::view(
            self.engines.root(),
            catalog::host_hardware(),
            &settings.local_mirror,
            settings.local_download_skipped,
            &settings.local_asr_id,
            &settings.local_llm_id,
            &self.engines.downloading(),
        )
    }
}

fn version_of(app: &AppHandle) -> String {
    full_version(&app.package_info().version.to_string(), commit())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn spawn_listener(
    app: AppHandle,
    control: Sender<Control>,
    hotkeys: Arc<Mutex<HotkeyShare>>,
    listener: Arc<Mutex<String>>,
) {
    std::thread::Builder::new()
        .name("newtype-hotkey".into())
        .spawn(move || {
            let mut engine = HotkeyEngine::default();
            let suppress_hook_until = AtomicU64::new(0);
            let (mut seen, mut capturing) = {
                let share = hotkeys.lock().expect("hotkeys");
                engine.set_bindings(share.bindings.clone());
                hotkey::set_altgr_bound(engine.uses_alt_right());
                let want = hotkey::os_capture();
                engine.set_capture(want);
                (share.generation, want)
            };
            #[cfg(windows)]
            publish_swallow(&hotkeys.lock().expect("hotkeys").bindings);
            let hook_app = app.clone();
            let on_edge = move |edge: KeyEdge| -> bool {
                let want = hotkey::os_capture();
                if want != capturing {
                    capturing = want;
                    engine.set_capture(capturing);
                }
                if !crate::key_feed::from_webview() && !capturing {
                    let until = suppress_hook_until.load(Ordering::Relaxed);
                    if now_ms() < until {
                        return false;
                    }
                }
                {
                    let share = hotkeys.lock().expect("hotkeys");
                    if share.generation != seen {
                        seen = share.generation;
                        engine.set_bindings(share.bindings.clone());
                        hotkey::set_altgr_bound(engine.uses_alt_right());
                        #[cfg(windows)]
                        publish_swallow(&share.bindings);
                    }
                    if share.capture != capturing {
                        capturing = share.capture || hotkey::os_capture();
                        engine.set_capture(capturing);
                    }
                }
                let outcome = engine.consume(edge);
                let capture_ended = outcome.events.iter().any(|event| {
                    matches!(
                        event,
                        EngineEvent::Captured(_) | EngineEvent::CaptureCancelled
                    )
                });
                if capture_ended {
                    capturing = false;
                    hotkey::set_os_capture(false);
                    let mut share = hotkeys.lock().expect("hotkeys");
                    share.capture = false;
                    if crate::key_feed::from_webview() {
                        suppress_hook_until.store(now_ms().saturating_add(400), Ordering::Relaxed);
                    }
                }
                let starting = outcome
                    .events
                    .iter()
                    .any(|event| matches!(event, EngineEvent::Pressed(_)));
                if starting {
                    let phase = hook_app.state::<Dictation>().phase.lock().expect("phase").clone();
                    if phase != "recording" && phase != "processing" {
                        focus::remember(&hook_app.state::<Dictation>().focus_target, false);
                    }
                }
                for engine_event in &outcome.events {
                    match engine_event {
                        EngineEvent::Captured(chord) => {
                            crate::diag::record(&format!("capture result {chord}"));
                        }
                        EngineEvent::CaptureCancelled => {
                            crate::diag::record("capture cancel");
                        }
                        _ => {}
                    }
                    crate::diag::record(&format!("hotkey {engine_event:?}"));
                    let _ = control.send(Control::Engine(engine_event.clone()));
                }
                outcome.suppress || capturing
            };
            set_listener(
                &app,
                &listener,
                crate::locale::text("Listening for the right-hand Control key.", "正在监听右侧 Ctrl 键。"),
            );
            crate::diag::record("hotkey listener starting");
            crate::key_feed::install(on_edge);
            #[cfg(windows)]
            {
                if let Err(err) = crate::win_hotkey::run() {
                    let message = crate::locale::pick(
                        format!("Hotkey listener is not running. {err}"),
                        format!("快捷键监听没有运行。{err}"),
                    );
                    set_listener(&app, &listener, &message);
                    crate::diag::record(&message);
                }
            }
            #[cfg(not(windows))]
            {
                let listen_result = rdev::listen(move |event| {
                    let edge = match event.event_type {
                        rdev::EventType::KeyPress(key) => {
                            hotkey::key_from_rdev(&key).map(KeyEdge::Down)
                        }
                        rdev::EventType::KeyRelease(key) => {
                            hotkey::key_from_rdev(&key).map(KeyEdge::Up)
                        }
                        _ => None,
                    };
                    if let Some(edge) = edge {
                        crate::key_feed::push(edge, false);
                    }
                });
                if let Err(err) = listen_result {
                    let message = crate::locale::pick(
                        format!(
                            "Hotkey listener is not running ({err:?}). On macOS, grant Accessibility and, if the hotkey still does nothing, Input Monitoring, then restart newtype."
                        ),
                        format!("快捷键监听没有运行（{err:?}）。请在 macOS 上允许辅助功能。如果快捷键仍然没反应，再允许输入监视，然后重启 newtype。"),
                    );
                    set_listener(&app, &listener, &message);
                    crate::diag::record(&message);
                } else {
                    let message = crate::locale::text("Hotkey listener stopped.", "快捷键监听已停止。");
                    set_listener(&app, &listener, message);
                    crate::diag::record(message);
                }
            }
        })
        .expect("hotkey thread");
}

#[cfg(windows)]
fn publish_swallow(bindings: &[(HotkeyMode, hotkey::Chord)]) {
    let vks: Vec<u32> = bindings
        .iter()
        .flat_map(|(_, chord)| chord.iter())
        .filter(|key| !hotkey::is_modifier(**key))
        .filter_map(|key| hotkey::virtual_key(**key))
        .collect();
    crate::win_hotkey::set_swallowed_vks(&vks);
}

fn set_listener(app: &AppHandle, listener: &Mutex<String>, message: &str) {
    *listener.lock().expect("listener") = message.to_string();
    let queued = app.clone();
    let _ = app.run_on_main_thread(move || {
        crate::tray::refresh_menu(&queued);
    });
}

fn session(app: AppHandle, inbox: Receiver<Control>, control: Sender<Control>) {
    let mut talk = TalkController::new(TalkMode::Toggle);
    let mut mic: Option<MicSession> = None;
    let mut route = PathLabel::Cloud.text().to_string();
    let mut started: Option<Instant> = None;
    let mut busy = false;
    while let Ok(message) = inbox.recv() {
        talk.talk_mode = current_talk_mode(&app);
        match message {
            Control::Engine(EngineEvent::Captured(chord)) => {
                let state = app.state::<Dictation>();
                let mode = state.hotkeys.lock().expect("hotkeys").capture_mode.clone();
                {
                    let mut share = state.hotkeys.lock().expect("hotkeys");
                    share.capture = false;
                    share.generation = share.generation.wrapping_add(1);
                }
                drop(state);
                let label = hotkey::parse_chord(&chord)
                    .map(|parsed| hotkey::chord_label(&parsed, hotkey::host_os()))
                    .unwrap_or_else(|_| chord.clone());
                let _ = app.emit(
                    "dictation://capture",
                    CaptureEvent {
                        mode,
                        hotkey: chord,
                        label,
                        phase: "pending".into(),
                    },
                );
            }
            Control::Engine(EngineEvent::CaptureProgress(chord)) => {
                let state = app.state::<Dictation>();
                let mode = state.hotkeys.lock().expect("hotkeys").capture_mode.clone();
                drop(state);
                let label = hotkey::parse_chord(&chord)
                    .map(|parsed| hotkey::chord_label(&parsed, hotkey::host_os()))
                    .unwrap_or_else(|_| chord.clone());
                let _ = app.emit(
                    "dictation://capture",
                    CaptureEvent {
                        mode,
                        hotkey: chord,
                        label,
                        phase: "listening".into(),
                    },
                );
            }
            Control::Engine(EngineEvent::CaptureCancelled) => {
                let state = app.state::<Dictation>();
                let mode = state.hotkeys.lock().expect("hotkeys").capture_mode.clone();
                {
                    let mut share = state.hotkeys.lock().expect("hotkeys");
                    share.capture = false;
                    share.generation = share.generation.wrapping_add(1);
                }
                drop(state);
                let _ = app.emit(
                    "dictation://capture",
                    CaptureEvent {
                        mode,
                        hotkey: String::new(),
                        label: String::new(),
                        phase: "idle".into(),
                    },
                );
            }
            Control::Engine(event) => {
                let effect = talk.on_event(&event, now_ms());
                apply(
                    &app,
                    &control,
                    &mut talk,
                    &mut mic,
                    &mut route,
                    &mut started,
                    &mut busy,
                    effect,
                );
            }
            Control::Toggle => {
                if busy {
                    emit_phase(
                        &app,
                        "processing",
                        &route,
                        crate::locale::text(
                            "Still working on the last dictation.",
                            "上一条口授还在处理。",
                        ),
                        0,
                        0.0,
                        "",
                    );
                } else if mic.is_some() {
                    talk.active = None;
                    finish_recording(&app, &mut mic, &mut started, &mut busy, &route, false);
                } else {
                    talk.active = Some(HotkeyMode::Dictate);
                    talk.started_ms = now_ms();
                    begin_recording(
                        &app,
                        &control,
                        &mut talk,
                        &mut mic,
                        &mut route,
                        &mut started,
                        HotkeyMode::Dictate,
                    );
                }
            }
            Control::Cancel => {
                app.state::<Dictation>()
                    .cancel
                    .store(true, Ordering::Relaxed);
                *app.state::<Dictation>().locked.lock().expect("locked") = None;
                talk.active = None;
                if mic.take().is_some() {
                    started = None;
                    bump(&app);
                    emit_phase(
                        &app,
                        "cancelled",
                        &route,
                        crate::locale::text(
                            "Cancelled. Nothing was inserted.",
                            "已取消。没有插入。",
                        ),
                        0,
                        0.0,
                        "",
                    );
                    schedule_hide(&app, 900);
                } else if !busy {
                    app.state::<Dictation>()
                        .cancel
                        .store(false, Ordering::Relaxed);
                    emit_phase(&app, "idle", &route, "", 0, 0.0, "");
                }
            }
            Control::AutoStop => {
                if mic.is_some() {
                    talk.active = None;
                    finish_recording(&app, &mut mic, &mut started, &mut busy, &route, false);
                }
            }
        }
    }
}

fn current_talk_mode(app: &AppHandle) -> TalkMode {
    let settings = app
        .state::<Dictation>()
        .settings
        .lock()
        .expect("settings")
        .clone();
    dictate_config::effective(&settings, hotkey::host_os())
        .map(|effective| effective.talk_mode)
        .unwrap_or(TalkMode::Toggle)
}

fn apply(
    app: &AppHandle,
    control: &Sender<Control>,
    talk: &mut TalkController,
    mic: &mut Option<MicSession>,
    route: &mut String,
    started: &mut Option<Instant>,
    busy: &mut bool,
    effect: TalkEffect,
) {
    match effect {
        TalkEffect::Start(mode) => {
            begin_recording(app, control, talk, mic, route, started, mode);
        }
        TalkEffect::Stop => {
            finish_recording(app, mic, started, busy, route, false);
        }
        TalkEffect::Cancel => {
            mic.take();
            *started = None;
            bump(app);
            emit_phase(
                app,
                "cancelled",
                route,
                crate::locale::text("Cancelled. Nothing was inserted.", "已取消。没有插入。"),
                0,
                0.0,
                "",
            );
            schedule_hide(app, 900);
        }
        TalkEffect::CancelThenStart(mode) => {
            mic.take();
            *started = None;
            bump(app);
            *app.state::<Dictation>().locked.lock().expect("locked") = None;
            talk.active = Some(mode);
            begin_recording(app, control, talk, mic, route, started, mode);
        }
        TalkEffect::Ignore => {}
    }
}

fn begin_recording(
    app: &AppHandle,
    control: &Sender<Control>,
    talk: &mut TalkController,
    mic: &mut Option<MicSession>,
    route: &mut String,
    started: &mut Option<Instant>,
    mode: HotkeyMode,
) {
    if mic.is_some() {
        return;
    }
    let state = app.state::<Dictation>();
    focus::remember(&state.focus_target, true);
    focus::log_saved(&state.focus_target);
    state.cancel.store(false, Ordering::Relaxed);
    let settings = state.settings.lock().expect("settings").clone();
    let effective = match dictate_config::effective(&settings, hotkey::host_os()) {
        Ok(effective) => effective,
        Err(err) => {
            talk.active = None;
            emit_phase(app, "error", route, &err, 0, 0.0, "");
            schedule_hide(app, 3200);
            return;
        }
    };
    let ready = readiness_of(&state);
    let decision = route_for(&state, &effective, ready);
    let hardware = catalog::host_hardware();
    let llm_id = catalog::active_id(catalog::Role::Llm, &settings.local_llm_id, hardware);
    let mut job = job_for(mode, &effective);
    *state.session_label.lock().expect("mode") = job.voice_label();
    *route = decision.label.text().to_string();
    if let Some(message) = command_block(&job, decision, &llm_id) {
        talk.active = None;
        emit_phase(app, "error", route, &message, 0, 0.0, "");
        show_indicator(app);
        schedule_hide(app, 4200);
        return;
    }
    *state.locked.lock().expect("locked") = Some(LockedSession {
        decision,
        preference: effective.preference,
        job: job.clone(),
    });
    crate::diag::record(&format!(
        "route decision preference={} path={} label=\"{}\" reason={:?} block={:?}",
        effective.preference.as_str(),
        decision.label.kind(),
        decision.label.text(),
        decision.reason,
        decision.block
    ));
    let hold = matches!(
        decision.block,
        Block::LocalNotReady | Block::CloudUnreachable
    );
    if (decision.block != Block::None || decision.asr == Slot::None) && !hold {
        talk.active = None;
        *state.locked.lock().expect("locked") = None;
        let message = router::block_message_for(decision.block, effective.preference)
            .unwrap_or("Dictation cannot start.");
        emit_phase(app, "error", route, message, 0, 0.0, "");
        show_indicator(app);
        schedule_hide(app, 3600);
        return;
    }
    let on_device = decision.asr == Slot::Local;
    if decision.asr == Slot::Cloud {
        if effective.asr_base.is_empty() || effective.asr_model.is_empty() {
            talk.active = None;
            *state.locked.lock().expect("locked") = None;
            emit_phase(
                app,
                "error",
                route,
                "Set the speech model and base URL before dictating.",
                0,
                0.0,
                "",
            );
            schedule_hide(app, 3600);
            return;
        }
        if !speech_credentials_ready(&effective.asr_provider, &settings, &state.dev_secrets) {
            talk.active = None;
            *state.locked.lock().expect("locked") = None;
            let store = state.secure_store.lock().expect("store").clone();
            let message = if store == "ready" {
                "Save an API key for the speech provider. Audio is not sent until you do."
                    .to_string()
            } else {
                store
            };
            emit_phase(app, "error", route, &message, 0, 0.0, "");
            schedule_hide(app, 5000);
            return;
        }
    }
    let mic_state = permissions::microphone_state(&state.mic_status.lock().expect("mic"));
    if matches!(mic_state.as_str(), "denied" | "restricted") {
        talk.active = None;
        *state.locked.lock().expect("locked") = None;
        let current = permissions::snapshot("");
        let (prompt, _) = permissions::access_prompt(
            std::env::consts::OS,
            &mic_state,
            &current.accessibility_state,
        );
        let message = if prompt.is_empty() {
            "Microphone access is off.".to_string()
        } else {
            prompt
        };
        emit_phase(app, "error", route, &message, 0, 0.0, "");
        show_indicator(app);
        schedule_hide(app, 5000);
        return;
    }
    let mut selection_note = String::new();
    if matches!(mode, HotkeyMode::Command) {
        let captured = selection::capture_focused();
        selection_note = captured.note;
        job = Job::Command {
            selection: captured.text,
        };
        *state.session_label.lock().expect("mode") = job.voice_label();
        if let Some(locked) = state.locked.lock().expect("locked").as_mut() {
            locked.job = job.clone();
        }
    }
    let privacy = if hold {
        router::block_message_for(decision.block, effective.preference)
            .unwrap_or(crate::locale::text(
                "This recording stays on this computer.",
                "这段录音留在这台电脑上。",
            ))
            .to_string()
    } else if on_device {
        crate::locale::text("Audio stays on this computer.", "音频留在这台电脑上。").to_string()
    } else {
        crate::locale::text(
            "Audio will be sent to your speech provider when you stop.",
            "停止后，音频会发往你的语音供应商。",
        )
        .to_string()
    };
    let recording_line = if selection_note.is_empty() {
        format!("{}. {privacy}", job.voice_label())
    } else {
        format!("{}. {privacy} {selection_note}", job.voice_label())
    };
    match audio::MicSession::start(&effective.microphone) {
        Ok(session) => {
            *state.mic_status.lock().expect("mic") = "granted".into();
            let handle = session.watch();
            let started_at = Instant::now();
            *mic = Some(session);
            *started = Some(started_at);
            let generation = bump(app);
            emit_phase(app, "recording", route, &recording_line, 0, 0.0, "");
            show_indicator(app);
            let app_loop = app.clone();
            let control = control.clone();
            let route_loop = route.clone();
            let recording_loop = recording_line.to_string();
            tauri::async_runtime::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    if app_loop
                        .state::<Dictation>()
                        .generation
                        .load(Ordering::Relaxed)
                        != generation
                    {
                        break;
                    }
                    let elapsed = started_at.elapsed().as_millis() as u64;
                    emit_phase(
                        &app_loop,
                        "recording",
                        &route_loop,
                        &recording_loop,
                        elapsed,
                        handle.level(),
                        "",
                    );
                    if handle.capped() || elapsed >= 9 * 60 * 1000 {
                        let _ = control.send(Control::AutoStop);
                        break;
                    }
                }
            });
        }
        Err(err) => {
            talk.active = None;
            *state.locked.lock().expect("locked") = None;
            *state.mic_status.lock().expect("mic") = format!("unavailable: {err}");
            emit_phase(
                app,
                "error",
                route,
                &format!("Microphone: {err}"),
                0,
                0.0,
                "",
            );
            show_indicator(app);
            schedule_hide(app, 4200);
        }
    }
}

fn finish_recording(
    app: &AppHandle,
    mic: &mut Option<MicSession>,
    started: &mut Option<Instant>,
    busy: &mut bool,
    route: &str,
    _auto: bool,
) {
    let Some(session) = mic.take() else {
        return;
    };
    let _elapsed = started.take();
    bump(app);
    *busy = true;
    emit_phase(
        app,
        "processing",
        route,
        crate::locale::text("Transcribing…", "转写中…"),
        0,
        0.0,
        "",
    );
    show_indicator(app);
    let clip = session.stop();
    if audio::is_silent(&clip.samples) {
        *app.state::<Dictation>().locked.lock().expect("locked") = None;
        *busy = false;
        emit_phase(
            app,
            "silence",
            route,
            crate::locale::text(
                "No speech detected. Nothing was inserted.",
                "没有检测到语音。没有插入。",
            ),
            0,
            0.0,
            "",
        );
        schedule_hide(app, 1600);
        return;
    }
    let outcome = transcribe_and_polish(app, clip);
    if app
        .state::<Dictation>()
        .cancel
        .swap(false, Ordering::Relaxed)
    {
        *busy = false;
        emit_phase(
            app,
            "cancelled",
            route,
            crate::locale::text("Cancelled. Nothing was inserted.", "已取消。没有插入。"),
            0,
            0.0,
            "",
        );
        schedule_hide(app, 1200);
        return;
    }
    match outcome {
        Ok(done) => {
            let mut message = done.message;
            let mut phase = done.phase;
            if done.insert && !done.text.is_empty() {
                match deliver_text(app, &done.text) {
                    Delivered::Pasted => {}
                    Delivered::Copied(notice) => {
                        message = notice;
                        phase = "copied";
                    }
                    Delivered::Failed(err) => {
                        message = crate::locale::pick(
                            format!("{message} Could not paste: {err} Copy it from history."),
                            format!("{message} 无法粘贴：{err} 请从历史复制。"),
                        );
                        phase = "error";
                    }
                }
            }
            emit_phase(app, phase, &done.route, &message, 0, 0.0, &done.text);
            schedule_hide(app, 2200);
        }
        Err(failed) => {
            emit_phase(app, "error", &failed.route, &failed.message, 0, 0.0, "");
            schedule_hide(app, 4200);
        }
    }
    *busy = false;
}

struct Done {
    text: String,
    route: String,
    phase: &'static str,
    message: String,
    insert: bool,
}

struct ProcessError {
    route: String,
    message: String,
}

fn transcribe_and_polish(app: &AppHandle, clip: AudioClip) -> Result<Done, ProcessError> {
    let state = app.state::<Dictation>();
    let settings = state.settings.lock().expect("settings").clone();
    let effective = match dictate_config::effective(&settings, hotkey::host_os()) {
        Ok(effective) => effective,
        Err(err) => {
            return Err(ProcessError {
                route: PathLabel::Cloud.text().into(),
                message: err,
            });
        }
    };
    let ready = readiness_of(&state);
    let locked = state
        .locked
        .lock()
        .expect("locked")
        .take()
        .unwrap_or(LockedSession {
            decision: route_for(&state, &effective, ready),
            preference: effective.preference,
            job: Job::Polish(effective.polish),
        });
    let dev_secrets = state.dev_secrets.clone();
    let engines = Arc::clone(&state.engines);
    let hardware = catalog::host_hardware();
    let asr_id = catalog::active_id(catalog::Role::Asr, &settings.local_asr_id, hardware);
    let llm_id = catalog::active_id(catalog::Role::Llm, &settings.local_llm_id, hardware);
    let groq_cooldown = Arc::clone(&state.groq_cooldown_until_ms);
    drop(state);

    let wav = audio::encode_wav_16k(&clip);
    let local_clip = AudioClip {
        samples: clip.samples.clone(),
        sample_rate: clip.sample_rate,
    };
    let engine_name = Arc::new(Mutex::new("local".to_string()));
    let shown = Arc::new(Mutex::new(locked.decision.label.text().to_string()));
    let app_event = app.clone();
    let shown_event = Arc::clone(&shown);
    let on_event = move |line: &str| {
        crate::diag::record(line);
        if line.contains("retrying the same") {
            let label = PathLabel::LocalAutoFallback.text();
            *shown_event.lock().expect("route") = label.to_string();
            emit_phase(
                &app_event,
                "processing",
                label,
                "Cloud was unavailable. Using the local model for this recording.",
                0,
                0.0,
                "",
            );
        }
    };
    let report_app = app.clone();
    let report_route = Arc::clone(&shown);
    let report = move |message: &str| {
        let route = report_route.lock().expect("route").clone();
        emit_phase(&report_app, "processing", &route, message, 0, 0.0, "");
    };
    let engine_for_audio = Arc::clone(&engine_name);
    let engines_audio = Arc::clone(&engines);
    let asr_for_audio = asr_id.clone();
    let llm_for_audio = llm_id.clone();
    let hotwords = effective.hotwords.clone();
    let llm_ready = ready.llm;
    let job_audio = locked.job.clone();
    let local_audio = move |_wav: &[u8]| match local_run::run(
        &engines_audio,
        hardware.os,
        AudioClip {
            samples: local_clip.samples.clone(),
            sample_rate: local_clip.sample_rate,
        },
        &asr_for_audio,
        &llm_for_audio,
        llm_ready,
        &job_audio,
        &hotwords,
        &front::front_app_name(),
        &report,
    ) {
        Ok(outcome) => {
            *engine_for_audio.lock().expect("engine") = outcome.engine;
            let mut body = outcome.finished;
            if !outcome.note.is_empty() {
                if !body.reason.is_empty() {
                    body.reason.push(' ');
                }
                body.reason.push_str(&outcome.note);
            }
            Ok(body)
        }
        Err(err) => Err(err),
    };
    let engines_rewrite = engines;
    let llm_for_rewrite = llm_id;
    let hotwords_rewrite = effective.hotwords.clone();
    let job_rewrite = locked.job.clone();
    let report_rewrite = {
        let report_app = app.clone();
        let report_route = Arc::clone(&shown);
        move |message: &str| {
            let route = report_route.lock().expect("route").clone();
            emit_phase(&report_app, "processing", &route, message, 0, 0.0, "");
        }
    };
    let local_rewrite = move |transcript: &str| {
        local_run::rewrite(
            &engines_rewrite,
            &llm_for_rewrite,
            transcript,
            &job_rewrite,
            &hotwords_rewrite,
            &front::front_app_name(),
            &report_rewrite,
        )
    };
    let asr_provider = effective.asr_provider.clone();
    let llm_provider = effective.llm_provider.clone();
    let asr_base = effective.asr_base.clone();
    let asr_model = effective.asr_model.clone();
    let asr_wire = effective.asr_wire;
    let llm_base = effective.llm_base.clone();
    let llm_model = effective.llm_model.clone();
    let thinking = effective.thinking;
    let llm_implemented = effective.llm_implemented;
    let hotwords_cloud = effective.hotwords.clone();
    let speech_hint = crate::pipeline::hotwords::asr_hint(&hotwords_cloud);
    let volc_app_id = settings
        .providers
        .get("volcengine")
        .map(|config| config.volc_app_id.clone())
        .unwrap_or_default();
    let asr_served = Arc::new(Mutex::new(asr_provider.clone()));
    let rate_fallback = settings.groq_rate_limit_fallback;
    let openai_base = settings
        .providers
        .get("openai")
        .map(|config| config.base.clone())
        .filter(|base| !base.trim().is_empty())
        .unwrap_or_else(|| {
            cloud::preset("openai")
                .map(|preset| preset.default_base.to_string())
                .unwrap_or_default()
        });
    let cloud_asr = {
        let asr_served = Arc::clone(&asr_served);
        let dev_secrets = dev_secrets.clone();
        let cooldown = Arc::clone(&groq_cooldown);
        let speech_hint = speech_hint.clone();
        let openai_base = openai_base.clone();
        move |audio: &[u8]| {
            if asr_provider != "groq" {
                if asr_wire == AsrWire::Volcengine {
                    let speech_key = secrets::get_key("volcengine-speech", &dev_secrets)
                        .ok()
                        .flatten()
                        .filter(|value| !value.is_empty())
                        .or_else(|| secrets::get_key(&asr_provider, &dev_secrets).ok().flatten())
                        .unwrap_or_default();
                    let token = secrets::get_key("volcengine-token", &dev_secrets)
                        .ok()
                        .flatten()
                        .unwrap_or_default();
                    return volcengine::transcribe_wav(
                        &VolcAuth {
                            api_key: speech_key,
                            app_id: volc_app_id.clone(),
                            token,
                        },
                        &speech_hint,
                        audio,
                    );
                }
                let key = match secrets::get_key(&asr_provider, &dev_secrets) {
                    Ok(key) => key,
                    Err(err) => return Err(err),
                };
                let Some(key) = key.filter(|value| !value.is_empty()) else {
                    return Err("Speech API key is missing.".into());
                };
                return CloudAsr {
                    base: asr_base.clone(),
                    key,
                    model: asr_model.clone(),
                    wire: asr_wire,
                    prompt: speech_hint.clone(),
                }
                .transcribe(audio);
            }
            let primary = || {
                if asr_wire == AsrWire::Volcengine {
                    let speech_key = secrets::get_key("volcengine-speech", &dev_secrets)
                        .ok()
                        .flatten()
                        .filter(|value| !value.is_empty())
                        .or_else(|| secrets::get_key(&asr_provider, &dev_secrets).ok().flatten())
                        .unwrap_or_default();
                    let token = secrets::get_key("volcengine-token", &dev_secrets)
                        .ok()
                        .flatten()
                        .unwrap_or_default();
                    return volcengine::transcribe_wav(
                        &VolcAuth {
                            api_key: speech_key,
                            app_id: volc_app_id.clone(),
                            token,
                        },
                        &speech_hint,
                        audio,
                    )
                    .map_err(ProviderError::plain);
                }
                let key = secrets::get_key(&asr_provider, &dev_secrets)
                    .map_err(ProviderError::plain)?
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| ProviderError::plain("Speech API key is missing."))?;
                cloud::transcribe_detailed(
                    &asr_base,
                    &key,
                    &asr_model,
                    asr_wire,
                    &speech_hint,
                    audio,
                )
            };
            let openai = || {
                let key = secrets::get_key("openai", &dev_secrets)
                    .map_err(ProviderError::plain)?
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| ProviderError::plain("OpenAI API key is missing."))?;
                let model = cloud::preset("openai")
                    .map(|preset| preset.default_asr)
                    .unwrap_or("gpt-transcribe");
                cloud::transcribe_detailed(
                    &openai_base,
                    &key,
                    model,
                    AsrWire::Transcriptions,
                    &speech_hint,
                    audio,
                )
            };
            let openai_ready = secrets::has_key("openai", &dev_secrets);
            match step_with_fallback(
                "asr",
                &asr_provider,
                rate_fallback,
                openai_ready,
                &cooldown,
                primary,
                openai,
            ) {
                Ok((text, served)) => {
                    *asr_served.lock().expect("asr provider") = served;
                    Ok(text)
                }
                Err(err) => Err(err),
            }
        }
    };
    let dev_secrets_llm = app.state::<Dictation>().dev_secrets.clone();
    let job_cloud = locked.job.clone();
    let llm_served = Arc::new(Mutex::new(llm_provider.clone()));
    let cloud_rewrite = {
        let llm_served = Arc::clone(&llm_served);
        let cooldown = Arc::clone(&groq_cooldown);
        let openai_base = openai_base.clone();
        move |transcript: &str| -> Result<FinishedText, String> {
            let run = |base: &str, key: &str, model: &str, thinking: Thinking| {
                let slot = Arc::new(Mutex::new(None));
                let llm = DetailedLlm {
                    base: base.to_string(),
                    key: key.to_string(),
                    model: model.to_string(),
                    thinking,
                    error: Arc::clone(&slot),
                };
                match rewrite::attempt_job(
                    Audience::Cloud,
                    &job_cloud,
                    transcript,
                    &hotwords_cloud,
                    &front::front_app_name(),
                    &llm,
                ) {
                    Ok(body) => Ok(body),
                    Err(message) => Err(slot
                        .lock()
                        .expect("llm error")
                        .clone()
                        .unwrap_or_else(|| ProviderError::plain(message))),
                }
            };
            let primary = || {
                let key = secrets::get_key(&llm_provider, &dev_secrets_llm)
                    .ok()
                    .flatten()
                    .filter(|value| !value.is_empty());
                if !llm_implemented || key.is_none() {
                    if job_cloud.needs_capable_local() {
                        return Err(ProviderError::plain(
                            "No rewrite model is configured for this mode.",
                        ));
                    }
                    let ruled =
                        crate::pipeline::polish::fallback_ruled(transcript, &hotwords_cloud);
                    return Ok(FinishedText {
                        raw: transcript.to_string(),
                        ruled: ruled.clone(),
                        text: ruled,
                        used_fallback: true,
                        reason:
                            "No rewrite model is configured, so the ruled transcript was inserted."
                                .into(),
                    });
                }
                run(&llm_base, &key.unwrap_or_default(), &llm_model, thinking)
            };
            let openai = || {
                let key = secrets::get_key("openai", &dev_secrets_llm)
                    .ok()
                    .flatten()
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| ProviderError::plain("OpenAI API key is missing."))?;
                let model = cloud::preset("openai")
                    .map(|preset| preset.default_llm)
                    .unwrap_or("gpt-4o-mini");
                let thinking = cloud::thinking_for("openai", model, "auto", "");
                run(&openai_base, &key, model, thinking)
            };
            let openai_ready = secrets::has_key("openai", &dev_secrets_llm);
            match step_with_fallback(
                "polish",
                &llm_provider,
                rate_fallback,
                openai_ready,
                &cooldown,
                primary,
                openai,
            ) {
                Ok((body, served)) => {
                    *llm_served.lock().expect("llm provider") = served;
                    Ok(body)
                }
                Err(err) => Err(err),
            }
        }
    };
    let words_for_rules = effective.hotwords.clone();
    let outcome = session::dispatch(
        locked.preference,
        locked.decision,
        ready.asr,
        ready.llm,
        &wav,
        &on_event,
        cloud_asr,
        cloud_rewrite,
        local_audio,
        local_rewrite,
        |text| crate::pipeline::polish::fallback_ruled(text, &words_for_rules),
    );
    let route_now = shown.lock().expect("route").clone();
    match outcome {
        Outcome::Done(done) => {
            let provider = match done.via {
                Via::Cloud => format!(
                    "{} / {}",
                    asr_served.lock().expect("asr provider"),
                    llm_served.lock().expect("llm provider")
                ),
                Via::FallbackRewrite => {
                    format!("{} / local", asr_served.lock().expect("asr provider"))
                }
                Via::Local | Via::FallbackAudio => {
                    format!("local / {}", engine_name.lock().expect("engine"))
                }
            };
            let label = if done.via == Via::Local {
                route_now
            } else {
                done.label.text().to_string()
            };
            let insert = task::should_insert(&locked.job, &done.body.text, done.body.used_fallback);
            let phase = if !insert {
                "withheld"
            } else if done.body.used_fallback
                || matches!(done.via, Via::FallbackAudio | Via::FallbackRewrite)
            {
                "fallback"
            } else {
                "inserted"
            };
            let message = if insert {
                done.message
            } else {
                task::withheld_message(&locked.job, &done.body.reason)
            };
            remember(
                app,
                &effective,
                &locked.job,
                &label,
                &provider,
                &done.body.raw,
                &done.body.ruled,
                &done.body.text,
                done.body.used_fallback,
                &message,
                done.keep_audio.then_some(wav.as_slice()),
            );
            Ok(Done {
                text: done.body.text,
                route: label,
                phase,
                message,
                insert,
            })
        }
        Outcome::Failed(failed) => {
            let label = failed.label.text().to_string();
            remember(
                app,
                &effective,
                &locked.job,
                &label,
                "kept",
                &failed.raw,
                &failed.ruled,
                &failed.raw,
                true,
                &failed.message,
                failed.keep_audio.then_some(wav.as_slice()),
            );
            Err(ProcessError {
                route: label,
                message: failed.message,
            })
        }
    }
}

struct DetailedLlm {
    base: String,
    key: String,
    model: String,
    thinking: Thinking,
    error: Arc<Mutex<Option<ProviderError>>>,
}

impl LlmComplete for DetailedLlm {
    fn complete(&self, request: &LlmRequest) -> Result<String, String> {
        match cloud::complete_detailed(
            &self.base,
            &self.key,
            &self.model,
            self.thinking,
            &request.system,
            &request.user,
            request.max_tokens,
        ) {
            Ok(text) => Ok(text),
            Err(err) => {
                let message = err.message.clone();
                *self.error.lock().expect("llm error") = Some(err);
                Err(message)
            }
        }
    }
}

fn step_with_fallback<T>(
    step: &str,
    provider: &str,
    enabled: bool,
    openai_ready: bool,
    cooldown: &AtomicU64,
    primary: impl FnOnce() -> Result<T, ProviderError>,
    openai: impl FnOnce() -> Result<T, ProviderError>,
) -> Result<(T, String), String> {
    let mut until = cooldown.load(Ordering::Relaxed);
    let mut log = Vec::new();
    let result = failover::with_openai_fallback(
        step,
        provider,
        enabled,
        openai_ready,
        now_ms(),
        &mut until,
        primary,
        openai,
        &mut log,
    );
    cooldown.store(until, Ordering::Relaxed);
    for line in &log {
        crate::diag::record(line);
    }
    match result {
        Ok(outcome) => Ok((outcome.value, outcome.provider)),
        Err(err) => Err(err.message),
    }
}

fn remember(
    app: &AppHandle,
    effective: &dictate_config::Effective,
    job: &Job,
    route: &str,
    provider: &str,
    raw: &str,
    ruled: &str,
    text: &str,
    used_fallback: bool,
    note: &str,
    audio: Option<&[u8]>,
) {
    let state = app.state::<Dictation>();
    if effective.history_retention == history::Retention::Off {
        return;
    }
    let entry = HistoryEntry {
        id: format!("{}-{}", now_ms(), state.generation.load(Ordering::Relaxed)),
        created_at_ms: now_ms(),
        app_version: version_of(app),
        mode: job.mode_name().into(),
        route: route.to_string(),
        provider: provider.to_string(),
        polish_level: match effective.polish {
            PolishLevel::Polish => "polish",
            PolishLevel::Literal => "literal",
        }
        .into(),
        raw: raw.into(),
        ruled: ruled.into(),
        text: text.into(),
        used_fallback,
        note: note.into(),
        audio_kept: audio.is_some(),
        dictionary_applied: Vec::new(),
        dictionary_possible: Vec::new(),
    };
    if let Some(bytes) = audio {
        let path = audio_file(&state.history_path, &entry.id);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Err(err) = std::fs::write(&path, bytes) {
            crate::diag::record(&format!("could not keep the recording: {err}"));
        }
    }
    let mut history = state.history.lock().expect("history");
    history::push(&mut history, entry);
    let removed = history::prune(&mut history, effective.history_retention, now_ms());
    let saved = history::save(&state.history_path, &history);
    drop(history);
    for id in removed {
        let _ = std::fs::remove_file(audio_file(&state.history_path, &id));
    }
    let _ = saved;
}

enum Delivered {
    Pasted,
    Copied(String),
    Failed(String),
}

fn deliver_text(app: &AppHandle, text: &str) -> Delivered {
    let target = app
        .state::<Dictation>()
        .focus_target
        .lock()
        .expect("focus")
        .clone();
    match focus::insert_with_target(text, target.as_ref()) {
        InsertOutcome::Pasted => Delivered::Pasted,
        InsertOutcome::Copied => Delivered::Copied(focus::notice().to_string()),
        InsertOutcome::Failed(err) => Delivered::Failed(err),
    }
}

fn bump(app: &AppHandle) -> u64 {
    app.state::<Dictation>()
        .generation
        .fetch_add(1, Ordering::Relaxed)
        + 1
}

fn emit_phase(
    app: &AppHandle,
    phase: &str,
    route: &str,
    message: &str,
    elapsed_ms: u64,
    level: f32,
    text: &str,
) {
    if let Ok(mut slot) = app.state::<Dictation>().phase.lock() {
        *slot = phase.to_string();
    }
    if phase != "recording" {
        crate::diag::record(&format!("phase {phase}: {message}"));
    }
    let _ = app.emit(
        "dictation://phase",
        PhaseEvent {
            phase: phase.into(),
            route: route.into(),
            message: message.into(),
            elapsed_ms,
            level,
            text: text.into(),
            mode: app
                .state::<Dictation>()
                .session_label
                .lock()
                .map(|label| label.clone())
                .unwrap_or_default(),
        },
    );
    show_indicator(app);
    if phase != "recording" {
        crate::tray::refresh_menu(app);
    }
}

const LAYOUT_HIDDEN: u8 = 0;
const LAYOUT_FLOATING: u8 = 1;
#[cfg(windows)]
const LAYOUT_TASKBAR: u8 = 2;

fn show_indicator(app: &AppHandle) {
    sync_indicator(app);
}

#[cfg(windows)]
fn watch_taskbar(app: AppHandle) {
    std::thread::Builder::new()
        .name("newtype-taskbar".into())
        .spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_millis(400));
            let hop = app.clone();
            let shown = app.clone();
            if hop
                .run_on_main_thread(move || sync_indicator(&shown))
                .is_err()
            {
                break;
            }
        })
        .ok();
}

fn sync_indicator(app: &AppHandle) {
    use std::sync::atomic::AtomicU8;
    static LAST: AtomicU8 = AtomicU8::new(255);
    if app.get_webview_window("indicator").is_none() {
        open_indicator(app);
    }
    let Some(window) = app.get_webview_window("indicator") else {
        log_indicator("window", "indicator window could not be created");
        return;
    };
    let raw = app
        .state::<Dictation>()
        .settings
        .lock()
        .map(|settings| settings.indicator_place.clone())
        .unwrap_or_default();
    let place = dictate_config::indicator_place(&raw);
    log_indicator("mode", &format!("indicator mode {place}"));
    match place {
        "hidden" => {
            let _ = window.hide();
            mark_indicator(&window, "hidden");
            LAST.store(LAYOUT_HIDDEN, Ordering::Relaxed);
            clear_indicator_log("fallback");
            clear_indicator_log("strip");
        }
        "taskbar" => {
            if !dock_indicator(app, &window, &LAST) {
                let previous = LAST.swap(LAYOUT_FLOATING, Ordering::Relaxed);
                show_floating(&window, previous != LAYOUT_FLOATING);
            }
        }
        _ => {
            let previous = LAST.swap(LAYOUT_FLOATING, Ordering::Relaxed);
            show_floating(&window, previous != LAYOUT_FLOATING);
            clear_indicator_log("fallback");
            clear_indicator_log("strip");
        }
    }
}

#[cfg(windows)]
fn remembered_dock_failure(taskbar: crate::taskbar_dock::PxRect) -> Option<String> {
    dock_failures().lock().expect("dock failure").as_ref().and_then(
        |(rect, reason)| (*rect == taskbar).then(|| reason.clone()),
    )
}

#[cfg(windows)]
fn remember_dock_failure(taskbar: crate::taskbar_dock::PxRect, reason: &str) {
    // These fail before the window moves, so the next tick can try again.
    if reason == "indicator window has no handle" || reason == "could not size the strip" {
        return;
    }
    *dock_failures().lock().expect("dock failure") = Some((taskbar, reason.to_string()));
}

#[cfg(windows)]
fn clear_dock_failure() {
    *dock_failures().lock().expect("dock failure") = None;
}

#[cfg(windows)]
fn dock_failures() -> &'static std::sync::Mutex<Option<(crate::taskbar_dock::PxRect, String)>> {
    static LAST: std::sync::OnceLock<
        std::sync::Mutex<Option<(crate::taskbar_dock::PxRect, String)>>,
    > = std::sync::OnceLock::new();
    LAST.get_or_init(|| std::sync::Mutex::new(None))
}

fn log_indicator(category: &str, line: &str) {
    note_indicator(category, Some(line));
}

fn clear_indicator_log(category: &str) {
    note_indicator(category, None);
}

fn note_indicator(category: &str, line: Option<&str>) {
    use std::collections::BTreeMap;
    use std::sync::Mutex;
    static LAST: Mutex<BTreeMap<String, String>> = Mutex::new(BTreeMap::new());
    let mut last = LAST.lock().expect("indicator log");
    let Some(line) = line else {
        last.remove(category);
        return;
    };
    if last.get(category).map(String::as_str) == Some(line) {
        return;
    }
    last.insert(category.to_string(), line.to_string());
    crate::diag::record(line);
}

fn show_floating(window: &tauri::WebviewWindow, reposition: bool) {
    if reposition || !window.is_visible().unwrap_or(false) {
        place(window);
        crate::overlay::configure_indicator(window);
        crate::overlay::show_without_activating(window);
    } else {
        crate::overlay::clamp_indicator(window, None);
    }
    mark_indicator(window, "floating");
}

fn dock_indicator(
    _app: &AppHandle,
    window: &tauri::WebviewWindow,
    last: &std::sync::atomic::AtomicU8,
) -> bool {
    #[cfg(windows)]
    {
        let reading = match crate::taskbar_dock::read_taskbar() {
            Ok(reading) => reading,
            Err(reason) => {
                log_indicator(
                    "fallback",
                    &format!("indicator fallback floating: {reason}"),
                );
                clear_indicator_log("strip");
                return false;
            }
        };
        log_indicator(
            "taskbar",
            &format!(
                "indicator taskbar rect {},{} {}x{} edge {}",
                reading.taskbar.x,
                reading.taskbar.y,
                reading.taskbar.width,
                reading.taskbar.height,
                reading.edge.as_str()
            ),
        );
        log_indicator(
            "tray",
            &format!(
                "indicator tray rect {},{} {}x{}",
                reading.tray.x, reading.tray.y, reading.tray.width, reading.tray.height
            ),
        );
        if reading.fullscreen {
            let _ = window.hide();
            log_indicator("hidden", "indicator hidden: fullscreen app");
            clear_indicator_log("fallback");
            clear_indicator_log("strip");
            return true;
        }
        let thickness = match reading.edge {
            crate::taskbar_dock::BarEdge::Left | crate::taskbar_dock::BarEdge::Right => {
                reading.taskbar.width
            }
            _ => reading.taskbar.height,
        };
        let (desired, minimum) = crate::taskbar_dock::dock_sizes(thickness);
        let icons_end = reading.icons_end.or_else(|| match reading.edge {
            crate::taskbar_dock::BarEdge::Top | crate::taskbar_dock::BarEdge::Bottom => {
                Some(reading.taskbar.x + reading.taskbar.width / 2)
            }
            crate::taskbar_dock::BarEdge::Left | crate::taskbar_dock::BarEdge::Right => {
                Some(reading.taskbar.y + reading.taskbar.height / 2)
            }
        });
        match crate::taskbar_dock::fit_dock(&crate::taskbar_dock::DockQuery {
            taskbar: reading.taskbar,
            edge: reading.edge,
            tray_start: reading.tray_start,
            icons_end,
            desired,
            minimum,
            gap: 4,
        }) {
            crate::taskbar_dock::DockFit::Collapsed => {
                let _ = window.hide();
                log_indicator("hidden", "indicator hidden: taskbar collapsed");
                clear_indicator_log("fallback");
                clear_indicator_log("strip");
                true
            }
            crate::taskbar_dock::DockFit::NoRoom => {
                log_indicator(
                    "fallback",
                    "indicator fallback floating: gap beside the tray is too small",
                );
                clear_indicator_log("strip");
                false
            }
            crate::taskbar_dock::DockFit::Slot(slot) => {
                if let Some(reason) = remembered_dock_failure(reading.taskbar) {
                    log_indicator(
                        "fallback",
                        &format!("indicator fallback floating: {reason}"),
                    );
                    clear_indicator_log("strip");
                    return false;
                }
                match place_taskbar_strip(window, slot) {
                    Ok(visible) => {
                        clear_dock_failure();
                        log_indicator(
                            "strip",
                            &format!(
                                "indicator strip rect {},{} {}x{}",
                                visible.x, visible.y, visible.width, visible.height
                            ),
                        );
                        clear_indicator_log("fallback");
                        clear_indicator_log("hidden");
                        mark_indicator(window, "taskbar");
                        last.store(LAYOUT_TASKBAR, Ordering::Relaxed);
                        true
                    }
                    Err(reason) => {
                        remember_dock_failure(reading.taskbar, &reason);
                        log_indicator(
                            "fallback",
                            &format!("indicator fallback floating: {reason}"),
                        );
                        clear_indicator_log("strip");
                        false
                    }
                }
            }
        }
    }
    #[cfg(not(windows))]
    {
        let _ = (_app, window, last);
        log_indicator(
            "fallback",
            "indicator fallback floating: taskbar docking is windows-only",
        );
        false
    }
}

#[cfg(windows)]
fn place_taskbar_strip(
    window: &tauri::WebviewWindow,
    slot: crate::taskbar_dock::PxRect,
) -> Result<crate::taskbar_dock::PxRect, String> {
    use tauri::{PhysicalPosition, PhysicalSize, Position, Size};
    let width = slot.width.max(1) as u32;
    let height = slot.height.max(1) as u32;
    window
        .set_size(Size::Physical(PhysicalSize { width, height }))
        .map_err(|_| "could not size the strip".to_string())?;
    window
        .set_position(Position::Physical(PhysicalPosition {
            x: slot.x,
            y: slot.y,
        }))
        .map_err(|_| "could not move the strip".to_string())?;
    crate::overlay::configure_indicator(window);
    let hwnd = crate::overlay::indicator_hwnd(window)
        .ok_or_else(|| "indicator window has no handle".to_string())?;
    let visible = crate::taskbar_dock::place_strip(hwnd, slot)?;
    if !window.is_visible().unwrap_or(false) {
        crate::overlay::show_without_activating(window);
    }
    Ok(visible)
}

fn mark_indicator(window: &tauri::WebviewWindow, place: &str) {
    let _ = window.eval(format!("document.body.dataset.place='{place}'"));
}

fn schedule_hide(app: &AppHandle, delay_ms: u64) {
    let app = app.clone();
    let generation = app.state::<Dictation>().generation.load(Ordering::Relaxed);
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
        if app.state::<Dictation>().generation.load(Ordering::Relaxed) == generation {
            let route = app.state::<Dictation>().route_label();
            emit_phase(&app, "idle", &route, "", 0, 0.0, "");
        }
    });
}

fn place(window: &tauri::WebviewWindow) {
    let dir = window.app_handle().path().app_config_dir().ok();
    crate::overlay::place_indicator(window, dir.as_deref());
}

fn open_indicator(app: &AppHandle) {
    if app.get_webview_window("indicator").is_some() {
        return;
    }
    let built = WebviewWindowBuilder::new(
        app,
        "indicator",
        WebviewUrl::App("index.html?view=indicator".into()),
    )
    .title("newtype")
    .inner_size(256.0, 44.0)
    .decorations(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .focused(false)
    .visible(false)
    .transparent(true)
    .background_color(tauri::webview::Color(0, 0, 0, 0))
    .resizable(false)
    .shadow(false)
    .initialization_script("window.__newtypeIndicator = true;")
    .build();
    match built {
        Ok(window) => {
            crate::overlay::configure_indicator(&window);
            crate::diag::record("indicator window created");
        }
        Err(_) => crate::diag::record("indicator window could not be created"),
    }
}

pub fn save_settings_patch(
    app: &AppHandle,
    patch: &serde_json::Value,
) -> Result<Snapshot, String> {
    let stored = app
        .state::<Dictation>()
        .settings
        .lock()
        .expect("settings")
        .clone();
    let merged = dictate_config::merge_ui_settings(&stored, patch)?;
    save_settings(app, merged)
}

pub fn save_settings(app: &AppHandle, mut settings: DictationSettings) -> Result<Snapshot, String> {
    dictate_config::normalize(&mut settings);
    let warnings = dictate_config::validate(&settings, hotkey::host_os())?;
    let effective = dictate_config::effective(&settings, hotkey::host_os())?;
    let state = app.state::<Dictation>();
    let previous = state.settings.lock().expect("settings").clone();
    dictate_config::save(&state.settings_path, &settings)?;
    {
        let mut share = state.hotkeys.lock().expect("hotkeys");
        share.bindings = effective.bindings;
        share.generation = share.generation.wrapping_add(1);
    }
    *state.settings.lock().expect("settings") = settings.clone();
    crate::locale::set_preference(&settings.ui_language);
    log_applied_hotkeys(&previous, &settings);
    let _ = warnings;
    apply_route_mode(&state, true);
    drop(state);
    publish_route(app);
    crate::material::apply(app, settings.window_translucent);
    sync_indicator(app);
    Ok(app.state::<Dictation>().snapshot(app))
}

pub fn shutdown(app: &AppHandle) {
    if let Some(state) = app.try_state::<Dictation>() {
        state.engines.shutdown();
    }
}

pub fn fallback_line(app: &AppHandle) -> String {
    let state = app.state::<Dictation>();
    let ready = readiness_of(&state);
    library::fallback_menu(ready.asr, ready.llm).into()
}

pub fn local_download(
    app: &AppHandle,
    ids: Vec<String>,
    confirm_network: bool,
) -> Result<(), String> {
    let _ = confirm_network;
    egress::authorize(egress::Purpose::ModelDownload)?;
    let state = app.state::<Dictation>();
    let settings = state.settings.lock().expect("settings").clone();
    let mirror = Mirror::parse(&settings.local_mirror).unwrap_or(Mirror::Auto);
    let engines = Arc::clone(&state.engines);
    drop(state);
    engines.spawn_download(app.clone(), ids, hotkey::host_os(), mirror)
}

pub fn local_cancel_download(app: &AppHandle) {
    app.state::<Dictation>().engines.cancel_download();
}

pub fn local_delete(app: &AppHandle, id: &str) -> Result<Snapshot, String> {
    let state = app.state::<Dictation>();
    library::delete_model(state.engines.root(), id)?;
    state.engines.shutdown();
    crate::diag::record(&format!("deleted local model {id}"));
    crate::tray::refresh_menu(app);
    Ok(state.snapshot(app))
}

pub fn local_skip_download(app: &AppHandle) -> Result<Snapshot, String> {
    let state = app.state::<Dictation>();
    {
        let mut settings = state.settings.lock().expect("settings");
        settings.local_download_skipped = true;
        dictate_config::save(&state.settings_path, &settings)?;
    }
    crate::diag::record("local model download skipped");
    crate::tray::refresh_menu(app);
    Ok(state.snapshot(app))
}

pub fn local_set_mirror(app: &AppHandle, mirror: &str) -> Result<Snapshot, String> {
    let parsed = Mirror::parse(mirror).ok_or_else(|| "Unknown download source.".to_string())?;
    let state = app.state::<Dictation>();
    {
        let mut settings = state.settings.lock().expect("settings");
        settings.local_mirror = parsed.as_str().into();
        dictate_config::save(&state.settings_path, &settings)?;
    }
    crate::tray::refresh_menu(app);
    Ok(state.snapshot(app))
}

pub fn local_use_model(app: &AppHandle, role: &str, id: &str) -> Result<Snapshot, String> {
    if role != "asr" && role != "llm" {
        return Err(format!("Unknown model role \"{role}\"."));
    }
    if !id.is_empty() {
        let card = catalog::card(id).ok_or_else(|| format!("Unknown model \"{id}\"."))?;
        let expected = if role == "asr" {
            catalog::Role::Asr
        } else {
            catalog::Role::Llm
        };
        if card.role != expected {
            return Err(format!("{} is not a {role} model.", card.name));
        }
    }
    let state = app.state::<Dictation>();
    {
        let mut settings = state.settings.lock().expect("settings");
        if role == "asr" {
            settings.local_asr_id = id.to_string();
        } else {
            settings.local_llm_id = id.to_string();
        }
        dictate_config::save(&state.settings_path, &settings)?;
        let hardware = catalog::host_hardware();
        let asr = catalog::active_id(catalog::Role::Asr, &settings.local_asr_id, hardware);
        let llm = catalog::active_id(catalog::Role::Llm, &settings.local_llm_id, hardware);
        state.engines.retain(&asr, &llm);
    }
    crate::diag::record(&format!("selected local model role={role}"));
    crate::tray::refresh_menu(app);
    Ok(state.snapshot(app))
}

pub fn set_hotkey(app: &AppHandle, mode: &str, chord: &str) -> Result<Snapshot, String> {
    let state = app.state::<Dictation>();
    let mut settings = state.settings.lock().expect("settings").clone();
    drop(state);
    dictate_config::assign_hotkey(&mut settings, mode, chord, hotkey::host_os())?;
    save_settings(app, settings)
}

fn log_applied_hotkeys(previous: &DictationSettings, settings: &DictationSettings) {
    let os = hotkey::host_os();
    let rows = [
        (
            "dictate",
            HotkeyMode::Dictate,
            &previous.dictate_hotkey,
            &settings.dictate_hotkey,
        ),
        (
            "translate",
            HotkeyMode::Translate,
            &previous.translate_hotkey,
            &settings.translate_hotkey,
        ),
        (
            "command",
            HotkeyMode::Command,
            &previous.command_hotkey,
            &settings.command_hotkey,
        ),
    ];
    for (name, mode, before, after) in rows {
        if before == after {
            continue;
        }
        let described = match hotkey::effective_chord(after, mode, os) {
            Ok(chord) => format!(
                "{} ({})",
                hotkey::canonical(&chord),
                hotkey::chord_label(&chord, os)
            ),
            Err(_) => after.clone(),
        };
        crate::diag::record(&format!("applied hotkey {name} {described}"));
    }
}

pub fn set_key(app: &AppHandle, provider: &str, key: &str) -> Result<String, String> {
    let state = app.state::<Dictation>();
    let store_status = state.secure_store.lock().expect("store").clone();
    if store_status != "ready" {
        return Err(store_status);
    }
    secrets::set_key(provider, key, &state.dev_secrets)?;
    let settings = state.settings.lock().expect("settings").clone();
    dictate_config::save(&state.settings_path, &settings)?;
    let message = format!(
        "Saved the {provider} key and read it back from {}.",
        secrets::store_name()
    );
    crate::diag::record(&format!("stored credential for provider {provider}"));
    Ok(message)
}

pub fn listener_line(app: &AppHandle) -> String {
    app.state::<Dictation>()
        .listener
        .lock()
        .expect("listener")
        .clone()
}

pub fn log_line() -> String {
    crate::diag::path().unwrap_or_else(|| "log not open".into())
}

pub fn clear_key(app: &AppHandle, provider: &str) -> Result<(), String> {
    let path = app.state::<Dictation>().dev_secrets.clone();
    secrets::clear_key(provider, &path)
}

fn speech_credentials_ready(
    provider: &str,
    settings: &DictationSettings,
    secrets_path: &std::path::Path,
) -> bool {
    if provider == "volcengine" {
        let app_id = settings
            .providers
            .get("volcengine")
            .map(|config| !config.volc_app_id.trim().is_empty())
            .unwrap_or(false);
        let token = secrets::has_key("volcengine-token", secrets_path);
        let speech = secrets::has_key("volcengine-speech", secrets_path)
            || secrets::has_key("volcengine", secrets_path);
        return speech || (app_id && token);
    }
    secrets::has_key(provider, secrets_path)
}

fn volc_auth(settings: &DictationSettings, secrets_path: &std::path::Path) -> VolcAuth {
    let speech = secrets::get_key("volcengine-speech", secrets_path)
        .ok()
        .flatten()
        .filter(|value| !value.is_empty())
        .or_else(|| {
            secrets::get_key("volcengine", secrets_path)
                .ok()
                .flatten()
                .filter(|value| !value.is_empty())
        })
        .unwrap_or_default();
    let token = secrets::get_key("volcengine-token", secrets_path)
        .ok()
        .flatten()
        .unwrap_or_default();
    let app_id = settings
        .providers
        .get("volcengine")
        .map(|config| config.volc_app_id.clone())
        .unwrap_or_default();
    VolcAuth {
        api_key: speech,
        app_id,
        token,
    }
}

pub fn test_connection(app: &AppHandle, provider: &str, kind: &str) -> Result<String, String> {
    let state = app.state::<Dictation>();
    let settings = state.settings.lock().expect("settings").clone();
    if provider == "volcengine" && kind != "rewrite" {
        return volcengine::test_connection(&volc_auth(&settings, &state.dev_secrets));
    }
    let effective = dictate_config::effective(&settings, hotkey::host_os())?;
    let (base, model_ready) = if provider == effective.llm_provider {
        (
            effective.llm_base,
            effective.llm_implemented || provider == "volcengine",
        )
    } else if provider == settings.asr_provider {
        (provider_base(&settings, provider), true)
    } else {
        (provider_base(&settings, provider), true)
    };
    let _ = model_ready;
    let key = secrets::get_key(provider, &state.dev_secrets)?
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "Save an API key for this provider first.".to_string())?;
    if base.is_empty() {
        return Err("This provider needs a base URL before it can be tested.".into());
    }
    cloud::test_connection(&base, &key)
}

pub fn list_models(app: &AppHandle, provider: &str) -> Result<Vec<String>, String> {
    let state = app.state::<Dictation>();
    let settings = state.settings.lock().expect("settings").clone();
    let base = provider_base(&settings, provider);
    let key = secrets::get_key(provider, &state.dev_secrets)?
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "Save an API key for this provider first.".to_string())?;
    cloud::list_model_ids(&base, &key)
}

fn provider_base(settings: &DictationSettings, provider: &str) -> String {
    dictate_config::effective(settings, hotkey::host_os())
        .ok()
        .and_then(|effective| {
            if provider == effective.asr_provider {
                Some(effective.asr_base)
            } else if provider == effective.llm_provider {
                Some(effective.llm_base)
            } else {
                None
            }
        })
        .or_else(|| {
            settings.providers.get(provider).map(|config| {
                if !config.base.trim().is_empty() {
                    config.base.trim().to_string()
                } else if !config.llm_base.trim().is_empty() {
                    config.llm_base.trim().to_string()
                } else {
                    config.asr_base.trim().to_string()
                }
            })
        })
        .unwrap_or_default()
}

pub fn begin_capture(app: &AppHandle, mode: &str) -> Result<(), String> {
    if !matches!(mode, "dictate" | "translate" | "command") {
        crate::diag::record(&format!("capture start rejected mode={mode}"));
        return Err("Unknown hotkey.".into());
    }
    // Publish capture before any window call. Focusing first let the hook keep
    // matching Dictate while this command was still inside set_focus.
    hotkey::set_os_capture(true);
    {
        let state = app.state::<Dictation>();
        let mut share = state.hotkeys.lock().expect("hotkeys");
        share.capture = true;
        share.capture_mode = mode.to_string();
        share.generation = share.generation.wrapping_add(1);
    }
    crate::diag::record(&format!("capture start mode={mode}"));
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        if !window.is_focused().unwrap_or(false) {
            let _ = window.set_focus();
        }
    }
    Ok(())
}

pub fn capture_key(app: &AppHandle, code: &str, down: bool, alt: bool) {
    let capturing = app
        .state::<Dictation>()
        .hotkeys
        .lock()
        .expect("hotkeys")
        .capture;
    if !capturing {
        return;
    }
    let Some(edge) = hotkey::edge_from_code(code, down, alt) else {
        return;
    };
    crate::key_feed::push(edge, true);
}

pub fn is_busy(app: &AppHandle) -> bool {
    app.try_state::<Dictation>()
        .and_then(|state| state.phase.lock().ok().map(|phase| phase.clone()))
        .is_some_and(|phase| phase == "recording" || phase == "processing")
}

pub fn remember_focus(app: &AppHandle) {
    if let Some(state) = app.try_state::<Dictation>() {
        focus::remember(&state.focus_target, false);
    }
}

pub fn cancel_capture(app: &AppHandle) {
    hotkey::set_os_capture(false);
    let state = app.state::<Dictation>();
    let mut share = state.hotkeys.lock().expect("hotkeys");
    share.capture = false;
    share.generation = share.generation.wrapping_add(1);
    drop(share);
    crate::diag::record("capture cancel");
}

fn job_for(mode: HotkeyMode, effective: &dictate_config::Effective) -> Job {
    match mode {
        HotkeyMode::Dictate => Job::Polish(effective.polish),
        HotkeyMode::Translate => Job::Translate {
            target: effective.target_language.clone(),
        },
        HotkeyMode::Command => Job::Command { selection: None },
    }
}

fn command_block(job: &Job, decision: RouteDecision, llm_id: &str) -> Option<String> {
    if !matches!(job, Job::Command { .. }) {
        return None;
    }
    if decision.block != Block::None || decision.asr == Slot::None {
        return None;
    }
    if decision.llm == Slot::Cloud {
        return None;
    }
    if decision.llm == Slot::Local && task::local_rewrite_capable(llm_id) {
        return None;
    }
    Some(task::small_model_reason(job))
}

pub fn history(app: &AppHandle) -> Vec<HistoryEntry> {
    let state = app.state::<Dictation>();
    let words = state.settings.lock().expect("settings").hotwords.clone();
    let mut entries = state.history.lock().expect("history").clone();
    for entry in &mut entries {
        let hit = crate::pipeline::hotwords::analyze(&entry.raw, &words);
        entry.dictionary_applied = hit.applied;
        entry.dictionary_possible = hit.possible;
    }
    entries
}

pub fn delete_history(app: &AppHandle, id: &str) -> Result<(), String> {
    let state = app.state::<Dictation>();
    let audio = audio_file(&state.history_path, id);
    let mut history = state.history.lock().expect("history");
    history.retain(|entry| entry.id != id);
    let saved = history::save(&state.history_path, &history);
    drop(history);
    let _ = std::fs::remove_file(audio);
    saved
}

pub fn clear_history(app: &AppHandle) -> Result<(), String> {
    let state = app.state::<Dictation>();
    let ids: Vec<String> = {
        let mut history = state.history.lock().expect("history");
        let ids = history.iter().map(|entry| entry.id.clone()).collect();
        history.clear();
        history::save(&state.history_path, &history)?;
        ids
    };
    for id in ids {
        let _ = std::fs::remove_file(audio_file(&state.history_path, &id));
    }
    Ok(())
}

pub fn reinsert_history(app: &AppHandle, id: &str) -> Result<(), String> {
    let text = {
        let state = app.state::<Dictation>();
        let history = state.history.lock().expect("history");
        history
            .iter()
            .find(|entry| entry.id == id)
            .map(|entry| entry.text.clone())
            .ok_or_else(|| "That history entry is gone.".to_string())?
    };
    if text.trim().is_empty() {
        return Err("That history entry has no text to insert.".into());
    }
    let target = app
        .state::<Dictation>()
        .focus_target
        .lock()
        .expect("focus")
        .clone();
    match focus::insert_with_target(&text, target.as_ref()) {
        InsertOutcome::Pasted => Ok(()),
        InsertOutcome::Copied => Err(focus::notice().to_string()),
        InsertOutcome::Failed(err) => Err(err),
    }
}

pub fn export_dictionary(app: &AppHandle) -> Result<String, String> {
    let settings = app
        .state::<Dictation>()
        .settings
        .lock()
        .expect("settings")
        .clone();
    crate::pipeline::hotwords::export_json(&settings.hotwords)
}

pub fn import_dictionary(app: &AppHandle, body: &str) -> Result<Snapshot, String> {
    let incoming = crate::pipeline::hotwords::import_any(body).map_err(|err| err.to_string())?;
    let state = app.state::<Dictation>();
    let mut settings = state.settings.lock().expect("settings").clone();
    settings.hotwords = crate::pipeline::hotwords::merge(&settings.hotwords, &incoming);
    crate::pipeline::hotwords::validate(&settings.hotwords).map_err(|err| err.to_string())?;
    drop(state);
    save_settings(app, settings)
}

pub fn rerun_history(app: &AppHandle, id: &str) -> Result<String, String> {
    let state = app.state::<Dictation>();
    let phase = state.phase.lock().expect("phase").clone();
    if phase == "recording" || phase == "processing" {
        return Err("Finish the current dictation before re-running a recording.".into());
    }
    let entry = state
        .history
        .lock()
        .expect("history")
        .iter()
        .find(|entry| entry.id == id)
        .cloned()
        .ok_or_else(|| "That history entry is gone.".to_string())?;
    if !entry.audio_kept {
        return Err("This entry has no saved recording to re-run.".into());
    }
    let bytes = std::fs::read(audio_file(&state.history_path, id))
        .map_err(|_| "The saved recording is missing.".to_string())?;
    let clip = audio::decode_wav(&bytes)?;
    let settings = state.settings.lock().expect("settings").clone();
    let effective = dictate_config::effective(&settings, hotkey::host_os())?;
    let ready = readiness_of(&state);
    let decision = route_for(&state, &effective, ready);
    let mode = match entry.mode.as_str() {
        "translate" => HotkeyMode::Translate,
        "command" => HotkeyMode::Command,
        _ => HotkeyMode::Dictate,
    };
    let job = job_for(mode, &effective);
    let hardware = catalog::host_hardware();
    let llm_id = catalog::active_id(catalog::Role::Llm, &settings.local_llm_id, hardware);
    if let Some(message) = command_block(&job, decision, &llm_id) {
        return Err(message);
    }
    *state.session_label.lock().expect("mode") = job.voice_label();
    *state.locked.lock().expect("locked") = Some(LockedSession {
        decision,
        preference: effective.preference,
        job,
    });
    drop(state);
    match transcribe_and_polish(app, clip) {
        Ok(done) => {
            if done.insert && !done.text.is_empty() {
                match deliver_text(app, &done.text) {
                    Delivered::Pasted => {}
                    Delivered::Copied(notice) => return Ok(notice),
                    Delivered::Failed(err) => return Err(err),
                }
            }
            Ok(done.message)
        }
        Err(failed) => Err(failed.message),
    }
}

pub fn copy_history(app: &AppHandle, id: &str) -> Result<(), String> {
    let state = app.state::<Dictation>();
    let text = {
        let history = state.history.lock().expect("history");
        history
            .iter()
            .find(|entry| entry.id == id)
            .map(|entry| entry.text.clone())
            .ok_or_else(|| "That history entry is gone.".to_string())?
    };
    insert::copy_text(&text)
}

pub fn microphones() -> Result<Vec<String>, String> {
    audio::input_devices()
}

pub fn probe_microphone(app: &AppHandle, device: &str) -> Result<String, String> {
    let session = audio::MicSession::start(device)?;
    std::thread::sleep(std::time::Duration::from_millis(350));
    let level = session.level();
    let clip = session.stop();
    let heard = if audio::is_silent(&clip.samples) {
        "opened, but no speech was heard"
    } else {
        "opened and heard audio"
    };
    *app.state::<Dictation>().mic_status.lock().expect("mic") = "granted".into();
    Ok(format!("Microphone {heard} (level {level:.2})."))
}

fn audio_file(history_path: &std::path::Path, id: &str) -> std::path::PathBuf {
    history_path
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("history-audio")
        .join(format!("{id}.wav"))
}

fn apply_route_mode(state: &Dictation, cancel_downloads: bool) {
    let settings = state.settings.lock().expect("settings").clone();
    let preference =
        RoutePreference::parse(&settings.preference).unwrap_or(RoutePreference::CloudDefault);
    let mode = match preference {
        RoutePreference::CloudDefault | RoutePreference::CloudOnly => NetMode::Open,
        RoutePreference::LocalOnly => NetMode::LocalOnly,
        RoutePreference::StrictOffline => NetMode::StrictOffline,
    };
    let previous = egress::process_mode();
    egress::set_mode(mode);
    if mode == NetMode::Open && previous != NetMode::Open {
        state.probe_epoch.fetch_add(1, Ordering::Relaxed);
        *state.reachability.lock().expect("reachability") = Reachability::Unknown;
        *state.notice.lock().expect("notice") = String::new();
        state.probe_wake.store(true, Ordering::Relaxed);
    } else if mode == NetMode::Open {
        state.probe_wake.store(true, Ordering::Relaxed);
    } else {
        *state.notice.lock().expect("notice") = String::new();
        if mode == NetMode::StrictOffline && cancel_downloads {
            state.engines.cancel_download();
        }
    }
    crate::diag::record(&format!("route mode {}", preference.as_str()));
}

fn connectivity_line(state: &Dictation) -> String {
    match egress::process_mode() {
        NetMode::StrictOffline => crate::locale::text(
            "Strict offline. No cloud calls, probes, update checks, or model downloads.",
            "严格离线。不会发起云端调用、探测、更新检查或模型下载。",
        )
        .into(),
        NetMode::LocalOnly => crate::locale::text(
            "Local only. Connectivity probes are off.",
            "仅本地。已关闭连通性探测。",
        )
        .into(),
        NetMode::Open => match *state.reachability.lock().expect("reachability") {
            Reachability::Unknown => crate::locale::text(
                "Checking the configured cloud provider.",
                "正在检查配置的云端供应商。",
            )
            .into(),
            Reachability::Reachable => {
                crate::locale::text("Cloud provider reachable.", "云端供应商可以连通。").into()
            }
            Reachability::Unreachable => {
                crate::locale::text("Cloud provider unreachable.", "云端供应商连不上。").into()
            }
        },
    }
}

fn notice_for(
    transition: probe::Transition,
    preference: RoutePreference,
    local_ready: bool,
) -> String {
    match (transition, preference) {
        (probe::Transition::Offline, RoutePreference::CloudDefault) if local_ready => {
            crate::locale::text(
                "The cloud provider is unreachable. New dictations use the local model.",
                "云端供应商连不上。新的口授改用本地模型。",
            )
            .into()
        }
        (probe::Transition::Offline, RoutePreference::CloudDefault) => crate::locale::text(
            "The cloud provider is unreachable, and the local speech model is not installed. Recordings stay on this computer.",
            "云端供应商连不上，本地语音模型也还没安装。录音留在这台电脑上。",
        )
        .into(),
        (probe::Transition::Offline, RoutePreference::CloudOnly) => crate::locale::text(
            "The cloud provider is unreachable. Cloud only will not use local models.",
            "云端供应商连不上。仅云端不会使用本地模型。",
        )
        .into(),
        (probe::Transition::Online, RoutePreference::CloudDefault) => crate::locale::text(
            "The cloud provider is back. The next dictation uses cloud.",
            "云端供应商已恢复。下一次口授走云端。",
        )
        .into(),
        (probe::Transition::Online, RoutePreference::CloudOnly) => crate::locale::text(
            "The cloud provider is reachable again.",
            "云端供应商又可以连通了。",
        )
        .into(),
        _ => String::new(),
    }
}

fn publish_route(app: &AppHandle) {
    let snap = app.state::<Dictation>().snapshot(app);
    let _ = app.emit(
        "dictation://route",
        RouteEvent {
            route_label: snap.route_label,
            route_detail: snap.route_detail,
            route_notice: snap.route_notice,
            connectivity: snap.connectivity,
        },
    );
    crate::tray::refresh_menu(app);
}

fn spawn_probe(app: AppHandle) {
    let wake = Arc::clone(&app.state::<Dictation>().probe_wake);
    crate::pipeline::netwatch::spawn(Arc::clone(&wake));
    std::thread::Builder::new()
        .name("newtype-probe".into())
        .spawn(move || {
            let mut debounce = probe::Debounce::new();
            let config = probe::DebounceConfig::standard();
            let mut next = Instant::now();
            let mut seen_epoch = 0u64;
            loop {
                std::thread::sleep(Duration::from_secs(1));
                let state = app.state::<Dictation>();
                let epoch = state.probe_epoch.load(Ordering::Relaxed);
                if epoch != seen_epoch {
                    seen_epoch = epoch;
                    debounce.reset();
                    *state.reachability.lock().expect("reachability") = Reachability::Unknown;
                }
                let poked = state.probe_wake.swap(false, Ordering::Relaxed);
                let due = Instant::now() >= next;
                if !poked && !due {
                    drop(state);
                    continue;
                }
                next = Instant::now() + probe::INTERVAL;
                if egress::process_mode() != NetMode::Open {
                    drop(state);
                    continue;
                }
                let settings = state.settings.lock().expect("settings").clone();
                drop(state);
                let urls = match dictate_config::effective(&settings, hotkey::host_os()) {
                    Ok(effective) => probe::remote_bases(
                        &effective.asr_base,
                        &effective.llm_base,
                        effective.asr_implemented,
                        effective.llm_implemented,
                    ),
                    Err(_) => Vec::new(),
                };
                let sample = if urls.is_empty() {
                    Ok(true)
                } else {
                    probe::ping(&urls)
                };
                let Ok(up) = sample else {
                    continue;
                };
                let before = debounce.verdict();
                let transition = debounce.observe(up, Instant::now(), &config);
                let after = debounce.verdict();
                if before == after && transition == probe::Transition::None {
                    continue;
                }
                {
                    let state = app.state::<Dictation>();
                    *state.reachability.lock().expect("reachability") = after;
                    if transition != probe::Transition::None {
                        let preference = RoutePreference::parse(&settings.preference)
                            .unwrap_or(RoutePreference::CloudDefault);
                        let ready = readiness_of(&state).asr;
                        *state.notice.lock().expect("notice") =
                            notice_for(transition, preference, ready);
                    }
                }
                crate::diag::record(&format!(
                    "route probe {before:?} -> {after:?} transition={transition:?}"
                ));
                publish_route(&app);
            }
        })
        .expect("probe thread");
}
