use crate::{engine, panel, state::AppState};
use serde::Serialize;
use std::{
    collections::HashMap,
    sync::atomic::{AtomicBool, Ordering},
};
use tauri::{plugin::TauriPlugin, AppHandle, Manager, Wry};
use tauri_plugin_global_shortcut::{Code, GlobalShortcutExt, Shortcut, ShortcutState};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyError {
    pub action_id: String,
    pub message: String,
}

pub fn plugin() -> TauriPlugin<Wry> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, shortcut, event| {
            if shortcut.id() == escape_key().id() && ESCAPE_TAKEN.load(Ordering::SeqCst) {
                if event.state() == ShortcutState::Pressed {
                    panel::escape(app);
                }
                return;
            }
            // Fire on release: the key that completes the combo is up by then,
            // so it can't mix into the Ctrl+C we are about to send.
            if event.state() != ShortcutState::Released {
                return;
            }
            let action_id = app.state::<AppState>().hotkeys.lock().unwrap().get(&shortcut.id()).cloned();
            match action_id.as_deref() {
                Some(PANEL) => panel::trigger(app),
                Some(action_id) => engine::trigger(app, action_id),
                None => {}
            }
        })
        .build()
}

/// The id the panel's hotkey goes by, next to the action ids.
pub const PANEL: &str = "panel";

fn escape_key() -> Shortcut {
    Shortcut::new(None, Code::Escape)
}

/// Esc is ours: taken while the panel is up, so it closes the panel even when
/// the app underneath has the keyboard.
static ESCAPE_TAKEN: AtomicBool = AtomicBool::new(false);

/// Takes Esc while the panel is up and gives it back after. An Esc some action
/// already uses stays with that action.
pub fn escape(app: &AppHandle, take: bool) {
    let shortcuts = app.global_shortcut();
    if take {
        if !ESCAPE_TAKEN.load(Ordering::SeqCst) && !shortcuts.is_registered(escape_key()) {
            ESCAPE_TAKEN.store(shortcuts.register(escape_key()).is_ok(), Ordering::SeqCst);
        }
    } else if ESCAPE_TAKEN.swap(false, Ordering::SeqCst) {
        let _ = shortcuts.unregister(escape_key());
    }
}

/// Re-registers every action hotkey and the panel's from the current settings.
pub fn apply(app: &AppHandle) {
    let state = app.state::<AppState>();
    let shortcuts = app.global_shortcut();
    let _ = shortcuts.unregister_all();
    ESCAPE_TAKEN.store(false, Ordering::SeqCst);

    // Built aside and swapped in at the end: no lock is held while the OS registers keys.
    let mut map = HashMap::new();
    let mut errors = Vec::new();
    let settings = state.settings();

    if !settings.paused && !state.suspended.load(Ordering::SeqCst) {
        let actions =
            settings.actions.iter().filter(|a| a.enabled).map(|a| (a.id.as_str(), a.hotkey.as_str()));
        let panel = settings.panel.enabled.then_some((PANEL, settings.panel.hotkey.as_str()));
        for (id, hotkey) in actions.chain(panel).filter(|(_, hotkey)| !hotkey.trim().is_empty()) {
            let error = |message: &str| HotkeyError { action_id: id.to_string(), message: message.into() };
            let shortcut: Shortcut = match hotkey.parse() {
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
                    map.insert(shortcut.id(), id.to_string());
                }
                Err(_) => errors.push(error("Сочетание занято другой программой")),
            }
        }
    }
    *state.hotkeys.lock().unwrap() = map;
    *state.hotkey_errors.lock().unwrap() = errors;
}
