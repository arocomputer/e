//! The full reader must open after a long transcript and restore its main buffer.
#![cfg(unix)]
mod common;

use std::process::Command;
use ulo::core::providers::{ChatMessage, ToolCall};
use ulo::core::session::SessionLog;

#[test]
fn long_transcript_reader_shows_full_output_and_restores_the_main_screen() {
    let _lock = common::env_lock();
    let home = common::Home::new("pty-reader");
    let workspace = home.dir.join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    let workspace = workspace.canonicalize().unwrap();
    home.write("models.json", r#"{"providers":{"mock":{"base_url":"http://127.0.0.1:9","api":"openai-completions","catalog":"none","models":["test"]}}}"#);
    home.write("auth.json", r#"{"mock":{"key":"test"}}"#);
    home.write("settings.json", r#"{"theme":"dark"}"#);
    home.write(
        "trust.json",
        serde_json::json!({workspace.to_str().unwrap(): {"trusted":true}}).to_string(),
    );
    let mut log = SessionLog::create(&workspace, "mock/test").unwrap();
    for i in 0..30 {
        log.append(&ChatMessage::user(format!("earlier question {i}")))
            .unwrap();
        log.append(&ChatMessage::assistant(
            format!("earlier answer {i}"),
            Vec::new(),
        ))
        .unwrap();
    }
    log.append(&ChatMessage::assistant(
        "",
        vec![ToolCall {
            id: "probe".into(),
            name: "read".into(),
            arguments: r#"{"path":"report.txt"}"#.into(),
            signature: None,
        }],
    ))
    .unwrap();
    log.append(&ChatMessage::tool_result_with_meta(
        "probe",
        "READER_OUTPUT_END",
        ulo::core::tools::ToolOutcome::Completed,
        "1 line",
    ))
    .unwrap();
    drop(log);

    let capture = home.dir.join("reader.raw");
    let result = Command::new("python3")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../scripts/ptycap.py"
        ))
        .arg(&capture)
        .args(["100", "30", "1.5", "8"])
        .arg(env!("CARGO_BIN_EXE_ulo"))
        .args([
            "--continue",
            "--no-save",
            "--no-tools",
            "--no-extensions",
            "--model",
            "mock/test",
        ])
        .current_dir(workspace)
        .env("ULO_HOME", &home.dir)
        .env("CAP_PROMPT", "\u{f}")
        .env("CAP_WAIT_FOR", "Review · ←/→ switch")
        .env("CAP_EXIT", "\u{f}")
        .env("CAP_EXIT_WAIT", "1")
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let raw = std::fs::read_to_string(capture).unwrap();
    let (_, review) = raw
        .split_once("\x1b[?1049h")
        .expect("reader must enter its own terminal buffer");
    let (review, restored) = review
        .split_once("\x1b[?1049l")
        .expect("closing must restore the main buffer");
    assert!(
        review.contains("READER_OUTPUT_END"),
        "reader opened blank or at the wrong end"
    );
    assert!(review.contains("Review · ←/→ switch · ctrl o close · PgUp/PgDn scroll · Esc close"));
    assert!(
        !restored.contains("READER_OUTPUT_END"),
        "full output leaked into normal transcript"
    );
    assert!(
        !raw.contains("\x1b[3J"),
        "opening and closing must not clear scrollback"
    );
    assert!(
        raw.contains("\x1b[?1006h") && !restored.contains("\x1b[?1006l"),
        "mouse capture must remain active for conversation scrolling after review closes"
    );
}
