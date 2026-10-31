//! Provider-agnostic LLM layer: one request/reply shape, the `LlmProvider` trait, and the retry
//! and error mapping every provider shares. Each provider is one file next to this one.
//!
//! Logging rule: status codes, timings, token counts and the provider's own error JSON are safe.
//! Request bodies, response text, headers and `reqwest` error details are never logged.

pub mod anthropic;
pub mod openai;
#[cfg(test)]
mod test_support;

use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use secrecy::SecretString;
use serde::{Deserialize, Serialize};

pub const TIMEOUT: Duration = Duration::from_secs(20);
pub const BACKOFF: Duration = Duration::from_secs(1);

/// The AI provider a request goes to. Chosen per request from settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Provider {
    #[default]
    Anthropic,
    #[serde(rename = "openai")]
    OpenAi,
}

impl Provider {
    #[cfg(test)]
    pub const ALL: [Provider; 2] = [Provider::Anthropic, Provider::OpenAi];

    /// Name used in UI messages.
    pub fn name(self) -> &'static str {
        match self {
            Provider::Anthropic => "Anthropic",
            Provider::OpenAi => "OpenAI",
        }
    }

    /// Only the Responses API takes a `reasoning` parameter.
    fn sends_reasoning(self) -> bool {
        self == Provider::OpenAi
    }
}

impl std::fmt::Display for Provider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ApiError {
    #[error("No {0} API key — open Settings")]
    NoKey(Provider),
    #[error("Invalid {0} API key")]
    Unauthorized(Provider),
    #[error("No credits left on your {0} account")]
    NoCredits(Provider),
    #[error("Too many requests, wait a moment")]
    RateLimited,
    #[error("Service overloaded, try again")]
    Overloaded,
    #[error("No response from server")]
    NoResponse,
    #[error("Unexpected reply from server")]
    BadReply,
    #[error("Request failed (HTTP {0})")]
    Other(u16),
}

pub struct LlmRequest<'a> {
    pub model: &'a str,
    pub max_tokens: u32,
    pub temperature: Option<f32>,
    pub system: &'a str,
    pub user: &'a str,
}

/// What the model said: text, or a refusal it stated instead of answering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LlmReply {
    Text(String),
    /// Produced by providers that report refusals separately (OpenAI); Anthropic does not.
    Refusal(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmResponse {
    pub reply: LlmReply,
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// The reply stopped at the token limit.
    pub truncated: bool,
}

impl LlmResponse {
    /// The reply's text, whichever kind it is.
    pub fn text(&self) -> &str {
        match &self.reply {
            LlmReply::Text(t) | LlmReply::Refusal(t) => t,
        }
    }
}

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// One AI provider. Callers hold a `dyn LlmProvider` and never see the concrete client.
pub trait LlmProvider: Send + Sync {
    fn complete<'a>(
        &'a self,
        key: &'a SecretString,
        req: &'a LlmRequest<'a>,
    ) -> BoxFuture<'a, Result<LlmResponse, ApiError>>;
}

/// What one HTTP attempt can fail with, before it is mapped to a user-facing `ApiError`.
#[derive(Debug)]
pub enum Failure {
    Timeout,
    Network,
    /// The account is out of credits (provider-specific detection, never retried).
    NoCredits,
    Status { code: u16, message: String },
}

impl Failure {
    fn retryable(&self) -> bool {
        match self {
            Failure::Timeout | Failure::Network => true,
            Failure::NoCredits => false,
            Failure::Status { code, .. } => matches!(code, 429 | 500 | 502 | 503 | 529),
        }
    }

    fn into_error(self, provider: Provider) -> ApiError {
        match self {
            Failure::Timeout | Failure::Network => ApiError::NoResponse,
            Failure::NoCredits => ApiError::NoCredits(provider),
            Failure::Status { code, .. } => match code {
                401 => ApiError::Unauthorized(provider),
                429 => ApiError::RateLimited,
                500..=599 => ApiError::Overloaded,
                other => ApiError::Other(other),
            },
        }
    }
}

/// Send one request and read the whole body. The `reqwest` error is deliberately dropped: it can
/// carry the URL and request details, which must never reach a log or a message.
pub(crate) async fn execute(request: reqwest::RequestBuilder) -> Result<(u16, String), Failure> {
    let classify = |e: reqwest::Error| {
        if e.is_timeout() {
            Failure::Timeout
        } else {
            Failure::Network
        }
    };
    let response = request.send().await.map_err(classify)?;
    let status = response.status().as_u16();
    let text = response.text().await.map_err(classify)?;
    Ok((status, text))
}

/// A temperature of 0.2 as the API should see it, not 0.20000000298.
pub(crate) fn round_temperature(t: f32) -> f64 {
    (f64::from(t) * 100.0).round() / 100.0
}

/// Optional request parameters a model may reject. `None` / `false` means "leave it out".
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Extras {
    pub temperature: Option<f32>,
    pub reasoning: bool,
}

/// Which optional parameters an HTTP 400 names: `(temperature, reasoning)`.
fn rejected_params(f: &Failure) -> (bool, bool) {
    match f {
        Failure::Status { code: 400, message } => {
            let m = message.to_lowercase();
            (m.contains("temperature"), m.contains("reasoning"))
        }
        _ => (false, false),
    }
}

fn remembered(set: &Mutex<HashSet<String>>, model: &str) -> bool {
    set.lock().unwrap_or_else(|e| e.into_inner()).contains(model)
}

fn remember(set: &Mutex<HashSet<String>>, model: &str) {
    set.lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(model.to_owned());
}

/// Retry and fallback rules shared by all providers: one retry with backoff on 429, 500, 502,
/// 503, 529, timeout or network error, and a retry without `temperature` / `reasoning` when the
/// API rejects it.
pub struct Retry {
    provider: Provider,
    backoff: Duration,
    /// Models whose API rejected `temperature`; remembered for the rest of the session.
    no_temperature: Mutex<HashSet<String>>,
    /// Same for `reasoning`.
    no_reasoning: Mutex<HashSet<String>>,
}

impl Retry {
    pub fn new(provider: Provider, backoff: Duration) -> Self {
        Self {
            provider,
            backoff,
            no_temperature: Mutex::new(HashSet::new()),
            no_reasoning: Mutex::new(HashSet::new()),
        }
    }

    /// Runs `attempt(extras)` until it succeeds or the rules above give up.
    pub async fn run<F, Fut>(
        &self,
        req: &LlmRequest<'_>,
        attempt: F,
    ) -> Result<LlmResponse, ApiError>
    where
        F: Fn(Extras) -> Fut,
        Fut: Future<Output = Result<LlmResponse, Failure>>,
    {
        let provider = self.provider.name();
        let mut extras = Extras {
            temperature: req
                .temperature
                .filter(|_| !remembered(&self.no_temperature, req.model)),
            reasoning: self.provider.sends_reasoning()
                && !remembered(&self.no_reasoning, req.model),
        };
        let mut retried = false;
        loop {
            let started = Instant::now();
            let result = attempt(extras).await;
            let ms = started.elapsed().as_millis() as u64;
            let failure = match result {
                Ok(reply) => {
                    tracing::info!(
                        provider,
                        model = req.model,
                        ms,
                        input_tokens = reply.input_tokens,
                        output_tokens = reply.output_tokens,
                        truncated = reply.truncated,
                        "API ok"
                    );
                    return Ok(reply);
                }
                Err(f) => f,
            };
            match &failure {
                Failure::Status { code, message } => {
                    tracing::warn!(provider, model = req.model, ms, status = *code, api_message = %message, "API error")
                }
                Failure::NoCredits => {
                    tracing::warn!(provider, model = req.model, ms, "API: no credits")
                }
                Failure::Timeout => tracing::warn!(provider, model = req.model, ms, "API timeout"),
                Failure::Network => {
                    tracing::warn!(provider, model = req.model, ms, "API network error")
                }
            }

            let (names_temperature, names_reasoning) = rejected_params(&failure);
            let drop_temperature = names_temperature && extras.temperature.is_some();
            let drop_reasoning = names_reasoning && extras.reasoning;
            if drop_temperature || drop_reasoning {
                if drop_temperature {
                    remember(&self.no_temperature, req.model);
                    extras.temperature = None;
                }
                if drop_reasoning {
                    remember(&self.no_reasoning, req.model);
                    extras.reasoning = false;
                }
                continue;
            }
            if failure.retryable() && !retried {
                retried = true;
                tokio::time::sleep(self.backoff).await;
                continue;
            }
            return Err(failure.into_error(self.provider));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_messages_match_the_spec() {
        use Provider::{Anthropic, OpenAi};
        assert_eq!(
            ApiError::NoKey(Anthropic).to_string(),
            "No Anthropic API key — open Settings"
        );
        assert_eq!(
            ApiError::NoKey(OpenAi).to_string(),
            "No OpenAI API key — open Settings"
        );
        assert_eq!(
            ApiError::Unauthorized(OpenAi).to_string(),
            "Invalid OpenAI API key"
        );
        assert_eq!(
            ApiError::NoCredits(Anthropic).to_string(),
            "No credits left on your Anthropic account"
        );
        assert_eq!(
            ApiError::RateLimited.to_string(),
            "Too many requests, wait a moment"
        );
        assert_eq!(
            ApiError::Overloaded.to_string(),
            "Service overloaded, try again"
        );
        assert_eq!(ApiError::NoResponse.to_string(), "No response from server");
    }

    #[test]
    fn status_mapping() {
        for p in Provider::ALL {
            let map = |code| {
                Failure::Status {
                    code,
                    message: String::new(),
                }
                .into_error(p)
            };
            assert_eq!(map(401), ApiError::Unauthorized(p));
            assert_eq!(map(429), ApiError::RateLimited);
            assert_eq!(map(500), ApiError::Overloaded);
            assert_eq!(map(502), ApiError::Overloaded);
            assert_eq!(map(503), ApiError::Overloaded);
            assert_eq!(map(529), ApiError::Overloaded);
            assert_eq!(map(403), ApiError::Other(403));
            assert_eq!(Failure::NoCredits.into_error(p), ApiError::NoCredits(p));
            assert_eq!(Failure::Timeout.into_error(p), ApiError::NoResponse);
            assert_eq!(Failure::Network.into_error(p), ApiError::NoResponse);
        }
    }

    #[test]
    fn retryable_statuses_match_the_spec() {
        let status = |code| Failure::Status {
            code,
            message: String::new(),
        };
        for code in [429, 500, 502, 503, 529] {
            assert!(status(code).retryable(), "{code}");
        }
        for code in [400, 401, 403, 404] {
            assert!(!status(code).retryable(), "{code}");
        }
        assert!(Failure::Timeout.retryable() && Failure::Network.retryable());
        assert!(!Failure::NoCredits.retryable());
    }

    #[test]
    fn provider_serializes_as_lowercase_ids() {
        assert_eq!(
            serde_json::to_string(&Provider::Anthropic).unwrap(),
            "\"anthropic\""
        );
        assert_eq!(
            serde_json::to_string(&Provider::OpenAi).unwrap(),
            "\"openai\""
        );
        assert_eq!(
            serde_json::from_str::<Provider>("\"openai\"").unwrap(),
            Provider::OpenAi
        );
        assert!(serde_json::from_str::<Provider>("\"gemini\"").is_err());
    }
}
