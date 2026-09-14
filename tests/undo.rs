//! `/undo`: write and edit snapshot what they replace; undoing restores
//! those bytes, removes a file the tool created, walks back through several
//! changes, and reports when nothing is left.

use std::sync::atomic::AtomicBool;

use e::core::tools::{ToolOutcome, ToolRuntime};

fn run(runtime: &ToolRuntime, cwd: &std::path::Path, name: &str, args: &str) -> ToolOutcome {
    let cancel = AtomicBool::new(false);
    let out = runtime.run_streaming(name, args, cwd, &cancel, |_, _| {});
    assert_eq!(out.outcome, ToolOutcome::Completed, "{}", out.content);
    out.outcome
}

#[test]
fn undo_walks_back_through_writes_and_edits() {
    let ws = std::env::temp_dir().join(format!("e-undo-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&ws);
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::write(ws.join("a.txt"), "one\ntwo\n").unwrap();
    let runtime = ToolRuntime::default();
    assert_eq!(runtime.undo_depth(), 0);
    assert_eq!(runtime.undo_last().unwrap(), None, "nothing to undo yet");

    run(
        &runtime,
        &ws,
        "edit",
        r#"{"path":"a.txt","old_string":"two","new_string":"TWO"}"#,
    );
    run(
        &runtime,
        &ws,
        "write",
        r#"{"path":"b.txt","content":"fresh\n"}"#,
    );
    run(
        &runtime,
        &ws,
        "write",
        r#"{"path":"a.txt","content":"rewritten\n"}"#,
    );
    assert_eq!(runtime.undo_depth(), 3);

    assert_eq!(runtime.undo_last().unwrap().as_deref(), Some("write a.txt"));
    assert_eq!(
        std::fs::read_to_string(ws.join("a.txt")).unwrap(),
        "one\nTWO\n"
    );
    assert_eq!(runtime.undo_last().unwrap().as_deref(), Some("write b.txt"));
    assert!(!ws.join("b.txt").exists(), "a created file is removed");
    assert_eq!(runtime.undo_last().unwrap().as_deref(), Some("edit a.txt"));
    assert_eq!(
        std::fs::read_to_string(ws.join("a.txt")).unwrap(),
        "one\ntwo\n"
    );
    assert_eq!(runtime.undo_last().unwrap(), None);

    // A restored file reads as fresh: the next edit needs no re-read.
    run(
        &runtime,
        &ws,
        "edit",
        r#"{"path":"a.txt","old_string":"one","new_string":"1"}"#,
    );
    let _ = std::fs::remove_dir_all(&ws);
}

#[cfg(unix)]
#[test]
fn a_failed_write_leaves_nothing_to_undo() {
    use std::os::unix::fs::PermissionsExt;
    let ws = std::env::temp_dir().join(format!("e-undo-fail-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&ws);
    std::fs::create_dir_all(ws.join("locked")).unwrap();
    std::fs::set_permissions(ws.join("locked"), std::fs::Permissions::from_mode(0o500)).unwrap();
    let runtime = ToolRuntime::default();
    let cancel = AtomicBool::new(false);
    let out = runtime.run_streaming(
        "write",
        r#"{"path":"locked/new.txt","content":"x"}"#,
        &ws,
        &cancel,
        |_, _| {},
    );
    assert_ne!(out.outcome, ToolOutcome::Completed);
    assert_eq!(
        runtime.undo_depth(),
        0,
        "a write that did not happen cannot be undone"
    );
    std::fs::set_permissions(ws.join("locked"), std::fs::Permissions::from_mode(0o700)).unwrap();
    let _ = std::fs::remove_dir_all(&ws);
}
