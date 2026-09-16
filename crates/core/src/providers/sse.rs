//! Bounded incremental SSE framing shared by the four wire dialects.

use super::{
    transport::next_sse_chunk_within, FinishReason, ProviderError, ResponseContext, StreamEnd,
    STREAM_IDLE_SECS,
};

/// Incremental SSE splitter: feed raw bytes, get complete `data:` payloads.
/// Handles CRLF, multi-line data fields, and the `[DONE]` sentinel (returned
/// as a payload; dialects decide what it means).
pub struct SseSplitter {
    buffer: Vec<u8>,
    scan_from: usize,
    oversized: bool,
}

impl Default for SseSplitter {
    fn default() -> Self {
        Self::new()
    }
}

impl SseSplitter {
    pub fn new() -> Self {
        SseSplitter {
            buffer: Vec::new(),
            scan_from: 0,
            oversized: false,
        }
    }

    /// Convenience entry point for tests and already-decoded sources.
    pub fn feed(&mut self, chunk: &str) -> Vec<String> {
        self.feed_bytes(chunk.as_bytes())
    }

    /// Preserve arbitrary HTTP body chunk boundaries as bytes. Decoding each
    /// chunk separately would replace a UTF-8 code point split across two
    /// chunks before the complete SSE event could be reassembled.
    pub fn feed_bytes(&mut self, chunk: &[u8]) -> Vec<String> {
        self.buffer.extend_from_slice(chunk);
        let mut events = Vec::new();
        // Events are separated by a blank line.
        while let Some(relative) = find_event_end(&self.buffer[self.scan_from..]) {
            let pos = self.scan_from + relative;
            let rest_at = skip_separator(&self.buffer, pos);
            if pos > MAX_SSE_EVENT_BYTES {
                self.buffer.drain(..rest_at);
                self.scan_from = 0;
                self.oversized = true;
                continue;
            }
            let raw = String::from_utf8_lossy(&self.buffer[..pos]).into_owned();
            self.buffer.drain(..rest_at);
            self.scan_from = 0;
            let mut data = String::new();
            for line in raw.lines() {
                let line = line.strip_suffix('\r').unwrap_or(line);
                if let Some(value) = line.strip_prefix("data:") {
                    if !data.is_empty() {
                        data.push('\n');
                    }
                    data.push_str(value.strip_prefix(' ').unwrap_or(value));
                }
            }
            if !data.is_empty() {
                events.push(data);
            }
        }
        // Only the final three old bytes can begin a delimiter completed by a
        // later chunk. Resuming there makes a byte-at-a-time malformed stream
        // linear rather than repeatedly rescanning its whole pending frame.
        if self.buffer.len() > MAX_SSE_EVENT_BYTES {
            self.buffer.clear();
            self.scan_from = 0;
            self.oversized = true;
        } else {
            self.scan_from = self.buffer.len().saturating_sub(3);
        }
        events
    }

    fn take_oversized(&mut self) -> bool {
        std::mem::take(&mut self.oversized)
    }
}

/// A single SSE event should be a small JSON frame. Leave ample room for a
/// provider that emits one unusually large tool call, but never let a broken
/// gateway grow an unterminated frame without bound.
pub const MAX_SSE_EVENT_BYTES: usize = 8 * 1024 * 1024;

/// Drives a streaming response body as SSE. `next()` yields complete `data:`
/// payloads with idle time bounded; the body ending before the dialect saw
/// its terminal frame is a broken stream, not a successful empty reply, so
/// EOF is an error by contract. Payloads that fail the dialect's JSON parse
/// are reported to `malformed()` and counted rather than silently vanishing.
pub struct SseStream<S> {
    stream: S,
    splitter: SseSplitter,
    queue: std::collections::VecDeque<String>,
    malformed: u32,
    event_timeout: std::time::Duration,
    pub response: ResponseContext,
}

impl<S, T, E> SseStream<S>
where
    S: futures::Stream<Item = Result<T, E>> + Unpin,
    T: AsRef<[u8]>,
    E: std::error::Error + 'static,
{
    pub fn new(stream: S) -> Self {
        SseStream {
            stream,
            splitter: SseSplitter::new(),
            queue: std::collections::VecDeque::new(),
            malformed: 0,
            event_timeout: std::time::Duration::from_secs(STREAM_IDLE_SECS),
            response: ResponseContext::default(),
        }
    }

    /// Associate errors from this stream with its HTTP response.
    pub fn with_response(mut self, response: ResponseContext) -> Self {
        self.response = response;
        self
    }

    /// Testable form of `new`: the timeout measures time until a complete SSE
    /// event, not time until the next arbitrary transport chunk.
    pub fn with_event_timeout(stream: S, event_timeout: std::time::Duration) -> Self {
        let mut value = Self::new(stream);
        value.event_timeout = event_timeout;
        value
    }

    /// The next complete payload; EOF fails as a stall (see the type docs).
    pub async fn next(&mut self) -> Result<String, ProviderError> {
        self.next_payload()
            .await
            .map_err(|error| error.with_response(self.response.clone()))
    }

    /// Read one event before the public boundary attaches response metadata.
    async fn next_payload(&mut self) -> Result<String, ProviderError> {
        let deadline = tokio::time::Instant::now() + self.event_timeout;
        loop {
            if let Some(payload) = self.queue.pop_front() {
                return Ok(payload);
            }
            if self.splitter.take_oversized() {
                return Err(ProviderError::rejected(format!(
                    "provider sent an SSE event larger than {MAX_SSE_EVENT_BYTES} bytes"
                )));
            }
            let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
            if remaining.is_zero() {
                return Err(ProviderError::stalled(
                    "stream stalled before completing an SSE event",
                ));
            }
            match next_sse_chunk_within(&mut self.stream, remaining).await? {
                Some(chunk) => {
                    self.queue.extend(self.splitter.feed_bytes(chunk.as_ref()));
                }
                None => return Err(ProviderError::stalled("stream ended unexpectedly")),
            }
        }
    }

    /// Record one payload the dialect could not parse.
    pub fn malformed(&mut self) {
        self.malformed += 1;
    }

    /// Finish successfully: fold the hygiene counters into the turn result.
    pub fn end(&self, finish: FinishReason) -> StreamEnd {
        StreamEnd {
            finish,
            malformed: self.malformed,
        }
    }
}

fn find_event_end(buf: &[u8]) -> Option<usize> {
    let find = |needle: &[u8]| {
        buf.windows(needle.len())
            .position(|window| window == needle)
    };
    let lf = find(b"\n\n");
    let crlf = find(b"\r\n\r\n");
    match (lf, crlf) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

fn skip_separator(buf: &[u8], pos: usize) -> usize {
    if buf[pos..].starts_with(b"\r\n\r\n") {
        pos + 4
    } else {
        pos + 2
    }
}
