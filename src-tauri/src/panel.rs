//! The panel on glass under the text. Its hotkey writes something new where the
//! caret is ("write for me"), or, with text selected anywhere, offers what to do
//! with it and shows the answer before it goes in. Answers that can't go back
//! in (a selection on a web page) end up here too.
//!
//! The hotkey's thread keeps the field it read and waits for the page: the
//! accessibility objects behind it stay on the thread that made them.

use crate::{
    engine::{self, Scope, Source, Written},
    glass, hud,
    input::{self, Target},
    llm,
    popup::{self, Fallback, Rect},
    state::AppState,
};
use serde::{Deserialize, Serialize};
use std::{
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc::{self, RecvTimeoutError, Sender},
        Mutex,
    },
    thread,
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter, LogicalSize, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

const LABEL: &str = "panel";
pub const WIDTH: f64 = 420.0;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    seq: u64,
    /// `compose`, `selection` or `answer`.
    mode: &'static str,
    /// The selection the panel works on, or the text an answer is for.
    text: String,
    /// An answer that is already there (mode `answer`).
    answer: String,
    /// Why the answer is here and not in the field.
    note: Option<String>,
    /// The answer can go back where the text came from.
    writable: bool,
    actions: Vec<ActionView>,
    target_lang: String,
    secondary_lang: String,
}

#[derive(Clone, Serialize)]
pub struct ActionView {
    id: String,
    name: String,
}

enum Reply {
    Write(String),
}

struct Job {
    view: View,
    /// The hotkey's thread, waiting to write the answer; none for a bare answer.
    reply: Option<Sender<Reply>>,
    anchor: Option<Rect>,
    target: Target,
    /// Had focus once: losing it after that means the user went elsewhere.
    focused: bool,
    shown: bool,
}

static JOB: Mutex<Option<Job>> = Mutex::new(None);
static SEQ: AtomicU64 = AtomicU64::new(0);

pub fn trigger(app: &AppHandle) {
    let state = app.state::<AppState>();
    if state.busy.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    thread::spawn(move || {
        hud::set_anchor(None);
        if let Err(message) = session(&app) {
            hud::error(&app, &message);
        }
        app.state::<AppState>().busy.store(false, Ordering::SeqCst);
    });
}

fn session(app: &AppHandle) -> Result<(), String> {
    let settings = app.state::<AppState>().settings();
    engine::preflight(app, &settings)?;
    let cx = engine::context();
    hud::set_anchor(cx.anchor);
    let (mode, source) = match engine::read(Scope::Selection, cx.terminal)? {
        Some(source) => ("selection", source),
        None => ("compose", Source::caret()),
    };
    let (sender, replies) = mpsc::channel();
    let writable = source.editable != Some(false);
    let seq = open(
        app,
        mode,
        source.text.clone(),
        String::new(),
        None,
        writable,
        Some(sender),
        cx.anchor,
        cx.target,
    )?;

    let opened = Instant::now();
    loop {
        match replies.recv_timeout(Duration::from_secs(1)) {
            Ok(Reply::Write(text)) => {
                return match engine::write(&source, &text, cx.target)? {
                    Written::Done => Ok(()),
                    Written::Missed(why) => {
                        show_answer(app, source.text, text, Some(why), cx.anchor, cx.target);
                        Ok(())
                    }
                };
            }
            // Closed: the job and its sender are gone.
            Err(RecvTimeoutError::Disconnected) => return Ok(()),
            Err(RecvTimeoutError::Timeout) => {
                // A page that never came up must not hold the hotkeys forever.
                let stuck = JOB.lock().unwrap().as_ref().is_some_and(|job| job.view.seq == seq && !job.shown);
                if stuck && opened.elapsed() > Duration::from_secs(8) {
                    close(app, false);
                    return Err("Панель не открылась, попробуй ещё раз".into());
                }
            }
        }
    }
}

/// Shows an answer that has nowhere to go, with a way to copy it.
pub fn show_answer(
    app: &AppHandle,
    text: String,
    answer: String,
    note: Option<&str>,
    anchor: Option<Rect>,
    target: Target,
) {
    let note = note.map(str::to_string);
    if let Err(e) = open(app, "answer", text, answer, note, false, None, anchor, target) {
        hud::error(app, &e);
    }
}

#[allow(clippy::too_many_arguments)]
fn open(
    app: &AppHandle,
    mode: &'static str,
    text: String,
    answer: String,
    note: Option<String>,
    writable: bool,
    reply: Option<Sender<Reply>>,
    anchor: Option<Rect>,
    target: Target,
) -> Result<u64, String> {
    let settings = app.state::<AppState>().settings();
    let seq = SEQ.fetch_add(1, Ordering::SeqCst) + 1;
    let view = View {
        seq,
        mode,
        text,
        answer,
        note,
        writable,
        actions: settings
            .actions
            .iter()
            .map(|a| ActionView { id: a.id.clone(), name: a.name.clone() })
            .collect(),
        target_lang: settings.target_lang.trim().to_string(),
        secondary_lang: settings.secondary_lang.trim().to_string(),
    };
    *JOB.lock().unwrap() = Some(Job { view, reply, anchor, target, focused: false, shown: false });
    let window = window(app)?;
    // The theme may have changed since the last time.
    glass::acrylic(&window);
    // A page that is still loading asks for the view itself once it is up.
    let _ = app.emit_to(LABEL, "panel-open", seq);
    Ok(seq)
}

fn window(app: &AppHandle) -> Result<WebviewWindow, String> {
    if let Some(window) = app.get_webview_window(LABEL) {
        return Ok(window);
    }
    let builder = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("panel.html".into()))
        .title("Airtype")
        .inner_size(WIDTH, 160.0)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .decorations(false)
        .transparent(true)
        // Linux has no glass, so the page draws the sheet and its shadow itself.
        .shadow(cfg!(not(target_os = "linux")))
        .always_on_top(true)
        .skip_taskbar(true)
        .visible_on_all_workspaces(true)
        .visible(false);
    #[cfg(target_os = "macos")]
    let builder = builder.effects(tauri::utils::config::WindowEffectsConfig {
        effects: vec![tauri::window::Effect::Popover],
        state: Some(tauri::window::EffectState::Active),
        radius: Some(14.0),
        color: None,
        interactive: false,
    });
    builder.build().map_err(|e| format!("Панель не открылась: {e}"))
}

pub fn view() -> Option<View> {
    JOB.lock().unwrap().as_ref().map(|job| job.view.clone())
}

/// The page has laid itself out at this size (logical pixels): place and show.
pub fn fit(app: &AppHandle, seq: u64, height: f64) {
    let anchor = {
        let mut job = JOB.lock().unwrap();
        let Some(job) = job.as_mut().filter(|job| job.view.seq == seq) else { return };
        job.shown = true;
        job.anchor
    };
    let Some(window) = app.get_webview_window(LABEL) else { return };
    let _ = window.set_size(LogicalSize::new(WIDTH, height));
    popup::place(app, &window, WIDTH, height, anchor, Fallback::Pointer);
    if !window.is_visible().unwrap_or(false) {
        let _ = window.show();
    }
    let _ = window.set_focus();
}

/// The page's request to the model: an action on the selection, an instruction
/// of the user's own, or "write for me".
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Ask {
    /// An action id, `custom` or `compose`.
    action: String,
    /// Translate into this; for `compose`, write in it (empty: the request's own language).
    #[serde(default)]
    lang: String,
    /// For `compose`: polite, friendly... Empty: as the request says.
    #[serde(default)]
    tone: String,
    /// What to write, or what to do with the selection.
    #[serde(default)]
    request: String,
}

pub async fn run(app: AppHandle, ask: Ask) -> Result<String, String> {
    let text = JOB.lock().unwrap().as_ref().map(|job| job.view.text.clone()).ok_or("Панель уже закрыта")?;
    let settings = app.state::<AppState>().settings();
    let prompt = match ask.action.as_str() {
        "compose" => {
            if ask.request.trim().is_empty() {
                return Err("Напиши, что нужно".into());
            }
            llm::compose(&ask.lang, &ask.tone, ask.request.trim())
        }
        id => {
            let (target, secondary) = languages(&ask.lang, &settings.target_lang, &settings.secondary_lang);
            let prompt = match id {
                "custom" if ask.request.trim().is_empty() => {
                    return Err("Напиши, что сделать с текстом".into())
                }
                "custom" => ask.request.trim().to_string(),
                id => {
                    settings.actions.iter().find(|a| a.id == id).ok_or("Действие не найдено")?.prompt.clone()
                }
            };
            llm::transform(&prompt, &target, &secondary, &text)
        }
    };
    tauri::async_runtime::spawn_blocking(move || llm::transform_text(&app, &settings, &prompt))
        .await
        .map_err(|e| e.to_string())?
}

/// The language picked in the panel becomes the target; the fallback for text
/// already in it is the other of the two from the settings.
fn languages(picked: &str, target: &str, secondary: &str) -> (String, String) {
    let same = |a: &str, b: &str| a.to_lowercase() == b.to_lowercase();
    let picked = picked.trim();
    let (target, secondary) = (target.trim(), secondary.trim());
    if picked.is_empty() || same(picked, target) {
        (target.into(), secondary.into())
    } else if same(picked, secondary) {
        (secondary.into(), target.into())
    } else {
        (picked.into(), secondary.into())
    }
}

/// Puts the answer in: back to the app first, then the hotkey's thread writes it.
pub fn write(app: &AppHandle, text: String) {
    let Some(job) = JOB.lock().unwrap().take() else { return };
    // While one of our windows is in front we may hand focus back; once it is
    // hidden, Windows would no longer let us.
    input::restore_focus(job.target);
    hide(app);
    if let Some(reply) = job.reply {
        let _ = reply.send(Reply::Write(text));
    }
}

/// Closes the panel. `restore` gives focus back to the app the text came from
/// (Esc); after a click elsewhere the user is already where they want to be.
pub fn close(app: &AppHandle, restore: bool) {
    let Some(job) = JOB.lock().unwrap().take() else { return };
    if restore {
        input::restore_focus(job.target);
    }
    hide(app);
    // Dropping the job drops its sender, which tells the hotkey's thread.
}

fn hide(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.hide();
    }
}

/// The panel's window gained or lost focus.
pub fn focus_changed(app: &AppHandle, focused: bool) {
    let blurred = {
        let mut job = JOB.lock().unwrap();
        let Some(job) = job.as_mut() else { return };
        if focused {
            job.focused = true;
        }
        !focused && job.focused
    };
    if blurred {
        close(app, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picked_language_swaps_the_pair() {
        let pair = |picked| languages(picked, "English", "Русский");
        assert_eq!(pair(""), ("English".into(), "Русский".into()));
        assert_eq!(pair("english"), ("English".into(), "Русский".into()));
        assert_eq!(pair("русский"), ("Русский".into(), "English".into()));
        assert_eq!(pair("Deutsch"), ("Deutsch".into(), "Русский".into()));
    }
}
