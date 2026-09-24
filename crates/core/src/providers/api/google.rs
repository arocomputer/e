//! The Gemini dialect: `POST {base}/models/{model}:streamGenerateContent`
//! with `alt=sse`, `x-goog-api-key` auth. Candidate chunks stream text,
//! thought summaries, and function calls; thought signatures on function
//! calls are captured and replayed verbatim — the API requires them back on
//! the next request of a tool loop. A signature verifies against the thought
//! text that preceded it, so that text is committed as a "reasoning" history
//! item too and replayed ahead of the function call it signs (mirroring how
//! the Anthropic dialect replays signed thinking blocks). Effort maps to
//! `thinkingConfig.thinkingLevel`. The stream has no `[DONE]` sentinel: the
//! chunk carrying `finishReason` is terminal.

use std::collections::HashMap;

use serde_json::{json, Value};
use tokio::sync::mpsc;

use super::event_stream;
use crate::providers::runtime::Authorization;
use crate::providers::{
    http, require_success, send_request, with_attribution, Event, FinishReason, ProviderError,
    Request, StreamEnd, ToolCall, Usage,
};

/// Older Gemini responses carried no wire id. A UUID fallback stays unique
/// across resumed processes and later model switches; Gemini 3's own id wins
/// whenever present and is returned verbatim with the function result.
fn synthesize_call_id(name: &str) -> String {
    format!("{name}-{}", uuid::Uuid::new_v4())
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

/// History → contents. Assistant turns replay their function calls with
/// thought signatures; tool results ride user turns as functionResponse
/// parts, consecutive results joining one turn to mirror the batch.
/// functionResponse.name comes from the assistant call the id refers to —
/// ids may be another dialect's (a mid-session model switch), so the name is
/// never derived from the id's spelling. Signed thought text committed as
/// "reasoning" messages replays verbatim at the head of the assistant turn it
/// preceded — the API validates a function call's signature against that
/// thought content.
fn history(request: &Request) -> Vec<Value> {
    let mut contents: Vec<Value> = Vec::new();
    let mut call_names: HashMap<String, String> = HashMap::new();
    let mut pending_thoughts: Vec<Value> = Vec::new();
    for m in &request.messages {
        match m.role() {
            "assistant" => {
                let mut parts = std::mem::take(&mut pending_thoughts);
                if !m.content.is_empty() {
                    parts.push(json!({"text": m.content}));
                }
                for call in m.tool_calls() {
                    call_names.insert(call.id.clone(), call.name.clone());
                    let args: Value = serde_json::from_str(&call.arguments).unwrap_or(json!({}));
                    let mut part = json!({"functionCall": {
                        "id": call.id, "name": call.name, "args": args
                    }});
                    if let Some(signature) = &call.signature {
                        part["thoughtSignature"] = json!(signature);
                    }
                    parts.push(part);
                }
                if !parts.is_empty() {
                    contents.push(json!({"role": "model", "parts": parts}));
                }
            }
            "tool" => {
                let name = m
                    .tool_call_id()
                    .and_then(|id| call_names.get(id))
                    .cloned()
                    .unwrap_or_default();
                let part = json!({"functionResponse": {
                    "id": m.tool_call_id().cloned().unwrap_or_default(),
                    "name": name,
                    "response": {"output": m.content},
                }});
                push_tool_result(&mut contents, part);
            }
            "reasoning" => {
                // Only this dialect's own items; items from other dialects
                // (Anthropic thinking blocks, Responses reasoning JSON) mean
                // nothing here.
                if let Ok(item) = serde_json::from_str::<Value>(&m.content) {
                    if item["type"].as_str() == Some("gemini_thought") {
                        if let Some(text) = item["text"].as_str() {
                            pending_thoughts.push(json!({"text": text, "thought": true}));
                        }
                    }
                }
            }
            _ => {
                // A turn boundary without an assistant message orphans any
                // buffered thought text; replaying it elsewhere would fail
                // the signature check.
                pending_thoughts.clear();
                let mut parts = Vec::new();
                if !m.content.is_empty() {
                    parts.push(json!({"text": m.content}));
                }
                parts.extend(m.images().iter().map(|image| {
                    json!({"inlineData": {
                        "mimeType": image.media_type,
                        "data": image.data,
                    }})
                }));
                contents.push(json!({"role": "user", "parts": parts}));
            }
        }
    }
    contents
}

/// Join a functionResponse part to the user turn holding its batch's other
/// results, or open that turn.
fn push_tool_result(contents: &mut Vec<Value>, part: Value) {
    match contents.last_mut() {
        Some(last)
            if last["role"] == "user" && last["parts"][0]["functionResponse"].is_object() =>
        {
            // The guard proves `parts` is a non-empty array; the
            // else arm is the safe fallback, not a panic.
            match last["parts"].as_array_mut() {
                Some(parts) => parts.push(part),
                None => contents.push(json!({"role": "user", "parts": [part]})),
            }
        }
        _ => contents.push(json!({"role": "user", "parts": [part]})),
    }
}

/// The request body: system instruction, history, tools, and thinking.
fn body(request: &Request) -> Value {
    let mut body = json!({
        "systemInstruction": {"parts": [{"text": request.system}]},
        "contents": history(request),
    });
    if !request.tools.is_empty() {
        // OpenAI-shaped schemas → Gemini function declarations.
        let declarations: Vec<Value> = request
            .tools
            .iter()
            .map(|t| {
                json!({
                    "name": t["function"]["name"],
                    "description": t["function"]["description"],
                    "parameters": t["function"]["parameters"],
                })
            })
            .collect();
        body["tools"] = json!([{"functionDeclarations": declarations}]);
    }
    if let Some(effort) = &request.effort {
        body["generationConfig"] = json!({
            "thinkingConfig": {"thinkingLevel": effort, "includeThoughts": true},
        });
    }
    body
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
                .post(format!(
                    "{}/models/{}:streamGenerateContent?alt=sse",
                    request.model.base_url, request.model.id
                ))
                .header("x-goog-api-key", &authorization.bearer)
                .header("accept", "text/event-stream")
                .json(body),
            request,
        ))
        .await?,
    )
    .await
}

/// One response's stream: maps candidate chunks to [`Event`]s until the
/// chunk carrying `finishReason`.
struct Reader<'a> {
    tx: &'a mpsc::Sender<Event>,
    /// Cumulative — the latest frame wins.
    usage: Option<Usage>,
}

impl<'a> Reader<'a> {
    fn new(tx: &'a mpsc::Sender<Event>) -> Self {
        Self { tx, usage: None }
    }

    /// Read chunks until one carries `finishReason`, or the stream fails.
    async fn read(mut self, response: reqwest::Response) -> Result<StreamEnd, ProviderError> {
        let mut sse = event_stream(response);
        loop {
            let payload = sse.next().await?;
            let Ok(value) = serde_json::from_str::<Value>(&payload) else {
                sse.malformed();
                continue;
            };
            // Gemini streams a failure after the headers as a `google.rpc.Status`
            // frame `{"error":{"code":500,"status":"INTERNAL",…}}`; the body
            // then ends without a finishReason, which would read as a stall.
            if let Some(error) = value.get("error").filter(|ulo| ulo.is_object()) {
                return Err(
                    ProviderError::from_error_frame(error).with_response(sse.response.clone())
                );
            }
            if let Some(meta) = value.get("usageMetadata").filter(|u| u.is_object()) {
                self.usage = Some(usage(meta));
            }
            if let Some(reason) = value["promptFeedback"]["blockReason"].as_str() {
                return Err(ProviderError::rejected(format!("prompt blocked: {reason}"))
                    .with_response(sse.response.clone()));
            }
            let candidate = &value["candidates"][0];
            if let Some(parts) = candidate["content"]["parts"].as_array() {
                for part in parts {
                    self.part(part).await;
                }
            }
            if let Some(reason) = candidate["finishReason"].as_str() {
                if let Some(usage) = self.usage {
                    self.emit(Event::Usage(usage)).await;
                }
                return Ok(sse.end(finish_reason(reason)));
            }
        }
    }

    async fn emit(&self, event: Event) {
        let _ = self.tx.send(event).await;
    }

    /// One candidate part: thought text (streamed, and kept as a replayable
    /// item), answer text, or a complete function call.
    async fn part(&self, part: &Value) {
        if let Some(text) = part["text"].as_str() {
            if part["thought"].as_bool().unwrap_or(false) {
                self.emit(Event::ReasoningDelta(text.into())).await;
                if !text.is_empty() {
                    let item = json!({"type": "gemini_thought", "text": text});
                    self.emit(Event::ReasoningItem(item.to_string())).await;
                }
            } else if !text.is_empty() {
                self.emit(Event::TextDelta(text.into())).await;
            }
        }
        if part["functionCall"].is_object() {
            let call = function_call(part);
            // A nameless call is dropped; Gemini delivers each call whole, so
            // its whole lifecycle is emitted at once.
            if !call.name.is_empty() {
                let key = call.id.clone();
                self.emit(Event::ToolCallStart { key: key.clone() }).await;
                if !call.arguments.is_empty() {
                    self.emit(Event::ToolArgumentsDelta {
                        key: key.clone(),
                        delta: call.arguments.clone(),
                    })
                    .await;
                }
                self.emit(Event::ToolCallEnd { key }).await;
                self.emit(Event::ToolCall(call)).await;
            }
        }
    }
}

/// A functionCall part as a call, its thought signature kept for replay.
fn function_call(part: &Value) -> ToolCall {
    let wire = &part["functionCall"];
    let name = wire["name"].as_str().unwrap_or("").to_string();
    ToolCall {
        id: wire["id"]
            .as_str()
            .filter(|id| !id.is_empty())
            .map(String::from)
            .unwrap_or_else(|| synthesize_call_id(&name)),
        name,
        // `args` is optional on the wire; a call without it
        // is a call with no arguments, not the string "null".
        arguments: match &wire["args"] {
            args if args.is_object() => args.to_string(),
            _ => "{}".into(),
        },
        signature: part["thoughtSignature"].as_str().map(String::from),
    }
}

/// A `usageMetadata` frame as disjoint counters. Thought tokens are output.
fn usage(meta: &Value) -> Usage {
    let total = meta["promptTokenCount"].as_u64().unwrap_or(0);
    let cached = meta["cachedContentTokenCount"].as_u64().unwrap_or(0);
    Usage {
        input: total.saturating_sub(cached),
        output: meta["candidatesTokenCount"]
            .as_u64()
            .unwrap_or(0)
            .saturating_add(meta["thoughtsTokenCount"].as_u64().unwrap_or(0)),
        cache_read: cached,
        ..Usage::default()
    }
}

/// Gemini's `finishReason` as the seam's finish.
fn finish_reason(reason: &str) -> FinishReason {
    match reason {
        "STOP" => FinishReason::Normal,
        "MAX_TOKENS" => FinishReason::Length,
        "SAFETY" | "PROHIBITED_CONTENT" | "BLOCKLIST" | "SPII" | "IMAGE_SAFETY" => {
            FinishReason::ContentFilter
        }
        other => FinishReason::Other(other.to_string()),
    }
}
