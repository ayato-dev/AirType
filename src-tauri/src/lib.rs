mod clipboard;
mod commands;
mod config;
mod engine;
mod field;
mod glass;
mod hotkeys;
mod hud;
mod input;
mod llm;
mod local;
mod panel;
mod popup;
mod state;
mod tray;
mod update;

use state::AppState;
use std::sync::atomic::Ordering;
use tauri::{Manager, RunEvent, WindowEvent};
use tauri_plugin_autostart::MacosLauncher;

pub fn run() {
    let app = tauri::Builder::default()
        // Must be first: a second launch just opens the settings of the running one.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| tray::show_settings(app)))
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        .plugin(hotkeys::plugin())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .setup(|app| {
            // Menu bar app: no Dock icon.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let settings_path = app.path().app_config_dir()?.join("settings.json");
            let loaded = config::load(&settings_path);
            let notice = loaded.notice.is_some();
            app.manage(AppState::new(loaded, settings_path));

            // X11 serves the clipboard from the process that filled it, and arboard stops
            // serving when its last instance is dropped. Keep one alive so the text put
            // back after a paste outlives the run.
            #[cfg(target_os = "linux")]
            if let Ok(clipboard) = arboard::Clipboard::new() {
                std::mem::forget(clipboard);
            }

            let handle = app.handle();
            tray::create(handle)?;
            hotkeys::apply(handle);
            hud::init(handle);
            update::watch(handle);

            // Off the main thread: the keychain can wait on a macOS permission prompt,
            // and a server left behind by a crash takes a moment to stop.
            let handle = handle.clone();
            std::thread::spawn(move || {
                local::reap_left_behind(&handle);
                let settings = handle.state::<AppState>().settings();
                let needs_setup = settings.requires_key() && config::api_key(settings.provider).is_none();
                if notice || needs_setup || !input::accessibility_granted(false) {
                    tray::show_settings(&handle);
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| match event {
            // Settings are destroyed on close and built again on demand; the HUD
            // and the panel live for the whole run and only hide.
            WindowEvent::CloseRequested { api, .. } if window.label() != "main" => {
                api.prevent_close();
                if window.label() == "panel" {
                    panel::close_any(window.app_handle());
                } else {
                    let _ = window.hide();
                }
            }
            WindowEvent::Focused(focused) if window.label() == "panel" => {
                panel::focus_changed(window.app_handle(), *focused);
            }
            // Settings closed while recording a hotkey: the page can't turn them back on.
            WindowEvent::Destroyed if window.label() == "main" => {
                let app = window.app_handle();
                if app.state::<AppState>().suspended.swap(false, Ordering::SeqCst) {
                    hotkeys::apply(app);
                }
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_state,
            commands::save_settings,
            commands::set_api_key,
            commands::test_connection,
            commands::suspend_hotkeys,
            commands::set_autostart,
            commands::request_accessibility,
            commands::accessibility_status,
            commands::hide_settings,
            commands::hud_fit,
            commands::panel_view,
            commands::panel_fit,
            commands::panel_run,
            commands::panel_cancel,
            commands::panel_write,
            commands::panel_copy,
            commands::panel_close,
            commands::local_status,
            commands::local_download,
            commands::local_cancel,
            commands::local_delete,
            commands::update_status,
            commands::update_check,
            commands::update_install,
        ])
        .build(tauri::generate_context!())
        .expect("failed to start airtype");

    app.run(|_app, event| match event {
        // Keep running with no visible windows; only "Quit" in the tray exits.
        RunEvent::ExitRequested { api, code: None, .. } => api.prevent_exit(),
        RunEvent::Exit => local::shutdown(),
        _ => {}
    });
}
