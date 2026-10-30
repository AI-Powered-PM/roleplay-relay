//! What the hotkeys do. Milestone 2: the incoming translation flow.

use std::io;
use std::time::Instant;

use tauri::{AppHandle, Manager};
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::claude::{self, ApiError};
use crate::context::{ChatContext, Direction};
use crate::guard::{self, Limiter};
use crate::overlay::{self, Kind};
use crate::prompts::{self, Mode};
use crate::{paths, secrets, settings};

const ACTION_INCOMING: &str = "incoming";

/// Shared, long-lived pieces. Registered with `app.manage` at startup.
pub struct AppState {
    pub limiter: Limiter,
    pub client: claude::Client,
    pub context: ChatContext,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            limiter: Limiter::default(),
            client: claude::Client::new(),
            context: ChatContext::default(),
        }
    }
}

/// Hotkey entry point: returns immediately, the work runs on the async runtime.
pub fn translate_incoming(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        if let Err(refusal) = state.limiter.try_begin(ACTION_INCOMING, Instant::now()) {
            if let Some(msg) = refusal.message() {
                overlay::push(&app, Kind::Info, None, msg);
            }
            return;
        }
        let outcome = run_incoming(&app).await;
        state.limiter.finish(ACTION_INCOMING);
        match outcome {
            Ok((original, translation)) => {
                overlay::push(&app, Kind::Result, Some(&original), &translation)
            }
            Err(message) => overlay::push(&app, Kind::Error, None, &message),
        }
    });
}

/// Returns `(original, translation)` or the message to show. The clipboard is only read.
async fn run_incoming(app: &AppHandle) -> Result<(String, String), String> {
    // A clipboard with an image or nothing in it reads as an error; both mean "no text".
    let raw = app.clipboard().read_text().unwrap_or_default();
    let input = guard::check_input(&raw).map_err(|e| e.to_string())?;

    let data = paths::data_dir(app)?;
    let cfg = settings::load(app, &data)?;
    let mode = cfg.modes.incoming.clone();

    let key = match secrets::get_api_key() {
        Ok(Some(k)) => k,
        Ok(None) => return Err(ApiError::NoKey.to_string()),
        Err(e) => return Err(e.to_string()),
    };

    let system = load_system(app, &data, &cfg.active_character)?;
    let state = app.state::<AppState>();
    let user = prompts::build_user_message(&state.context.recent(&cfg.active_character), &input);

    overlay::push(app, Kind::Busy, None, "Translating…");

    let request = claude::Request {
        model: &mode.model,
        max_tokens: mode.max_tokens,
        temperature: mode.temperature,
        system: &system,
        user: &user,
    };
    let reply = state
        .client
        .complete(&key, &request)
        .await
        .map_err(|e| e.to_string())?;

    // Incoming output is only displayed, so no strict parsing: show what came back.
    let translation = reply.text.trim().to_owned();
    if translation.is_empty() {
        return Err(ApiError::BadReply.to_string());
    }

    state
        .context
        .push(&cfg.active_character, Direction::In, &input);
    if cfg.debug_log_chat {
        tracing::info!(original = %input, translation = %translation, "incoming translated");
    }
    Ok((input, translation))
}

/// Prompts are re-read on every request. If the file was deleted, restore the bundled
/// default (without touching anything else) and read it again.
fn load_system(app: &AppHandle, data: &std::path::Path, character: &str) -> Result<String, String> {
    match prompts::build_system(data, Mode::Incoming, character) {
        Ok(s) => Ok(s),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            paths::ensure_layout(app, data)?;
            prompts::build_system(data, Mode::Incoming, character).map_err(|e| prompt_error(&e))
        }
        Err(e) => Err(prompt_error(&e)),
    }
}

fn prompt_error(e: &io::Error) -> String {
    tracing::error!("could not read prompt file: {}", e.kind());
    "Could not read the prompt file — see Settings → Open prompts folder".to_owned()
}
