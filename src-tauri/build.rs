use tauri_build::{AppManifest, Attributes};

// Listing the app commands here makes Tauri generate an `allow-<command>` permission
// for each one, so every window only gets the commands its capability names.
const COMMANDS: &[&str] = &[
    "has_api_key",
    "set_api_key",
    "clear_api_key",
    "get_settings",
    "save_settings",
    "open_data_folder",
    "get_hotkey_status",
    "get_autostart",
    "set_autostart",
    "test_api_key",
    "get_game_profile",
    "get_last_mode",
    "preview_overlay",
    "close_settings",
    "list_languages",
    "get_prompt_language_notes",
    "get_compose_state",
    "compose_submit",
    "compose_copy_anyway",
    "compose_cancel",
];

fn main() {
    tauri_build::try_build(Attributes::new().app_manifest(AppManifest::new().commands(COMMANDS)))
        .expect("failed to run tauri-build");
}
