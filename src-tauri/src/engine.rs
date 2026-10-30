//! One run of an action: take the text from the focused field, send it to the
//! model, put the answer back.
//!
//! The text is read through the accessibility API when the app supports it and
//! with Ctrl/Cmd+C otherwise; then the clipboard goes back at once. A one-line
//! answer is typed in as keystrokes and never touches the clipboard; a longer one
//! is pasted, and the clipboard comes back as soon as the app has read it. When
//! the answer can't go in (a web page, a PDF), the panel shows it instead.

use crate::{
    clipboard::{self, Pasted, Saved},
    config::{self, Provider, Settings},
    field::{self, Focused, Replaced, Snapshot},
    hud,
    input::{self, Chord, Target},
    llm, local, panel,
    popup::Rect,
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
        hud::set_anchor(None);
        match run(&app, &action_id) {
            Ok(Ran::Written) if show_hud => hud::done(&app),
            Ok(Ran::Written) => {}
            Ok(Ran::Shown) => hud::hide(&app),
            Err(message) => hud::error(&app, &message),
        }
        app.state::<AppState>().busy.store(false, Ordering::SeqCst);
    });
}

enum Ran {
    Written,
    /// The answer is in the panel.
    Shown,
}

fn run(app: &AppHandle, action_id: &str) -> Result<Ran, String> {
    let settings = app.state::<AppState>().settings();
    let action = settings.actions.iter().find(|a| a.id == action_id).cloned().ok_or("Действие не найдено")?;
    preflight(app, &settings)?;

    let cx = context();
    hud::set_anchor(cx.anchor);
    let source = read(Scope::Field, cx.terminal)?.ok_or(NOTHING_FOUND)?;
    if settings.show_hud {
        hud::working(app, &action.id, &action.name);
    }
    let prompt = llm::action_prompt(&settings, &action, &source.text);
    let output = llm::transform_text(app, &settings, &prompt, None)?;

    if source.editable == Some(false) {
        panel::show_answer(app, source.text, output, None, cx.anchor, cx.target);
        return Ok(Ran::Shown);
    }
    match write(&source, &output, cx.target)? {
        Written::Done => Ok(Ran::Written),
        Written::Missed(why) => {
            panel::show_answer(app, source.text, output, Some(why), cx.anchor, cx.target);
            Ok(Ran::Shown)
        }
    }
}

/// What can fail before anything is touched: no key, no model, no permission.
pub fn preflight(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    if !input::SUPPORTED {
        return Err("Эта система пока не поддерживается".into());
    }
    if settings.provider == Provider::Local {
        if !local::model(&settings.local.model).is_some_and(|m| local::downloaded(app, m)) {
            tray::show_settings(app);
            return Err("Сначала скачай модель в настройках".into());
        }
    } else if settings.requires_key() && config::api_key(settings.provider).is_none() {
        tray::show_settings(app);
        return Err(llm::NO_KEY.into());
    }
    if !input::accessibility_granted(false) {
        input::accessibility_granted(true);
        return Err("Дай Airtype «Универсальный доступ» в настройках macOS".into());
    }
    Ok(())
}

/// Where the user is when the hotkey fires.
pub struct Context {
    pub target: Target,
    pub terminal: bool,
    /// The selection, caret or field on screen, for the HUD and the panel to stand next to.
    pub anchor: Option<Rect>,
}

pub fn context() -> Context {
    input::wait_modifiers_released(Duration::from_millis(1500));
    Context { target: input::foreground(), terminal: input::terminal_focused(), anchor: field::anchor() }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// The selection, or the whole field when nothing is selected: what an action works on.
    Field,
    /// Only a selection. Nothing selected is not an error, there is just nothing to read.
    Selection,
}

/// Where the text came from, and so how the answer goes back.
pub struct Source {
    pub text: String,
    snapshot: Option<Snapshot>,
    /// The text is the whole field and it isn't selected: select it before writing.
    select_all: bool,
    pub editable: Option<bool>,
}

impl Source {
    /// Nothing was read; an answer goes in at the caret.
    pub fn caret() -> Self {
        Source { text: String::new(), snapshot: None, select_all: false, editable: None }
    }
}

const PASSWORD: &str = "Это поле для пароля, его не трогаю";
const SELECT_IN_TERMINAL: &str = "В терминале сначала выдели текст";
const NOTHING_FOUND: &str = "Не нашёл текст. Выдели его или поставь курсор в поле";
const SELECT_FIRST: &str = "Выдели текст, с которым работать";

pub fn read(scope: Scope, terminal: bool) -> Result<Option<Source>, String> {
    let selection_only = scope == Scope::Selection;
    let (select_all, editable) = match field::focused() {
        Focused::Password => return Err(PASSWORD.into()),
        Focused::Text(snapshot) if snapshot.whole && selection_only => return Ok(None),
        // A terminal's "field" is its whole scrollback; never send or replace that.
        Focused::Text(snapshot) if snapshot.whole && terminal => return Err(SELECT_IN_TERMINAL.into()),
        Focused::Text(snapshot) if !snapshot.text.trim().is_empty() => {
            return Ok(Some(Source {
                text: snapshot.text.clone(),
                select_all: snapshot.whole,
                editable: snapshot.editable,
                snapshot: Some(snapshot),
            }));
        }
        // Empty as the app reports it; some apps just don't report the text.
        Focused::Text(snapshot) => (true, snapshot.editable),
        Focused::Unknown { select_all, editable } => (select_all, editable),
    };
    // Ctrl+C in a Windows console with nothing selected interrupts the running program.
    if terminal && cfg!(windows) {
        return if selection_only { Ok(None) } else { Err(SELECT_IN_TERMINAL.into()) };
    }
    let select_all = select_all && !terminal && !selection_only;
    match copy(select_all)? {
        // Whatever Ctrl+C took is still selected, so the answer replaces just that.
        Some(text) => Ok(Some(Source { text, snapshot: None, select_all: false, editable })),
        None if selection_only => Ok(None),
        None => Err(if select_all { NOTHING_FOUND } else { SELECT_FIRST }.into()),
    }
}

/// Copies the selection (with `select_all`, the whole field) and gives the
/// clipboard back straight away, before the model is even asked.
fn copy(select_all: bool) -> Result<Option<String>, String> {
    let saved = Saved::capture().ok_or(clipboard::BUSY)?;
    let text = match Clipboard::new() {
        Ok(mut clipboard) => Ok(grab_text(&mut clipboard, select_all)),
        Err(e) => Err(format!("Буфер обмена недоступен: {e}")),
    };
    saved.restore();
    text
}

pub enum Written {
    Done,
    /// The answer couldn't go in, and this is why.
    Missed(&'static str),
}

const WINDOW_CHANGED: &str = "Пока нейронка думала, открылось другое окно, поэтому ответ здесь";
const CHANGED: &str = "Текст поменялся, пока нейронка думала, поэтому ответ здесь";
const NOWHERE: &str = "Тут не нашлось поля для ответа, поэтому он здесь";

/// Puts the answer where the text came from.
pub fn write(source: &Source, output: &str, target: Target) -> Result<Written, String> {
    if !input::return_to(target) {
        return Ok(Written::Missed(WINDOW_CHANGED));
    }
    if let Some(snapshot) = &source.snapshot {
        match snapshot.replace(output) {
            Replaced::Done => return Ok(Written::Done),
            Replaced::Changed => return Ok(Written::Missed(CHANGED)),
            Replaced::Unsupported => {}
        }
    }
    if source.select_all {
        input::press(Chord::SelectAll);
        thread::sleep(Duration::from_millis(60));
    }
    if input::type_text(output) {
        return Ok(Written::Done);
    }
    match clipboard::paste(output, || input::press(Chord::Paste))? {
        Pasted::Missed => Ok(Written::Missed(NOWHERE)),
        Pasted::Taken | Pasted::Assumed => Ok(Written::Done),
    }
}

/// Copies the selection; with nothing selected, takes the whole field when
/// `select_all` says the focus is a field and not a page or a list.
fn grab_text(clipboard: &mut Clipboard, select_all: bool) -> Option<String> {
    let _ = clipboard.clear();
    input::press(Chord::Copy);
    if let Some(text) = wait_for_text(clipboard, Duration::from_millis(400)) {
        return Some(text);
    }
    if !select_all {
        return None;
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
