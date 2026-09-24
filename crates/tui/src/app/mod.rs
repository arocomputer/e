//! The interactive frame: App state, key handling, and the paint loop.
//!
//! The binary (`main.rs`) owns CLI dispatch (`auth`, `ask`, `docs`, …) and
//! hands off here once a session should open.

mod input;

mod runtime;
pub use runtime::{run, Requests, RunOptions};

use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event as TermEvent, KeyCode, KeyEvent, KeyModifiers, KeyboardEnhancementFlags,
    PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::{execute, terminal};
use std::io::Write;
use std::time::{Duration, Instant};

use crate::authpanel::{self, AuthStage};
use crate::background::stdout_is_tty;
use crate::composer::{Editor, EditorResult, Key};
use crate::menu::{
    Menu, MenuItem, MenuKind, HINT_MODELS, HINT_SCOPED, HINT_SESSIONS, HINT_SKILLS, HINT_USE,
};
use crate::screen::Painter;
use crate::statusline::{
    statusline, RecoveredStatus, RetryStatus, StatusData, Turn, TurnPhase, RECOVERED_VISIBLE_MS,
};
use crate::theme::Theme;
use crate::transcript::{Block, Kind, Transcript};
use crate::trustpanel::{self, TrustStage};
use ulo_core::agent::{Agent, AgentOptions, SessionEvent};
use ulo_core::output::{format_duration, format_tokens};
use ulo_core::providers::catalog::{self as model, Model};

mod clipboard;
mod conversation;

mod events;
mod extui;
mod frame;
mod sessions;
pub(crate) use extui::chord_of;
mod login;
mod menus;
mod viewer;
use viewer::Viewer;

/// Per-turn frontend bookkeeping; the engine state lives in the Agent.
struct ActiveTurn {
    /// The current assistant text block, if one is streaming.
    block: Option<usize>,
    /// The live thinking block for the current burst, if reasoning has
    /// streamed. Ending a burst detaches this so the next reasoning opens a
    /// fresh block. Finished thoughts retain their source and display mode;
    /// this index is only the open segment.
    thinking_block: Option<usize>,
    turn: Turn,
    started: Instant,
    error: Option<String>,
    error_summary: Option<String>,
    /// tool id → stable group block, so lifecycle events update in place.
    tool_blocks: std::collections::HashMap<u64, usize>,
    /// tool id → the tool's name, for the `render` hook's subject.
    tool_names: std::collections::HashMap<u64, String>,
    /// Batch members not yet terminal, including pending calls.
    pending_tools: usize,
    /// Set when the turn was stopped because the device slept past the
    /// resume window: the stop line is already in the transcript, so the
    /// cancelled row is suppressed at TurnEnd.
    sleep_stopped: bool,
    /// Accumulated provider-billed estimate for this turn, when the model
    /// declares rates. Unlike the context gauge, every request step counts.
    cost_usd: Option<f64>,
}

/// The queued-prompt review's working state: a keyed snapshot of the
/// queue (oldest first), which entry the composer holds, and whether a
/// draft is showing at all (↓ past the newest hides it). The turn keeps
/// steering meanwhile — an entry it already drained commits as a fresh
/// prompt instead of resurrecting the sent text.
struct QueueReview {
    entries: Vec<(u64, String)>,
    dirty: Vec<bool>,
    selected: usize,
    visible: bool,
}

/// Asynchronous work landing back in the frame loop.
enum AppJob {
    /// An input hook's verdict on a submitted line: consume/replace/notice.
    /// Images from `-i` or the composer clipboard ride through the text hook;
    /// the hook never sees their bytes, but they still attach to whatever text
    /// its verdict submits.
    InputVerdict {
        sequence: u64,
        text: String,
        images: Option<Vec<ulo_core::providers::ImageInput>>,
        verdict: ulo_core::extensions::InputVerdict,
    },
    /// A finished `!` shell command: what ran and what it printed. Tagged
    /// with the session epoch it started in.
    Shell {
        cmd: String,
        output: ulo_core::tools::ToolOutput,
        epoch: u64,
    },
    /// An extension command or shortcut finished. Tagged with the session
    /// epoch it started in.
    Command {
        result: ulo_core::extensions::CommandResult,
        epoch: u64,
    },
    /// Argument completions for `/command prefix` arrived; shown only if the
    /// composer still says exactly that.
    Completions {
        command: String,
        prefix: String,
        items: Vec<ulo_core::extensions::Completion>,
    },
    /// A /reload finished: the restarted extension host.
    Reloaded(std::sync::Arc<ulo_core::extensions::ExtensionHost>),
    /// An extension's `render` hook answered for a finished entry. Tagged
    /// with the session epoch; a late answer for a session that moved on
    /// is dropped.
    Rendered {
        target: RenderTarget,
        show: ulo_core::extensions::Show,
        epoch: u64,
    },
    /// The background updater installed a new version.
    Updated(String),
    /// Clipboard content read asynchronously, tied to the draft that requested it.
    ClipboardPaste {
        generation: u64,
        paste: Result<clipboard::Paste, String>,
        /// The pasted text to restore when a path attachment cannot load —
        /// a clipboard read has nothing to restore, a paste does.
        fallback: Option<String>,
    },
    /// A provider model-list refresh finished; rebuild an open picker.
    CatalogRefreshed,
}

/// Which finished entry a `render` hook answer belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RenderTarget {
    /// A tool's stored output, by output id.
    Tool(u64),
    /// A completed reply, by transcript index and its length when asked,
    /// so a rebuilt transcript never takes a stale body.
    Assistant { index: usize, len: usize },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InputRoute {
    ApiKey,
    Hook,
    Direct,
}

fn input_route(awaiting_api_key: bool, has_input_hook: bool) -> InputRoute {
    match (awaiting_api_key, has_input_hook) {
        (true, _) => InputRoute::ApiKey,
        (false, true) => InputRoute::Hook,
        (false, false) => InputRoute::Direct,
    }
}

/// Input hooks run concurrently so a slow extension does not block the frame,
/// but their verdicts must be applied in submission order. Otherwise a fast
/// second line can overtake a slow first one and reverse the conversation.
type InputVerdictItem = (
    String,
    Option<Vec<ulo_core::providers::ImageInput>>,
    ulo_core::extensions::InputVerdict,
);

#[derive(Default)]
struct PendingInputVerdicts {
    next_sequence: u64,
    next_to_apply: u64,
    ready: std::collections::BTreeMap<u64, InputVerdictItem>,
}

impl PendingInputVerdicts {
    fn reserve(&mut self) -> u64 {
        let sequence = self.next_sequence;
        self.next_sequence += 1;
        sequence
    }

    fn complete(
        &mut self,
        sequence: u64,
        text: String,
        images: Option<Vec<ulo_core::providers::ImageInput>>,
        verdict: ulo_core::extensions::InputVerdict,
    ) -> Vec<InputVerdictItem> {
        self.ready.insert(sequence, (text, images, verdict));
        let mut ordered = Vec::new();
        while let Some(item) = self.ready.remove(&self.next_to_apply) {
            ordered.push(item);
            self.next_to_apply += 1;
        }
        ordered
    }
}

struct ActiveLogin {
    flow_id: u64,
    cancellation: ulo_core::auth::login::LoginCancellation,
    task: tokio::task::JoinHandle<()>,
    wait_for_callback: bool,
}

impl Drop for ActiveLogin {
    fn drop(&mut self) {
        self.cancellation.cancel();
        self.task.abort();
        if self.wait_for_callback {
            ulo_core::auth::login::wait_for_callback_release();
        }
    }
}

struct App {
    theme: Theme,
    /// The composer's chord overrides from `~/.ulo/keybindings.json`. Reread
    /// alongside the theme — startup, /settings close, /reload — never
    /// mid-keystroke.
    keymap: crate::keybindings::Keymap,
    transcript: Transcript,
    editor: Editor,
    attachments: input::Attachments,
    agent: Agent,
    active: Option<ActiveTurn>,
    overlay: Option<String>,
    armed_at: Option<Instant>,
    should_quit: bool,
    /// Prompt-side tokens of the latest request ≈ current context size.
    context_tokens: u64,
    /// A provider awaiting a pasted API key; the next submit is the secret.
    pending_key: Option<String>,
    /// The open picker, if any — commands, files, models.
    menu: Option<Menu>,
    /// The scoped-models picker's staged scope: what Space has toggled but
    /// Ctrl+S has not yet committed. None when not staging (the picker shows
    /// the saved scope); Some when the picker is open with edits pending.
    staged_scope: Option<Vec<String>>,
    /// The sign-in panel, when /login is active.
    auth: Option<AuthStage>,
    /// The settings panel, when /settings is active.
    settings: Option<crate::settingspanel::SettingsPanel>,
    /// Whether retained thinking is expanded in the main transcript.
    show_thinking: bool,
    thinking_hint: String,
    /// Background job narration (login flows) into the transcript.
    jobs: tokio::sync::mpsc::Sender<String>,
    /// How a login flow ended; control flow reads this, never the notices.
    logins: tokio::sync::mpsc::Sender<ulo_core::auth::login::Outcome>,
    /// The owned OAuth task; dropping it cancels polling and callback waits.
    login_task: Option<ActiveLogin>,
    /// Monotonic identity used to ignore a canceled flow's queued outcome.
    login_sequence: u64,
    /// Extension host; commands and prompts come back on `results`.
    host: std::sync::Arc<ulo_core::extensions::ExtensionHost>,
    results: tokio::sync::mpsc::Sender<AppJob>,
    /// Completed input-hook calls waiting for earlier submissions to finish.
    input_verdicts: PendingInputVerdicts,
    /// A /compact summary is being generated; cleared when it lands or fails.
    compacting: bool,
    /// Messages typed while compacting; submitted once the swap lands.
    held_prompts: Vec<String>,
    /// First visit to this directory: the trust question, until answered.
    trust: Option<TrustStage>,
    /// A command-line prompt held until the first-visit trust choice is
    /// persisted, so its system prompt reflects that choice.
    pending_initial: Option<String>,
    pending_initial_images: Vec<ulo_core::providers::ImageInput>,
    /// Transcript index of the running `!` block, updated on completion.
    shell_block: Option<usize>,
    /// A /reload is restarting the extension host; prompts are held.
    reloading: bool,
    /// Transcript index of the reload notice, replaced when reload finishes.
    reload_block: Option<usize>,
    /// Full tool outputs for the ctrl+o review screen: (id, title, content),
    /// newest last, capped; ids link tool children to their details.
    outputs: Vec<(u64, String, String)>,
    output_seq: u64,
    /// The ctrl+o full-detail viewer, when open.
    viewer: Option<Viewer>,
    /// Main transcript row at the top of a paused view; None follows the tail.
    conversation_scroll: Option<usize>,
    scroll_lines: usize,
    scroll_hint: String,
    /// The review screen's projected rows, cached between frames: the
    /// projection only rebuilds when the transcript or the output store
    /// changed (the cache's fingerprint), or the width or depth moved —
    /// not on every 33ms paint.
    viewer_cache: Option<(u64, usize, bool, Vec<String>)>,
    /// The queued-prompt review: ↑ on an empty composer while prompts wait
    /// loads the newest into the composer for editing; the turn keeps
    /// steering while the review edits the queue.
    queue_review: Option<QueueReview>,
    /// Bumped whenever session identity changes (/new, resume). Async work
    /// launched in one epoch may not mutate a later one: a late extension
    /// command or shell result carries the epoch it started in and is
    /// dropped on mismatch.
    session_epoch: u64,
    /// A new version is installed on disk; /reload switches to it.
    update_installed: Option<String>,
    /// Exit the loop and exec the (updated) binary with -c.
    relaunch: bool,
    /// The latest frame has waited on the paint thread long enough to make
    /// terminal output, rather than provider work, the current bottleneck.
    rendering_delayed: bool,
    /// Dedupe one paint-failure episode while retries keep posting frames.
    last_paint_failure: Option<String>,
    /// OSC-11 background detection, probed once at startup before the
    /// event stream owns stdin. Re-probing mid-session would block the
    /// loop and swallow keystrokes, so a changed terminal background
    /// applies on restart.
    light_background: bool,
    /// Cached statusline inputs. Deriving them reads `~/.ulo/auth.json` and
    /// `~/.ulo/settings.json`; doing that per frame stalls streaming, so
    /// they refresh only via `refresh_status_cache`.
    bottom_pinned: bool,
    live_preview_rows: usize,
    tool_label_rows: usize,
    tool_history_limit: usize,
    tool_history_hint: String,
    signed_in: bool,
    status_effort: Option<String>,
    /// Where extensions' `ui.*` / `session.*` requests arrive; handed to
    /// every host this session starts (launch, /reload).
    requests: tokio::sync::mpsc::Sender<ulo_core::extensions::HostRequest>,
    /// Modal requests waiting for the footer to be free, first-come.
    ui_queue: extui::UiQueue,
    /// The open modal request, if any.
    ui_prompt: Option<extui::UiPrompt>,
    /// Each extension's `ui.status` text, by extension name.
    ext_status: std::collections::BTreeMap<String, String>,
    /// Each extension's `ui.activity` text, the `{activity}` token of the
    /// row below the transcript.
    ext_activity: std::collections::BTreeMap<String, String>,
    /// The extension panel below the composer, one slot.
    ext_panel: Option<extui::ExtPanel>,
    /// The side pane an extension opened, one at a time.
    pane: Option<crate::pane::Pane>,
    /// Set by each paint: the pane is open but off screen (too narrow,
    /// unfocused), so the status row says how to reach it.
    pane_hidden: bool,
    /// Extensions' widget rows above the composer, by `extension/key`.
    widgets: std::collections::BTreeMap<String, Vec<Vec<extui::Span>>>,
    /// Where the regions go and what the status row says
    /// (`~/.ulo/layout.json`), reread with the theme and keymap.
    layout: ulo_core::config::layout::Layout,
    /// ctrl+g was pressed: the frame loop hands the terminal to the
    /// external editor before its next select.
    external_edit: bool,
}

impl App {
    /// ctrl+p / ctrl+shift+p: cycle through the scope (or all available
    /// models when no scope is set), persisting the switch. The statusline is
    /// the feedback — it shows the new model immediately.
    fn cycle_model(&mut self, forward: bool) {
        let pool = model::cycle_pool();
        if pool.len() <= 1 {
            let scoped = model::scope().map(|s| !s.is_empty()).unwrap_or(false);
            self.notice(
                if scoped && pool.is_empty() {
                    "no scoped models are currently available; your saved scope is unchanged"
                } else if scoped {
                    "only one model in scope"
                } else {
                    "only one model available"
                }
                .into(),
            );
            return;
        }
        let current = self.agent.model_slug();
        let idx = pool
            .iter()
            .position(|m| model::slug(m) == current)
            .unwrap_or(0);
        let next = if forward {
            (idx + 1) % pool.len()
        } else {
            (idx + pool.len() - 1) % pool.len()
        };
        if let Err(error) = persist_model(&pool[next]) {
            self.notice(format!("could not save model choice: {error}"));
            return;
        }
        self.agent.model = pool[next].clone();
        self.refresh_status_cache();
        self.emit(
            "model_change",
            serde_json::json!({"model": self.agent.model_slug()}),
        );
    }

    /// Queued-prompt review keys, the reference's grammar: ↑ on an empty
    /// composer while prompts wait opens the newest for editing (queue
    /// draining pauses); ↑/↓ step older/newer, ↓ past the newest hides the
    /// draft; Enter commits edits back to the queue and resumes — an empty
    /// draft leaves its entry unchanged; Backspace on an emptied draft
    /// deletes the entry. Returns true when the key was consumed.
    fn queue_review_key(&mut self, code: KeyCode) -> bool {
        if self.trust.is_some()
            || self.auth.is_some()
            || self.settings.is_some()
            || self.menu.is_some()
        {
            return false;
        }
        let Some(mut review) = self.queue_review.take() else {
            if code == KeyCode::Up && self.editor.is_empty() && self.active.is_some() {
                let entries = self.agent.queue_snapshot();
                let Some(selected) = entries.len().checked_sub(1) else {
                    return false;
                };
                let dirty = vec![false; entries.len()];
                self.editor.set_text(&entries[selected].1);
                self.queue_review = Some(QueueReview {
                    entries,
                    dirty,
                    selected,
                    visible: true,
                });
                return true;
            }
            return false;
        };
        let stash = |review: &mut QueueReview, text: String| {
            if review.entries[review.selected].1 != text {
                review.entries[review.selected].1 = text;
                review.dirty[review.selected] = true;
            }
        };
        let consumed = match code {
            KeyCode::Up => {
                if review.visible {
                    stash(&mut review, self.editor.expanded_text());
                    if review.selected > 0 {
                        review.selected -= 1;
                        self.editor.set_text(&review.entries[review.selected].1);
                    }
                    true
                } else if self.editor.is_empty() {
                    review.visible = true;
                    self.editor.set_text(&review.entries[review.selected].1);
                    true
                } else {
                    false
                }
            }
            KeyCode::Down if review.visible => {
                stash(&mut review, self.editor.expanded_text());
                if review.selected + 1 < review.entries.len() {
                    review.selected += 1;
                    self.editor.set_text(&review.entries[review.selected].1);
                } else {
                    self.editor.set_text("");
                    review.visible = false;
                }
                true
            }
            // Enter with the draft hidden and new text typed is a fresh
            // prompt: fall through so the ordinary submit takes it (and
            // closes the review).
            KeyCode::Enter if review.visible || self.editor.is_empty() => {
                // The visible draft commits only when it holds text — an
                // emptied draft sends its entry unchanged.
                if review.visible && !self.editor.is_empty() {
                    stash(&mut review, self.editor.expanded_text());
                }
                if review.dirty.iter().any(|d| *d) {
                    // Only edited entries rewrite: a trim drops an entry that
                    // emptied, and leaves untouched entries verbatim — a
                    // multi-line prompt's trailing newline is not the user's
                    // doing. An edit to an entry the turn already drained
                    // lands as a fresh prompt, not a resurrection.
                    let mut edits = Vec::new();
                    let mut removed = Vec::new();
                    for ((key, entry), dirty) in review.entries.iter().zip(&review.dirty) {
                        if !dirty {
                            continue;
                        }
                        match entry.trim() {
                            "" => removed.push(*key),
                            trimmed => edits.push((*key, trimmed.to_string())),
                        }
                    }
                    self.agent.update_queued(edits, removed);
                }
                self.editor.set_text("");
                return true;
            }
            KeyCode::Backspace if review.visible && self.editor.is_empty() => {
                let (key, _) = review.entries.remove(review.selected);
                review.dirty.remove(review.selected);
                self.agent.update_queued(Vec::new(), vec![key]);
                if review.entries.is_empty() {
                    return true;
                }
                review.selected = review.selected.min(review.entries.len() - 1);
                self.editor.set_text(&review.entries[review.selected].1);
                true
            }
            _ => false,
        };
        self.queue_review = Some(review);
        consumed
    }

    /// Close the review without committing the visible draft. The draft is
    /// discarded with the review; the queue was never paused, so there is
    /// nothing to resume.
    fn close_queue_review(&mut self) {
        if self.queue_review.take().is_some() {
            self.editor.set_text("");
        }
    }

    fn open_settings(&mut self) {
        self.menu = None;
        self.settings = Some(crate::settingspanel::SettingsPanel::new(
            self.agent.effort_levels(),
        ));
    }

    fn dispatch_command(&mut self, command: String) {
        match command.as_str() {
            "/login" => self.open_login_menu(),
            "/models" | "/model" => self.open_model_menu(),
            "/scoped-models" => self.open_scoped_menu(),
            "/reload" => self.reload(),
            "/settings" => self.open_settings(),
            "/resume" => self.open_resume_menu(),
            "/copy" => self.copy_last(),
            other => self.submit(other.to_string()),
        }
    }

    /// `!cmd`: run it through the bash tool off-task; the result arrives as
    /// AppJob::Shell. Idle only — mid-turn the history is the model's.
    fn run_shell(&mut self, cmd: String) {
        if self.agent.is_streaming() || self.compacting {
            self.notice("busy — run shell commands between turns".into());
            return;
        }
        if self.shell_block.is_some() {
            self.notice("a shell command is still running".into());
            return;
        }
        self.transcript.push(Block::new(Kind::Shell, cmd.clone()));
        self.shell_block = Some(self.transcript.blocks.len() - 1);
        let results = self.results.clone();
        let cwd = self.agent.cwd();
        let epoch = self.session_epoch;
        ulo_core::config::home::spawn(async move {
            let shell_cmd = cmd.clone();
            let home = ulo_core::config::home::home();
            let output = tokio::task::spawn_blocking(move || {
                ulo_core::config::home::with_home(home, || {
                    ulo_core::tools::run_shell(&shell_cmd, &cwd)
                })
            })
            .await
            .unwrap_or(ulo_core::tools::ToolOutput {
                content: "shell command panicked".into(),
                outcome: ulo_core::tools::ToolOutcome::Failed,
                summary: "error".into(),
                display: None,
            });
            let _ = results.send(AppJob::Shell { cmd, output, epoch }).await;
        });
    }

    /// Store a full tool output for the review screen, returning its stable
    /// id. Eviction under the budget leaves a dangling id behind — the
    /// screen then says "Full saved result unavailable.", honestly.
    fn remember_output(&mut self, title: String, content: String) -> u64 {
        const OUTPUT_BUDGET: usize = 4 * 1024 * 1024;
        self.output_seq += 1;
        let id = self.output_seq;
        self.outputs.push((id, title, content));
        let mut bytes: usize = self.outputs.iter().map(|(_, _, body)| body.len()).sum();
        while self.outputs.len() > 1 && (self.outputs.len() > 50 || bytes > OUTPUT_BUDGET) {
            let removed = self.outputs.remove(0).2.len();
            bytes = bytes.saturating_sub(removed);
        }
        id
    }

    /// Ask the extensions that render `subject` for a body to show instead
    /// of `content`, off the loop; the answer comes back as a job.
    fn request_render(&self, subject: &str, name: &str, content: &str, target: RenderTarget) {
        if !self.host.renders(subject) {
            return;
        }
        let host = self.host.clone();
        let results = self.results.clone();
        let epoch = self.session_epoch;
        let (subject, name, content) = (subject.to_string(), name.to_string(), content.to_string());
        ulo_core::config::home::spawn(async move {
            if let Some(show) = host.hook_render(&subject, &name, &content).await {
                let _ = results
                    .send(AppJob::Rendered {
                        target,
                        show,
                        epoch,
                    })
                    .await;
            }
        });
    }

    /// A `render` answer lands: a tool's stored output takes the body (a
    /// diff in ulo's row grammar), a reply takes it as its markdown.
    fn apply_render(&mut self, target: RenderTarget, show: ulo_core::extensions::Show, epoch: u64) {
        if epoch != self.session_epoch {
            return;
        }
        let body = ulo_core::tools::sanitize_display(&show.body);
        match target {
            RenderTarget::Tool(id) => {
                let body = match show.format {
                    ulo_core::extensions::Format::Diff => {
                        ulo_core::tools::diffview::from_unified(&body)
                    }
                    _ => body,
                };
                if let Some(entry) = self.outputs.iter_mut().find(|(oid, _, _)| *oid == id) {
                    entry.2 = body;
                    self.viewer_cache = None;
                }
            }
            RenderTarget::Assistant { index, len } => {
                if let Some(block) = self.transcript.blocks.get_mut(index) {
                    if block.kind == Kind::Assistant && block.text.len() == len {
                        block.text = match show.format {
                            ulo_core::extensions::Format::Diff => {
                                format!("```diff\n{body}\n```")
                            }
                            _ => body,
                        };
                        block.touch();
                    }
                }
            }
        }
    }

    fn output_body(outputs: &[(u64, String, String)], id: u64) -> Option<&str> {
        outputs
            .iter()
            .find(|(stored, _, _)| *stored == id)
            .map(|(_, _, body)| body.as_str())
    }

    /// /reload, the reference behavior: refresh what a session caches. In ulo
    /// that is the extension host (restarted) and the theme (re-resolved) —
    /// skills, prompts, AGENTS.md, settings, and models.json are read fresh
    /// on every use already.
    /// A just-trusted repository's `.ulo/packages` may list packages not on
    /// disk: install them now, in the background, and say so — the one
    /// moment trust and a network fetch belong together. The result lands
    /// as a notice; `/reload` picks the packages up.
    fn install_project_packages(&mut self) {
        let cwd = self.agent.cwd().to_path_buf();
        let missing = ulo_core::resources::packages::project_missing(&cwd);
        if missing.is_empty() {
            return;
        }
        self.notice(format!(
            "installing {} from .ulo/packages…",
            match missing.len() {
                1 => "1 package".to_string(),
                n => format!("{n} packages"),
            }
        ));
        let results = self.results.clone();
        let epoch = self.session_epoch;
        ulo_core::config::home::spawn(async move {
            let outcomes = ulo_core::resources::packages::install_project(&cwd).await;
            let failed = outcomes.iter().filter(|r| r.is_err()).count();
            let lines: Vec<String> = outcomes
                .into_iter()
                .map(|r| r.unwrap_or_else(|ulo| ulo))
                .collect();
            let notice = if failed == 0 {
                format!("{} — /reload to use them", lines.join("; "))
            } else {
                format!("{} — fix and run `ulo install`", lines.join("; "))
            };
            let result = ulo_core::extensions::CommandResult {
                notice: Some(notice),
                show: None,
                prompt: None,
                session_name: None,
            };
            let _ = results.send(AppJob::Command { result, epoch }).await;
        });
    }

    fn reload(&mut self) {
        if self.agent.is_streaming() {
            self.notice("wait for the turn to finish before /reload".into());
            return;
        }
        if self.compacting {
            self.notice("wait for compaction to finish before /reload".into());
            return;
        }
        if self.reloading {
            return;
        }
        // With a freshly installed update on disk, /reload becomes the
        // switch: exit through the normal cleanup and exec the new binary
        // with -c, which resumes this session.
        if self.update_installed.is_some() {
            self.relaunch = true;
            self.should_quit = true;
            return;
        }
        self.reloading = true;
        self.reload_block = Some(self.transcript.push(Block::new(Kind::Notice, "reloading…")));
        // The old host's surfaces die with it; a modal it was waiting on is
        // answered "cancelled" by the drop.
        self.ui_queue.clear();
        self.cancel_ui_prompt();
        if self
            .menu
            .as_ref()
            .is_some_and(|m| m.kind == MenuKind::Extension)
        {
            self.menu = None;
        }
        self.ext_panel = None;
        self.pane = None;
        self.widgets.clear();
        self.ext_status.clear();
        self.ext_activity.clear();
        let old = self.host.clone();
        let jobs = self.jobs.clone();
        let results = self.results.clone();
        let requests = self.requests.clone();
        let path = self.agent.session_path().map(|p| p.display().to_string());
        ulo_core::config::home::spawn(async move {
            old.event("session_shutdown", serde_json::json!({"reason": "reload"}))
                .await;
            old.shutdown().await;
            let host = ulo_core::extensions::ExtensionHost::start(jobs, Some(requests)).await;
            host.event(
                "session_start",
                serde_json::json!({"reason": "reload", "path": path}),
            )
            .await;
            let _ = results.send(AppJob::Reloaded(host)).await;
        });
    }

    fn copy_last(&mut self) {
        let last = self
            .agent
            .history_snapshot()
            .iter()
            .rev()
            .find(|m| m.role() == "assistant" && !m.content.trim().is_empty())
            .map(|m| m.content.clone());
        match last {
            Some(text) => {
                // OSC 52: the terminal-native clipboard, no helper binary,
                // works over ssh too. Terminals without it silently ignore
                // the sequence.
                use base64::Engine;
                let encoded = base64::engine::general_purpose::STANDARD.encode(text.as_bytes());
                let ok = write!(std::io::stdout(), "\x1b]52;c;{encoded}\x07").is_ok();
                let _ = std::io::stdout().flush();
                self.notice(if ok {
                    "copied the last reply".into()
                } else {
                    "copy failed".into()
                });
            }
            None => self.notice("nothing to copy yet".into()),
        }
    }

    /// Reload the theme from settings, using the startup background probe.
    fn apply_theme(&mut self) {
        self.theme =
            crate::theme::resolve(&ulo_core::config::settings::theme(), self.light_background);
        self.transcript.invalidate();
    }

    /// Re-read `~/.ulo/keybindings.json`. A malformed or missing file fails
    /// open to no overrides — never an error that blocks typing.
    fn apply_keymap(&mut self) {
        self.keymap = crate::keybindings::load();
        self.layout = ulo_core::config::layout::load();
    }

    /// Refresh cached sign-in, effort, and layout preferences from disk.
    /// Call after sign-in, model switches, effort cycles, settings changes,
    /// and /reload.
    fn refresh_status_cache(&mut self) {
        self.signed_in =
            ulo_core::auth::signed_in(&ulo_core::auth::load(), &self.agent.model.provider);
        self.status_effort = self.agent.effort();
        self.bottom_pinned = ulo_core::config::settings::tui_mode() == "fullscreen";
        self.show_thinking = ulo_core::config::settings::show_thinking();
        self.thinking_hint = ulo_core::config::settings::get_string("thinking_hint")
            .unwrap_or_else(|| "Thinking · ctrl o to view".into());
        self.scroll_lines = ulo_core::config::settings::get_u64("scroll_lines")
            .filter(|n| (1..=100).contains(n))
            .unwrap_or(3) as usize;
        self.scroll_hint = ulo_core::config::settings::get_string("scroll_hint")
            .unwrap_or_else(|| "Scrolled · End to follow".into());
        self.scroll_hint = ulo_core::tools::sanitize_display(&self.scroll_hint).replace('\n', " ");
        self.tool_history_limit = ulo_core::config::settings::get_u64("tool_history_limit")
            .filter(|n| *n <= 1000)
            .unwrap_or(10) as usize;
        self.tool_history_hint = ulo_core::config::settings::get_string("tool_history_hint")
            .unwrap_or_else(|| "{count} earlier successful tools · ctrl o to view".into());
        self.live_preview_rows = ulo_core::config::settings::get_u64("tool_preview_rows")
            .filter(|n| *n <= 20)
            .unwrap_or(5) as usize;
        self.tool_label_rows = ulo_core::config::settings::get_u64("tool_label_rows")
            .filter(|n| (1..=20).contains(n))
            .unwrap_or(2) as usize;
        for block in &mut self.transcript.blocks {
            let collapsed = (block.kind == Kind::Thinking && !self.show_thinking)
                .then(|| self.thinking_hint.clone());
            if block.collapsed != collapsed
                || block.tool_history_limit != self.tool_history_limit
                || block.tool_history_hint != self.tool_history_hint
            {
                block.collapsed = collapsed;
                block.tool_history_limit = self.tool_history_limit;
                block.tool_history_hint = self.tool_history_hint.clone();
                block.touch();
            }
            if block.live_preview_rows != self.live_preview_rows
                || block.tool_label_rows != self.tool_label_rows
            {
                block.live_preview_rows = self.live_preview_rows;
                block.tool_label_rows = self.tool_label_rows;
                block.touch();
            }
        }
    }

    fn notice(&mut self, text: String) {
        self.transcript.push(Block::new(Kind::Notice, text));
    }

    /// A submitted prompt joins up-arrow recall for this session and the
    /// history file for the next. API keys never come through here.
    fn remember_prompt(&mut self, text: String) {
        // A memory-only run (`--no-save`) leaves no trace on disk, prompts
        // included; recall still works within the session.
        if self.agent.saves_session() {
            crate::history::append(&text);
        }
        self.editor.push_history(text);
    }
}

/// Replace /reload's in-progress notice, or append the result if that block
/// disappeared when another command cleared the transcript.
fn finish_reload_notice(transcript: &mut Transcript, reload_block: Option<usize>) {
    const STARTED: &str = "reloading…";
    const FINISHED: &str = "reloaded extensions, themes, and config — skills, prompts, and AGENTS.md are always read fresh";

    if let Some(block) = reload_block
        .and_then(|index| transcript.blocks.get_mut(index))
        .filter(|block| block.kind == Kind::Notice && block.text == STARTED)
    {
        block.text = FINISHED.into();
        block.touch();
    } else {
        transcript.push(Block::new(Kind::Notice, FINISHED));
    }
}

/// The terminal tab title: the custom glyph, a dot, then the session name
/// or the working directory (the reference prefers the session name and
/// falls back to the workspace path).
fn tab_title(path: &str, session_name: Option<&str>) -> String {
    let label = session_name
        .filter(|n| !n.trim().is_empty())
        .unwrap_or(path);
    format!("ulo · {label}")
}

/// Write a title without letting a path or session name terminate its OSC.
fn set_tab_title(title: &str) {
    // Escape codes into a pipe are garbage in the pipe; titles only make
    // sense on a terminal.
    if !stdout_is_tty() {
        return;
    }
    let title = ulo_core::tools::sanitize_display(title).replace('\n', " ");
    let mut out = std::io::stdout();
    let _ = write!(out, "\x1b]0;{title}\x07");
    let _ = out.flush();
}

/// The tab title's path: a short showcase, never the full absolute path.
/// Under $HOME the prefix collapses to `~`; elsewhere only the last two
/// components are shown, so a volume-qualified worktree reads cleanly
/// instead of bleeding its whole path into the tab.
fn title_path() -> String {
    title_path_from(
        &std::env::current_dir().unwrap_or_default(),
        &std::env::var("HOME").unwrap_or_default(),
    )
}

/// The shortening rule, split out for tests.
fn title_path_from(cwd: &std::path::Path, home: &str) -> String {
    use std::path::Component;
    let under_home = !home.is_empty() && cwd.starts_with(home);
    let mut comps: Vec<&str> = cwd
        .components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s.to_str().unwrap_or_default()),
            _ => None,
        })
        .collect();
    // The `~` marker replaces the whole home prefix, not one level of it.
    if under_home {
        let prefix = std::path::Path::new(home)
            .components()
            .filter_map(|c| match c {
                Component::Normal(s) => Some(s.to_str().unwrap_or_default()),
                _ => None,
            })
            .count();
        comps.drain(..prefix.min(comps.len()));
    }
    let tail = comps.split_off(comps.len().saturating_sub(2)).join("/");
    if under_home {
        if tail.is_empty() {
            "~".to_string()
        } else {
            format!("~/{tail}")
        }
    } else if tail.is_empty() {
        // The root itself stays a slash rather than a bare "".
        "/".to_string()
    } else {
        tail
    }
}

/// The path as `~/…` when it lives under $HOME, whole otherwise — the
/// statusline's identity tail and the picker workspace labels share the
/// shape.
fn collapse_home(path: &std::path::Path) -> String {
    let shown = path.to_string_lossy().into_owned();
    match std::env::var("HOME") {
        Ok(home) if !home.is_empty() && shown.starts_with(&home) => {
            format!("~{}", &shown[home.len()..])
        }
        _ => shown,
    }
}

fn ago(ms: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    let secs = now.saturating_sub(ms) / 1000;
    if secs < 60 {
        "now".to_string()
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else if secs < 86400 {
        format!("{}h", secs / 3600)
    } else {
        format!("{}d", secs / 86400)
    }
}

fn system_prompt() -> String {
    ulo_core::agent::context::system_prompt_here()
}

fn persist_model(m: &Model) -> std::io::Result<()> {
    ulo_core::config::settings::set_string("model", &model::slug(m))
}

/// The text after a slash command, only on a word boundary: `/login x` →
/// `Some(" x")`, `/login` → `Some("")`, `/loginfoo` → `None` (so a typo falls
/// through to the unknown-command notice instead of inventing an argument).
fn command_arg<'a>(input: &'a str, command: &str) -> Option<&'a str> {
    input
        .strip_prefix(command)
        .filter(|rest| rest.is_empty() || rest.starts_with(' '))
}

/// Stable labels used in composer chrome and the transcript for attachments.
fn image_labels(count: usize) -> String {
    (1..=count)
        .map(|index| format!("[Image {index}]"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Add attachment labels when the prompt text does not already carry them.
fn display_image_prompt(text: &str, count: usize) -> String {
    let labels = image_labels(count);
    let already_labeled = text.starts_with(&labels);
    if already_labeled {
        text.to_string()
    } else if text.trim().is_empty() {
        labels
    } else {
        format!("{labels} {text}")
    }
}

/// A screenshot tool commonly pastes an absolute temporary path followed by
/// the user's prompt. Return the existing image prefix and the text after it.
/// Checking extension boundaries from left to right also handles spaces in the
/// filename without requiring shell quoting.
fn leading_image_prompt(input: &str) -> Option<(&str, &str)> {
    let lower = input.to_ascii_lowercase();
    for extension in [".png", ".jpg", ".jpeg", ".gif", ".webp"] {
        let mut from = 0;
        while let Some(relative) = lower[from..].find(extension) {
            let end = from + relative + extension.len();
            let boundary = input[end..].chars().next().is_none_or(char::is_whitespace);
            if boundary && std::path::Path::new(&input[..end]).is_file() {
                return Some((&input[..end], input[end..].trim_start()));
            }
            from = end;
        }
    }
    None
}

fn is_literal_slash_prompt(input: &str) -> bool {
    input == "/"
        || input.starts_with("/ ")
        || input
            .split_whitespace()
            .next()
            .is_some_and(|token| token[1..].contains('/'))
}

/// Decide whether a command-line prompt may start now or must wait for the
/// first-visit trust panel. Kept separate so launch ordering stays testable
/// without constructing a terminal frame.
/// Command names dispatch resolves before templates and extension commands.
/// Keep in sync with `dispatch_command`'s match arms.
fn is_builtin_command(name: &str) -> bool {
    matches!(
        name,
        "login"
            | "models"
            | "model"
            | "effort"
            | "scoped-models"
            | "reload"
            | "resume"
            | "new"
            | "clear"
            | "copy"
            | "compact"
            | "fork"
            | "export"
            | "undo"
            | "usage"
            | "trust"
            | "settings"
            | "help"
            | "version"
            | "quit"
            | "exit"
    )
}

/// The `/` picker's functional group for a built-in command, shown as its
/// right-aligned category. `value` is the slashed command (`/login`); an
/// unknown name falls to General.
fn builtin_category(value: &str) -> &'static str {
    match value {
        "/login" => "Account",
        "/models" | "/effort" | "/scoped-models" => "Model",
        "/resume" | "/new" | "/tree" | "/compact" | "/fork" | "/export" => "Session",
        "/trust" | "/undo" => "Workspace",
        _ => "General",
    }
}

/// Display order of the `/` picker's category tags: Account, Model,
/// Session, Workspace, then the General catch-all — so same-tag rows sit
/// together instead of scattering through the list.
fn category_rank(meta: &str) -> u8 {
    match meta {
        "Account" => 0,
        "Model" => 1,
        "Session" => 2,
        "Workspace" => 3,
        _ => 4,
    }
}

fn stage_initial_prompt(
    initial: String,
    awaiting_trust: bool,
    pending: &mut Option<String>,
) -> Option<String> {
    if initial.trim().is_empty() {
        return None;
    }
    if awaiting_trust {
        *pending = Some(initial);
        None
    } else {
        Some(initial)
    }
}

/// Replace this process with the current ulo binary, optionally in a new cwd.
/// Extensions may choose arguments and environment, but never an arbitrary
/// executable.
pub fn relaunch_self(
    cwd: &str,
    args: &[String],
    env: &std::collections::BTreeMap<String, Option<String>>,
) -> std::io::Result<()> {
    use std::os::unix::process::CommandExt;
    let exe = std::env::current_exe()?;
    let mut command = std::process::Command::new(exe);
    command.current_dir(cwd).args(args);
    for (name, value) in env {
        if name.is_empty()
            || name.contains('=')
            || name.contains('\0')
            || value.as_deref().is_some_and(|value| value.contains('\0'))
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "invalid relaunch environment entry",
            ));
        }
        match value {
            Some(value) => {
                command.env(name, value);
            }
            None => {
                command.env_remove(name);
            }
        }
    }
    Err(command.exec())
}

/// /tree's rewind points: every user-turn node's id, one-line preview, and
/// whether its parent already has more than one child — a branch point,
/// meaning /tree was used at that spot before.
fn tree_items(nodes: &[ulo_core::session::Node]) -> Vec<(String, String, bool)> {
    let mut children_of = std::collections::HashMap::<&str, usize>::new();
    for n in nodes {
        if let Some(p) = n.parent.as_deref() {
            *children_of.entry(p).or_insert(0) += 1;
        }
    }
    nodes
        .iter()
        .filter(|n| n.message.role() == "user")
        .map(|n| {
            let preview: String = n
                .message
                .content
                .lines()
                .next()
                .unwrap_or("")
                .chars()
                .take(60)
                .collect();
            let branched = n
                .parent
                .as_deref()
                .map(|p| children_of.get(p).copied().unwrap_or(0) > 1)
                .unwrap_or(false);
            (n.id.clone(), preview, branched)
        })
        .collect()
}

/// The rewind target for a chosen node: its parent, the message history before
/// it (repaired the same way a resume's is, so a crash-cut ancestor never
/// replays as a dangling call), and its prompt text for the composer. None
/// means the id no longer resolves or the ancestor path is corrupt.
fn rewind_target(
    nodes: &[ulo_core::session::Node],
    node_id: &str,
) -> Option<(
    Option<String>,
    Vec<ulo_core::providers::ChatMessage>,
    String,
)> {
    let by_id: std::collections::HashMap<&str, &ulo_core::session::Node> =
        nodes.iter().map(|n| (n.id.as_str(), n)).collect();
    let target = *by_id.get(node_id)?;
    let head = target.parent.clone();
    let mut path_ids = Vec::new();
    let mut cursor = head.clone();
    let mut seen = std::collections::HashSet::new();
    while let Some(id) = cursor {
        if !seen.insert(id.clone()) {
            return None;
        }
        let node = by_id.get(id.as_str()).copied()?;
        path_ids.push(id.clone());
        cursor = node.parent.clone();
    }
    path_ids.reverse();
    let mut messages = path_ids
        .iter()
        .filter_map(|id| by_id.get(id.as_str()).map(|n| n.message.clone()))
        .collect();
    ulo_core::session::repair_history(&mut messages);
    Some((head, messages, target.message.content.clone()))
}

/// The composer's editing keymap: a user's `~/.ulo/keybindings.json` chord
/// override is consulted first (`Some(action)` overrides, `Some(None)`
/// swallows the chord, `None` means "not mentioned"); anything left
/// unmentioned falls through to ulo's built-in bindings below, so an empty or
/// missing file reproduces this function's behavior exactly.
fn key_of(event: &KeyEvent, keymap: &crate::keybindings::Keymap) -> Option<Key> {
    // Crossterm can report Command/Super through the enhanced keyboard
    // protocol. Never degrade an unhandled modified key to printable text.
    if event
        .modifiers
        .intersects(KeyModifiers::SUPER | KeyModifiers::HYPER | KeyModifiers::META)
    {
        return None;
    }
    let ctrl = event.modifiers.contains(KeyModifiers::CONTROL);
    let alt = event.modifiers.contains(KeyModifiers::ALT);
    let shift = event.modifiers.contains(KeyModifiers::SHIFT);
    if let Some(base) = crate::keybindings::base_name(event.code) {
        let chord = ulo_core::config::chord::chord_string(ctrl, alt, shift, &base);
        if let Some(bound) = keymap.lookup(&chord) {
            return bound;
        }
    }
    Some(match (event.code, ctrl, alt) {
        (KeyCode::Enter, ..) if shift || alt => Key::Newline,
        (KeyCode::Enter, ..) => Key::Enter,
        (KeyCode::Backspace, _, true) => Key::KillWord,
        (KeyCode::Backspace, ..) => Key::Backspace,
        (KeyCode::Delete, ..) => Key::Delete,
        // Shift extends a selection through the same motions — the
        // reference's shift-arrow grammar; typing then replaces the range.
        (KeyCode::Left, _, true) if shift => Key::SelectWordLeft,
        (KeyCode::Right, _, true) if shift => Key::SelectWordRight,
        (KeyCode::Left, ..) if shift => Key::SelectLeft,
        (KeyCode::Right, ..) if shift => Key::SelectRight,
        (KeyCode::Up, ..) if shift => Key::SelectUp,
        (KeyCode::Down, ..) if shift => Key::SelectDown,
        (KeyCode::Home, ..) if shift => Key::SelectHome,
        (KeyCode::End, ..) if shift => Key::SelectEnd,
        (KeyCode::Left, _, true) => Key::WordLeft,
        (KeyCode::Right, _, true) => Key::WordRight,
        (KeyCode::Left, ..) => Key::Left,
        (KeyCode::Right, ..) => Key::Right,
        (KeyCode::Up, ..) => Key::Up,
        (KeyCode::Down, ..) => Key::Down,
        (KeyCode::Home, ..) => Key::Home,
        (KeyCode::End, ..) => Key::End,
        (KeyCode::Char('a'), true, _) => Key::Home,
        (KeyCode::Char('e'), true, _) => Key::End,
        (KeyCode::Char('k'), true, _) => Key::KillToEnd,
        (KeyCode::Char('u'), true, _) => Key::KillToStart,
        (KeyCode::Char('w'), true, _) => Key::KillWord,
        (KeyCode::Char('b'), true, _) => Key::Left,
        (KeyCode::Char('f'), true, _) => Key::Right,
        (KeyCode::Char('j'), true, _) => Key::Newline,
        // ctrl+d with text deletes forward, completing the emacs chord
        // family (a/ulo/k/u/w/b/f). On an empty composer the app-level
        // handler above has already taken it as quit, like a shell EOF.
        (KeyCode::Char('d'), true, _) => Key::Delete,
        (KeyCode::Char(c), false, false) => Key::Char(c),
        _ => return None,
    })
}

#[cfg(test)]
mod tests;
