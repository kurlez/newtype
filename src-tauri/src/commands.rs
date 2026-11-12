use crate::pipeline;
use crate::updater::{self, StateDto};
use serde::Serialize;
use tauri::AppHandle;

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
