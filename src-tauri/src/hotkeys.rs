//! Global hotkeys. Only `tauri-plugin-global-shortcut` (RegisterHotKey) is used; never a
//! low-level keyboard hook.

use std::sync::Mutex;
use std::time::Instant;

use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use tauri_plugin_notification::NotificationExt;

use crate::overlay;
use crate::settings::Hotkeys;
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
pub struct Registered {
    map: Mutex<Vec<(Shortcut, Action)>>,
    status: Mutex<Vec<HotkeyStatus>>,
}

pub fn plugin() -> tauri::plugin::TauriPlugin<Wry> {
    tauri_plugin_global_shortcut::Builder::new()
        .with_handler(|app, shortcut, event| {
            // Only the key-down edge; key-up must not trigger anything.
            if event.state() != ShortcutState::Pressed {
                return;
            }
            let action = app
                .state::<Registered>()
                .map
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

/// Result of registering one hotkey; shown in Settings (red when `ok` is false).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyStatus {
    /// Field name in `Hotkeys` (`translate`, `nextPart`, ...).
    pub action: String,
    pub ok: bool,
    pub error: Option<String>,
}

fn try_register(app: &AppHandle, text: &str, taken: &[Shortcut]) -> Result<Shortcut, String> {
    let shortcut = text
        .parse::<Shortcut>()
        .map_err(|_| "Not a valid key combination".to_string())?;
    if shortcut.mods.is_empty() {
        return Err("Needs at least one modifier (Ctrl, Alt, Shift)".into());
    }
    if taken.contains(&shortcut) {
        return Err("Same as another hotkey".into());
    }
    app.global_shortcut().register(shortcut).map_err(|e| {
        tracing::warn!("hotkey {text} not registered: {e}");
        "Used by another app".to_string()
    })?;
    Ok(shortcut)
}

/// Drop every registered hotkey and register `keys` from scratch. A failure does not stop the
/// others. Failures are returned, logged, and (when `notify`) shown as a tray notification.
pub fn apply(app: &AppHandle, keys: &Hotkeys, notify: bool) -> Vec<HotkeyStatus> {
    let wanted = [
        ("translate", &keys.translate, Action::Translate),
        ("compose", &keys.compose, Action::Compose),
        ("quick", &keys.quick, Action::Quick),
        ("nextPart", &keys.next_part, Action::NextPart),
        ("overlayToggle", &keys.overlay_toggle, Action::ToggleOverlay),
    ];
    let _ = app.global_shortcut().unregister_all();
    let state = app.state::<Registered>();
    let mut active: Vec<(Shortcut, Action)> = Vec::new();
    let mut statuses = Vec::new();
    let mut failed = Vec::new();
    for (name, text, action) in wanted {
        let taken: Vec<Shortcut> = active.iter().map(|(s, _)| *s).collect();
        match try_register(app, text, &taken) {
            Ok(s) => {
                tracing::info!("hotkey {text} registered");
                active.push((s, action));
                statuses.push(HotkeyStatus { action: name.into(), ok: true, error: None });
            }
            Err(e) => {
                failed.push(text.clone());
                statuses.push(HotkeyStatus { action: name.into(), ok: false, error: Some(e) });
            }
        }
    }
    *state.map.lock().unwrap_or_else(|e| e.into_inner()) = active;
    *state.status.lock().unwrap_or_else(|e| e.into_inner()) = statuses.clone();
    if notify && !failed.is_empty() {
        let app = app.clone();
        let body = format!("Hotkey not available: {}", failed.join(", "));
        // At startup this runs inside `setup`, before the event loop is up; show it a moment later.
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            let _ = app
                .notification()
                .builder()
                .title(crate::APP_NAME)
                .body(body)
                .show();
        });
    }
    statuses
}

pub fn register_all(app: &AppHandle) {
    let cfg = paths::data_dir(app)
        .and_then(|d| settings::load(app, &d))
        .unwrap_or_default();
    apply(app, &cfg.hotkeys, true);
}

pub fn status(app: &AppHandle) -> Vec<HotkeyStatus> {
    app.state::<Registered>()
        .status
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_hotkeys_parse_with_modifiers() {
        let k = Hotkeys::default();
        for t in [&k.translate, &k.compose, &k.quick, &k.next_part, &k.overlay_toggle] {
            let s: Shortcut = t.parse().unwrap();
            assert!(!s.mods.is_empty());
        }
    }

    #[test]
    fn bare_key_has_no_modifier() {
        let s: Shortcut = "T".parse().unwrap();
        assert!(s.mods.is_empty());
    }
}
