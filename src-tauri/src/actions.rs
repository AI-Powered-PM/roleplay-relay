//! What the hotkeys and the compose window do: incoming translation (milestone 2) and the
//! outgoing flows - compose and quick clipboard mode (milestone 3).

use std::io;
use std::time::Instant;

use serde::Serialize;
use tauri::{AppHandle, Manager};
use tauri_plugin_clipboard_manager::ClipboardExt;

use crate::claude::{self, ApiError};
use crate::compose::{self, ComposeState};
use crate::context::{ChatContext, Direction};
use crate::guard::{self, GuardError, Limiter, Refusal};
use crate::outguard::{self, Block, Processed};
use crate::overlay::{self, Kind};
use crate::prompts::{self, Mode};
use crate::{paths, secrets, settings, split};

const ACTION_INCOMING: &str = "incoming";
const ACTION_COMPOSE: &str = "compose";
const ACTION_QUICK: &str = "quick";

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

    let system = load_system(app, &data, Mode::Incoming, &cfg.active_character)?;
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
fn load_system(
    app: &AppHandle,
    data: &std::path::Path,
    mode: Mode,
    character: &str,
) -> Result<String, String> {
    match prompts::build_system(data, mode, character) {
        Ok(s) => Ok(s),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            paths::ensure_layout(app, data)?;
            prompts::build_system(data, mode, character).map_err(|e| prompt_error(&e))
        }
        Err(e) => Err(prompt_error(&e)),
    }
}

fn prompt_error(e: &io::Error) -> String {
    tracing::error!("could not read prompt file: {}", e.kind());
    "Could not read the prompt file — see Settings → Open prompts folder".to_owned()
}

// ---------------------------------------------------------------------------------------------
// Outgoing: compose and quick clipboard mode
// ---------------------------------------------------------------------------------------------

const UNEXPECTED_REPLY: &str = "Unexpected reply — nothing copied";
const CLIPBOARD_WRITE_FAILED: &str = "Could not write to the clipboard";

/// A reply that passed (or, for "Copy anyway", failed) the Output guard.
#[derive(Debug, Clone)]
pub struct Draft {
    pub en: String,
    /// `None` for Grammar (never asked for) and when the model left it out.
    pub back: Option<String>,
    pub character: String,
    pub mode: Mode,
}

enum Generated {
    Ok(Draft),
    Declined(String),
    Blocked(Draft, Block),
}

/// A failed request. `raw` is set when the model replied but not in the expected shape.
#[derive(Debug, Clone, Serialize)]
pub struct Failure {
    pub message: String,
    pub raw: Option<String>,
}

impl From<String> for Failure {
    fn from(message: String) -> Self {
        Self { message, raw: None }
    }
}

fn input_message(e: GuardError, compose: bool) -> String {
    match (e, compose) {
        (GuardError::LooksLikeSecret, true) => "Text looks like a key/password — not sent".into(),
        (GuardError::Empty, true) => "Type a message first".into(),
        (e, _) => e.to_string(),
    }
}

/// The whole outgoing pipeline up to, but not including, the clipboard: input guard, request,
/// parse, Output guard. `compose` only changes the wording of input-guard messages.
async fn generate(
    app: &AppHandle,
    mode: Mode,
    raw_input: &str,
    compose: bool,
) -> Result<Generated, Failure> {
    let input = guard::check_input(raw_input).map_err(|e| input_message(e, compose))?;
    if !compose {
        // Compose has its own spinner; quick mode has no window of its own.
        overlay::push(app, Kind::Busy, None, "Working…");
    }

    let data = paths::data_dir(app)?;
    let cfg = settings::load(app, &data)?;
    let mode_cfg = match mode {
        Mode::Incoming => cfg.modes.incoming.clone(),
        Mode::Direct => cfg.modes.direct.clone(),
        Mode::Rp => cfg.modes.rp.clone(),
        Mode::Grammar => cfg.modes.grammar.clone(),
    };

    let key = match secrets::get_api_key() {
        Ok(Some(k)) => k,
        Ok(None) => return Err(ApiError::NoKey.to_string().into()),
        Err(e) => return Err(e.to_string().into()),
    };

    let system = load_system(app, &data, mode, &cfg.active_character)?;
    let state = app.state::<AppState>();
    let user = prompts::build_user_message(&state.context.recent(&cfg.active_character), &input);

    let request = claude::Request {
        model: &mode_cfg.model,
        max_tokens: mode_cfg.max_tokens,
        temperature: mode_cfg.temperature,
        system: &system,
        user: &user,
    };
    let reply = state
        .client
        .complete(&key, &request)
        .await
        .map_err(|e| e.to_string())?;

    if cfg.debug_log_chat {
        tracing::info!(input = %input, reply = %reply.text, "outgoing generated");
    }
    let processed = outguard::process_reply(mode, &reply.text).map_err(|u| Failure {
        message: UNEXPECTED_REPLY.into(),
        raw: Some(u.raw),
    })?;

    let draft = |en: String, back: Option<String>| Draft {
        en,
        back,
        character: cfg.active_character.clone(),
        mode,
    };
    Ok(match processed {
        Processed::Ok { en, back } => Generated::Ok(draft(en, back)),
        Processed::Declined { reason } => Generated::Declined(reason),
        Processed::Blocked { en, back, block } => Generated::Blocked(draft(en, back), block),
    })
}

/// Put part 1 into the clipboard, remember the line as context and show it in the overlay.
/// The overlay push is part of the copy on purpose (Hard rule 6): nothing is copied unseen.
fn deliver(app: &AppHandle, draft: &Draft) -> Result<(), String> {
    // TODO(M4): keep all parts for `Ctrl+Alt+N`; show them in the overlay with the current one marked.
    let parts = split::split_for_chat(&draft.en);
    let first = parts.first().cloned().unwrap_or_default();
    app.clipboard().write_text(first).map_err(|e| {
        tracing::error!("clipboard write failed: {e}");
        CLIPBOARD_WRITE_FAILED.to_owned()
    })?;

    app.state::<AppState>()
        .context
        .push(&draft.character, Direction::Out, &draft.en);

    let back = match (draft.mode, &draft.back) {
        (Mode::Grammar, _) => None,
        (_, Some(b)) => Some(b.as_str()),
        (_, None) => Some("(no back-translation)"),
    };
    let mut note = "Copied — paste it in the game".to_owned();
    if draft.en.chars().count() > split::CHAT_LIMIT {
        // TODO(M4): remove once the splitter is in.
        note.push_str(". Longer than 255 characters — splitting comes in the next update");
    }
    overlay::push_outgoing(app, &draft.en, back, &note);
    Ok(())
}

/// Which outgoing mode Ctrl+Alt+E uses: the last one chosen in compose.
fn last_outgoing_mode(app: &AppHandle) -> Mode {
    paths::data_dir(app)
        .map(|d| settings::load_last_mode(app, &d))
        .unwrap_or(Mode::Direct)
}

/// What the compose window gets back from `compose_submit`.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SubmitResult {
    /// Copied; compose has been closed.
    Done,
    /// Nothing happened (double Enter, or the window was closed meanwhile).
    Ignored,
    /// The model refused; `reason` is its explanation (Russian).
    Declined { reason: String },
    /// The Output guard blocked it. Nothing copied; "Copy anyway" is offered.
    Blocked {
        en: String,
        back: Option<String>,
        reason: String,
    },
}

pub async fn compose_submit(
    app: &AppHandle,
    text: &str,
    mode: Mode,
) -> Result<SubmitResult, Failure> {
    if !mode.is_outgoing() {
        return Err(Failure::from("Unknown mode".to_owned()));
    }
    let state = app.state::<AppState>();
    let compose_state = app.state::<ComposeState>();
    match state.limiter.try_begin(ACTION_COMPOSE, Instant::now()) {
        Ok(()) => {}
        Err(Refusal::Ignored) => return Ok(SubmitResult::Ignored),
        Err(r) => return Err(Failure::from(r.message().unwrap_or("Slow down").to_owned())),
    }
    let generation = compose_state.generation();
    compose_state.take_pending();
    if let Ok(data) = paths::data_dir(app) {
        let _ = settings::save_last_mode(app, &data, mode);
    }

    let generated = generate(app, mode, text, true).await;
    state.limiter.finish(ACTION_COMPOSE);

    // Esc (or a new open) while the request ran: drop the result, copy nothing.
    if compose_state.generation() != generation {
        return Ok(SubmitResult::Ignored);
    }
    match generated? {
        Generated::Ok(draft) => {
            deliver(app, &draft)?;
            compose::close(app);
            Ok(SubmitResult::Done)
        }
        Generated::Declined(reason) => Ok(SubmitResult::Declined { reason }),
        Generated::Blocked(draft, block) => {
            let result = SubmitResult::Blocked {
                en: draft.en.clone(),
                back: draft.back.clone(),
                reason: block.to_string(),
            };
            compose_state.set_pending(draft);
            Ok(result)
        }
    }
}

/// The "Copy anyway" button: copies the result the Output guard blocked.
pub fn compose_copy_anyway(app: &AppHandle) -> Result<(), String> {
    let Some(draft) = app.state::<ComposeState>().take_pending() else {
        return Err("Nothing to copy".to_owned());
    };
    deliver(app, &draft)?;
    compose::close(app);
    Ok(())
}

/// Hotkey entry point for quick clipboard mode (Ctrl+Alt+E).
pub fn quick_clipboard(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        if let Err(refusal) = state.limiter.try_begin(ACTION_QUICK, Instant::now()) {
            if let Some(msg) = refusal.message() {
                overlay::push(&app, Kind::Info, None, msg);
            }
            return;
        }
        let mode = last_outgoing_mode(&app);
        let raw = app.clipboard().read_text().unwrap_or_default();
        let generated = generate(&app, mode, &raw, false).await;
        state.limiter.finish(ACTION_QUICK);

        match generated {
            Ok(Generated::Ok(draft)) => {
                if let Err(message) = deliver(&app, &draft) {
                    overlay::push(&app, Kind::Error, None, &message);
                }
            }
            Ok(Generated::Declined(reason)) => overlay::push(&app, Kind::Info, None, &reason),
            Ok(Generated::Blocked(draft, block)) => {
                let hotkey = paths::data_dir(&app)
                    .and_then(|d| settings::load(&app, &d))
                    .map(|s| s.hotkeys.compose)
                    .unwrap_or_default();
                let message = format!("{block} — open compose ({hotkey}) to write it yourself");
                overlay::push(&app, Kind::Error, Some(&draft.en), &message);
            }
            Err(f) => overlay::push(&app, Kind::Error, f.raw.as_deref(), &f.message),
        }
    });
}
