//! Dictation session: hotkey, microphone, cloud speech, polish, paste, history.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use std::sync::mpsc::{self, Receiver, Sender};

use crate::build_info::{commit, full_version};
use crate::dictate_config::{self, DictationSettings};
use crate::permissions::{self, PermissionSnapshot};
use crate::pipeline::asr::AsrProvider;
use crate::pipeline::audio::{self, AudioClip, MicSession};
use crate::pipeline::catalog::{self, Mirror};
use crate::pipeline::cloud::{self, CloudAsr, CloudLlm, Preset};
use crate::pipeline::engines::Engines;
use crate::pipeline::library::{self, LibraryView};
use crate::pipeline::local_run;
use crate::pipeline::front;
use crate::pipeline::history::{self, HistoryEntry};
use crate::pipeline::hotkey::{
    self, EngineEvent, HotkeyEngine, HotkeyMode, KeyEdge, TalkController, TalkEffect, TalkMode,
};
use crate::pipeline::insert;
use crate::pipeline::polish::{self, PolishLevel};
use crate::pipeline::router::{self, Block, Reachability, RouteInput, Slot};
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
    control: Sender<Control>,
    inbox: Mutex<Option<Receiver<Control>>>,
    cancel: Arc<AtomicBool>,
    generation: Arc<AtomicU64>,
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
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct CaptureEvent {
    mode: String,
    hotkey: String,
    label: String,
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
pub struct Snapshot {
    pub settings: DictationSettings,
    pub hotkey_labels: Vec<String>,
    pub hotkey_chords: Vec<String>,
    pub warnings: Vec<String>,
    pub verified_on: String,
    pub keys: std::collections::BTreeMap<String, bool>,
    pub presets: Vec<PresetDto>,
    pub permissions: PermissionSnapshot,
    pub listener: String,
    pub secure_store: String,
    pub log_path: String,
    pub route_label: String,
    pub route_detail: String,
    pub phase: String,
    pub version: String,
    pub dev_key_store: bool,
    pub local: LibraryView,
}

impl Dictation {
    pub fn load(app: &AppHandle) -> Result<Self, String> {
        let config_dir = app
            .path()
            .app_config_dir()
            .map_err(|err| err.to_string())?;
        let data_dir = app.path().app_data_dir().map_err(|err| err.to_string())?;
        std::fs::create_dir_all(&config_dir).map_err(|err| err.to_string())?;
        std::fs::create_dir_all(&data_dir).map_err(|err| err.to_string())?;
        let log_dir = app.path().app_log_dir().unwrap_or_else(|_| config_dir.clone());
        crate::diag::init(&log_dir);
        let secure_store = match secrets::probe(&config_dir.join("dev-secrets.json")) {
            Ok(()) => {
                crate::diag::record(&format!("credential store ready ({})", secrets::store_name()));
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
        let effective = dictate_config::effective(&settings, hotkey::host_os())
            .unwrap_or_else(|_| dictate_config::effective(&DictationSettings::default(), hotkey::host_os()).expect("defaults"));
        let (control, inbox) = mpsc::channel();
        Ok(Self {
            settings_path,
            history_path: history_path.clone(),
            dev_secrets,
            engines,
            settings: Mutex::new(settings),
            history: Mutex::new(history::load(&history_path)),
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
            control,
            inbox: Mutex::new(Some(inbox)),
            cancel: Arc::new(AtomicBool::new(false)),
            generation: Arc::new(AtomicU64::new(1)),
        })
    }

    pub fn launch(app: AppHandle) {
        open_indicator(&app);
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
        let (labels, chords, warnings, route_label, route_detail) = match &effective {
            Ok(effective) => {
                let ready = readiness_of(self);
                let decision = route_for(effective, ready);
                let detail = route_detail(decision, ready);
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
                "Cloud · 云端".into(),
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
        Snapshot {
            settings,
            hotkey_labels: labels,
            hotkey_chords: chords,
            warnings,
            verified_on: cloud::VERIFIED_ON.into(),
            keys,
            presets: cloud::presets().iter().map(preset_dto).collect(),
            permissions: permissions::snapshot(&self.mic_status.lock().expect("mic")),
            listener: self.listener.lock().expect("listener").clone(),
            secure_store: self.secure_store.lock().expect("store").clone(),
            log_path: self.log_path.clone(),
            route_label,
            route_detail,
            phase: self.phase.lock().expect("phase").clone(),
            version: version_of(app),
            dev_key_store: cfg!(not(any(target_os = "macos", target_os = "windows"))),
            local: self.library_view(),
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
                let decision = route_for(&effective, ready);
                (
                    decision.label.text().to_string(),
                    route_detail(decision, ready),
                )
            }
            Err(err) => ("Cloud · 云端".into(), err),
        }
    }
}

pub fn toggle(app: &AppHandle) {
    let _ = app.state::<Dictation>().control.send(Control::Toggle);
}

pub fn cancel(app: &AppHandle) {
    app.state::<Dictation>().cancel.store(true, Ordering::Relaxed);
    let _ = app.state::<Dictation>().control.send(Control::Cancel);
}

pub fn route_label(app: &AppHandle) -> String {
    app.state::<Dictation>().route_label()
}

fn preset_dto(preset: &Preset) -> PresetDto {
    PresetDto {
        id: preset.id.into(),
        label: preset.label.into(),
        asr_ready: preset.asr_ready,
        llm_ready: preset.llm_ready,
        base_editable: preset.base_editable,
        default_base: preset.default_base.into(),
        default_asr: preset.default_asr.into(),
        default_llm: preset.default_llm.into(),
        asr_models: preset.asr_models.iter().map(|id| (*id).to_string()).collect(),
        llm_models: preset.llm_models.iter().map(|id| (*id).to_string()).collect(),
        asr_wire: preset.asr_wire.as_str().into(),
        note: preset.note.into(),
        verified: preset.verified.into(),
    }
}

fn route_for(effective: &dictate_config::Effective, ready: library::Readiness) -> router::RouteDecision {
    router::decide(RouteInput {
        preference: effective.preference,
        // M3: replace Unknown with a debounced endpoint probe. Unknown keeps
        // cloud-default on the cloud path even when local models are installed.
        reachability: Reachability::Unknown,
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

fn route_detail(decision: router::RouteDecision, ready: library::Readiness) -> String {
    if let Some(message) = router::block_message(decision.block) {
        return message.to_string();
    }
    match decision.asr {
        Slot::Local if decision.llm == Slot::None => {
            "Audio stays on this computer. The rewrite model is not installed, so dictation inserts the ruled transcript.".into()
        }
        Slot::Local => "Audio stays on this computer. Speech and rewrite use the installed local models.".into(),
        Slot::Cloud if !ready.fallback_ready() => {
            "Speech goes to the provider you configured. Offline fallback is not ready until both local models are installed.".into()
        }
        _ => "Speech goes to the provider you configured. The path name stays visible.".into(),
    }
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
            let (mut seen, mut capturing) = {
                let share = hotkeys.lock().expect("hotkeys");
                engine.set_bindings(share.bindings.clone());
                engine.set_capture(share.capture);
                (share.generation, share.capture)
            };
            let mut on_edge = move |edge: KeyEdge| {
                {
                    let share = hotkeys.lock().expect("hotkeys");
                    if share.generation != seen {
                        seen = share.generation;
                        engine.set_bindings(share.bindings.clone());
                        if share.capture != capturing {
                            capturing = share.capture;
                            engine.set_capture(capturing);
                        }
                    }
                }
                for engine_event in engine.on_edge(edge) {
                    crate::diag::record(&format!("hotkey {engine_event:?}"));
                    let _ = control.send(Control::Engine(engine_event));
                }
            };
            set_listener(&app, &listener, "Listening for the right-hand Control key.");
            crate::diag::record("hotkey listener starting");
            #[cfg(windows)]
            {
                if let Err(err) = crate::win_hotkey::run(on_edge) {
                    let message = format!("Hotkey listener is not running. {err}");
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
                        on_edge(edge);
                    }
                });
                if let Err(err) = listen_result {
                    let message = format!(
                        "Hotkey listener is not running ({err:?}). On macOS, grant Accessibility and, if the hotkey still does nothing, Input Monitoring, then restart newtype."
                    );
                    set_listener(&app, &listener, &message);
                    crate::diag::record(&message);
                } else {
                    let message = "Hotkey listener stopped.";
                    set_listener(&app, &listener, message);
                    crate::diag::record(message);
                }
            }
        })
        .expect("hotkey thread");
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
    let mut route = String::from("Cloud · 云端");
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
                )
                ;
            }
            Control::Toggle => {
                if busy {
                    emit_phase(&app, "processing", &route, "Still working on the last dictation.", 0, 0.0, "");
                } else if mic.is_some() {
                    talk.active = None;
                    finish_recording(&app, &mut mic, &mut started, &mut busy, &route, false);
                } else {
                    talk.active = Some(HotkeyMode::Dictate);
                    talk.started_ms = now_ms();
                    begin_recording(&app, &control, &mut talk, &mut mic, &mut route, &mut started);
                }
            }
            Control::Cancel => {
                app.state::<Dictation>().cancel.store(true, Ordering::Relaxed);
                talk.active = None;
                if mic.take().is_some() {
                    started = None;
                    bump(&app);
                    emit_phase(&app, "cancelled", &route, "Cancelled. Nothing was inserted.", 0, 0.0, "");
                    schedule_hide(&app, 900);
                } else if !busy {
                    app.state::<Dictation>().cancel.store(false, Ordering::Relaxed);
                    emit_phase(&app, "idle", &route, "", 0, 0.0, "");
                    hide_indicator(&app);
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
    let settings = app.state::<Dictation>().settings.lock().expect("settings").clone();
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
        TalkEffect::StartDictate => {
            begin_recording(app, control, talk, mic, route, started);
        }
        TalkEffect::Stop => {
            finish_recording(app, mic, started, busy, route, false);
        }
        TalkEffect::Cancel => {
            mic.take();
            *started = None;
            bump(app);
            emit_phase(app, "cancelled", route, "Cancelled. Nothing was inserted.", 0, 0.0, "");
            schedule_hide(app, 900);
        }
        TalkEffect::Stub(mode) | TalkEffect::CancelThenStub(mode) => {
            if matches!(effect, TalkEffect::CancelThenStub(_)) {
                mic.take();
                *started = None;
                bump(app);
            }
            let message = match mode {
                HotkeyMode::Translate => "Translate is registered and not implemented in this build.",
                HotkeyMode::Command => "Command mode is registered and not implemented in this build.",
                HotkeyMode::Dictate => "Dictation is ready.",
            };
            emit_phase(app, "stub", route, message, 0, 0.0, "");
            schedule_hide(app, 1800);
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
) {
    if mic.is_some() {
        return;
    }
    let state = app.state::<Dictation>();
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
    let decision = route_for(&effective, ready);
    *route = decision.label.text().to_string();
    if decision.block != Block::None || decision.asr == Slot::None {
        talk.active = None;
        let message = router::block_message(decision.block).unwrap_or("Dictation cannot start.");
        emit_phase(app, "error", route, message, 0, 0.0, "");
        show_indicator(app);
        schedule_hide(app, 3600);
        return;
    }
    let on_device = decision.asr == Slot::Local;
    if !on_device {
        if effective.asr_base.is_empty() || effective.asr_model.is_empty() {
            talk.active = None;
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
        if !secrets::has_key(&effective.asr_provider, &state.dev_secrets) {
            talk.active = None;
            let store = state.secure_store.lock().expect("store").clone();
            let message = if store == "ready" {
                "Save an API key for the speech provider. Audio is not sent until you do.".to_string()
            } else {
                store
            };
            emit_phase(app, "error", route, &message, 0, 0.0, "");
            schedule_hide(app, 5000);
            return;
        }
    }
    let recording_line = if on_device {
        "Recording. Audio stays on this computer."
    } else {
        "Recording. Audio will be sent to your speech provider when you stop."
    };
    match audio::MicSession::start(&effective.microphone) {
        Ok(session) => {
            *state.mic_status.lock().expect("mic") = "granted".into();
            let handle = session.watch();
            let started_at = Instant::now();
            *mic = Some(session);
            *started = Some(started_at);
            let generation = bump(app);
            emit_phase(app, "recording", route, recording_line, 0, 0.0, "");
            show_indicator(app);
            let app_loop = app.clone();
            let control = control.clone();
            let route_loop = route.clone();
            let recording_loop = recording_line.to_string();
            tauri::async_runtime::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    if app_loop.state::<Dictation>().generation.load(Ordering::Relaxed) != generation {
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
            *state.mic_status.lock().expect("mic") = format!("unavailable: {err}");
            emit_phase(app, "error", route, &format!("Microphone: {err}"), 0, 0.0, "");
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
    emit_phase(app, "processing", route, "Transcribing…", 0, 0.0, "");
    show_indicator(app);
    let clip = session.stop();
    if audio::is_silent(&clip.samples) {
        *busy = false;
        emit_phase(app, "silence", route, "No speech detected. Nothing was inserted.", 0, 0.0, "");
        schedule_hide(app, 1600);
        return;
    }
    let outcome = transcribe_and_polish(app, clip, route.to_string());
    if app.state::<Dictation>().cancel.swap(false, Ordering::Relaxed) {
        *busy = false;
        emit_phase(app, "cancelled", route, "Cancelled. Nothing was inserted.", 0, 0.0, "");
        schedule_hide(app, 1200);
        return;
    }
    match outcome {
        Ok(done) => {
            let mut message = if done.used_fallback {
                format!("Inserted the ruled transcript. {}", done.reason)
            } else {
                "Inserted.".into()
            };
            if !done.text.is_empty() {
                if let Err(err) = insert::insert_text(&done.text) {
                    message = format!("{message} Could not paste: {err} Copy it from history.");
                }
            }
            let phase = if done.used_fallback { "fallback" } else { "inserted" };
            emit_phase(app, phase, route, &message, 0, 0.0, &done.text);
            schedule_hide(app, 1800);
        }
        Err(err) => {
            emit_phase(app, "error", route, &err, 0, 0.0, "");
            schedule_hide(app, 4200);
        }
    }
    *busy = false;
}

struct Done {
    text: String,
    used_fallback: bool,
    reason: String,
}

fn transcribe_and_polish(app: &AppHandle, clip: AudioClip, route: String) -> Result<Done, String> {
    let state = app.state::<Dictation>();
    let settings = state.settings.lock().expect("settings").clone();
    let effective = dictate_config::effective(&settings, hotkey::host_os())?;
    let ready = readiness_of(&state);
    let decision = route_for(&effective, ready);
    if decision.asr == Slot::Local {
        return transcribe_local(app, clip, &route, &effective, &settings);
    }
    if local_run::refuses_network(effective.preference) || decision.asr != Slot::Cloud {
        let message = router::block_message(decision.block)
            .unwrap_or("Audio was not sent.")
            .to_string();
        remember(app, &effective, &route, "local", "", "", "", true, &message);
        return Err(message);
    }
    // M3 can retry this recording on the local engines after a cloud failure.
    // `retry_same_audio_on_cloud_failure` stays false until that probe exists.
    let _retry_on_local = false;
    let key = secrets::get_key(&effective.asr_provider, &state.dev_secrets)?
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "Speech API key is missing.".to_string())?;
    let wav = audio::encode_wav_16k(&clip);
    let asr = CloudAsr {
        base: effective.asr_base.clone(),
        key,
        model: effective.asr_model.clone(),
        wire: effective.asr_wire,
    };
    let transcript = match asr.transcribe(&wav) {
        Ok(text) => text,
        Err(err) => {
            remember(
                app,
                &effective,
                &route,
                &format!("{} / {}", effective.asr_provider, effective.llm_provider),
                "",
                "",
                "",
                true,
                &err,
            );
            return Err(err.to_string());
        }
    };
    let app_context = front::front_app_name();
    let llm_key = secrets::get_key(&effective.llm_provider, &state.dev_secrets)
        .ok()
        .flatten()
        .filter(|value| !value.is_empty());
    let finished = if !effective.llm_implemented || llm_key.is_none() {
        let ruled = crate::pipeline::hotwords::apply(&transcript, &effective.hotwords);
        polish::FinishedText {
            raw: transcript,
            ruled: ruled.clone(),
            text: ruled,
            used_fallback: true,
            reason: "No rewrite model is configured, so the ruled transcript was inserted.".into(),
        }
    } else {
        let llm = CloudLlm {
            base: effective.llm_base.clone(),
            key: llm_key.unwrap_or_default(),
            model: effective.llm_model.clone(),
            thinking: effective.thinking,
        };
        polish::finish_transcript(
            &transcript,
            &effective.hotwords,
            effective.polish,
            &app_context,
            &llm,
        )
    };
    remember(
        app,
        &effective,
        &route,
        &format!("{} / {}", effective.asr_provider, effective.llm_provider),
        &finished.raw,
        &finished.ruled,
        &finished.text,
        finished.used_fallback,
        &finished.reason,
    );
    Ok(Done {
        text: finished.text,
        used_fallback: finished.used_fallback,
        reason: finished.reason,
    })
}

fn transcribe_local(
    app: &AppHandle,
    clip: AudioClip,
    route: &str,
    effective: &dictate_config::Effective,
    settings: &crate::dictate_config::DictationSettings,
) -> Result<Done, String> {
    let state = app.state::<Dictation>();
    let hardware = catalog::host_hardware();
    let asr_id = catalog::active_id(catalog::Role::Asr, &settings.local_asr_id, hardware);
    let llm_id = catalog::active_id(catalog::Role::Llm, &settings.local_llm_id, hardware);
    let llm_ready = library::engine_ready(state.engines.root(), &llm_id, hardware.os);
    let engines = Arc::clone(&state.engines);
    drop(state);
    let report = |message: &str| emit_phase(app, "processing", route, message, 0, 0.0, "");
    match local_run::run(
        &engines,
        hardware.os,
        clip,
        &asr_id,
        &llm_id,
        llm_ready,
        effective.polish,
        &effective.hotwords,
        &front::front_app_name(),
        &report,
    ) {
        Ok(outcome) => {
            let mut reason = outcome.finished.reason.clone();
            if !outcome.note.is_empty() {
                if !reason.is_empty() {
                    reason.push(' ');
                }
                reason.push_str(&outcome.note);
            }
            let provider = format!("local / {}", outcome.engine);
            remember(
                app,
                effective,
                route,
                &provider,
                &outcome.finished.raw,
                &outcome.finished.ruled,
                &outcome.finished.text,
                outcome.finished.used_fallback,
                &reason,
            );
            Ok(Done {
                text: outcome.finished.text,
                used_fallback: outcome.finished.used_fallback,
                reason,
            })
        }
        Err(err) => {
            remember(
                app,
                effective,
                route,
                "local",
                "",
                "",
                "",
                true,
                &err,
            );
            Err(err)
        }
    }
}

fn remember(
    app: &AppHandle,
    effective: &dictate_config::Effective,
    route: &str,
    provider: &str,
    raw: &str,
    ruled: &str,
    text: &str,
    used_fallback: bool,
    note: &str,
) {
    let state = app.state::<Dictation>();
    let entry = HistoryEntry {
        id: format!("{}-{}", now_ms(), state.generation.load(Ordering::Relaxed)),
        created_at_ms: now_ms(),
        app_version: version_of(app),
        mode: "dictate".into(),
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
    };
    let mut history = state.history.lock().expect("history");
    history::push(&mut history, entry);
    let _ = history::save(&state.history_path, &history);
}

fn bump(app: &AppHandle) -> u64 {
    app.state::<Dictation>()
        .generation
        .fetch_add(1, Ordering::Relaxed)
        + 1
}

fn emit_phase(app: &AppHandle, phase: &str, route: &str, message: &str, elapsed_ms: u64, level: f32, text: &str) {
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
        },
    );
    if phase == "idle" {
        hide_indicator(app);
    } else {
        show_indicator(app);
    }
    if phase != "recording" {
        crate::tray::refresh_menu(app);
    }
}

fn show_indicator(app: &AppHandle) {
    if app.get_webview_window("indicator").is_none() {
        open_indicator(app);
        crate::diag::record("indicator window was missing and was created again");
    }
    if let Some(window) = app.get_webview_window("indicator") {
        if window.is_visible().unwrap_or(false) {
            return;
        }
        place(&window);
        if window.show().is_err() {
            crate::diag::record("indicator window could not be shown");
        }
    } else {
        crate::diag::record("indicator window could not be created");
    }
}

fn hide_indicator(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("indicator") {
        let _ = window.hide();
    }
}

fn schedule_hide(app: &AppHandle, delay_ms: u64) {
    let app = app.clone();
    let generation = app.state::<Dictation>().generation.load(Ordering::Relaxed);
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
        if app.state::<Dictation>().generation.load(Ordering::Relaxed) == generation {
            hide_indicator(&app);
            if let Ok(mut phase) = app.state::<Dictation>().phase.lock() {
                *phase = "idle".into();
            }
        }
    });
}

fn place(window: &tauri::WebviewWindow) {
    let Ok(Some(monitor)) = window.current_monitor() else {
        return;
    };
    let size = monitor.size();
    let scale = monitor.scale_factor();
    let width = 440.0 * scale;
    let height = 84.0 * scale;
    let _ = window.set_size(tauri::Size::Physical(tauri::PhysicalSize {
        width: width as u32,
        height: height as u32,
    }));
    let x = (size.width as f64 - width) / 2.0;
    let y = size.height as f64 - height - 64.0 * scale;
    let _ = window.set_position(tauri::Position::Physical(tauri::PhysicalPosition {
        x: x as i32,
        y: y as i32,
    }));
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
    .inner_size(440.0, 84.0)
    .decorations(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .focused(false)
    .visible(false)
    .resizable(false)
    .shadow(true)
    .initialization_script("window.__newtypeIndicator = true;")
    .build();
    if let Ok(window) = built {
        let _ = window.set_focusable(false);
    }
}

pub fn save_settings(app: &AppHandle, mut settings: DictationSettings) -> Result<Snapshot, String> {
    dictate_config::normalize(&mut settings);
    let warnings = dictate_config::validate(&settings, hotkey::host_os())?;
    let effective = dictate_config::effective(&settings, hotkey::host_os())?;
    let state = app.state::<Dictation>();
    dictate_config::save(&state.settings_path, &settings)?;
    {
        let mut share = state.hotkeys.lock().expect("hotkeys");
        share.bindings = effective.bindings;
        share.generation = share.generation.wrapping_add(1);
    }
    *state.settings.lock().expect("settings") = settings;
    let _ = warnings;
    crate::tray::refresh_menu(app);
    Ok(state.snapshot(app))
}

pub fn shutdown(app: &AppHandle) {
    if let Some(state) = app.try_state::<Dictation>() {
        state.engines.shutdown();
    }
}

pub fn fallback_line(app: &AppHandle) -> String {
    let state = app.state::<Dictation>();
    if readiness_of(&state).fallback_ready() {
        "Local fallback: ready".into()
    } else {
        "Local fallback: not ready".into()
    }
}

pub fn local_download(app: &AppHandle, ids: Vec<String>, confirm_network: bool) -> Result<(), String> {
    let state = app.state::<Dictation>();
    let settings = state.settings.lock().expect("settings").clone();
    if settings.preference == "strictOffline" && !confirm_network {
        return Err("Strict offline is on. Downloading contacts the model host you picked. Allow it once, or leave strict offline.".into());
    }
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

pub fn test_connection(app: &AppHandle, provider: &str) -> Result<String, String> {
    let state = app.state::<Dictation>();
    let settings = state.settings.lock().expect("settings").clone();
    let effective = dictate_config::effective(&settings, hotkey::host_os())?;
    let (base, model_ready) = if provider == effective.llm_provider {
        (effective.llm_base, effective.llm_implemented || provider == "volcengine")
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
        return Err("Unknown hotkey.".into());
    }
    let state = app.state::<Dictation>();
    let mut share = state.hotkeys.lock().expect("hotkeys");
    share.capture = true;
    share.capture_mode = mode.to_string();
    share.generation = share.generation.wrapping_add(1);
    Ok(())
}

pub fn cancel_capture(app: &AppHandle) {
    let state = app.state::<Dictation>();
    let mut share = state.hotkeys.lock().expect("hotkeys");
    share.capture = false;
    share.generation = share.generation.wrapping_add(1);
}

pub fn history(app: &AppHandle) -> Vec<HistoryEntry> {
    app.state::<Dictation>().history.lock().expect("history").clone()
}

pub fn delete_history(app: &AppHandle, id: &str) -> Result<(), String> {
    let state = app.state::<Dictation>();
    let mut history = state.history.lock().expect("history");
    history.retain(|entry| entry.id != id);
    history::save(&state.history_path, &history)
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
