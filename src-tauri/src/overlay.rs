//! The overlay window: shown without activating, click-through, auto-hiding.
//! It receives everything through the `overlay://update` event; it has no commands.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewWindow};

use crate::{paths, settings};

pub const LABEL: &str = "overlay";
const EVENT: &str = "overlay://update";
const HISTORY_EVENT: &str = "overlay://history";
const HISTORY_LIMIT: usize = 10;
const MARGIN_LOGICAL: f64 = 24.0;
/// Reading speed used to keep long results on screen: characters per second.
const READ_CHARS_PER_SEC: usize = 25;
const MAX_AUTO_HIDE_SECS: u64 = 120;

/// Auto-hide delay for an update. A result with a back-translation or parts stays at least long
/// enough to read what is visible (current part or text, plus the back-translation).
fn auto_hide_delay(update: &Update, auto_hide_seconds: u32) -> u64 {
    let base = u64::from(auto_hide_seconds);
    if update.kind != Kind::Result || (update.back.is_none() && update.parts.is_none()) {
        return base;
    }
    let current = match (&update.parts, update.current) {
        (Some(parts), Some(i)) if i < parts.len() => parts[i].as_str(),
        _ => update.text.as_str(),
    };
    let chars = current.chars().count() + update.back.as_deref().map_or(0, |b| b.chars().count());
    reading_delay(chars, base)
}

fn reading_delay(visible_chars: usize, auto_hide_seconds: u64) -> u64 {
    let reading = visible_chars.div_ceil(READ_CHARS_PER_SEC) as u64;
    auto_hide_seconds.max(reading).min(MAX_AUTO_HIDE_SECS)
}

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
    /// All parts of a long message, when it was split. `text` then holds the whole message.
    pub parts: Option<Vec<String>>,
    /// Index into `parts` of the part that is in the clipboard now.
    pub current: Option<usize>,
    pub style: Style,
}

/// Payload of `overlay://history`: oldest first, newest last.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct HistoryPayload {
    entries: Vec<Update>,
    style: Style,
}

/// Bounded in-memory record of what the overlay showed. Never persisted.
#[derive(Default)]
struct History(VecDeque<Update>);

impl History {
    fn push(&mut self, update: Update) {
        // Stepping through a split message with Ctrl+Alt+N re-pushes the same message with a new
        // current part; that updates its entry instead of adding a duplicate.
        if update.parts.is_some() {
            if let Some(last) = self.0.back_mut() {
                if last.parts.is_some() && last.text == update.text {
                    *last = update;
                    return;
                }
            }
        }
        if self.0.len() == HISTORY_LIMIT {
            self.0.pop_front();
        }
        self.0.push_back(update);
    }

    fn entries(&self) -> Vec<Update> {
        self.0.iter().cloned().collect()
    }
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
    history: Mutex<History>,
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
            parts: None,
            current: None,
            style: style_from(&s),
        },
    );
}

/// An outgoing line that is now in the clipboard: the text, its back-translation (if any)
/// and a note saying so. Showing it here is what lets the player check what they will paste.
pub fn push_outgoing(
    app: &AppHandle,
    text: &str,
    back: Option<&str>,
    note: &str,
    parts: Option<(&[String], usize)>,
) {
    let s = load_settings(app);
    push_update(
        app,
        Update {
            kind: Kind::Result,
            original: None,
            text: text.to_owned(),
            back: back.map(str::to_owned),
            note: Some(note.to_owned()),
            parts: parts.map(|(p, _)| p.to_vec()),
            current: parts.map(|(_, i)| i),
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
        state
            .history
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(update.clone());
    }
    show_timed(app, &update, &s);
}

/// Settings → `Preview overlay`: a sample entry drawn with the values currently in the form
/// (out-of-range numbers are clamped like on save). Same window, same no-activate and
/// click-through rules as any update; no request is made and nothing enters the history.
pub fn preview(app: &AppHandle, form: settings::OverlaySettings, user_language: &str) {
    let s = settings::Settings {
        overlay: form,
        ..settings::Settings::default()
    }
    .sanitized();
    let lang = crate::languages::resolve(user_language);
    let update = Update {
        kind: Kind::Result,
        original: Some(crate::languages::SAMPLE_ORIGINAL.to_owned()),
        text: lang.sample_translation.to_owned(),
        back: None,
        note: Some("Preview".to_owned()),
        parts: None,
        current: None,
        style: style_from(&s),
    };
    show_timed(app, &update, &s);
}

/// Show `update`, then hide it after the auto-hide time unless the toggle pinned the overlay.
fn show_timed(app: &AppHandle, update: &Update, s: &settings::Settings) {
    let Some(state) = app.try_state::<OverlayState>() else {
        return;
    };
    present(app, update, s);

    let generation = state.generation.fetch_add(1, Ordering::SeqCst) + 1;
    if state.pinned.load(Ordering::SeqCst) {
        tracing::info!("overlay pinned by toggle: auto-hide not scheduled");
        return;
    }
    let app = app.clone();
    let secs = auto_hide_delay(update, s.overlay.auto_hide_seconds);
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
    let entries = state
        .history
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .entries();
    if entries.is_empty() {
        let update = Update {
            kind: Kind::Info,
            original: None,
            text: "Nothing to show yet".into(),
            back: None,
            note: None,
            parts: None,
            current: None,
            style: style_from(&s),
        };
        present(app, &update, &s);
        return;
    }
    let _ = app.emit_to(
        LABEL,
        HISTORY_EVENT,
        &HistoryPayload {
            entries,
            style: style_from(&s),
        },
    );
    show_window(app, &s);
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
    let _ = app.emit_to(LABEL, EVENT, update);
    show_window(app, s);
}

fn show_window(app: &AppHandle, s: &settings::Settings) {
    let Some(w) = app.get_webview_window(LABEL) else {
        return;
    };
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
    let margin = (MARGIN_LOGICAL * scale).round() as i32;
    // The window is transparent and click-through, so it takes the whole work area (minus margins):
    // a long result then never has to be clipped by the window itself.
    let area = m.work_area();
    let mpos = area.position;
    let msize = area.size;
    let width = ((f64::from(s.overlay.width) * scale).round() as u32).min(msize.width);
    let height = msize.height.saturating_sub(2 * margin as u32).max(1);
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
    fn auto_hide_delay_follows_reading_time() {
        assert_eq!(reading_delay(100, 12), 12);
        assert_eq!(reading_delay(2000, 12), 80);
        assert_eq!(reading_delay(5000, 12), 120);

        let mut u = entry("short");
        assert_eq!(auto_hide_delay(&u, 12), 12);
        u.back = Some("x".repeat(1995));
        assert_eq!(auto_hide_delay(&u, 12), 80);
        u.kind = Kind::Error;
        assert_eq!(auto_hide_delay(&u, 12), 12);
    }

    fn entry(text: &str) -> Update {
        Update {
            kind: Kind::Result,
            original: None,
            text: text.into(),
            back: None,
            note: None,
            parts: None,
            current: None,
            style: Style {
                font_size: 16,
                opacity: 0.8,
                position: "top-right".into(),
            },
        }
    }

    #[test]
    fn history_keeps_last_ten_in_order() {
        let mut h = History::default();
        for i in 0..13 {
            h.push(entry(&i.to_string()));
        }
        let texts: Vec<String> = h.entries().into_iter().map(|u| u.text).collect();
        assert_eq!(texts.len(), HISTORY_LIMIT);
        assert_eq!(texts.first().unwrap(), "3");
        assert_eq!(texts.last().unwrap(), "12");
    }

    #[test]
    fn history_updates_split_message_in_place() {
        let mut h = History::default();
        h.push(entry("first"));
        let mut a = entry("long message");
        a.parts = Some(vec!["a".into(), "b".into()]);
        a.current = Some(0);
        h.push(a.clone());
        a.current = Some(1);
        h.push(a);
        let e = h.entries();
        assert_eq!(e.len(), 2);
        assert_eq!(e[1].current, Some(1));
    }

    #[test]
    fn update_serializes_camel_case_with_lowercase_kind() {
        let u = Update {
            kind: Kind::Result,
            original: None,
            text: "t".into(),
            back: None,
            note: None,
            parts: None,
            current: None,
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
