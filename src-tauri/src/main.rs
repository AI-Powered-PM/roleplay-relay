#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod actions;
mod claude;
mod commands;
mod compose;
mod context;
mod guard;
mod hotkeys;
mod logging;
mod outguard;
mod overlay;
mod parse;
mod paths;
mod prompts;
mod secrets;
mod settings;
mod split;

use tauri::menu::{CheckMenuItemBuilder, MenuBuilder, MenuItemBuilder, SubmenuBuilder};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, WindowEvent};

fn show_settings(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

fn build_tray(app: &tauri::App) -> tauri::Result<()> {
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

    let menu = MenuBuilder::new(app)
        .item(&overlay)
        .item(&characters)
        .item(&settings)
        .separator()
        .item(&quit)
        .build()?;

    let mut tray = TrayIconBuilder::new()
        .tooltip("WoW Chat Helper")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "overlay" => overlay::toggle(app),
            "settings" => show_settings(app),
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
        .plugin(hotkeys::plugin())
        .manage(actions::AppState::new())
        .manage(overlay::OverlayState::default())
        .manage(compose::ComposeState::default())
        .manage(hotkeys::Registered::default())
        .invoke_handler(tauri::generate_handler![
            commands::has_api_key,
            commands::set_api_key,
            commands::clear_api_key,
            commands::get_settings,
            commands::save_settings,
            commands::open_data_folder,
            commands::get_compose_state,
            commands::compose_submit,
            commands::compose_copy_anyway,
            commands::compose_cancel,
        ])
        .setup(|app| {
            let handle = app.handle();
            let data = paths::data_dir(handle)?;
            std::fs::create_dir_all(&data)?;
            if let Err(e) = logging::init(&data.join("logs")) {
                eprintln!("logging disabled: {e}");
            }
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
            tracing::info!("started");
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
            // Closing Settings hides it; the app lives in the tray.
            if window.label() == "settings" {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running WoW Chat Helper");
}
