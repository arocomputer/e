//! The agent: one session, one event stream, the tool loop.
//!
//! A turn is: request → stream text/reasoning/tool-call events → if the model
//! called tools, run them (yolo — no gate), append results, request again;
//! repeat until a reply arrives with no tool calls. Steering messages typed
//! mid-turn are drained between steps, before the next request. The whole turn
//! emits on one ordered channel and ends with exactly one `TurnEnd`.

pub mod compact;
pub mod context;
mod event;
mod persistence;
use persistence::{note_persist, TurnLog};
pub mod failure;
pub mod retry;
mod turn;
pub mod wake;

/// The continuation message committed after a sleep-caused mid-reply loss:
/// history already holds the truncated reply, so this is the whole prompt
/// the model needs to finish its own sentence.
const SLEEP_CONTINUATION: &str = "Your previous reply was cut off because the device slept. \
Continue from exactly where it stopped.";

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::sync::mpsc;

use crate::cli::ToolMode;
use crate::providers::catalog::{slug, Model};
use crate::providers::{
    self, ChatMessage, Event as ProviderEvent, FailureCause, FinishReason, Request, ToolCall,
};
use crate::session::SessionLog;
use crate::tools;

/// Steps (provider requests, tool batches between them) one turn may run
/// before it stops and asks to be continued. A backstop against a model
/// stuck calling tools forever — set far above any legitimate turn.
const MAX_STEPS: u32 = 256;

fn clone_request(r: &Request) -> Request {
    Request {
        model: r.model.clone(),
        system: r.system.clone(),
        messages: r.messages.clone(),
        effort: r.effort.clone(),
        session_id: r.session_id.clone(),
        tools: r.tools.clone(),
    }
}

/// Why a compaction left history as it was because the user cancelled it.
const COMPACTION_CANCELLED: &str = "compaction cancelled; history was preserved";

/// Build and install a checkpoint without exposing a partial history swap.
/// Cancellation stops the provider request; failed summaries leave the log intact.
async fn compact_log(
    log: &TurnLog,
    system: &str,
    cancel: &Arc<AtomicBool>,
    host: Option<&Arc<crate::extensions::ExtensionHost>>,
    focus: Option<String>,
) -> Result<bool, String> {
    let history = log
        .history
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .clone();
    let (older, kept) = compact::split(&history, log.model.context_window);
    if older.is_empty() {
        return Ok(false);
    }
    tokio::select! {
        biased;
        _ = wait_cancelled(cancel) => return Err(COMPACTION_CANCELLED.into()),
        _ = log.events.send(SessionEvent::Compacting) => {}
    }
    // Reserve completion capacity before doing work or changing history.
    // Once installed, the checkpoint can then be published without an await.
    let completion = tokio::select! {
        biased;
        _ = wait_cancelled(cancel) => return Err(COMPACTION_CANCELLED.into()),
        result = log.events.reserve() => result.map_err(|_| "session event receiver closed; history was preserved".to_string())?,
    };
    let session_id = log
        .session
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .as_ref()
        .map(|session| session.id().to_string())
        .unwrap_or_default();
    if let Some(h) = host {
        h.event("compact_start", serde_json::json!({})).await;
    }
    let summary = summarize_older(log, &older, session_id, cancel, host, focus).await?;
    let mut projected = vec![ChatMessage::user(compact::seed(&summary.text))];
    projected.extend(kept.iter().cloned());
    let tokens = compact::estimate_request_tokens(system, &projected);
    if tokens >= compact::estimate_request_tokens(system, &history)
        || compact::should_compact(tokens, log.model.context_window)
    {
        return Err("compaction did not reduce context enough; history was preserved".into());
    }
    if cancel.load(Ordering::SeqCst) {
        return Err(COMPACTION_CANCELLED.into());
    }
    install_checkpoint(log, &summary, kept, history, cancel).await?;
    if let Some(h) = host {
        h.event("compact_end", serde_json::json!({"summary": summary.text}))
            .await;
    }
    completion.send(SessionEvent::Compacted {
        summary: summary.text,
        context_tokens: tokens,
        response: summary.response,
        pricing: log.model.pricing.clone(),
    });
    Ok(true)
}

/// Summarize the older messages, then let a `compact_summary` hook rewrite
/// the text, warning when the stored summary lacks expected sections. Esc
/// abandons either step.
async fn summarize_older(
    log: &TurnLog,
    older: &[ChatMessage],
    session_id: String,
    cancel: &AtomicBool,
    host: Option<&Arc<crate::extensions::ExtensionHost>>,
    focus: Option<String>,
) -> Result<compact::Summary, String> {
    let mut summary = tokio::select! {
        result = compact::summarize(log.model.clone(), older, session_id, focus.as_deref()) => result?,
        _ = wait_cancelled(cancel) => return Err(COMPACTION_CANCELLED.into()),
    };
    // Extensions get the last word on the summary, not the history: the
    // hook is bounded and fails open, so a silent one changes nothing.
    if let Some(h) = host.filter(|h| h.has_hook("compact_summary")) {
        let rewritten = {
            let hook = h.hook_compact_summary(&summary.text);
            tokio::pin!(hook);
            tokio::select! {
                text = &mut hook => text,
                _ = wait_cancelled(cancel) => return Err(COMPACTION_CANCELLED.into()),
            }
        };
        if let Some(text) = rewritten {
            summary.text = text;
        }
    }
    // Judged after the hook: the sections that matter are the stored ones.
    let missing = compact::missing_sections(&summary.text);
    if !missing.is_empty() {
        let _ = log
            .events
            .send(SessionEvent::Warning(format!(
                "compaction summary is missing {}; it was kept as written",
                missing.join(", ")
            )))
            .await;
    }
    Ok(summary)
}

/// Swap the checkpoint into the log on a blocking thread. It installs only
/// if history still matches the snapshot it was built from and the save
/// succeeds; otherwise history stays as it was.
async fn install_checkpoint(
    log: &TurnLog,
    summary: &compact::Summary,
    kept: Vec<ChatMessage>,
    history: Vec<ChatMessage>,
    cancel: &Arc<AtomicBool>,
) -> Result<(), String> {
    let writer = log.clone();
    let checkpoint = summary.text.clone();
    let response = summary.response.clone();
    let installation_cancel = cancel.clone();
    let installed = tokio::task::spawn_blocking(move || {
        writer.load_compacted(
            &checkpoint,
            Some(response),
            kept,
            &installation_cancel,
            &history,
        )
    })
    .await
    .map_err(|error| format!("compaction commit failed: {error}"))?;
    if !installed {
        if cancel.load(Ordering::SeqCst) {
            return Err(COMPACTION_CANCELLED.into());
        }
        return Err("compaction could not be installed; history changed or could not be saved; history was preserved".into());
    }
    Ok(())
}

/// Resolve when Esc (or any interrupt) has been requested. Polled on a short
/// interval so a stalled provider stream — which never yields another event —
/// cannot strand the turn with `running` stuck true and Esc inert.
async fn wait_cancelled(cancel: &AtomicBool) {
    while !cancel.load(Ordering::SeqCst) {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Sleep for `delay`, but give up early the moment Esc is pressed. A retry
/// backoff can run up to 30 seconds — a bare `sleep` would leave Esc inert
/// for the whole wait, the same stalled-spinner bug the stream loop already
/// guards against. Returns false when cancelled before the delay elapsed.
async fn sleep_cancellable(delay: Duration, cancel: &AtomicBool) -> bool {
    tokio::select! {
        _ = tokio::time::sleep(delay) => true,
        _ = wait_cancelled(cancel) => false,
    }
}

/// Supervise one run on the runtime: its heartbeat, its workers (the first
/// runs `compact_only`; a continuation compacts only when no prompt is
/// waiting), and the `turn_end` event once the run settles.
fn spawn_supervisor(context: turn::Context, compact_only: bool) -> tokio::task::JoinHandle<()> {
    // The heartbeat belongs to the supervisor too. If the turn worker
    // panics, it is stopped instead of leaking into later turns.
    let heartbeat_stop = Arc::new(AtomicBool::new(false));
    let heartbeat = tokio::spawn(wake::heartbeat(
        context.wake.clone(),
        heartbeat_stop.clone(),
        Duration::from_secs(1),
    ));
    let events = context.events.clone();
    let host = context.host.clone();
    let pending = context.pending.clone();
    let compact_requested = context.compact_requested.clone();
    let mut first_worker = true;
    let spawn_worker = move || {
        let compact_only = if first_worker {
            compact_only
        } else {
            context
                .pending
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .items
                .is_empty()
        };
        first_worker = false;
        tokio::spawn(crate::config::home::scope(
            context.log.home.clone(),
            turn::run(context.clone(), compact_only),
        ))
    };
    tokio::spawn(async move {
        let aborted = supervise_turn(spawn_worker, events, pending, compact_requested).await;
        heartbeat_stop.store(true, Ordering::SeqCst);
        heartbeat.abort();
        let _ = heartbeat.await;
        if let Some(h) = &host {
            h.event("turn_end", serde_json::json!({"aborted": aborted}))
                .await;
        }
    })
}

/// Own completion and the submission race. A prompt arriving before the
/// terminal event is published is consumed by another worker in this run.
async fn supervise_turn<F>(
    mut spawn_worker: F,
    events: mpsc::Sender<SessionEvent>,
    pending: Arc<Mutex<PendingQueue>>,
    compact_requested: Arc<AtomicBool>,
) -> bool
where
    F: FnMut() -> tokio::task::JoinHandle<turn::Outcome>,
{
    let _ = events.send(SessionEvent::TurnStart).await;
    loop {
        let (aborted, failed) = match spawn_worker().await {
            Ok(outcome) => (
                outcome == turn::Outcome::Cancelled,
                outcome == turn::Outcome::Failed,
            ),
            Err(error) => {
                let failure = if error.is_panic() {
                    format!("turn worker panicked: {error}")
                } else {
                    format!("turn worker stopped unexpectedly: {error}")
                };
                let _ = events.send(SessionEvent::Error(failure)).await;
                (false, true)
            }
        };
        // Reserve before locking: publishing completion and becoming idle are
        // one transaction with submit, without blocking a Tokio worker.
        let permits = events.reserve_many(2).await;
        let mut queue = pending.lock().unwrap_or_else(|error| error.into_inner());
        if !aborted
            && !failed
            && (!queue.items.is_empty() || compact_requested.load(Ordering::SeqCst))
        {
            continue;
        }
        let discarded: Vec<String> = queue
            .items
            .drain(..)
            .map(|(_, message)| message.content)
            .collect();
        compact_requested.store(false, Ordering::SeqCst);
        queue.running = false;
        if let Ok(mut permits) = permits {
            if !discarded.is_empty() {
                if let Some(permit) = permits.next() {
                    permit.send(SessionEvent::Discarded(discarded));
                }
            }
            if let Some(permit) = permits.next() {
                permit.send(SessionEvent::TurnEnd { aborted });
            }
        }
        return aborted;
    }
}

pub use event::{SessionEvent, ToolCallPresentation};

/// The effort a model would use given its declared `levels` and the saved
/// setting: the saved value when this model supports it, else the model's
/// strong default — `high` when declared, otherwise its first level. None
/// when the model has no reasoning knob at all.
pub fn effort(levels: &[String], saved: Option<&str>) -> Option<String> {
    if levels.is_empty() {
        return None;
    }
    match saved {
        Some(v) if levels.iter().any(|l| l == v) => Some(v.to_string()),
        _ => Some(if levels.iter().any(|l| l == "high") {
            "high".to_string()
        } else {
            levels[0].clone()
        }),
    }
}

/// The next level after `current` in the model's cycle, wrapping around.
pub fn next_effort(levels: &[String], current: &str) -> String {
    let idx = levels.iter().position(|l| l == current).unwrap_or(0);
    levels[(idx + 1) % levels.len()].clone()
}

#[derive(Clone, Debug)]
pub struct AgentOptions {
    /// Explicit workspace and configuration paths for in-process callers.
    pub cwd: Option<PathBuf>,
    pub home: Option<PathBuf>,
    pub save_session: bool,
    pub tool_mode: ToolMode,
    pub effort_override: Option<String>,
    /// A positive built-in tool allowlist for this run. `None` is the full
    /// built-in and extension set. It composes under `tool_mode`, so no-tools
    /// mode always wins.
    pub allowed_tools: Option<Vec<String>>,
}

impl Default for AgentOptions {
    fn default() -> Self {
        AgentOptions {
            cwd: None,
            home: None,
            save_session: true,
            tool_mode: ToolMode::All,
            effort_override: None,
            allowed_tools: None,
        }
    }
}

#[derive(Default)]
struct PendingQueue {
    running: bool,
    next_id: u64,
    /// Whole messages, not just text: a queued image prompt keeps its
    /// attachments until the turn drains it.
    items: Vec<(u64, ChatMessage)>,
}

pub struct Agent {
    home: PathBuf,
    tools: Arc<tools::ToolRuntime>,
    pub model: Model,
    /// The extension host; None means built-in tools only.
    host: Option<std::sync::Arc<crate::extensions::ExtensionHost>>,
    cwd: PathBuf,
    history: Arc<Mutex<Vec<ChatMessage>>>,
    events: mpsc::Sender<SessionEvent>,
    /// Messages typed while a turn runs. Entries are keyed so the frontend
    /// can edit or drop a specific one after snapshotting: the turn loop
    /// drains concurrently, and a key it already took is gone — the review
    /// commits such an entry as a fresh prompt instead of resurrecting it.
    pending: Arc<Mutex<PendingQueue>>,
    cancel: Arc<AtomicBool>,
    compact_requested: Arc<AtomicBool>,
    /// What the next requested compaction should focus on (`/compact <focus>`),
    /// taken by the turn that performs it.
    compact_focus: Arc<Mutex<Option<String>>>,
    /// Nested `AGENTS.md` directories already loaded this session.
    instructions_loaded: Arc<Mutex<std::collections::HashSet<PathBuf>>>,
    /// Messages an extension attached to the next prompt (`session.send`
    /// with `when: "next_turn"`): committed just before it, never alone.
    next_turn: Mutex<Vec<ChatMessage>>,
    /// The supervisor owns the worker's terminal event. Keeping its handle
    /// prevents the turn from becoming unobserved background work.
    turn_task: Option<tokio::task::JoinHandle<()>>,
    /// The session log; every committed message is appended.
    session: Arc<Mutex<Option<SessionLog>>>,
    /// An extension-set display name, applied when the log exists or when it
    /// is created on the first message.
    session_name: Arc<Mutex<Option<String>>>,
    /// Latch for the persistence-failure warning (see `note_persist`).
    persist_warned: Arc<AtomicBool>,
    /// Display ids for tool lifecycle events, unique across the whole
    /// session: an Esc-detached task from an earlier turn keeps a live
    /// events sender, and a per-turn counter would let its stale ToolEnd
    /// collide with (and corrupt) a later turn's row.
    tool_seq: Arc<AtomicU64>,
    /// The tools the model may see and call right now, when an extension
    /// narrowed them (`session.tools`); None is everything. Built-in and
    /// extension names alike, checked at advertisement and at execution.
    active_tools: Arc<Mutex<Option<Vec<String>>>>,
    /// The latest observed system-sleep gap. Written by the turn's
    /// heartbeat task; tests write it through [`Agent::inject_sleep_gap`].
    wake: wake::Shared,
    options: AgentOptions,
}

impl Agent {
    pub fn new(model: Model) -> (Self, mpsc::Receiver<SessionEvent>) {
        Self::with_options(model, AgentOptions::default())
    }

    pub fn with_options(
        model: Model,
        options: AgentOptions,
    ) -> (Self, mpsc::Receiver<SessionEvent>) {
        let (events, rx) = mpsc::channel(256);
        let process_cwd = std::env::current_dir().unwrap_or_default();
        let cwd = options.cwd.clone().unwrap_or_else(|| process_cwd.clone());
        let home = options
            .home
            .clone()
            .unwrap_or_else(crate::config::home::home);
        let cwd = if cwd.is_absolute() {
            cwd
        } else {
            process_cwd.join(cwd)
        };
        let home = if home.is_absolute() {
            home
        } else {
            process_cwd.join(home)
        };
        let agent = Agent {
            home,
            tools: Arc::new(tools::ToolRuntime::default()),
            model,
            host: None,
            cwd,
            history: Arc::new(Mutex::new(Vec::new())),
            events,
            pending: Arc::new(Mutex::new(PendingQueue::default())),
            cancel: Arc::new(AtomicBool::new(false)),
            compact_requested: Arc::new(AtomicBool::new(false)),
            compact_focus: Arc::new(Mutex::new(None)),
            instructions_loaded: Arc::new(Mutex::new(Default::default())),
            next_turn: Mutex::new(Vec::new()),
            turn_task: None,
            session: Arc::new(Mutex::new(None)),
            session_name: Arc::new(Mutex::new(None)),
            persist_warned: Arc::new(AtomicBool::new(false)),
            tool_seq: Arc::new(AtomicU64::new(0)),
            active_tools: Arc::new(Mutex::new(None)),
            wake: wake::shared(),
            options,
        };
        (agent, rx)
    }

    /// Test seam: record a sleep gap as if the heartbeat had just observed
    /// the machine wake. The turn loop attributes stream losses to it when
    /// the attempt was in flight across the gap.
    pub fn inject_sleep_gap(&mut self, duration: std::time::Duration) {
        *self.wake.lock().unwrap_or_else(|e| e.into_inner()) = Some(wake::SleepGap {
            duration,
            woke_at: Instant::now(),
        });
    }

    /// Attach the extension host: its tools join (and may override) the
    /// built-ins, and its hooks gate every tool call.
    pub fn set_host(&mut self, host: std::sync::Arc<crate::extensions::ExtensionHost>) {
        self.host = Some(host);
    }

    pub fn is_streaming(&self) -> bool {
        self.pending
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .running
    }
    pub fn model_slug(&self) -> String {
        slug(&self.model)
    }
    /// The model's declared effort levels, in order; empty when it has no
    /// reasoning knob.
    pub fn effort_levels(&self) -> Vec<String> {
        self.model.effort.clone()
    }
    /// The effort for the next request: the saved setting when this model
    /// supports it, else the model's strong default (`high` when declared,
    /// otherwise its first level).
    pub fn effort(&self) -> Option<String> {
        let saved = self.options.effort_override.clone().or_else(|| {
            crate::config::home::with_home(self.home.clone(), || {
                crate::config::settings::get_string("effort")
            })
        });
        effort(&self.model.effort, saved.as_deref())
    }
    /// Select one of the current model's declared effort levels and persist it.
    /// Returns false when this model does not accept the requested value.
    pub fn set_effort(&mut self, effort: &str) -> Result<bool, std::io::Error> {
        if !self.model.effort.iter().any(|level| level == effort) {
            return Ok(false);
        }
        crate::config::home::with_home(self.home.clone(), || {
            crate::config::settings::set_string("effort", effort)
        })?;
        // Keep the running agent in sync too. In particular, this replaces a
        // launch-time override so /effort and shift+tab take effect now rather
        // than only after e restarts.
        self.options.effort_override = Some(effort.to_string());
        Ok(true)
    }

    /// Select one of the model's effort levels for this run only — nothing
    /// is written to settings. `e rpc` changes a session's effort this way:
    /// one client's session must not rewrite the user's saved preference.
    /// False when the model does not accept the value.
    pub fn set_run_effort(&mut self, effort: &str) -> bool {
        if !self.model.effort.iter().any(|level| level == effort) {
            return false;
        }
        self.options.effort_override = Some(effort.to_string());
        true
    }

    /// The settings panel wrote the persisted effort directly. Stop applying
    /// an older launch/runtime override so the new value takes effect now.
    pub fn use_saved_effort(&mut self) {
        self.options.effort_override = None;
    }

    /// Advance to the model's next effort level and persist it. None when
    /// the model has no reasoning knob.
    pub fn cycle_effort(&mut self) -> Result<Option<String>, std::io::Error> {
        let levels = self.effort_levels();
        if levels.is_empty() {
            return Ok(None);
        }
        let Some(current) = self.effort() else {
            return Ok(None);
        };
        let next = next_effort(&levels, current.as_str());
        if !self.set_effort(&next)? {
            return Ok(None);
        }
        Ok(Some(next))
    }
    pub fn history_snapshot(&self) -> Vec<ChatMessage> {
        self.history
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
    pub fn load_history(&mut self, messages: Vec<ChatMessage>) {
        self.tools = Arc::new(tools::ToolRuntime::default());
        self.reset_session_scoped();
        self.remember_instructions(&messages);
        *self.history.lock().unwrap_or_else(|e| e.into_inner()) = messages;
    }

    /// The nested `AGENTS.md` a history already carries count as loaded:
    /// a resumed or rewound session must neither repeat them nor lose them.
    fn remember_instructions(&self, messages: &[ChatMessage]) {
        *self
            .instructions_loaded
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = turn::instruction_dirs(messages);
    }
    pub fn clear(&mut self) {
        self.tools = Arc::new(tools::ToolRuntime::default());
        self.reset_session_scoped();
        self.history
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }

    /// The commit handle over this agent's history, session, and warning
    /// latch — the one way messages enter the record.
    fn log(&self) -> TurnLog {
        TurnLog {
            home: self.home.clone(),
            history: self.history.clone(),
            session: self.session.clone(),
            cwd: self.cwd.clone(),
            model: self.model.clone(),
            session_name: self.session_name.clone(),
            persist_warned: self.persist_warned.clone(),
            events: self.events.clone(),
            save_session: self.options.save_session,
        }
    }

    /// Commit a user-visible fact into history and the session log without
    /// starting a turn — the `!` shell passthrough records its output this way
    /// so the model sees what the user ran.
    pub fn record_user(&self, text: String) {
        self.log().commit(ChatMessage::user(text));
    }

    /// Add a user-role message the model sees but the transcript does not,
    /// without starting a turn — an extension's `session.send` with
    /// `internal: true`.
    pub fn record_internal(&self, text: String) {
        let mut message = ChatMessage::user(text);
        message.mark_internal();
        self.log().commit(message);
    }

    /// Hold an internal message until the next prompt, then commit it just
    /// ahead of that prompt — context that should ride with what the user
    /// says next rather than sit alone in the conversation.
    pub fn attach_to_next_turn(&self, text: String) {
        let mut message = ChatMessage::user(text);
        message.mark_internal();
        self.next_turn
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(message);
    }

    fn take_next_turn(&self) -> Vec<ChatMessage> {
        std::mem::take(&mut *self.next_turn.lock().unwrap_or_else(|e| e.into_inner()))
    }

    /// State that belongs to one session's run and must not outlive it: the
    /// nested instructions already loaded, an extension's tool narrowing,
    /// and messages attached to the next turn.
    fn reset_session_scoped(&self) {
        self.instructions_loaded
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
        self.set_active_tools(None);
        self.next_turn
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }

    /// Whether this agent writes a session log — and, by the same token,
    /// whether anything it does should persist to disk.
    pub fn saves_session(&self) -> bool {
        self.options.save_session
    }

    /// `/undo`: revert the newest write or edit this session made. Idle
    /// only — mid-turn the files belong to the running tools.
    pub fn undo_last_change(&self) -> Result<Option<String>, String> {
        self.tools.undo_last()
    }

    pub fn undo_depth(&self) -> usize {
        self.tools.undo_depth()
    }

    /// Narrow (or with None, restore) the tools advertised and executable
    /// from the next request on. Names are built-in or extension tools.
    pub fn set_active_tools(&self, names: Option<Vec<String>>) {
        *self.active_tools.lock().unwrap_or_else(|e| e.into_inner()) = names;
    }

    pub fn active_tools(&self) -> Option<Vec<String>> {
        self.active_tools
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Replace the history with the compaction seed plus the kept recent
    /// messages, committing everything into a fresh session file so the
    /// compacted state is itself resumable; the old file stays untouched.
    /// The fresh log is created before the old one is detached: if creation
    /// fails, the old log stays attached and later turns append to it, so a
    /// crash resumes into the complete pre-compaction conversation instead
    /// of a new file holding only an unanchored tail.
    ///
    /// Runs the file I/O on the blocking pool — this is called from the
    /// TUI's own async event loop, and a multi-message session write must
    /// not stall it any more than the turn loop's own commits are allowed
    /// to (see `TurnLog::commit_async`).
    pub async fn load_compacted(&self, summary: &str, kept: Vec<ChatMessage>) -> bool {
        let log = self.log();
        let summary = summary.to_string();
        let expected = self.history_snapshot();
        tokio::task::spawn_blocking(move || {
            log.load_compacted(&summary, None, kept, &AtomicBool::new(false), &expected)
        })
        .await
        .unwrap_or(false)
    }

    /// Attach a session log; created lazily on the first message when None.
    pub fn set_session(&self, session: Option<SessionLog>) {
        *self.session.lock().unwrap_or_else(|e| e.into_inner()) = if self.options.save_session {
            session
        } else {
            None
        };
    }

    /// The active session's file, if a log exists yet — None before the
    /// first message is committed. `/tree` reads this file directly rather
    /// than tracking the graph in memory.
    pub fn session_path(&self) -> Option<PathBuf> {
        // Poison-contained like every other lock here: a panicked holder
        // must not cascade into panics on every later reader.
        self.session
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|s| s.path().to_path_buf())
    }

    /// The active conversation's stable session id, or None before the first
    /// message creates the log. Sent to gateways that ask for a per-
    /// conversation handle (see `providers::with_attribution`).
    pub fn session_id(&self) -> Option<String> {
        self.session
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|s| s.id().to_string())
    }

    /// Rewind: point the session at an earlier node (`/tree`'s choice) and
    /// mirror the path from root to that node into in-memory history. The
    /// file itself is untouched — the next commit attaches after `head`, so
    /// the abandoned tail survives as a sibling branch, not an overwrite.
    /// Same lock order as `commit`: history before session.
    pub fn rewind_to(&self, head: Option<String>, messages: Vec<ChatMessage>) {
        let mut history_guard = self.history.lock().unwrap_or_else(|e| e.into_inner());
        let mut session_guard = self.session.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(session) = session_guard.as_mut() {
            session.set_head(head);
        }
        // Only the nested AGENTS.md the kept branch carries stay loaded:
        // one past the new head loads again on the next touch, one before
        // it is not repeated.
        self.remember_instructions(&messages);
        *history_guard = messages;
    }

    /// Name this session: applies immediately when a log exists, otherwise
    /// when the log is created on the first message. Either way the name is
    /// idempotent — the last one wins.
    pub fn set_session_name(&self, name: String) {
        *self.session_name.lock().unwrap_or_else(|e| e.into_inner()) = Some(name);
        if !self.options.save_session {
            return;
        }
        let mut guard = self.session.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(s) = guard.as_mut() {
            if let Some(name) = self
                .session_name
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone()
            {
                let result = s.set_name(&name);
                drop(guard);
                note_persist(&self.persist_warned, result, &self.events);
            }
        }
    }

    /// Adopt the name a resumed session carries (or clear it for a fresh
    /// one). In-memory only — the log already holds its own name entries.
    pub fn adopt_session_name(&self, name: Option<String>) {
        *self.session_name.lock().unwrap_or_else(|e| e.into_inner()) = name;
    }

    /// Prompts waiting on the running turn (steering not yet drained).
    pub fn queued_count(&self) -> usize {
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .items
            .len()
    }

    /// Snapshot the queued prompts' text, oldest first, with the keys the
    /// review commit sends back. Purely a read: the turn keeps steering
    /// while the review is open.
    pub fn queue_snapshot(&self) -> Vec<(u64, String)> {
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .items
            .iter()
            .filter(|(_, message)| !message.is_internal())
            .map(|(id, message)| (*id, message.content.clone()))
            .collect()
    }

    /// Apply the queued-prompt review's edits. Each `(key, text)` replaces
    /// that entry in place when the turn has not drained it yet, or queues
    /// it fresh when it has — the user's edited intent, either way. Keys in
    /// `removed` drop their entry if it is still waiting. Unnamed entries
    /// are untouched.
    pub fn update_queued(&self, edits: Vec<(u64, String)>, removed: Vec<u64>) {
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        for (key, text) in edits {
            if let Some(entry) = pending.items.iter_mut().find(|(id, _)| *id == key) {
                entry.1.content = text;
            } else {
                // Drained while the review held it: the edit is still the
                // user's intent — submit it as a fresh steering message.
                pending.next_id += 1;
                let key = pending.next_id;
                pending.items.push((key, ChatMessage::user(text)));
            }
        }
        for key in removed {
            pending.items.retain(|(id, _)| *id != key);
        }
    }

    /// The extension-set session name, if any (the derived title still
    /// exists but the name overrides it in /resume).
    pub fn session_name(&self) -> Option<String> {
        self.session_name
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Drop the session name — a fresh session starts unnamed.
    pub fn clear_session_name(&self) {
        *self.session_name.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    pub fn cwd(&self) -> PathBuf {
        self.cwd.clone()
    }

    /// Assemble this agent's prompt using its own workspace and home.
    pub fn system_prompt(&self) -> String {
        crate::config::home::with_home(self.home.clone(), || context::system_prompt(&self.cwd))
    }

    /// Queue a message. If a turn is running it steers (drained next step);
    /// otherwise it starts a turn.
    /// A message typed while a turn runs never fires immediately: it is held
    /// and steered into the turn at the next step. Returns true if held,
    /// false if it began a fresh turn.
    pub fn submit(&mut self, text: String, system: String) -> bool {
        self.submit_message(ChatMessage::user(text), system)
    }

    /// Submit a normalized user message (used for image attachments).
    /// Steering remains text-only because a running request cannot safely
    /// acquire a new binary payload halfway through its provider stream.
    pub fn submit_message(&mut self, message: ChatMessage, system: String) -> bool {
        let attached = self.take_next_turn();
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        if pending.running {
            for held in attached.into_iter().chain(std::iter::once(message)) {
                pending.next_id += 1;
                let key = pending.next_id;
                pending.items.push((key, held));
            }
            drop(pending);
            return true;
        }
        pending.running = true;
        drop(pending);
        let log = self.log();
        for held in attached {
            log.commit(held);
        }
        log.commit(message);
        self.start(system, false);
        false
    }

    /// Hold a message for the running turn only. Unlike `submit`, this never
    /// starts a turn: when none is running it returns false and records
    /// nothing, so a caller racing the turn's end cannot commit a stray
    /// prompt. The check and the hold share the queue lock with the
    /// supervisor's end-of-turn transition.
    pub fn steer(&mut self, text: String) -> bool {
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        if !pending.running {
            return false;
        }
        pending.next_id += 1;
        let key = pending.next_id;
        pending.items.push((key, ChatMessage::user(text)));
        true
    }

    /// Submit a prompt and hold steering messages for the turn in one
    /// critical section. The worker's first step drains the queue before it
    /// builds its first request, so a steer enqueued here — rather than
    /// handed over after the turn started, when a fast turn could finish
    /// first — can never be raced out of the conversation. When a turn is
    /// already running everything queues as steering, mirroring
    /// `submit_message`; it never starts a second turn. The return follows
    /// `submit_message`: true when the prompt was only held as steering,
    /// false when it started a fresh turn.
    pub fn submit_message_with_steers(
        &mut self,
        message: ChatMessage,
        system: String,
        steers: Vec<ChatMessage>,
    ) -> bool {
        let mut pending = self.pending.lock().unwrap_or_else(|e| e.into_inner());
        if pending.running {
            for held in steers.into_iter().chain(std::iter::once(message)) {
                pending.next_id += 1;
                let key = pending.next_id;
                pending.items.push((key, held));
            }
            return true;
        }
        pending.running = true;
        for held in steers {
            pending.next_id += 1;
            let key = pending.next_id;
            pending.items.push((key, held));
        }
        drop(pending);
        self.log().commit(message);
        self.start(system, false);
        false
    }

    /// Request a checkpoint at the next provider boundary, or immediately
    /// when idle. No frontend needs to summarize or replace history.
    pub fn request_compaction(&mut self, system: String) {
        self.request_compaction_with(system, None);
    }

    /// `request_compaction` with what the summary should focus on.
    pub fn request_compaction_with(&mut self, system: String, focus: Option<String>) {
        *self
            .compact_focus
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = focus.filter(|f| !f.trim().is_empty());
        let mut queue = self
            .pending
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        self.compact_requested.store(true, Ordering::SeqCst);
        if queue.running {
            return;
        }
        queue.running = true;
        drop(queue);
        self.start(system, true);
    }

    /// Run a turn (or, with `compact_only`, just a checkpoint) under a fresh
    /// cancellation token. The supervisor owns the run from here.
    fn start(&mut self, system: String, compact_only: bool) {
        // A detached task retains its turn's permanently cancelled token.
        self.cancel = Arc::new(AtomicBool::new(false));
        let context = self.turn_context(system);
        self.turn_task = Some(spawn_supervisor(context, compact_only));
    }

    /// Everything the turn's workers share, captured now: later model or
    /// option changes apply to the next turn, not this one.
    fn turn_context(&self, system: String) -> turn::Context {
        let model = self.model.clone();
        let tool_mode = if model.supports_tools {
            self.options.tool_mode
        } else {
            ToolMode::None
        };
        // The request's allowlist is shared across the turn's tool tasks.
        let allowed_tools = self.options.allowed_tools.clone().map(Arc::new);
        let system = crate::config::home::with_home(self.home.clone(), || {
            match (tool_mode, allowed_tools.as_deref()) {
                (ToolMode::None, _) => format!("{system}\n\n{}", context::no_tools_notice()),
                (ToolMode::All, Some(tools)) if tools.is_empty() => {
                    format!("{system}\n\n{}", context::no_tools_notice())
                }
                (ToolMode::All, Some(tools)) => {
                    format!("{system}\n\n{}", context::tool_allowlist_notice(tools))
                }
                (ToolMode::All, None) => system,
            }
        });
        turn::Context {
            log: self.log(),
            events: self.events.clone(),
            history: self.history.clone(),
            cancel: self.cancel.clone(),
            model,
            cwd: self.cwd.clone(),
            effort: self.effort(),
            pending: self.pending.clone(),
            host: self.host.clone(),
            tool_seq: self.tool_seq.clone(),
            active_tools: self.active_tools.clone(),
            wake: self.wake.clone(),
            system,
            allowed_tools,
            compact_requested: self.compact_requested.clone(),
            compact_focus: self.compact_focus.clone(),
            instructions_loaded: self.instructions_loaded.clone(),
            tool_runtime: self.tools.clone(),
            tool_mode,
        }
    }

    pub fn interrupt(&mut self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::SeqCst);
    }
}

/// What one tool call runs with: the policy that may refuse it, the host
/// that may guard or serve it, and where its output goes.
struct ToolRunContext {
    tools: Arc<tools::ToolRuntime>,
    host: Option<std::sync::Arc<crate::extensions::ExtensionHost>>,
    tool_mode: ToolMode,
    allowed_tools: Option<Arc<Vec<String>>>,
    /// An extension's narrowing for this turn (`session.tools`), enforced
    /// at execution like the request allowlist: a provider can still emit
    /// any name.
    active_tools: Option<Arc<Vec<String>>>,
    cwd: PathBuf,
    cancel: Arc<AtomicBool>,
    id: u64,
    events: mpsc::Sender<SessionEvent>,
}

/// Dispatch one tool call: the turn's policy or extension hooks may block
/// it, an extension that owns the name serves it, otherwise the built-in
/// runs on a blocking thread.
async fn run_tool(context: ToolRunContext, name: &str, arguments: &str) -> tools::ToolOutput {
    if let Some(refused) = refusal(&context, name) {
        return refused;
    }
    // Hooks still guard allowlisted built-ins, but an extension cannot replace
    // one by claiming the same name.
    let builtins_only = context.allowed_tools.is_some();
    if let (ToolMode::All, Some(h)) = (context.tool_mode, &context.host) {
        if let Some(stopped) = hook_tool_call(h, name, arguments, &context.cancel).await {
            return stopped;
        }
        if !builtins_only && h.owns_tool(name) {
            return run_extension_tool(h, name, arguments, &context).await;
        }
    }
    run_builtin(context, name, arguments).await
}

/// Why this call cannot run at all, checked before any hook sees it: an
/// extension narrowed the toolset, the turn was cancelled, tools are off, or
/// the request's allowlist leaves the name out.
fn refusal(context: &ToolRunContext, name: &str) -> Option<tools::ToolOutput> {
    let blocked = |content: String| tool_output(content, tools::ToolOutcome::Blocked, "blocked");
    if let Some(active) = &context.active_tools {
        if !active.iter().any(|a| a == name) && !tools::always_available(name) {
            return Some(blocked(format!(
                "tool {name} is not active right now — an extension narrowed the toolset"
            )));
        }
    }
    if context.cancel.load(Ordering::SeqCst) {
        return Some(tool_output(
            "tool cancelled before execution".into(),
            tools::ToolOutcome::Cancelled,
            "cancelled",
        ));
    }
    if !context.tool_mode.allows() {
        return Some(blocked(format!("tool blocked by no-tools mode: {name}")));
    }
    // Enforce the request's list at execution too. The advertised schemas are
    // not a security boundary because a provider can still emit any tool name.
    if let Some(allowed) = &context.allowed_tools {
        if !allowed.iter().any(|a| a == name) && !tools::always_available(name) {
            return Some(blocked(format!(
                "tool blocked by the request's tool allowlist: {name}"
            )));
        }
    }
    None
}

/// Run the extensions' `tool_call` hooks. Some output means the call ends
/// here: a hook blocked it or the turn was cancelled while they ran.
async fn hook_tool_call(
    h: &crate::extensions::ExtensionHost,
    name: &str,
    arguments: &str,
    cancel: &AtomicBool,
) -> Option<tools::ToolOutput> {
    // The hook chain is bounded per extension, but Esc must not wait out
    // even one silent hook's timeout: race it against the cancel flag.
    // Dropping the hook future also drops its pending-map entry.
    let hook = h.hook_tool_call(name, arguments);
    tokio::pin!(hook);
    let blocked = tokio::select! {
        verdict = &mut hook => verdict,
        _ = wait_cancelled(cancel) => {
            return Some(tool_output(
                "tool cancelled".into(),
                tools::ToolOutcome::Cancelled,
                "cancelled",
            ));
        }
    };
    blocked.map(|reason| {
        tool_output(
            format!("Tool call blocked by extension: {reason}"),
            tools::ToolOutcome::Blocked,
            "blocked",
        )
    })
}

/// Call the extension that owns `name`, streaming its progress as tool
/// output, and shape its result into a tool row.
async fn run_extension_tool(
    h: &crate::extensions::ExtensionHost,
    name: &str,
    arguments: &str,
    context: &ToolRunContext,
) -> tools::ToolOutput {
    let (events, id) = (&context.events, context.id);
    let (progress, mut updates) = mpsc::channel(64);
    let call = h.call_tool_streaming(name, arguments, progress);
    tokio::pin!(call);
    let result = loop {
        tokio::select! {
            result = &mut call => break result,
            update = updates.recv() => {
                if let Some(update) = update {
                    forward_extension_update(events, id, update).await;
                }
            }
            _ = wait_cancelled(&context.cancel) => {
                return tool_output(
                    "extension tool cancelled".into(),
                    tools::ToolOutcome::Cancelled,
                    "cancelled",
                );
            }
        }
    };
    // The response and the last queued update can become ready in
    // the same select tick. Preserve wire order by draining every
    // update the host accepted before publishing the final result.
    while let Ok(update) = updates.try_recv() {
        forward_extension_update(events, id, update).await;
    }
    // An extension tool may name the session as a side effect; the
    // UI applies it on SessionEvent::Named.
    if let Some(new_name) = result.session_name.clone() {
        let _ = events.send(SessionEvent::Named(new_name)).await;
    }
    extension_output(result, &context.tools)
}

/// An extension tool's result as a tool row. The row and the viewer take
/// the extension's shape when it gives one; the model still reads `content`
/// alone. A diff body is converted to the reference row grammar here so the
/// viewer paints it like a built-in edit's.
fn extension_output(
    result: crate::extensions::ToolResult,
    tool_runtime: &tools::ToolRuntime,
) -> tools::ToolOutput {
    let outcome = if result.is_error {
        tools::ToolOutcome::Failed
    } else {
        tools::ToolOutcome::Completed
    };
    let summary = result
        .summary
        .filter(|s| !s.trim().is_empty())
        .map(|s| tools::sanitize_display(&s))
        .unwrap_or_else(|| {
            if outcome.is_error() {
                "error".into()
            } else {
                "done".into()
            }
        });
    // `display` is the extension's text for the viewer: sanitized
    // like the summary, so a control sequence paints as characters.
    let display = match (result.format, result.display) {
        (crate::extensions::Format::Diff, body) => {
            let rows = tools::diffview::from_unified(&tools::sanitize_display(
                body.as_deref().unwrap_or(&result.content),
            ));
            (!rows.is_empty()).then(|| tools::truncate(rows))
        }
        (_, Some(body)) => Some(tools::truncate(tools::sanitize_display(&body))),
        (_, None) => None,
    };
    tools::ToolOutput {
        content: tool_runtime.cap(result.content),
        outcome,
        summary,
        display,
    }
}

/// Run a built-in tool on a blocking thread, previewing its live output.
async fn run_builtin(context: ToolRunContext, name: &str, arguments: &str) -> tools::ToolOutput {
    let ToolRunContext {
        tools: tool_runtime,
        cwd,
        cancel,
        id,
        events,
        ..
    } = context;
    let name = name.to_string();
    let arguments = arguments.to_string();
    tokio::task::spawn_blocking(move || {
        tool_runtime.run_streaming(&name, &arguments, &cwd, &cancel, |stream, chunk| {
            let chunk = tools::sanitize_display(chunk);
            if !chunk.is_empty() {
                // Live output is a preview. A slow consumer must not prevent
                // the command from checking its timeout or cancellation.
                // ToolEnd carries the retained output even if preview chunks drop.
                let _ = events.try_send(SessionEvent::ToolOutput { id, stream, chunk });
            }
        })
    })
    .await
    .unwrap_or(tool_output(
        "tool panicked".into(),
        tools::ToolOutcome::Failed,
        "error",
    ))
}

/// A tool row with no viewer body of its own.
fn tool_output(content: String, outcome: tools::ToolOutcome, summary: &str) -> tools::ToolOutput {
    tools::ToolOutput {
        content,
        outcome,
        summary: summary.into(),
        display: None,
    }
}

async fn forward_extension_update(
    events: &mpsc::Sender<SessionEvent>,
    id: u64,
    update: crate::extensions::ToolProgress,
) {
    let chunk = tools::sanitize_display(&update.chunk);
    if !chunk.is_empty() {
        let _ = events
            .send(SessionEvent::ToolOutput {
                id,
                stream: update.stream,
                chunk,
            })
            .await;
    }
}

#[cfg(test)]
mod option_tests {
    use super::*;

    #[tokio::test]
    async fn compaction_cancels_when_either_event_slot_is_backpressured() {
        for start_blocked in [false, true] {
            let (mut agent, _events) = Agent::with_options(
                crate::providers::catalog::builtin_catalog().remove(0),
                AgentOptions {
                    save_session: false,
                    ..AgentOptions::default()
                },
            );
            agent.load_history(vec![
                ChatMessage::user("x".repeat(10_000)),
                ChatMessage::user("recent"),
            ]);
            let mut log = agent.log();
            log.model.context_window = 8192;
            // Capacity must be reserved before any provider request can start.
            log.model.provider = "review-unconfigured".into();
            log.model.base_url = "http://127.0.0.1:0".into();
            let (events, _receiver) = mpsc::channel(1);
            if start_blocked {
                events.try_send(SessionEvent::TurnStart).unwrap();
            }
            log.events = events;
            let cancel = Arc::new(AtomicBool::new(false));
            let compaction = compact_log(&log, "system", &cancel, None, None);
            tokio::pin!(compaction);
            assert!(
                tokio::time::timeout(Duration::from_millis(20), &mut compaction)
                    .await
                    .is_err()
            );
            cancel.store(true, Ordering::SeqCst);
            let result = tokio::time::timeout(Duration::from_secs(1), compaction)
                .await
                .unwrap();
            assert_eq!(
                result.unwrap_err(),
                "compaction cancelled; history was preserved"
            );
            assert_eq!(agent.history_snapshot().len(), 2);
        }
    }

    #[test]
    fn checkpoint_installation_rejects_a_snapshot_missing_a_concurrent_commit() {
        let root = std::env::temp_dir().join(format!("e-compact-stale-{}", uuid::Uuid::new_v4()));
        for save_session in [false, true] {
            let home = root.join(save_session.to_string());
            let (agent, _events) = Agent::with_options(
                crate::providers::catalog::builtin_catalog().remove(0),
                AgentOptions {
                    home: Some(home.clone()),
                    cwd: Some(home),
                    save_session,
                    ..AgentOptions::default()
                },
            );
            let log = agent.log();
            log.append(ChatMessage::user("original")).unwrap();
            let snapshot = agent.history_snapshot();
            let original_path = agent.session_path();
            agent.record_user("shell output committed during summary".into());
            assert!(!log.load_compacted(
                "stale summary",
                None,
                vec![],
                &AtomicBool::new(false),
                &snapshot
            ));
            assert_eq!(
                agent.history_snapshot()[1].content,
                "shell output committed during summary"
            );
            assert_eq!(agent.session_path(), original_path);
            if let Some(path) = original_path {
                assert_eq!(
                    SessionLog::load(&path).unwrap()[1].content,
                    "shell output committed during summary"
                );
            }
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cancellation_during_checkpoint_preparation_keeps_the_original_session() {
        let home = std::env::temp_dir().join(format!("e-compact-cancel-{}", uuid::Uuid::new_v4()));
        let (agent, _events) = Agent::with_options(
            crate::providers::catalog::builtin_catalog().remove(0),
            AgentOptions {
                home: Some(home.clone()),
                cwd: Some(home.clone()),
                ..AgentOptions::default()
            },
        );
        let log = agent.log();
        log.append(ChatMessage::user("original")).unwrap();
        let original = agent.session_path().unwrap();
        let expected = agent.history_snapshot();
        let directory = original.parent().unwrap();
        // Hold preparation after the new log is created, before it can commit.
        let name = log.session_name.lock().unwrap();
        let worker = log.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let task = std::thread::spawn(move || {
            worker.load_compacted("summary", None, vec![], &worker_cancel, &expected)
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let logs = std::fs::read_dir(directory)
                .unwrap()
                .flatten()
                .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "jsonl"))
                .count();
            if logs == 2 {
                break;
            }
            assert!(Instant::now() < deadline, "checkpoint was not staged");
            std::thread::sleep(Duration::from_millis(5));
        }
        cancel.store(true, Ordering::SeqCst);
        drop(name);
        assert!(!task.join().unwrap());
        assert_eq!(agent.session_path().unwrap(), original);
        assert_eq!(agent.history_snapshot()[0].content, "original");
        assert_eq!(
            std::fs::read_dir(directory)
                .unwrap()
                .flatten()
                .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "jsonl"))
                .count(),
            1
        );
        drop(agent);
        drop(log);
        std::fs::remove_dir_all(home).unwrap();
    }

    #[test]
    fn no_save_commits_to_memory_without_opening_a_session() {
        let (agent, _events) = Agent::with_options(
            crate::providers::catalog::default_model(),
            AgentOptions {
                save_session: false,
                ..AgentOptions::default()
            },
        );
        agent
            .log()
            .append(ChatMessage::user("kept in memory"))
            .unwrap();

        assert_eq!(agent.history_snapshot().len(), 1);
        assert!(agent.session.lock().unwrap().is_none());
    }

    #[tokio::test]
    async fn a_panicking_turn_still_reports_one_terminal_event() {
        let (events, mut rx) = mpsc::channel(8);
        let worker = || {
            tokio::spawn(async {
                panic!("test turn panic");
            })
        };
        let aborted = supervise_turn(
            worker,
            events,
            Arc::new(Mutex::new(PendingQueue::default())),
            Arc::new(AtomicBool::new(false)),
        )
        .await;

        assert!(!aborted);
        assert!(matches!(rx.recv().await, Some(SessionEvent::TurnStart)));
        let error = rx.recv().await.expect("panic must be reported");
        assert!(
            matches!(error, SessionEvent::Error(message) if message.contains("test turn panic"))
        );
        assert!(matches!(
            rx.recv().await,
            Some(SessionEvent::TurnEnd { aborted: false })
        ));
        assert!(rx.try_recv().is_err(), "TurnEnd must be emitted once");
    }

    #[tokio::test]
    async fn a_steered_prompt_queues_whole_with_its_images() {
        let (mut agent, _rx) = Agent::with_options(
            crate::providers::catalog::builtin_catalog().remove(0),
            AgentOptions {
                save_session: false,
                ..AgentOptions::default()
            },
        );
        agent.pending.lock().unwrap().running = true;
        let held = agent.submit_message(
            ChatMessage::user_with_images(
                "what is in this screenshot",
                vec![crate::providers::ImageInput {
                    media_type: "image/png".into(),
                    data: std::sync::Arc::from("AA=="),
                }],
            ),
            String::new(),
        );
        assert!(held, "a running turn steers instead of starting");
        let pending = agent.pending.lock().unwrap();
        let (id, message) = &pending.items[0];
        assert_eq!(*id, 1);
        assert_eq!(message.content, "what is in this screenshot");
        let crate::providers::MessageKind::User { images, .. } = &message.kind else {
            panic!("a steered user prompt stays a user message");
        };
        assert_eq!(images.len(), 1, "attachments survive the queue");
    }

    #[tokio::test]
    async fn a_prompt_in_the_completion_gap_is_consumed_before_turn_end() {
        let (events, mut rx) = mpsc::channel(8);
        let queue = Arc::new(Mutex::new(PendingQueue {
            running: true,
            ..PendingQueue::default()
        }));
        let pending = queue.clone();
        let calls = Arc::new(AtomicU64::new(0));
        let count = calls.clone();
        let factory = move || {
            let pending = pending.clone();
            let count = count.clone();
            tokio::spawn(async move {
                let attempt = count.fetch_add(1, Ordering::SeqCst);
                let mut pending = pending.lock().unwrap();
                if attempt == 0 {
                    // The worker has decided to stop, but completion has not
                    // been published. A concurrent submit still belongs here.
                    pending.items.push((1, ChatMessage::user("late prompt")));
                } else {
                    assert_eq!(pending.items.remove(0).1.content, "late prompt");
                }
                turn::Outcome::Complete
            })
        };
        supervise_turn(
            factory,
            events,
            queue.clone(),
            Arc::new(AtomicBool::new(false)),
        )
        .await;
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert!(!queue.lock().unwrap().running);
        assert!(matches!(rx.recv().await, Some(SessionEvent::TurnStart)));
        assert!(matches!(
            rx.recv().await,
            Some(SessionEvent::TurnEnd { aborted: false })
        ));
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn execution_policy_blocks_a_disallowed_call_even_if_requested() {
        let (events, _rx) = mpsc::channel(1);
        let output = run_tool(
            ToolRunContext {
                tools: Arc::new(tools::ToolRuntime::default()),
                host: None,
                tool_mode: ToolMode::None,
                allowed_tools: None,
                active_tools: None,
                cwd: std::path::PathBuf::from("."),
                cancel: Arc::new(AtomicBool::new(false)),
                id: 1,
                events,
            },
            "bash",
            r#"{"command":"touch should-not-exist"}"#,
        )
        .await;

        assert_eq!(output.outcome, tools::ToolOutcome::Blocked);
        assert!(output.content.contains("no-tools"));
    }

    #[tokio::test]
    async fn request_allowlist_is_enforced_at_execution() {
        let (events, _rx) = mpsc::channel(1);
        let output = run_tool(
            ToolRunContext {
                tools: Arc::new(tools::ToolRuntime::default()),
                host: None,
                tool_mode: ToolMode::All,
                allowed_tools: Some(Arc::new(vec!["read".into(), "grep".into()])),
                active_tools: None,
                cwd: std::path::PathBuf::from("."),
                cancel: Arc::new(AtomicBool::new(false)),
                id: 1,
                events,
            },
            "bash",
            r#"{"command":"touch should-not-exist"}"#,
        )
        .await;

        assert_eq!(output.outcome, tools::ToolOutcome::Blocked);
        assert!(output.content.contains("tool allowlist"));
    }

    #[tokio::test]
    async fn steer_holds_nothing_while_idle() {
        let (mut agent, _events) = Agent::with_options(
            crate::providers::catalog::builtin_catalog().remove(0),
            AgentOptions {
                save_session: false,
                ..AgentOptions::default()
            },
        );
        assert!(!agent.steer("late".into()));
        assert!(agent.history_snapshot().is_empty());
        assert_eq!(agent.queued_count(), 0);
    }
}
