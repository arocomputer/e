//! Wire dialects: one module per upstream API shape. Each exposes
//! `run(&Request, &Sender<Event>)` with the contract documented on
//! `providers::stream` — exactly one terminal event per call.
//!
//! Every dialect reads the same way: `run` builds the `body` (from `history`
//! and the request's options), `send`s it, and hands the response to a
//! `Reader`, whose `read` loop names each wire frame it understands and
//! returns at the dialect's terminal frame.

pub mod anthropic;
pub mod completions;
pub mod google;
pub mod responses;

use crate::providers::{ResponseContext, SseStream, ToolCall};

/// A successful response's body as SSE, its stream errors tagged with the
/// response they came from.
fn event_stream(
    response: reqwest::Response,
) -> SseStream<impl futures::Stream<Item = reqwest::Result<impl AsRef<[u8]>>> + Unpin> {
    let context = ResponseContext::from_response(&response);
    SseStream::new(response.bytes_stream()).with_response(context)
}

/// A call with nothing known yet, for dialects that learn its id, name, and
/// arguments from later fragments.
fn blank_call() -> ToolCall {
    ToolCall {
        id: String::new(),
        name: String::new(),
        arguments: String::new(),
        signature: None,
    }
}
