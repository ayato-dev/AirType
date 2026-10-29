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
    /// Why the settings file was set aside, if it was.
    notice: Option<String>,
    platform: &'static str,
    /// Linux under Wayland, where hotkeys reach only X11 apps.
    wayland: bool,
    accessibility: bool,
    autostart: bool,
    version: String,
}

/// Keychain calls can block on a macOS permission prompt, so they never run on
/// the main thread or on an async worker.
async fn keychain<T: Send + 'static>(call: impl FnOnce() -> T + Send + 'static) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(call).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_state(app: AppHandle) -> Result<UiState, String> {
    let keys = keychain(|| KeyStatus {
        anthropic: config::api_key(Provider::Anthropic).is_some(),
        openai: config::api_key(Provider::Openai).is_some(),
    })
    .await?;
    let state = app.state::<AppState>();
    let hotkey_errors = state.hotkey_errors.lock().unwrap().clone();
    let notice = state.notice.lock().unwrap().clone();
    Ok(UiState {
        settings: state.settings(),
        keys,
        hotkey_errors,
        notice,
        platform: std::env::consts::OS,
        wayland: input::wayland(),
        accessibility: input::accessibility_granted(false),
        autostart: app.autolaunch().is_enabled().unwrap_or(false),
        version: app.package_info().version.to_string(),
    })
}

#[tauri::command]
pub fn save_settings(app: AppHandle, settings: Settings) -> Result<Vec<HotkeyError>, String> {
    state::apply_settings(&app, settings)
}

#[tauri::command]
pub async fn set_api_key(provider: Provider, key: String) -> Result<(), String> {
    keychain(move || config::set_api_key(provider, &key)).await?
}

/// Runs the first action on a sample phrase so the user can check key and model.
#[tauri::command]
pub async fn test_connection(state: State<'_, AppState>, text: String) -> Result<String, String> {
    let settings = state.settings();
    let action = settings.actions.first().ok_or("Нет ни одного действия")?;
    let provider = settings.provider;
    let api_key = keychain(move || config::api_key(provider)).await?;
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

/// Closes settings for good; the tray builds them again when needed.
#[tauri::command]
pub fn hide_settings(window: WebviewWindow) {
    let _ = window.close();
}
