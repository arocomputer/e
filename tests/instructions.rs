//! Nested `AGENTS.md`: a tool touching a path under a directory with one
//! adds that file to the conversation before the next request, once per
//! session, in a trusted workspace only.

mod common;

use common::{env_lock, serve_sse, test_model, Home};
use e::core::agent::{Agent, SessionEvent};
use e::core::providers::catalog::Api;

const READ_SUB: &str = concat!(
    "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c1\",",
    "\"function\":{\"name\":\"read\",\"arguments\":\"{\\\"path\\\":\\\"sub/deep/file.txt\\\"}\"}}]}}]}\n\n",
    "data: {\"choices\":[{\"finish_reason\":\"tool_calls\"}]}\n\n",
    "data: [DONE]\n\n",
);
const REPLY: &str = concat!(
    "data: {\"choices\":[{\"delta\":{\"content\":\"done\"}}]}\n\n",
    "data: [DONE]\n\n",
);

fn workspace(label: &str) -> std::path::PathBuf {
    let ws = std::env::temp_dir().join(format!("e-nested-{label}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&ws);
    std::fs::create_dir_all(ws.join("sub/deep")).unwrap();
    std::fs::write(ws.join("sub/deep/file.txt"), "hello\n").unwrap();
    std::fs::write(ws.join("sub/AGENTS.md"), "Be gentle with sub.\n").unwrap();
    std::fs::write(ws.join("sub/deep/AGENTS.md"), "Deep rules win.\n").unwrap();
    ws.canonicalize().unwrap()
}

async fn run_turn(
    agent: &mut Agent,
    rx: &mut tokio::sync::mpsc::Receiver<SessionEvent>,
) -> Vec<String> {
    let mut loaded = Vec::new();
    while let Some(event) = rx.recv().await {
        match event {
            SessionEvent::Instructions { path } => loaded.push(path),
            SessionEvent::TurnEnd { .. } => break,
            _ => {}
        }
    }
    let _ = agent;
    loaded
}

#[allow(clippy::await_holding_lock)]
#[tokio::test(flavor = "multi_thread")]
async fn a_trusted_workspace_loads_nested_instructions_once_nearest_last() {
    let _lock = env_lock();
    let (port, server) = serve_sse(&[READ_SUB, REPLY, READ_SUB, REPLY]);
    let home = Home::new("nested");
    home.auth(r#"{"mock":{"key":"k"}}"#);
    let ws = workspace("trusted");
    std::env::set_current_dir(&ws).unwrap();
    e::core::config::trust::set(&ws, true).unwrap();

    let (mut agent, mut rx) = Agent::new(test_model("mock", port, Api::Completions));
    agent.submit("read the file".into(), "sys".into());
    let loaded = run_turn(&mut agent, &mut rx).await;
    assert_eq!(
        loaded,
        vec![
            ws.join("sub/AGENTS.md").display().to_string(),
            ws.join("sub/deep/AGENTS.md").display().to_string()
        ],
        "outermost first, nearest last"
    );
    // Second turn touches the same directory: nothing loads twice.
    agent.submit("again".into(), "sys".into());
    assert!(run_turn(&mut agent, &mut rx).await.is_empty());

    let requests = server.join().unwrap();
    assert!(
        !requests[0].contains("Be gentle"),
        "not in the first request"
    );
    assert!(
        requests[1].contains("Be gentle with sub."),
        "in the request after the batch"
    );
    let sub = requests[1].find("Be gentle").unwrap();
    let deep = requests[1].find("Deep rules win").unwrap();
    assert!(sub < deep, "nearest instructions come last");
    assert_eq!(
        requests[3].matches("Be gentle").count(),
        1,
        "loaded once per session"
    );
    let _ = std::fs::remove_dir_all(&ws);
}

/// A symlink under the checkout must not reach instructions outside it,
/// and a FIFO named `AGENTS.md` must not hang the turn.
#[cfg(unix)]
#[allow(clippy::await_holding_lock)]
#[tokio::test(flavor = "multi_thread")]
async fn linked_and_irregular_instruction_files_are_not_loaded() {
    const READ_LINKED: &str = concat!(
        "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c1\",",
        "\"function\":{\"name\":\"read\",\"arguments\":\"{\\\"path\\\":\\\"linked/file.txt\\\"}\"}},",
        "{\"index\":1,\"id\":\"c2\",",
        "\"function\":{\"name\":\"read\",\"arguments\":\"{\\\"path\\\":\\\"fifo/file.txt\\\"}\"}}]}}]}\n\n",
        "data: {\"choices\":[{\"finish_reason\":\"tool_calls\"}]}\n\n",
        "data: [DONE]\n\n",
    );
    let _lock = env_lock();
    let (port, server) = serve_sse(&[READ_LINKED, REPLY]);
    let home = Home::new("nested-links");
    home.auth(r#"{"mock":{"key":"k"}}"#);
    let ws = workspace("links");
    // An outside directory with instructions, reachable through a link.
    let outside = std::env::temp_dir().join(format!("e-nested-outside-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&outside);
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("AGENTS.md"), "OUTSIDE RULES\n").unwrap();
    std::fs::write(outside.join("file.txt"), "x\n").unwrap();
    std::os::unix::fs::symlink(&outside, ws.join("linked")).unwrap();
    // A FIFO where instructions would be.
    std::fs::create_dir_all(ws.join("fifo")).unwrap();
    std::fs::write(ws.join("fifo/file.txt"), "x\n").unwrap();
    let status = std::process::Command::new("mkfifo")
        .arg(ws.join("fifo/AGENTS.md"))
        .status()
        .unwrap();
    assert!(status.success());
    std::env::set_current_dir(&ws).unwrap();
    e::core::config::trust::set(&ws, true).unwrap();

    let (mut agent, mut rx) = Agent::new(test_model("mock", port, Api::Completions));
    agent.submit("read both".into(), "sys".into());
    let loaded = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        run_turn(&mut agent, &mut rx),
    )
    .await
    .expect("a FIFO must not hang the turn");
    assert!(loaded.is_empty(), "{loaded:?}");
    let requests = server.join().unwrap();
    assert!(!requests[1].contains("OUTSIDE RULES"));
    let _ = std::fs::remove_dir_all(&ws);
    let _ = std::fs::remove_dir_all(&outside);
}

/// A resumed history already carrying a directory's instructions is not
/// given them again.
#[allow(clippy::await_holding_lock)]
#[tokio::test(flavor = "multi_thread")]
async fn a_resumed_history_does_not_repeat_its_instructions() {
    let _lock = env_lock();
    let (port, server) = serve_sse(&[READ_SUB, REPLY, READ_SUB, REPLY]);
    let home = Home::new("nested-resume");
    home.auth(r#"{"mock":{"key":"k"}}"#);
    let ws = workspace("resume");
    std::env::set_current_dir(&ws).unwrap();
    e::core::config::trust::set(&ws, true).unwrap();

    let (mut agent, mut rx) = Agent::new(test_model("mock", port, Api::Completions));
    agent.submit("read the file".into(), "sys".into());
    assert_eq!(run_turn(&mut agent, &mut rx).await.len(), 2);
    let history = agent.history_snapshot();

    // A fresh agent resuming that history: the instructions it carries
    // count as loaded.
    let (mut resumed, mut rx) = Agent::new(test_model("mock", port, Api::Completions));
    resumed.load_history(history);
    resumed.submit("again".into(), "sys".into());
    assert!(run_turn(&mut resumed, &mut rx).await.is_empty());
    let requests = server.join().unwrap();
    assert_eq!(
        requests[3].matches("Be gentle").count(),
        1,
        "once, not twice"
    );
    let _ = std::fs::remove_dir_all(&ws);
}

#[allow(clippy::await_holding_lock)]
#[tokio::test(flavor = "multi_thread")]
async fn an_untrusted_workspace_loads_nothing() {
    let _lock = env_lock();
    let (port, server) = serve_sse(&[READ_SUB, REPLY]);
    let home = Home::new("nested-untrusted");
    home.auth(r#"{"mock":{"key":"k"}}"#);
    let ws = workspace("untrusted");
    std::env::set_current_dir(&ws).unwrap();

    let (mut agent, mut rx) = Agent::new(test_model("mock", port, Api::Completions));
    agent.submit("read the file".into(), "sys".into());
    assert!(run_turn(&mut agent, &mut rx).await.is_empty());
    let requests = server.join().unwrap();
    assert!(!requests[1].contains("Be gentle"));
    let _ = std::fs::remove_dir_all(&ws);
}

#[allow(clippy::await_holding_lock)]
#[tokio::test(flavor = "multi_thread")]
async fn a_message_attached_to_the_next_turn_precedes_the_prompt_once() {
    let _lock = env_lock();
    let (port, server) = serve_sse(&[REPLY, REPLY]);
    let home = Home::new("next-turn");
    home.auth(r#"{"mock":{"key":"k"}}"#);
    let ws = workspace("next-turn");
    std::env::set_current_dir(&ws).unwrap();
    let (mut agent, mut rx) = Agent::new(test_model("mock", port, Api::Completions));
    agent.attach_to_next_turn("remember: the deploy target is staging".into());
    agent.submit("what is the target?".into(), "sys".into());
    run_turn(&mut agent, &mut rx).await;
    agent.submit("and again".into(), "sys".into());
    run_turn(&mut agent, &mut rx).await;
    let history = agent.history_snapshot();
    assert!(history[0].is_internal() && history[0].content.contains("staging"));
    assert_eq!(history[1].content, "what is the target?");
    let requests = server.join().unwrap();
    let first = &requests[0];
    assert!(first.find("staging").unwrap() < first.find("what is the target").unwrap());
    assert_eq!(
        requests[1].matches("staging").count(),
        1,
        "attached once, not per turn"
    );
    let _ = std::fs::remove_dir_all(&ws);
}
