//! Global hotkeys. Only `tauri-plugin-global-shortcut` (RegisterHotKey) is used; never a
//! low-level keyboard hook.

use std::sync::Mutex;
use std::time::Instant;

use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::overlay::{self, Kind};
use crate::{actions, compose, paths, settings};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Translate,
    Compose,
    Quick,
    NextPart,
    ToggleOverlay,
}

#[derive(Default)]
pub struct Registered(Mutex<Vec<(Shortcut, Action)>>);

pub fn plugin() -> tauri::plugin::TauriPlugin<Wry> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, shortcut, event| {
            // Only the key-down edge; key-up must not trigger anything.
            if event.state() != ShortcutState::Pressed {
                return;
            }
            let action = app
                .state::<Registered>()
                .0
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .find(|(s, _)| s == shortcut)
                .map(|(_, a)| *a);
            match action {
                Some(Action::Translate) => actions::translate_incoming(app),
                Some(Action::Compose) => {
                    // A held key auto-repeats; open once per press.
                    let state = app.state::<actions::AppState>();
                    if state.limiter.debounce("compose-open", Instant::now()) {
                        compose::open(app);
                    }
                }
                Some(Action::Quick) => actions::quick_clipboard(app),
                Some(Action::NextPart) => actions::next_part(app),
                Some(Action::ToggleOverlay) => {
                    // Auto-repeat while the keys are held would flip the overlay on and off.
                    let state = app.state::<actions::AppState>();
                    if state.limiter.debounce("toggle", Instant::now()) {
                        overlay::toggle(app);
                    }
                }
                None => {}
            }
        })
        .build()
}

/// Register the configured hotkeys. A failure (combo taken by another app,
/// or unparseable) is logged and shown in the overlay; the rest still register.
pub fn register_all(app: &AppHandle) {
    let cfg = paths::data_dir(app)
        .and_then(|d| settings::load(app, &d))
        .unwrap_or_default();
    let wanted = [
        (cfg.hotkeys.translate, Action::Translate),
        (cfg.hotkeys.compose, Action::Compose),
        (cfg.hotkeys.quick, Action::Quick),
        (cfg.hotkeys.next_part, Action::NextPart),
        (cfg.hotkeys.overlay_toggle, Action::ToggleOverlay),
    ];

    let registered = app.state::<Registered>();
    let mut failures = Vec::new();
    for (text, action) in wanted {
        let outcome = text
            .parse::<Shortcut>()
            .map_err(|e| e.to_string())
            .and_then(|s| {
                app.global_shortcut()
                    .register(s)
                    .map(|_| s)
                    .map_err(|e| e.to_string())
            });
        match outcome {
            Ok(shortcut) => {
                tracing::info!("hotkey {text} registered");
                registered
                    .0
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push((shortcut, action));
            }
            Err(e) => {
                tracing::warn!("hotkey {text} not registered: {e}");
                failures.push(text);
            }
        }
    }
    if !failures.is_empty() {
        // `register_all` runs inside `setup`, before the event loop is up. Touching windows
        // from here can block, so the message is shown from a task a moment later.
        let app = app.clone();
        let message = format!(
            "Hotkey not available (used by another app?): {}",
            failures.join(", ")
        );
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            overlay::push(&app, Kind::Error, None, &message);
        });
    }
}
