//! The chat-completions dialect: `POST {base}/chat/completions`, Bearer key,
//! SSE deltas at `choices[0].delta`, streamed tool-call argument fragments
//! accumulated by index, a final usage frame, `[DONE]` sentinel.

use std::collections::BTreeMap;

use serde_json::{json, Map, Value};
use tokio::sync::mpsc;

use super::{blank_call, event_stream};
use crate::providers::runtime::Authorization;
use crate::providers::{
    http, require_success, retry_after_seconds, send_request, with_attribution, Event,
    FinishReason, ProviderError, Request, ResponseContext, StreamEnd, ToolCall, Usage,
};

/// Provider/model/level combinations that rejected our `reasoning_effort`
/// during this run. A backend may reject one level while accepting the rest,
/// and a custom base URL may reuse a provider and model id, so the full wire
/// destination and selected value belong in the key. Process-scoped and
/// fail-open: a poisoned lock merely causes another probe.
fn reasoning_rejected() -> &'static std::sync::Mutex<std::collections::HashSet<String>> {
    static SET: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> =
        std::sync::OnceLock::new();
    SET.get_or_init(Default::default)
}

fn reasoning_key(model: &crate::providers::catalog::Model, effort: &str) -> String {
    format!(
        "{}\n{}\n{}\n{effort}",
        model.provider, model.base_url, model.id
    )
}

fn reasoning_is_rejected(model: &crate::providers::catalog::Model, effort: &str) -> bool {
    let key = reasoning_key(model, effort);
    reasoning_rejected()
        .lock()
        .map(|set| set.contains(&key))
        .unwrap_or(false)
}

fn mark_reasoning_rejected(model: &crate::providers::catalog::Model, effort: &str) {
    let key = reasoning_key(model, effort);
    if let Ok(mut set) = reasoning_rejected().lock() {
        set.insert(key);
    }
}

pub async fn run(
    request: &Request,
    authorization: &Authorization,
    tx: &mpsc::Sender<Event>,
) -> Result<StreamEnd, ProviderError> {
    let body = body(request);
    let response = send_repaired(request, authorization, body).await?;
    Reader::new(tx).read(response).await
}

/// History as chat messages, the system prompt first.
fn history(request: &Request) -> Vec<Value> {
    let mut messages = vec![json!({"role": "system", "content": request.system})];
    for m in &request.messages {
        match m.role() {
            "assistant" if !m.tool_calls().is_empty() => {
                let calls: Vec<_> = m
                    .tool_calls()
                    .iter()
                    .map(|c| {
                        json!({"id": c.id, "type": "function",
                               "function": {"name": c.name, "arguments": c.arguments}})
                    })
                    .collect();
                let mut msg = json!({"role": "assistant", "tool_calls": calls});
                if !m.content.is_empty() {
                    msg["content"] = json!(m.content);
                }
                messages.push(msg);
            }
            "tool" => messages.push(json!({
                "role": "tool",
                "tool_call_id": m.tool_call_id().cloned().unwrap_or_default(),
                "content": m.content,
            })),
            // Responses-dialect reasoning items mean nothing here.
            "reasoning" => {}
            role => {
                if m.images().is_empty() {
                    messages.push(json!({"role": role, "content": m.content}));
                } else {
                    let mut content = Vec::new();
                    if !m.content.is_empty() {
                        content.push(json!({"type": "text", "text": m.content}));
                    }
                    content.extend(m.images().iter().map(|image| {
                        json!({
                            "type": "image_url",
                            "image_url": {"url": image.data_url()},
                        })
                    }));
                    messages.push(json!({"role": role, "content": content}));
                }
            }
        }
    }
    messages
}

/// The request body: history, usage reporting, effort, and tools as given.
fn body(request: &Request) -> Value {
    let mut body = json!({
        "model": request.model.id,
        "messages": history(request),
        "stream": true,
        "stream_options": {"include_usage": true},
    });
    // Reasoning effort rides the OpenAI-standard field; only sent when the
    // model declares a knob, so gateways that never heard of it never see
    // it. `off` has no wire encoding here — absence is the closest thing.
    // Skipped for a model already known to reject it (see `reasoning_rejected`).
    if let Some(effort) = request.effort.as_deref().filter(|ulo| *ulo != "off") {
        if !reasoning_is_rejected(&request.model, effort) {
            body["reasoning_effort"] = json!(effort);
        }
    }
    if !request.tools.is_empty() {
        body["tools"] = json!(request.tools);
    }
    body
}

/// One chat-completions request. `send_repaired` retries it once without an
/// optional field (`stream_options`, `reasoning_effort`) when a strict gateway
/// names that field — or the reasoning knob — in its validation error.
async fn send(
    request: &Request,
    authorization: &Authorization,
    body: &Value,
) -> Result<reqwest::Response, ProviderError> {
    send_request(with_attribution(
        http()?
            .post(format!("{}/chat/completions", request.model.base_url))
            .bearer_auth(&authorization.bearer)
            .header("accept", "text/event-stream")
            .json(body),
        request,
    ))
    .await
}

/// Send the body and require a 2xx, repairing a validation rejection once.
///
/// A validation rejection over an optional field we added is recoverable:
/// drop the offending field(s) and retry once, rather than carry a
/// provider-specific compatibility table. A rejected request generated no
/// content, so the retry is safe. The body is our own typed object; a
/// non-object can't be repaired, so it falls through to the error.
async fn send_repaired(
    request: &Request,
    authorization: &Authorization,
    mut body: Value,
) -> Result<reqwest::Response, ProviderError> {
    let first = send(request, authorization, &body).await?;
    if !matches!(first.status().as_u16(), 400 | 422) {
        return require_success(first).await;
    }
    let status = first.status();
    let retry_after = retry_after_seconds(&first);
    let response_context = ResponseContext::from_response(&first);
    let text = first.text().await.unwrap_or_default();
    let lower = text.to_ascii_lowercase();
    let rejected_effort = request.effort.as_deref().filter(|_| {
        // Do not treat every mention of "thinking" as an effort failure.
        // Validation errors about reasoning history or content must reach
        // the user unchanged. The standard field name is authoritative;
        // the second phrase is the Go gateway's field-less rejection.
        lower.contains("reasoning_effort")
            || lower.contains("thinking cannot be customized")
            || lower.contains("thinking can't be customized")
    });
    let repaired = body
        .as_object_mut()
        .is_some_and(|object| drop_named_fields(object, &lower, rejected_effort.is_some()));
    if !repaired {
        return Err(ProviderError::from_status(status, &text)
            .with_retry_after(retry_after)
            .with_response(response_context));
    }
    let healed = require_success(send(request, authorization, &body).await?).await?;
    // Remember only after the request without the field succeeds. A
    // second failure means effort was not the cause of the rejection.
    if let Some(effort) = rejected_effort {
        mark_reasoning_rejected(&request.model, effort);
    }
    Ok(healed)
}

/// Remove the optional fields a lowercased validation error blames; whether
/// anything was removed.
fn drop_named_fields(object: &mut Map<String, Value>, lower: &str, effort_rejected: bool) -> bool {
    let mut changed = false;
    // Usage is useful but not essential; strict OpenAI-compatible
    // gateways commonly name it in the validation error.
    if lower.contains("stream_options") && object.remove("stream_options").is_some() {
        changed = true;
    }
    if effort_rejected && object.remove("reasoning_effort").is_some() {
        changed = true;
    }
    changed
}

/// One response's stream: maps `choices[0]` deltas to [`Event`]s until
/// `[DONE]`.
struct Reader<'a> {
    tx: &'a mpsc::Sender<Event>,
    /// Tool-call fragments accumulate per stream index until [DONE].
    calls: BTreeMap<u64, ToolCall>,
    finish: FinishReason,
}

impl<'a> Reader<'a> {
    fn new(tx: &'a mpsc::Sender<Event>) -> Self {
        Self {
            tx,
            calls: BTreeMap::new(),
            finish: FinishReason::Normal,
        }
    }

    /// Read chunks until `[DONE]` or an error frame.
    async fn read(mut self, response: reqwest::Response) -> Result<StreamEnd, ProviderError> {
        let mut sse = event_stream(response);
        loop {
            let payload = sse.next().await?;
            if payload == "[DONE]" {
                self.close_calls().await;
                return Ok(sse.end(self.finish));
            }
            let Ok(value) = serde_json::from_str::<Value>(&payload) else {
                sse.malformed();
                continue;
            };
            // Error frames fail the response without flushing unfinished tool calls.
            if let Some(error) = value.get("error").filter(|error| !error.is_null()) {
                return Err(
                    ProviderError::from_error_frame(error).with_response(sse.response.clone())
                );
            }
            if let Some(delta) = value["choices"][0]["delta"].as_object() {
                self.delta(delta).await;
            }
            if let Some(reason) = value["choices"][0]["finish_reason"].as_str() {
                self.finish = finish_reason(reason);
                // A finish_reason of tool_calls closes the accumulation early.
                if self.finish == FinishReason::ToolCalls {
                    self.close_calls().await;
                }
            }
            if let Some(frame) = value.get("usage").filter(|u| !u.is_null()) {
                self.emit(Event::Usage(usage(frame))).await;
            }
        }
    }

    async fn emit(&self, event: Event) {
        let _ = self.tx.send(event).await;
    }

    /// One delta: answer text, reasoning under either common key, and tool
    /// call fragments.
    async fn delta(&mut self, delta: &Map<String, Value>) {
        if let Some(text) = delta.get("content").and_then(|v| v.as_str()) {
            if !text.is_empty() {
                self.emit(Event::TextDelta(text.into())).await;
            }
        }
        for key in ["reasoning_content", "reasoning"] {
            if let Some(text) = delta.get(key).and_then(|v| v.as_str()) {
                if !text.is_empty() {
                    self.emit(Event::ReasoningDelta(text.into())).await;
                }
            }
        }
        if let Some(calls) = delta.get("tool_calls").and_then(|v| v.as_array()) {
            for fragment in calls {
                self.call_fragment(fragment).await;
            }
        }
    }

    /// Fold one tool-call fragment into the call at its index, opening it on
    /// first sight. Names stream in pieces too.
    async fn call_fragment(&mut self, fragment: &Value) {
        let index = fragment["index"].as_u64().unwrap_or(0);
        if !self.calls.contains_key(&index) {
            self.emit(Event::ToolCallStart {
                key: index.to_string(),
            })
            .await;
        }
        let entry = self.calls.entry(index).or_insert_with(blank_call);
        if let Some(id) = fragment["id"].as_str() {
            entry.id = id.into();
        }
        if let Some(name) = fragment["function"]["name"].as_str() {
            entry.name.push_str(name);
        }
        if let Some(args) = fragment["function"]["arguments"].as_str() {
            entry.arguments.push_str(args);
            if !args.is_empty() {
                self.emit(Event::ToolArgumentsDelta {
                    key: index.to_string(),
                    delta: args.to_string(),
                })
                .await;
            }
        }
    }

    /// Close every accumulated call; a nameless one is dropped.
    async fn close_calls(&mut self) {
        for (index, call) in std::mem::take(&mut self.calls) {
            if !call.name.is_empty() {
                self.emit(Event::ToolCallEnd {
                    key: index.to_string(),
                })
                .await;
                self.emit(Event::ToolCall(call)).await;
            }
        }
    }
}

/// A chat-completions `finish_reason` as the seam's finish.
fn finish_reason(reason: &str) -> FinishReason {
    match reason {
        "stop" => FinishReason::Normal,
        "tool_calls" => FinishReason::ToolCalls,
        "length" => FinishReason::Length,
        "content_filter" => FinishReason::ContentFilter,
        other => FinishReason::Other(other.to_string()),
    }
}

/// A usage frame as disjoint counters: cached prompt tokens leave `input`.
fn usage(usage: &Value) -> Usage {
    let cached = usage["prompt_tokens_details"]["cached_tokens"]
        .as_u64()
        .unwrap_or(0);
    let total = usage["prompt_tokens"].as_u64().unwrap_or(0);
    Usage {
        input: total.saturating_sub(cached),
        output: usage["completion_tokens"].as_u64().unwrap_or(0),
        cache_read: cached,
        ..Usage::default()
    }
}
