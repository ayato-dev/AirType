//! The panel on glass under the text. Its hotkey writes something new where the
//! caret is ("write for me"), or, with text selected anywhere, offers what to do
//! with it and shows the answer before it goes in. Answers that can't go back
//! in (a selection on a web page) end up here too.
//!
//! The hotkey's thread keeps the field it read and waits for the page: the
//! accessibility objects behind it stay on the thread that made them.
//!
//! The panel belongs to the window it was opened over and goes away like a
//! menu: Esc, a click outside it, or switching to another window closes it.

use crate::{
    engine::{self, Scope, Source, Written},
    glass, hotkeys, hud,
    input::{self, Front, Target},
    llm,
    popup::{self, Anchor, Fallback, Spot},
    state::AppState,
};
use serde::{Deserialize, Serialize};
use std::{
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc::{self, RecvTimeoutError, Sender},
        Arc, Mutex,
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
    /// Decided when the panel opens: once it is up the mouse is over the panel itself.
    spot: Spot,
    target: Target,
    /// Had focus once: losing it after that means the user went elsewhere.
    focused: bool,
    shown: bool,
}

static JOB: Mutex<Option<Job>> = Mutex::new(None);
static SEQ: AtomicU64 = AtomicU64::new(0);
/// The request the page is waiting for; set to drop it.
static RUNNING: Mutex<Option<Arc<AtomicBool>>> = Mutex::new(None);

/// Drops the request in flight, so a closed panel or "Again" doesn't leave a
/// paid request (or the local model) working for nobody.
pub fn cancel_running() {
    if let Some(cancel) = RUNNING.lock().unwrap().take() {
        cancel.store(true, Ordering::SeqCst);
    }
}

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
    let cx = engine::context()?;
    hud::set_anchor(cx.anchor);
    let (mode, source) = match engine::read(Scope::Selection, cx.terminal)? {
        Some(source) => ("selection", source),
        None => ("compose", Source::caret()),
    };
    // Refused now rather than after the user picks an action, and before a
    // megabyte of text lands in the page.
    llm::check_size(&settings, source.text.chars().count())?;
    let (sender, replies) = mpsc::channel();
    let writable = source.editable != Some(false);
    // Where the selection was; the answer that can't go in later stands there too.
    let spot = popup::locate(app, source.anchor(cx.anchor), Fallback::Pointer);
    let seq =
        open(app, mode, source.text.clone(), String::new(), None, writable, Some(sender), spot, cx.target)?;

    let opened = Instant::now();
    loop {
        match replies.recv_timeout(Duration::from_secs(1)) {
            Ok(Reply::Write(text)) => {
                return match engine::write(&source, &text, cx.target) {
                    Written::Done => Ok(()),
                    Written::Missed(why) => {
                        answer_at(app, source.text, text, Some(why), spot, cx.target);
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
                    close(app, seq, false);
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
    anchor: Option<Anchor>,
    target: Target,
) {
    let spot = popup::locate(app, anchor, Fallback::Pointer);
    answer_at(app, text, answer, note, spot, target);
}

fn answer_at(app: &AppHandle, text: String, answer: String, note: Option<&str>, spot: Spot, target: Target) {
    let note = note.map(str::to_string);
    if let Err(e) = open(app, "answer", text, answer, note, false, None, spot, target) {
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
    spot: Spot,
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
    // A panel still waiting for an answer is replaced: nobody will see that answer.
    cancel_running();
    *JOB.lock().unwrap() = Some(Job { view, reply, spot, target, focused: false, shown: false });
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
        .visible(false)
        // Windows: a webview built focused takes the focus while the window is still
        // hidden, and losing it again closes the job before the page has asked for it.
        .focused(cfg!(not(windows)));
    #[cfg(target_os = "macos")]
    let builder = builder.effects(tauri::utils::config::WindowEffectsConfig {
        effects: vec![tauri::window::Effect::Popover],
        state: Some(tauri::window::EffectState::Active),
        radius: Some(14.0),
        color: None,
        interactive: false,
    });
    let window = builder.build().map_err(|e| format!("Панель не открылась: {e}"))?;
    glass::fading(&window);
    Ok(window)
}

pub fn view() -> Option<View> {
    JOB.lock().unwrap().as_ref().map(|job| job.view.clone())
}

/// The page has laid itself out at this size (logical pixels): place and show.
pub fn fit(app: &AppHandle, seq: u64, height: f64) {
    let (spot, target, first) = {
        let mut job = JOB.lock().unwrap();
        let Some(job) = job.as_mut().filter(|job| job.view.seq == seq) else { return };
        let first = !job.shown;
        job.shown = true;
        (job.spot, job.target, first)
    };
    let Some(window) = app.get_webview_window(LABEL) else { return };
    let _ = window.set_size(LogicalSize::new(WIDTH, height));
    popup::put(app, &window, WIDTH, height, spot);
    glass::show(&window);
    // The page refits on every line typed; taking focus each time would pull the
    // app forward again and again.
    if !window.is_focused().unwrap_or(false) {
        let _ = window.set_focus();
    }
    if first {
        hotkeys::escape(app, true);
        // Closed while Esc was being taken: its close found nothing to give back.
        if !JOB.lock().unwrap().as_ref().is_some_and(|job| job.view.seq == seq) {
            hotkeys::escape(app, false);
            return;
        }
        watch(app, seq, target);
    }
}

/// Closes the panel the way a menu closes: when another window comes to the
/// front, when the user goes back to the app the text came from after being in
/// the panel, or at a click anywhere outside it. Focus events alone miss the
/// case where the panel never got the focus (Windows can refuse it).
fn watch(app: &AppHandle, seq: u64, target: Target) {
    let app = app.clone();
    thread::spawn(move || {
        let mut been_in_panel = false;
        let mut pressed = input::mouse_down();
        loop {
            thread::sleep(Duration::from_millis(25));
            if !JOB.lock().unwrap().as_ref().is_some_and(|job| job.view.seq == seq) {
                return;
            }
            let away = match input::front(target) {
                Some(Front::Us) => {
                    been_in_panel = true;
                    false
                }
                Some(Front::Target) => been_in_panel,
                Some(Front::Other) => true,
                None => false,
            };
            let down = input::mouse_down();
            let clicked_outside = down && !pressed && !pointer_inside(&app);
            pressed = down;
            if away || clicked_outside {
                close(&app, seq, false);
                return;
            }
        }
    });
}

/// True when unsure, so a click is never taken for one outside by mistake.
fn pointer_inside(app: &AppHandle) -> bool {
    let Some(window) = app.get_webview_window(LABEL) else { return true };
    let (Ok(at), Ok(size), Some((x, y))) =
        (window.outer_position(), window.outer_size(), popup::pointer(app))
    else {
        return true;
    };
    // macOS: the window at its own screen's scale, the pointer at the primary's;
    // both go back to points to compare.
    let div = if popup::POINTS { window.scale_factor().unwrap_or(1.0) } else { 1.0 };
    let (left, top) = (f64::from(at.x) / div, f64::from(at.y) / div);
    let (right, bottom) = (left + f64::from(size.width) / div, top + f64::from(size.height) / div);
    (left..right).contains(&x) && (top..bottom).contains(&y)
}

/// Esc, wherever the keyboard is: the page decides whether it closes a list or the panel.
pub fn escape(app: &AppHandle) {
    if JOB.lock().unwrap().is_some() {
        let _ = app.emit_to(LABEL, "panel-escape", ());
    }
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
    let (seq, text) = JOB
        .lock()
        .unwrap()
        .as_ref()
        .map(|job| (job.view.seq, job.view.text.clone()))
        .ok_or("Панель уже закрыта")?;
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
    let cancel = Arc::new(AtomicBool::new(false));
    if let Some(previous) = RUNNING.lock().unwrap().replace(cancel.clone()) {
        previous.store(true, Ordering::SeqCst);
    }
    // A close or "Back" before the flag was in place had nothing to cancel.
    if !JOB.lock().unwrap().as_ref().is_some_and(|job| job.view.seq == seq) {
        cancel_running();
        return Err("Панель уже закрыта".into());
    }
    let flag = cancel.clone();
    let result = tauri::async_runtime::spawn_blocking(move || {
        llm::transform_text(&app, &settings, &prompt, Some(&flag))
    })
    .await
    .map_err(|e| e.to_string());
    let mut running = RUNNING.lock().unwrap();
    if running.as_ref().is_some_and(|current| Arc::ptr_eq(current, &cancel)) {
        *running = None;
    }
    result?
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
/// `seq` is the panel the page shows; a newer one is left alone.
pub fn write(app: &AppHandle, seq: u64, text: String) {
    let Some(job) = take(seq) else { return };
    cancel_running();
    // While one of our windows is in front we may hand focus back; once it is
    // hidden, Windows would no longer let us.
    input::restore_focus(job.target);
    hide(app);
    if let Some(reply) = job.reply {
        let _ = reply.send(Reply::Write(text));
    }
}

/// Closes the panel if it still shows job `seq`: a late close (the page's timer
/// after Copy, a lost focus) must not take a panel that opened since. `restore`
/// gives focus back to the app the text came from (Esc); after a click
/// elsewhere the user is already where they want to be.
pub fn close(app: &AppHandle, seq: u64, restore: bool) {
    let Some(job) = take(seq) else { return };
    cancel_running();
    if restore {
        input::restore_focus(job.target);
    }
    hide(app);
    // Dropping the job drops its sender, which tells the hotkey's thread.
}

/// The window itself was closed (Alt+F4): whatever it shows goes.
pub fn close_any(app: &AppHandle) {
    let seq = JOB.lock().unwrap().as_ref().map(|job| job.view.seq);
    if let Some(seq) = seq {
        close(app, seq, true);
    }
}

fn take(seq: u64) -> Option<Job> {
    let mut job = JOB.lock().unwrap();
    job.as_ref().is_some_and(|job| job.view.seq == seq).then(|| job.take()).flatten()
}

fn hide(app: &AppHandle) {
    hotkeys::escape(app, false);
    if let Some(window) = app.get_webview_window(LABEL) {
        glass::hide(&window);
    }
}

/// The panel's window gained or lost focus. Only focus it had while shown
/// counts: a hidden window can get and lose it as it is made (Windows does).
pub fn focus_changed(app: &AppHandle, focused: bool) {
    let blurred = {
        let mut job = JOB.lock().unwrap();
        let Some(job) = job.as_mut() else { return };
        if focused && job.shown {
            job.focused = true;
        }
        (!focused && job.focused).then_some(job.view.seq)
    };
    if let Some(seq) = blurred {
        close(app, seq, false);
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
