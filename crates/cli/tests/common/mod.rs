//! Shared fixtures for the integration tests.
#![allow(dead_code)] // each test crate takes a subset of the helpers
//!
//! Each `tests/*.rs` crate `mod common;`s this directory. `ULO_HOME` is
//! process-global, so anything that writes it takes `env_lock()` first —
//! including across `#[tokio::test]` awaits (each test has its own runtime).

use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::Mutex;
use std::thread::JoinHandle;

use ulo::core::providers::catalog::{Api, Model, Thinking};

static ENV_LOCK: Mutex<()> = Mutex::new(());

/// Hold this for the whole test whenever `ULO_HOME` or a provider `key_env`
/// must stay ours. A prior panic must not cascade as `PoisonError`.
pub fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    ENV_LOCK.lock().unwrap_or_else(|ulo| ulo.into_inner())
}

/// Registry env keys leak a developer's real sign-in into signed-out
/// assertions. Clear every declared `key_env` for this process.
pub fn clear_env_keys() {
    for provider in ulo::core::providers::registry::all() {
        if let Some(env) = &provider.auth.key_env {
            std::env::remove_var(env);
        }
    }
}

/// A unique `ULO_HOME`; dropping it restores the prior value and removes its files.
pub struct Home {
    pub dir: PathBuf,
    previous: Option<std::ffi::OsString>,
}

impl Home {
    pub fn new(label: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "ulo-{label}-{}-{}",
            std::process::id(),
            uuid::Uuid::now_v7()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let previous = std::env::var_os("ULO_HOME");
        std::env::set_var("ULO_HOME", &dir);
        Home { dir, previous }
    }

    /// Create executable extension fixtures in the same isolated home.
    pub fn with_extension(name: &str, body: &str) -> Self {
        Self::with_extensions(&[(name, body)])
    }

    pub fn with_extensions(entries: &[(&str, &str)]) -> Self {
        let home = Self::new("extensions");
        let extensions = home.dir.join("extensions");
        std::fs::create_dir_all(&extensions).unwrap();
        for (name, body) in entries {
            let path = extensions.join(name);
            std::fs::write(&path, body).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
            }
        }
        home
    }

    pub fn write(&self, name: &str, contents: impl AsRef<[u8]>) {
        std::fs::write(self.dir.join(name), contents).unwrap();
    }

    pub fn auth(&self, json: &str) {
        self.write("auth.json", json);
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => std::env::set_var("ULO_HOME", value),
            None => std::env::remove_var("ULO_HOME"),
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// HTTP response wrapping an SSE body.
pub fn sse_response(body: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
        body.len(),
        body
    )
}

/// Accept one connection per response, capture each request, write the
/// canned reply. The join handle yields the captured requests in order.
pub fn serve_raw(responses: Vec<String>) -> (u16, JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = std::thread::spawn(move || {
        let mut captured = Vec::new();
        for response in responses {
            let (mut sock, _) = listener.accept().unwrap();
            let mut buf = vec![0u8; 262144];
            let n = sock.read(&mut buf).unwrap();
            captured.push(String::from_utf8_lossy(&buf[..n]).into_owned());
            sock.write_all(response.as_bytes()).unwrap();
        }
        captured
    });
    (port, handle)
}

/// `serve_raw` for responses that are not text — a release tarball.
pub fn serve_raw_bytes(responses: Vec<Vec<u8>>) -> (u16, JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let handle = std::thread::spawn(move || {
        let mut captured = Vec::new();
        for response in responses {
            let (mut sock, _) = listener.accept().unwrap();
            let mut buf = vec![0u8; 262144];
            let n = sock.read(&mut buf).unwrap();
            captured.push(String::from_utf8_lossy(&buf[..n]).into_owned());
            sock.write_all(&response).unwrap();
        }
        captured
    });
    (port, handle)
}

pub fn serve_sse(bodies: &[&str]) -> (u16, JoinHandle<Vec<String>>) {
    serve_raw(bodies.iter().copied().map(sse_response).collect())
}

pub fn test_model(provider: &str, port: u16, api: Api) -> Model {
    Model {
        provider: provider.into(),
        id: "test".into(),
        base_url: format!("http://127.0.0.1:{port}"),
        api,
        catalog: ulo::core::providers::registry::CatalogStrategy::Openai,
        responses_mount: ulo::core::providers::registry::ResponsesMount::Platform,
        provider_supports_tools: true,
        provider_image_input: false,
        effort: vec!["low".into(), "medium".into(), "high".into()],
        thinking: Thinking::Manual,
        context_window: 200_000,
        max_output: None,
        supports_tools: true,
        image_input: false,
        pricing: None,
    }
}

/// The OpenAI-shaped tool schema the dialects convert (or pass through).
pub fn read_tool() -> serde_json::Value {
    serde_json::json!({
        "type": "function",
        "function": {
            "name": "read",
            "description": "read a file",
            "parameters": {"type": "object", "properties": {}}
        }
    })
}

pub fn request_json(sent: &str) -> serde_json::Value {
    serde_json::from_str(sent.split("\r\n\r\n").nth(1).expect("request has a body"))
        .expect("request body is JSON")
}

/// Decode reviewed response recordings; `serve_sse` still owns the loopback transport.
pub fn provider_recording(json: &str) -> Vec<String> {
    let data: serde_json::Value = serde_json::from_str(json).unwrap();
    data["responses"]
        .as_array()
        .unwrap()
        .iter()
        .map(|body| body.as_str().unwrap().to_owned())
        .collect()
}
