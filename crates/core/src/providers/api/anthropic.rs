//! The Anthropic Messages dialect: `{base}/v1/messages`, `x-api-key` auth.
//!
//! Same event grammar as the reference client: SSE data payloads carry a
//! `type` — content_block_start/delta/stop stream text, thinking, and
//! tool_use input JSON; message_start/message_delta carry usage. Effort maps
//! to an extended-thinking token budget (manual) or `output_config.effort`
//! (adaptive), per the model's declared thinking mode.

use std::collections::BTreeMap;

use serde_json::{json, Value};
use tokio::sync::mpsc;

use super::event_stream;
use crate::providers::catalog::Thinking;
use crate::providers::runtime::Authorization;
use crate::providers::{
    http, require_success, send_request, with_attribution, Event, FailureCause, FinishReason,
    ProviderError, Request, StreamEnd, ToolCall, Usage,
};

const ANTHROPIC_VERSION: &str = "2023-06-01";
/// Default output ceiling per reply, for models that don't declare their own
/// (lower) `max_output` — a per-model catalog fact, since it varies by
/// model (e.g. claude-haiku-4-5's ~8k against this 32k default).
const MAX_TOKENS: u64 = 32_000;

/// Extended-thinking budgets per effort; each stays under the default
/// MAX_TOKENS, but a smaller declared `max_output` clamps this further
/// below (see the `max_tokens - 1024` clamp in `thinking`).
fn thinking_budget(effort: &str) -> u64 {
    match effort {
        "low" => 4_000,
        "high" => 24_000,
        _ => 12_000,
    }
}

pub async fn run(
    request: &Request,
    authorization: &Authorization,
    tx: &mpsc::Sender<Event>,
) -> Result<StreamEnd, ProviderError> {
    let body = body(request);
    let response = send(request, authorization, &body).await?;
    Reader::new(tx).read(response).await
}

/// History → content blocks. Tool results ride user turns, and the results
/// of one step's parallel calls share a single user turn: split across
/// messages, the API still accepts them but the model learns to stop calling
/// tools in parallel. Signed thinking blocks committed as "reasoning"
/// messages replay verbatim at the head of the assistant turn they preceded —
/// the API requires them back, complete with signatures, when continuing a
/// tool loop.
fn history(request: &Request) -> Vec<Value> {
    let mut messages: Vec<Value> = Vec::new();
    let mut pending_thinking: Vec<Value> = Vec::new();
    for m in &request.messages {
        match m.role() {
            "assistant" => {
                let mut content = std::mem::take(&mut pending_thinking);
                if !m.content.is_empty() {
                    content.push(json!({"type": "text", "text": m.content}));
                }
                for call in m.tool_calls() {
                    let input: Value = serde_json::from_str(&call.arguments).unwrap_or(json!({}));
                    content.push(json!({
                        "type": "tool_use", "id": call.id, "name": call.name, "input": input,
                    }));
                }
                if !content.is_empty() {
                    messages.push(json!({"role": "assistant", "content": content}));
                }
            }
            "tool" => push_tool_result(
                &mut messages,
                json!({
                    "type": "tool_result",
                    "tool_use_id": m.tool_call_id().cloned().unwrap_or_default(),
                    "content": m.content,
                }),
            ),
            "reasoning" => {
                // Only this dialect's own blocks; items from other dialects
                // (Responses reasoning JSON) mean nothing here.
                if let Ok(block) = serde_json::from_str::<Value>(&m.content) {
                    if is_thinking(&block) {
                        pending_thinking.push(block);
                    }
                }
            }
            _ => {
                // A turn boundary without an assistant message orphans any
                // buffered thinking; replaying it elsewhere would fail the
                // signature check.
                pending_thinking.clear();
                let mut content = Vec::new();
                if !m.content.is_empty() {
                    content.push(json!({"type": "text", "text": m.content}));
                }
                content.extend(m.images().iter().map(|image| {
                    json!({
                        "type": "image",
                        "source": {
                            "type": "base64",
                            "media_type": image.media_type,
                            "data": image.data,
                        }
                    })
                }));
                messages.push(json!({"role": "user", "content": content}));
            }
        }
    }
    messages
}

/// Join a tool result to the user turn holding its batch's other results, or
/// open that turn.
fn push_tool_result(messages: &mut Vec<Value>, block: Value) {
    match messages.last_mut() {
        Some(last) if last["role"] == "user" && last["content"][0]["type"] == "tool_result" => {
            // The guard proves `content` is a non-empty array;
            // the else arm is the safe fallback, not a panic.
            match last["content"].as_array_mut() {
                Some(blocks) => blocks.push(block),
                None => messages.push(json!({"role": "user", "content": [block]})),
            }
        }
        _ => messages.push(json!({"role": "user", "content": [block]})),
    }
}

/// A signed or redacted thinking block, as this dialect stores reasoning.
fn is_thinking(block: &Value) -> bool {
    matches!(
        block["type"].as_str(),
        Some("thinking") | Some("redacted_thinking")
    )
}

/// Moving cache breakpoint on the last cacheable content block: the system
/// block alone caches only the prefix ahead of the conversation, so every
/// step of a tool loop re-billed the whole history uncached. With the tail
/// marked, each request extends the previous step's cached prefix instead.
/// Thinking blocks can't carry cache_control; skip them.
fn mark_cache_tail(messages: &mut [Value]) {
    if let Some(last) = messages.last_mut() {
        if let Some(blocks) = last["content"].as_array_mut() {
            if let Some(block) = blocks.iter_mut().rev().find(|b| !is_thinking(b)) {
                block["cache_control"] = json!({"type": "ephemeral"});
            }
        }
    }
}

/// The request body: history, the output ceiling, tools, and thinking.
fn body(request: &Request) -> Value {
    // The output ceiling must fit both the model's own max output (some
    // models allow far less than the 32k default — claude-haiku-4-5 caps
    // at ~8k) and its window: a fixed default against a small declared
    // window would be rejected before generation either way.
    let ceiling = request.model.max_output.unwrap_or(MAX_TOKENS);
    let max_tokens = ceiling.min((request.model.context_window / 2).max(1024));
    let mut messages = history(request);
    mark_cache_tail(&mut messages);
    let mut body = json!({
        "model": request.model.id,
        "max_tokens": max_tokens,
        "stream": true,
        "system": [{"type": "text", "text": request.system,
                    "cache_control": {"type": "ephemeral"}}],
        "messages": messages,
    });
    if !request.tools.is_empty() {
        // OpenAI-shaped schemas → Anthropic tool declarations.
        let tools: Vec<Value> = request
            .tools
            .iter()
            .map(|t| {
                json!({
                    "name": t["function"]["name"],
                    "description": t["function"]["description"],
                    "input_schema": t["function"]["parameters"],
                })
            })
            .collect();
        body["tools"] = json!(tools);
    }
    if let Some(effort) = &request.effort {
        thinking(&mut body, request, effort, max_tokens);
    }
    body
}

/// Map effort onto the model's declared thinking mode.
fn thinking(body: &mut Value, request: &Request, effort: &str, max_tokens: u64) {
    // Adaptive-thinking models (Claude 4.7+) reject the legacy manual
    // shape with a 400 before generation; they take the effort through
    // output_config instead. Manual models keep the token budget.
    match request.model.thinking {
        Thinking::Adaptive => {
            body["thinking"] = json!({"type": "adaptive"});
            body["output_config"] = json!({"effort": effort});
        }
        Thinking::Manual => {
            // The budget must stay strictly under max_tokens with real
            // headroom or the request is rejected. On a small declared
            // window, max_tokens itself can be too small to leave that
            // headroom above a sane minimum budget — there enabling
            // thinking at all would only produce an invalid request, so
            // skip it and let the reply generate without it.
            if max_tokens >= 2048 {
                body["thinking"] = json!({
                    "type": "enabled",
                    "budget_tokens": thinking_budget(effort).min(max_tokens - 1024),
                });
            }
        }
    }
}

/// Post the body and require a 2xx.
async fn send(
    request: &Request,
    authorization: &Authorization,
    body: &Value,
) -> Result<reqwest::Response, ProviderError> {
    require_success(
        send_request(with_attribution(
            http()?
                .post(format!("{}/v1/messages", request.model.base_url))
                .header("x-api-key", &authorization.bearer)
                .header("anthropic-version", ANTHROPIC_VERSION)
                .header("accept", "text/event-stream")
                .json(body),
            request,
        ))
        .await?,
    )
    .await
}

/// One response's stream: maps Messages events to [`Event`]s until
/// `message_stop`.
struct Reader<'a> {
    tx: &'a mpsc::Sender<Event>,
    /// Tool input JSON streams in fragments per content block index.
    open_tools: BTreeMap<usize, ToolCall>,
    /// A thinking block accumulates text and its opaque signature; on stop it
    /// becomes a replayable reasoning item.
    open_thinking: Option<(String, String)>,
    /// Prompt usage from `message_start`; output tokens join from
    /// `message_delta`.
    usage: Usage,
    finish: FinishReason,
}

impl<'a> Reader<'a> {
    fn new(tx: &'a mpsc::Sender<Event>) -> Self {
        Self {
            tx,
            open_tools: BTreeMap::new(),
            open_thinking: None,
            usage: Usage::default(),
            finish: FinishReason::Normal,
        }
    }

    /// Read frames until `message_stop` or an error frame.
    async fn read(mut self, response: reqwest::Response) -> Result<StreamEnd, ProviderError> {
        let mut sse = event_stream(response);
        loop {
            let payload = sse.next().await?;
            let Ok(value) = serde_json::from_str::<Value>(&payload) else {
                sse.malformed();
                continue;
            };
            match value["type"].as_str().unwrap_or("") {
                "message_start" => self.message_start(&value["message"]["usage"]),
                "content_block_start" => self.block_start(&value).await,
                "content_block_delta" => self.block_delta(&value).await,
                "content_block_stop" => self.block_stop(&value).await,
                "message_delta" => self.message_delta(&value),
                "message_stop" => {
                    self.emit(Event::Usage(self.usage)).await;
                    return Ok(sse.end(self.finish));
                }
                "error" => return Err(failure(&value).with_response(sse.response.clone())),
                _ => {}
            }
        }
    }

    async fn emit(&self, event: Event) {
        let _ = self.tx.send(event).await;
    }

    /// Anthropic reports disjoint prompt categories. Older responses expose
    /// only the creation total; ulo requests ordinary ephemeral caching, so any
    /// unclassified write belongs to the five-minute bucket.
    fn message_start(&mut self, usage: &Value) {
        self.usage.input = usage["input_tokens"].as_u64().unwrap_or(0);
        self.usage.cache_read = usage["cache_read_input_tokens"].as_u64().unwrap_or(0);
        let creation = usage["cache_creation_input_tokens"].as_u64().unwrap_or(0);
        let write_5m = usage["cache_creation"]["ephemeral_5m_input_tokens"]
            .as_u64()
            .unwrap_or(0);
        let write_1h = usage["cache_creation"]["ephemeral_1h_input_tokens"]
            .as_u64()
            .unwrap_or(0);
        let classified = write_5m.saturating_add(write_1h);
        self.usage.cache_write_5m = write_5m.saturating_add(creation.saturating_sub(classified));
        self.usage.cache_write_1h = write_1h;
    }

    /// Open a tool call or thinking block; a redacted block is complete here.
    async fn block_start(&mut self, value: &Value) {
        let index = block_index(value);
        let block = &value["content_block"];
        match block["type"].as_str().unwrap_or("") {
            "tool_use" => {
                // Anthropic names a tool_use block up front, so we can refuse
                // a nameless one here rather than let it dangle: a call we
                // never open needs no ToolCallEnd, keeping the start/end
                // lifecycle the consumer relies on balanced. The other
                // dialects can't do this — their name streams in with the
                // arguments deltas — so they gate at the close instead.
                let name = block["name"].as_str().unwrap_or("").to_string();
                if name.is_empty() {
                    // Skip the block; the stream keeps going.
                    return;
                }
                self.open_tools.insert(
                    index,
                    ToolCall {
                        id: block["id"].as_str().unwrap_or("").to_string(),
                        name,
                        arguments: String::new(),
                        signature: None,
                    },
                );
                self.emit(Event::ToolCallStart {
                    key: index.to_string(),
                })
                .await;
            }
            "thinking" => self.open_thinking = Some((String::new(), String::new())),
            // Arrives complete, no deltas; preserved verbatim.
            "redacted_thinking" => self.emit(Event::ReasoningItem(block.to_string())).await,
            _ => {}
        }
    }

    /// Stream text, thinking, a thinking signature, or tool input JSON.
    async fn block_delta(&mut self, value: &Value) {
        let delta = &value["delta"];
        match delta["type"].as_str().unwrap_or("") {
            "text_delta" => {
                if let Some(text) = delta["text"].as_str() {
                    self.emit(Event::TextDelta(text.to_string())).await;
                }
            }
            "thinking_delta" => {
                if let Some(text) = delta["thinking"].as_str() {
                    if let Some((thinking, _)) = &mut self.open_thinking {
                        thinking.push_str(text);
                    }
                    self.emit(Event::ReasoningDelta(text.to_string())).await;
                }
            }
            "signature_delta" => {
                if let Some((_, signature)) = &mut self.open_thinking {
                    signature.push_str(delta["signature"].as_str().unwrap_or(""));
                }
            }
            "input_json_delta" => {
                let index = block_index(value);
                if let Some(call) = self.open_tools.get_mut(&index) {
                    let partial = delta["partial_json"].as_str().unwrap_or("");
                    call.arguments.push_str(partial);
                    if !partial.is_empty() {
                        self.emit(Event::ToolArgumentsDelta {
                            key: index.to_string(),
                            delta: partial.to_string(),
                        })
                        .await;
                    }
                }
            }
            _ => {}
        }
    }

    /// Close the block at this index: a tool call is complete, and a signed
    /// thinking block becomes a replayable reasoning item.
    async fn block_stop(&mut self, value: &Value) {
        let index = block_index(value);
        if let Some(mut call) = self.open_tools.remove(&index) {
            // Only named blocks reach here: nameless tool_use
            // blocks are refused at content_block_start, so there
            // is nothing to gate on.
            if call.arguments.is_empty() {
                call.arguments = "{}".into();
            }
            self.emit(Event::ToolCallEnd {
                key: index.to_string(),
            })
            .await;
            self.emit(Event::ToolCall(call)).await;
        }
        if let Some((thinking, signature)) = self.open_thinking.take() {
            // Only a signed block is replayable; an unsigned one
            // has nothing the API demands back.
            if !signature.is_empty() {
                let block = json!({
                    "type": "thinking",
                    "thinking": thinking,
                    "signature": signature,
                });
                self.emit(Event::ReasoningItem(block.to_string())).await;
            }
        }
    }

    /// Output tokens so far and, once known, why the message stopped.
    fn message_delta(&mut self, value: &Value) {
        if let Some(out) = value["usage"]["output_tokens"].as_u64() {
            self.usage.output = out;
        }
        if let Some(reason) = value["delta"]["stop_reason"].as_str() {
            self.finish = match reason {
                "end_turn" | "stop_sequence" => FinishReason::Normal,
                "tool_use" => FinishReason::ToolCalls,
                "max_tokens" => FinishReason::Length,
                "refusal" => FinishReason::Refusal,
                other => FinishReason::Other(other.to_string()),
            };
        }
    }
}

/// The content block a frame refers to.
fn block_index(value: &Value) -> usize {
    value["index"].as_u64().unwrap_or(0) as usize
}

/// The error a mid-stream `error` frame reports. The frame carries its own
/// type — the API's way of saying "overloaded" or "rate limited" once a
/// connection is already open, distinct from an HTTP status. A quota message
/// wins over the type.
fn failure(value: &Value) -> ProviderError {
    let message = value["error"]["message"]
        .as_str()
        .unwrap_or("unknown provider error")
        .to_string();
    let text_cause = crate::providers::classify_text(&message);
    let cause = if text_cause == Some(FailureCause::QuotaExhausted) {
        FailureCause::QuotaExhausted
    } else {
        match value["error"]["type"].as_str().unwrap_or("") {
            "overloaded_error" | "api_error" => FailureCause::ProviderUnavailable,
            "rate_limit_error" => FailureCause::RateLimited,
            "authentication_error" | "permission_error" => FailureCause::Auth,
            // Unknown types still get the message classifier.
            _ => text_cause.unwrap_or(FailureCause::Rejected),
        }
    };
    ProviderError::frame(message, cause).with_code(value["error"]["type"].as_str())
}
