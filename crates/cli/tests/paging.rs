//! Tool-result paging: a long bash output reaches the model as its tail with
//! a notice naming a kept result; `read_result` reads any byte window or the
//! matching lines; grep and extension-style results cap the same way; the
//! store is bounded and an evicted id is reported, not guessed.

use std::sync::atomic::AtomicBool;

use ulo::core::tools::{ToolOutcome, ToolRuntime};

fn run(runtime: &ToolRuntime, name: &str, args: &str) -> ulo::core::tools::ToolOutput {
    let cancel = AtomicBool::new(false);
    runtime.run_streaming(name, args, std::path::Path::new("."), &cancel, |_, _| {})
}

fn result_id(notice: &str) -> u64 {
    let start = notice.find("\"id\": ").expect("notice names an id") + 6;
    notice[start..]
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .unwrap()
}

#[test]
fn long_bash_output_pages_from_the_beginning() {
    let runtime = ToolRuntime::default();
    // 100k numbered lines: far past the 32 KiB copy, well inside retention.
    let output = run(
        &runtime,
        "bash",
        r#"{"command":"seq 1 100000 | sed 's/^/line /'"}"#,
    );
    assert_eq!(output.outcome, ToolOutcome::Completed);
    assert!(
        output.content.starts_with("… [truncated:"),
        "{}",
        &output.content[..80]
    );
    assert!(output.content.contains("read_result"));
    assert!(
        output.content.trim_end().ends_with("line 100000"),
        "the tail is what the model sees"
    );
    assert!(
        !output.content.contains("\nline 1\n"),
        "the head is deferred"
    );
    let id = result_id(&output.content);

    let head = run(
        &runtime,
        "read_result",
        &format!(r#"{{"id":{id},"limit":64}}"#),
    );
    assert_eq!(head.outcome, ToolOutcome::Completed);
    assert!(
        head.content.starts_with("line 1\nline 2\n"),
        "{}",
        head.content
    );
    assert!(head.content.contains("continue with offset 64"));

    let next = run(
        &runtime,
        "read_result",
        &format!(r#"{{"id":{id},"offset":64,"limit":16}}"#),
    );
    assert!(
        next.content.starts_with("ine 10\nline 11"),
        "{}",
        next.content
    );

    let found = run(
        &runtime,
        "read_result",
        &format!(r#"{{"id":{id},"query":"line 4242"}}"#),
    );
    assert!(
        found.content.contains("4242\tline 4242\n"),
        "{}",
        found.content
    );
    assert!(found.content.contains("42420\tline 42420\n"));
    assert_eq!(found.summary, "11 matches");

    let past = run(
        &runtime,
        "read_result",
        &format!(r#"{{"id":{id},"offset":99999999}}"#),
    );
    assert_eq!(past.outcome, ToolOutcome::Failed);
    let missing = run(&runtime, "read_result", r#"{"id":9999}"#);
    assert!(missing.content.contains("no result 9999"));
}

#[test]
fn capped_results_keep_the_whole_and_the_store_evicts_the_oldest() {
    let runtime = ToolRuntime::default();
    let big = "x".repeat(40 * 1024);
    let shown = runtime.cap(big.clone());
    assert!(shown.starts_with(&"x".repeat(32 * 1024)));
    assert!(
        shown.contains("read_result {\"id\": 1, \"offset\": 32768}"),
        "{}",
        &shown[32768..]
    );
    assert_eq!(runtime.result(1).as_deref(), Some(big.as_str()));
    assert_eq!(
        runtime.cap("short".into()),
        "short",
        "under the cap nothing is kept"
    );

    // 32 entries are kept; the 33rd evicts the first.
    for _ in 0..32 {
        runtime.retain_result("y".repeat(10));
    }
    assert!(runtime.result(1).is_none());
    assert!(runtime.result(33).is_some());
    // Bytes bound too: one 16 MiB result pushes everything else out.
    runtime.retain_result("z".repeat(16 * 1024 * 1024));
    assert!(runtime.result(33).is_none());
    assert!(runtime.result(34).is_some());
}

#[test]
fn read_result_survives_every_tool_narrowing() {
    // A narrowed toolset (an extension's session.tools, an rpc allowlist)
    // still advertises the pager, or the truncation notice would point at a
    // tool the model cannot call.
    let names = |schemas: Vec<serde_json::Value>| -> Vec<String> {
        schemas
            .iter()
            .filter_map(|s| s["function"]["name"].as_str().map(str::to_string))
            .collect()
    };
    let narrowed = names(ulo::core::tools::restrict_to(
        ulo::core::tools::schemas(),
        Some(&["read".to_string()]),
    ));
    assert_eq!(
        narrowed,
        vec!["read".to_string(), "read_result".to_string()]
    );
    assert!(ulo::core::tools::always_available("read_result"));
    assert!(!ulo::core::tools::always_available("bash"));
}

/// `read_result` reads its window the way `read` does: a numeric string or
/// an integral float is the number it spells, never silently the first
/// window again.
#[test]
fn read_result_accepts_numeric_strings_and_integral_floats() {
    let runtime = ToolRuntime::default();
    let output = run(
        &runtime,
        "bash",
        r#"{"command":"seq 1 100000 | sed 's/^/line /'"}"#,
    );
    let id = result_id(&output.content);
    let by_string = run(
        &runtime,
        "read_result",
        &format!(r#"{{"id":{id},"offset":"64","limit":"16"}}"#),
    );
    assert!(
        by_string.content.starts_with("ine 10\nline 11"),
        "{}",
        by_string.content
    );
    let by_float = run(
        &runtime,
        "read_result",
        &format!(r#"{{"id":{id},"offset":64.0,"limit":16.0}}"#),
    );
    assert_eq!(by_float.content, by_string.content);
    let bad = run(
        &runtime,
        "read_result",
        &format!(r#"{{"id":{id},"offset":"sixty"}}"#),
    );
    assert_eq!(bad.outcome, ToolOutcome::Failed);
    assert!(bad.content.contains("offset must be"), "{}", bad.content);
}

/// A limit smaller than the next character still returns that character:
/// an empty window that points at its own offset would loop forever.
#[test]
fn read_result_never_returns_an_empty_window_before_the_end() {
    let runtime = ToolRuntime::default();
    let output = run(&runtime, "bash", r#"{"command":"yes é | head -c 40000"}"#);
    let id = result_id(&output.content);
    let tiny = run(
        &runtime,
        "read_result",
        &format!(r#"{{"id":{id},"limit":1}}"#),
    );
    assert!(tiny.content.starts_with("é\n"), "{:?}", tiny.content);
    assert!(
        tiny.content.contains("continue with offset 2"),
        "{}",
        tiny.content
    );
}
