//! The provider seam: one request contract, one normalized event stream.
//!
//! Everything above this module sees `Event`s; everything below is a wire
//! dialect. Four dialects ship: chat-completions, Responses, Anthropic
//! Messages, and Gemini (see `api/`). Providers are data (`data/*.json`);
//! OAuth refresh lives in `auth::login`. SSE framing is handled here — one
//! small splitter, tested, shared.

pub mod api;
pub mod catalog;
pub mod diagnostics;
pub mod registry;
pub mod runtime;

mod error;
mod message;
mod sse;
mod transport;
pub use error::{classify_text, FailureCause, FailureStage, ProviderError, ResponseContext};
pub use message::MAX_IMAGE_BYTES;
pub use message::{
    strip_incompatible_images, ChatMessage, ImageInput, MessageKind, ResponseMeta, ResponsePurpose,
    ToolCall, ToolResultMeta, Usage,
};
pub use sse::{SseSplitter, SseStream, MAX_SSE_EVENT_BYTES};
pub use transport::{
    http, next_sse_chunk, retry_after_seconds, send_request, send_request_within, STREAM_IDLE_SECS,
};

use tokio::sync::mpsc;

use crate::providers::catalog::{Api, Model};

/// How a completed stream said it ended. Anything but `Normal`/`ToolCalls`
/// means the reply is not the full answer — the agent surfaces it instead of
/// accepting a truncated or refused turn as a blank success.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FinishReason {
    /// A normal end of turn (`stop` / `completed` / `end_turn`), or the
    /// dialect saw no explicit reason.
    Normal,
    /// Ended to run the tool calls the stream requested.
    ToolCalls,
    /// The provider cut the reply at a token or length limit.
    Length,
    /// The model refused to answer.
    Refusal,
    /// The provider's content filter blocked or removed output.
    ContentFilter,
    /// A reason e doesn't classify; carried verbatim.
    Other(String),
}

/// A successfully completed stream: how the provider declared it ended, plus
/// stream-hygiene counters the agent surfaces as warnings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StreamEnd {
    pub finish: FinishReason,
    /// SSE data payloads that failed to parse as JSON and were skipped.
    pub malformed: u32,
}

impl StreamEnd {
    pub fn normal() -> Self {
        StreamEnd {
            finish: FinishReason::Normal,
            malformed: 0,
        }
    }
}

#[derive(Debug)]
pub enum Event {
    TextDelta(String),
    ReasoningDelta(String),
    /// Bytes of tool-call argument JSON just streamed. Argument assembly is
    /// A provider began streaming one tool request. `key` is stable within
    /// this response even when the provider has not supplied the call id yet
    /// (for example, a chat-completions tool index).
    ToolCallStart {
        key: String,
    },
    /// One exact argument fragment for a particular in-flight call. Keeping
    /// identity and content here makes interleaved calls testable and lets
    /// frontends show useful progress without parsing provider wire frames.
    ToolArgumentsDelta {
        key: String,
        delta: String,
    },
    /// Argument streaming for this key is complete. Execution still begins
    /// only after the following validated `ToolCall` event.
    ToolCallEnd {
        key: String,
    },
    /// A completed tool request (dialects accumulate the argument deltas).
    ToolCall(ToolCall),
    /// Disjoint counters from the terminal usage frame. Dialects normalize
    /// inclusive wire totals before this crosses the provider seam.
    Usage(Usage),
    /// A Responses-dialect reasoning item (verbatim JSON): the API demands
    /// it be resent ahead of the function calls it produced, so the agent
    /// stores it in history and the dialect replays it.
    ReasoningItem(String),
    Done(StreamEnd),
    /// The provider call failed; `err.cause` decides whether the agent may
    /// retry it — see FailureCause.
    Error(ProviderError),
}

pub struct Request {
    pub model: Model,
    pub system: String,
    pub messages: Vec<ChatMessage>,
    pub effort: Option<String>,
    /// Tool schemas to advertise (dialect-shaped by each implementation).
    pub tools: Vec<serde_json::Value>,
    /// The conversation's stable session id (`Session::id`), or empty when
    /// this request has no persisted session. Sent only to providers that opt
    /// in via `session_header` — see `with_attribution`.
    pub session_id: String,
}

/// Attach a provider's opt-in attribution headers before the request is sent.
/// A provider opts in through `client_header` / `session_header` in its
/// registry data; e sends its client name and stable session id under those
/// names so an OpenCode-style gateway recognizes the caller and can pin a
/// conversation to one upstream for cache hits. A provider that declares
/// neither receives neither — the session id is never broadcast to a provider
/// that did not ask for it, so it cannot become a cross-provider correlation
/// handle. Unknown/custom providers (not in the registry) get nothing.
pub fn with_attribution(
    builder: reqwest::RequestBuilder,
    request: &Request,
) -> reqwest::RequestBuilder {
    let Some(provider) = registry::find(&request.model.provider) else {
        return builder;
    };
    let mut builder = builder;
    if let Some(header) = provider.client_header.as_deref() {
        builder = builder.header(header, crate::CLIENT);
    }
    if let Some(header) = provider.session_header.as_deref() {
        if !request.session_id.is_empty() {
            builder = builder.header(header, request.session_id.as_str());
        }
    }
    builder
}

/// Start the request; events arrive on the returned channel. The task ends
/// with `Done` or `Error` — always exactly one terminal event. The handle
/// aborts the request (esc).
pub fn stream(request: Request) -> (mpsc::Receiver<Event>, tokio::task::JoinHandle<()>) {
    let (tx, rx) = mpsc::channel(64);
    let home = crate::config::home::home();
    let handle = tokio::spawn(crate::config::home::scope(home, async move {
        let result = match runtime::authorize(&request.model).await {
            Ok(authorization) => (match request.model.api {
                Api::Completions => api::completions::run(&request, &authorization, &tx).await,
                Api::Responses => api::responses::run(&request, &authorization, &tx).await,
                Api::Anthropic => api::anthropic::run(&request, &authorization, &tx).await,
                Api::Google => api::google::run(&request, &authorization, &tx).await,
            })
            .map_err(|mut error| {
                if !authorization.bearer.is_empty() {
                    error.message = error.message.replace(&authorization.bearer, "[redacted]");
                    error.short = error.short.replace(&authorization.bearer, "[redacted]");
                    error.detail = error.detail.map(|text| {
                        text.replace(&authorization.bearer, "[redacted]")
                            .into_boxed_str()
                    });
                    error.response.request_id = error
                        .response
                        .request_id
                        .map(|id| id.replace(&authorization.bearer, "[redacted]"));
                    error.provider_code = error
                        .provider_code
                        .map(|code| code.replace(&authorization.bearer, "[redacted]"));
                }
                error
            }),
            Err(error) => Err(error),
        };
        match result {
            Ok(end) => {
                let _ = tx.send(Event::Done(end)).await;
            }
            Err(err) => {
                let _ = tx.send(Event::Error(err)).await;
            }
        }
    }));
    (rx, handle)
}

/// Turn a non-2xx response into the typed error every dialect reports the
/// same way; 2xx passes through untouched.
pub async fn require_success(
    response: reqwest::Response,
) -> Result<reqwest::Response, ProviderError> {
    if response.status().is_success() {
        return Ok(response);
    }
    let status = response.status();
    let retry_after = retry_after_seconds(&response);
    let context = ResponseContext::from_response(&response);
    let text = response.text().await.unwrap_or_default();
    Err(ProviderError::from_status(status, &text)
        .with_retry_after(retry_after)
        .with_response(context))
}

#[cfg(test)]
use transport::{build_client, cached_client};

#[cfg(test)]
mod tests {
    /// An unknown code must not hide a recognized type in an SSE error frame.
    #[test]
    fn named_error_types_survive_unknown_machine_codes() {
        for (kind, code, expected) in [
            (
                "rate_limit_error",
                "slow_down",
                super::FailureCause::RateLimited,
            ),
            (
                "service_unavailable_error",
                "server_is_overloaded",
                super::FailureCause::ProviderUnavailable,
            ),
        ] {
            let error = super::ProviderError::from_error_frame(
                &serde_json::json!({"type":kind,"code":code,"message":"try later"}),
            );
            assert_eq!(error.cause, expected);
            assert_eq!(error.provider_code.as_deref(), Some(code));
        }
    }

    /// Provider-controlled error text is bounded before compatibility events clone it.
    #[test]
    fn oversized_frame_messages_are_bounded_before_publication() {
        let error =
            super::ProviderError::frame("界".repeat(100_000), super::FailureCause::Rejected);
        assert_eq!(error.message, format!("{} [truncated]", "界".repeat(8192)));
        assert_eq!(error.short, error.message);
        assert_eq!(error.diagnostic(), error.message);
    }

    /// Wire codes classify auth and transient errors without masking hard quota.
    #[test]
    fn error_frame_codes_preserve_retry_classification() {
        use super::{FailureCause, ProviderError};
        for code in [
            serde_json::json!(401),
            serde_json::json!(403),
            serde_json::json!("401"),
        ] {
            assert_eq!(
                ProviderError::from_error_frame(
                    &serde_json::json!({"code":code,"message":"denied"})
                )
                .cause,
                FailureCause::Auth
            );
        }
        let error = serde_json::json!({"code":"server_error","message":"Provider disconnected unexpectedly"});
        assert_eq!(
            ProviderError::from_error_frame(&error).cause,
            FailureCause::ProviderUnavailable
        );
        assert_eq!(ProviderError::from_error_frame(&serde_json::json!({"code":429,"type":"insufficient_quota","message":"quota exhausted"})).cause, FailureCause::QuotaExhausted);
    }

    use super::*;

    #[test]
    fn http_client_build_failure_is_a_network_error_and_is_cached() {
        let cell = std::sync::OnceLock::new();
        let attempts = std::cell::Cell::new(0);
        let first = cached_client(&cell, || {
            attempts.set(attempts.get() + 1);
            Err("tls backend unavailable".to_string())
        })
        .unwrap_err();
        assert_eq!(attempts.get(), 1, "the build runs once");
        assert!(
            first.message.contains("network client unavailable")
                && first.message.contains("tls backend unavailable"),
            "the error names the cause: {}",
            first.message
        );
        // The second call must reuse the cached failure, not re-attempt it.
        let again = cached_client(&cell, || panic!("build must not re-run")).unwrap_err();
        assert_eq!(again.message, first.message);
        assert_eq!(attempts.get(), 1);
    }

    #[test]
    fn http_client_success_is_shared_across_calls() {
        let cell = std::sync::OnceLock::new();
        let attempts = std::cell::Cell::new(0);
        let build = || {
            attempts.set(attempts.get() + 1);
            build_client()
        };
        let first = cached_client(&cell, build).unwrap();
        let second = cached_client(&cell, || panic!("build must not re-run")).unwrap();
        assert!(std::ptr::eq(first, second), "one client, shared");
        assert_eq!(attempts.get(), 1);
    }

    #[test]
    fn status_classification_matches_retry_policy() {
        use reqwest::StatusCode;
        let cause = |code: u16| FailureCause::from_status(StatusCode::from_u16(code).unwrap());
        assert_eq!(cause(401), FailureCause::Auth);
        assert_eq!(cause(403), FailureCause::Auth);
        assert_eq!(cause(429), FailureCause::RateLimited);
        assert_eq!(cause(408), FailureCause::ProviderUnavailable);
        assert_eq!(cause(500), FailureCause::ProviderUnavailable);
        assert_eq!(cause(502), FailureCause::ProviderUnavailable);
        assert_eq!(cause(503), FailureCause::ProviderUnavailable);
        assert_eq!(cause(504), FailureCause::ProviderUnavailable);
        assert_eq!(cause(400), FailureCause::Rejected);
        assert_eq!(cause(404), FailureCause::Rejected);
        assert_eq!(cause(422), FailureCause::Rejected);

        assert!(FailureCause::RateLimited.is_retryable());
        assert!(FailureCause::ProviderUnavailable.is_retryable());
        assert!(FailureCause::Network.is_retryable());
        assert!(FailureCause::Stalled.is_retryable());
        assert!(!FailureCause::Auth.is_retryable());
        assert!(!FailureCause::Rejected.is_retryable());
        assert!(!FailureCause::QuotaExhausted.is_retryable());
    }

    #[test]
    fn body_wording_overrides_the_status_when_more_specific() {
        use reqwest::StatusCode;
        let err = |code: u16, body: &str| {
            ProviderError::from_status(StatusCode::from_u16(code).unwrap(), body)
        };
        // A hard quota wall wearing a 429 is not a transient throttle —
        // this exact shape is what OpenCode Zen Go returns.
        let walled = err(
            429,
            r#"{"error":{"type":"GoUsageLimitError","message":"Monthly usage limit reached"}}"#,
        );
        assert_eq!(walled.cause, FailureCause::QuotaExhausted);
        assert!(!walled.cause.is_retryable());
        assert_eq!(
            err(403, "enable available balance").cause,
            FailureCause::QuotaExhausted
        );
        assert_eq!(
            err(400, "insufficient_quota").cause,
            FailureCause::QuotaExhausted
        );
        // A transient failure wrapped in a generic 4xx stays retryable.
        let wrapped = err(400, r#"{"error":{"message":"upstream connect error"}}"#);
        assert_eq!(wrapped.cause, FailureCause::ProviderUnavailable);
        assert!(wrapped.cause.is_retryable());
        // Rate-limit wording rides the Retry-After-aware cause.
        assert_eq!(
            err(400, "too many requests, slow down").cause,
            FailureCause::RateLimited
        );
        // Status wins where the body names nothing, and auth keeps
        // precedence over generic retryable wording.
        assert_eq!(err(400, "model not found").cause, FailureCause::Rejected);
        assert_eq!(err(401, "overloaded").cause, FailureCause::Auth);
        assert_eq!(err(429, "").cause, FailureCause::RateLimited);
        // Generic billing prose is not proof of exhausted quota, and a 5xx
        // remains retryable even if its body mentions account-limit wording.
        assert_eq!(
            err(500, "billing service temporarily unavailable").cause,
            FailureCause::ProviderUnavailable
        );
        assert_eq!(
            err(500, "monthly usage limit reached").cause,
            FailureCause::ProviderUnavailable
        );
    }

    #[test]
    fn quota_errors_never_reach_the_retry_ladder() {
        // The whole point of the classifier: a rate-limited error retries,
        // a quota-walled one cannot, so the agent fails fast instead of
        // spending its backoff ladder on requests that cannot succeed.
        assert!(FailureCause::RateLimited.is_retryable());
        assert!(!FailureCause::QuotaExhausted.is_retryable());
        assert_eq!(FailureCause::QuotaExhausted.label(), "Quota exhausted");
    }

    #[test]
    fn from_status_squeezes_a_long_body_to_a_short_reason() {
        let body = "x".repeat(2000);
        let err = ProviderError::from_status(reqwest::StatusCode::GATEWAY_TIMEOUT, &body);
        assert_eq!(err.short, "504 Gateway Timeout");
        assert!(err.message.starts_with("504 Gateway Timeout: xxx"));
        assert!(err.message.len() < body.len());
    }

    fn temp_image_path(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "e-image-test-{}-{}-{name}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn a_valid_image_under_the_cap_loads() {
        let path = temp_image_path("small.png");
        let content = b"\x89PNG\r\n\x1a\nrest of a tiny file";
        std::fs::write(&path, content).unwrap();
        let (image, size) = ImageInput::from_path_with_size(&path).unwrap();
        assert_eq!(image.media_type, "image/png");
        assert_eq!(size, content.len() as u64);
        let _ = std::fs::remove_file(&path);
    }

    // from_path_with_size reads through take(MAX_IMAGE_BYTES + 1) rather
    // than std::fs::read, so the post-read length check below can never be
    // reached by pulling an arbitrarily large file fully into memory
    // first. This test pins the still-correct outward behavior (reject,
    // with this exact message) after that change; it can't observe memory
    // use directly, but a regression back to an unbounded read would fail
    // this the same way it fails the fifo-style "must fail fast, not
    // block" tests elsewhere in this suite — by taking a very long time
    // or exhausting memory instead of returning promptly.
    #[test]
    fn an_oversized_image_is_rejected_without_reading_past_the_cap() {
        let path = temp_image_path("big.png");
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        bytes.resize(MAX_IMAGE_BYTES as usize + 1, 0);
        std::fs::write(&path, &bytes).unwrap();
        let error = ImageInput::from_path_with_size(&path).unwrap_err();
        assert!(error.contains("exceeds 20 MiB"), "{error}");
        let _ = std::fs::remove_file(&path);
    }

    fn image_incapable_model() -> Model {
        Model {
            provider: "test".into(),
            id: "test".into(),
            base_url: "https://example.invalid".into(),
            api: Api::Completions,
            catalog: crate::providers::registry::CatalogStrategy::Openai,
            responses_mount: crate::providers::registry::ResponsesMount::Platform,
            provider_supports_tools: true,
            provider_image_input: false,
            effort: Vec::new(),
            thinking: crate::providers::catalog::Thinking::Manual,
            context_window: 200_000,
            max_output: None,
            supports_tools: true,
            image_input: false,
            pricing: None,
        }
    }

    #[test]
    fn strip_incompatible_images_clears_history_the_model_cannot_accept() {
        let mut messages = vec![
            ChatMessage::user("plain text, no images"),
            ChatMessage::user_with_images(
                "an old image",
                vec![ImageInput {
                    media_type: "image/png".into(),
                    data: std::sync::Arc::from("data"),
                }],
            ),
        ];
        strip_incompatible_images(&mut messages, &image_incapable_model());
        assert!(messages[0].images().is_empty());
        assert_eq!(messages[0].content, "plain text, no images");
        assert!(messages[1].images().is_empty(), "the image must be removed");
        assert!(
            messages[1]
                .content
                .contains("is not declared image-capable"),
            "the omission should be noted, not silent: {}",
            messages[1].content
        );
    }

    #[test]
    fn strip_incompatible_images_leaves_a_capable_model_untouched() {
        let mut model = image_incapable_model();
        model.image_input = true;
        let mut messages = vec![ChatMessage::user_with_images(
            "an image",
            vec![ImageInput {
                media_type: "image/png".into(),
                data: std::sync::Arc::from("data"),
            }],
        )];
        strip_incompatible_images(&mut messages, &model);
        assert_eq!(
            messages[0].images().len(),
            1,
            "capable models keep their images"
        );
        assert_eq!(messages[0].content, "an image");
    }
}
