//! Shared HTTP client, request deadlines, and transport error classification.

use super::{FailureStage, ProviderError};

/// The shared provider/auth client: one pool, so connections (and their
/// TLS handshakes) are reused across turns and tool steps. Connect is bounded;
/// the overall request is not — a live SSE stream can run for minutes. The
/// wait for response headers is bounded in `send_request`, idle bodies
/// per-chunk in `next_sse_chunk`.
/// Redirects are disabled: custom credential headers and private bodies must
/// stay at the configured endpoint. Release downloads use a separate client.
///
/// A failed build (the TLS backend cannot initialize) is an environment
/// state, not a code bug, so it surfaces as a network error at the point of
/// use: the request fails with a reason and the session survives. The
/// outcome is cached either way — TLS init does not heal mid-process, and
/// re-attempting a deterministic failure on every request only multiplies it.
pub fn http() -> Result<&'static reqwest::Client, ProviderError> {
    static CLIENT: std::sync::OnceLock<Result<reqwest::Client, String>> =
        std::sync::OnceLock::new();
    cached_client(&CLIENT, build_client)
}

/// The build steps for the shared client, kept separate from the cache glue
/// so the glue is testable without touching process-global state.
pub(super) fn build_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        // Credentials in custom headers and request bodies must never be
        // forwarded to a redirect target. Downloads use their own client.
        .redirect(reqwest::redirect::Policy::none())
        .user_agent(format!("e/{}", crate::VERSION))
        .connect_timeout(std::time::Duration::from_secs(30))
        // After a system sleep, pooled connections are dead but look
        // alive locally — a request that reuses one would sit out the
        // full stall budget before failing. Keepalives retire them
        // quickly on both sides instead.
        .tcp_keepalive(std::time::Duration::from_secs(15))
        .pool_idle_timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())
}

/// One cached build outcome, mapped to the error callers see. `build` runs
/// at most once per cell — a failure is remembered, not retried.
pub(super) fn cached_client(
    cell: &std::sync::OnceLock<Result<reqwest::Client, String>>,
    build: impl FnOnce() -> Result<reqwest::Client, String>,
) -> Result<&reqwest::Client, ProviderError> {
    match cell.get_or_init(build) {
        Ok(client) => Ok(client),
        Err(reason) => Err(ProviderError::network(format!(
            "network client unavailable: {reason}"
        ))),
    }
}

/// Seconds of provider silence — awaiting response headers or the next body
/// chunk — before the request is declared stalled.
pub const STREAM_IDLE_SECS: u64 = 180;

/// Send the request, bounding the wait for response headers. The client
/// bounds connect and `next_sse_chunk` bounds body reads; this closes the gap
/// between them — an accepted request the provider never answers — with the
/// same budget: silence is a stall wherever it happens.
pub async fn send_request(
    builder: reqwest::RequestBuilder,
) -> Result<reqwest::Response, ProviderError> {
    send_request_within(builder, std::time::Duration::from_secs(STREAM_IDLE_SECS)).await
}

/// `send_request` with an explicit bound (tests shrink it to milliseconds).
pub async fn send_request_within(
    builder: reqwest::RequestBuilder,
    wait: std::time::Duration,
) -> Result<reqwest::Response, ProviderError> {
    match tokio::time::timeout(wait, builder.send()).await {
        Ok(Ok(response)) => Ok(response),
        Ok(Err(e)) => {
            let message = format!("request failed: {}", transport_error_chain(&e));
            if e.is_builder() {
                // The request could not be built (a malformed base URL, an
                // invalid header) — no retry ladder changes that.
                Err(ProviderError::rejected(message))
            } else if e.is_connect() {
                Err(ProviderError::network(message))
            } else {
                // A loss while sending or awaiting headers does not prove
                // the provider never received the request.
                let mut error = ProviderError::stalled(message);
                error.stage = FailureStage::ResponseHeaders;
                Err(error)
            }
        }
        // The request was written but never answered — it may have been
        // delivered, so a retry is a calculated risk, not a certainty.
        Err(_) => {
            let mut error = ProviderError::stalled(format!(
                "no response from provider for {}s",
                wait.as_secs()
            ));
            error.stage = FailureStage::ResponseHeaders;
            Err(error)
        }
    }
}

/// Await the next SSE body chunk. An idle socket fails instead of hanging the
/// turn forever — the agent then ends with a visible error rather than a
/// spinner that Esc cannot clear.
pub async fn next_sse_chunk<S, T, E>(stream: &mut S) -> Result<Option<T>, ProviderError>
where
    S: futures::Stream<Item = Result<T, E>> + Unpin,
    E: std::error::Error + 'static,
{
    next_sse_chunk_within(stream, std::time::Duration::from_secs(STREAM_IDLE_SECS)).await
}

pub(super) async fn next_sse_chunk_within<S, T, E>(
    stream: &mut S,
    wait: std::time::Duration,
) -> Result<Option<T>, ProviderError>
where
    S: futures::Stream<Item = Result<T, E>> + Unpin,
    E: std::error::Error + 'static,
{
    match tokio::time::timeout(wait, futures::StreamExt::next(stream)).await {
        Ok(None) => Ok(None),
        Ok(Some(Ok(chunk))) => Ok(Some(chunk)),
        // A broken body transport (reset, truncated chunking) is retryable
        // by cause; the agent still refuses to retry once content streamed.
        Ok(Some(Err(e))) => Err(ProviderError::stalled(format!(
            "provider response interrupted: {}",
            transport_error_chain(&e)
        ))),
        Err(_) => Err(ProviderError::stalled(
            "stream stalled before completing an SSE event",
        )),
    }
}

/// Keep the cause hidden by reqwest's generic body-decoding headline.
/// Bound the chain and redact request URLs, which can contain credentials.
fn transport_error_chain(error: &(dyn std::error::Error + 'static)) -> String {
    let mut parts = Vec::new();
    let mut next = Some(error);
    for _ in 0..5 {
        let Some(error) = next else { break };
        let mut message = error.to_string();
        if let Some(url) = error.downcast_ref::<reqwest::Error>().and_then(|e| e.url()) {
            message = message.replace(url.as_str(), "<provider URL>");
        }
        if parts.last() != Some(&message) {
            parts.push(message);
        }
        next = error.source();
    }
    parts.join(": ")
}

/// Parse `Retry-After` as whole seconds. Every provider we talk to sends the
/// numeric form; the HTTP-date form doesn't appear in practice here.
pub fn retry_after_seconds(response: &reqwest::Response) -> Option<u64> {
    response
        .headers()
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()
}
