//! Commands callable from the WebView. Keep this list in sync with build.rs and capabilities/.

use tauri::AppHandle;
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_opener::OpenerExt;

use crate::actions::{self, Failure, SubmitResult};
use crate::llm::Provider;
use crate::prompts::Mode;
use crate::{compose, hotkeys, languages, paths, prompts, secrets, settings, updates};

#[tauri::command]
pub fn has_api_key(provider: Provider) -> Result<bool, String> {
    secrets::has_api_key(provider).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_api_key(provider: Provider, key: String) -> Result<(), String> {
    secrets::set_api_key(provider, key).map_err(|e| e.to_string())?;
    tracing::info!(provider = provider.name(), "API key saved");
    Ok(())
}

#[tauri::command]
pub fn clear_api_key(provider: Provider) -> Result<(), String> {
    secrets::clear_api_key(provider).map_err(|e| e.to_string())?;
    tracing::info!(provider = provider.name(), "API key cleared");
    Ok(())
}

/// Settings `Test key`: one minimal request with the saved key. Returns only the mapped error.
#[tauri::command]
pub async fn test_api_key(app: AppHandle, provider: Provider) -> Result<(), String> {
    actions::test_api_key(&app, provider).await
}

#[tauri::command]
pub fn get_settings(app: AppHandle) -> Result<settings::Settings, String> {
    settings::load(&app, &paths::data_dir(&app)?)
}

/// Saves, then re-registers the hotkeys so changes apply without a restart.
/// Returns the per-hotkey outcome for the red marks in Settings.
#[tauri::command]
pub async fn save_settings(
    app: AppHandle,
    settings: settings::Settings,
) -> Result<Vec<hotkeys::HotkeyStatus>, String> {
    let dir = paths::data_dir(&app)?;
    let keys = settings.hotkeys.clone();
    settings::save(&app, &dir, settings)?;
    updates::refresh(&app);
    Ok(hotkeys::apply(&app, &keys, true))
}

#[tauri::command]
pub fn get_hotkey_status(app: AppHandle) -> Vec<hotkeys::HotkeyStatus> {
    hotkeys::status(&app)
}

#[tauri::command]
pub fn get_autostart(app: AppHandle) -> Result<bool, String> {
    app.autolaunch().is_enabled().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_autostart(app: AppHandle, enabled: bool) -> Result<(), String> {
    let launch = app.autolaunch();
    let result = if enabled { launch.enable() } else { launch.disable() };
    result.map_err(|e| e.to_string())
}

/// Opens one of three fixed sub-folders of the data directory. The WebView cannot pick a path.
#[tauri::command]
pub fn open_data_folder(app: AppHandle, sub: String) -> Result<(), String> {
    let name = match sub.as_str() {
        "prompts" => "prompts",
        "characters" => "characters",
        "logs" => "logs",
        _ => return Err("Unknown folder".into()),
    };
    let dir = paths::data_dir(&app)?.join(name);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    app.opener()
        .open_path(dir.to_string_lossy(), None::<&str>)
        .map_err(|e| e.to_string())
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameInfo {
    pub id: &'static str,
    pub display_name: &'static str,
    pub chat_limit: usize,
}

/// Read-only: lets Settings show the chat limit without hardcoding it.
#[tauri::command]
pub fn get_game_profile() -> GameInfo {
    let p = crate::game::active();
    GameInfo {
        id: p.id,
        display_name: p.display_name,
        chat_limit: p.chat_limit,
    }
}

/// The mode `Convert copied text` uses (the last one picked in the reply window).
#[tauri::command]
pub fn get_last_mode(app: AppHandle) -> Mode {
    paths::data_dir(&app)
        .map(|d| settings::load_last_mode(&app, &d))
        .unwrap_or(Mode::Direct)
}

/// Settings `Preview overlay`: shows a sample with the unsaved form values. No request.
#[tauri::command]
pub fn preview_overlay(app: AppHandle, overlay: settings::OverlaySettings, user_language: String) {
    crate::overlay::preview(&app, overlay, &user_language);
}

/// Hides the Settings window. Closing it asks the form first (`settings://close-requested`),
/// then the form calls this once there is nothing left to save or the user chose.
#[tauri::command]
pub fn close_settings(app: AppHandle) {
    use tauri::Manager;
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.hide();
    }
}

/// The `Your language` dropdown entries, in table order.
#[tauri::command]
pub fn list_languages() -> Vec<languages::LanguageOption> {
    languages::options()
}

/// Prompt files that were edited and do not use `{{user_language}}` (names only).
#[tauri::command]
pub fn get_prompt_language_notes(app: AppHandle) -> Result<Vec<String>, String> {
    let dir = paths::data_dir(&app)?;
    Ok(prompts::files_without_language(&dir)
        .into_iter()
        .map(str::to_owned)
        .collect())
}

// ---- compose window ----

#[derive(serde::Serialize)]
pub struct ComposeInfo {
    pub mode: Mode,
}

#[tauri::command]
pub fn get_compose_state(app: AppHandle) -> ComposeInfo {
    let mode = paths::data_dir(&app)
        .map(|d| settings::load_last_mode(&app, &d))
        .unwrap_or(Mode::Direct);
    ComposeInfo { mode }
}

#[tauri::command]
pub async fn compose_submit(
    app: AppHandle,
    text: String,
    mode: Mode,
) -> Result<SubmitResult, Failure> {
    actions::compose_submit(&app, &text, mode).await
}

/// Copies the result the Output guard blocked. Takes no text: it can only copy what Rust holds.
#[tauri::command]
pub fn compose_copy_anyway(app: AppHandle) -> Result<(), String> {
    actions::compose_copy_anyway(&app)
}

#[tauri::command]
pub fn compose_cancel(app: AppHandle) {
    compose::close(&app);
}

// ---- updates ----

#[tauri::command]
pub fn get_update_status(app: AppHandle) -> updates::UpdateStatus {
    updates::status(&app)
}

#[tauri::command]
pub async fn check_for_updates_now(app: AppHandle) -> updates::UpdateStatus {
    updates::check_now(&app).await
}

/// Takes no argument: Rust opens one fixed releases URL.
#[tauri::command]
pub fn open_releases_page(app: AppHandle) -> Result<(), String> {
    updates::open_releases_page(&app)
}
