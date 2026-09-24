//! Deliver extension hooks and events with bounded waits and fail-open results.

use super::*;

impl ExtensionHost {
    /// Ask every extension with the `tool_call` hook. The first explicit block
    /// wins; transport failures and timeouts allow (fail open).
    pub async fn hook_tool_call(&self, name: &str, arguments: &str) -> Option<String> {
        let args: Value =
            serde_json::from_str(arguments).unwrap_or(Value::String(arguments.into()));
        for ext in &self.extensions {
            if !ext.manifest.hooks.iter().any(|h| h == "tool_call") {
                continue;
            }
            if let Ok(value) = self
                .request(
                    ext,
                    "hook.tool_call",
                    json!({"name": name, "arguments": args}),
                    HOOK_TIMEOUT,
                )
                .await
            {
                let verdict: HookVerdict = serde_json::from_value(value).unwrap_or_default();
                if verdict.block {
                    return Some(
                        verdict
                            .reason
                            .unwrap_or_else(|| format!("blocked by {}", ext.manifest.name)),
                    );
                }
            }
        }
        None
    }

    /// Whether any extension listens for input — the app skips the hook
    /// round-trip entirely when none does.
    pub fn has_input_hook(&self) -> bool {
        self.extensions
            .iter()
            .any(|ulo| ulo.manifest.hooks.iter().any(|h| h == "input"))
    }

    /// Ask every extension with the `input` hook. The first extension to
    /// consume or replace a line wins; transport failures and timeouts allow
    /// (fail open — a slow extension never eats a user's message). An
    /// allowing extension may still attach a `notice`; those accumulate,
    /// one per line, onto whichever verdict is finally returned.
    pub async fn hook_input(&self, text: &str) -> InputVerdict {
        // Fast path: no extension listens at all.
        if !self
            .extensions
            .iter()
            .any(|ulo| ulo.manifest.hooks.iter().any(|h| h == "input"))
        {
            return InputVerdict::default();
        }
        let mut notices: Vec<String> = Vec::new();
        for ext in &self.extensions {
            if !ext.manifest.hooks.iter().any(|h| h == "input") {
                continue;
            }
            if let Ok(value) = self
                .request(ext, "hook.input", json!({"text": text}), HOOK_TIMEOUT)
                .await
            {
                let mut verdict: InputVerdict = serde_json::from_value(value).unwrap_or_default();
                notices.extend(verdict.notice.take().filter(|n| !n.trim().is_empty()));
                if verdict.consume || verdict.replace.as_deref().is_some_and(|r| !r.is_empty()) {
                    verdict.notice = join_notices(notices);
                    return verdict;
                }
            }
        }
        InputVerdict {
            notice: join_notices(notices),
            ..InputVerdict::default()
        }
    }

    /// Ask every extension with the `before_turn` hook, in order, what to
    /// add to this turn: system-prompt paragraphs and conversation
    /// messages. Failures contribute nothing (fail open).
    pub async fn hook_before_turn(&self, prompt: &str) -> BeforeTurn {
        let mut out = BeforeTurn::default();
        for ext in &self.extensions {
            if !ext.manifest.hooks.iter().any(|h| h == "before_turn") {
                continue;
            }
            if let Ok(value) = self
                .request(
                    ext,
                    "hook.before_turn",
                    json!({"prompt": prompt}),
                    HOOK_TIMEOUT,
                )
                .await
            {
                let result: BeforeTurnResult = serde_json::from_value(value).unwrap_or_default();
                if let Some(suffix) = result.system_suffix.filter(|s| !s.trim().is_empty()) {
                    out.system_suffixes.push(suffix);
                }
                if let Some(message) = result.message.filter(|m| !m.content.trim().is_empty()) {
                    out.messages.push(message);
                }
            }
        }
        out
    }

    /// Let every extension with the `tool_result` hook rewrite what the
    /// model reads, in order — each sees the previous one's text. None when
    /// nothing changed.
    pub async fn hook_tool_result(
        &self,
        name: &str,
        content: &str,
        is_error: bool,
    ) -> Option<String> {
        let mut current: Option<String> = None;
        for ext in &self.extensions {
            if !ext.manifest.hooks.iter().any(|h| h == "tool_result") {
                continue;
            }
            let text = current.as_deref().unwrap_or(content);
            if let Ok(value) = self
                .request(
                    ext,
                    "hook.tool_result",
                    json!({"name": name, "content": text, "is_error": is_error}),
                    HOOK_TIMEOUT,
                )
                .await
            {
                let patch: ToolResultPatch = serde_json::from_value(value).unwrap_or_default();
                if let Some(replacement) = patch.content {
                    current = Some(replacement);
                }
            }
        }
        current
    }

    /// Whether any extension asked to render `subject` — `tool:<name>` or
    /// `assistant` — through its `render` hook.
    pub fn renders(&self, subject: &str) -> bool {
        self.extensions
            .iter()
            .any(|ulo| Self::wants_render(&ulo.manifest, subject))
    }

    pub(super) fn wants_render(manifest: &Manifest, subject: &str) -> bool {
        manifest.hooks.iter().any(|h| h == "render")
            && manifest.renders.iter().any(|r| {
                r == subject || (r == "tool:*" && subject.starts_with("tool:")) || r == "*"
            })
    }

    /// Ask every extension that renders `subject`, in order, for the body
    /// to show instead of `content`; each sees the previous answer. None
    /// when nobody changed anything. `kind` is `tool` or `assistant`,
    /// `name` the tool (empty for a reply).
    pub async fn hook_render(&self, subject: &str, name: &str, content: &str) -> Option<Show> {
        let mut current: Option<Show> = None;
        let kind = subject.split(':').next().unwrap_or(subject);
        for ext in &self.extensions {
            if !Self::wants_render(&ext.manifest, subject) {
                continue;
            }
            let text = current.as_ref().map(|s| s.body.as_str()).unwrap_or(content);
            if let Ok(value) = self
                .request(
                    ext,
                    "hook.render",
                    json!({"kind": kind, "name": name, "content": text}),
                    HOOK_TIMEOUT,
                )
                .await
            {
                let result: RenderResult = serde_json::from_value(value).unwrap_or_default();
                if let Some(body) = result.body {
                    current = Some(Show {
                        title: String::new(),
                        body,
                        format: result.format,
                    });
                }
            }
        }
        current
    }

    /// Let every extension with the `compact_summary` hook edit the summary
    /// about to replace the conversation, in order. None when unchanged.
    pub async fn hook_compact_summary(&self, summary: &str) -> Option<String> {
        let mut current: Option<String> = None;
        for ext in &self.extensions {
            if !ext.manifest.hooks.iter().any(|h| h == "compact_summary") {
                continue;
            }
            let text = current.as_deref().unwrap_or(summary);
            if let Ok(value) = self
                .request(
                    ext,
                    "hook.compact_summary",
                    json!({"summary": text}),
                    HOOK_TIMEOUT,
                )
                .await
            {
                let result: CompactSummaryResult =
                    serde_json::from_value(value).unwrap_or_default();
                if let Some(replacement) = result.summary.filter(|s| !s.trim().is_empty()) {
                    current = Some(replacement);
                }
            }
        }
        current
    }

    /// Whether any extension declared one of these hooks — callers skip the
    /// round trip (and its bookkeeping) entirely when none did.
    pub fn has_hook(&self, hook: &str) -> bool {
        self.extensions
            .iter()
            .any(|ulo| ulo.manifest.hooks.iter().any(|h| h == hook))
    }

    /// Fire-and-forget lifecycle event to every subscribed extension. A
    /// version-1 manifest (no `events`) receives `turn_end` alone. try_send:
    /// a child that stopped reading stdin gets its queue dropped, never our
    /// loop.
    pub async fn event(&self, name: &str, params: Value) {
        let line =
            json!({"method": "event", "params": {"name": name, "extra": params}}).to_string();
        for ext in &self.extensions {
            let subscribed = match &ext.manifest.events {
                None => name == "turn_end",
                Some(events) => events.iter().any(|ulo| ulo == name),
            };
            if subscribed {
                let _ = ext.writer.try_send(line.clone());
            }
        }
    }

    /// A notification to one extension by name (`ui.key` for an
    /// interactive panel). Unknown names and full queues are dropped.
    pub fn notify_extension(&self, extension: &str, method: &str, params: Value) {
        if let Some(ext) = self
            .extensions
            .iter()
            .find(|ulo| ulo.manifest.name == extension)
        {
            let line = json!({"method": method, "params": params}).to_string();
            let _ = ext.writer.try_send(line);
        }
    }
}
