//! `Session`: one conversation, built once, prompted many times. The builder
//! resolves everything that can fail up front — model, effort, tool names,
//! a session file to resume — so a built session's only remaining failure
//! mode is a turn.
//!
//! Every read of the user's configuration runs inside the session's home
//! scope (`config::home::with_home`), never through the process environment:
//! two sessions with different homes coexist in one process.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::sync::mpsc;

use e::core::agent::{Agent, AgentOptions, SessionEvent};
use e::core::cli::ToolMode;
use e::core::config::home;
use e::core::extensions::ExtensionHost;
use e::core::providers::catalog::{self, Model};
use e::core::providers::{ChatMessage, ImageInput};
use e::core::session::{self as log, SessionLog};

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
        self.persist = persist;
        self
    }

    /// Start the home's extensions for their tools and hooks. Off by
    /// default: extensions are user-installed executables, and starting
    /// them is a decision for the host. Startup hooks and extension flags
    /// are CLI concerns and do not run here.
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
        let model = home::with_home(home.clone(), || resolve_model(self.model.as_deref()))?;
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
                if let Some(unknown) = names.iter().find(|name| !e::core::tools::is_builtin(name)) {
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

        if let Some(path) = &self.resume {
            // Ownership first: a file another e is appending to must not be
            // replayed into a second, diverging history.
            let (session, messages, name) = home::with_home(home.clone(), || {
                let session = SessionLog::reopen(path)?;
                let messages = SessionLog::load(path)?;
                Ok::<_, std::io::Error>((session, messages, log::name_of(path)))
            })?;
            agent.load_history(messages);
            agent.set_session(Some(session));
            agent.adopt_session_name(name);
        } else if !self.history.is_empty() {
            agent.load_history(self.history);
        }

        let (host, notices) = if self.extensions {
            let (sender, receiver) = mpsc::channel(256);
            let host = home::scope(home.clone(), ExtensionHost::start(sender)).await;
            agent.set_host(host.clone());
            (Some(host), Some(receiver))
        } else {
            (None, None)
        };

        Ok(Session {
            agent,
            events,
            notices,
            host,
            home,
            instructions: self.instructions,
            stale: false,
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
    /// Extension notices, merged into a running turn's stream; None when
    /// extensions are off.
    pub(crate) notices: Option<mpsc::Receiver<String>>,
    host: Option<Arc<ExtensionHost>>,
    home: PathBuf,
    instructions: Option<String>,
    /// A turn was dropped mid-run: its interrupted tail is still queued in
    /// `events`, and the next turn drains it to `TurnEnd` before submitting.
    pub(crate) stale: bool,
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

    /// The conversation so far, in e's persisted message shape.
    pub fn history(&self) -> Vec<Message> {
        self.agent.history_snapshot()
    }

    /// Forget the conversation. A persisted session starts a fresh file on
    /// the next prompt; the old one stays on disk.
    pub fn clear(&mut self) {
        self.agent.clear();
        self.agent.set_session(None);
        self.agent.clear_session_name();
    }

    /// The session log's path once a persisted conversation has its first
    /// message; None for memory-only sessions.
    pub fn path(&self) -> Option<PathBuf> {
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
        if let Some(host) = self.host.take() {
            host.shutdown().await;
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
