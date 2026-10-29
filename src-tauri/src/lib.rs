mod commands;
mod config;
mod engine;
mod field;
mod hotkeys;
mod hud;
mod input;
mod llm;
mod state;
mod tray;

use state::AppState;
use tauri::{Manager, RunEvent, WindowEvent};
use tauri_plugin_autostart::MacosLauncher;

pub fn run() {
    let app = tauri::Builder::default()
        // Must be first: a second launch just opens the settings of the running one.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| tray::show_settings(app)))
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        .plugin(hotkeys::plugin())
        .setup(|app| {
            // Menu bar app: no Dock icon.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let settings_path = app.path().app_config_dir()?.join("settings.json");
            let settings = config::load(&settings_path);
            let needs_setup = settings.requires_key() && config::api_key(settings.provider).is_none();
            app.manage(AppState::new(settings, settings_path));

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

            if needs_setup || !input::accessibility_granted(false) {
                tray::show_settings(handle);
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing settings only hides them; the app keeps living in the tray.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_state,
            commands::save_settings,
            commands::set_api_key,
            commands::test_connection,
            commands::suspend_hotkeys,
            commands::set_autostart,
            commands::request_accessibility,
            commands::hide_settings,
        ])
        .build(tauri::generate_context!())
        .expect("failed to start airtype");

    app.run(|_app, event| {
        // Keep running with no visible windows; only "Quit" in the tray exits.
        if let RunEvent::ExitRequested { api, code: None, .. } = event {
            api.prevent_exit();
        }
    });
}
