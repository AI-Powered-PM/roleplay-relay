//! OpenAI Responses API client (not Chat Completions). Non-streaming, 20 s timeout; retry and
//! optional-parameter fallback live in `super::Retry`.

use std::time::Duration;

use reqwest::header::HeaderValue;
use secrecy::{ExposeSecret, SecretString};
use serde_json::{json, Value};

use super::{
    execute, round_temperature, ApiError, BoxFuture, Extras, Failure, LlmProvider, LlmReply,
    LlmRequest, LlmResponse, Provider, Retry, BACKOFF, TIMEOUT,
};

const ENDPOINT_PATH: &str = "/v1/responses";
pub const DEFAULT_BASE_URL: &str = "https://api.openai.com";

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
            retry: Retry::new(Provider::OpenAi, backoff),
        }
    }

    async fn attempt(
        &self,
        key: &SecretString,
        req: &LlmRequest<'_>,
        extras: Extras,
    ) -> Result<LlmResponse, Failure> {
        let body = build_body(req, extras);

        let mut auth = HeaderValue::from_str(&format!("Bearer {}", key.expose_secret()))
            .map_err(|_| Failure::Status {
                code: 401,
                message: "API key is not a valid header value".into(),
            })?;
        auth.set_sensitive(true);

        let (status, text) = execute(
            self.http
                .post(format!("{}{}", self.base_url, ENDPOINT_PATH))
                .header("authorization", auth)
                .header("content-type", "application/json")
                .body(body.to_string()),
        )
        .await?;
        if !(200..300).contains(&status) {
            let err = api_error(&text);
            if is_no_credits(status, &err.code) {
                return Err(Failure::NoCredits);
            }
            return Err(Failure::Status {
                code: status,
                message: err.message,
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

/// The Responses API request body. `store: false` keeps chat text out of OpenAI's stored
/// responses; `reasoning: none` keeps short chat lines fast and `temperature` accepted.
fn build_body(req: &LlmRequest<'_>, extras: Extras) -> Value {
    let mut body = json!({
        "model": req.model,
        "instructions": req.system,
        "input": req.user,
        "max_output_tokens": req.max_tokens,
        "store": false,
    });
    if extras.reasoning {
        body["reasoning"] = json!({ "effort": "none" });
    }
    if let Some(t) = extras.temperature {
        body["temperature"] = json!(round_temperature(t));
    }
    body
}

struct ApiErrorBody {
    /// Message plus the offending `param`, if any. Safe to log: it never contains the key.
    message: String,
    code: String,
}

/// OpenAI error bodies look like `{"error":{"message":"...","type":"...","param":"...","code":"..."}}`.
fn api_error(body: &str) -> ApiErrorBody {
    let v: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let e = &v["error"];
    let mut message = e["message"].as_str().unwrap_or_default().to_owned();
    if let Some(param) = e["param"].as_str() {
        message.push_str(&format!(" (param: {param})"));
    }
    ApiErrorBody {
        message,
        code: e["code"].as_str().unwrap_or_default().to_owned(),
    }
}

/// Out of credits is an HTTP 429 with code `insufficient_quota` (plain 429 is rate limiting).
fn is_no_credits(status: u16, code: &str) -> bool {
    status == 429 && code == "insufficient_quota"
}

fn parse_reply(body: &str) -> Option<LlmResponse> {
    let v: Value = serde_json::from_str(body).ok()?;
    let status = v["status"].as_str().unwrap_or("completed");
    if status == "failed" {
        return None;
    }
    let mut text = String::new();
    let mut refusal: Option<String> = None;
    for item in v["output"].as_array()? {
        if item["type"] != "message" {
            continue;
        }
        for part in item["content"].as_array().into_iter().flatten() {
            match part["type"].as_str() {
                Some("output_text") => text.push_str(part["text"].as_str().unwrap_or_default()),
                Some("refusal") => {
                    let r = refusal.get_or_insert_with(String::new);
                    r.push_str(part["refusal"].as_str().unwrap_or_default());
                }
                _ => {}
            }
        }
    }
    // A refusal wins over any partial text: nothing the model declined to say is used.
    let reply = match refusal {
        Some(r) => LlmReply::Refusal(r),
        None => LlmReply::Text(text),
    };
    Some(LlmResponse {
        reply,
        input_tokens: v["usage"]["input_tokens"].as_u64().unwrap_or(0),
        output_tokens: v["usage"]["output_tokens"].as_u64().unwrap_or(0),
        truncated: status == "incomplete",
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::test_support::{body_of, mock, Canned};
    use tokio::net::TcpListener;

    fn canned(status: u16, body: Value) -> Canned {
        Canned {
            status,
            body: body.to_string(),
            delay: Duration::ZERO,
        }
    }

    fn ok(text: &str) -> Canned {
        canned(
            200,
            json!({
                "status": "completed",
                "output": [
                    { "type": "reasoning", "summary": [] },
                    { "type": "message", "content": [{ "type": "output_text", "text": text }] }
                ],
                "usage": { "input_tokens": 11, "output_tokens": 7 }
            }),
        )
    }

    fn err(status: u16, message: &str, code: Option<&str>) -> Canned {
        canned(
            status,
            json!({ "error": { "message": message, "type": "x", "param": null, "code": code } }),
        )
    }

    fn client(url: &str) -> Client {
        Client::with_config(url, Duration::from_secs(5), Duration::from_millis(5))
    }

    fn key() -> SecretString {
        SecretString::from("test-key-123".to_owned())
    }

    fn req(temp: Option<f32>) -> LlmRequest<'static> {
        LlmRequest {
            model: "gpt-6-luna",
            max_tokens: 800,
            temperature: temp,
            system: "SYS",
            user: "USER",
        }
    }

    #[tokio::test]
    async fn success_sends_expected_request_and_parses_reply() {
        let (url, seen) = mock(vec![ok("Hola")]).await;
        let reply = client(&url)
            .complete(&key(), &req(Some(0.2)))
            .await
            .unwrap();
        assert_eq!(
            reply,
            LlmResponse {
                reply: LlmReply::Text("Hola".into()),
                input_tokens: 11,
                output_tokens: 7,
                truncated: false
            }
        );

        let raw = seen.lock().unwrap()[0].clone();
        let lower = raw.to_lowercase();
        assert!(raw.starts_with("POST /v1/responses "));
        assert!(lower.contains("authorization: bearer test-key-123"));
        assert!(!lower.contains("x-api-key"));
        assert!(lower.contains("content-type: application/json"));
        let body = body_of(&raw);
        assert_eq!(
            body,
            json!({
                "model": "gpt-6-luna",
                "instructions": "SYS",
                "input": "USER",
                "max_output_tokens": 800,
                "temperature": 0.2,
                "reasoning": { "effort": "none" },
                "store": false
            })
        );
    }

    #[test]
    fn request_body_always_has_store_false_and_leaves_out_dropped_params() {
        let full = build_body(
            &req(Some(0.3)),
            Extras {
                temperature: Some(0.3),
                reasoning: true,
            },
        );
        assert_eq!(full["store"], false);
        assert_eq!(full["reasoning"], json!({ "effort": "none" }));
        assert_eq!(full["temperature"], 0.3);

        let bare = build_body(
            &req(None),
            Extras {
                temperature: None,
                reasoning: false,
            },
        );
        assert_eq!(bare["store"], false);
        assert!(bare.get("temperature").is_none() && bare.get("reasoning").is_none());
        assert!(bare.get("messages").is_none() && bare.get("max_tokens").is_none());
    }

    #[test]
    fn reply_parsing_joins_output_text_parts_and_skips_other_items() {
        let body = json!({
            "status": "completed",
            "output": [
                { "type": "reasoning" },
                { "type": "message", "content": [
                    { "type": "output_text", "text": "a" },
                    { "type": "annotation_thing" },
                    { "type": "output_text", "text": "b" }
                ] },
                { "type": "message", "content": [{ "type": "output_text", "text": "c" }] }
            ]
        })
        .to_string();
        assert_eq!(parse_reply(&body).unwrap().text(), "abc");
        assert!(parse_reply("not json").is_none());
        assert!(parse_reply("{}").is_none());
        assert!(parse_reply(&json!({ "status": "failed", "output": [] }).to_string()).is_none());
    }

    #[test]
    fn refusal_part_becomes_a_refusal_reply() {
        let body = json!({
            "status": "completed",
            "output": [{ "type": "message", "content": [
                { "type": "refusal", "refusal": "I can't help with that." }
            ] }]
        })
        .to_string();
        let r = parse_reply(&body).unwrap();
        assert_eq!(r.reply, LlmReply::Refusal("I can't help with that.".into()));
        assert_eq!(r.text(), "I can't help with that.");
    }

    #[test]
    fn refusal_wins_over_partial_text() {
        let body = json!({
            "status": "completed",
            "output": [{ "type": "message", "content": [
                { "type": "output_text", "text": "<en>Sure, here" },
                { "type": "refusal", "refusal": "No." }
            ] }]
        })
        .to_string();
        assert_eq!(
            parse_reply(&body).unwrap().reply,
            LlmReply::Refusal("No.".into())
        );
    }

    #[test]
    fn incomplete_status_marks_the_reply_truncated() {
        let body = json!({
            "status": "incomplete",
            "incomplete_details": { "reason": "max_output_tokens" },
            "output": [{ "type": "message", "content": [
                { "type": "output_text", "text": "half a sent" }
            ] }]
        })
        .to_string();
        let r = parse_reply(&body).unwrap();
        assert!(r.truncated);
        assert_eq!(r.text(), "half a sent");
    }

    #[tokio::test]
    async fn refusal_end_to_end() {
        let refusal = canned(
            200,
            json!({
                "status": "completed",
                "output": [{ "type": "message", "content": [
                    { "type": "refusal", "refusal": "Declined." }
                ] }]
            }),
        );
        let (url, _) = mock(vec![refusal]).await;
        let r = client(&url).complete(&key(), &req(None)).await.unwrap();
        assert_eq!(r.reply, LlmReply::Refusal("Declined.".into()));
    }

    #[tokio::test]
    async fn retries_once_on_429_then_succeeds() {
        let (url, seen) = mock(vec![err(429, "slow down", Some("rate_limit_exceeded")), ok("fine")]).await;
        let reply = client(&url).complete(&key(), &req(None)).await.unwrap();
        assert_eq!(reply.text(), "fine");
        assert_eq!(seen.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn retries_502_and_503() {
        for code in [502, 503] {
            let (url, seen) = mock(vec![err(code, "bad gateway", None), ok("fine")]).await;
            let reply = client(&url).complete(&key(), &req(None)).await.unwrap();
            assert_eq!(reply.text(), "fine");
            assert_eq!(seen.lock().unwrap().len(), 2);
        }
    }

    #[tokio::test]
    async fn gives_up_after_one_retry() {
        let (url, seen) = mock(vec![err(503, "busy", None), err(503, "busy", None), ok("never")]).await;
        let e = client(&url).complete(&key(), &req(None)).await.unwrap_err();
        assert_eq!(e, ApiError::Overloaded);
        assert_eq!(seen.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn no_retry_on_401() {
        let (url, seen) = mock(vec![err(401, "Incorrect API key", Some("invalid_api_key")), ok("never")]).await;
        let e = client(&url).complete(&key(), &req(None)).await.unwrap_err();
        assert_eq!(e, ApiError::Unauthorized(Provider::OpenAi));
        assert_eq!(e.to_string(), "Invalid OpenAI API key");
        assert_eq!(seen.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn insufficient_quota_is_no_credits_and_not_retried() {
        let (url, seen) = mock(vec![
            err(429, "You exceeded your current quota.", Some("insufficient_quota")),
            ok("never"),
        ])
        .await;
        let e = client(&url).complete(&key(), &req(None)).await.unwrap_err();
        assert_eq!(e, ApiError::NoCredits(Provider::OpenAi));
        assert_eq!(e.to_string(), "No credits left on your OpenAI account");
        assert_eq!(seen.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn temperature_rejection_retries_without_it_and_is_remembered() {
        let (url, seen) = mock(vec![
            err(400, "Unsupported parameter: 'temperature' is not supported with this model.", None),
            ok("one"),
            ok("two"),
        ])
        .await;
        let c = client(&url);
        assert_eq!(c.complete(&key(), &req(Some(0.7))).await.unwrap().text(), "one");
        assert_eq!(c.complete(&key(), &req(Some(0.7))).await.unwrap().text(), "two");

        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 3);
        assert!(body_of(&seen[0]).get("temperature").is_some());
        for raw in &seen[1..] {
            let b = body_of(raw);
            assert!(b.get("temperature").is_none(), "must not be sent again");
            assert_eq!(b["reasoning"], json!({ "effort": "none" }), "reasoning stays");
        }
    }

    #[tokio::test]
    async fn reasoning_rejection_retries_without_it_and_is_remembered() {
        let (url, seen) = mock(vec![
            err(400, "Unsupported parameter: 'reasoning.effort' is not supported with this model.", None),
            ok("one"),
            ok("two"),
        ])
        .await;
        let c = client(&url);
        assert_eq!(c.complete(&key(), &req(Some(0.7))).await.unwrap().text(), "one");
        assert_eq!(c.complete(&key(), &req(Some(0.7))).await.unwrap().text(), "two");

        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 3);
        assert!(body_of(&seen[0]).get("reasoning").is_some());
        for raw in &seen[1..] {
            let b = body_of(raw);
            assert!(b.get("reasoning").is_none(), "must not be sent again");
            assert_eq!(b["temperature"], 0.7, "temperature stays");
            assert_eq!(b["store"], false);
        }
    }

    #[tokio::test]
    async fn rejection_is_remembered_per_model() {
        let (url, seen) = mock(vec![
            err(400, "'temperature' is not supported", None),
            ok("one"),
            ok("two"),
        ])
        .await;
        let c = client(&url);
        c.complete(&key(), &req(Some(0.7))).await.unwrap();
        let other = LlmRequest {
            model: "gpt-6.1-terra",
            ..req(Some(0.7))
        };
        c.complete(&key(), &other).await.unwrap();
        assert!(body_of(&seen.lock().unwrap()[2]).get("temperature").is_some());
    }

    #[tokio::test]
    async fn param_named_in_the_param_field_counts_as_named() {
        let named = canned(
            400,
            json!({ "error": { "message": "Unsupported value.", "param": "temperature", "code": "unsupported_value" } }),
        );
        let (url, seen) = mock(vec![named, ok("fine")]).await;
        let r = client(&url)
            .complete(&key(), &req(Some(0.7)))
            .await
            .unwrap();
        assert_eq!(r.text(), "fine");
        assert!(body_of(&seen.lock().unwrap()[1]).get("temperature").is_none());
    }

    #[tokio::test]
    async fn other_400_is_not_retried() {
        let (url, seen) = mock(vec![err(400, "max_output_tokens too small", None), ok("never")]).await;
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
}
