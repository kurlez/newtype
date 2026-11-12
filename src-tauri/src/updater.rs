use crate::build_info::{commit, full_version};
use crate::settings::{self, Channel, Settings};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons};
use tauri_plugin_updater::UpdaterExt;

const INITIAL_DELAY: Duration = Duration::from_secs(8);
const CHECK_EVERY: Duration = Duration::from_secs(60 * 60);

pub struct AppState {
    path: PathBuf,
    inner: Mutex<Inner>,
    busy: AtomicBool,
}

struct Inner {
    settings: Settings,
    phase: Phase,
    pending: Option<Pending>,
    prompted_for: Option<String>,
}

struct Pending {
    version: String,
    update: tauri_plugin_updater::Update,
    bytes: Vec<u8>,
}

enum Phase {
    Idle,
    Checking,
    UpToDate,
    Downloading { received: u64, total: Option<u64> },
    Ready,
    Error(String),
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StateDto {
    pub version: String,
    pub commit: String,
    pub full_version: String,
    pub channel: String,
    pub endpoint: String,
    pub phase: String,
    pub message: String,
    pub available_version: Option<String>,
    pub received_bytes: Option<u64>,
    pub total_bytes: Option<u64>,
}

impl AppState {
    pub fn load(app: &AppHandle) -> Result<Self, Box<dyn std::error::Error>> {
        let dir = app.path().app_config_dir()?;
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("settings.json");
        let settings = settings::load(&path);
        Ok(Self {
            path,
            inner: Mutex::new(Inner {
                settings,
                phase: Phase::Idle,
                pending: None,
                prompted_for: None,
            }),
            busy: AtomicBool::new(false),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().expect("update state")
    }
}

impl Inner {
    fn dto(&self, version: &str) -> StateDto {
        let (phase, message, available, received, total) = match &self.phase {
            Phase::Idle => (
                "idle",
                "Update checks run at launch and about once an hour.".to_string(),
                None,
                None,
                None,
            ),
            Phase::Checking => (
                "checking",
                format!("Checking the {} channel…", self.settings.channel.as_str()),
                None,
                None,
                None,
            ),
            Phase::UpToDate => (
                "upToDate",
                format!(
                    "This is the latest {} build.",
                    self.settings.channel.as_str()
                ),
                None,
                None,
                None,
            ),
            Phase::Downloading { received, total } => (
                "downloading",
                "Downloading the update in the background…".to_string(),
                self.pending.as_ref().map(|pending| pending.version.clone()),
                Some(*received),
                *total,
            ),
            Phase::Ready => {
                let available = self
                    .pending
                    .as_ref()
                    .map(|pending| pending.version.clone())
                    .unwrap_or_default();
                (
                    "ready",
                    format!("Update {available} is downloaded. Restart to apply it."),
                    Some(available),
                    None,
                    None,
                )
            }
            Phase::Error(message) => ("error", message.clone(), None, None, None),
        };
        StateDto {
            version: version.to_string(),
            commit: commit().to_string(),
            full_version: full_version(version, commit()),
            channel: self.settings.channel.as_str().to_string(),
            endpoint: self.settings.channel.endpoint().to_string(),
            phase: phase.to_string(),
            message,
            available_version: available,
            received_bytes: received,
            total_bytes: total,
        }
    }
}

pub fn snapshot(app: &AppHandle) -> StateDto {
    let version = current_version(app);
    let state = app.state::<AppState>();
    let dto = state.lock().dto(&version);
    dto
}

pub fn spawn_background_checks(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(INITIAL_DELAY).await;
        loop {
            check(&app, false).await;
            tokio::time::sleep(CHECK_EVERY).await;
        }
    });
}

pub async fn set_channel(app: &AppHandle, channel: &str) -> Result<StateDto, String> {
    let parsed = Channel::parse(channel)
        .ok_or_else(|| format!("Unknown channel \"{channel}\". Use dev or stable."))?;
    let state = app.state::<AppState>();
    {
        let mut inner = state.lock();
        inner.settings.channel = parsed;
        inner.pending = None;
        inner.prompted_for = None;
        inner.phase = Phase::Idle;
        settings::save(&state.path, &inner.settings)?;
    }
    publish(app);
    Ok(check(app, true).await)
}

pub async fn check(app: &AppHandle, manual: bool) -> StateDto {
    let state = app.state::<AppState>();
    if state
        .busy
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return snapshot(app);
    }
    let guard = BusyGuard(&state.busy);
    let channel = state.lock().settings.channel;
    {
        let mut inner = state.lock();
        inner.phase = Phase::Checking;
    }
    publish(app);

    let built = app
        .updater_builder()
        .endpoints(vec![endpoint_url(channel)])
        .and_then(|builder| builder.build());
    let updater = match built {
        Ok(updater) => updater,
        Err(err) => {
            fail(app, &friendly(channel, &err.to_string()));
            drop(guard);
            return snapshot(app);
        }
    };
    let update = match updater.check().await {
        Ok(update) => update,
        Err(err) => {
            fail(app, &friendly(channel, &err.to_string()));
            drop(guard);
            return snapshot(app);
        }
    };
    let Some(update) = update else {
        let mut inner = state.lock();
        inner.pending = None;
        inner.phase = Phase::UpToDate;
        drop(inner);
        publish(app);
        drop(guard);
        return snapshot(app);
    };

    let version = update.version.clone();
    let already_downloaded = state
        .lock()
        .pending
        .as_ref()
        .is_some_and(|pending| pending.version == version);
    if already_downloaded {
        state.lock().phase = Phase::Ready;
        let prompt = should_prompt(app, &version, manual);
        publish(app);
        drop(guard);
        if prompt {
            prompt_restart(app, &version);
        }
        return snapshot(app);
    }

    {
        let mut inner = state.lock();
        inner.phase = Phase::Downloading {
            received: 0,
            total: None,
        };
    }
    publish(app);

    let app_for_progress = app.clone();
    let download = update
        .download(
            move |chunk, total| {
                let state = app_for_progress.state::<AppState>();
                if let Phase::Downloading {
                    received,
                    total: slot,
                } = &mut state.lock().phase
                {
                    *received += chunk as u64;
                    *slot = total;
                }
                publish(&app_for_progress);
            },
            || {},
        )
        .await;

    let bytes = match download {
        Ok(bytes) => bytes,
        Err(err) => {
            fail(app, &format!("Could not download update {version}: {err}"));
            drop(guard);
            return snapshot(app);
        }
    };

    let prompt = should_prompt(app, &version, manual);
    {
        let mut inner = state.lock();
        inner.pending = Some(Pending {
            version: version.clone(),
            update,
            bytes,
        });
        inner.phase = Phase::Ready;
    }
    publish(app);
    drop(guard);
    if prompt {
        prompt_restart(app, &version);
    }
    snapshot(app)
}

fn endpoint_url(channel: Channel) -> url::Url {
    channel
        .endpoint()
        .parse()
        .expect("update endpoint is a valid url")
}

fn should_prompt(app: &AppHandle, version: &str, manual: bool) -> bool {
    if manual {
        return true;
    }
    let state = app.state::<AppState>();
    let mut inner = state.lock();
    if inner.prompted_for.as_deref() == Some(version) {
        false
    } else {
        inner.prompted_for = Some(version.to_string());
        true
    }
}

fn prompt_restart(app: &AppHandle, version: &str) {
    let app = app.clone();
    let message = format!("Update {version} is downloaded. Restart newtype to apply it?");
    app.dialog()
        .message(message)
        .title("newtype")
        .buttons(MessageDialogButtons::OkCancelCustom(
            "Restart".into(),
            "Later".into(),
        ))
        .show(move |restart| {
            if restart {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = apply(&app).await;
                });
            }
        });
}

fn fail(app: &AppHandle, message: &str) {
    let state = app.state::<AppState>();
    state.lock().phase = Phase::Error(message.to_string());
    publish(app);
}

fn friendly(channel: Channel, raw: &str) -> String {
    if channel == Channel::Stable
        && (raw.contains("404") || raw.to_ascii_lowercase().contains("not found"))
    {
        format!("{raw}\nNo stable release is published yet. Stay on dev to receive main builds.")
    } else {
        raw.to_string()
    }
}

struct BusyGuard<'a>(&'a AtomicBool);

impl Drop for BusyGuard<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

fn current_version(app: &AppHandle) -> String {
    app.package_info().version.to_string()
}

fn publish(app: &AppHandle) {
    let dto = snapshot(app);
    let _ = app.emit("update://state", dto.clone());
    crate::tray::refresh(app, &dto);
}

pub async fn apply(app: &AppHandle) -> Result<StateDto, String> {
    let pending = {
        let state = app.state::<AppState>();
        let taken = state.lock().pending.take();
        taken
    };
    let Some(pending) = pending else {
        return Err("No downloaded update is waiting.".into());
    };
    let version = pending.version.clone();
    match pending.update.install(&pending.bytes) {
        Ok(()) => {
            // `restart` does not return. On Windows the installer usually
            // exits the process before we get here; if it does not, restart
            // still replaces the running process.
            app.restart();
        }
        Err(err) => {
            let state = app.state::<AppState>();
            let message = format!("Could not apply update {version}: {err}");
            {
                let mut inner = state.lock();
                inner.pending = Some(pending);
                inner.phase = Phase::Error(message.clone());
            }
            publish(app);
            Err(message)
        }
    }
}
