use crate::{
    state::{self, AppState},
    update,
};
use std::sync::Mutex;
#[cfg(not(target_os = "macos"))]
use tauri::tray::{MouseButton, MouseButtonState, TrayIconEvent};
use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    webview::PageLoadEvent,
    AppHandle, Manager, WebviewWindowBuilder, Wry,
};

/// The tray menu and its "update to …" line, there only while an update waits.
static MENU: Mutex<Option<Menu<Wry>>> = Mutex::new(None);
static UPDATE_ITEM: Mutex<Option<MenuItem<Wry>>> = Mutex::new(None);

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let paused = app.state::<AppState>().settings().paused;
    let open = MenuItem::with_id(app, "open", "Настройки", true, None::<&str>)?;
    let pause = CheckMenuItem::with_id(app, "pause", "Пауза", true, paused, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Выйти", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&open, &pause, &separator, &quit])?;
    *app.state::<AppState>().pause_item.lock().unwrap() = Some(pause.clone());
    *MENU.lock().unwrap() = Some(menu.clone());

    let builder =
        TrayIconBuilder::with_id("main").tooltip("Airtype").menu(&menu).on_menu_event(move |app, event| {
            match event.id().as_ref() {
                "open" => show_settings(app),
                "pause" => state::set_paused(app, pause.is_checked().unwrap_or(false)),
                // The settings show how the download goes.
                "update" => {
                    show_settings(app);
                    update::install(app);
                }
                "quit" => app.exit(0),
                _ => {}
            }
        });

    // macOS: monochrome template icon and the menu on left click, like other menu bar apps.
    #[cfg(target_os = "macos")]
    let builder = builder
        .icon(tauri::include_image!("icons/tray-template.png"))
        .icon_as_template(true)
        .show_menu_on_left_click(true);

    // Windows: the colour icon; left click opens settings, right click the menu.
    #[cfg(not(target_os = "macos"))]
    let builder = {
        let builder = builder.show_menu_on_left_click(false).on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_settings(tray.app_handle());
            }
        });
        match app.default_window_icon() {
            Some(icon) => builder.icon(icon.clone()),
            None => builder,
        }
    };

    builder.build(app)?;
    Ok(())
}

/// Shows or removes the "update to …" line at the top of the menu.
pub fn set_update(app: &AppHandle, version: Option<&str>) {
    let Some(menu) = MENU.lock().unwrap().clone() else { return };
    let mut item = UPDATE_ITEM.lock().unwrap();
    match (version, item.as_ref()) {
        (Some(version), Some(existing)) => {
            let _ = existing.set_text(format!("Обновить до {version}"));
        }
        (Some(version), None) => {
            if let Ok(new) =
                MenuItem::with_id(app, "update", format!("Обновить до {version}"), true, None::<&str>)
            {
                if menu.insert(&new, 0).is_ok() {
                    *item = Some(new);
                }
            }
        }
        (None, Some(existing)) => {
            let _ = menu.remove(existing);
            *item = None;
        }
        (None, None) => {}
    }
}

/// Opens the settings window. It is created on demand and destroyed on close, so
/// a web view doesn't sit in memory while the app only waits in the tray.
pub fn show_settings(app: &AppHandle) {
    let handle = app.clone();
    // Windows are built on the main thread; this may be called from the action thread.
    let _ = app.run_on_main_thread(move || {
        if let Some(window) = handle.get_webview_window("main") {
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_focus();
            return;
        }
        let Some(config) = handle.config().app.windows.iter().find(|w| w.label == "main").cloned() else {
            return;
        };
        let built = WebviewWindowBuilder::from_config(&handle, &config).map(|builder| {
            // Shown once the page has loaded, so there is no empty frame first.
            builder.visible(false).on_page_load(|window, payload| {
                if payload.event() == PageLoadEvent::Finished {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            })
        });
        match built.and_then(WebviewWindowBuilder::build) {
            Ok(window) => crate::glass::round(&window),
            Err(e) => eprintln!("airtype: settings window: {e}"),
        }
    });
}
