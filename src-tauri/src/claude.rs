//! Anthropic Messages API client. Non-streaming, 20 s timeout, one retry, temperature fallback.
//!
//! Logging rule: status codes, timings, token counts and Anthropic's own error JSON are safe.
//! Request bodies, response text, headers and `reqwest` error details are never logged.

use std::collections::HashSet;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use reqwest::header::HeaderValue;
use secrecy::{ExposeSecret, SecretString};
use serde_json::{json, Value};

const ENDPOINT_PATH: &str = "/v1/messages";
pub const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";
const API_VERSION: &str = "2023-06-01";
const TIMEOUT: Duration = Duration::from_secs(20);
const BACKOFF: Duration = Duration::from_secs(1);

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

pub struct Request<'a> {
    pub model: &'a str,
    pub max_tokens: u32,
    pub temperature: Option<f32>,
    pub system: &'a str,
    pub user: &'a str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    pub text: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

/// What one HTTP attempt can fail with, before it is mapped to a user-facing `ApiError`.
#[derive(Debug)]
enum Failure {
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

pub struct Client {
    http: reqwest::Client,
    base_url: String,
    backoff: Duration,
    /// Models whose API rejected `temperature`; remembered for the rest of the session.
    no_temperature: Mutex<HashSet<String>>,
}

impl Client {
    pub fn new() -> Self {
        Self::with_config(DEFAULT_BASE_URL, TIMEOUT, BACKOFF)
    }

    pub fn with_config(base_url: &str, timeout: Duration, backoff: Duration) -> Self {
        let http = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .expect("HTTP client config is valid");
        Self {
            http,
            base_url: base_url.trim_end_matches('/').to_owned(),
            backoff,
            no_temperature: Mutex::new(HashSet::new()),
        }
    }

    pub async fn complete(&self, key: &SecretString, req: &Request<'_>) -> Result<Reply, ApiError> {
        let mut temperature = req
            .temperature
            .filter(|_| !self.rejects_temperature(req.model));
        let mut retried = false;
        loop {
            let started = Instant::now();
            let result = self.attempt(key, req, temperature).await;
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

    fn rejects_temperature(&self, model: &str) -> bool {
        self.no_temperature
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .contains(model)
    }

    async fn attempt(
        &self,
        key: &SecretString,
        req: &Request<'_>,
        temperature: Option<f32>,
    ) -> Result<Reply, Failure> {
        let mut body = json!({
            "model": req.model,
            "max_tokens": req.max_tokens,
            "system": req.system,
            "messages": [{ "role": "user", "content": req.user }],
        });
        if let Some(t) = temperature {
            // Round through f32 noise so the body says 0.2, not 0.20000000298.
            body["temperature"] = json!((f64::from(t) * 100.0).round() / 100.0);
        }

        let mut key_header =
            HeaderValue::from_str(key.expose_secret()).map_err(|_| Failure::Status {
                code: 401,
                message: "API key is not a valid header value".into(),
            })?;
        key_header.set_sensitive(true);

        let response = self
            .http
            .post(format!("{}{}", self.base_url, ENDPOINT_PATH))
            .header("x-api-key", key_header)
            .header("anthropic-version", API_VERSION)
            .header("content-type", "application/json")
            .body(body.to_string())
            .send()
            .await
            // Deliberately drop the reqwest error: it can carry URL and request details.
            .map_err(|e| {
                if e.is_timeout() {
                    Failure::Timeout
                } else {
                    Failure::Network
                }
            })?;

        let status = response.status();
        let text = response.text().await.map_err(|e| {
            if e.is_timeout() {
                Failure::Timeout
            } else {
                Failure::Network
            }
        })?;
        if !status.is_success() {
            return Err(Failure::Status {
                code: status.as_u16(),
                message: api_error_message(&text),
            });
        }
        parse_reply(&text).ok_or(Failure::Status {
            code: 200,
            message: "unparseable reply".into(),
        })
    }
}

fn is_temperature_rejection(f: &Failure) -> bool {
    matches!(f, Failure::Status { code: 400, message } if message.to_lowercase().contains("temperature"))
}

/// Anthropic error bodies look like `{"type":"error","error":{"type":"...","message":"..."}}`.
/// That text is safe to log; anything else is dropped.
fn api_error_message(body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v["error"]["message"].as_str().map(str::to_owned))
        .unwrap_or_default()
}

fn parse_reply(body: &str) -> Option<Reply> {
    let v: Value = serde_json::from_str(body).ok()?;
    let text: String = v["content"]
        .as_array()?
        .iter()
        .filter(|b| b["type"] == "text")
        .filter_map(|b| b["text"].as_str())
        .collect();
    Some(Reply {
        text,
        input_tokens: v["usage"]["input_tokens"].as_u64().unwrap_or(0),
        output_tokens: v["usage"]["output_tokens"].as_u64().unwrap_or(0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    #[derive(Clone)]
    struct Canned {
        status: u16,
        body: String,
        delay: Duration,
    }

    fn ok(text: &str) -> Canned {
        Canned {
            status: 200,
            body: json!({
                "content": [{ "type": "text", "text": text }],
                "usage": { "input_tokens": 11, "output_tokens": 7 }
            })
            .to_string(),
            delay: Duration::ZERO,
        }
    }

    fn err(status: u16, message: &str) -> Canned {
        Canned {
            status,
            body: json!({ "type": "error", "error": { "type": "x", "message": message } })
                .to_string(),
            delay: Duration::ZERO,
        }
    }

    /// Tiny HTTP server: answers requests in order with the canned responses and records
    /// each raw request (headers + body).
    async fn mock(responses: Vec<Canned>) -> (String, Arc<Mutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::<String>::new()));
        let seen2 = seen.clone();
        let queue = Arc::new(Mutex::new(
            responses
                .into_iter()
                .collect::<std::collections::VecDeque<_>>(),
        ));
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                let seen = seen2.clone();
                let queue = queue.clone();
                tokio::spawn(async move {
                    let mut buf = Vec::new();
                    let mut chunk = [0u8; 4096];
                    loop {
                        let n = sock.read(&mut chunk).await.unwrap_or(0);
                        if n == 0 {
                            return;
                        }
                        buf.extend_from_slice(&chunk[..n]);
                        let text = String::from_utf8_lossy(&buf).into_owned();
                        if let Some(split) = text.find("\r\n\r\n") {
                            let len = text[..split]
                                .lines()
                                .find_map(|l| {
                                    l.to_lowercase()
                                        .strip_prefix("content-length:")
                                        .map(|v| v.trim().parse::<usize>().unwrap())
                                })
                                .unwrap_or(0);
                            if buf.len() >= split + 4 + len {
                                break;
                            }
                        }
                    }
                    seen.lock()
                        .unwrap()
                        .push(String::from_utf8_lossy(&buf).into_owned());
                    let canned = queue
                        .lock()
                        .unwrap()
                        .pop_front()
                        .expect("mock ran out of responses");
                    tokio::time::sleep(canned.delay).await;
                    let resp = format!(
                        "HTTP/1.1 {} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
                        canned.status,
                        canned.body.len(),
                        canned.body
                    );
                    let _ = sock.write_all(resp.as_bytes()).await;
                    let _ = sock.shutdown().await;
                });
            }
        });
        (url, seen)
    }

    fn client(url: &str) -> Client {
        Client::with_config(url, Duration::from_secs(5), Duration::from_millis(5))
    }

    fn key() -> SecretString {
        SecretString::from("test-key-123".to_owned())
    }

    fn req(temp: Option<f32>) -> Request<'static> {
        Request {
            model: "claude-haiku-5-5",
            max_tokens: 800,
            temperature: temp,
            system: "SYS",
            user: "USER",
        }
    }

    #[tokio::test]
    async fn success_sends_expected_request_and_parses_reply() {
        let (url, seen) = mock(vec![ok("Привет")]).await;
        let reply = client(&url)
            .complete(&key(), &req(Some(0.2)))
            .await
            .unwrap();
        assert_eq!(
            reply,
            Reply {
                text: "Привет".into(),
                input_tokens: 11,
                output_tokens: 7
            }
        );

        let raw = seen.lock().unwrap()[0].clone();
        let lower = raw.to_lowercase();
        assert!(raw.starts_with("POST /v1/messages "));
        assert!(lower.contains("x-api-key: test-key-123"));
        assert!(lower.contains("anthropic-version: 2023-06-01"));
        assert!(lower.contains("content-type: application/json"));
        let body: Value = serde_json::from_str(raw.split("\r\n\r\n").nth(1).unwrap()).unwrap();
        assert_eq!(body["model"], "claude-haiku-5-5");
        assert_eq!(body["max_tokens"], 800);
        assert_eq!(body["temperature"], 0.2);
        assert_eq!(body["system"], "SYS");
        assert_eq!(
            body["messages"],
            json!([{ "role": "user", "content": "USER" }])
        );
        assert!(body.get("stream").is_none());
    }

    #[tokio::test]
    async fn retries_once_on_429_then_succeeds() {
        let (url, seen) = mock(vec![err(429, "slow"), ok("fine")]).await;
        let reply = client(&url).complete(&key(), &req(None)).await.unwrap();
        assert_eq!(reply.text, "fine");
        assert_eq!(seen.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn gives_up_after_one_retry() {
        let (url, seen) = mock(vec![err(529, "busy"), err(529, "busy"), ok("never")]).await;
        let e = client(&url).complete(&key(), &req(None)).await.unwrap_err();
        assert_eq!(e, ApiError::Overloaded);
        assert_eq!(seen.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn no_retry_on_401() {
        let (url, seen) = mock(vec![err(401, "bad key"), ok("never")]).await;
        let e = client(&url).complete(&key(), &req(None)).await.unwrap_err();
        assert_eq!(e, ApiError::Unauthorized);
        assert_eq!(seen.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn temperature_rejection_retries_without_it_and_is_remembered() {
        let (url, seen) = mock(vec![
            err(400, "`temperature` is not supported for this model"),
            ok("one"),
            ok("two"),
        ])
        .await;
        let c = client(&url);
        assert_eq!(
            c.complete(&key(), &req(Some(0.7))).await.unwrap().text,
            "one"
        );
        assert_eq!(
            c.complete(&key(), &req(Some(0.7))).await.unwrap().text,
            "two"
        );

        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 3);
        assert!(seen[0].contains("\"temperature\""));
        assert!(!seen[1].contains("\"temperature\""));
        assert!(
            !seen[2].contains("\"temperature\""),
            "second call must not send it again"
        );
    }

    #[tokio::test]
    async fn other_400_is_not_retried() {
        let (url, seen) = mock(vec![err(400, "max_tokens too large"), ok("never")]).await;
        let e = client(&url)
            .complete(&key(), &req(Some(0.3)))
            .await
            .unwrap_err();
        assert_eq!(e, ApiError::Other(400));
        assert_eq!(seen.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn timeout_retries_once_then_reports_no_response() {
        let slow = Canned {
            delay: Duration::from_millis(400),
            ..ok("late")
        };
        let (url, seen) = mock(vec![slow.clone(), slow]).await;
        let c = Client::with_config(&url, Duration::from_millis(100), Duration::from_millis(5));
        let e = c.complete(&key(), &req(None)).await.unwrap_err();
        assert_eq!(e, ApiError::NoResponse);
        assert_eq!(seen.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn connection_refused_is_no_response() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);
        let e = client(&url).complete(&key(), &req(None)).await.unwrap_err();
        assert_eq!(e, ApiError::NoResponse);
    }

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

    #[test]
    fn reply_parsing_joins_text_blocks_and_skips_others() {
        let body = json!({
            "content": [
                { "type": "text", "text": "a" },
                { "type": "thinking", "thinking": "x" },
                { "type": "text", "text": "b" }
            ]
        })
        .to_string();
        assert_eq!(parse_reply(&body).unwrap().text, "ab");
        assert!(parse_reply("not json").is_none());
        assert!(parse_reply("{}").is_none());
    }
}
