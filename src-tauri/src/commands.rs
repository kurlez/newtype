use crate::pipeline;
use crate::updater::{self, StateDto};
use serde::Serialize;
use tauri::{AppHandle, Manager};

#[derive(Serialize)]
pub struct PipelineModule {
    name: String,
    status: String,
    note: String,
}

#[tauri::command]
pub fn get_state(app: AppHandle) -> StateDto {
    updater::snapshot(&app)
}

#[tauri::command]
pub async fn set_channel(app: AppHandle, channel: String) -> Result<StateDto, String> {
    updater::set_channel(&app, &channel).await
}

#[tauri::command]
pub async fn check_for_updates(app: AppHandle) -> StateDto {
    updater::check(&app, true).await
}

#[tauri::command]
pub async fn apply_update(app: AppHandle) -> Result<StateDto, String> {
    updater::apply(&app).await
}

#[tauri::command]
pub fn dictation_snapshot(app: AppHandle) -> crate::dictation::Snapshot {
    app.state::<crate::dictation::Dictation>().snapshot(&app)
}

#[tauri::command]
pub fn dictation_save(
    app: AppHandle,
    settings: crate::dictate_config::DictationSettings,
) -> Result<crate::dictation::Snapshot, String> {
    crate::dictation::save_settings(&app, settings)
}

#[tauri::command]
pub fn dictation_set_key(app: AppHandle, provider: String, key: String) -> Result<String, String> {
    crate::dictation::set_key(&app, &provider, &key)
}

#[tauri::command]
pub fn dictation_clear_key(app: AppHandle, provider: String) -> Result<(), String> {
    crate::dictation::clear_key(&app, &provider)
}

#[tauri::command]
pub async fn dictation_test_connection(
    app: AppHandle,
    provider: String,
    kind: Option<String>,
) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::dictation::test_connection(&app, &provider, kind.as_deref().unwrap_or("speech"))
    })
        .await
        .map_err(|err| err.to_string())?
}

#[tauri::command]
pub async fn dictation_list_models(
    app: AppHandle,
    provider: String,
) -> Result<Vec<String>, String> {
    tauri::async_runtime::spawn_blocking(move || crate::dictation::list_models(&app, &provider))
        .await
        .map_err(|err| err.to_string())?
}

#[tauri::command]
pub fn dictation_request_access(app: AppHandle, kind: String) -> Result<String, String> {
    match kind.as_str() {
        "accessibility" => {
            let granted = crate::permissions::prompt_accessibility()?;
            Ok(if granted {
                "Accessibility is granted.".into()
            } else {
                "Accessibility is not granted yet. Use the button to open System Settings, enable newtype, then come back.".into()
            })
        }
        "microphone" => crate::dictation::probe_microphone(&app, ""),
        other => Err(format!("Unknown permission \"{other}\".")),
    }
}

#[tauri::command]
pub fn dictation_open_privacy(kind: String) -> Result<(), String> {
    crate::permissions::open_privacy(&kind)
}

#[tauri::command]
pub fn dictation_capture_hotkey(app: AppHandle, mode: String) -> Result<(), String> {
    crate::dictation::begin_capture(&app, &mode)
}

#[tauri::command]
pub fn dictation_cancel_capture(app: AppHandle) {
    crate::dictation::cancel_capture(&app);
}

#[tauri::command]
pub fn dictation_capture_key(app: AppHandle, code: String, down: bool, alt: bool) {
    crate::dictation::capture_key(&app, &code, down, alt);
}

#[tauri::command]
pub fn dictation_set_hotkey(
    app: AppHandle,
    mode: String,
    hotkey: String,
) -> Result<crate::dictation::Snapshot, String> {
    crate::dictation::set_hotkey(&app, &mode, &hotkey)
}

#[tauri::command]
pub fn dictation_record_toggle(app: AppHandle) {
    crate::dictation::toggle(&app);
}

#[tauri::command]
pub fn dictation_cancel(app: AppHandle) {
    crate::dictation::cancel(&app);
}

#[tauri::command]
pub fn dictation_history(app: AppHandle) -> Vec<crate::pipeline::history::HistoryEntry> {
    crate::dictation::history(&app)
}

#[tauri::command]
pub fn dictation_delete_history(app: AppHandle, id: String) -> Result<(), String> {
    crate::dictation::delete_history(&app, &id)
}

#[tauri::command]
pub fn dictation_copy_history(app: AppHandle, id: String) -> Result<(), String> {
    crate::dictation::copy_history(&app, &id)
}

#[tauri::command]
pub fn dictation_reinsert_history(app: AppHandle, id: String) -> Result<(), String> {
    crate::dictation::reinsert_history(&app, &id)
}

#[tauri::command]
pub fn dictation_clear_history(app: AppHandle) -> Result<(), String> {
    crate::dictation::clear_history(&app)
}

#[tauri::command]
pub async fn dictation_rerun_history(app: AppHandle, id: String) -> Result<String, String> {
    tauri::async_runtime::spawn_blocking(move || crate::dictation::rerun_history(&app, &id))
        .await
        .map_err(|err| err.to_string())?
}

#[tauri::command]
pub fn dictation_export_dictionary(app: AppHandle) -> Result<String, String> {
    crate::dictation::export_dictionary(&app)
}

#[tauri::command]
pub fn dictation_import_dictionary(
    app: AppHandle,
    body: String,
) -> Result<crate::dictation::Snapshot, String> {
    crate::dictation::import_dictionary(&app, &body)
}

#[tauri::command]
pub fn dictation_microphones() -> Result<Vec<String>, String> {
    crate::dictation::microphones()
}

#[tauri::command]
pub fn dictation_probe_microphone(app: AppHandle, device: String) -> Result<String, String> {
    crate::dictation::probe_microphone(&app, &device)
}

#[tauri::command]
pub fn dictation_reset_models(app: AppHandle) -> Result<crate::dictation::Snapshot, String> {
    let mut settings = app
        .state::<crate::dictation::Dictation>()
        .snapshot(&app)
        .settings;
    crate::dictate_config::reset_models(&mut settings);
    crate::dictation::save_settings(&app, settings)
}

#[tauri::command]
pub fn local_download(app: AppHandle, ids: Vec<String>, confirm_network: bool) -> Result<(), String> {
    crate::dictation::local_download(&app, ids, confirm_network)
}

#[tauri::command]
pub fn local_cancel_download(app: AppHandle) {
    crate::dictation::local_cancel_download(&app);
}

#[tauri::command]
pub fn local_delete(app: AppHandle, id: String) -> Result<crate::dictation::Snapshot, String> {
    crate::dictation::local_delete(&app, &id)
}

#[tauri::command]
pub fn local_skip_download(app: AppHandle) -> Result<crate::dictation::Snapshot, String> {
    crate::dictation::local_skip_download(&app)
}

#[tauri::command]
pub fn local_set_mirror(app: AppHandle, mirror: String) -> Result<crate::dictation::Snapshot, String> {
    crate::dictation::local_set_mirror(&app, &mirror)
}

#[tauri::command]
pub fn local_use_model(
    app: AppHandle,
    role: String,
    id: String,
) -> Result<crate::dictation::Snapshot, String> {
    crate::dictation::local_use_model(&app, &role, &id)
}

#[tauri::command]
pub fn pipeline_status() -> Vec<PipelineModule> {
    pipeline::inventory()
        .into_iter()
        .map(|module| PipelineModule {
            name: module.name.to_string(),
            status: module.status.to_string(),
            note: module.note.to_string(),
        })
        .collect()
}
