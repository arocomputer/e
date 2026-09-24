//! Spawn extension subprocesses and bound JSONL reads, requests, and replies.
//!
//! [`spawn`] reads as the whole story: start the child, relay its stderr,
//! register its [`Link`], start the stdin writer and the stdout router, then
//! run the `initialize` handshake.

use std::sync::atomic::AtomicUsize;
use std::sync::OnceLock;

use tokio::process::{ChildStderr, ChildStdin, ChildStdout};

use super::*;

/// Start one extension and complete its handshake. The child is registered
/// in `startup_registry` before `initialize` is sent, so a cancelled startup
/// can still find and kill it; a failed handshake reaps it before returning.
pub(super) async fn spawn(
    path: &PathBuf,
    cwd: &Path,
    notices: mpsc::Sender<String>,
    startup_registry: Option<&Arc<Mutex<Vec<Arc<Link>>>>>,
    requests: Option<mpsc::Sender<HostRequest>>,
) -> Result<Extension, String> {
    let mut child = tokio::process::Command::new(path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .current_dir(cwd)
        // Every exit path reaps through `Link::reap`; this is the backstop
        // for a child that outlives the reap timeout and lands in tokio's
        // orphan queue instead.
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("failed to start: {e}"))?;

    let stdin = child.stdin.take().ok_or("no stdin")?;
    let stdout = child.stdout.take().ok_or("no stdout")?;
    let stderr = child.stderr.take();
    let source = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "extension".into());
    if let Some(stderr) = stderr {
        tokio::spawn(relay_stderr(stderr, notices.clone(), source.clone()));
    }

    let link = Arc::new(Link {
        alive: AtomicBool::new(true),
        pending: Mutex::new(HashMap::new()),
        progress: Mutex::new(HashMap::new()),
        child: tokio::sync::Mutex::new(Some(child)),
        exit_notice: Mutex::new((None, false)),
        notices,
    });
    // Register before the handshake begins. If the startup future is
    // cancelled while we await `initialize`, the guard still finds this
    // child and kills it.
    if let Some(registry) = startup_registry {
        registry
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(link.clone());
    }

    let (writer, writer_rx) = mpsc::channel::<String>(64);
    tokio::spawn(write_stdin(stdin, writer_rx, link.clone()));
    let name: Arc<OnceLock<String>> = Arc::new(OnceLock::new());
    let ui = requests.is_some();
    let router = Router {
        name: name.clone(),
        requests,
        inflight: Arc::new(AtomicUsize::new(0)),
        writer: writer.clone(),
        early: Vec::new(),
    };
    tokio::spawn(read_stdout(stdout, link.clone(), router));

    let ext = Extension {
        manifest: Manifest::default(),
        writer,
        link: link.clone(),
    };
    let manifest = match handshake(&ext, cwd, ui).await {
        Ok(manifest) => manifest,
        Err(reason) => {
            link.reap().await;
            return Err(reason);
        }
    };
    link.install_exit_notice(exit_notice(&source, &manifest));
    let _ = name.set(manifest.name.clone());
    Ok(Extension { manifest, ..ext })
}

/// Report each stderr line as a notice from `source` until the pipe closes.
///
/// stderr is diagnostics, not protocol: an over-long line is reported
/// and its remainder dropped, then reading goes on. The pipe must
/// stay open as long as the child lives — closing it would turn the
/// child's next stderr write into SIGPIPE/EPIPE and kill it.
async fn relay_stderr(stderr: ChildStderr, notices: mpsc::Sender<String>, source: String) {
    let mut reader = BufReader::new(stderr);
    loop {
        match read_bounded_line(&mut reader, MAX_EXTENSION_LINE_BYTES).await {
            Ok(Some(line)) if !line.trim().is_empty() => {
                let _ = notices.try_send(format!("extension {source}: {line}"));
            }
            Ok(Some(_)) => {}
            Ok(None) => break,
            Err(error) => {
                let _ = notices.try_send(format!("extension {source}: {error}"));
                // Only the byte cap leaves the stream mid-line; a
                // UTF-8 failure has already consumed its line, and
                // resyncing there would eat the next one too.
                let mid_line = error
                    .get_ref()
                    .is_none_or(|inner| !inner.is::<std::string::FromUtf8Error>());
                if mid_line && discard_line(&mut reader).await.is_err() {
                    break;
                }
            }
        }
    }
}

/// Serialize outgoing lines to the child's stdin, one per message. A failed
/// write means the extension is gone.
async fn write_stdin(mut stdin: ChildStdin, mut lines: mpsc::Receiver<String>, link: Arc<Link>) {
    while let Some(line) = lines.recv().await {
        let written: std::io::Result<()> = async {
            stdin.write_all(line.as_bytes()).await?;
            stdin.write_all(b"\n").await?;
            stdin.flush().await
        }
        .await;
        if written.is_err() {
            link.exited();
            break;
        }
    }
}

/// Route every stdout line: responses to pending waiters, notifies to the
/// app, progress to the running tool, and the extension's own requests to
/// the surface owner.
async fn read_stdout(stdout: ChildStdout, link: Arc<Link>, mut router: Router) {
    let mut reader = BufReader::new(stdout);
    while let Ok(Some(line)) = read_bounded_line(&mut reader, MAX_EXTENSION_LINE_BYTES).await {
        match protocol::parse_incoming(&line) {
            Some(Incoming::Request { id, method, params }) => router.request(id, method, params),
            Some(Incoming::Response { id, result }) => router.response(&link, id, result),
            Some(Incoming::Notify { message }) => {
                // Notices are best-effort UI output. A full transcript
                // channel must never hold up response dispatch.
                let _ = link.notices.try_send(message);
            }
            Some(Incoming::ToolUpdate { id, stream, chunk }) => {
                let target = link
                    .progress
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .get(&id)
                    .cloned();
                if let Some(tx) = target {
                    // Backpressure keeps progress ordered ahead of the
                    // response line that follows it on extension stdout.
                    let _ = tx.send(ToolProgress { stream, chunk }).await;
                }
            }
            None => {}
        }
    }
    // stdout is done: the process exited, closed its end, or broke the
    // protocol past the line cap. It is finished either way — fail the
    // waiters, say so, and reap it.
    link.exited();
    link.reap().await;
}

/// Send `initialize` and accept the manifest it answers with. `ui` says
/// whether `ui.*` requests can reach someone; false under `e -p`.
async fn handshake(ext: &Extension, cwd: &Path, ui: bool) -> Result<Manifest, String> {
    let host_shim = ExtensionHost {
        extensions: Vec::new(),
        ids: AtomicU64::new(1_000_000),
    };
    let init = json!({
        "protocol": protocol::PROTOCOL_VERSION,
        "capabilities": protocol::CAPABILITIES,
        "ui": ui,
        "e_version": crate::VERSION,
        "cwd": cwd.display().to_string(),
        // Namespaced extension config from ~/.e/settings.json:
        // {"extensions":{"<name>":{…}}} — each extension reads its own key.
        "extensions_config": crate::config::settings::extensions_config(),
    });
    let value = host_shim
        .request(ext, "initialize", init, INIT_TIMEOUT)
        .await
        .map_err(|e| format!("initialize {e}"))?;
    let manifest: Manifest =
        serde_json::from_value(value).map_err(|e| format!("bad manifest: {e}"))?;
    if manifest.name.is_empty() {
        return Err("manifest has no name".into());
    }
    Ok(manifest)
}

/// The stdout reader's state for the extension's own requests: where they
/// go, how many are unanswered, and the writer their replies go back down.
struct Router {
    /// The manifest name, learned from the manifest response so the
    /// requests an extension sends right behind its manifest carry it.
    name: Arc<OnceLock<String>>,
    requests: Option<mpsc::Sender<HostRequest>>,
    inflight: Arc<AtomicUsize>,
    writer: mpsc::Sender<String>,
    /// Requests sent on the heels of the manifest, before its name is known
    /// here: held, in order, until the manifest response names the extension.
    early: Vec<(Value, String, Value)>,
}

impl Router {
    /// An extension's own request: forwarded, or held while it is still
    /// unnamed.
    fn request(&mut self, id: Value, method: String, params: Value) {
        if self.name.get().is_none() {
            self.early.push((id, method, params));
            return;
        }
        self.forward(id, method, params);
    }

    /// A response to one of the host's requests: wake its waiter.
    fn response(&mut self, link: &Link, id: u64, result: Result<Value, String>) {
        // The first response an extension ever sends is its
        // manifest; learning the name here (rather than after
        // the handshake task parses it) lets requests sent
        // right behind the manifest carry the right name.
        if self.name.get().is_none() {
            if let Some(name) = result
                .as_ref()
                .ok()
                .and_then(|v| v.get("name"))
                .and_then(Value::as_str)
                .filter(|n| !n.is_empty())
            {
                let _ = self.name.set(name.to_string());
            }
        }
        if let Some(tx) = link
            .pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&id)
        {
            let _ = tx.send(result);
        }
        if self.name.get().is_some() {
            for (id, method, params) in std::mem::take(&mut self.early) {
                self.forward(id, method, params);
            }
        }
    }

    /// Hand one extension request to the surface owner, or answer it at once
    /// when there is none, the owner is gone, or the extension already has
    /// [`MAX_INFLIGHT_REQUESTS`] unanswered. The reply, whenever it comes,
    /// goes back down the extension's stdin with the extension's own id.
    fn forward(&self, id: Value, method: String, params: Value) {
        let Some(requests) = &self.requests else {
            self.refuse(&id, "no ui".into());
            return;
        };
        if self.inflight.load(Ordering::SeqCst) >= MAX_INFLIGHT_REQUESTS {
            self.refuse(
                &id,
                format!("too many requests in flight (limit {MAX_INFLIGHT_REQUESTS})"),
            );
            return;
        }
        let (tx, rx) = oneshot::channel();
        let request = HostRequest {
            extension: self.name.get().cloned().unwrap_or_default(),
            method,
            params,
            reply: Some(tx),
        };
        if requests.try_send(request).is_err() {
            self.refuse(&id, "ui unavailable".into());
            return;
        }
        self.inflight.fetch_add(1, Ordering::SeqCst);
        let inflight = self.inflight.clone();
        let writer = self.writer.clone();
        tokio::spawn(async move {
            let result = rx.await.unwrap_or_else(|_| Err("request dropped".into()));
            inflight.fetch_sub(1, Ordering::SeqCst);
            let _ = writer.send(answer(&id, result)).await;
        });
    }

    /// An immediate error is still a reply the extension waits for: it
    /// queues for the writer like any other rather than being dropped when
    /// the channel is momentarily full.
    fn refuse(&self, id: &Value, error: String) {
        let line = answer(id, Err(error));
        let writer = self.writer.clone();
        tokio::spawn(async move {
            let _ = writer.send(line).await;
        });
    }
}

/// The reply line for an extension's request `id`.
fn answer(id: &Value, result: Result<Value, String>) -> String {
    match result {
        Ok(value) => json!({"id": id, "result": value}),
        Err(error) => json!({"id": id, "error": error}),
    }
    .to_string()
}

/// The transcript line for an extension that dies mid-session. A guard
/// (`tool_call`/`input` hooks) that is gone deserves a louder line: the
/// hooks fail open, so its protection has silently lapsed.
pub(super) fn exit_notice(source: &str, manifest: &Manifest) -> String {
    let guards: Vec<&str> = manifest
        .hooks
        .iter()
        .map(String::as_str)
        .filter(|hook| matches!(*hook, "tool_call" | "input"))
        .collect();
    if guards.is_empty() {
        format!("extension {source}: exited")
    } else {
        format!(
            "extension {source}: exited — its {} hook no longer applies",
            guards.join(" and ")
        )
    }
}

/// Read one line without allowing an unbounded peer to grow memory
/// indefinitely — a misbehaving extension on the other flavor of this call,
/// or (via the `pub` re-export) an RPC client sending an unterminated or
/// giant line. The newline is consumed but not returned.
///
/// Errors the instant the cap is crossed, before a newline is even in
/// view — deliberately, not just as a memory bound: a still-growing line
/// with no newline yet (a firehose, or a client that never terminates one)
/// must be cut off promptly rather than read forever looking for a
/// newline that may never come. The stream is left mid-line, never
/// resynced: protocol readers treat the error as fatal to their loop, and
/// the one diagnostics reader (extension stderr) resyncs itself with
/// `discard_line`.
pub async fn read_bounded_line<R>(
    reader: &mut R,
    max_bytes: usize,
) -> std::io::Result<Option<String>>
where
    R: AsyncBufRead + Unpin,
{
    let mut line = Vec::new();
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            if line.is_empty() {
                return Ok(None);
            }
            break;
        }

        if let Some(newline) = available.iter().position(|byte| *byte == b'\n') {
            if line.len().saturating_add(newline) > max_bytes {
                // Leave the newline in place so the stream is mid-line
                // here exactly as in the no-newline-yet case below.
                reader.consume(newline);
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("line exceeded {max_bytes} bytes"),
                ));
            }
            line.extend_from_slice(&available[..newline]);
            reader.consume(newline + 1);
            break;
        }

        let count = available.len();
        if line.len().saturating_add(count) > max_bytes {
            reader.consume(count);
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("line exceeded {max_bytes} bytes"),
            ));
        }
        line.extend_from_slice(available);
        reader.consume(count);
    }

    if line.last() == Some(&b'\r') {
        line.pop();
    }
    String::from_utf8(line)
        .map(Some)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
}

/// Skip to just past the next newline (or EOF): the resync after
/// `read_bounded_line` hits its cap on a stream where dropping the rest of
/// the line is the right call. Reads and discards without buffering, so a
/// firehose costs nothing but time.
async fn discard_line<R>(reader: &mut R) -> std::io::Result<()>
where
    R: AsyncBufRead + Unpin,
{
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            return Ok(());
        }
        match available.iter().position(|byte| *byte == b'\n') {
            Some(newline) => {
                reader.consume(newline + 1);
                return Ok(());
            }
            None => {
                let count = available.len();
                reader.consume(count);
            }
        }
    }
}
