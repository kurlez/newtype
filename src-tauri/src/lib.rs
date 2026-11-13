mod build_info;
mod commands;
mod diag;
mod dictate_config;
mod dictation;
mod key_feed;
mod locale;
mod material;
mod overlay;
mod permissions;
mod pipeline;
mod secrets;
mod settings;
mod taskbar_dock;
mod tray;
mod updater;
#[cfg(windows)]
mod win_hotkey;

use tauri::Manager;

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tray::reveal(app);
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            let state = updater::AppState::load(app.handle())?;
            app.manage(state);
            let dictation = dictation::Dictation::load(app.handle())?;
            app.manage(dictation);
            tray::install(app.handle())?;
            dictation::Dictation::launch(app.handle().clone());
            updater::spawn_background_checks(app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                if window.label() == "main" {
                    // Stay in the taskbar and Alt+Tab. Closing minimizes.
                    if window.minimize().is_err() {
                        let _ = window.show();
                    }
                } else {
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_state,
            commands::set_channel,
            commands::check_for_updates,
            commands::apply_update,
            commands::pipeline_status,
            commands::dictation_snapshot,
            commands::dictation_save,
            commands::dictation_set_key,
            commands::dictation_clear_key,
            commands::dictation_test_connection,
            commands::dictation_list_models,
            commands::dictation_request_access,
            commands::dictation_open_privacy,
            commands::dictation_capture_hotkey,
            commands::dictation_cancel_capture,
            commands::dictation_capture_key,
            commands::dictation_set_hotkey,
            commands::dictation_record_toggle,
            commands::dictation_cancel,
            commands::dictation_history,
            commands::dictation_delete_history,
            commands::dictation_copy_history,
            commands::dictation_reinsert_history,
            commands::dictation_clear_history,
            commands::dictation_rerun_history,
            commands::dictation_export_dictionary,
            commands::dictation_import_dictionary,
            commands::dictation_microphones,
            commands::dictation_probe_microphone,
            commands::dictation_reset_models,
            commands::local_download,
            commands::local_cancel_download,
            commands::local_delete,
            commands::local_skip_download,
            commands::local_set_mirror,
            commands::local_use_model,
            commands::indicator_save_position,
            commands::indicator_open_history,
        ])
        .build(tauri::generate_context!())
        .expect("error while building newtype")
        .run(|app, event| {
            if let tauri::RunEvent::Exit = event {
                dictation::shutdown(app);
            }
        });
}
