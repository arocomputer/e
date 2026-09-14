//! Session export: one HTML page per branch — prompts literal, replies as
//! rendered markdown, tool calls folded with their results, internal and
//! reasoning records left out, and every string escaped.

use e::core::providers::{ChatMessage, ToolCall};

#[test]
fn export_renders_the_branch_and_escapes_everything() {
    let mut steer = ChatMessage::user("hidden steering echo");
    steer.mark_internal();
    let messages = vec![
        ChatMessage::user("fix <main> & tell me"),
        ChatMessage::assistant(
            "Looking.",
            vec![ToolCall {
                id: "c1".into(),
                name: "read".into(),
                arguments: "{\"path\":\"<x>\"}".into(),
                signature: None,
            }],
        ),
        ChatMessage::tool_result("c1", "1\tfn main() {}\n<script>alert(1)</script>"),
        steer,
        ChatMessage::assistant(
            "Done: **fixed** `main`.\n\n```rust\nfn main() {}\n```",
            Vec::new(),
        ),
    ];
    let page = e::core::export::html("my <session>", "mock/test", &messages);
    assert!(page.starts_with("<!doctype html>"));
    assert!(page.contains("<title>my &lt;session&gt;</title>"));
    assert!(page.contains("<div class=\"user\">fix &lt;main&gt; &amp; tell me</div>"));
    assert!(page.contains("<strong>fixed</strong> <code>main</code>"));
    assert!(page.contains("<pre><code class=\"language-rust\">fn main() {}"));
    assert!(page.contains("<summary>read {&quot;path&quot;:&quot;&lt;x&gt;&quot;}</summary>"));
    assert!(page.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
    assert!(
        !page.contains("<script>"),
        "nothing from the session executes"
    );
    assert!(
        !page.contains("hidden steering echo"),
        "internal records stay out"
    );
}
