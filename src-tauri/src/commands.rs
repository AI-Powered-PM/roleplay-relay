//! Commands callable from the WebView. Keep this list in sync with build.rs and capabilities/.

use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

use crate::actions::{self, Failure, SubmitResult};
use crate::prompts::Mode;
use crate::{compose, paths, secrets, settings};

#[tauri::command]
pub fn has_api_key() -> Result<bool, String> {
    secrets::has_api_key().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_api_key(key: String) -> Result<(), String> {
    secrets::set_api_key(key).map_err(|e| e.to_string())?;
    tracing::info!("API key saved");
    Ok(())
}

#[tauri::command]
pub fn clear_api_key() -> Result<(), String> {
    secrets::clear_api_key().map_err(|e| e.to_string())?;
    tracing::info!("API key cleared");
    Ok(())
}

#[tauri::command]
pub fn get_settings(app: AppHandle) -> Result<settings::Settings, String> {
    settings::load(&app, &paths::data_dir(&app)?)
}

#[tauri::command]
pub fn save_settings(app: AppHandle, settings: settings::Settings) -> Result<(), String> {
    settings::save(&app, &paths::data_dir(&app)?, settings)
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
