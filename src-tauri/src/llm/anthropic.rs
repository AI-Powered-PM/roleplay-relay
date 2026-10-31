//! Anthropic Messages API client. Non-streaming, 20 s timeout; retry rules live in `super::Retry`.

use std::time::Duration;

use reqwest::header::HeaderValue;
use secrecy::{ExposeSecret, SecretString};
use serde_json::{json, Value};

use super::{
    execute, round_temperature, ApiError, BoxFuture, Extras, Failure, LlmProvider, LlmReply,
    LlmRequest, LlmResponse, Provider, Retry, BACKOFF, TIMEOUT,
};

const ENDPOINT_PATH: &str = "/v1/messages";
pub const DEFAULT_BASE_URL: &str = "https://api.anthropic.com";
const API_VERSION: &str = "2023-06-01";

pub struct Client {
    http: reqwest::Client,
    base_url: String,
    retry: Retry,
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
            retry: Retry::new(Provider::Anthropic, backoff),
        }
    }

    async fn attempt(
        &self,
        key: &SecretString,
        req: &LlmRequest<'_>,
        extras: Extras,
    ) -> Result<LlmResponse, Failure> {
        let body = build_body(req, extras.temperature);

        let mut key_header =
            HeaderValue::from_str(key.expose_secret()).map_err(|_| Failure::Status {
                code: 401,
                message: "API key is not a valid header value".into(),
            })?;
        key_header.set_sensitive(true);

        let (status, text) = execute(
            self.http
                .post(format!("{}{}", self.base_url, ENDPOINT_PATH))
                .header("x-api-key", key_header)
                .header("anthropic-version", API_VERSION)
                .header("content-type", "application/json")
                .body(body.to_string()),
        )
        .await?;
        if !(200..300).contains(&status) {
            let message = api_error_message(&text);
            if is_no_credits(status, &message) {
                return Err(Failure::NoCredits);
            }
            return Err(Failure::Status {
                code: status,
                message,
            });
        }
        parse_reply(&text).ok_or(Failure::Status {
            code: 200,
            message: "unparseable reply".into(),
        })
    }
}

impl LlmProvider for Client {
    fn complete<'a>(
        &'a self,
        key: &'a SecretString,
        req: &'a LlmRequest<'a>,
    ) -> BoxFuture<'a, Result<LlmResponse, ApiError>> {
        Box::pin(async move {
            self.retry
                .run(req, |extras| self.attempt(key, req, extras))
                .await
        })
    }
}

/// The Messages API request body.
fn build_body(req: &LlmRequest<'_>, temperature: Option<f32>) -> Value {
    let mut body = json!({
        "model": req.model,
        "max_tokens": req.max_tokens,
        "system": req.system,
        "messages": [{ "role": "user", "content": req.user }],
    });
    if let Some(t) = temperature {
        body["temperature"] = json!(round_temperature(t));
    }
    body
}

/// Anthropic error bodies look like `{"type":"error","error":{"type":"...","message":"..."}}`.
/// That text is safe to log; anything else is dropped.
fn api_error_message(body: &str) -> String {
    serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v["error"]["message"].as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// Out of credits is an HTTP 400 whose message mentions the credit balance.
fn is_no_credits(status: u16, message: &str) -> bool {
    status == 400 && message.to_lowercase().contains("credit balance")
}

fn parse_reply(body: &str) -> Option<LlmResponse> {
    let v: Value = serde_json::from_str(body).ok()?;
    let text: String = v["content"]
        .as_array()?
        .iter()
        .filter(|b| b["type"] == "text")
        .filter_map(|b| b["text"].as_str())
        .collect();
    Some(LlmResponse {
        reply: LlmReply::Text(text),
        input_tokens: v["usage"]["input_tokens"].as_u64().unwrap_or(0),
        output_tokens: v["usage"]["output_tokens"].as_u64().unwrap_or(0),
        truncated: v["stop_reason"] == "max_tokens",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::test_support::{body_of, mock, Canned};
    use tokio::net::TcpListener;

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

    fn client(url: &str) -> Client {
        Client::with_config(url, Duration::from_secs(5), Duration::from_millis(5))
    }

    fn key() -> SecretString {
        SecretString::from("test-key-123".to_owned())
    }

    fn req(temp: Option<f32>) -> LlmRequest<'static> {
        LlmRequest {
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
            LlmResponse {
                reply: LlmReply::Text("Привет".into()),
                input_tokens: 11,
                output_tokens: 7,
                truncated: false
            }
        );

        let raw = seen.lock().unwrap()[0].clone();
        let lower = raw.to_lowercase();
        assert!(raw.starts_with("POST /v1/messages "));
        assert!(lower.contains("x-api-key: test-key-123"));
        assert!(lower.contains("anthropic-version: 2023-06-01"));
        assert!(lower.contains("content-type: application/json"));
        let body = body_of(&raw);
        assert_eq!(body["model"], "claude-haiku-5-5");
        assert_eq!(body["max_tokens"], 800);
        assert_eq!(body["temperature"], 0.2);
        assert_eq!(body["system"], "SYS");
        assert_eq!(
            body["messages"],
            json!([{ "role": "user", "content": "USER" }])
        );
        assert!(body.get("stream").is_none());
        assert!(body.get("reasoning").is_none() && body.get("store").is_none());
    }

    #[test]
    fn request_body_is_byte_identical_to_the_pre_refactor_client() {
        // Captured from the client as it was in claude.rs before the llm/ split.
        let with_temp = r#"{"max_tokens":800,"messages":[{"content":"USER","role":"user"}],"model":"claude-haiku-5-5","system":"SYS","temperature":0.2}"#;
        let without_temp = r#"{"max_tokens":800,"messages":[{"content":"USER","role":"user"}],"model":"claude-haiku-5-5","system":"SYS"}"#;
        assert_eq!(build_body(&req(Some(0.2)), Some(0.2)).to_string(), with_temp);
        assert_eq!(build_body(&req(Some(0.2)), None).to_string(), without_temp);
        assert_eq!(build_body(&req(None), None).to_string(), without_temp);
    }

    #[tokio::test]
    async fn retries_once_on_429_then_succeeds() {
        let (url, seen) = mock(vec![err(429, "slow"), ok("fine")]).await;
        let reply = client(&url).complete(&key(), &req(None)).await.unwrap();
        assert_eq!(reply.text(), "fine");
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
        assert_eq!(e, ApiError::Unauthorized(Provider::Anthropic));
        assert_eq!(e.to_string(), "Invalid Anthropic API key");
        assert_eq!(seen.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn credit_balance_400_is_no_credits_and_not_retried() {
        let (url, seen) = mock(vec![
            err(
                400,
                "Your credit balance is too low to access the Anthropic API.",
            ),
            ok("never"),
        ])
        .await;
        let e = client(&url).complete(&key(), &req(None)).await.unwrap_err();
        assert_eq!(e, ApiError::NoCredits(Provider::Anthropic));
        assert_eq!(e.to_string(), "No credits left on your Anthropic account");
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
            c.complete(&key(), &req(Some(0.7))).await.unwrap().text(),
            "one"
        );
        assert_eq!(
            c.complete(&key(), &req(Some(0.7))).await.unwrap().text(),
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
    fn reply_parsing_joins_text_blocks_and_skips_others() {
        let body = json!({
            "content": [
                { "type": "text", "text": "a" },
                { "type": "thinking", "thinking": "x" },
                { "type": "text", "text": "b" }
            ]
        })
        .to_string();
        assert_eq!(parse_reply(&body).unwrap().text(), "ab");
        assert!(parse_reply("not json").is_none());
        assert!(parse_reply("{}").is_none());
    }

    #[test]
    fn max_tokens_stop_reason_marks_the_reply_truncated() {
        let cut = json!({
            "content": [{ "type": "text", "text": "half a sent" }],
            "stop_reason": "max_tokens"
        })
        .to_string();
        let r = parse_reply(&cut).unwrap();
        assert!(r.truncated);
        assert_eq!(r.text(), "half a sent");
        let done = json!({
            "content": [{ "type": "text", "text": "ok" }],
            "stop_reason": "end_turn"
        })
        .to_string();
        assert!(!parse_reply(&done).unwrap().truncated);
    }
}
