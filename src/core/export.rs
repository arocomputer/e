//! Session export: one self-contained HTML page for a conversation branch,
//! for sharing or reading outside the terminal. Assistant text renders as
//! HTML through the same markdown parser the transcript uses; user prompts
//! stay literal; tool calls and results fold into `<details>`; reasoning,
//! internal steering echoes, and system notices stay out — they are not
//! the conversation. Everything is escaped, so a session that quoted HTML
//! cannot inject it into the page.

use crate::core::providers::{ChatMessage, MessageKind};

fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

fn markdown(text: &str) -> String {
    let parser = pulldown_cmark::Parser::new(text);
    let mut html = String::new();
    pulldown_cmark::html::push_html(&mut html, parser);
    html
}

const STYLE: &str = "\
:root{color-scheme:light dark;--fg:#1f1f1f;--bg:#fff;--rail:#7c3aed;--muted:#6b6b6b;--box:#f4f4f5;--border:#e4e4e7}\
@media(prefers-color-scheme:dark){:root{--fg:#e6e6e6;--bg:#111;--muted:#9a9a9a;--box:#1b1b1f;--border:#2a2a2e}}\
body{margin:0;padding:24px 16px;background:var(--bg);color:var(--fg);font:15px/1.5 system-ui,sans-serif}\
main{max-width:860px;margin:0 auto}header{color:var(--muted);margin-bottom:24px;font-size:13px}\
h1{font-size:18px;margin:0 0 4px;color:var(--fg)}\
.user{border-left:3px solid var(--rail);padding:4px 12px;margin:20px 0;white-space:pre-wrap;font-weight:600}\
.assistant{margin:16px 0}.assistant pre{background:var(--box);border:1px solid var(--border);padding:12px;overflow:auto;border-radius:6px}\
.assistant code{font-family:ui-monospace,monospace;font-size:13px}\
details{margin:8px 0;border:1px solid var(--border);border-radius:6px;background:var(--box)}\
summary{cursor:pointer;padding:6px 10px;color:var(--muted);font-family:ui-monospace,monospace;font-size:13px}\
details pre{margin:0;padding:10px;white-space:pre-wrap;font-size:12px;overflow:auto;max-height:480px}";

/// The page for `messages` (one branch, oldest first). `title` is the
/// session name or file stem; `model` the model the session ran on.
pub fn html(title: &str, model: &str, messages: &[ChatMessage]) -> String {
    let mut body = String::new();
    // Tool results follow their calls; pair them by call id so a result
    // folds under the call it answers.
    let results: std::collections::HashMap<&str, &ChatMessage> = messages
        .iter()
        .filter_map(|m| m.tool_call_id().map(|id| (id.as_str(), m)))
        .collect();
    for message in messages {
        match &message.kind {
            MessageKind::User { .. } if message.is_internal() => {}
            MessageKind::User { images, .. } => {
                body.push_str("<div class=\"user\">");
                body.push_str(&escape(&message.content));
                if !images.is_empty() {
                    body.push_str(&format!(
                        " <span class=\"muted\">[{} image{}]</span>",
                        images.len(),
                        if images.len() == 1 { "" } else { "s" }
                    ));
                }
                body.push_str("</div>\n");
            }
            MessageKind::Assistant { tool_calls, .. } => {
                if !message.content.trim().is_empty() {
                    body.push_str("<div class=\"assistant\">");
                    body.push_str(&markdown(&message.content));
                    body.push_str("</div>\n");
                }
                for call in tool_calls {
                    body.push_str("<details><summary>");
                    body.push_str(&escape(&call.name));
                    body.push(' ');
                    body.push_str(&escape(&call.arguments));
                    body.push_str("</summary>");
                    if let Some(result) = results.get(call.id.as_str()) {
                        body.push_str("<pre>");
                        body.push_str(&escape(&result.content));
                        body.push_str("</pre>");
                    }
                    body.push_str("</details>\n");
                }
            }
            MessageKind::Tool { .. } | MessageKind::Reasoning | MessageKind::System => {}
        }
    }
    format!(
        "<!doctype html>\n<html lang=\"en\"><head><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width,initial-scale=1\"><title>{title}</title><style>{STYLE}</style></head>\n<body><main><header><h1>{title}</h1>{model} · exported by e</header>\n{body}</main></body></html>\n",
        title = escape(title),
        model = escape(model),
    )
}
