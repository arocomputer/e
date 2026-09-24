//! The Responses-API dialect.
//!
//! One dialect, more than one deployment: the ChatGPT backend mounts it at
//! `{base}/codex/responses` behind a subscription OAuth (bearer + account-id
//! header, lazy refresh); other providers serve the same event grammar at
//! `{base}/responses` behind a plain key. The provider id — not this module —
//! names the account type. OAuth refresh lives in `auth::login`.

use std::collections::BTreeMap;

use serde_json::{json, Value};
use tokio::sync::mpsc;

use super::{blank_call, event_stream};
use crate::providers::registry::ResponsesMount;
use crate::providers::runtime::Authorization;
use crate::providers::{
    http, require_success, send_request, with_attribution, Event, FailureCause, FinishReason,
    ProviderError, Request, StreamEnd, ToolCall, Usage,
};

pub async fn run(
    request: &Request,
    authorization: &Authorization,
    tx: &mpsc::Sender<Event>,
) -> Result<StreamEnd, ProviderError> {
    let body = body(request);
    let response = send(request, authorization, body).await?;
    Reader::new(tx).read(response).await
}

/// History as Responses-API items: messages, function calls, and their
/// outputs.
fn history(request: &Request) -> Vec<Value> {
    let mut input: Vec<Value> = Vec::new();
    // Reasoning items wait for the assistant output they produced: the API
    // rejects a reasoning item "without its required following item", and
    // a reply that ran out of tokens while thinking left exactly that.
    let mut pending_reasoning: Vec<Value> = Vec::new();
    for m in &request.messages {
        match m.role() {
            "assistant" => {
                if !m.content.is_empty() || !m.tool_calls().is_empty() {
                    input.append(&mut pending_reasoning);
                }
                if !m.content.is_empty() {
                    input.push(json!({
                        "type": "message", "role": "assistant",
                        "content": [{"type": "output_text", "text": m.content}],
                    }));
                }
                for call in m.tool_calls() {
                    input.push(json!({
                        "type": "function_call",
                        "call_id": call.id,
                        "name": call.name,
                        "arguments": call.arguments,
                    }));
                }
            }
            "reasoning" => {
                // Only this dialect's own items; Anthropic thinking blocks
                // stored under the same role would 400 here.
                if let Ok(item) = serde_json::from_str::<Value>(&m.content) {
                    if item["type"].as_str() == Some("reasoning") {
                        pending_reasoning.push(item);
                    }
                }
            }
            "tool" => {
                pending_reasoning.clear();
                input.push(json!({
                    "type": "function_call_output",
                    "call_id": m.tool_call_id().cloned().unwrap_or_default(),
                    "output": m.content,
                }));
            }
            role => {
                pending_reasoning.clear();
                let mut content = Vec::new();
                if !m.content.is_empty() {
                    content.push(json!({"type": "input_text", "text": m.content}));
                }
                content.extend(
                    m.images()
                        .iter()
                        .map(|image| json!({"type": "input_image", "image_url": image.data_url()})),
                );
                input.push(json!({
                    "type": "message", "role": role,
                    "content": content,
                }));
            }
        }
    }
    input
}

/// The request body every mount shares; the Codex mount adds its cache key
/// in `send`.
fn body(request: &Request) -> Value {
    let mut body = json!({
        "model": request.model.id,
        "store": false,
        "stream": true,
        "instructions": request.system,
        "input": history(request),
        "text": {"verbosity": "low"},
        "include": ["reasoning.encrypted_content"],
        "tool_choice": "auto",
        "parallel_tool_calls": true,
    });
    if let Some(effort) = &request.effort {
        body["reasoning"] = json!({"effort": effort, "summary": "auto"});
    }
    if !request.tools.is_empty() {
        // The Responses dialect wants flat tools ({type, name, …}) — the
        // chat-completions nesting 400s with "Missing required parameter:
        // 'tools[0].name'". Caught by the first live codex turn.
        let tools: Vec<Value> = request
            .tools
            .iter()
            .map(|t| {
                json!({
                    "type": "function",
                    "name": t["function"]["name"],
                    "description": t["function"]["description"],
                    "parameters": t["function"]["parameters"],
                    "strict": false,
                })
            })
            .collect();
        body["tools"] = json!(tools);
    }
    body
}

/// Post the body to the model's mount and require a 2xx.
async fn send(
    request: &Request,
    authorization: &Authorization,
    mut body: Value,
) -> Result<reqwest::Response, ProviderError> {
    let builder = match request.model.responses_mount {
        // `prompt_cache_key` and the session headers are ChatGPT-backend
        // (codex) idioms: plain-key providers on `{base}/responses` neither
        // need the account-dependent body field nor accept an unknown
        // parameter from a strict upstream.
        ResponsesMount::Codex => {
            let account = authorization.account_id.as_deref().ok_or_else(|| {
                ProviderError::auth("Codex Responses authorization has no account id")
            })?;
            let session_id = codex_session_id(request);
            body["prompt_cache_key"] = json!(session_id);
            http()?
                .post(format!("{}/codex/responses", request.model.base_url))
                .header("chatgpt-account-id", account)
                .header("originator", "ulo")
                .header("OpenAI-Beta", "responses=experimental")
                .header("session-id", &session_id)
                .header("x-client-request-id", uuid::Uuid::new_v4().to_string())
        }
        ResponsesMount::Platform => http()?.post(format!("{}/responses", request.model.base_url)),
    };
    let builder = builder
        .bearer_auth(&authorization.bearer)
        .header("accept", "text/event-stream");
    require_success(send_request(with_attribution(builder, request).json(&body)).await?).await
}

/// The Codex cache key and session header. They pin the conversation to one
/// upstream prefix cache; a fresh value per request would miss it on every
/// step of a tool loop. A request without a session (a headless one-off) gets
/// one key for the process.
fn codex_session_id(request: &Request) -> String {
    if !request.session_id.is_empty() {
        return request.session_id.clone();
    }
    static PROCESS_SESSION: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    PROCESS_SESSION
        .get_or_init(|| uuid::Uuid::new_v4().to_string())
        .clone()
}

/// One response's stream: maps Responses events to [`Event`]s until the
/// terminal frame.
struct Reader<'a> {
    tx: &'a mpsc::Sender<Event>,
    /// function_call items accumulate argument deltas keyed by item id.
    calls: BTreeMap<String, ToolCall>,
    /// Argument bytes already sent per call, so a done item's arguments are
    /// streamed only when no delta carried them.
    streamed_arguments: BTreeMap<String, String>,
    refused: bool,
    /// Text sent per (output item, content part), so a completion snapshot
    /// adds only the suffix the deltas missed.
    text_parts: BTreeMap<(u64, u64), String>,
}

impl<'a> Reader<'a> {
    fn new(tx: &'a mpsc::Sender<Event>) -> Self {
        Self {
            tx,
            calls: BTreeMap::new(),
            streamed_arguments: BTreeMap::new(),
            refused: false,
            text_parts: BTreeMap::new(),
        }
    }

    /// Read frames until `[DONE]`, a completion, or a failure frame.
    async fn read(mut self, response: reqwest::Response) -> Result<StreamEnd, ProviderError> {
        let mut sse = event_stream(response);
        loop {
            let payload = sse.next().await?;
            if payload == "[DONE]" {
                return Ok(sse.end(self.finish()));
            }
            let Ok(value) = serde_json::from_str::<Value>(&payload) else {
                sse.malformed();
                continue;
            };
            match value["type"].as_str().unwrap_or("") {
                "response.output_text.delta" => self.text_delta(&value).await,
                "response.refusal.delta" => {
                    self.refused = true;
                    if let Some(text) = value["delta"].as_str() {
                        self.emit(Event::TextDelta(text.into())).await;
                    }
                }
                "response.refusal.done" => self.refused = true,
                "response.output_text.done" => {
                    if let Some(text) = value["text"].as_str() {
                        self.complete_text(text_key(&value), text).await;
                    }
                }
                "response.content_part.done" => {
                    if value["part"]["type"].as_str() == Some("output_text") {
                        if let Some(text) = value["part"]["text"].as_str() {
                            self.complete_text(text_key(&value), text).await;
                        }
                    }
                }
                "response.reasoning_text.delta" | "response.reasoning_summary_text.delta" => {
                    if let Some(text) = value["delta"].as_str() {
                        self.emit(Event::ReasoningDelta(text.into())).await;
                    }
                }
                "response.reasoning_summary_part.done" => {
                    self.emit(Event::ReasoningDelta("\n\n".into())).await;
                }
                "response.output_item.added" => self.item_added(&value["item"]).await,
                "response.function_call_arguments.delta" => self.arguments_delta(&value).await,
                "response.output_item.done" => self.item_done(&value).await,
                kind @ ("response.completed" | "response.done" | "response.incomplete") => {
                    let finish = self.completed(kind, &value).await;
                    return Ok(sse.end(finish));
                }
                "response.failed" => {
                    return Err(failure(&value).with_response(sse.response.clone()));
                }
                // The API's other failure frame: a top-level event carrying
                // `code` and `message`, after which the body just ends.
                // Left unread it would pass for a stall and be retried.
                "error" => {
                    return Err(
                        ProviderError::from_error_frame(&value).with_response(sse.response.clone())
                    );
                }
                _ => {}
            }
        }
    }

    async fn emit(&self, event: Event) {
        let _ = self.tx.send(event).await;
    }

    /// How a stream that ended without an incomplete-details frame finished.
    fn finish(&self) -> FinishReason {
        if self.refused {
            FinishReason::Refusal
        } else {
            FinishReason::Normal
        }
    }

    /// Stream answer text and remember it against its part.
    async fn text_delta(&mut self, value: &Value) {
        if let Some(text) = value["delta"].as_str() {
            self.text_parts
                .entry(text_key(value))
                .or_default()
                .push_str(text);
            self.emit(Event::TextDelta(text.into())).await;
        }
    }

    /// Recover a missing text suffix from a completed part without replaying
    /// streamed bytes.
    async fn complete_text(&mut self, key: (u64, u64), text: &str) {
        let sent = self.text_parts.entry(key).or_default();
        // A conflicting snapshot cannot be appended to an already-visible answer.
        if let Some(suffix) = text
            .strip_prefix(sent.as_str())
            .filter(|suffix| !suffix.is_empty())
        {
            let _ = self.tx.send(Event::TextDelta(suffix.into())).await;
            *sent = text.into();
        }
    }

    /// Gateways may deliver message text only in completion snapshots.
    async fn complete_message(&mut self, index: u64, item: &Value) {
        if item["type"].as_str() != Some("message") {
            return;
        }
        if let Some(content) = item["content"].as_array() {
            for (part, value) in content.iter().enumerate() {
                if value["type"].as_str() == Some("output_text") {
                    if let Some(text) = value["text"].as_str() {
                        self.complete_text((index, part as u64), text).await;
                    }
                }
            }
        }
    }

    /// Open a function call; its arguments may already be on the item.
    async fn item_added(&mut self, item: &Value) {
        if item["type"].as_str() != Some("function_call") {
            return;
        }
        let key = call_key(item);
        self.emit(Event::ToolCallStart { key: key.clone() }).await;
        let arguments = item["arguments"].as_str().unwrap_or("");
        if !arguments.is_empty() {
            self.streamed_arguments
                .insert(key.clone(), arguments.into());
            self.emit(Event::ToolArgumentsDelta {
                key: key.clone(),
                delta: arguments.into(),
            })
            .await;
        }
        self.calls.insert(
            key,
            ToolCall {
                id: item["call_id"].as_str().unwrap_or("").into(),
                name: item["name"].as_str().unwrap_or("").into(),
                arguments: arguments.into(),
                signature: None,
            },
        );
    }

    /// Append one argument fragment to its open call.
    async fn arguments_delta(&mut self, value: &Value) {
        let key = value["item_id"].as_str().unwrap_or("").to_string();
        let delta = value["delta"].as_str().unwrap_or("");
        if let Some(call) = self.calls.get_mut(&key) {
            call.arguments.push_str(delta);
        }
        if !delta.is_empty() {
            self.streamed_arguments
                .entry(key.clone())
                .or_default()
                .push_str(delta);
            self.emit(Event::ToolArgumentsDelta {
                key,
                delta: delta.to_string(),
            })
            .await;
        }
    }

    /// A finished output item: a message snapshot, a reasoning item to
    /// replay, or a function call to close.
    async fn item_done(&mut self, value: &Value) {
        let item = &value["item"];
        self.complete_message(value["output_index"].as_u64().unwrap_or(0), item)
            .await;
        if item["type"].as_str() == Some("reasoning") {
            // Must be replayed verbatim on the next request, ahead
            // of the calls it produced — the API 400s otherwise.
            self.emit(Event::ReasoningItem(item.to_string())).await;
        }
        if item["type"].as_str() == Some("function_call") {
            self.close_call(item).await;
        }
    }

    /// Close a function call from its done item, which carries the
    /// authoritative fields. A call that was never announced opens here; a
    /// nameless one is dropped.
    async fn close_call(&mut self, item: &Value) {
        let key = call_key(item);
        let was_open = self.calls.contains_key(&key);
        let mut call = self.calls.remove(&key).unwrap_or_else(blank_call);
        if let Some(id) = item["call_id"].as_str() {
            call.id = id.into();
        }
        if let Some(name) = item["name"].as_str() {
            call.name = name.into();
        }
        if let Some(args) = item["arguments"].as_str() {
            if !args.is_empty() {
                call.arguments = args.into();
            }
        }
        if call.name.is_empty() {
            return;
        }
        if !was_open {
            self.emit(Event::ToolCallStart { key: key.clone() }).await;
        }
        if call.arguments.is_empty() {
            call.arguments = "{}".into();
        }
        if self
            .streamed_arguments
            .remove(&key)
            .is_none_or(|arguments| arguments.is_empty())
            && call.arguments != "{}"
        {
            self.emit(Event::ToolArgumentsDelta {
                key: key.clone(),
                delta: call.arguments.clone(),
            })
            .await;
        }
        self.emit(Event::ToolCallEnd { key }).await;
        self.emit(Event::ToolCall(call)).await;
    }

    /// The terminal response frame: recover snapshot-only text, report
    /// usage, and say how the reply finished.
    async fn completed(&mut self, kind: &str, value: &Value) -> FinishReason {
        if let Some(output) = value["response"]["output"].as_array() {
            for (index, item) in output.iter().enumerate() {
                self.complete_message(index as u64, item).await;
            }
        }
        let usage = &value["response"]["usage"];
        if usage.is_object() {
            let cached = usage["input_tokens_details"]["cached_tokens"]
                .as_u64()
                .unwrap_or(0);
            let total = usage["input_tokens"].as_u64().unwrap_or(0);
            self.emit(Event::Usage(Usage {
                input: total.saturating_sub(cached),
                output: usage["output_tokens"].as_u64().unwrap_or(0),
                cache_read: cached,
                ..Usage::default()
            }))
            .await;
        }
        // `response.incomplete` is a truncated reply the API still
        // delivers with a 200 — name why instead of passing it off
        // as a finished turn.
        if kind != "response.incomplete" {
            return self.finish();
        }
        match value["response"]["incomplete_details"]["reason"]
            .as_str()
            .unwrap_or("")
        {
            "max_output_tokens" | "max_tokens" => FinishReason::Length,
            "content_filter" => FinishReason::ContentFilter,
            other => FinishReason::Other(format!("incomplete: {other}")),
        }
    }
}

/// Text identity spans both the output item and its content parts.
fn text_key(value: &Value) -> (u64, u64) {
    (
        value["output_index"].as_u64().unwrap_or(0),
        value["content_index"].as_u64().unwrap_or(0),
    )
}

/// A function call's stream key: its item id, else its call id.
fn call_key(item: &Value) -> String {
    item["id"]
        .as_str()
        .or(item["call_id"].as_str())
        .unwrap_or("")
        .to_string()
}

/// The error a `response.failed` frame reports. A quota message wins over
/// the frame's code.
fn failure(value: &Value) -> ProviderError {
    let error = &value["response"]["error"];
    let message = error["message"]
        .as_str()
        .unwrap_or("response failed")
        .to_string();
    let text_cause = crate::providers::classify_text(&message);
    let cause = if text_cause == Some(FailureCause::QuotaExhausted) {
        FailureCause::QuotaExhausted
    } else {
        match error["code"].as_str().unwrap_or("") {
            "rate_limit_exceeded" => FailureCause::RateLimited,
            "server_error" | "internal_error" => FailureCause::ProviderUnavailable,
            // Unknown codes still get the message classifier.
            _ => text_cause.unwrap_or(FailureCause::Rejected),
        }
    };
    ProviderError::frame(message, cause).with_code(error["code"].as_str())
}
