//! One run of an action: grab the text from the focused field, send it to the
//! model, paste the answer back, then put the user's clipboard back as it was.

use crate::{
    config, hud,
    input::{self, Chord},
    llm,
    state::AppState,
    tray,
};
use arboard::Clipboard;
use std::{
    sync::atomic::Ordering,
    thread,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Manager};

pub fn trigger(app: &AppHandle, action_id: &str) {
    let state = app.state::<AppState>();
    if state.busy.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    let action_id = action_id.to_string();
    thread::spawn(move || {
        let show_hud = app.state::<AppState>().settings().show_hud;
        match run(&app, &action_id) {
            Ok(()) if show_hud => hud::done(&app),
            Ok(()) => {}
            Err(message) => hud::error(&app, &message),
        }
        app.state::<AppState>().busy.store(false, Ordering::SeqCst);
    });
}

fn run(app: &AppHandle, action_id: &str) -> Result<(), String> {
    if !input::SUPPORTED {
        return Err("эта система пока не поддерживается".into());
    }
    let state = app.state::<AppState>();
    let settings = state.settings();
    let action = settings.actions.iter().find(|a| a.id == action_id).cloned().ok_or("действие не найдено")?;

    let api_key = config::api_key(settings.provider);
    if api_key.is_none() && settings.requires_key() {
        tray::show_settings(app);
        return Err("сначала добавь api ключ в настройках".into());
    }
    if !input::accessibility_granted(false) {
        input::accessibility_granted(true);
        return Err("дай airtype доступ в «универсальном доступе»".into());
    }

    input::wait_modifiers_released(Duration::from_millis(1500));
    let target = input::foreground();
    let mut clipboard = Clipboard::new().map_err(|e| format!("буфер обмена недоступен: {e}"))?;
    let saved = clipboard.get_text().ok();

    let Some(text) = grab_text(&mut clipboard) else {
        restore(&mut clipboard, saved);
        return Err("не нашёл текст: выдели его или поставь курсор в поле".into());
    };

    if settings.show_hud {
        hud::working(app, &action.name);
    }
    let system = llm::system_prompt(&settings, &action);
    let result = tauri::async_runtime::block_on(llm::complete(
        &state.http,
        &settings,
        api_key.as_deref(),
        &system,
        &text,
    ));
    let output = match result {
        Ok(output) => output,
        Err(e) => {
            restore(&mut clipboard, saved);
            return Err(e);
        }
    };

    input::restore_focus(target);
    set_private(&mut clipboard, output).map_err(|e| format!("не смог положить текст в буфер: {e}"))?;
    thread::sleep(Duration::from_millis(40));
    input::press(Chord::Paste);
    // Apps read the clipboard a moment after Ctrl+V, so don't swap it back too early.
    thread::sleep(Duration::from_millis(350));
    restore(&mut clipboard, saved);
    Ok(())
}

/// Copies the selection; with nothing selected, takes the whole field.
fn grab_text(clipboard: &mut Clipboard) -> Option<String> {
    let _ = clipboard.clear();
    input::press(Chord::Copy);
    if let Some(text) = wait_for_text(clipboard, Duration::from_millis(400)) {
        return Some(text);
    }
    input::press(Chord::SelectAll);
    thread::sleep(Duration::from_millis(60));
    input::press(Chord::Copy);
    wait_for_text(clipboard, Duration::from_millis(400))
}

fn wait_for_text(clipboard: &mut Clipboard, timeout: Duration) -> Option<String> {
    let start = Instant::now();
    loop {
        if let Ok(text) = clipboard.get_text() {
            if !text.trim().is_empty() {
                return Some(text);
            }
        }
        if start.elapsed() >= timeout {
            return None;
        }
        thread::sleep(Duration::from_millis(15));
    }
}

fn restore(clipboard: &mut Clipboard, saved: Option<String>) {
    match saved {
        Some(text) => {
            let _ = set_private(clipboard, text);
        }
        None => {
            let _ = clipboard.clear();
        }
    }
}

/// Sets text without adding it to clipboard history (Win+V, clipboard managers).
fn set_private(clipboard: &mut Clipboard, text: String) -> Result<(), arboard::Error> {
    #[cfg(target_os = "macos")]
    use arboard::SetExtApple as _;
    #[cfg(all(unix, not(target_os = "macos")))]
    use arboard::SetExtLinux as _;
    #[cfg(windows)]
    use arboard::SetExtWindows as _;
    clipboard.set().exclude_from_history().text(text)
}
