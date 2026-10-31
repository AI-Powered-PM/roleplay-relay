//! Provider-agnostic LLM layer: one request/reply shape, the `LlmProvider` trait, and the retry
//! and error mapping every provider shares. Each provider is one file next to this one.
//!
//! Logging rule: status codes, timings, token counts and the provider's own error JSON are safe.
//! Request bodies, response text, headers and `reqwest` error details are never logged.

pub mod anthropic;

use std::collections::HashSet;
use std::future::Future;
use std::pin::Pin;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use secrecy::SecretString;

pub const TIMEOUT: Duration = Duration::from_secs(20);
pub const BACKOFF: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ApiError {
    #[error("No API key — open Settings")]
    NoKey,
    #[error("Invalid API key")]
    Unauthorized,
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
    /// Produced by providers that report refusals separately; Anthropic does not.
    #[allow(dead_code)]
    Refusal(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmResponse {
    pub reply: LlmReply,
    pub input_tokens: u64,
    pub output_tokens: u64,
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
    Status { code: u16, message: String },
}

impl Failure {
    fn retryable(&self) -> bool {
        match self {
            Failure::Timeout | Failure::Network => true,
            Failure::Status { code, .. } => matches!(code, 429 | 500 | 529),
        }
    }

    fn into_error(self) -> ApiError {
        match self {
            Failure::Timeout | Failure::Network => ApiError::NoResponse,
            Failure::Status { code, .. } => match code {
                401 => ApiError::Unauthorized,
                429 => ApiError::RateLimited,
                500..=599 => ApiError::Overloaded,
                other => ApiError::Other(other),
            },
        }
    }
}

fn is_temperature_rejection(f: &Failure) -> bool {
    matches!(f, Failure::Status { code: 400, message } if message.to_lowercase().contains("temperature"))
}

/// Retry and fallback rules shared by all providers: one retry with backoff on 429, 500, 529,
/// timeout or network error, and a retry without `temperature` when the API rejects it.
pub struct Retry {
    backoff: Duration,
    /// Models whose API rejected `temperature`; remembered for the rest of the session.
    no_temperature: Mutex<HashSet<String>>,
}

impl Retry {
    pub fn new(backoff: Duration) -> Self {
        Self {
            backoff,
            no_temperature: Mutex::new(HashSet::new()),
        }
    }

    fn rejects_temperature(&self, model: &str) -> bool {
        self.no_temperature
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains(model)
    }

    /// Runs `attempt(temperature)` until it succeeds or the rules above give up.
    pub async fn run<F, Fut>(
        &self,
        req: &LlmRequest<'_>,
        attempt: F,
    ) -> Result<LlmResponse, ApiError>
    where
        F: Fn(Option<f32>) -> Fut,
        Fut: Future<Output = Result<LlmResponse, Failure>>,
    {
        let mut temperature = req
            .temperature
            .filter(|_| !self.rejects_temperature(req.model));
        let mut retried = false;
        loop {
            let started = Instant::now();
            let result = attempt(temperature).await;
            let ms = started.elapsed().as_millis() as u64;
            let failure = match result {
                Ok(reply) => {
                    tracing::info!(
                        model = req.model,
                        ms,
                        input_tokens = reply.input_tokens,
                        output_tokens = reply.output_tokens,
                        "API ok"
                    );
                    return Ok(reply);
                }
                Err(f) => f,
            };
            match &failure {
                Failure::Status { code, message } => {
                    tracing::warn!(model = req.model, ms, status = *code, api_message = %message, "API error")
                }
                Failure::Timeout => tracing::warn!(model = req.model, ms, "API timeout"),
                Failure::Network => tracing::warn!(model = req.model, ms, "API network error"),
            }

            if temperature.is_some() && is_temperature_rejection(&failure) {
                self.no_temperature
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(req.model.to_owned());
                temperature = None;
                continue;
            }
            if failure.retryable() && !retried {
                retried = true;
                tokio::time::sleep(self.backoff).await;
                continue;
            }
            return Err(failure.into_error());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_messages_match_the_spec() {
        assert_eq!(ApiError::NoKey.to_string(), "No API key — open Settings");
        assert_eq!(ApiError::Unauthorized.to_string(), "Invalid API key");
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
        let map = |code| {
            Failure::Status {
                code,
                message: String::new(),
            }
            .into_error()
        };
        assert_eq!(map(401), ApiError::Unauthorized);
        assert_eq!(map(429), ApiError::RateLimited);
        assert_eq!(map(500), ApiError::Overloaded);
        assert_eq!(map(503), ApiError::Overloaded);
        assert_eq!(map(529), ApiError::Overloaded);
        assert_eq!(map(403), ApiError::Other(403));
        assert_eq!(Failure::Timeout.into_error(), ApiError::NoResponse);
        assert_eq!(Failure::Network.into_error(), ApiError::NoResponse);
    }
}
