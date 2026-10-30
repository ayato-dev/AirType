// Declaring the commands makes each one denied unless a capability allows it,
// so every window gets only what its page calls (see capabilities/).
const SETTINGS: &[&str] = &[
    "get_state",
    "save_settings",
    "set_api_key",
    "test_connection",
    "suspend_hotkeys",
    "set_autostart",
    "request_accessibility",
    "accessibility_status",
    "hide_settings",
    "local_status",
    "local_download",
    "local_cancel",
    "local_delete",
];
const HUD: &[&str] = &["hud_fit"];
const PANEL: &[&str] =
    &["panel_view", "panel_fit", "panel_run", "panel_cancel", "panel_write", "panel_copy", "panel_close"];

fn main() {
    let commands: &'static [&'static str] = [SETTINGS, HUD, PANEL].concat().leak();
    let attributes =
        tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(commands));
    tauri_build::try_build(attributes).expect("failed to run tauri-build");
}
