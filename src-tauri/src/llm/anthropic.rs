//! Anthropic Messages API client. Non-streaming, 20 s timeout; retry rules live in `super::Retry`.

use std::time::Duration;

use reqwest::header::HeaderValue;
use secrecy::{ExposeSecret, SecretString};
use serde_json::{json, Value};

use super::{
    ApiError, BoxFuture, Failure, LlmProvider, LlmReply, LlmRequest, LlmResponse, Retry, BACKOFF,
    TIMEOUT,
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
            retry: Retry::new(backoff),
        }
    }

    async fn attempt(
        &self,
        key: &SecretString,
        req: &LlmRequest<'_>,
        temperature: Option<f32>,
    ) -> Result<LlmResponse, Failure> {
        let body = build_body(req, temperature);

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

impl LlmProvider for Client {
    fn complete<'a>(
        &'a self,
        key: &'a SecretString,
        req: &'a LlmRequest<'a>,
    ) -> BoxFuture<'a, Result<LlmResponse, ApiError>> {
        Box::pin(async move {
            self.retry
                .run(req, |temperature| self.attempt(key, req, temperature))
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
        // Round through f32 noise so the body says 0.2, not 0.20000000298.
        body["temperature"] = json!((f64::from(t) * 100.0).round() / 100.0);
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
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
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
}
