//! Updates from the releases. The app looks for one a little after start and
//! then twice a day; a newer version shows up in the tray menu and the settings,
//! and installs when the user asks. Every update is signed, and the updater
//! checks it against the public key the app ships with.

use crate::{local, tray};
use serde::Serialize;
use std::{sync::Mutex, thread, time::Duration};
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::{Updater, UpdaterExt};

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    /// This install can update itself.
    supported: bool,
    /// The newer version, once a check found one.
    available: Option<String>,
    /// `idle`, `checking`, `downloading`, `installing` or `error`.
    stage: &'static str,
    done: u64,
    total: u64,
    message: Option<String>,
}

static STATUS: Mutex<Status> =
    Mutex::new(Status { supported: false, available: None, stage: "idle", done: 0, total: 0, message: None });

pub fn status() -> Status {
    Status { supported: supported(), ..STATUS.lock().unwrap().clone() }
}

/// The updater replaces the AppImage or the app bundle, or runs the Windows
/// installer. A .deb install gets a new .deb instead.
fn supported() -> bool {
    !cfg!(target_os = "linux") || std::env::var_os("APPIMAGE").is_some()
}

fn set(app: &AppHandle, change: impl FnOnce(&mut Status)) {
    let status = {
        let mut status = STATUS.lock().unwrap();
        change(&mut status);
        Status { supported: supported(), ..status.clone() }
    };
    let _ = app.emit("update", status);
}

fn updater(app: &AppHandle) -> Result<Updater, String> {
    app.updater_builder()
        // Windows: the installer runs and the app exits on the spot, past the
        // exit event; the local model's server goes first.
        .on_before_exit(local::shutdown)
        .build()
        .map_err(|e| e.to_string())
}

/// Moves to `stage` unless a check or an install is already under way; one
/// lock for both, so two clicks (or a click and the timer) can't both start.
fn begin(app: &AppHandle, stage: &'static str) -> bool {
    let status = {
        let mut status = STATUS.lock().unwrap();
        if matches!(status.stage, "checking" | "downloading" | "installing") {
            return false;
        }
        status.stage = stage;
        status.message = None;
        if stage == "downloading" {
            status.done = 0;
            status.total = 0;
        }
        Status { supported: supported(), ..status.clone() }
    };
    let _ = app.emit("update", status);
    true
}

/// Checks in the background: soon after start, then every 12 hours.
pub fn watch(app: &AppHandle) {
    if !supported() {
        return;
    }
    let app = app.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_secs(30));
        loop {
            // Offline or GitHub down: say nothing, the next round will tell.
            let _ = tauri::async_runtime::block_on(check(&app, false));
            thread::sleep(Duration::from_secs(12 * 60 * 60));
        }
    });
}

/// Looks for a newer version. `asked`: the user pressed the button, so a
/// failure is worth showing.
pub async fn check(app: &AppHandle, asked: bool) -> Result<Option<String>, String> {
    if !begin(app, "checking") {
        return Ok(status().available);
    }
    let found = match updater(app) {
        Ok(updater) => updater.check().await.map_err(|e| e.to_string()),
        Err(e) => Err(e),
    };
    match found {
        Ok(update) => {
            let version = update.map(|u| u.version);
            set(app, |s| {
                s.stage = "idle";
                s.available = version.clone();
            });
            tray::set_update(app, version.as_deref());
            Ok(version)
        }
        Err(e) => {
            let message = format!("Не проверилось: {e}");
            set(app, |s| {
                s.stage = if asked { "error" } else { "idle" };
                s.message = asked.then(|| message.clone());
            });
            Err(message)
        }
    }
}

/// Downloads and installs the update, then starts the new version. On Windows
/// the installer takes over and closes the app itself.
pub fn install(app: &AppHandle) {
    if !begin(app, "downloading") {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = download_and_install(&app).await {
            set(&app, |s| {
                s.stage = "error";
                s.message = Some(format!("Не обновилось: {e}"));
            });
        }
    });
}

async fn download_and_install(app: &AppHandle) -> Result<(), String> {
    let update =
        updater(app)?.check().await.map_err(|e| e.to_string())?.ok_or("уже стоит последняя версия")?;
    let mut done = 0u64;
    let mut shown = 0u64;
    update
        .download_and_install(
            |chunk, total| {
                done += chunk as u64;
                // A progress event per percent is plenty for the page.
                let total = total.unwrap_or(0);
                let step = if total == 0 { 1 << 20 } else { total / 100 };
                if done - shown >= step || done == total {
                    shown = done;
                    set(app, |s| {
                        s.done = done;
                        s.total = total;
                    });
                }
            },
            || set(app, |s| s.stage = "installing"),
        )
        .await
        .map_err(|e| e.to_string())?;
    // Goes through the exit event, which stops the local model's server too.
    app.request_restart();
    Ok(())
}
