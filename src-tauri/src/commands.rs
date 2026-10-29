use crate::{
    config::{self, Provider, Settings},
    hotkeys::{self, HotkeyError},
    input, llm,
    state::{self, AppState},
};
use serde::Serialize;
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Manager, State, WebviewWindow};
use tauri_plugin_autostart::ManagerExt;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyStatus {
    anthropic: bool,
    openai: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UiState {
    settings: Settings,
    keys: KeyStatus,
    hotkey_errors: Vec<HotkeyError>,
    platform: &'static str,
    accessibility: bool,
    autostart: bool,
    version: String,
}

#[tauri::command]
pub fn get_state(app: AppHandle, state: State<'_, AppState>) -> UiState {
    UiState {
        settings: state.settings(),
        keys: KeyStatus {
            anthropic: config::api_key(Provider::Anthropic).is_some(),
            openai: config::api_key(Provider::Openai).is_some(),
        },
        hotkey_errors: state.hotkey_errors.lock().unwrap().clone(),
        platform: std::env::consts::OS,
        accessibility: input::accessibility_granted(false),
        autostart: app.autolaunch().is_enabled().unwrap_or(false),
        version: app.package_info().version.to_string(),
    }
}

#[tauri::command]
pub fn save_settings(app: AppHandle, settings: Settings) -> Result<Vec<HotkeyError>, String> {
    state::apply_settings(&app, settings)
}

#[tauri::command]
pub fn set_api_key(provider: Provider, key: String) -> Result<(), String> {
    config::set_api_key(provider, &key)
}

/// Runs the first action on a sample phrase so the user can check key and model.
#[tauri::command]
pub async fn test_connection(state: State<'_, AppState>, text: String) -> Result<String, String> {
    let settings = state.settings();
    let action = settings.actions.first().ok_or("Нет ни одного действия")?;
    let api_key = config::api_key(settings.provider);
    if api_key.is_none() && settings.requires_key() {
        return Err("Сначала вставь ключ API".into());
    }
    let system = llm::system_prompt(&settings, action);
    llm::complete(&state.http, &settings, api_key.as_deref(), &system, &text).await
}

/// Turns global hotkeys off while the settings window records a new combination.
#[tauri::command]
pub fn suspend_hotkeys(app: AppHandle, suspended: bool) -> Vec<HotkeyError> {
    let state = app.state::<AppState>();
    state.suspended.store(suspended, Ordering::SeqCst);
    hotkeys::apply(&app);
    let errors = state.hotkey_errors.lock().unwrap().clone();
    errors
}

#[tauri::command]
pub fn set_autostart(app: AppHandle, enabled: bool) -> Result<(), String> {
    let autolaunch = app.autolaunch();
    let result = if enabled { autolaunch.enable() } else { autolaunch.disable() };
    result.map_err(|e| e.to_string())
}

#[tauri::command]
pub fn request_accessibility() -> bool {
    let granted = input::accessibility_granted(true);
    #[cfg(target_os = "macos")]
    if !granted {
        let _ = std::process::Command::new("open")
            .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility")
            .spawn();
    }
    granted
}

#[tauri::command]
pub fn hide_settings(window: WebviewWindow) {
    let _ = window.hide();
}
