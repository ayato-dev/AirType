use crate::{
    config::{self, Provider, Settings},
    hotkeys::{self, HotkeyError},
    hud, input, llm, local, panel,
    state::{self, AppState},
    update,
};
use serde::Serialize;
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Manager, WebviewWindow};
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
    /// Windows 11 rounds the settings window; the page follows its corners.
    rounded: bool,
    /// Linux under Wayland, where hotkeys reach only X11 apps.
    wayland: bool,
    /// There is a llama.cpp build for this system.
    local_supported: bool,
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
        rounded: crate::glass::rounded(),
        wayland: input::wayland(),
        local_supported: local::supported(),
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
pub async fn test_connection(app: AppHandle, text: String) -> Result<String, String> {
    let settings = app.state::<AppState>().settings();
    let action = settings.actions.first().ok_or("Нет ни одного действия")?;
    let prompt = llm::action_prompt(&settings, action, &text);
    // One request at a time: a check must not run beside a hotkey's action.
    if app.state::<AppState>().busy.swap(true, Ordering::SeqCst) {
        return Err("Сейчас идёт другое действие, попробуй чуть позже".into());
    }
    // Reads the keychain and may start the local model: off the async workers.
    let handle = app.clone();
    let result =
        tauri::async_runtime::spawn_blocking(move || llm::transform_text(&handle, &settings, &prompt, None))
            .await
            .map_err(|e| e.to_string());
    app.state::<AppState>().busy.store(false, Ordering::SeqCst);
    result?
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

/// The HUD page has laid out its slip at this size.
#[tauri::command]
pub fn hud_fit(app: AppHandle, width: f64, height: f64) {
    hud::fit(&app, width, height);
}

#[tauri::command]
pub fn panel_view() -> Option<panel::View> {
    panel::view()
}

#[tauri::command]
pub fn panel_fit(app: AppHandle, seq: u64, height: f64) {
    panel::fit(&app, seq, height);
}

#[tauri::command]
pub async fn panel_run(app: AppHandle, ask: panel::Ask) -> Result<String, String> {
    panel::run(app, ask).await
}

/// The page dropped the answer it was waiting for ("Back").
#[tauri::command]
pub fn panel_cancel() {
    panel::cancel_running();
}

#[tauri::command]
pub fn panel_write(app: AppHandle, seq: u64, text: String) {
    panel::write(&app, seq, text);
}

/// Copying is the user's own choice here, so it goes to the clipboard like any copy.
#[tauri::command]
pub fn panel_copy(text: String) -> Result<(), String> {
    arboard::Clipboard::new().and_then(|mut clipboard| clipboard.set_text(text)).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn panel_close(app: AppHandle, seq: u64, restore: bool) {
    panel::close(&app, seq, restore);
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalModel {
    id: &'static str,
    name: &'static str,
    note: &'static str,
    size: u64,
    downloaded: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalStatus {
    models: Vec<LocalModel>,
    download: Option<local::Progress>,
    /// The engine can run on the video card here.
    gpu: bool,
}

#[tauri::command]
pub fn local_status(app: AppHandle) -> LocalStatus {
    let models = local::MODELS
        .iter()
        .map(|m| LocalModel {
            id: m.id,
            name: m.name,
            note: m.note,
            size: m.size,
            downloaded: local::downloaded(&app, m),
        })
        .collect();
    LocalStatus { models, download: local::progress(), gpu: local::HAS_GPU }
}

#[tauri::command]
pub fn local_download(app: AppHandle, model: String) -> Result<(), String> {
    local::download(&app, &model)
}

#[tauri::command]
pub fn local_cancel() {
    local::cancel();
}

#[tauri::command]
pub async fn local_delete(app: AppHandle, model: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || local::delete(&app, &model))
        .await
        .map_err(|e| e.to_string())?
}

/// Closes settings for good; the tray builds them again when needed. The page
/// has saved by now, so this skips its close-requested handler.
#[tauri::command]
pub fn hide_settings(window: WebviewWindow) {
    let _ = window.destroy();
}

/// Cheap enough to poll while the user is in System Settings: no keychain.
#[tauri::command]
pub fn accessibility_status() -> bool {
    input::accessibility_granted(false)
}

#[tauri::command]
pub fn update_status() -> update::Status {
    update::status()
}

#[tauri::command]
pub async fn update_check(app: AppHandle) -> Result<Option<String>, String> {
    update::check(&app, true).await
}

#[tauri::command]
pub fn update_install(app: AppHandle) {
    update::install(&app);
}
