//! The overlay window: shown without activating, click-through, auto-hiding.
//! It receives everything through the `overlay://update` event; it has no commands.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewWindow};

use crate::{paths, settings};

pub const LABEL: &str = "overlay";
const EVENT: &str = "overlay://update";
const HEIGHT_LOGICAL: f64 = 420.0;
const MARGIN_LOGICAL: f64 = 24.0;

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Busy,
    Result,
    Error,
    Info,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Style {
    pub font_size: u32,
    pub opacity: f32,
    pub position: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Update {
    pub kind: Kind,
    /// Source text, shown dimmed and clamped to two lines.
    pub original: Option<String>,
    pub text: String,
    /// Russian back-translation of an outgoing line, shown under it.
    pub back: Option<String>,
    /// Short line above everything else, e.g. "Copied to clipboard".
    pub note: Option<String>,
    pub style: Style,
}

#[derive(Default)]
pub struct OverlayState {
    /// Bumped on every update/hide; an auto-hide timer only fires if its generation is current.
    generation: AtomicU64,
    /// Shown by the toggle: stays until toggled off.
    pinned: AtomicBool,
    /// Our own record of visibility. The window is shown and hidden through raw Win32 calls,
    /// so Tauri's cached visible flag cannot be trusted.
    shown: AtomicBool,
    last: Mutex<Option<Update>>,
}

/// One-time window setup. Must run on startup, before the first show.
pub fn init(app: &AppHandle) {
    let Some(w) = app.get_webview_window(LABEL) else {
        return;
    };
    let _ = w.set_ignore_cursor_events(true);
    #[cfg(windows)]
    platform::make_non_activating(&w);
}

fn truncate(s: &str, max: usize) -> String {
    let flat: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= max {
        flat
    } else {
        let cut: String = flat.chars().take(max).collect();
        format!("{}…", cut.trim_end())
    }
}

fn style_from(s: &settings::Settings) -> Style {
    Style {
        font_size: s.overlay.font_size,
        opacity: s.overlay.opacity,
        position: s.overlay.position.clone(),
    }
}

fn load_settings(app: &AppHandle) -> settings::Settings {
    paths::data_dir(app)
        .and_then(|d| settings::load(app, &d))
        .unwrap_or_default()
}

/// Show an update. Never activates the window.
pub fn push(app: &AppHandle, kind: Kind, original: Option<&str>, text: &str) {
    let s = load_settings(app);
    push_update(
        app,
        Update {
            kind,
            original: original.map(|o| truncate(o, 240)),
            text: text.to_owned(),
            back: None,
            note: None,
            style: style_from(&s),
        },
    );
}

/// An outgoing line that is now in the clipboard: the text, its back-translation (if any)
/// and a note saying so. Showing it here is what lets the player check what they will paste.
pub fn push_outgoing(app: &AppHandle, text: &str, back: Option<&str>, note: &str) {
    let s = load_settings(app);
    push_update(
        app,
        Update {
            kind: Kind::Result,
            original: None,
            text: text.to_owned(),
            back: back.map(str::to_owned),
            note: Some(note.to_owned()),
            style: style_from(&s),
        },
    );
}

fn push_update(app: &AppHandle, update: Update) {
    let s = load_settings(app);
    let Some(state) = app.try_state::<OverlayState>() else {
        return;
    };
    if update.kind != Kind::Busy {
        *state.last.lock().unwrap_or_else(|e| e.into_inner()) = Some(update.clone());
    }
    present(app, &update, &s);

    let generation = state.generation.fetch_add(1, Ordering::SeqCst) + 1;
    if state.pinned.load(Ordering::SeqCst) {
        tracing::info!("overlay pinned by toggle: auto-hide not scheduled");
        return;
    }
    let app = app.clone();
    let secs = u64::from(s.overlay.auto_hide_seconds);
    tracing::info!(secs, generation, "overlay auto-hide scheduled");
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(Duration::from_secs(secs)).await;
        let Some(state) = app.try_state::<OverlayState>() else {
            return;
        };
        let current = state.generation.load(Ordering::SeqCst);
        let pinned = state.pinned.load(Ordering::SeqCst);
        if current == generation && !pinned {
            tracing::info!(generation, "overlay auto-hide fired");
            hide(&app);
        } else {
            tracing::info!(generation, current, pinned, "overlay auto-hide skipped");
        }
    });
}

/// Show/hide from the hotkey or tray. Shown this way it stays until toggled off.
pub fn toggle(app: &AppHandle) {
    let Some(state) = app.try_state::<OverlayState>() else {
        return;
    };
    if state.shown.load(Ordering::SeqCst) {
        state.pinned.store(false, Ordering::SeqCst);
        state.generation.fetch_add(1, Ordering::SeqCst);
        hide(app);
        return;
    }
    state.pinned.store(true, Ordering::SeqCst);
    state.generation.fetch_add(1, Ordering::SeqCst);
    let s = load_settings(app);
    let last = state.last.lock().unwrap_or_else(|e| e.into_inner()).clone();
    let update = last.map(|mut u| {
        u.style = style_from(&s);
        u
    });
    let update = update.unwrap_or_else(|| Update {
        kind: Kind::Info,
        original: None,
        text: "Nothing to show yet".into(),
        back: None,
        note: None,
        style: style_from(&s),
    });
    present(app, &update, &s);
}

pub fn hide(app: &AppHandle) {
    if let Some(state) = app.try_state::<OverlayState>() {
        state.shown.store(false, Ordering::SeqCst);
    }
    if let Some(w) = app.get_webview_window(LABEL) {
        #[cfg(windows)]
        {
            if platform::hide(&w) {
                return;
            }
        }
        let _ = w.hide();
    }
}

fn present(app: &AppHandle, update: &Update, s: &settings::Settings) {
    let Some(w) = app.get_webview_window(LABEL) else {
        return;
    };
    let _ = app.emit_to(LABEL, EVENT, update);
    place(&w, s);
    show_without_activating(&w);
    if let Some(state) = app.try_state::<OverlayState>() {
        state.shown.store(true, Ordering::SeqCst);
    }
}

/// Size and corner placement on the monitor the overlay is on (primary as a fallback).
fn place(w: &WebviewWindow, s: &settings::Settings) {
    let monitor = w
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| w.primary_monitor().ok().flatten());
    let Some(m) = monitor else { return };
    let scale = m.scale_factor();
    let width = (f64::from(s.overlay.width) * scale).round() as u32;
    let height = (HEIGHT_LOGICAL * scale).round() as u32;
    let margin = (MARGIN_LOGICAL * scale).round() as i32;
    let mpos = m.position();
    let msize = m.size();
    let x = if s.overlay.position.ends_with("left") {
        mpos.x + margin
    } else {
        mpos.x + msize.width as i32 - width as i32 - margin
    };
    let y = if s.overlay.position.starts_with("top") {
        mpos.y + margin
    } else {
        mpos.y + msize.height as i32 - height as i32 - margin
    };
    let _ = w.set_size(PhysicalSize::new(width, height));
    let _ = w.set_position(PhysicalPosition::new(x, y));
}

fn show_without_activating(w: &WebviewWindow) {
    #[cfg(windows)]
    {
        if platform::show_no_activate(w) {
            return;
        }
    }
    let _ = w.show();
}

#[cfg(windows)]
mod platform {
    use tauri::WebviewWindow;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, ShowWindow, GWL_EXSTYLE, HWND_TOPMOST,
        SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_HIDE, SW_SHOWNOACTIVATE, WS_EX_NOACTIVATE,
        WS_EX_TOOLWINDOW,
    };

    fn hwnd_of(w: &WebviewWindow) -> Option<HWND> {
        // Go through the raw pointer so this does not depend on which `windows` version
        // Tauri was built with.
        let h = w.hwnd().ok()?;
        Some(HWND(h.0 as *mut _))
    }

    /// WS_EX_NOACTIVATE: clicks never activate it. WS_EX_TOOLWINDOW: no taskbar / Alt+Tab entry.
    pub fn make_non_activating(w: &WebviewWindow) {
        let Some(hwnd) = hwnd_of(w) else { return };
        // SAFETY: `hwnd` is the live handle of our own overlay window.
        unsafe {
            let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            let add = (WS_EX_NOACTIVATE.0 | WS_EX_TOOLWINDOW.0) as isize;
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style | add);
        }
    }

    pub fn hide(w: &WebviewWindow) -> bool {
        let Some(hwnd) = hwnd_of(w) else { return false };
        // SAFETY: `hwnd` is the live handle of our own overlay window.
        unsafe {
            let _ = ShowWindow(hwnd, SW_HIDE);
        }
        true
    }

    /// SW_SHOWNOACTIVATE shows the window without stealing focus from whatever is in front,
    /// then HWND_TOPMOST + SWP_NOACTIVATE keeps it above a borderless game window.
    pub fn show_no_activate(w: &WebviewWindow) -> bool {
        let Some(hwnd) = hwnd_of(w) else { return false };
        // SAFETY: `hwnd` is the live handle of our own overlay window.
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            let _ = SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                0,
                0,
                0,
                0,
                SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
            );
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_flattens_and_cuts() {
        assert_eq!(truncate("a\n b   c", 10), "a b c");
        assert_eq!(truncate(&"x".repeat(300), 240).chars().count(), 241);
        assert!(truncate(&"x".repeat(300), 240).ends_with('…'));
    }

    #[test]
    fn update_serializes_camel_case_with_lowercase_kind() {
        let u = Update {
            kind: Kind::Result,
            original: None,
            text: "t".into(),
            back: None,
            note: None,
            style: Style {
                font_size: 16,
                opacity: 0.8,
                position: "top-right".into(),
            },
        };
        let v = serde_json::to_value(&u).unwrap();
        assert_eq!(v["kind"], "result");
        assert_eq!(v["style"]["fontSize"], 16);
    }
}
