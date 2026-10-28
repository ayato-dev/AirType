use crate::state::{self, AppState};
#[cfg(not(target_os = "macos"))]
use tauri::tray::{MouseButton, MouseButtonState, TrayIconEvent};
use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    AppHandle, Manager,
};

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let paused = app.state::<AppState>().settings().paused;
    let open = MenuItem::with_id(app, "open", "Настройки", true, None::<&str>)?;
    let pause = CheckMenuItem::with_id(app, "pause", "Пауза", true, paused, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Выйти", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&open, &pause, &separator, &quit])?;
    *app.state::<AppState>().pause_item.lock().unwrap() = Some(pause.clone());

    let builder =
        TrayIconBuilder::with_id("main").tooltip("Airtype").menu(&menu).on_menu_event(move |app, event| {
            match event.id().as_ref() {
                "open" => show_settings(app),
                "pause" => state::set_paused(app, pause.is_checked().unwrap_or(false)),
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

pub fn show_settings(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}
