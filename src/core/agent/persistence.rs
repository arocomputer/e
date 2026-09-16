//! Commit turn history and session logs while reporting persistence failures once.

use super::*;

/// One handle for everything a commit needs — history, the session log, and
/// the persistence-warning latch — so the turn loop appends a message with
/// one call instead of threading six parameters through every site.
#[derive(Clone)]
pub(super) struct TurnLog {
    pub(super) home: PathBuf,
    pub(super) history: Arc<Mutex<Vec<ChatMessage>>>,
    pub(super) session: Arc<Mutex<Option<SessionLog>>>,
    pub(super) cwd: PathBuf,
    pub(super) model: Model,
    pub(super) session_name: Arc<Mutex<Option<String>>>,
    pub(super) persist_warned: Arc<AtomicBool>,
    pub(super) events: mpsc::Sender<SessionEvent>,
    pub(super) save_session: bool,
}

impl TurnLog {
    /// Append a message to history and to the session log, creating the log
    /// on the first message. The in-memory turn always proceeds; a
    /// persistence failure warns once per episode (see `note_persist`) —
    /// silently losing history is the one thing this must never do.
    pub(super) fn commit(&self, message: ChatMessage) {
        let result = self.append(message);
        note_persist(&self.persist_warned, result, &self.events);
    }

    /// Same as `commit`, but for the turn loop's own async task: `append`
    /// does synchronous session-file I/O, which would otherwise block that
    /// task's tokio worker thread on every committed message. Running it on
    /// the blocking pool gives it the same treatment `run_tool` already
    /// gives the built-in tools' blocking work, instead of the turn loop
    /// being the one place that skips it.
    pub(super) async fn commit_async(&self, message: ChatMessage) {
        let log = self.clone();
        let result = match tokio::task::spawn_blocking(move || log.append(message)).await {
            Ok(result) => result,
            Err(_) => Err(std::io::Error::other("session append task panicked")),
        };
        note_persist(&self.persist_warned, result, &self.events);
    }

    /// Persist a provider response with no replayable message, honoring no-save mode.
    pub(super) async fn record_response(&self, response: providers::ResponseMeta) {
        if !self.save_session {
            return;
        }
        let log = self.clone();
        let result = tokio::task::spawn_blocking(move || {
            let mut session = log.session.lock().unwrap_or_else(|e| e.into_inner());
            match session.as_mut() {
                Some(session) => session.append_response(response),
                None => Ok(()),
            }
        })
        .await
        .unwrap_or_else(|_| Err(std::io::Error::other("response append task panicked")));
        note_persist(&self.persist_warned, result, &self.events);
    }

    /// Persist diagnostic metadata outside model history, honoring no-save mode.
    pub(super) async fn record_error(&self, details: failure::ErrorDetails) {
        if !self.save_session {
            return;
        }
        let log = self.clone();
        let result = tokio::task::spawn_blocking(move || {
            let mut session = log.session.lock().unwrap_or_else(|e| e.into_inner());
            match session.as_mut() {
                Some(session) => session.record_error(details),
                None => Ok(()),
            }
        })
        .await
        .unwrap_or_else(|_| Err(std::io::Error::other("diagnostic append task panicked")));
        note_persist(&self.persist_warned, result, &self.events);
    }

    pub(super) fn append(&self, message: ChatMessage) -> std::io::Result<()> {
        crate::core::config::home::with_home(self.home.clone(), || self.append_inner(message))
    }

    pub(super) fn append_inner(&self, message: ChatMessage) -> std::io::Result<()> {
        // Keep the history/session commit together relative to checkpoint swaps.
        let mut history = self.history.lock().unwrap_or_else(|e| e.into_inner());
        history.push(message.clone());
        if !self.save_session {
            return Ok(());
        }
        let mut guard = self.session.lock().unwrap_or_else(|e| e.into_inner());
        if guard.is_none() {
            let mut created = SessionLog::create(&self.cwd, &slug(&self.model))?;
            // A pending name applies before the first record. It is
            // best-effort: failing here must not discard the freshly created
            // log — dropping it would make the next commit open a different
            // file and strand every message already in memory outside any
            // session.
            if let Some(name) = self
                .session_name
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone()
            {
                let _ = created.set_name(&name);
            }
            *guard = Some(created);
        }
        match guard.as_mut() {
            Some(s) => s.append(&message),
            None => Ok(()),
        }
    }

    /// Replace history with the compaction seed plus the kept recent
    /// messages, writing them into a fresh session file so the compacted
    /// state is itself resumable; the old file stays untouched. The fresh
    /// log is created before the old one is detached: if creation fails,
    /// the old log stays attached and later turns append to it, so a crash
    /// resumes into the complete pre-compaction conversation instead of a
    /// new file holding only an unanchored tail. Blocking session I/O —
    /// callers run it off the async task (`Agent::load_compacted`).
    /// Installation requires the exact history supplied in `expected`.
    pub(super) fn load_compacted(
        &self,
        summary: &str,
        response: Option<providers::ResponseMeta>,
        kept: Vec<ChatMessage>,
        cancel: &AtomicBool,
        expected: &[ChatMessage],
    ) -> bool {
        crate::core::config::home::with_home(self.home.clone(), || {
            self.install_compacted(summary, response, kept, cancel, expected)
        })
    }

    /// Prepare the new log before checking cancellation at the commit boundary.
    pub(super) fn install_compacted(
        &self,
        summary: &str,
        response: Option<providers::ResponseMeta>,
        kept: Vec<ChatMessage>,
        cancel: &AtomicBool,
        expected: &[ChatMessage],
    ) -> bool {
        let mut seed_message = ChatMessage::user(crate::core::agent::compact::seed(summary));
        if let Some(response) = response {
            seed_message = seed_message.with_response(response);
        }
        let mut fresh_history = Vec::with_capacity(kept.len() + 1);
        fresh_history.push(seed_message.clone());
        fresh_history.extend(kept);

        // Same lock order as `commit`: history before session.
        let mut history_guard = self.history.lock().unwrap_or_else(|e| e.into_inner());
        // A summary only describes its snapshot. Concurrent commits must survive.
        if cancel.load(Ordering::SeqCst) || expected != history_guard.as_slice() {
            return false;
        }
        if !self.save_session {
            *history_guard = fresh_history;
            return true;
        }
        let mut guard = self.session.lock().unwrap_or_else(|e| e.into_inner());
        let result = match SessionLog::create(&self.cwd, &slug(&self.model)) {
            Ok(mut created) => {
                // Same best-effort pending-name application as `commit`.
                if let Some(name) = self
                    .session_name
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone()
                {
                    let _ = created.set_name(&name);
                }
                for message in &fresh_history {
                    if let Err(error) = created.append(message) {
                        // This file never became the active compacted log. Do
                        // not leave a plausible partial session in /resume.
                        let failed_path = created.path().to_path_buf();
                        drop(created);
                        let _ = std::fs::remove_file(failed_path);
                        note_persist(&self.persist_warned, Err(error), &self.events);
                        return false;
                    }
                }
                if cancel.load(Ordering::SeqCst) {
                    let path = created.path().to_path_buf();
                    drop(created);
                    let _ = std::fs::remove_file(path);
                    return false;
                }
                *guard = Some(created);
                *history_guard = fresh_history;
                Ok(())
            }
            Err(e) => Err(e),
        };
        let installed = result.is_ok();
        note_persist(&self.persist_warned, result, &self.events);
        installed
    }
}

/// Turn a commit result into at most one warning per failure episode: the
/// first failure warns, later ones stay quiet until a commit succeeds again.
/// The latch sets only when the warning was actually delivered — a full
/// channel at the first failure must not silently swallow the episode.
pub(super) fn note_persist(
    warned: &AtomicBool,
    result: std::io::Result<()>,
    events: &mpsc::Sender<SessionEvent>,
) {
    match result {
        Ok(()) => warned.store(false, Ordering::SeqCst),
        Err(e) => {
            if !warned.load(Ordering::SeqCst)
                && events
                    .try_send(SessionEvent::Warning(format!(
                        "session not saved: {e} — the conversation continues in memory only"
                    )))
                    .is_ok()
            {
                warned.store(true, Ordering::SeqCst);
            }
        }
    }
}
