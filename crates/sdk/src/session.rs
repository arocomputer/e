//! `Session`: one conversation, built once, prompted many times. The builder
//! resolves everything that can fail up front — model, effort, tool names,
//! a session file to resume — so a built session's only remaining failure
//! mode is a turn.
//!
//! Every read of the user's configuration runs inside the session's home
//! scope (`config::home::with_home`), never through the process environment:
//! two sessions with different homes coexist in one process.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::sync::mpsc;

use e_core::agent::{Agent, AgentOptions, SessionEvent};
use e_core::cli::ToolMode;
use e_core::config::home;
use e_core::extensions::ExtensionHost;
use e_core::providers::catalog::{self, Model};
use e_core::providers::{ChatMessage, ImageInput};
use e_core::session::{self as log, SessionLog};

use crate::turn::{Start, Turn};
use crate::{Error, Message, SavedSession};

/// Which built-in tools a session advertises and runs. Extensions' tools
/// join `All`; `Only` is built-ins only, enforced at execution too — a
/// model that names a tool outside the list gets a blocked result.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Tools {
    #[default]
    All,
    None,
    Only(Vec<String>),
}

/// One user message: text plus any image attachments. Plain strings convert
/// directly, so `session.prompt("hi")` needs no wrapper.
#[derive(Clone, Debug, PartialEq)]
pub struct Prompt {
    pub(crate) text: String,
    pub(crate) images: Vec<ImageInput>,
}

impl Prompt {
    pub fn new(text: impl Into<String>) -> Self {
        Prompt {
            text: text.into(),
            images: Vec::new(),
        }
    }

    /// Attach an in-memory image.
    pub fn image(mut self, image: ImageInput) -> Self {
        self.images.push(image);
        self
    }

    /// Attach an image file (PNG, JPEG, GIF, or WebP; e's size limits apply).
    pub fn image_file(self, path: impl AsRef<Path>) -> Result<Self, Error> {
        let image = ImageInput::from_path(path.as_ref()).map_err(Error::Image)?;
        Ok(self.image(image))
    }
}

impl From<&str> for Prompt {
    fn from(text: &str) -> Self {
        Prompt::new(text)
    }
}

impl From<String> for Prompt {
    fn from(text: String) -> Self {
        Prompt::new(text)
    }
}

impl From<&String> for Prompt {
    fn from(text: &String) -> Self {
        Prompt::new(text.as_str())
    }
}

/// Configures a [`Session`]. Defaults: the process's working directory,
/// the user's e home (`E_HOME`, else `~/.e`), their configured default
/// model, every built-in tool, memory-only, no extensions.
#[derive(Clone, Debug, Default)]
pub struct SessionBuilder {
    cwd: Option<PathBuf>,
    home: Option<PathBuf>,
    model: Option<String>,
    effort: Option<String>,
    tools: Tools,
    persist: bool,
    extensions: bool,
    instructions: Option<String>,
    resume: Option<PathBuf>,
    history: Vec<Message>,
}

impl SessionBuilder {
    /// The workspace tools operate in and whose AGENTS.md and skills load.
    pub fn cwd(mut self, path: impl Into<PathBuf>) -> Self {
        self.cwd = Some(path.into());
        self
    }

    /// The configuration home to read (credentials, models.json, settings,
    /// AGENTS.md, extensions) and, when persisting, to write sessions under.
    /// Scoped to this session; the process environment is never changed.
    pub fn home(mut self, path: impl Into<PathBuf>) -> Self {
        self.home = Some(path.into());
        self
    }

    /// `provider/id`, a bare id, or a unique substring of an available
    /// model. Resolved at build against providers with credentials.
    pub fn model(mut self, query: impl Into<String>) -> Self {
        self.model = Some(query.into());
        self
    }

    /// Reasoning effort, one of the model's declared levels. Validated at
    /// build; without it the model's strong default applies.
    pub fn effort(mut self, level: impl Into<String>) -> Self {
        self.effort = Some(level.into());
        self
    }

    pub fn tools(mut self, tools: Tools) -> Self {
        self.tools = tools;
        self
    }

    /// Write the conversation to a JSONL session log under the home's
    /// `sessions/`, the same files `e -r` resumes. Off by default: an
    /// embedding must opt into leaving files in the user's home.
    pub fn persist(mut self, persist: bool) -> Self {
        // A resumed session must keep appending to the file it came from:
        // turning persistence off after `resume` would also release the
        // file's lock and let another process pick up the same log.
        self.persist = persist || self.resume.is_some();
        self
    }

    /// Start the home's extensions for their tools and hooks. Off by
    /// default: extensions are user-installed executables, and starting
    /// them is a decision for the host. They run in the session's `cwd` and
    /// see it at `initialize`. Startup hooks do not run, and no command-line
    /// flags are parsed for them: the host process's argv is not e's.
    pub fn extensions(mut self, enabled: bool) -> Self {
        self.extensions = enabled;
        self
    }

    /// Host instructions appended to e's own system prompt, after the
    /// skills catalog and project context. The base prompt stays e's so the
    /// agent keeps its tool discipline; replace it wholesale only through
    /// the home's `settings.json` `system_prompt`, as the terminal does.
    pub fn instructions(mut self, text: impl Into<String>) -> Self {
        self.instructions = Some(text.into());
        self
    }

    /// Continue a saved session file in place: its active branch becomes
    /// the history and new messages append to the same file. Implies
    /// `persist(true)` and takes the file's lock, so a build fails while
    /// another e has it open.
    pub fn resume(mut self, path: impl Into<PathBuf>) -> Self {
        self.resume = Some(path.into());
        self.persist = true;
        self
    }

    /// Seed the conversation with existing messages — a transcript read
    /// with [`crate::transcript`], or one the host stored itself. Ignored
    /// when `resume` is set, which loads from the file instead.
    pub fn history(mut self, messages: Vec<Message>) -> Self {
        self.history = messages;
        self
    }

    /// This workspace's saved sessions in this home, newest first.
    pub fn saved(&self) -> Vec<SavedSession> {
        let cwd = resolve_cwd(self.cwd.clone());
        home::with_home(resolve_home(self.home.clone()), || log::list(&cwd))
    }

    pub async fn build(self) -> Result<Session, Error> {
        let cwd = resolve_cwd(self.cwd);
        let home = resolve_home(self.home);
        // These checks read workspace metadata and home configuration, and
        // the persistence probe writes a temporary file. Run them on the
        // blocking pool so a slow disk cannot stall a current-thread runtime.
        let model_query = self.model.clone();
        let probe_cwd = cwd.clone();
        let blocking_home = home.clone();
        let persist = self.persist && self.resume.is_none();
        let model = tokio::task::spawn_blocking(move || -> Result<Model, Error> {
            let meta = std::fs::metadata(&probe_cwd).map_err(|error| Error::Cwd {
                path: probe_cwd.clone(),
                reason: error.to_string(),
            })?;
            if !meta.is_dir() {
                return Err(Error::Cwd {
                    path: probe_cwd,
                    reason: "not a directory".into(),
                });
            }
            home::with_home(blocking_home, || {
                let model = resolve_model(model_query.as_deref())?;
                // Persistence is checked up front like every other option: a
                // home that cannot create logs must fail here, not keep the
                // first prompt memory-only behind a warning.
                if persist {
                    SessionLog::preflight(&probe_cwd).map_err(Error::Session)?;
                }
                Ok(model)
            })
        })
        .await
        .map_err(|_| Error::Session(std::io::Error::other("session build task panicked")))??;
        if let Some(effort) = &self.effort {
            if !model.effort.iter().any(|level| level == effort) {
                return Err(Error::Effort {
                    model: catalog::slug(&model),
                    effort: effort.clone(),
                    supported: if model.effort.is_empty() {
                        "none".into()
                    } else {
                        model.effort.join(", ")
                    },
                });
            }
        }
        let (tool_mode, allowed_tools) = match self.tools {
            Tools::All => (ToolMode::All, None),
            Tools::None => (ToolMode::None, None),
            Tools::Only(names) => {
                if let Some(unknown) = names.iter().find(|name| !e_core::tools::is_builtin(name)) {
                    return Err(Error::UnknownTool(unknown.clone()));
                }
                (ToolMode::All, Some(names))
            }
        };
        let options = AgentOptions {
            cwd: Some(cwd),
            home: Some(home.clone()),
            save_session: self.persist,
            tool_mode,
            effort_override: self.effort,
            allowed_tools,
        };
        let (mut agent, events) = Agent::with_options(model, options);
        let cwd = agent.cwd();

        if let Some(path) = &self.resume {
            // Ownership first: a file another e is appending to must not be
            // replayed into a second, diverging history. The read and parse
            // run on the blocking pool: a large session must not stall the
            // executor (and every other future on a current-thread runtime).
            let resume_path = path.clone();
            let blocking_home = home.clone();
            let (session, messages, name) = tokio::task::spawn_blocking(move || {
                home::with_home(blocking_home, || {
                    let session = SessionLog::reopen(&resume_path)?;
                    let messages = SessionLog::load(&resume_path)?;
                    Ok::<_, std::io::Error>((session, messages, log::name_of(&resume_path)))
                })
            })
            .await
            .map_err(|_| Error::Session(std::io::Error::other("session load task panicked")))??;
            agent.load_history(messages);
            agent.set_session(Some(session));
            agent.adopt_session_name(name);
        } else if !self.history.is_empty() {
            let messages = self.history;
            if self.persist {
                let blocking_home = home.clone();
                let seed_cwd = cwd.clone();
                let model = agent.model_slug();
                let (session, messages) = tokio::task::spawn_blocking(move || {
                    home::with_home(blocking_home, || {
                        let session = SessionLog::create_with(&seed_cwd, &model, &messages)?;
                        Ok::<_, std::io::Error>((session, messages))
                    })
                })
                .await
                .map_err(|_| {
                    Error::Session(std::io::Error::other("session seed task panicked"))
                })??;
                agent.load_history(messages);
                agent.set_session(Some(session));
            } else {
                agent.load_history(messages);
            }
        }

        let (host, notices, startup_notices) = if self.extensions {
            let (sender, mut receiver) = mpsc::channel(256);
            // Startup diagnostics arrive on the bounded channel while the
            // host is still starting; read them as they come so a home with
            // many broken extensions cannot fill it and stall startup.
            let start = home::scope(
                home.clone(),
                ExtensionHost::start_in(sender, cwd, Vec::new(), None),
            );
            tokio::pin!(start);
            let mut startup_notices = VecDeque::new();
            let host = loop {
                tokio::select! {
                    host = &mut start => break host,
                    Some(notice) = receiver.recv() => startup_notices.push_back(notice),
                }
            };
            // The host reports a failed extension in the same poll it
            // finishes, so its last notices are still queued when the loop
            // ends. Left there, they would reach the first turn only when no
            // core event happened to be ready ahead of them.
            while let Ok(notice) = receiver.try_recv() {
                startup_notices.push_back(notice);
            }
            agent.set_host(host.clone());
            (Some(host), Some(receiver), startup_notices)
        } else {
            (None, None, VecDeque::new())
        };

        Ok(Session {
            agent,
            events,
            notices,
            startup_notices,
            host,
            home,
            instructions: self.instructions,
            stale: false,
            pending_clear: false,
        })
    }
}

fn resolve_cwd(cwd: Option<PathBuf>) -> PathBuf {
    let process_cwd = std::env::current_dir().unwrap_or_default();
    match cwd {
        Some(path) if path.is_absolute() => path,
        Some(path) => process_cwd.join(path),
        None => process_cwd,
    }
}

fn resolve_home(home: Option<PathBuf>) -> PathBuf {
    let path = home.unwrap_or_else(home::home);
    if path.is_absolute() {
        path
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    }
}

/// Runs inside the home scope. An explicit query must resolve; with none,
/// the configured default is used only when some provider is signed in.
fn resolve_model(query: Option<&str>) -> Result<Model, Error> {
    match query {
        Some(query) => catalog::resolve(query).ok_or_else(|| Error::ModelUnavailable(query.into())),
        None if catalog::available().is_empty() => Err(Error::NoProvider),
        None => Ok(catalog::default_model()),
    }
}

/// One conversation against one working directory. Build it with
/// [`Session::builder`], drive it with [`Session::prompt`].
pub struct Session {
    pub(crate) agent: Agent,
    pub(crate) events: mpsc::Receiver<SessionEvent>,
    /// Extension notices, delivered between a running turn's core events;
    /// None when extensions are off.
    pub(crate) notices: Option<mpsc::Receiver<String>>,
    /// Diagnostics extensions raised while starting, delivered before the
    /// next turn's first event.
    pub(crate) startup_notices: VecDeque<String>,
    host: Option<Arc<ExtensionHost>>,
    home: PathBuf,
    instructions: Option<String>,
    /// A turn was dropped mid-run: its interrupted tail is still queued in
    /// `events`, and the next turn drains it to `TurnEnd` before submitting.
    pub(crate) stale: bool,
    /// A `clear` that arrived while a dropped turn's final commits were
    /// still running: history and path are hidden from now on, and the
    /// agent's own reset is deferred to the next prompt.
    pub(crate) pending_clear: bool,
}

impl Session {
    pub fn builder() -> SessionBuilder {
        SessionBuilder::default()
    }

    /// Start a turn. Nothing is sent until the returned [`Turn`] is first
    /// polled; await it for just the [`crate::Reply`], or iterate its
    /// events first.
    pub fn prompt(&mut self, prompt: impl Into<Prompt>) -> Turn<'_> {
        let prompt = prompt.into();
        let start = if !prompt.images.is_empty() && !self.agent.model.image_input {
            Start::Refused(format!(
                "model `{}` is not declared image-capable",
                self.agent.model_slug()
            ))
        } else {
            Start::Prompt(ChatMessage::user_with_images(prompt.text, prompt.images))
        };
        Turn::new(self, start)
    }

    /// Summarize older history into a checkpoint now, instead of waiting for
    /// the context window to fill. The turn emits `Compacting` then
    /// `Compacted`, or fails with history intact; it sends no user message.
    pub fn compact(&mut self) -> Turn<'_> {
        Turn::new(self, Start::Compact)
    }

    /// The active model, as `provider/id`.
    pub fn model(&self) -> String {
        self.agent.model_slug()
    }

    /// Switch models between turns. History carries over; images an
    /// incapable model cannot accept are omitted from its requests.
    pub fn set_model(&mut self, query: &str) -> Result<(), Error> {
        let model = home::with_home(self.home.clone(), || resolve_model(Some(query)))?;
        self.agent.model = model;
        Ok(())
    }

    /// The reasoning effort the next request will use, if the model has one.
    pub fn effort(&self) -> Option<String> {
        self.agent.effort()
    }

    /// The conversation so far, in e's persisted message shape. Empty once
    /// `clear` has been called, even before a dropped turn's commits stop.
    pub fn history(&self) -> Vec<Message> {
        if self.pending_clear {
            return Vec::new();
        }
        self.agent.history_snapshot()
    }

    /// Forget the conversation. While a dropped turn's final commits may
    /// still be running, only the reset is deferred: the next prompt
    /// performs the agent's clear before starting the turn.
    pub fn clear(&mut self) {
        if self.agent.is_streaming() {
            self.agent.interrupt();
            self.stale = true;
            self.pending_clear = true;
        } else {
            self.reset_agent();
        }
    }

    /// Empty the agent's history and detach its session log.
    pub(crate) fn reset_agent(&mut self) {
        self.agent.clear();
        self.agent.set_session(None);
        self.agent.clear_session_name();
    }

    /// The session log's path once a persisted conversation has its first
    /// message; None for memory-only sessions, and while a `clear` waits for
    /// a dropped turn's final commits.
    pub fn path(&self) -> Option<PathBuf> {
        if self.pending_clear {
            return None;
        }
        self.agent.session_path()
    }

    pub fn cwd(&self) -> PathBuf {
        self.agent.cwd()
    }

    /// e's assembled prompt for this workspace and home, plus the host's
    /// instructions. Recomputed per turn so edits to AGENTS.md land.
    pub(crate) fn system_prompt(&self) -> String {
        let base = self.agent.system_prompt();
        match &self.instructions {
            Some(instructions) => format!("{base}\n\n{instructions}"),
            None => base,
        }
    }

    /// Stop any interrupted turn's leftovers and shut extensions down
    /// cleanly. Dropping a session does the same on a best-effort basis; a
    /// host that started extensions should prefer `close`.
    pub async fn close(mut self) {
        self.agent.interrupt();
        // Keep `host` in place until shutdown completes. If this future is
        // cancelled mid-await, `Drop` still finds the host and schedules its
        // fallback. Taking it first would leave every extension process
        // running with no one left to stop it.
        if let Some(host) = self.host.clone() {
            host.shutdown().await;
            self.host = None;
        }
    }
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("model", &self.model())
            .field("cwd", &self.cwd())
            .field("home", &self.home)
            .field("path", &self.path())
            .field("messages", &self.agent.history_snapshot().len())
            .field("extensions", &self.host.is_some())
            .finish()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // The agent's own drop interrupts a running turn and its background
        // processes. Extensions need an async shutdown; run it if a runtime
        // is still here to run it on.
        if let Some(host) = self.host.take() {
            if let Ok(handle) = tokio::runtime::Handle::try_current() {
                handle.spawn(async move { host.shutdown().await });
            }
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// A home with one keyed mock provider and one extension that exits
    /// before its handshake. No request is ever sent.
    fn home_with_broken_extension() -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let home = std::env::temp_dir().join(format!(
            "e-sdk-unit-broken-{}-{}",
            std::process::id(),
            uuid::Uuid::now_v7()
        ));
        std::fs::create_dir_all(home.join("extensions")).unwrap();
        std::fs::write(home.join("auth.json"), r#"{"mock":{"key":"k"}}"#).unwrap();
        std::fs::write(
            home.join("models.json"),
            r#"{"providers":{"mock":{"base_url":"http://127.0.0.1:1","api":"completions","models":["test"]}}}"#,
        )
        .unwrap();
        let script = home.join("extensions").join("broken");
        std::fs::write(&script, "#!/bin/sh\nexit 1\n").unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        home
    }

    // The host reports a failed extension in the same poll it finishes
    // starting, so a notice read only while the host starts is still queued
    // when build returns. It must be in the pre-turn queue, not left for the
    // turn to find between core events.
    #[tokio::test(flavor = "multi_thread")]
    async fn build_collects_a_failed_extensions_notice_before_the_first_turn() {
        let home = home_with_broken_extension();
        let session = Session::builder()
            .home(&home)
            .model("mock/test")
            .extensions(true)
            .build()
            .await
            .unwrap();
        let queued: Vec<_> = session.startup_notices.iter().cloned().collect();
        session.close().await;
        let _ = std::fs::remove_dir_all(&home);
        assert!(
            queued.iter().any(|notice| notice.contains("broken")),
            "the failed extension's notice must be collected by build, got: {queued:?}"
        );
    }
}
