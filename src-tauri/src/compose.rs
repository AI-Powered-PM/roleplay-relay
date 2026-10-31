//! The compose window: shown by hotkey, hidden (not destroyed) after use.
//!
//! Focus return (Hard rule 1): before showing, the current foreground window is remembered as an
//! opaque handle. It is never inspected or matched against anything. On close that exact window
//! is brought back, then compose is hidden.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::actions::Draft;

pub const LABEL: &str = "compose";
const EVENT_OPEN: &str = "compose://open";

#[derive(Default)]
pub struct ComposeState {
    /// Foreground window before compose opened (HWND as an integer; 0 = none).
    previous: Mutex<isize>,
    /// Bumped on every close. A request that finishes after its window was closed is discarded,
    /// so nothing reaches the clipboard after Esc.
    generation: AtomicU64,
    /// A result the Output guard blocked, waiting for "Copy anyway".
    pending: Mutex<Option<Draft>>,
}

impl ComposeState {
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    pub fn set_pending(&self, draft: Draft) {
        *self.pending.lock().unwrap_or_else(|e| e.into_inner()) = Some(draft);
    }

    pub fn take_pending(&self) -> Option<Draft> {
        self.pending.lock().unwrap_or_else(|e| e.into_inner()).take()
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase")]
struct OpenEvent {
    mode: crate::prompts::Mode,
}

/// Hotkey entry point: show compose centered, focused, with the textarea ready.
pub fn open(app: &AppHandle) {
    let Some(w) = app.get_webview_window(LABEL) else {
        return;
    };
    let state = app.state::<ComposeState>();
    let already_open = w.is_visible().unwrap_or(false);
    if !already_open {
        // Only when compose is not up yet; otherwise the foreground window would be compose.
        *state.previous.lock().unwrap_or_else(|e| e.into_inner()) = platform::foreground();
        state.take_pending();
    }
    let mode = crate::paths::data_dir(app)
        .map(|d| crate::settings::load_last_mode(app, &d))
        .unwrap_or(crate::prompts::Mode::Direct);
    if !already_open {
        let _ = w.center();
    }
    let _ = w.show();
    let _ = w.set_focus();
    if !already_open {
        let _ = app.emit_to(LABEL, EVENT_OPEN, OpenEvent { mode });
    }
}

/// Hide compose and return to the window that was in front before it opened.
pub fn close(app: &AppHandle) {
    let state = app.state::<ComposeState>();
    state.generation.fetch_add(1, Ordering::SeqCst);
    state.take_pending();
    let previous = std::mem::take(&mut *state.previous.lock().unwrap_or_else(|e| e.into_inner()));
    // While compose is still the foreground window this process is allowed to hand focus on.
    platform::restore(previous);
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.hide();
    }
}

#[cfg(windows)]
mod platform {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, IsWindow, SetForegroundWindow};

    pub fn foreground() -> isize {
        // SAFETY: plain query, no arguments.
        unsafe { GetForegroundWindow().0 as isize }
    }

    pub fn restore(handle: isize) {
        if handle == 0 {
            return;
        }
        let hwnd = HWND(handle as *mut _);
        // SAFETY: `IsWindow` accepts any handle value; the window may have closed meanwhile.
        unsafe {
            if IsWindow(Some(hwnd)).as_bool() {
                let _ = SetForegroundWindow(hwnd);
            }
        }
    }
}

#[cfg(not(windows))]
mod platform {
    pub fn foreground() -> isize {
        0
    }
    pub fn restore(_handle: isize) {}
}
