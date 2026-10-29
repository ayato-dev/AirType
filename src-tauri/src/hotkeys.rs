use crate::{engine, state::AppState};
use serde::Serialize;
use std::sync::atomic::Ordering;
use tauri::{plugin::TauriPlugin, AppHandle, Manager, Wry};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyError {
    pub action_id: String,
    pub message: String,
}

pub fn plugin() -> TauriPlugin<Wry> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, shortcut, event| {
            // Fire on release: the key that completes the combo is up by then,
            // so it can't mix into the Ctrl+C we are about to send.
            if event.state() != ShortcutState::Released {
                return;
            }
            let action_id = app.state::<AppState>().hotkeys.lock().unwrap().get(&shortcut.id()).cloned();
            if let Some(action_id) = action_id {
                engine::trigger(app, &action_id);
            }
        })
        .build()
}

/// Re-registers every action hotkey from the current settings.
pub fn apply(app: &AppHandle) {
    let state = app.state::<AppState>();
    let shortcuts = app.global_shortcut();
    let _ = shortcuts.unregister_all();

    let mut map = state.hotkeys.lock().unwrap();
    map.clear();
    let mut errors = Vec::new();
    let settings = state.settings();

    if !settings.paused && !state.suspended.load(Ordering::SeqCst) {
        for action in settings.actions.iter().filter(|a| a.enabled && !a.hotkey.trim().is_empty()) {
            let error = |message: &str| HotkeyError { action_id: action.id.clone(), message: message.into() };
            let shortcut: Shortcut = match action.hotkey.parse() {
                Ok(shortcut) => shortcut,
                Err(_) => {
                    errors.push(error("Не понял это сочетание"));
                    continue;
                }
            };
            if map.contains_key(&shortcut.id()) {
                errors.push(error("Это сочетание уже у другого действия"));
                continue;
            }
            match shortcuts.register(shortcut) {
                Ok(()) => {
                    map.insert(shortcut.id(), action.id.clone());
                }
                Err(_) => errors.push(error("Сочетание занято другой программой")),
            }
        }
    }
    *state.hotkey_errors.lock().unwrap() = errors;
}
