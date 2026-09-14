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
