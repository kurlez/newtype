mod build_info;
mod commands;
mod pipeline;
mod settings;
mod tray;
mod updater;

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
            tray::install(app.handle())?;
            updater::spawn_background_checks(app.handle().clone());
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
                #[cfg(target_os = "macos")]
                let _ = window
                    .app_handle()
                    .set_activation_policy(tauri::ActivationPolicy::Accessory);
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_state,
            commands::set_channel,
            commands::check_for_updates,
            commands::apply_update,
            commands::pipeline_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running newtype");
}
