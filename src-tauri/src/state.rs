use crate::config::{self, Settings};
use crate::hotkeys::{self, HotkeyError};
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{atomic::AtomicBool, Mutex},
};
use tauri::{menu::CheckMenuItem, AppHandle, Emitter, Manager, Wry};

pub struct AppState {
    settings: Mutex<Settings>,
    settings_path: PathBuf,
    pub http: reqwest::Client,
    /// An action is running; extra hotkey presses are ignored until it ends.
    pub busy: AtomicBool,
    /// Hotkeys are off while the settings window records a new combination.
    pub suspended: AtomicBool,
    /// Registered shortcut id -> action id.
    pub hotkeys: Mutex<HashMap<u32, String>>,
    pub hotkey_errors: Mutex<Vec<HotkeyError>>,
    pub pause_item: Mutex<Option<CheckMenuItem<Wry>>>,
}

impl AppState {
    pub fn new(settings: Settings, settings_path: PathBuf) -> Self {
        Self {
            settings: Mutex::new(settings),
            settings_path,
            http: crate::llm::http_client(),
            busy: AtomicBool::new(false),
            suspended: AtomicBool::new(false),
            hotkeys: Mutex::new(HashMap::new()),
            hotkey_errors: Mutex::new(Vec::new()),
            pause_item: Mutex::new(None),
        }
    }

    pub fn settings(&self) -> Settings {
        self.settings.lock().unwrap().clone()
    }

    pub fn store(&self, settings: Settings) -> Result<(), String> {
        config::save(&self.settings_path, &settings)?;
        *self.settings.lock().unwrap() = settings;
        Ok(())
    }
}

/// Saves new settings and brings hotkeys, tray and windows in line with them.
pub fn apply_settings(app: &AppHandle, settings: Settings) -> Result<Vec<HotkeyError>, String> {
    let state = app.state::<AppState>();
    let old = state.settings();
    state.store(settings.clone())?;

    if old.actions != settings.actions || old.paused != settings.paused {
        hotkeys::apply(app);
    }
    if old.paused != settings.paused {
        if let Some(item) = state.pause_item.lock().unwrap().as_ref() {
            let _ = item.set_checked(settings.paused);
        }
    }
    let _ = app.emit_to("main", "settings-changed", &settings);
    let errors = state.hotkey_errors.lock().unwrap().clone();
    Ok(errors)
}

pub fn set_paused(app: &AppHandle, paused: bool) {
    let mut settings = app.state::<AppState>().settings();
    settings.paused = paused;
    let _ = apply_settings(app, settings);
}
