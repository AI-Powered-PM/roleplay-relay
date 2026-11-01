//! Check-only update notifier. Asks the GitHub API which release is the latest and tells the user
//! when it is newer than the running app. It never downloads, installs or runs anything; the
//! download page is one fixed URL opened from here.
//!
//! Logging rule: HTTP status or error kind only, never a body or headers.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use regex::Regex;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;

use crate::{paths, settings};

/// The only place the repository slug is spelled.
pub const RELEASES_API_URL: &str =
    "https://api.github.com/repos/AI-Powered-PM/roleplay-relay/releases/latest";
pub const RELEASES_PAGE_URL: &str = "https://github.com/AI-Powered-PM/roleplay-relay/releases/latest";

const STATE_FILE: &str = "update.json";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const FIRST_TICK: Duration = Duration::from_secs(30);
const TICK: Duration = Duration::from_secs(60 * 60);
const DUE_AFTER_SECS: u64 = 24 * 60 * 60;
const MANUAL_GAP: Duration = Duration::from_secs(10);
const FAILED_TEXT: &str = "Could not check for updates. Try again later.";

#[derive(Debug, thiserror::Error, PartialEq)]
pub enum UpdateError {
    #[error("network error")]
    Network,
    #[error("timeout")]
    Timeout,
    #[error("HTTP status {0}")]
    Status(u16),
    #[error("unreadable response")]
    BadJson,
    #[error("unusable tag")]
    BadTag,
}

// ---- pure helpers ----

/// `v0.2.0` or `0.2.0` only. Pre-release suffixes and anything else are rejected.
pub fn parse_version(text: &str) -> Option<(u64, u64, u64)> {
    let re = Regex::new(r"^v?(\d+)\.(\d+)\.(\d+)$").expect("valid regex");
    let c = re.captures(text.trim())?;
    Some((
        c[1].parse().ok()?,
        c[2].parse().ok()?,
        c[3].parse().ok()?,
    ))
}

/// True when `latest` is strictly newer than `current`. Unparseable input counts as not newer.
pub fn is_newer(current: &str, latest: &str) -> bool {
    match (parse_version(current), parse_version(latest)) {
        (Some(c), Some(l)) => l > c,
        _ => false,
    }
}

/// One notification per version.
pub fn should_notify(latest: &str, last_notified: Option<&str>) -> bool {
    match last_notified {
        None => true,
        Some(prev) => is_newer(prev, latest),
    }
}

/// `tag_name` of the release JSON, if there is one.
pub fn parse_latest_tag(json: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    v.get("tag_name")?.as_str().map(str::to_owned)
}

/// A check is due when there was no success yet, the stored time is in the future (clock moved),
/// or the last success is at least 24 h old.
pub fn is_due(now_secs: u64, last_success: Option<u64>) -> bool {
    match last_success {
        None => true,
        Some(t) if t > now_secs => true,
        Some(t) => now_secs - t >= DUE_AFTER_SECS,
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ---- network ----

/// One GET to `url` (the releases API in production). Returns the latest version as `x.y.z`.
pub async fn fetch_latest(
    url: &str,
    current: &str,
    timeout: Duration,
) -> Result<String, UpdateError> {
    let http = reqwest::Client::builder()
        .timeout(timeout)
        .build()
        .expect("HTTP client config is valid");
    let product = crate::APP_NAME.replace(' ', "");
    let resp = http
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header("User-Agent", format!("{product}/{current}"))
        .send()
        .await
        .map_err(classify)?;
    let status = resp.status();
    if !status.is_success() {
        return Err(UpdateError::Status(status.as_u16()));
    }
    let body = resp.text().await.map_err(classify)?;
    let tag = parse_latest_tag(&body).ok_or(UpdateError::BadJson)?;
    let (a, b, c) = parse_version(&tag).ok_or(UpdateError::BadTag)?;
    Ok(format!("{a}.{b}.{c}"))
}

fn classify(e: reqwest::Error) -> UpdateError {
    if e.is_timeout() {
        UpdateError::Timeout
    } else {
        UpdateError::Network
    }
}

// ---- persisted state ----

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "camelCase")]
pub struct Persisted {
    pub latest_version: Option<String>,
    pub last_success_at: Option<u64>,
    pub last_notified_version: Option<String>,
}

fn state_path(dir: &Path) -> PathBuf {
    dir.join(STATE_FILE)
}

/// Missing file = defaults, silently. Unreadable or corrupt file = defaults plus one warn line.
pub fn load_state(dir: &Path) -> Persisted {
    match std::fs::read_to_string(state_path(dir)) {
        Ok(text) => serde_json::from_str(&text).unwrap_or_else(|_| {
            tracing::warn!("{STATE_FILE} is corrupt; starting without it");
            Persisted::default()
        }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Persisted::default(),
        Err(e) => {
            tracing::warn!("{STATE_FILE} not readable: {}", e.kind());
            Persisted::default()
        }
    }
}

fn save_state(dir: &Path, state: &Persisted) {
    let text = match serde_json::to_string_pretty(state) {
        Ok(t) => t,
        Err(_) => return,
    };
    if let Err(e) = std::fs::write(state_path(dir), text) {
        tracing::warn!("{STATE_FILE} not saved: {}", e.kind());
    }
}

// ---- runtime state ----

/// What Settings sees.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    pub current_version: String,
    pub latest_version: Option<String>,
    pub update_available: bool,
    pub checking: bool,
    pub error: Option<String>,
}

struct Inner {
    status: UpdateStatus,
    persisted: Persisted,
    last_manual: Option<Instant>,
    /// Version currently offered in the tray menu.
    tray_shown: Option<String>,
}

pub struct UpdateState(Mutex<Inner>);

impl Default for UpdateState {
    /// Managed on the Builder, before any window exists; the version is empty until `setup`
    /// calls `set_current_version`, and no check runs until then.
    fn default() -> Self {
        Self::new("")
    }
}

impl UpdateState {
    pub fn new(current_version: &str) -> Self {
        Self(Mutex::new(Inner {
            status: UpdateStatus {
                current_version: current_version.to_owned(),
                latest_version: None,
                update_available: false,
                checking: false,
                error: None,
            },
            persisted: Persisted::default(),
            last_manual: None,
            tray_shown: None,
        }))
    }

    pub fn set_current_version(&self, version: &str) {
        self.lock().status.current_version = version.to_owned();
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// Takes the one in-flight slot. False when a check runs already, when a manual check comes less
/// than 10 s after the last manual one, or (auto) when no check is due.
fn begin(state: &UpdateState, manual: bool, now: u64, at: Instant) -> bool {
    let mut g = state.lock();
    if g.status.checking || g.status.current_version.is_empty() {
        return false;
    }
    if manual {
        if g.last_manual.is_some_and(|t| at.duration_since(t) < MANUAL_GAP) {
            return false;
        }
        g.last_manual = Some(at);
    } else if !is_due(now, g.persisted.last_success_at) {
        return false;
    }
    g.status.checking = true;
    true
}

pub fn status(app: &AppHandle) -> UpdateStatus {
    app.state::<UpdateState>().lock().status.clone()
}

fn emit(app: &AppHandle) {
    let _ = app.emit_to("settings", "updates://status", status(app));
}

fn enabled(app: &AppHandle) -> bool {
    paths::data_dir(app)
        .and_then(|d| settings::load(app, &d))
        .map(|s| s.check_for_updates)
        .unwrap_or(true)
}

/// Makes the tray menu match "enabled and an update is known".
fn sync_tray(app: &AppHandle, enabled: bool) {
    let state = app.state::<UpdateState>();
    let want = {
        let g = state.lock();
        if enabled && g.status.update_available {
            g.status.latest_version.clone()
        } else {
            None
        }
    };
    {
        let mut g = state.lock();
        if g.tray_shown == want {
            return;
        }
        g.tray_shown = want.clone();
    }
    if let Some(tray) = app.tray_by_id(crate::TRAY_ID) {
        match crate::tray_menu(app, want.as_deref()) {
            Ok(menu) => {
                let _ = tray.set_menu(Some(menu));
            }
            Err(e) => tracing::warn!("tray menu not updated: {e}"),
        }
    }
}

/// Fills the status from the cached `update.json` (no network) when updates are on.
fn apply_cache(app: &AppHandle) {
    let state = app.state::<UpdateState>();
    let mut g = state.lock();
    if g.status.latest_version.is_some() {
        return;
    }
    if let Some(latest) = g.persisted.latest_version.clone() {
        g.status.update_available = is_newer(&g.status.current_version, &latest);
        g.status.latest_version = Some(latest);
    }
}

/// Called after Settings are saved, so a changed `Check for updates` shows in the tray at once.
pub fn refresh(app: &AppHandle) {
    let on = enabled(app);
    if on {
        apply_cache(app);
    }
    sync_tray(app, on);
    emit(app);
}

async fn run_check(app: &AppHandle, manual: bool) -> UpdateStatus {
    let state = app.state::<UpdateState>();
    if !begin(&state, manual, now_secs(), Instant::now()) {
        return status(app);
    }
    emit(app);
    let current = status(app).current_version;
    let result = fetch_latest(RELEASES_API_URL, &current, REQUEST_TIMEOUT).await;
    let on = enabled(app);
    let mut notify: Option<String> = None;
    {
        let mut g = state.lock();
        g.status.checking = false;
        match result {
            Ok(latest) => {
                let available = is_newer(&current, &latest);
                tracing::info!("update check: current={current} latest={latest} update={available}");
                g.persisted.latest_version = Some(latest.clone());
                g.persisted.last_success_at = Some(now_secs());
                if available && on && should_notify(&latest, g.persisted.last_notified_version.as_deref())
                {
                    g.persisted.last_notified_version = Some(latest.clone());
                    notify = Some(latest.clone());
                }
                g.status.latest_version = Some(latest);
                g.status.update_available = available;
                g.status.error = None;
                if let Ok(dir) = paths::data_dir(app) {
                    save_state(&dir, &g.persisted);
                }
            }
            Err(e) => {
                match e {
                    UpdateError::Status(code) => tracing::warn!("update check failed: HTTP {code}"),
                    other => tracing::warn!("update check failed: {other}"),
                }
                if manual {
                    g.status.error = Some(FAILED_TEXT.to_owned());
                }
            }
        }
    }
    if let Some(latest) = notify {
        if let Err(e) = app
            .notification()
            .builder()
            .title(crate::APP_NAME)
            .body(format!(
                "Version {latest} is available. Open the tray menu to download it."
            ))
            .show()
        {
            tracing::warn!("notification not shown: {e}");
        }
    }
    sync_tray(app, on);
    emit(app);
    status(app)
}

/// Settings `Check now`.
pub async fn check_now(app: &AppHandle) -> UpdateStatus {
    run_check(app, true).await
}

/// Opens the fixed releases page in the default browser. Nothing from the API is ever opened.
pub fn open_releases_page(app: &AppHandle) -> Result<(), String> {
    app.opener()
        .open_url(RELEASES_PAGE_URL, None::<&str>)
        .map_err(|e| e.to_string())
}

/// Loads the cached state (no network), applies the startup rules and starts the hourly loop.
pub fn start(app: &AppHandle) {
    if let Ok(dir) = paths::data_dir(app) {
        app.state::<UpdateState>().lock().persisted = load_state(&dir);
    }
    refresh(app);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_TICK).await;
        loop {
            if enabled(&app) {
                run_check(&app, false).await;
            } else {
                sync_tray(&app, false);
            }
            tokio::time::sleep(TICK).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::test_support::{mock, Canned};

    const FIXTURE: &str = r#"{"url":"x","tag_name":"v0.2.0","name":"Roleplay Relay 0.2.0","draft":false,"prerelease":false}"#;

    #[test]
    fn parses_versions() {
        assert_eq!(parse_version("v0.2.0"), Some((0, 2, 0)));
        assert_eq!(parse_version("0.2.0"), Some((0, 2, 0)));
        assert_eq!(parse_version("0.2.0-beta.1"), None);
        assert_eq!(parse_version("latest"), None);
        assert_eq!(parse_version(""), None);
        assert_eq!(parse_version("1.2"), None);
        assert_eq!(parse_version("99999999999999999999.0.0"), None);
    }

    #[test]
    fn compares_numerically() {
        assert!(is_newer("0.1.9", "0.1.10"));
        assert!(!is_newer("0.1.10", "0.1.9"));
        assert!(!is_newer("0.2.0", "0.2.0"));
        assert!(!is_newer("0.2.0", "v0.1.9"));
        assert!(is_newer("0.1.1", "v0.2.0"));
        assert!(!is_newer("0.1.1", "0.2.0-beta.1"));
    }

    #[test]
    fn notifies_once_per_version() {
        assert!(should_notify("0.2.0", None));
        assert!(!should_notify("0.2.0", Some("0.2.0")));
        assert!(!should_notify("0.2.0", Some("0.3.0")));
        assert!(should_notify("0.3.0", Some("0.2.0")));
    }

    #[test]
    fn due_rules() {
        let day = 24 * 60 * 60;
        let now = 1_000_000;
        assert!(is_due(now, None));
        assert!(!is_due(now, Some(now - 23 * 60 * 60)));
        assert!(is_due(now, Some(now - day)));
        assert!(is_due(now, Some(now + 5)));
    }

    #[test]
    fn extracts_the_tag() {
        assert_eq!(parse_latest_tag(FIXTURE).as_deref(), Some("v0.2.0"));
        assert_eq!(parse_latest_tag(r#"{"name":"x"}"#), None);
        assert_eq!(parse_latest_tag("<html>"), None);
    }

    #[test]
    fn state_file_missing_corrupt_partial() {
        let dir = std::env::temp_dir().join(format!("rr-update-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let _ = std::fs::remove_file(state_path(&dir));
        assert_eq!(load_state(&dir), Persisted::default());

        std::fs::write(state_path(&dir), "{not json").unwrap();
        assert_eq!(load_state(&dir), Persisted::default());

        std::fs::write(state_path(&dir), r#"{"latestVersion":"0.2.0"}"#).unwrap();
        let p = load_state(&dir);
        assert_eq!(p.latest_version.as_deref(), Some("0.2.0"));
        assert_eq!(p.last_success_at, None);

        let full = Persisted {
            latest_version: Some("0.2.0".into()),
            last_success_at: Some(1_730_000_000),
            last_notified_version: Some("0.2.0".into()),
        };
        save_state(&dir, &full);
        assert_eq!(load_state(&dir), full);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn default_state_has_empty_version_and_runs_no_check_until_set() {
        let s = UpdateState::default();
        assert_eq!(s.lock().status.current_version, "");
        assert!(!begin(&s, true, 1_000, Instant::now()));
        assert!(!begin(&s, false, 1_000, Instant::now()));
        s.set_current_version("0.1.0");
        assert_eq!(s.lock().status.current_version, "0.1.0");
        assert!(begin(&s, true, 1_000, Instant::now()));
    }

    #[test]
    fn begin_shares_one_slot_and_throttles_manual() {
        let s = UpdateState::new("0.1.1");
        let t0 = Instant::now();
        assert!(begin(&s, false, 1_000, t0));
        assert!(!begin(&s, true, 1_000, t0), "in flight");
        s.lock().status.checking = false;
        assert!(begin(&s, true, 1_000, t0));
        s.lock().status.checking = false;
        assert!(!begin(&s, true, 1_000, t0 + Duration::from_secs(5)), "too soon");
        assert!(begin(&s, true, 1_000, t0 + Duration::from_secs(11)));
        s.lock().status.checking = false;
        s.lock().persisted.last_success_at = Some(1_000);
        assert!(!begin(&s, false, 1_100, t0), "auto check not due");
    }

    fn ok(body: &str) -> Canned {
        Canned { status: 200, body: body.into(), delay: Duration::ZERO }
    }
    fn status_only(code: u16) -> Canned {
        Canned { status: code, body: "{}".into(), delay: Duration::ZERO }
    }
    const T: Duration = Duration::from_secs(5);

    #[tokio::test]
    async fn newer_and_same_tags() {
        let (url, _) = mock(vec![ok(FIXTURE), ok(r#"{"tag_name":"0.1.1"}"#)]).await;
        let latest = fetch_latest(&url, "0.1.1", T).await.unwrap();
        assert_eq!(latest, "0.2.0");
        assert!(is_newer("0.1.1", &latest));
        let same = fetch_latest(&url, "0.1.1", T).await.unwrap();
        assert!(!is_newer("0.1.1", &same));
    }

    #[tokio::test]
    async fn failures_are_errors_not_panics() {
        let (url, _) = mock(vec![
            status_only(404),
            status_only(403),
            status_only(429),
            status_only(500),
            ok("not json"),
            ok(r#"{"tag_name":"0.2.0-beta.1"}"#),
        ])
        .await;
        for code in [404, 403, 429, 500] {
            assert_eq!(fetch_latest(&url, "0.1.1", T).await, Err(UpdateError::Status(code)));
        }
        assert_eq!(fetch_latest(&url, "0.1.1", T).await, Err(UpdateError::BadJson));
        assert_eq!(fetch_latest(&url, "0.1.1", T).await, Err(UpdateError::BadTag));
    }

    #[tokio::test]
    async fn timeout_is_an_error() {
        let (url, _) = mock(vec![Canned {
            status: 200,
            body: FIXTURE.into(),
            delay: Duration::from_millis(600),
        }])
        .await;
        let r = fetch_latest(&url, "0.1.1", Duration::from_millis(100)).await;
        assert_eq!(r, Err(UpdateError::Timeout));
    }

    #[tokio::test]
    async fn request_has_only_the_listed_headers_and_no_body() {
        let (url, seen) = mock(vec![ok(FIXTURE)]).await;
        fetch_latest(&url, "0.1.1", T).await.unwrap();
        let raw = seen.lock().unwrap()[0].clone();
        assert!(raw.starts_with("GET "), "{raw}");
        let (head, body) = raw.split_once("\r\n\r\n").unwrap();
        assert!(body.is_empty());
        let mut names: Vec<String> = head
            .lines()
            .skip(1)
            .map(|l| l.split(':').next().unwrap().to_lowercase())
            .collect();
        names.sort();
        assert_eq!(names, ["accept", "host", "user-agent", "x-github-api-version"]);
        let lower = head.to_lowercase();
        assert!(lower.contains("accept: application/vnd.github+json"));
        assert!(lower.contains("x-github-api-version: 2022-11-28"));
        assert!(head.contains("user-agent: RoleplayRelay/0.1.1"));
    }

    #[test]
    fn open_url_is_only_called_here_with_the_fixed_page() {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut hits = Vec::new();
        fn walk(dir: &Path, hits: &mut Vec<(String, String)>) {
            for e in std::fs::read_dir(dir).unwrap() {
                let p = e.unwrap().path();
                if p.is_dir() {
                    walk(&p, hits);
                } else if p.extension().is_some_and(|x| x == "rs") {
                    let text = std::fs::read_to_string(&p).unwrap();
                    for line in text.lines() {
                        if line.contains(concat!("open_", "url(")) {
                            hits.push((p.file_name().unwrap().to_string_lossy().into(), line.trim().into()));
                        }
                    }
                }
            }
        }
        walk(&src, &mut hits);
        // This test's own text counts too, so allow only updates.rs lines.
        assert!(hits.iter().all(|(f, _)| f == "updates.rs"), "{hits:?}");
        assert!(hits.iter().any(|(_, l)| l.contains("RELEASES_PAGE_URL")));
        let commands = std::fs::read_to_string(src.join("commands.rs")).unwrap();
        assert!(!commands.contains("url: String") && !commands.contains("url: &str"));
    }
}
