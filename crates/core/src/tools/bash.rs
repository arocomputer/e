//! The bash tool: spawn a shell command and stream its captured pipes.
//!
//! This remains spawn-and-capture, not a terminal daemon. A wall-clock timeout
//! or turn cancellation kills the command's process group. Pipe readers feed
//! one tagged queue so display and retained output keep observed ordering.
//!
//! A background process outlives its starting call and belongs to the agent's
//! registry. The model can check or kill its handle in later turns. Dropping
//! that agent stops its background processes. Everything else
//! about it — the process group, the 32KB retained tail, ANSI/carriage-return
//! cleanup — matches the foreground path; only the waiting is removed.

use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
#[cfg(unix)]
use std::os::fd::AsRawFd;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::{schema_object, OutputStream, ToolOutcome, ToolOutput};

pub fn schema() -> Value {
    schema_object(
        "bash",
        "Run a shell command in the workspace root and return its combined output. Each call is a fresh shell: cd, environment variables, and (unless started with `background: true`) background processes do not persist between calls. Output keeps the most recent 32KB when longer.\n\nFor something long-lived (a dev server, a watcher) that would otherwise block the turn: pass `background: true` to start it detached and get a `handle` back immediately, instead of waiting for it to exit. Check on it, or read more of its output, with a later call passing `handle` and no `command`; add `signal: \"kill\"` to stop it. A background process outlives the turn that started it but not its owning agent — nothing persists across a restart.",
        json!({
            "command": {"type": "string", "description": "The command to run. Omit when checking or killing a background process by `handle`."},
            "timeout": {"type": "integer", "description": "Seconds before the command is killed (default 120). Ignored when starting a background process — it runs until it exits or is killed."},
            "background": {"type": "boolean", "description": "Start `command` detached and return immediately with a `handle`, instead of waiting for it to finish."},
            "handle": {"type": "string", "description": "A background process's handle, from a prior background start. Returns its status and output so far; combine with `signal: \"kill\"` to stop it."},
            "signal": {"type": "string", "enum": ["kill"], "description": "Send with `handle` to kill that background process."}
        }),
        &[],
    )
}

/// Bytes kept per background process, tail-retained like the foreground
/// path's own cap — a runaway server logging forever must not grow forever.
const BACKGROUND_RETAIN_LIMIT: usize = 32 * 1024;
/// Finished jobs remain queryable briefly, but an autonomous session must not
/// retain every completed process forever.
const BACKGROUND_FINISHED_RETAIN: usize = 64;
const BACKGROUND_PROCESS_LIMIT: usize = 128;
static BACKGROUND_FINISHED_SEQUENCE: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy)]
enum ExitOutcome {
    Exited(i32),
    Killed,
}

struct BackgroundProcess {
    pid: u32,
    command: String,
    output: Mutex<Vec<u8>>,
    total_bytes: Mutex<usize>,
    exit: Mutex<Option<ExitOutcome>>,
    finished_sequence: AtomicU64,
}

/// Background handles belong to one agent and are killed when it is dropped.
#[derive(Default)]
pub(super) struct BackgroundRegistry {
    jobs: Mutex<HashMap<String, Arc<BackgroundProcess>>>,
}

impl Drop for BackgroundRegistry {
    fn drop(&mut self) {
        for process in self
            .jobs
            .get_mut()
            .unwrap_or_else(|error| error.into_inner())
            .values()
        {
            let exit = lock(&process.exit);
            if exit.is_none() {
                kill_group(process.pid);
            }
        }
    }
}

/// Shells run in their own process groups. Track each live leader so process
/// shutdown can kill the groups before the runtime exits.
static PROCESS_GROUPS: Mutex<Option<HashSet<u32>>> = Mutex::new(None);

fn track_group(pid: u32) {
    lock(&PROCESS_GROUPS)
        .get_or_insert_with(HashSet::new)
        .insert(pid);
}

fn untrack_group(pid: u32) {
    if let Some(groups) = lock(&PROCESS_GROUPS).as_mut() {
        groups.remove(&pid);
    }
}

/// Kill every live shell process group before the owning e process exits.
pub fn kill_tracked_processes() {
    let mut groups = lock(&PROCESS_GROUPS);
    for pid in groups.take().unwrap_or_default() {
        kill_group(pid);
    }
}

struct TrackedGroup(u32);

impl TrackedGroup {
    fn new(pid: u32) -> Self {
        track_group(pid);
        Self(pid)
    }
}

impl Drop for TrackedGroup {
    fn drop(&mut self) {
        untrack_group(self.0);
    }
}

fn prune_background(map: &mut HashMap<String, Arc<BackgroundProcess>>) {
    let mut finished: Vec<(String, u64)> = map
        .iter()
        .filter_map(|(id, process)| {
            let sequence = process.finished_sequence.load(Ordering::Relaxed);
            (sequence > 0).then(|| (id.clone(), sequence))
        })
        .collect();
    finished.sort_by_key(|(_, sequence)| *sequence);
    let excess = finished.len().saturating_sub(BACKGROUND_FINISHED_RETAIN);
    for (id, _) in finished.into_iter().take(excess) {
        map.remove(&id);
    }
}

fn register_background(
    registry: &BackgroundRegistry,
    id: String,
    process: Arc<BackgroundProcess>,
) -> bool {
    let mut guard = lock(&registry.jobs);
    let map = &mut *guard;
    prune_background(map);
    if map.len() >= BACKGROUND_PROCESS_LIMIT {
        return false;
    }
    map.insert(id, process);
    true
}

fn find_background(registry: &BackgroundRegistry, id: &str) -> Option<Arc<BackgroundProcess>> {
    lock(&registry.jobs).get(id).cloned()
}

/// A login shell running `command` in `cwd` with its pipes captured. It
/// leads a new session, so its process group is exactly the command and its
/// descendants, and one signal to the group reaches all of them.
fn shell(command: &str, cwd: &Path) -> Command {
    let mut cmd = Command::new("bash");
    cmd.arg("-lc")
        .arg(command)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    unsafe {
        cmd.pre_exec(|| {
            if libc::setsid() < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    cmd
}

/// Start `command` detached and return immediately with a handle. Output
/// keeps accumulating (capped) in the background; nothing here blocks the
/// calling turn.
fn start_background(command: &str, cwd: &Path, registry: &Arc<BackgroundRegistry>) -> ToolOutput {
    let mut child = match shell(command, cwd).spawn() {
        Ok(child) => child,
        Err(error) => return failure(&format!("bash: {error}")),
    };
    let pid = child.id();
    track_group(pid);
    let id = uuid::Uuid::new_v4().to_string();
    let process = Arc::new(BackgroundProcess {
        pid,
        command: command.to_string(),
        output: Mutex::new(Vec::new()),
        total_bytes: Mutex::new(0),
        exit: Mutex::new(None),
        finished_sequence: AtomicU64::new(0),
    });
    if !register_background(registry, id.clone(), process.clone()) {
        kill_group(pid);
        untrack_group(pid);
        let _ = child.wait();
        return failure("bash: background process limit reached; check or stop existing handles");
    }

    let stdout_thread = child.stdout.take().map(|pipe| {
        let process = process.clone();
        std::thread::spawn(move || drain_into_background(pipe, process))
    });
    let stderr_thread = child.stderr.take().map(|pipe| {
        let process = process.clone();
        std::thread::spawn(move || drain_into_background(pipe, process))
    });
    let registry = Arc::downgrade(registry);
    std::thread::spawn(move || {
        reap_background(child, process, stdout_thread, stderr_thread, registry)
    });

    ToolOutput {
        content: format!("started background process {id} (pid {pid}): {command}"),
        outcome: ToolOutcome::Completed,
        summary: format!("background {id}"),
        display: None,
    }
}

/// Drain one pipe into the process's capped, tail-retained buffer. No live
/// callback here — background output is read on demand, not streamed.
fn drain_into_background<R: std::io::Read>(mut pipe: R, process: Arc<BackgroundProcess>) {
    let mut buf = [0u8; 4096];
    loop {
        match pipe.read(&mut buf) {
            Ok(0) => break,
            Ok(count) => {
                let mut output = lock(&process.output);
                output.extend_from_slice(&buf[..count]);
                if output.len() > BACKGROUND_RETAIN_LIMIT {
                    keep_tail(&mut output, BACKGROUND_RETAIN_LIMIT);
                }
                drop(output);
                *lock(&process.total_bytes) += count;
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
}

/// Keep the leader unreaped while descendants hold pipes, reserving its PID.
/// Reaping and retiring the kill handle share locks with both shutdown paths.
fn reap_background(
    mut child: Child,
    process: Arc<BackgroundProcess>,
    stdout_thread: Option<std::thread::JoinHandle<()>>,
    stderr_thread: Option<std::thread::JoinHandle<()>>,
    registry: std::sync::Weak<BackgroundRegistry>,
) {
    if let Some(t) = stdout_thread {
        let _ = t.join();
    }
    if let Some(t) = stderr_thread {
        let _ = t.join();
    }
    let (status, mut exit) = loop {
        let exit = lock(&process.exit);
        let mut groups = lock(&PROCESS_GROUPS);
        let status = match child.try_wait() {
            Ok(None) => {
                drop(groups);
                drop(exit);
                std::thread::sleep(Duration::from_millis(10));
                continue;
            }
            Ok(Some(status)) => Ok(status),
            Err(error) => Err(error),
        };
        if let Some(groups) = groups.as_mut() {
            groups.remove(&process.pid);
        }
        break (status, exit);
    };
    *exit = Some(exit_outcome(status));
    drop(exit);
    process.finished_sequence.store(
        BACKGROUND_FINISHED_SEQUENCE.fetch_add(1, Ordering::Relaxed),
        Ordering::Relaxed,
    );
    if let Some(registry) = registry.upgrade() {
        prune_background(&mut lock(&registry.jobs));
    }
}

/// How a reaped background leader ended: death by a signal reads as killed,
/// and a status that could not be read as an unknown failure.
fn exit_outcome(status: std::io::Result<ExitStatus>) -> ExitOutcome {
    match status {
        Ok(status) => {
            #[cfg(unix)]
            {
                use std::os::unix::process::ExitStatusExt;
                match status.signal() {
                    Some(_) => ExitOutcome::Killed,
                    None => ExitOutcome::Exited(status.code().unwrap_or(-1)),
                }
            }
            #[cfg(not(unix))]
            {
                ExitOutcome::Exited(status.code().unwrap_or(-1))
            }
        }
        Err(_) => ExitOutcome::Exited(-1),
    }
}

/// Check on, read more from, or kill a background process by handle.
fn query_background(registry: &BackgroundRegistry, id: &str, kill: bool) -> ToolOutput {
    let Some(process) = find_background(registry, id) else {
        return failure(&format!("bash: no background process with handle {id}"));
    };
    if kill {
        kill_background(&process);
    }
    let retained = lock(&process.output).clone();
    let total_bytes = *lock(&process.total_bytes);
    let exit = *lock(&process.exit);
    let mut content = model_text(&retained);
    if total_bytes > retained.len() {
        content = earlier_dropped(total_bytes, retained.len(), &content);
    }
    let (outcome, summary, status_row) = match exit {
        None => (
            ToolOutcome::Completed,
            format!("running (pid {})", process.pid),
            format!(
                "[still running — pid {}, command: {}]",
                process.pid, process.command
            ),
        ),
        Some(ExitOutcome::Exited(0)) => (
            ToolOutcome::Completed,
            "exited 0".to_string(),
            "[exited 0]".to_string(),
        ),
        Some(ExitOutcome::Exited(code)) => (
            ToolOutcome::Failed,
            format!("exited {code}"),
            format!("[exited {code}]"),
        ),
        Some(ExitOutcome::Killed) => (
            ToolOutcome::Cancelled,
            "killed".to_string(),
            "[killed]".to_string(),
        ),
    };
    push_row(&mut content, &status_row);
    ToolOutput {
        content,
        outcome,
        summary,
        display: None,
    }
}

/// Kill a still-running background process and give the reaper a brief
/// window to record the exit. A finished one is left alone: its pid may
/// already belong to someone else.
fn kill_background(process: &BackgroundProcess) {
    // Hold the exit lock through signaling so the reaper cannot release and
    // reuse the PID between the liveness check and kill.
    let signalled = {
        let exit = lock(&process.exit);
        if exit.is_none() {
            kill_group(process.pid);
            true
        } else {
            false
        }
    };
    if signalled {
        let deadline = Instant::now() + Duration::from_millis(500);
        while lock(&process.exit).is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

/// Kill a process group whose child is its group leader. The clipboard
/// reader uses it too, for the same reason: descendants holding a pipe.
pub fn kill_group(pid: u32) {
    #[cfg(unix)]
    unsafe {
        // The child creates this process group in `pre_exec`. ESRCH simply
        // means every member has already exited; falling back to the positive
        // pid after wait risks signaling a newly reused pid.
        let _ = libc::kill(-(pid as i32), libc::SIGKILL);
    }
    #[cfg(not(unix))]
    if let Ok(mut child) = Command::new("kill").arg("-9").arg(pid.to_string()).spawn() {
        let _ = child.wait();
    }
}

/// Compatibility entry point for non-streaming callers.
pub fn run(args: &Value, cwd: &Path, state: &super::ToolRuntime) -> ToolOutput {
    run_streaming(args, cwd, state, &AtomicBool::new(false), |_, _| {})
}

/// Run bash and publish decoded stdout/stderr chunks while the process lives.
/// A `handle` checks a background process instead, and `background` starts
/// one; otherwise the command runs in the foreground until it exits, times
/// out, or `cancel` is set.
pub fn run_streaming<F>(
    args: &Value,
    cwd: &Path,
    state: &super::ToolRuntime,
    cancel: &AtomicBool,
    mut on_output: F,
) -> ToolOutput
where
    F: FnMut(OutputStream, &str),
{
    if let Some(handle) = args["handle"].as_str() {
        return query_background(
            &state.background,
            handle,
            args["signal"].as_str() == Some("kill"),
        );
    }
    let Some(command) = args["command"].as_str() else {
        return failure("bash: missing command (or a background `handle` to check)");
    };
    // Lenient models send `"true"` for a boolean; a dev server run in the
    // foreground blocks for the whole timeout, so the string counts too.
    let background = match &args["background"] {
        serde_json::Value::Bool(flag) => *flag,
        serde_json::Value::String(text) => text.trim().eq_ignore_ascii_case("true"),
        _ => false,
    };
    if background {
        return start_background(command, cwd, &state.background);
    }
    let timeout = match super::integer_arg(args, "timeout") {
        Ok(timeout) => timeout.unwrap_or(120).clamp(1, 600),
        Err(message) => return failure(&format!("bash: {message}")),
    };

    let mut child = match shell(command, cwd).spawn() {
        Ok(child) => child,
        Err(error) => return failure(&format!("bash: {error}")),
    };
    let _tracked_group = TrackedGroup::new(child.id());
    let pipes = Pipes::open(&mut child);
    let mut capture = Capture::default();
    let (status, ending) = match wait_for_exit(
        &mut child,
        &pipes,
        &mut capture,
        timeout,
        cancel,
        &mut on_output,
    ) {
        Ok(exited) => exited,
        Err(error) => return failure(&format!("bash: {error}")),
    };
    pipes.close(child.id(), &mut capture, &mut on_output);
    capture.flush_carries(&mut on_output);
    let content = capture.model_copy(state);
    verdict(content, ending, status.code(), timeout)
}

/// Why a foreground command stopped. Cancellation outranks a timeout that
/// fired first, since the user asked for it.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Ending {
    Exited,
    TimedOut,
    Cancelled,
}

/// Poll the command until it exits, publishing its output as it arrives and
/// killing its group on cancellation or at the deadline. Returns its exit
/// status and why it stopped.
fn wait_for_exit<F>(
    child: &mut Child,
    pipes: &Pipes,
    capture: &mut Capture,
    timeout: u64,
    cancel: &AtomicBool,
    on_output: &mut F,
) -> std::io::Result<(ExitStatus, Ending)>
where
    F: FnMut(OutputStream, &str),
{
    let deadline = Instant::now() + Duration::from_secs(timeout);
    let mut ending = Ending::Exited;
    loop {
        capture.drain_ready(&pipes.rx, on_output);
        if cancel.load(Ordering::SeqCst) {
            ending = Ending::Cancelled;
            kill_group(child.id());
        } else if Instant::now() >= deadline {
            if ending != Ending::Cancelled {
                ending = Ending::TimedOut;
            }
            kill_group(child.id());
        }
        if let Some(status) = child.try_wait()? {
            return Ok((status, ending));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// The foreground tool result: the model's copy, a marker when the command
/// was killed, and the outcome the row shows.
fn verdict(
    mut content: String,
    ending: Ending,
    exit_code: Option<i32>,
    timeout: u64,
) -> ToolOutput {
    let (outcome, summary) = match ending {
        Ending::Cancelled => {
            push_row(&mut content, "… [cancelled]");
            (ToolOutcome::Cancelled, "cancelled".to_string())
        }
        Ending::TimedOut => {
            push_row(
                &mut content,
                &format!("… [killed: exceeded the {timeout}s timeout]"),
            );
            (ToolOutcome::TimedOut, format!("timeout {timeout}s"))
        }
        Ending::Exited if exit_code == Some(0) => (ToolOutcome::Completed, "done".to_string()),
        Ending::Exited => (
            ToolOutcome::Failed,
            format!("exit {}", exit_code.unwrap_or(-1)),
        ),
    };
    ToolOutput {
        content,
        outcome,
        summary,
        display: None,
    }
}

/// Append `row` on its own line, with no blank line before it when `text`
/// is empty.
fn push_row(text: &mut String, row: &str) {
    if !text.is_empty() {
        text.push('\n');
    }
    text.push_str(row);
}

/// The reader threads that drain a foreground command's pipes into one
/// tagged queue, so display and retained output keep observed ordering.
struct Pipes {
    rx: mpsc::Receiver<(OutputStream, Vec<u8>)>,
    readers: Vec<std::thread::JoinHandle<()>>,
    stop: Arc<AtomicBool>,
}

impl Pipes {
    /// Take the child's stdout and stderr and start a reader for each.
    fn open(child: &mut Child) -> Self {
        let (tx, rx) = mpsc::sync_channel::<(OutputStream, Vec<u8>)>(64);
        let stop = Arc::new(AtomicBool::new(false));
        let mut readers = Vec::new();
        if let Some(stdout) = child.stdout.take() {
            readers.push(spawn_reader(
                stdout,
                OutputStream::Stdout,
                tx.clone(),
                stop.clone(),
            ));
        }
        if let Some(stderr) = child.stderr.take() {
            readers.push(spawn_reader(
                stderr,
                OutputStream::Stderr,
                tx.clone(),
                stop.clone(),
            ));
        }
        Self { rx, readers, stop }
    }

    /// After the shell exited, kill its group and collect what the readers
    /// still deliver.
    ///
    /// A shell can exit while a background descendant still owns its pipe.
    /// Background processes are outside this tool's contract, so close the
    /// group on natural exit too. Give readers a brief chance to observe EOF,
    /// then ask nonblocking readers to stop; never join a thread that is still
    /// stuck behind a setsid'd descendant which escaped the group.
    fn close<F>(self, pid: u32, capture: &mut Capture, on_output: &mut F)
    where
        F: FnMut(OutputStream, &str),
    {
        kill_group(pid);
        let drain_deadline = Instant::now() + Duration::from_millis(100);
        while self.running() && Instant::now() < drain_deadline {
            capture.drain_ready(&self.rx, on_output);
            std::thread::sleep(Duration::from_millis(5));
        }
        self.stop.store(true, Ordering::SeqCst);
        let stop_deadline = Instant::now() + Duration::from_millis(100);
        while self.running() && Instant::now() < stop_deadline {
            std::thread::sleep(Duration::from_millis(5));
        }
        for reader in self.readers {
            if reader.is_finished() {
                let _ = reader.join();
            }
        }
        while let Ok((stream, bytes)) = self.rx.try_recv() {
            capture.publish(stream, &bytes, on_output);
        }
    }

    /// Some reader has not finished yet.
    fn running(&self) -> bool {
        self.readers.iter().any(|reader| !reader.is_finished())
    }
}

/// Drain one process pipe and tag every chunk before joining the shared queue.
#[cfg(unix)]
fn spawn_reader<R>(
    pipe: R,
    stream: OutputStream,
    tx: mpsc::SyncSender<(OutputStream, Vec<u8>)>,
    stop: Arc<AtomicBool>,
) -> std::thread::JoinHandle<()>
where
    R: std::io::Read + AsRawFd + Send + 'static,
{
    unsafe {
        let fd = pipe.as_raw_fd();
        let flags = libc::fcntl(fd, libc::F_GETFL);
        if flags >= 0 {
            let _ = libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
        }
    }
    spawn_reader_loop(pipe, stream, tx, stop)
}

#[cfg(not(unix))]
fn spawn_reader<R>(
    pipe: R,
    stream: OutputStream,
    tx: mpsc::SyncSender<(OutputStream, Vec<u8>)>,
    stop: Arc<AtomicBool>,
) -> std::thread::JoinHandle<()>
where
    R: std::io::Read + Send + 'static,
{
    spawn_reader_loop(pipe, stream, tx, stop)
}

fn spawn_reader_loop<R>(
    mut pipe: R,
    stream: OutputStream,
    tx: mpsc::SyncSender<(OutputStream, Vec<u8>)>,
    stop: Arc<AtomicBool>,
) -> std::thread::JoinHandle<()>
where
    R: std::io::Read + Send + 'static,
{
    std::thread::spawn(move || {
        let mut buffer = [0u8; 4096];
        loop {
            match pipe.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => {
                    let mut chunk = (stream, buffer[..count].to_vec());
                    loop {
                        match tx.try_send(chunk) {
                            Ok(()) => break,
                            Err(mpsc::TrySendError::Disconnected(_)) => return,
                            Err(mpsc::TrySendError::Full(pending)) => {
                                if stop.load(Ordering::SeqCst) {
                                    return;
                                }
                                chunk = pending;
                                std::thread::sleep(Duration::from_millis(5));
                            }
                        }
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if stop.load(Ordering::SeqCst) {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(5));
                    continue;
                }
                Err(_) => break,
            }
            if stop.load(Ordering::SeqCst) {
                break;
            }
        }
    })
}

fn carry_index(stream: OutputStream) -> usize {
    match stream {
        OutputStream::Stdout => 0,
        OutputStream::Stderr => 1,
    }
}

/// A foreground command's output so far: the decoded text kept for the
/// model's copy and `read_result`, the raw byte count, and a per-stream
/// carry for a UTF-8 code point split across pipe reads — decoding each
/// chunk alone turned split points into U+FFFD live.
#[derive(Default)]
struct Capture {
    retained: Vec<u8>,
    total_bytes: usize,
    carries: [Vec<u8>; 2],
}

impl Capture {
    /// Publish what the readers have queued. Bound each drain so continuous
    /// output cannot starve cancellation.
    fn drain_ready<F>(&mut self, rx: &mpsc::Receiver<(OutputStream, Vec<u8>)>, on_output: &mut F)
    where
        F: FnMut(OutputStream, &str),
    {
        for (stream, bytes) in rx.try_iter().take(64) {
            self.publish(stream, &bytes, on_output);
        }
    }

    /// Retain a bounded suffix and publish every chunk so pipe draining never
    /// depends on the model-output cap. The tail is what is kept: compilers and
    /// test runners put the verdict at the end of a long log, so retaining the
    /// head handed the model 32KB of passing output and dropped the failure.
    /// Publishing goes through the stream's carry so only complete UTF-8 leaves;
    /// a split code point waits for its remaining bytes instead of becoming a
    /// replacement character.
    fn publish<F>(&mut self, stream: OutputStream, bytes: &[u8], on_output: &mut F)
    where
        F: FnMut(OutputStream, &str),
    {
        // Far more than the model's copy (the last 32 KiB): the rest is kept
        // for `read_result`, so a long log's beginning is deferred, not lost.
        const RETAIN_LIMIT: usize = 4 * 1024 * 1024;
        // Trim in slabs: draining a few bytes off the front of a 4 MiB buffer
        // on every chunk would make a long stream quadratic.
        const RETAIN_SLACK: usize = 512 * 1024;
        self.total_bytes = self.total_bytes.saturating_add(bytes.len());
        let carry = &mut self.carries[carry_index(stream)];
        carry.extend_from_slice(bytes);
        let text = drain_complete_utf8(carry);
        // The kept copy is the decoded text, per stream: raw stdout and stderr
        // bytes interleaved could split one code point around the other
        // stream's chunk, and a lossy decode of that later reads as garbage.
        self.retained.extend_from_slice(text.as_bytes());
        if self.retained.len() > RETAIN_LIMIT + RETAIN_SLACK {
            keep_tail(&mut self.retained, RETAIN_LIMIT);
        }
        if !text.is_empty() {
            on_output(stream, &text);
        }
    }

    /// The pipes are closed: whatever the carries still hold is genuinely
    /// incomplete output, published lossily rather than dropped.
    fn flush_carries<F>(&mut self, on_output: &mut F)
    where
        F: FnMut(OutputStream, &str),
    {
        for stream in [OutputStream::Stdout, OutputStream::Stderr] {
            let carry = &self.carries[carry_index(stream)];
            if !carry.is_empty() {
                let rest = String::from_utf8_lossy(carry);
                self.retained.extend_from_slice(rest.as_bytes());
                on_output(stream, &rest);
            }
        }
    }

    /// The model's copy is the tail: test runners put the verdict at the
    /// end of a long log. The marker leads, so a reader knows it is
    /// mid-stream before line one, and names the kept result to page into.
    fn model_copy(self, state: &super::ToolRuntime) -> String {
        let total_bytes = self.total_bytes;
        let kept = self.retained.len();
        let full = model_text(&self.retained);
        if full.len() > super::MAX_BYTES {
            let mut start = full.len() - super::MAX_BYTES;
            while !full.is_char_boundary(start) {
                start += 1;
            }
            let tail = full[start..].to_string();
            // Bytes dropped by retention are raw bytes against raw bytes: the
            // decoded copy is shorter by every colour code it shed, and those
            // were kept.
            let dropped = if total_bytes > kept {
                format!("; the first {} bytes were not kept", total_bytes - kept)
            } else {
                String::new()
            };
            let id = state.retain_result(full);
            format!(
                "… [truncated: {total_bytes} bytes total, showing the last {}; earlier output: read_result {{\"id\": {id}}}{dropped}]\n{tail}",
                tail.len()
            )
        } else if total_bytes > kept {
            earlier_dropped(total_bytes, kept, &full)
        } else {
            full
        }
    }
}

/// Output as the model reads it: decoded, stripped of colour codes and
/// progress-bar rewrites — token noise it should never pay for.
fn model_text(bytes: &[u8]) -> String {
    super::resolve_carriage_returns(&super::strip_ansi(&String::from_utf8_lossy(bytes)))
        .trim_end()
        .to_string()
}

/// `text` led by a marker saying only its last `kept` of `total` bytes were
/// retained.
fn earlier_dropped(total: usize, kept: usize, text: &str) -> String {
    format!(
        "… [truncated: {total} bytes total, showing the last {kept} — earlier output dropped]\n{text}"
    )
}

/// Trim `buf` from the front to its last `limit` bytes. The raw byte cut may
/// land inside a UTF-8 code point: discard only the orphaned continuation
/// prefix so the tail starts at a real boundary and lossy decoding doesn't
/// invent a leading U+FFFD.
fn keep_tail(buf: &mut Vec<u8>, limit: usize) {
    let excess = buf.len() - limit;
    buf.drain(..excess);
    let orphaned = buf
        .iter()
        .take_while(|byte| (**byte & 0b1100_0000) == 0b1000_0000)
        .count();
    buf.drain(..orphaned);
}

/// Decode everything decodable, leaving at most an incomplete trailing
/// sequence in the buffer. Interior invalid bytes become U+FFFD — they are
/// genuinely bad, not split.
fn drain_complete_utf8(carry: &mut Vec<u8>) -> String {
    let mut out = String::new();
    loop {
        match std::str::from_utf8(carry) {
            Ok(text) => {
                out.push_str(text);
                carry.clear();
                return out;
            }
            Err(e) => {
                let valid = e.valid_up_to();
                // valid_up_to() proves the prefix decodes; lossy is byte-
                // identical there and degrades instead of panicking if a
                // future refactor breaks that proof.
                out.push_str(&String::from_utf8_lossy(&carry[..valid]));
                match e.error_len() {
                    Some(bad) => {
                        out.push('\u{FFFD}');
                        carry.drain(..valid + bad);
                    }
                    None => {
                        // An incomplete sequence at the tail: keep it for
                        // the next chunk.
                        carry.drain(..valid);
                        return out;
                    }
                }
            }
        }
    }
}

/// Lock a mutex, recovering the data if a panicking holder poisoned it.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}

fn failure(message: &str) -> ToolOutput {
    ToolOutput {
        content: message.into(),
        outcome: ToolOutcome::Failed,
        summary: "error".into(),
        display: None,
    }
}

#[cfg(test)]
mod tests {

    /// A leader's zombie reserves its PID until inherited pipes have drained.
    #[cfg(unix)]
    #[test]
    fn background_reaper_reserves_pid_while_pipes_are_open() {
        let child = std::process::Command::new("sh")
            .args(["-c", "exit 0"])
            .spawn()
            .unwrap();
        let pid = child.id();
        let process = std::sync::Arc::new(super::BackgroundProcess {
            pid,
            command: String::new(),
            output: std::sync::Mutex::new(Vec::new()),
            total_bytes: std::sync::Mutex::new(0),
            exit: std::sync::Mutex::new(None),
            finished_sequence: std::sync::atomic::AtomicU64::new(0),
        });
        let (release, wait) = std::sync::mpsc::channel();
        let pipe = std::thread::spawn(move || {
            let _ = wait.recv();
        });
        let reaper = std::thread::spawn(move || {
            super::reap_background(child, process, Some(pipe), None, std::sync::Weak::new())
        });
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        let reserved = loop {
            let state = std::process::Command::new("ps")
                .args(["-o", "stat=", "-p", &pid.to_string()])
                .output()
                .unwrap();
            if String::from_utf8_lossy(&state.stdout).contains('Z') {
                break true;
            }
            if std::time::Instant::now() >= deadline {
                break false;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        release.send(()).unwrap();
        reaper.join().unwrap();
        assert!(reserved, "leader was reaped before inherited pipes closed");
    }

    use super::*;

    fn finished(sequence: u64) -> Arc<BackgroundProcess> {
        Arc::new(BackgroundProcess {
            pid: 0,
            command: String::new(),
            output: Mutex::new(Vec::new()),
            total_bytes: Mutex::new(0),
            exit: Mutex::new(Some(ExitOutcome::Exited(0))),
            finished_sequence: AtomicU64::new(sequence),
        })
    }

    /// A finished handle's pid may already belong to someone else. Here an
    /// unrelated group leader wears that pid; killing the handle must leave
    /// it alone.
    #[cfg(unix)]
    #[test]
    fn killing_a_finished_handle_does_not_signal_its_reused_pid() {
        let mut bystander = Command::new("sleep");
        bystander.arg("30").stdin(Stdio::null());
        unsafe {
            bystander.pre_exec(|| {
                if libc::setsid() < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let mut bystander = bystander.spawn().unwrap();
        let registry = BackgroundRegistry::default();
        let process = Arc::new(BackgroundProcess {
            pid: bystander.id(),
            command: String::new(),
            output: Mutex::new(Vec::new()),
            total_bytes: Mutex::new(0),
            exit: Mutex::new(Some(ExitOutcome::Exited(0))),
            finished_sequence: AtomicU64::new(1),
        });
        assert!(register_background(&registry, "done".into(), process));

        let out = query_background(&registry, "done", true);
        assert_eq!(out.summary, "exited 0");
        std::thread::sleep(Duration::from_millis(50));
        assert!(
            bystander.try_wait().unwrap().is_none(),
            "an unrelated process group was killed through a stale handle"
        );
        kill_group(bystander.id());
        let _ = bystander.wait();
    }

    #[test]
    fn finished_background_records_are_bounded() {
        let mut processes = HashMap::new();
        for sequence in 1..=(BACKGROUND_FINISHED_RETAIN as u64 + 2) {
            processes.insert(sequence.to_string(), finished(sequence));
        }
        prune_background(&mut processes);
        assert_eq!(processes.len(), BACKGROUND_FINISHED_RETAIN);
        assert!(!processes.contains_key("1"));
        assert!(processes.contains_key(&(BACKGROUND_FINISHED_RETAIN as u64 + 2).to_string()));
    }
}
