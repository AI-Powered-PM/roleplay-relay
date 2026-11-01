#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod actions;
mod commands;
mod compose;
mod context;
mod game;
mod guard;
mod hotkeys;
mod languages;
mod llm;
mod logging;
mod outguard;
mod overlay;
mod parse;
mod paths;
mod prompts;
mod secrets;
mod settings;
mod split;
mod updates;

/// Product name: tray tooltip, notification titles, error text. Window titles and the installer
/// name live in `tauri.conf.json`.
pub const APP_NAME: &str = "Roleplay Relay";
/// Folder under `%APPDATA%` that holds settings, prompts, characters and logs.
pub const DATA_DIR_NAME: &str = "RoleplayRelay";

use tauri::menu::{CheckMenuItemBuilder, Menu, MenuBuilder, MenuItemBuilder, SubmenuBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, WindowEvent, Wry};

fn show_settings(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

const TRAY_ID: &str = "main";

/// The tray menu; `update` is the newer version to offer, if any.
fn tray_menu(app: &AppHandle, update: Option<&str>) -> tauri::Result<Menu<Wry>> {
    let overlay = MenuItemBuilder::with_id("overlay", "Show/Hide overlay").build(app)?;
    let default_char = CheckMenuItemBuilder::with_id("char-default", "default")
        .checked(true)
        .enabled(false)
        .build(app)?;
    let characters = SubmenuBuilder::new(app, "Character")
        .item(&default_char)
        .build()?;
    let settings = MenuItemBuilder::with_id("settings", "Settings").build(app)?;
    let quit = MenuItemBuilder::with_id("quit", "Quit").build(app)?;

    let mut menu = MenuBuilder::new(app);
    if let Some(latest) = update {
        let download =
            MenuItemBuilder::with_id("download-update", format!("Download version {latest}"))
                .build(app)?;
        menu = menu.item(&download).separator();
    }
    menu.item(&overlay)
        .item(&characters)
        .item(&settings)
        .separator()
        .item(&quit)
        .build()
}

fn build_tray(app: &tauri::App) -> tauri::Result<()> {
    let menu = tray_menu(app.handle(), None)?;
    let mut tray = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip(APP_NAME)
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "overlay" => overlay::toggle(app),
            "settings" => show_settings(app),
            "download-update" => {
                if let Err(e) = updates::open_releases_page(app) {
                    tracing::warn!("releases page not opened: {e}");
                }
            }
            "quit" => app.exit(0),
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

fn main() {
    tauri::Builder::default()
        // Must be registered first: a second launch just opens Settings in the running app.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_settings(app);
        }))
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(hotkeys::plugin())
        .manage(actions::AppState::new())
        .manage(overlay::OverlayState::default())
        .manage(compose::ComposeState::default())
        .manage(hotkeys::Registered::default())
        // Settings is created from tauri.conf.json before `setup` runs and asks for the update
        // status on mount, so the state must exist before any window does.
        .manage(updates::UpdateState::default())
        .invoke_handler(tauri::generate_handler![
            commands::has_api_key,
            commands::set_api_key,
            commands::clear_api_key,
            commands::get_settings,
            commands::save_settings,
            commands::open_data_folder,
            commands::get_hotkey_status,
            commands::get_autostart,
            commands::set_autostart,
            commands::test_api_key,
            commands::get_game_profile,
            commands::get_last_mode,
            commands::preview_overlay,
            commands::close_settings,
            commands::list_languages,
            commands::get_prompt_language_notes,
            commands::get_compose_state,
            commands::compose_submit,
            commands::compose_copy_anyway,
            commands::compose_cancel,
            commands::get_update_status,
            commands::check_for_updates_now,
            commands::open_releases_page,
        ])
        .setup(|app| {
            let handle = app.handle();
            let data = paths::data_dir(handle)?;
            std::fs::create_dir_all(&data)?;
            if let Err(e) = logging::init(&data.join("logs")) {
                eprintln!("logging disabled: {e}");
            }
            logging::install_panic_hook();
            // The running app's own version (tauri.conf.json), never hardcoded.
            app.state::<updates::UpdateState>()
                .set_current_version(&app.package_info().version.to_string());
            if let Err(e) = paths::ensure_layout(handle, &data) {
                tracing::error!("could not prepare data folder: {e}");
            }
            // Write defaults on first run so settings.json exists and can be inspected.
            if let Ok(s) = settings::load(handle, &data) {
                let _ = settings::save(handle, &data, s);
            }
            build_tray(app)?;
            overlay::init(handle);
            hotkeys::register_all(handle);
            updates::start(handle);
            let profile = game::active();
            tracing::info!("started, game profile {} ({})", profile.id, profile.display_name);
            Ok(())
        })
        .on_window_event(|window, event| {
            // Alt+F4 on compose must not destroy it: close it the normal way (focus goes back).
            if window.label() == "compose" {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    compose::close(window.app_handle());
                }
            }
            // Closing Settings never destroys it (the app lives in the tray). The form decides:
            // it hides the window at once, or asks `Save changes?` first.
            if window.label() == "settings" {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    use tauri::Emitter;
                    if window
                        .emit_to("settings", "settings://close-requested", ())
                        .is_err()
                    {
                        let _ = window.hide();
                    }
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running Roleplay Relay");
}
