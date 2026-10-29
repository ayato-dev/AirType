mod clipboard;
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
            let loaded = config::load(&settings_path);
            let notice = loaded.notice.is_some();
            app.manage(AppState::new(loaded, settings_path));

            let handle = app.handle();
            tray::create(handle)?;
            hotkeys::apply(handle);

            // The keychain can wait on a macOS permission prompt; ask it off the main thread.
            let handle = handle.clone();
            std::thread::spawn(move || {
                let settings = handle.state::<AppState>().settings();
                let needs_setup = settings.requires_key() && config::api_key(settings.provider).is_none();
                if notice || needs_setup || !input::accessibility_granted(false) {
                    tray::show_settings(&handle);
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            // Settings are destroyed on close and built again on demand; the HUD
            // lives for the whole run and only hides.
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() != "main" {
                    api.prevent_close();
                    let _ = window.hide();
                }
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
