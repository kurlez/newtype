use crate::updater::{self, StateDto};
use std::sync::Mutex;
use tauri::image::Image;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::{AppHandle, Emitter, Manager};

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct NavigateEvent {
    tab: String,
    last: bool,
}

static LAST_MENU: Mutex<String> = Mutex::new(String::new());

pub fn install(app: &AppHandle) -> tauri::Result<()> {
    let menu = menu(app, &updater::snapshot(app))?;
    let mut builder = tauri::tray::TrayIconBuilder::with_id("main")
        .tooltip("newtype")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| {
            crate::dictation::remember_focus(app);
            match event.id().as_ref() {
            "history" => reveal_history(app, false),
            "last" => reveal_history(app, true),
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
            "dictate" => crate::dictation::toggle(app),
            _ => {}
        }
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
        "{}|{}|{}|{}|{}|{}",
        state.phase,
        state.full_version,
        state.available_version.as_deref().unwrap_or(""),
        crate::dictation::route_label(app),
        crate::dictation::fallback_line(app),
        crate::dictation::listener_line(app)
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

pub fn refresh_menu(app: &AppHandle) {
    refresh(app, &updater::snapshot(app));
}

pub fn reveal(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    if !crate::dictation::is_busy(app) {
        let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
    }
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        if crate::dictation::is_busy(app) {
            crate::overlay::show_without_activating(&window);
        } else {
            let _ = window.show();
            let _ = window.set_focus();
        }
    }
}

fn reveal_history(app: &AppHandle, last: bool) {
    reveal(app);
    let _ = app.emit(
        "ui://navigate",
        NavigateEvent {
            tab: "history".into(),
            last,
        },
    );
}

fn menu(app: &AppHandle, state: &StateDto) -> tauri::Result<Menu<tauri::Wry>> {
    let history = MenuItem::with_id(app, "history", crate::locale::text("History", "历史"), true, None::<&str>)?;
    let last = MenuItem::with_id(
        app,
        "last",
        crate::locale::text("Show last result", "显示上一条结果"),
        true,
        None::<&str>,
    )?;
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
    let route = MenuItem::with_id(
        app,
        "route",
        format!("Path: {}", crate::dictation::route_label(app)),
        false,
        None::<&str>,
    )?;
    let dictate = MenuItem::with_id(app, "dictate", "Start or stop dictation", true, None::<&str>)?;
    let fallback = MenuItem::with_id(
        app,
        "fallback",
        crate::dictation::fallback_line(app),
        false,
        None::<&str>,
    )?;
    let listener = MenuItem::with_id(
        app,
        "listener",
        format!("Hotkeys: {}", crate::dictation::listener_line(app)),
        false,
        None::<&str>,
    )?;
    let log = MenuItem::with_id(
        app,
        "log",
        format!("Log: {}", crate::dictation::log_line()),
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
                &history, &last, &open, &version, &commit, &route, &fallback, &listener, &log, &dictate, &sep_a, &check, &apply, &sep_b, &quit,
            ],
        )
    } else {
        Menu::with_items(
            app,
            &[&history, &last, &open, &version, &commit, &route, &fallback, &listener, &log, &dictate, &sep_a, &check, &sep_b, &quit],
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
