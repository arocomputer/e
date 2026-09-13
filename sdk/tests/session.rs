//! The SDK's contract against a mock provider: a session resolves its
//! model through an explicitly scoped home, a turn streams events and
//! settles into a reply, failure and cancellation leave the session usable,
//! tools and steering flow through, and persisted sessions resume.
//!
//! The mock provider is the repository's shared one (`tests/common`). Homes
//! are plain temp directories passed to the builder — never `E_HOME` — so
//! these tests also prove that configuration injection works without
//! touching the process environment.

#[path = "../../tests/common/mod.rs"]
mod common;

use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::Duration;

use common::{request_json, serve_raw, serve_sse};
use e_sdk::{Event, Message, Session, Stop, ToolOutcome, Tools, Usage};

/// One plain reply with usage, in the Completions dialect.
const OK: &str = concat!(
    "data: {\"choices\":[{\"delta\":{\"content\":\"ok\"}}]}\n\n",
    "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":1}}\n\n",
    "data: [DONE]\n\n",
);

/// A tool round: ask to read hello.txt, then (next request) reply.
const READ_HELLO: &str = concat!(
    "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c1\",",
    "\"function\":{\"name\":\"read\",\"arguments\":\"{\\\"path\\\":\\\"hello.txt\\\"}\"}}]}}]}\n\n",
    "data: {\"choices\":[{\"finish_reason\":\"tool_calls\"}]}\n\n",
    "data: [DONE]\n\n",
);
const AFTER_READ: &str = concat!(
    "data: {\"choices\":[{\"delta\":{\"content\":\"the file has two lines\"}}]}\n\n",
    "data: [DONE]\n\n",
);

/// A throwaway directory, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "e-sdk-{label}-{}-{}",
            std::process::id(),
            uuid::Uuid::now_v7()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        TempDir(dir)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// An e home declaring one mock provider per `(name, port)`, each signed
/// in with an API key and serving a model called `test`.
fn mock_home(label: &str, providers: &[(&str, u16)]) -> TempDir {
    let home = TempDir::new(label);
    let mut auth = serde_json::Map::new();
    let mut declared = serde_json::Map::new();
    for (name, port) in providers {
        auth.insert(name.to_string(), serde_json::json!({"key": "k"}));
        declared.insert(
            name.to_string(),
            serde_json::json!({
                "base_url": format!("http://127.0.0.1:{port}"),
                "api": "completions",
                "models": ["test"],
            }),
        );
    }
    std::fs::write(
        home.0.join("auth.json"),
        serde_json::Value::Object(auth).to_string(),
    )
    .unwrap();
    std::fs::write(
        home.0.join("models.json"),
        serde_json::json!({"providers": declared}).to_string(),
    )
    .unwrap();
    home
}

/// A workspace holding the file the tool-round fixture reads.
fn workspace_with_hello(label: &str) -> TempDir {
    let ws = TempDir::new(label);
    std::fs::write(ws.0.join("hello.txt"), "line one\nline two\n").unwrap();
    ws
}

fn message_texts(body: &serde_json::Value) -> Vec<String> {
    body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["content"].as_str().unwrap_or_default().to_string())
        .collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_turn_streams_text_and_settles_into_a_reply_without_writing_home() {
    let (port, server) = serve_sse(&[OK]);
    let home = mock_home("stream", &[("mock", port)]);
    std::fs::write(home.0.join("AGENTS.md"), "HOME_MARKER_7c1e").unwrap();
    let ws = TempDir::new("stream-ws");
    let mut session = Session::builder()
        .home(&home.0)
        .cwd(&ws.0)
        .model("mock/test")
        .instructions("Answer tersely.")
        .build()
        .await
        .unwrap();
    assert_eq!(session.model(), "mock/test");

    let mut turn = session.prompt("hi");
    let mut streamed = String::new();
    let mut usage = None;
    while let Some(event) = turn.next().await {
        match event {
            Event::Text(delta) => streamed.push_str(&delta),
            Event::Usage(u) => usage = Some(u),
            other => panic!("unexpected event {other:?}"),
        }
    }
    let reply = turn.finish().await.unwrap();
    assert_eq!(streamed, "ok");
    assert_eq!(reply.text, "ok");
    assert_eq!(reply.stop, Stop::Complete);
    let expected = Usage {
        input: 5,
        output: 1,
        ..Usage::default()
    };
    assert_eq!(usage, Some(expected));
    assert_eq!(reply.usage, expected);
    assert_eq!(session.history().len(), 2);
    assert_eq!(session.path(), None);
    assert!(
        !home.0.join("sessions").exists(),
        "a memory-only session must leave no files in the home"
    );

    let body = request_json(&server.join().unwrap()[0]);
    assert_eq!(body["model"], "test");
    let system = &message_texts(&body)[0];
    assert!(
        system.contains("HOME_MARKER_7c1e"),
        "the scoped home's AGENTS.md must reach the prompt, got: {system}"
    );
    assert!(
        system.ends_with("Answer tersely."),
        "host instructions must close the system prompt, got: {system}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_provider_failure_is_an_error_carrying_the_partial_reply() {
    let (port, _server) = serve_raw(vec![
        "HTTP/1.1 401 Unauthorized\r\ncontent-length: 0\r\nconnection: close\r\n\r\n".into(),
    ]);
    let home = mock_home("failure", &[("mock", port)]);
    let mut session = Session::builder()
        .home(&home.0)
        .model("mock/test")
        .build()
        .await
        .unwrap();

    let error = session.prompt("hi").await.unwrap_err();
    assert!(!error.message.is_empty());
    assert_eq!(error.reply.text, "");
    assert_eq!(error.reply.stop, Stop::Complete);
    // The failed turn's user message is still in history, so a retry
    // does not need to resend it.
    assert_eq!(session.history().len(), 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn dropping_a_running_turn_interrupts_it_and_the_session_recovers() {
    // A provider that sends headers and then never speaks again.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let stalled_port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let (mut sock, _) = listener.accept().unwrap();
        let mut buffer = [0u8; 8192];
        let _ = sock.read(&mut buffer);
        let _ = sock.write_all(
            b"HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ntransfer-encoding: chunked\r\n\r\n",
        );
        std::thread::sleep(Duration::from_secs(30));
    });
    let (good_port, _server) = serve_sse(&[OK]);
    let home = mock_home("drop", &[("stalled", stalled_port), ("mock", good_port)]);
    let mut session = Session::builder()
        .home(&home.0)
        .model("stalled/test")
        .build()
        .await
        .unwrap();

    let mut turn = session.prompt("hi");
    let first = tokio::time::timeout(Duration::from_millis(300), turn.next()).await;
    assert!(first.is_err(), "the stalled provider must yield nothing");
    drop(turn);

    session.set_model("mock/test").unwrap();
    let reply = tokio::time::timeout(Duration::from_secs(3), session.prompt("again"))
        .await
        .expect("the next turn must not wait on the stalled one")
        .unwrap();
    assert_eq!(reply.text, "ok");
    // Both prompts were committed; only the second got an answer.
    let roles: Vec<&str> = session.history().iter().map(Message::role).collect();
    assert_eq!(roles, vec!["user", "user", "assistant"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn cancelling_a_turn_stops_it_and_reports_cancelled() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let (mut sock, _) = listener.accept().unwrap();
        let mut buffer = [0u8; 8192];
        let _ = sock.read(&mut buffer);
        std::thread::sleep(Duration::from_secs(30));
    });
    let home = mock_home("cancel", &[("mock", port)]);
    let mut session = Session::builder()
        .home(&home.0)
        .model("mock/test")
        .build()
        .await
        .unwrap();

    let mut turn = session.prompt("hi");
    let _ = tokio::time::timeout(Duration::from_millis(300), turn.next()).await;
    turn.cancel();
    let reply = tokio::time::timeout(Duration::from_secs(3), turn.finish())
        .await
        .expect("cancel must end a stalled turn promptly")
        .unwrap();
    assert_eq!(reply.stop, Stop::Cancelled);
}

#[tokio::test(flavor = "multi_thread")]
async fn tool_calls_are_announced_with_name_and_arguments_and_run_for_real() {
    let (port, server) = serve_sse(&[READ_HELLO, AFTER_READ]);
    let home = mock_home("tools", &[("mock", port)]);
    let ws = workspace_with_hello("tools-ws");
    let mut session = Session::builder()
        .home(&home.0)
        .cwd(&ws.0)
        .model("mock/test")
        .build()
        .await
        .unwrap();

    let mut turn = session.prompt("how many lines in hello.txt?");
    let mut order = Vec::new();
    while let Some(event) = turn.next().await {
        match event {
            Event::ToolCall {
                id,
                name,
                arguments,
            } => {
                assert_eq!(name, "read");
                assert_eq!(arguments, r#"{"path":"hello.txt"}"#);
                order.push(("call", id));
            }
            Event::ToolStart { id } => order.push(("start", id)),
            Event::ToolEnd {
                id,
                outcome,
                content,
                ..
            } => {
                assert_eq!(outcome, ToolOutcome::Completed);
                assert!(content.contains("line two"), "tool output: {content}");
                order.push(("end", id));
            }
            _ => {}
        }
    }
    let reply = turn.finish().await.unwrap();
    assert_eq!(reply.text, "the file has two lines");
    assert_eq!((reply.tools.calls, reply.tools.failures), (1, 0));
    let id = order[0].1;
    assert_eq!(order, vec![("call", id), ("start", id), ("end", id)]);
    let requests = server.join().unwrap();
    assert!(
        requests[1].contains("line one"),
        "tool result not sent back"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn steering_lands_before_the_next_request() {
    let (port, server) = serve_sse(&[READ_HELLO, AFTER_READ]);
    let home = mock_home("steer", &[("mock", port)]);
    let ws = workspace_with_hello("steer-ws");
    let mut session = Session::builder()
        .home(&home.0)
        .cwd(&ws.0)
        .model("mock/test")
        .build()
        .await
        .unwrap();

    let mut turn = session.prompt("how many lines in hello.txt?");
    let mut steered = None;
    while let Some(event) = turn.next().await {
        match event {
            Event::ToolCall { .. } => assert!(turn.steer("and count the words too")),
            Event::Steered(text) => steered = Some(text),
            _ => {}
        }
    }
    // Past the end nothing is delivered and, more to the point, nothing is
    // recorded as a stray prompt.
    assert!(!turn.steer("too late"));
    turn.finish().await.unwrap();
    assert_eq!(steered.as_deref(), Some("and count the words too"));
    assert!(session
        .history()
        .iter()
        .all(|message| !message.content.contains("too late")));
    let second = request_json(&server.join().unwrap()[1]);
    assert!(
        message_texts(&second)
            .iter()
            .any(|m| m.contains("and count the words too")),
        "the steer must reach the model on the next request"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn tools_none_advertises_nothing_and_says_so() {
    let (port, server) = serve_sse(&[OK]);
    let home = mock_home("notools", &[("mock", port)]);
    let mut session = Session::builder()
        .home(&home.0)
        .model("mock/test")
        .tools(Tools::None)
        .build()
        .await
        .unwrap();
    session.prompt("hi").await.unwrap();
    let body = request_json(&server.join().unwrap()[0]);
    assert!(body.get("tools").is_none(), "no schemas: {body}");
    assert!(message_texts(&body)[0].contains("no tools"));
}

/// `steer()` before the first poll promises delivery "when the turn starts".
/// A turn fast enough to finish before anyone polls must not race the early
/// steer out of the conversation: it rides in the same critical section as
/// the prompt and lands in the very first request.
#[tokio::test(flavor = "multi_thread")]
async fn an_early_steer_lands_in_the_first_request() {
    let (port, server) = serve_sse(&[OK]);
    let home = mock_home("early-steer", &[("mock", port)]);
    let mut session = Session::builder()
        .home(&home.0)
        .model("mock/test")
        .build()
        .await
        .unwrap();

    let mut turn = session.prompt("first");
    assert!(turn.steer("ride along"), "steering a fresh turn holds");
    let reply = turn.finish().await.unwrap();
    assert_eq!(reply.text, "ok");
    let texts = message_texts(&request_json(&server.join().unwrap()[0]));
    assert_eq!(
        &texts[1..],
        ["first", "ride along"],
        "both messages precede any reply"
    );
}

/// Turning persistence off after `resume()` must not silently release the
/// resumed file: the session keeps appending to the log it came from.
#[tokio::test(flavor = "multi_thread")]
async fn resume_cannot_be_unpersisted_afterwards() {
    let (port, _server) = serve_sse(&[OK, OK, OK]);
    let home = mock_home("resume-locked", &[("mock", port)]);
    let ws = TempDir::new("resume-locked-ws");
    let builder = || {
        Session::builder()
            .home(&home.0)
            .cwd(&ws.0)
            .model("mock/test")
    };
    let mut session = builder().persist(true).build().await.unwrap();
    session.prompt("first").await.unwrap();
    let path = session.path().expect("a persisted session has a file");
    drop(session);

    let mut resumed = builder()
        .resume(&path)
        .persist(false)
        .build()
        .await
        .unwrap();
    resumed.prompt("second").await.unwrap();
    assert_eq!(
        e_sdk::transcript(&path).unwrap().len(),
        4,
        "the resumed file kept growing despite persist(false) after resume"
    );
}

/// With `persist(true)`, a home that cannot create session logs fails at
/// `build()` — the failure must not wait until the first prompt has already
/// run and quietly gone memory-only.
#[tokio::test]
async fn build_checks_the_home_can_persist_before_any_turn() {
    let (port, _server) = serve_sse(&[OK]);
    let home = mock_home("unwritable", &[("mock", port)]);
    let sessions = home.0.join("sessions");
    std::fs::create_dir(&sessions).unwrap();
    std::fs::set_permissions(&sessions, {
        use std::os::unix::fs::PermissionsExt;
        std::fs::Permissions::from_mode(0o500)
    })
    .unwrap();
    let result = Session::builder()
        .home(&home.0)
        .model("mock/test")
        .persist(true)
        .build()
        .await;
    std::fs::set_permissions(&sessions, {
        use std::os::unix::fs::PermissionsExt;
        std::fs::Permissions::from_mode(0o700)
    })
    .unwrap();
    let error = match result {
        Ok(_) => panic!("an unwritable sessions directory must fail the build"),
        Err(error) => error,
    };
    assert!(
        matches!(error, e_sdk::Error::Session(_)),
        "expected the persistence preflight, got: {error}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_persisted_session_is_listed_and_resumes_from_its_file() {
    let (port, server) = serve_sse(&[OK, OK]);
    let home = mock_home("persist", &[("mock", port)]);
    let ws = TempDir::new("persist-ws");
    let builder = || {
        Session::builder()
            .home(&home.0)
            .cwd(&ws.0)
            .model("mock/test")
    };

    let mut session = builder().persist(true).build().await.unwrap();
    session.prompt("first").await.unwrap();
    let path = session.path().expect("a persisted session has a file");
    assert!(path.starts_with(home.0.join("sessions")));
    drop(session);

    let saved = builder().saved();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].path, path);
    assert_eq!(saved[0].user_turns, 1);

    let mut resumed = builder().resume(&path).build().await.unwrap();
    assert_eq!(resumed.history().len(), 2);
    resumed.prompt("second").await.unwrap();
    assert_eq!(resumed.path(), Some(path.clone()));
    assert_eq!(e_sdk::transcript(&path).unwrap().len(), 4);

    let second = request_json(&server.join().unwrap()[1]);
    let texts = message_texts(&second);
    assert_eq!(&texts[1..], ["first", "ok", "second"]);
}

#[tokio::test(flavor = "multi_thread")]
async fn the_builder_refuses_what_cannot_work_before_any_request() {
    let home = mock_home("refuse", &[("mock", 1)]);
    let base = || Session::builder().home(&home.0).model("mock/test");

    let error = base()
        .tools(Tools::Only(vec!["read".into(), "teleport".into()]))
        .build()
        .await
        .unwrap_err();
    assert!(matches!(error, e_sdk::Error::UnknownTool(name) if name == "teleport"));

    let error = base().effort("max").build().await.unwrap_err();
    assert!(matches!(error, e_sdk::Error::Effort { .. }), "{error}");

    let error = Session::builder()
        .home(&home.0)
        .model("nobody/test")
        .build()
        .await
        .unwrap_err();
    assert!(
        matches!(error, e_sdk::Error::ModelUnavailable(_)),
        "{error}"
    );

    let empty = TempDir::new("empty-home");
    let error = Session::builder().home(&empty.0).build().await.unwrap_err();
    assert!(matches!(error, e_sdk::Error::NoProvider), "{error}");
}

/// Install an executable extension script in the home.
#[cfg(unix)]
fn install_extension(home: &TempDir, name: &str, script: &str) {
    use std::os::unix::fs::PermissionsExt;
    let dir = home.0.join("extensions");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, script).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// A protocol-speaking extension that records its `initialize` request in
/// its own working directory, then idles until e closes its stdin.
#[cfg(unix)]
const PROBE_EXTENSION: &str = r#"#!/bin/sh
read line
printf '%s
' "$line" > initialize.json
id=$(printf '%s' "$line" | sed -E 's/^\{"id":([0-9]+).*/\1/')
printf '{"id":%s,"result":{"name":"probe"}}
' "$id"
exec cat >/dev/null
"#;

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn extensions_run_in_the_session_workspace_not_the_process_directory() {
    let home = mock_home("ext-cwd", &[("mock", 1)]);
    install_extension(&home, "probe", PROBE_EXTENSION);
    let ws = TempDir::new("ext-cwd-ws");
    let session = Session::builder()
        .home(&home.0)
        .cwd(&ws.0)
        .model("mock/test")
        .extensions(true)
        .build()
        .await
        .unwrap();

    // The file landed in the workspace: the process ran there. Its content
    // says so too: `initialize` named the workspace, not the test's cwd.
    let recorded = std::fs::read_to_string(ws.0.join("initialize.json"))
        .expect("the extension must start in the session's cwd");
    let request: serde_json::Value = serde_json::from_str(&recorded).unwrap();
    assert_eq!(request["method"], "initialize");
    assert_eq!(request["params"]["cwd"], ws.0.display().to_string());
    session.close().await;
}

#[cfg(unix)]
#[tokio::test(flavor = "multi_thread")]
async fn broken_extensions_are_reported_before_the_first_turn_without_stalling_build() {
    let (port, _server) = serve_sse(&[OK]);
    let home = mock_home("ext-broken", &[("mock", port)]);
    install_extension(
        &home,
        "broken",
        "#!/bin/sh
exit 1
",
    );
    let mut session = tokio::time::timeout(
        Duration::from_secs(10),
        Session::builder()
            .home(&home.0)
            .model("mock/test")
            .extensions(true)
            .build(),
    )
    .await
    .expect("build must not wait on an unread notice")
    .unwrap();

    let mut turn = session.prompt("hi");
    let mut events = Vec::new();
    while let Some(event) = turn.next().await {
        events.push(event);
    }
    let reply = turn.finish().await.unwrap();
    assert_eq!(reply.text, "ok");
    assert!(
        matches!(&events[0], Event::Notice(notice) if notice.contains("broken")),
        "the startup diagnostic must lead the first turn, got: {events:?}"
    );
    session.close().await;
}

/// Two bash calls in one batch, one wave at a time: cancelling after the
/// first starts skips the second with no terminal event of its own. The
/// reply must still settle both — once each — and a late detached event
/// for the skipped turn must not leak into this one's stats.
const TWO_SLEEPS: &str = concat!(
    "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"c1\",",
    "\"function\":{\"name\":\"bash\",\"arguments\":\"{\\\"command\\\":\\\"sleep 5\\\"}\"}}]}}]}\n\n",
    "data: {\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":1,\"id\":\"c2\",",
    "\"function\":{\"name\":\"bash\",\"arguments\":\"{\\\"command\\\":\\\"sleep 5\\\"}\"}}]}}]}\n\n",
    "data: {\"choices\":[{\"finish_reason\":\"tool_calls\"}]}\n\n",
    "data: [DONE]\n\n",
);

#[tokio::test(flavor = "multi_thread")]
async fn a_cancelled_call_that_never_ran_still_counts_once_as_a_failure() {
    let (port, _server) = serve_sse(&[TWO_SLEEPS]);
    let home = mock_home("skip-count", &[("mock", port)]);
    std::fs::write(home.0.join("settings.json"), r#"{"tool_concurrency": 1}"#).unwrap();
    let ws = TempDir::new("skip-count-ws");
    let mut session = Session::builder()
        .home(&home.0)
        .cwd(&ws.0)
        .model("mock/test")
        .build()
        .await
        .unwrap();

    let mut turn = session.prompt("run both");
    let mut cancelled = false;
    while let Some(event) = turn.next().await {
        if matches!(event, Event::ToolStart { .. }) && !cancelled {
            turn.cancel();
            cancelled = true;
        }
    }
    assert!(cancelled, "the first call must have started");
    let reply = turn.finish().await.unwrap();
    assert_eq!(reply.stop, Stop::Cancelled);
    assert_eq!(
        (reply.tools.calls, reply.tools.failures),
        (2, 2),
        "every announced call settles exactly once"
    );
}

#[tokio::test]
async fn build_rejects_a_working_directory_that_is_not_one() {
    let home = mock_home("bad-cwd", &[("mock", 9)]);
    let file = home.0.join("a-file");
    std::fs::write(&file, "not a directory").unwrap();
    let error = Session::builder()
        .home(&home.0)
        .cwd(&file)
        .model("mock/test")
        .build()
        .await
        .expect_err("a file cannot be a workspace");
    assert!(
        matches!(error, e_sdk::Error::Cwd { .. }),
        "expected a cwd error, got: {error}"
    );
    // The message names the path and says what is wrong with it.
    let message = error.to_string();
    assert!(
        message.contains("a-file") && message.contains("not a directory"),
        "{message}"
    );
}

/// A session and its turns move between tasks: a server can build one per
/// request and drive it from wherever the request is handled.
#[test]
fn sessions_and_turns_are_send() {
    fn assert_send<T: Send>() {}
    assert_send::<Session>();
    assert_send::<e_sdk::Turn<'static>>();
}
