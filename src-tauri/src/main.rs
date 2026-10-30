#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod logging;
mod paths;
mod secrets;
mod settings;

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
    // Wired in milestone 2, when the overlay exists.
    let overlay = MenuItemBuilder::with_id("overlay", "Show/Hide overlay")
        .enabled(false)
        .build(app)?;
    let default_char = CheckMenuItemBuilder::with_id("char-default", "default")
        .checked(true)
        .enabled(false)
        .build(app)?;
    let characters = SubmenuBuilder::new(app, "Character").item(&default_char).build()?;
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
        .invoke_handler(tauri::generate_handler![
            commands::has_api_key,
            commands::set_api_key,
            commands::clear_api_key,
            commands::get_settings,
            commands::save_settings,
            commands::open_data_folder,
        ])
        .setup(|app| {
            let handle = app.handle();
            let data = paths::data_dir(handle)?;
            std::fs::create_dir_all(&data)?;
            if let Some(guard) = logging::init(&data.join("logs")) {
                app.manage(guard);
            }
            if let Err(e) = paths::ensure_layout(handle, &data) {
                tracing::error!("could not prepare data folder: {e}");
            }
            // Write defaults on first run so settings.json exists and can be inspected.
            if let Ok(s) = settings::load(handle, &data) {
                let _ = settings::save(handle, &data, s);
            }
            build_tray(app)?;
            tracing::info!("started");
            Ok(())
        })
        .on_window_event(|window, event| {
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
