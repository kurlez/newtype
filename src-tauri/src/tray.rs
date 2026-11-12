use crate::updater::{self, StateDto};
use std::sync::Mutex;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::{AppHandle, Manager};

static LAST_MENU: Mutex<String> = Mutex::new(String::new());

pub fn install(app: &AppHandle) -> tauri::Result<()> {
    let menu = menu(app, &updater::snapshot(app))?;
    let mut builder = tauri::tray::TrayIconBuilder::with_id("main")
        .tooltip("newtype")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => reveal(app),
            "check" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    updater::check(&app, true).await;
                });
            }
            "apply" => {
                let app = app.clone();
                tauri::async_runtime::spawn(async move {
                    let _ = updater::apply(&app).await;
                });
            }
            "quit" => app.exit(0),
            _ => {}
        });
    builder = builder.icon(tray_icon());
    #[cfg(target_os = "macos")]
    {
        builder = builder.icon_as_template(true);
    }
    builder.build(app)?;
    Ok(())
}

pub fn refresh(app: &AppHandle, state: &StateDto) {
    let key = format!(
        "{}|{}|{}",
        state.phase,
        state.full_version,
        state.available_version.as_deref().unwrap_or("")
    );
    let mut last = LAST_MENU.lock().expect("tray menu key");
    if *last == key {
        return;
    }
    *last = key;
    drop(last);
    let Some(tray) = app.tray_by_id("main") else {
        return;
    };
    if let Ok(menu) = menu(app, state) {
        let _ = tray.set_menu(Some(menu));
        let _ = tray.set_tooltip(Some(format!("newtype {}", state.full_version)));
    }
}

pub fn reveal(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn menu(app: &AppHandle, state: &StateDto) -> tauri::Result<Menu<tauri::Wry>> {
    let open = MenuItem::with_id(app, "open", "Open newtype", true, None::<&str>)?;
    let version = MenuItem::with_id(
        app,
        "version",
        state.full_version.as_str(),
        false,
        None::<&str>,
    )?;
    let commit = MenuItem::with_id(
        app,
        "commit",
        format!("commit {}", state.commit),
        false,
        None::<&str>,
    )?;
    let check = MenuItem::with_id(app, "check", "Check for updates", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let sep_a = PredefinedMenuItem::separator(app)?;
    let sep_b = PredefinedMenuItem::separator(app)?;
    if state.phase == "ready" {
        let label = match &state.available_version {
            Some(version) => format!("Restart to apply {version}"),
            None => "Restart to apply update".to_string(),
        };
        let apply = MenuItem::with_id(app, "apply", label, true, None::<&str>)?;
        Menu::with_items(
            app,
            &[
                &open, &version, &commit, &sep_a, &check, &apply, &sep_b, &quit,
            ],
        )
    } else {
        Menu::with_items(
            app,
            &[&open, &version, &commit, &sep_a, &check, &sep_b, &quit],
        )
    }
}

fn tray_icon() -> Image<'static> {
    let bytes: &[u8] = if cfg!(target_os = "macos") {
        include_bytes!("../icons/tray-template.png")
    } else {
        include_bytes!("../icons/32x32.png")
    };
    Image::from_bytes(bytes).expect("tray icon png")
}
