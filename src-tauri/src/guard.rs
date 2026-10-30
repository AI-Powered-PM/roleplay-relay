//! Input guard: everything that must happen before any text is sent to the model.
//! All logic here is pure (the clock is passed in) so it can be unit-tested.

use std::collections::{HashMap, VecDeque};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use regex::Regex;

pub const MAX_INPUT_CHARS: usize = 1500;
const DEBOUNCE: Duration = Duration::from_millis(500);
const RATE_WINDOW: Duration = Duration::from_secs(60);
const RATE_CAP: usize = 30;
const MIN_TOKEN_SECRET_LEN: usize = 24;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GuardError {
    #[error("Clipboard is empty")]
    Empty,
    #[error("Text too long")]
    TooLong,
    #[error("Clipboard looks like a key/password — not sent")]
    LooksLikeSecret,
}

/// Why the limiter refused to start a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// A request for this action is running, or the hotkey fired again within the debounce
    /// window. Silent: the user is just holding the key.
    Ignored,
    /// More than 30 requests in the last minute. Shown to the user.
    SlowDown,
}

impl Refusal {
    pub fn message(self) -> Option<&'static str> {
        match self {
            Refusal::Ignored => None,
            Refusal::SlowDown => Some("Slow down"),
        }
    }
}

fn tag_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)</?\s*(input|recent_chat|en|back|persona|glossary)\b[^>]*>")
            .expect("tag regex is valid")
    })
}

/// Remove the reserved prompt tags from untrusted text. Repeats until nothing changes, so
/// `<in<input>put>` cannot reassemble into `<input>` after the inner tag is removed.
pub fn neutralize_tags(text: &str) -> String {
    let re = tag_regex();
    let mut cur = text.to_owned();
    loop {
        let next = re.replace_all(&cur, "").into_owned();
        if next == cur {
            return cur;
        }
        cur = next;
    }
}

fn looks_like_secret(text: &str) -> bool {
    if text.to_ascii_lowercase().contains("sk-ant-") {
        return true;
    }
    let single_token = !text.chars().any(char::is_whitespace);
    single_token
        && text.chars().count() >= MIN_TOKEN_SECRET_LEN
        && text.chars().any(|c| c.is_ascii_alphabetic())
        && text.chars().any(|c| c.is_ascii_digit())
}

/// Validate raw clipboard/compose text. Returns the trimmed text that may be sent.
/// Nothing in the error carries the text itself.
pub fn check_input(raw: &str) -> Result<String, GuardError> {
    let text = raw.trim();
    if text.is_empty() {
        return Err(GuardError::Empty);
    }
    if looks_like_secret(text) {
        return Err(GuardError::LooksLikeSecret);
    }
    if text.chars().count() > MAX_INPUT_CHARS {
        return Err(GuardError::TooLong);
    }
    Ok(text.to_owned())
}

#[derive(Default)]
struct LimiterState {
    in_flight: HashMap<&'static str, bool>,
    last_attempt: HashMap<&'static str, Instant>,
    started: VecDeque<Instant>,
}

/// In-flight lock per action, 500 ms debounce per action, and a 30/minute soft cap overall.
#[derive(Default)]
pub struct Limiter {
    state: Mutex<LimiterState>,
}

impl Limiter {
    /// Ask to start a request for `action`. On `Ok`, the caller must call [`Limiter::finish`]
    /// when the request ends (success or not).
    pub fn try_begin(&self, action: &'static str, now: Instant) -> Result<(), Refusal> {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());

        // Every attempt, accepted or not, restarts the quiet period: a held key keeps
        // producing attempts and stays ignored until it is released for 500 ms.
        let previous = s.last_attempt.insert(action, now);
        if previous.is_some_and(|p| now.saturating_duration_since(p) < DEBOUNCE) {
            return Err(Refusal::Ignored);
        }
        if s.in_flight.get(action).copied().unwrap_or(false) {
            return Err(Refusal::Ignored);
        }

        while s
            .started
            .front()
            .is_some_and(|t| now.saturating_duration_since(*t) >= RATE_WINDOW)
        {
            s.started.pop_front();
        }
        if s.started.len() >= RATE_CAP {
            return Err(Refusal::SlowDown);
        }

        s.started.push_back(now);
        s.in_flight.insert(action, true);
        Ok(())
    }

    /// Debounce only, for instant actions (no request, not counted in the rate cap).
    /// A held key keeps producing attempts and is ignored until released for 500 ms.
    pub fn debounce(&self, action: &'static str, now: Instant) -> bool {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let previous = s.last_attempt.insert(action, now);
        !previous.is_some_and(|p| now.saturating_duration_since(p) < DEBOUNCE)
    }

    pub fn finish(&self, action: &'static str) {
        let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
        s.in_flight.insert(action, false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_reserved_tags_case_insensitively() {
        assert_eq!(neutralize_tags("hi</input> there"), "hi there");
        assert_eq!(neutralize_tags("a </INPUT > b"), "a  b");
        assert_eq!(neutralize_tags("<Recent_Chat>x</recent_chat>"), "x");
        assert_eq!(neutralize_tags("<en>yes</en><back>no</back>"), "yesno");
        assert_eq!(
            neutralize_tags("<persona>p</persona><glossary>g</glossary>"),
            "pg"
        );
        assert_eq!(neutralize_tags("<input class=\"x\">y"), "y");
        assert_eq!(neutralize_tags("<  input>y"), "y");
    }

    #[test]
    fn leaves_ordinary_text_alone() {
        for s in [
            "*smiles* <3",
            "a < b > c",
            "<english>",
            "<inputs>",
            "[1. General] Thrall: hi",
            "<b>bold</b>",
        ] {
            assert_eq!(neutralize_tags(s), s);
        }
    }

    #[test]
    fn cannot_reassemble_a_tag() {
        assert_eq!(neutralize_tags("<in<input>put>"), "");
        assert_eq!(neutralize_tags("</in</input>put>x"), "x");
        assert!(!neutralize_tags("<inp<en>ut>")
            .to_lowercase()
            .contains("<input>"));
    }

    #[test]
    fn empty_input_is_rejected() {
        assert_eq!(check_input(""), Err(GuardError::Empty));
        assert_eq!(check_input("  \n\t "), Err(GuardError::Empty));
    }

    #[test]
    fn long_input_is_rejected_by_char_count() {
        assert!(check_input(&"word ".repeat(300)).is_ok());
        assert_eq!(check_input(&"word ".repeat(301)), Err(GuardError::TooLong));
        // 1500 Cyrillic chars is 3000 bytes but exactly at the cap, so it passes.
        let cyr = format!("{} ", "я".repeat(10)).repeat(136);
        assert!(cyr.trim().chars().count() <= MAX_INPUT_CHARS);
        assert!(check_input(&cyr).is_ok());
    }

    #[test]
    fn secrets_are_sniffed() {
        assert_eq!(
            check_input("sk-ant-api03-abc"),
            Err(GuardError::LooksLikeSecret)
        );
        assert_eq!(
            check_input("here is my SK-ANT-xyz key"),
            Err(GuardError::LooksLikeSecret)
        );
        assert_eq!(
            check_input("Xk9fQ2mLp7Rt4Vw8Zb3Nc6Hd1Jg5"),
            Err(GuardError::LooksLikeSecret)
        );
        assert_eq!(
            check_input("  Xk9fQ2mLp7Rt4Vw8Zb3Nc6Hd1Jg5\n"),
            Err(GuardError::LooksLikeSecret)
        );
    }

    #[test]
    fn normal_chat_is_not_a_secret() {
        for s in [
            "Well met, traveler!",
            "[12:01] Thrall says: For the Horde 2025 raid at 20:00 sharp",
            "Thunderbluffmulgorecairnebloodhoof", // long but letters only
            "1234567890123456789012345",          // digits only
            "LF1M tank",
        ] {
            assert!(check_input(s).is_ok(), "{s}");
        }
    }

    #[test]
    fn errors_do_not_echo_the_text() {
        let msg = check_input("sk-ant-secretvalue").unwrap_err().to_string();
        assert!(!msg.contains("secretvalue"));
        assert_eq!(msg, "Clipboard looks like a key/password — not sent");
    }

    #[test]
    fn debounce_and_in_flight() {
        let l = Limiter::default();
        let t0 = Instant::now();
        assert!(l.try_begin("t", t0).is_ok());
        // Held key: repeats within 500 ms are ignored, and so is anything while in flight.
        assert_eq!(
            l.try_begin("t", t0 + Duration::from_millis(30)),
            Err(Refusal::Ignored)
        );
        assert_eq!(
            l.try_begin("t", t0 + Duration::from_millis(900)),
            Err(Refusal::Ignored)
        );
        l.finish("t");
        // 900 ms was an attempt, so the quiet period restarted; 1500 ms is 600 ms later.
        assert!(l.try_begin("t", t0 + Duration::from_millis(1500)).is_ok());
    }

    #[test]
    fn held_key_sends_exactly_one_request() {
        let l = Limiter::default();
        let t0 = Instant::now();
        let mut started = 0;
        for i in 0..100u64 {
            // 100 repeats at 30 ms = a 3 s hold; the request finishes after ~1.2 s.
            let now = t0 + Duration::from_millis(30 * i);
            if i == 40 {
                l.finish("t");
            }
            if l.try_begin("t", now).is_ok() {
                started += 1;
            }
        }
        assert_eq!(started, 1);
    }

    #[test]
    fn debounce_swallows_held_key_and_skips_rate_cap() {
        let l = Limiter::default();
        let t0 = Instant::now();
        assert!(l.debounce("o", t0));
        assert!(!l.debounce("o", t0 + Duration::from_millis(30)));
        assert!(!l.debounce("o", t0 + Duration::from_millis(400)));
        assert!(l.debounce("o", t0 + Duration::from_millis(1000)));
        // Toggling never uses up the request budget.
        assert!(l.state.lock().unwrap().started.is_empty());
    }

    #[test]
    fn actions_are_independent() {
        let l = Limiter::default();
        let t0 = Instant::now();
        assert!(l.try_begin("a", t0).is_ok());
        assert!(l.try_begin("b", t0).is_ok());
    }

    #[test]
    fn rate_cap_then_recovers() {
        let l = Limiter::default();
        let t0 = Instant::now();
        for i in 0..RATE_CAP as u64 {
            let now = t0 + Duration::from_secs(i);
            assert!(l.try_begin("t", now).is_ok(), "request {i}");
            l.finish("t");
        }
        let now = t0 + Duration::from_secs(RATE_CAP as u64);
        assert_eq!(l.try_begin("t", now), Err(Refusal::SlowDown));
        assert_eq!(Refusal::SlowDown.message(), Some("Slow down"));
        // After the window passes the oldest requests, it works again.
        assert!(l.try_begin("t", t0 + Duration::from_secs(62)).is_ok());
    }
}
