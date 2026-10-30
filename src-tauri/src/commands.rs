//! Commands callable from the WebView. Keep this list in sync with build.rs and capabilities/.

use tauri::AppHandle;
use tauri_plugin_opener::OpenerExt;

use crate::{paths, secrets, settings};

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
