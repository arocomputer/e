//! Classify provider failures and retain bounded diagnostics for retry decisions.

/// Why a provider request failed, and what that implies for retrying it. The
/// retry decision hangs off this alone, never off matching message text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureCause {
    /// Credentials are missing or were rejected locally, or the provider
    /// answered 401/403. Retrying cannot help; the user must sign in.
    Auth,
    /// Connection/DNS/TLS/setup failure — the request never left. Safe to
    /// retry.
    Network,
    /// Written but never confirmed complete: a header-wait or idle-body
    /// timeout, or the body transport broke mid-stream. May have been
    /// billed; the agent only retries when nothing has streamed yet, so a
    /// retry is a calculated risk, not a certainty.
    Stalled,
    /// HTTP 429, or a provider error frame naming a rate limit. Retry,
    /// honoring `Retry-After` when the provider sent one.
    RateLimited,
    /// The account cannot run this request at all — a subscription or
    /// free-tier wall (quota, billing, budget), not a transient throttle.
    /// Retrying only burns the backoff ladder against a hard limit; fail
    /// fast instead. Classified from the error body's own wording, since
    /// gateways (notably OpenCode Zen Go) return these as 429s or 403s
    /// that a status-only classifier would retry forever.
    QuotaExhausted,
    /// HTTP 408/500/502/503/504, or a provider error frame naming an outage
    /// — the provider is unwell right now, not that the request was bad.
    /// Retry.
    ProviderUnavailable,
    /// A rejected request (other 4xx), a provider error frame naming
    /// something else, or a stream that broke after content already
    /// arrived. Retrying would either fail identically or risk
    /// double-running something.
    Rejected,
}

impl FailureCause {
    /// Whether this cause alone permits a retry. Callers must still confirm
    /// nothing has streamed yet for the current attempt before acting on it.
    pub fn is_retryable(self) -> bool {
        matches!(
            self,
            FailureCause::Network
                | FailureCause::Stalled
                | FailureCause::RateLimited
                | FailureCause::ProviderUnavailable
        )
    }

    /// Classify an HTTP status the provider actually returned.
    pub(super) fn from_status(status: reqwest::StatusCode) -> FailureCause {
        match status.as_u16() {
            401 | 403 => FailureCause::Auth,
            429 => FailureCause::RateLimited,
            408 | 500 | 502 | 503 | 504 => FailureCause::ProviderUnavailable,
            _ => FailureCause::Rejected,
        }
    }

    /// A short human name for the retry row and the exhausted-campaign error.
    pub fn label(self) -> &'static str {
        match self {
            FailureCause::Auth => "Authentication",
            FailureCause::Network => "Network interrupted",
            FailureCause::Stalled => "No response from provider",
            FailureCause::RateLimited => "Rate limited",
            FailureCause::ProviderUnavailable => "Provider unavailable",
            FailureCause::QuotaExhausted => "Quota exhausted",
            FailureCause::Rejected => "Request failed",
        }
    }
}

/// Where the provider failure was observed, not a guess about its origin.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FailureStage {
    Authorization,
    Request,
    ResponseHeaders,
    ResponseBody,
    Stream,
}

/// Only allowlisted response metadata is retained. Never copy all headers.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ResponseContext {
    pub http_status: Option<u16>,
    pub request_id: Option<String>,
}

impl ResponseContext {
    /// Capture a bounded provider correlation id before consuming the body.
    pub fn from_response(response: &reqwest::Response) -> Self {
        let request_id = [
            "x-request-id",
            "request-id",
            "anthropic-request-id",
            "x-goog-request-id",
        ]
        .iter()
        .find_map(|name| response.headers().get(*name).and_then(|v| v.to_str().ok()))
        .map(|id| crate::tools::sanitize_display(&id.chars().take(256).collect::<String>()));
        Self {
            http_status: Some(response.status().as_u16()),
            request_id,
        }
    }
}

/// A typed provider failure, retained for retry decisions and backend diagnosis.
#[derive(Debug, Clone)]
pub struct ProviderError {
    /// Backend detail. The TUI uses the classified summary instead.
    pub message: String,
    /// Bounded diagnostic body when the compatibility message is shorter.
    pub detail: Option<Box<str>>,
    /// Squeezed to a line the activity row can hold, e.g. "504 Gateway
    /// Timeout". Equal to `message` when there is nothing shorter to say.
    pub short: String,
    pub cause: FailureCause,
    /// Seconds the provider asked us to wait (`Retry-After`), if it sent one.
    pub retry_after: Option<u64>,
    pub stage: FailureStage,
    pub response: Box<ResponseContext>,
    pub provider_code: Option<String>,
}

/// Identify account limits that retries cannot resolve, even with HTTP 429/403.
/// Includes provider-specific billing error names.
const QUOTA_EXHAUSTED_PATTERNS: &[&str] = &[
    "GoUsageLimitError",
    "FreeUsageLimitError",
    "monthly usage limit reached",
    "available balance",
    "insufficient_quota",
    "out of budget",
    "quota exceeded",
    "billing limit",
    "billing quota",
    "billing cap",
];

/// Error-body wording that marks a transient failure worth retrying
/// regardless of the HTTP status it traveled with (gateways wrap 503s in
/// 400s; streams die with transport phrasing).
const RETRYABLE_TEXT_PATTERNS: &[&str] = &[
    "overloaded",
    "service.?unavailable",
    "server.?error",
    "internal.?error",
    "provider.?returned.?error",
    "exceeded request buffer limit while retrying upstream",
    "network.?error",
    "connection.?error",
    "connection.?refused",
    "connection.?lost",
    "other side closed",
    "fetch failed",
    "getaddrinfo",
    "ENOTFOUND",
    "EAI_AGAIN",
    "upstream.?connect",
    "reset before headers",
    "socket hang up",
    "socket connection was closed",
    "timed.?out",
    "timeout",
    "terminated",
    "websocket.?closed",
    "websocket.?error",
    "ended without",
    "stream ended before message_stop",
    "stream ended before a terminal response event",
    "http2 request did not get a response",
    "retry delay",
    "you can retry your request",
    "try your request again",
    "please retry your request",
];

/// Throttle wording — retried with the `Retry-After`-aware delay rather
/// than the generic ladder.
const RATE_LIMITED_TEXT_PATTERNS: &[&str] =
    &["rate.?limit", "too many requests", "ResourceExhausted"];

fn compile(patterns: &[&str]) -> regex::Regex {
    // The pattern lists are static literals reviewed alongside this code;
    // an invalid one is a build bug CI catches, not a runtime state.
    // Scoped allow, proof: compile-time data.
    #[allow(clippy::expect_used)]
    regex::Regex::new(&format!("(?i){}", patterns.join("|"))).expect("pattern lists compile")
}

struct TextMatchers {
    quota: regex::Regex,
    rate: regex::Regex,
    retryable: regex::Regex,
}

fn text_matchers() -> &'static TextMatchers {
    static MATCHERS: std::sync::OnceLock<TextMatchers> = std::sync::OnceLock::new();
    MATCHERS.get_or_init(|| TextMatchers {
        quota: compile(QUOTA_EXHAUSTED_PATTERNS),
        rate: compile(RATE_LIMITED_TEXT_PATTERNS),
        retryable: compile(RETRYABLE_TEXT_PATTERNS),
    })
}

/// Classify an error message or HTTP body by its own wording, for the two
/// cases a status code cannot see: a hard quota wall wearing a 429, and a
/// transient failure wearing a generic 400. `None` when the text names no
/// recognizable cause — the caller's status-based classification stands.
pub fn classify_text(text: &str) -> Option<FailureCause> {
    let m = text_matchers();
    if m.quota.is_match(text) {
        return Some(FailureCause::QuotaExhausted);
    }
    if m.rate.is_match(text) {
        return Some(FailureCause::RateLimited);
    }
    if m.retryable.is_match(text) {
        return Some(FailureCause::ProviderUnavailable);
    }
    None
}

/// Bound both compatibility errors and stored diagnostics before cloning them.
fn bounded_diagnostic(text: &str) -> String {
    const LIMIT: usize = 8192;
    let mut chars = text.chars();
    let mut bounded: String = chars.by_ref().take(LIMIT).collect();
    if chars.next().is_some() {
        bounded.push_str(" [truncated]");
    }
    bounded
}

impl ProviderError {
    pub fn auth(message: impl Into<String>) -> Self {
        let message = message.into();
        ProviderError {
            short: message.clone(),
            message,
            cause: FailureCause::Auth,
            retry_after: None,
            stage: FailureStage::Authorization,
            response: Box::default(),
            provider_code: None,
            detail: None,
        }
    }
    pub fn network(message: impl Into<String>) -> Self {
        let message = message.into();
        ProviderError {
            short: message.clone(),
            message,
            cause: FailureCause::Network,
            retry_after: None,
            stage: FailureStage::Request,
            response: Box::default(),
            provider_code: None,
            detail: None,
        }
    }
    pub fn stalled(message: impl Into<String>) -> Self {
        let message = message.into();
        ProviderError {
            short: message.clone(),
            message,
            cause: FailureCause::Stalled,
            retry_after: None,
            stage: FailureStage::Stream,
            response: Box::default(),
            provider_code: None,
            detail: None,
        }
    }
    pub fn rejected(message: impl Into<String>) -> Self {
        let message = message.into();
        ProviderError {
            short: message.clone(),
            message,
            cause: FailureCause::Rejected,
            retry_after: None,
            stage: FailureStage::Stream,
            response: Box::default(),
            provider_code: None,
            detail: None,
        }
    }
    /// A provider error frame delivered mid-stream, already classified by
    /// the dialect that parsed it (e.g. Anthropic's `overloaded_error`).
    pub fn frame(message: impl Into<String>, cause: FailureCause) -> Self {
        let message = bounded_diagnostic(&message.into());
        ProviderError {
            short: message.clone(),
            message,
            cause,
            retry_after: None,
            stage: FailureStage::Stream,
            response: Box::default(),
            provider_code: None,
            detail: None,
        }
    }
    /// A bare `{"error":{…}}` frame inside a 200 stream — how OpenAI-style
    /// gateways and Gemini report a failure once the connection is open.
    /// The message's wording wins where it is specific (a quota wall);
    /// otherwise numeric status codes and named provider codes classify it.
    pub fn from_error_frame(error: &serde_json::Value) -> Self {
        let message = error["message"]
            .as_str()
            .or_else(|| error.as_str())
            .unwrap_or("unknown provider error")
            .to_string();
        let text_cause = classify_text(&error.to_string());
        let code = error
            .get("code")
            .filter(|v| !v.is_null())
            .unwrap_or(&error["type"]);
        let code = code
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| code.to_string());
        let cause = if text_cause == Some(FailureCause::QuotaExhausted) {
            FailureCause::QuotaExhausted
        } else {
            match code.as_str() {
                "401" | "403" | "invalid_api_key" | "authentication_error" | "permission_error" => {
                    FailureCause::Auth
                }
                "429" | "rate_limit_exceeded" | "rate_limit_error" => FailureCause::RateLimited,
                "408" | "server_error" | "internal_server_error" | "overloaded_error" => {
                    FailureCause::ProviderUnavailable
                }
                code if code.parse::<u16>().is_ok_and(|n| (500..=599).contains(&n)) => {
                    FailureCause::ProviderUnavailable
                }
                _ => text_cause.unwrap_or(FailureCause::Rejected),
            }
        };
        ProviderError::frame(message, cause).with_code_value(
            error
                .get("code")
                .filter(|v| !v.is_null())
                .unwrap_or(&error["type"]),
        )
    }
    /// Classify an HTTP status the provider actually returned; `body` is the
    /// response text the caller already read. The body's own wording wins
    /// where it is more specific than the status: a quota wall inside a 429
    /// is not a retryable throttle, and a wrapped 503 inside a 400 is not a
    /// rejected request.
    pub fn from_status(status: reqwest::StatusCode, body: &str) -> Self {
        let status_cause = FailureCause::from_status(status);
        let quota_can_override = status == reqwest::StatusCode::TOO_MANY_REQUESTS
            || (status.is_client_error() && status != reqwest::StatusCode::UNAUTHORIZED);
        let cause = match classify_text(body) {
            Some(FailureCause::QuotaExhausted) if quota_can_override => {
                FailureCause::QuotaExhausted
            }
            Some(text_cause) if status_cause == FailureCause::Rejected => text_cause,
            _ => status_cause,
        };
        let short = format!(
            "{} {}",
            status.as_u16(),
            status.canonical_reason().unwrap_or("error")
        );
        let snippet: String = body.chars().take(300).collect();
        let diagnostic: String = body.chars().take(4096).collect();
        let diagnostic = if body.chars().count() > 4096 {
            format!("{diagnostic} [truncated]")
        } else {
            diagnostic
        };
        let message = if snippet.is_empty() {
            short.clone()
        } else {
            format!("{short}: {snippet}")
        };
        let body_json = serde_json::from_str::<serde_json::Value>(body).unwrap_or_default();
        let code = body_json["error"]
            .get("code")
            .filter(|v| !v.is_null())
            .or_else(|| body_json["error"].get("type"))
            .or_else(|| body_json.get("code"))
            .unwrap_or(&serde_json::Value::Null);
        ProviderError {
            message,
            short,
            cause,
            retry_after: None,
            stage: FailureStage::ResponseBody,
            response: Box::new(ResponseContext {
                http_status: Some(status.as_u16()),
                request_id: None,
            }),
            provider_code: None,
            detail: Some(diagnostic.into_boxed_str()),
        }
        .with_code_value(code)
    }
    /// Keep diagnostic text bounded even when an SSE error frame is enormous.
    pub fn diagnostic(&self) -> String {
        bounded_diagnostic(self.detail.as_deref().unwrap_or(&self.message))
    }

    /// HTTP and SSE codes may be strings or numeric status values.
    pub fn with_code_value(mut self, value: &serde_json::Value) -> Self {
        self.provider_code = match value {
            serde_json::Value::String(code) => Some(code.chars().take(256).collect()),
            serde_json::Value::Number(code) => Some(code.to_string()),
            _ => None,
        };
        self
    }

    /// Attach metadata from the response whose body or frame failed.
    pub fn with_response(mut self, response: ResponseContext) -> Self {
        self.response = Box::new(response);
        self
    }

    /// Preserve the provider's machine-readable code separately from its wording.
    pub fn with_code(mut self, code: Option<&str>) -> Self {
        self.provider_code = code.map(|s| s.chars().take(256).collect());
        self
    }

    pub fn with_retry_after(mut self, seconds: Option<u64>) -> Self {
        self.retry_after = seconds;
        self
    }
}
