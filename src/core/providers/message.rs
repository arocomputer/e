//! Persisted messages, images, and response usage shared by every provider.

use super::catalog::{self, Model};
use serde::Serialize;

pub(crate) const MAX_IMAGE_BYTES: u64 = 20 * 1024 * 1024;
const MAX_IMAGE_COUNT: usize = 10;
const MAX_TOTAL_IMAGE_BYTES: u64 = 40 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, serde::Deserialize, Serialize)]
pub struct ImageInput {
    pub media_type: String,
    /// Base64 without a data-URL prefix. Sessions retain the bytes so resume
    /// does not depend on the original file still existing or staying still.
    pub data: std::sync::Arc<str>,
}

impl ImageInput {
    pub fn from_path(path: &std::path::Path) -> Result<Self, String> {
        Self::from_path_with_size(path).map(|(image, _)| image)
    }

    /// Load image bytes supplied by the clipboard rather than a file path.
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self, String> {
        Self::from_bytes_named(bytes, "clipboard").map(|(image, _)| image)
    }

    /// Check count and aggregate size when attachments arrive over several
    /// clipboard pastes instead of one `from_paths` call.
    pub fn validate_batch(images: &[Self]) -> Result<(), String> {
        if images.len() > MAX_IMAGE_COUNT {
            return Err(format!(
                "at most {MAX_IMAGE_COUNT} image attachments are allowed"
            ));
        }
        let total = images.iter().try_fold(0u64, |total, image| {
            let padding = image
                .data
                .bytes()
                .rev()
                .take_while(|byte| *byte == b'=')
                .count() as u64;
            let bytes = (image.data.len() as u64 / 4)
                .checked_mul(3)
                .and_then(|size| size.checked_sub(padding))
                .ok_or_else(|| "image attachment sizes overflowed".to_string())?;
            total
                .checked_add(bytes)
                .ok_or_else(|| "image attachment sizes overflowed".to_string())
        })?;
        if total > MAX_TOTAL_IMAGE_BYTES {
            return Err("image attachments exceed 40 MiB in total".into());
        }
        Ok(())
    }

    /// Load a bounded first-turn attachment batch. Keeping count, aggregate,
    /// file-type, and race-safe byte checks here gives the TUI, ask, and RPC
    /// paths one definition of a valid image batch.
    pub fn from_paths(paths: &[String]) -> Result<Vec<Self>, String> {
        if paths.len() > MAX_IMAGE_COUNT {
            return Err(format!(
                "at most {MAX_IMAGE_COUNT} --image attachments are allowed"
            ));
        }
        let declared_total = paths.iter().try_fold(0u64, |total, path| {
            let metadata = std::fs::metadata(path).map_err(|error| format!("{path}: {error}"))?;
            total
                .checked_add(metadata.len())
                .ok_or_else(|| "image attachment sizes overflowed".to_string())
        })?;
        if declared_total > MAX_TOTAL_IMAGE_BYTES {
            return Err("image attachments exceed 40 MiB in total".into());
        }

        let mut actual_total = 0u64;
        let mut images = Vec::with_capacity(paths.len());
        for path in paths {
            let (image, size) = Self::from_path_with_size(std::path::Path::new(path))?;
            actual_total = actual_total
                .checked_add(size)
                .ok_or_else(|| "image attachment sizes overflowed".to_string())?;
            if actual_total > MAX_TOTAL_IMAGE_BYTES {
                return Err("image attachments exceed 40 MiB in total".into());
            }
            images.push(image);
        }
        Ok(images)
    }

    pub(super) fn from_path_with_size(path: &std::path::Path) -> Result<(Self, u64), String> {
        let metadata =
            std::fs::metadata(path).map_err(|error| format!("{}: {error}", path.display()))?;
        if !metadata.is_file() {
            return Err(format!("{}: not a regular file", path.display()));
        }
        if metadata.len() > MAX_IMAGE_BYTES {
            return Err(format!("{}: image exceeds 20 MiB", path.display()));
        }
        // The file can change size between metadata() and here — grow after a
        // small reported size, or never end at all (a fifo, a procfs entry).
        // Read through a bounded reader so such a file cannot be pulled into
        // memory in full before the length check below ever runs.
        use std::io::Read as _;
        let file =
            std::fs::File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
        let mut bytes = Vec::new();
        file.take(MAX_IMAGE_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        Self::from_bytes_named(bytes, &path.display().to_string())
    }

    /// Validate, identify, and encode one already-read image.
    fn from_bytes_named(bytes: Vec<u8>, source: &str) -> Result<(Self, u64), String> {
        use base64::Engine as _;
        if bytes.len() as u64 > MAX_IMAGE_BYTES {
            return Err(format!("{source}: image exceeds 20 MiB"));
        }
        let media_type = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            "image/png"
        } else if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
            "image/jpeg"
        } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
            "image/gif"
        } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
            "image/webp"
        } else {
            return Err(format!(
                "{source}: unsupported image data (use png, jpg, gif, or webp)"
            ));
        };
        let size = bytes.len() as u64;
        Ok((
            ImageInput {
                media_type: media_type.into(),
                data: base64::engine::general_purpose::STANDARD
                    .encode(bytes)
                    .into(),
            },
            size,
        ))
    }

    pub fn data_url(&self) -> String {
        format!("data:{};base64,{}", self.media_type, self.data)
    }
}

/// One requested tool invocation, as the model asked for it.
#[derive(Clone, Debug, PartialEq, serde::Deserialize, Serialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// Raw JSON argument string, exactly as streamed.
    pub arguments: String,
    /// An opaque provider signature attached to the call (Gemini thought
    /// signatures); must be replayed verbatim on the next request of a tool
    /// loop when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

/// Presentation metadata persisted beside a tool result, ignored by provider
/// dialects and used to reconstruct the transcript on resume.
#[derive(Clone, Debug, PartialEq, Serialize, serde::Deserialize)]
pub struct ToolResultMeta {
    pub outcome: crate::core::tools::ToolOutcome,
    pub summary: String,
}

/// Disjoint token counters for one provider request. `input` excludes cache
/// reads and writes; `prompt_tokens` reconstructs the complete context size.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, serde::Deserialize)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    #[serde(default)]
    pub cache_read: u64,
    #[serde(default)]
    pub cache_write_5m: u64,
    #[serde(default)]
    pub cache_write_1h: u64,
}

impl Usage {
    /// Complete prompt size, including every cache category.
    pub fn prompt_tokens(self) -> u64 {
        self.input
            .saturating_add(self.cache_read)
            .saturating_add(self.cache_write_5m)
            .saturating_add(self.cache_write_1h)
    }

    /// Add another provider request without allowing malformed counters to wrap.
    pub fn add(&mut self, other: Usage) {
        self.input = self.input.saturating_add(other.input);
        self.output = self.output.saturating_add(other.output);
        self.cache_read = self.cache_read.saturating_add(other.cache_read);
        self.cache_write_5m = self.cache_write_5m.saturating_add(other.cache_write_5m);
        self.cache_write_1h = self.cache_write_1h.saturating_add(other.cache_write_1h);
    }
}

/// Provenance and accounting for one model response. Session logs persist this
/// beside, not inside, the provider-facing message and retain it through compaction.
#[derive(Clone, Debug, PartialEq, Serialize, serde::Deserialize)]
pub struct ResponseMeta {
    pub id: String,
    /// Completion time in Unix milliseconds; unlike the containing log-entry
    /// time, this remains stable when compaction carries the response forward.
    pub timestamp: u64,
    pub provider: String,
    pub model: String,
    pub purpose: ResponsePurpose,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
}

impl ResponseMeta {
    /// Mint one durable local response identity without exposing account data.
    pub fn new(model: &catalog::Model, purpose: ResponsePurpose, usage: Option<Usage>) -> Self {
        Self {
            id: uuid::Uuid::now_v7().to_string(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_millis() as u64)
                .unwrap_or(0),
            provider: model.provider.clone(),
            model: model.id.clone(),
            purpose,
            usage,
        }
    }
}

/// Why e made a provider request; non-chat work still belongs in usage totals.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponsePurpose {
    Turn,
    Compaction,
}

/// A conversation record. Response metadata is session provenance, not model
/// input, so ordinary message serialization deliberately leaves it out.
#[derive(Clone, Debug, PartialEq, Serialize, serde::Deserialize)]
pub struct ChatMessage {
    pub content: String,
    #[serde(flatten)]
    pub kind: MessageKind,
    #[serde(skip)]
    response: Option<Box<ResponseMeta>>,
}

/// Fields that are valid for each message role. Provider-owned reasoning
/// remains opaque and is replayed only by the dialect that recognizes it.
#[derive(Clone, Debug, PartialEq, Serialize, serde::Deserialize)]
#[serde(tag = "role", rename_all = "lowercase")]
pub enum MessageKind {
    User {
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        images: Vec<ImageInput>,
        #[serde(default, skip_serializing_if = "not_internal")]
        internal: bool,
    },
    Assistant {
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        tool_calls: Vec<ToolCall>,
    },
    Tool {
        tool_call_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tool_meta: Option<ToolResultMeta>,
    },
    Reasoning,
    System,
}

fn not_internal(internal: &bool) -> bool {
    !internal
}

impl ChatMessage {
    /// A user instruction without attachments.
    pub fn user(content: impl Into<String>) -> Self {
        Self::user_with_images(content, Vec::new())
    }

    /// A user instruction with persisted image bytes.
    pub fn user_with_images(content: impl Into<String>, images: Vec<ImageInput>) -> Self {
        Self {
            content: content.into(),
            kind: MessageKind::User {
                images,
                internal: false,
            },
            response: None,
        }
    }

    /// A model reply and the tool calls issued in that same response.
    pub fn assistant(content: impl Into<String>, tool_calls: Vec<ToolCall>) -> Self {
        Self {
            content: content.into(),
            kind: MessageKind::Assistant { tool_calls },
            response: None,
        }
    }

    /// Opaque signed or encrypted state, replayed by its provider dialect.
    pub fn reasoning(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            kind: MessageKind::Reasoning,
            response: None,
        }
    }

    /// A tool result linked to the assistant call that requested it.
    pub fn tool_result(call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            kind: MessageKind::Tool {
                tool_call_id: call_id.into(),
                tool_meta: None,
            },
            response: None,
        }
    }

    /// A tool result with the presentation metadata needed for session replay.
    pub fn tool_result_with_meta(
        call_id: impl Into<String>,
        content: impl Into<String>,
        outcome: crate::core::tools::ToolOutcome,
        summary: impl Into<String>,
    ) -> Self {
        Self {
            content: content.into(),
            kind: MessageKind::Tool {
                tool_call_id: call_id.into(),
                tool_meta: Some(ToolResultMeta {
                    outcome,
                    summary: summary.into(),
                }),
            },
            response: None,
        }
    }

    /// Attach response provenance for session persistence; provider dialects ignore it.
    pub fn with_response(mut self, response: ResponseMeta) -> Self {
        self.response = Some(Box::new(response));
        self
    }

    /// Response provenance restored from a session entry, if this message caused a request.
    pub fn response(&self) -> Option<&ResponseMeta> {
        self.response.as_deref()
    }

    /// Restore metadata held by the session envelope, never provider history.
    pub(crate) fn restore_response(&mut self, response: Option<ResponseMeta>) {
        self.response = response.map(Box::new);
    }

    /// Mark a continuation or steering echo without counting a new user turn.
    pub fn mark_internal(&mut self) {
        if let MessageKind::User { internal, .. } = &mut self.kind {
            *internal = true;
        }
    }

    /// Whether the record is a continuation or steering echo.
    pub fn is_internal(&self) -> bool {
        matches!(self.kind, MessageKind::User { internal: true, .. })
    }

    /// The stable persisted role name for this typed payload.
    pub fn role(&self) -> &'static str {
        match self.kind {
            MessageKind::User { .. } => "user",
            MessageKind::Assistant { .. } => "assistant",
            MessageKind::Tool { .. } => "tool",
            MessageKind::Reasoning => "reasoning",
            MessageKind::System => "system",
        }
    }

    /// Calls issued by an assistant, or an empty slice for other records.
    pub fn tool_calls(&self) -> &[ToolCall] {
        match &self.kind {
            MessageKind::Assistant { tool_calls, .. } => tool_calls,
            _ => &[],
        }
    }

    /// The associated call id, present only on tool results.
    pub fn tool_call_id(&self) -> Option<&String> {
        match &self.kind {
            MessageKind::Tool { tool_call_id, .. } => Some(tool_call_id),
            _ => None,
        }
    }

    /// Optional display information recorded with a tool result.
    pub fn tool_meta(&self) -> Option<&ToolResultMeta> {
        match &self.kind {
            MessageKind::Tool { tool_meta, .. } => tool_meta.as_ref(),
            _ => None,
        }
    }

    /// User attachments, or an empty slice for other records.
    pub fn images(&self) -> &[ImageInput] {
        match &self.kind {
            MessageKind::User { images, .. } => images,
            _ => &[],
        }
    }
}

/// Drop images the selected model can't accept from a request's message
/// history — a resumed session, or a mid-session model switch, can carry
/// image-bearing turns from an earlier, image-capable model forward to one
/// that isn't. `load_images` already refuses a *new* attachment against an
/// incompatible model; this is the historical case, where sending the
/// request unchanged would get the whole turn rejected by a backend that
/// doesn't understand image content at all. Never touches the session's
/// own stored history — callers pass their own copy of it (the agent turn
/// loop's `messages`, cloned fresh from `history` each step).
pub fn strip_incompatible_images(messages: &mut [ChatMessage], model: &Model) {
    if model.image_input {
        return;
    }
    for message in messages.iter_mut() {
        if message.images().is_empty() {
            continue;
        }
        let count = message.images().len();
        if let MessageKind::User { images, .. } = &mut message.kind {
            images.clear();
        }
        message.content.push_str(&format!(
            "\n\n[{count} image{} omitted: {} is not declared image-capable]",
            if count == 1 { "" } else { "s" },
            catalog::slug(model)
        ));
    }
}
