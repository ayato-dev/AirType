//! One run of an action: take the text from the focused field, send it to the
//! model, put the answer back.
//!
//! The text is read through the accessibility API when the app supports it and
//! with Ctrl/Cmd+C otherwise. The answer is written the same way where that is
//! safe, else pasted; the user's clipboard is put back as it was.

use crate::{
    config,
    field::{self, Focused, Replaced},
    hud,
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
        return Err("Эта система пока не поддерживается".into());
    }
    let state = app.state::<AppState>();
    let settings = state.settings();
    let action = settings.actions.iter().find(|a| a.id == action_id).cloned().ok_or("Действие не найдено")?;

    let api_key = config::api_key(settings.provider);
    if api_key.is_none() && settings.requires_key() {
        tray::show_settings(app);
        return Err("Сначала добавь ключ API в настройках".into());
    }
    if !input::accessibility_granted(false) {
        input::accessibility_granted(true);
        return Err("Дай Airtype «Универсальный доступ» в настройках macOS".into());
    }

    input::wait_modifiers_released(Duration::from_millis(1500));
    let target = input::foreground();
    let field = match field::focused() {
        Focused::Password => return Err("Это поле для пароля, его не трогаю".into()),
        Focused::Text(snapshot) if !snapshot.text.trim().is_empty() => Some(snapshot),
        Focused::Text(_) | Focused::Unknown => None,
    };
    let mut clipboard = Clipboard::new().map_err(|e| format!("Буфер обмена недоступен: {e}"))?;
    let saved = clipboard.get_text().ok();

    let text = match &field {
        Some(snapshot) => snapshot.text.clone(),
        None => match grab_text(&mut clipboard) {
            Some(text) => text,
            None => {
                restore(&mut clipboard, saved);
                return Err("Не нашёл текст. Выдели его или поставь курсор в поле".into());
            }
        },
    };

    if settings.show_hud {
        hud::working(app, &action.id, &action.name);
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
            // Read through the accessibility API, the clipboard was never touched.
            if field.is_none() {
                restore(&mut clipboard, saved);
            }
            return Err(e);
        }
    };

    input::restore_focus(target);
    if let Some(snapshot) = &field {
        match snapshot.replace(&output) {
            Replaced::Done => return Ok(()),
            Replaced::Changed => {
                // Pasting now would land on whatever the user is doing; leave the answer to them.
                let _ = clipboard.set_text(output);
                return Err("Текст поменялся, пока нейронка думала. Ответ лежит в буфере обмена".into());
            }
            Replaced::Unsupported if snapshot.whole => {
                input::press(Chord::SelectAll);
                thread::sleep(Duration::from_millis(60));
            }
            Replaced::Unsupported => {}
        }
    }
    paste(&mut clipboard, output, saved)
}

fn paste(clipboard: &mut Clipboard, output: String, saved: Option<String>) -> Result<(), String> {
    set_private(clipboard, output).map_err(|e| format!("Не смог положить текст в буфер обмена: {e}"))?;
    thread::sleep(Duration::from_millis(40));
    input::press(Chord::Paste);
    // Apps read the clipboard a moment after Ctrl+V, so don't swap it back too early.
    thread::sleep(Duration::from_millis(350));
    restore(clipboard, saved);
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
