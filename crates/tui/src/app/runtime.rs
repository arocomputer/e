//! Own terminal startup, the event loop, and terminal cleanup.
//!
//! [`run_scoped`] reads as the whole story: take the terminal, build the
//! app, say what launch has to say, run the [`FrameLoop`] until something
//! ends it, then shut down in order. Each select arm of the loop hands its
//! event to one named handler; a key press walks [`App::on_key`]'s
//! precedence from e's own chords down to the composer.

use std::ops::ControlFlow::{self, Break, Continue};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tokio::sync::mpsc::{Receiver, Sender};

use super::*;
use e_core::auth::login::Outcome as LoginOutcome;
use e_core::extensions::{ExtensionHost, HostRequest};

/// Launch inputs after extension startup hooks have rewritten arguments and cwd.
pub struct RunOptions {
    pub initial: String,
    pub continue_session: bool,
    pub resume_session: bool,
    pub model: Model,
    pub agent: AgentOptions,
    pub images: Vec<e_core::providers::ImageInput>,
}

/// The channel extensions' own requests travel on: created by the caller
/// before the host starts (so `initialize` can promise a UI), consumed here.
pub type Requests = (
    tokio::sync::mpsc::Sender<e_core::extensions::HostRequest>,
    tokio::sync::mpsc::Receiver<e_core::extensions::HostRequest>,
);

/// Frame pacing: every select arm may change what's on screen, but frames
/// are built at most once per interval — a token burst becomes one paint,
/// and a deferred paint fires when the interval lapses.
const FRAME_INTERVAL: Duration = Duration::from_millis(33);

/// Open the terminal session and restore terminal state when its loop ends.
pub async fn run(
    options: RunOptions,
    host: std::sync::Arc<e_core::extensions::ExtensionHost>,
    jobs_tx: tokio::sync::mpsc::Sender<String>,
    jobs_rx: tokio::sync::mpsc::Receiver<String>,
    requests: Requests,
) -> std::io::Result<()> {
    let home = options
        .agent
        .home
        .clone()
        .unwrap_or_else(e_core::config::home::home);
    e_core::config::home::scope(home, run_scoped(options, host, jobs_tx, jobs_rx, requests)).await
}

/// Run the terminal and its configuration reads within the selected home.
pub(super) async fn run_scoped(
    options: RunOptions,
    host: std::sync::Arc<e_core::extensions::ExtensionHost>,
    jobs_tx: tokio::sync::mpsc::Sender<String>,
    jobs_rx: tokio::sync::mpsc::Receiver<String>,
    requests: Requests,
) -> std::io::Result<()> {
    let (requests_tx, requests_rx) = requests;
    let RunOptions {
        initial,
        continue_session,
        resume_session,
        model,
        agent: agent_options,
        images,
    } = options;
    install_panic_hook();
    let guard = enter_terminal()?;
    // detect_light() probes the terminal background over OSC 11 (short
    // timeout) and falls back to COLORFGBG, then dark.
    let detected = crate::background::detect_light().unwrap_or(false);
    let theme = crate::theme::resolve(&e_core::config::settings::theme(), detected);
    let keymap = crate::keybindings::load();

    let (cols, rows) = terminal::size()?;
    // The launch anchor: the frame paints below where the user launched e,
    // never over what came before. A terminal that doesn't answer DSR 6n — a raw pty —
    // falls back to the screen's bottom row, the common launch spot.
    let anchor =
        crate::paint::background::query_cursor_row(rows).unwrap_or(rows.saturating_sub(1)) as usize;
    let painter = Painter::spawn(cols, rows, anchor);
    let (mut agent, session_events) = Agent::with_options(model, agent_options.clone());
    let (logins_tx, logins_rx) = tokio::sync::mpsc::channel::<LoginOutcome>(4);
    let (results_tx, results_rx) = tokio::sync::mpsc::channel::<AppJob>(16);
    agent.set_host(host.clone());
    let senders = Senders {
        jobs: jobs_tx,
        logins: logins_tx,
        results: results_tx,
        requests: requests_tx,
    };
    let mut app = App::new(agent, host, theme, keymap, detected, images, senders);
    app.start(&agent_options, resume_session, continue_session);

    use tokio::signal::unix::{signal, SignalKind};
    let input_paused = Arc::new(AtomicBool::new(false));
    let mut frame_loop = FrameLoop {
        painter,
        cols,
        rows,
        anchor,
        input: spawn_input_reader(input_paused.clone()),
        input_paused,
        session_events,
        requests: requests_rx,
        results: results_rx,
        jobs: jobs_rx,
        logins: logins_rx,
        tick: tokio::time::interval(Duration::from_millis(250)),
        // SIGTERM/SIGHUP (a kill, a closed tab) exit through the same cleanup as
        // /quit — the terminal is restored, the extension host shut down.
        sigterm: signal(SignalKind::terminate())?,
        sighup: signal(SignalKind::hangup())?,
        next_paint: tokio::time::Instant::now(),
        paint_deferred: false,
        event_buf: Vec::with_capacity(128),
    };

    app.stage_launch_prompt(initial, resume_session);
    frame_loop
        .painter
        .frame_in_view(app.frame(cols as usize, rows as usize), app.fixed_view());
    frame_loop.run(&mut app).await;
    shut_down(&mut app, &mut frame_loop.painter, guard).await;
    Ok(())
}

/// A panic mid-frame must not strand the shell in raw mode with a hidden
/// cursor or kitty keyboard flags — restore the terminal first, then
/// report as usual. (\x1b[<u pops the keyboard enhancement stack.) Only
/// a panic on this thread — the frame loop, driven by the runtime's
/// block_on — is fatal to the session; the paint thread, tool tasks and
/// the turn worker all run elsewhere and catch their own panics to keep
/// the session alive, so the hook must leave the terminal alone for them
/// (the hook fires before any catch_unwind gets its say).
fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    let frame_thread = std::thread::current().id();
    std::panic::set_hook(Box::new(move |info| {
        if std::thread::current().id() == frame_thread {
            let _ = terminal::disable_raw_mode();
            print!("\x1b[<u\x1b[?2004l\x1b[?25h\r\n");
            use std::io::Write as _;
            let _ = std::io::stdout().flush();
        }
        default_hook(info);
    }));
}

/// Raw mode first so the frame loop can take the terminal over. Theme
/// detection, which follows, queries the terminal (OSC 11 background color,
/// then COLORFGBG) so `auto` follows the real terminal theme instead of
/// defaulting to dark. The probe is timeout-bounded and runs after this,
/// where the TUI owns the terminal reader, so it can't block startup or
/// swallow keystrokes (audit #93). The guard exists before any further mode
/// changes, so every exit path restores them.
fn enter_terminal() -> std::io::Result<TerminalGuard> {
    terminal::enable_raw_mode()?;
    let guard = TerminalGuard;
    push_terminal_modes()?;
    Ok(guard)
}

/// Enable the modes the TUI reads input through: bracketed paste, mouse
/// capture, and the kitty keyboard protocol — without it, terminals send
/// plain Enter for shift+enter and multi-line entry is unreachable.
fn push_terminal_modes() -> std::io::Result<()> {
    execute!(
        std::io::stdout(),
        EnableBracketedPaste,
        EnableMouseCapture,
        PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
    )
}

/// Undo [`push_terminal_modes`]. Popping a mode that never got enabled is
/// harmless.
fn pop_terminal_modes() -> std::io::Result<()> {
    execute!(
        std::io::stdout(),
        PopKeyboardEnhancementFlags,
        DisableBracketedPaste,
        DisableMouseCapture
    )
}

/// Let the final frame land before the terminal is restored. Shells use
/// detached process groups, so stop them explicitly before this process
/// gives extensions their shutdown notification.
async fn shut_down(app: &mut App, painter: &mut Painter, guard: TerminalGuard) {
    painter.shutdown();
    e_core::tools::kill_tracked_processes();
    app.host
        .event("session_shutdown", serde_json::json!({"reason": "quit"}))
        .await;
    app.host.shutdown().await;
    drop(guard);
    // The tab title we set at launch (or from a session name) is ours to
    // clear — the reference leaves the terminal pristine on exit.
    set_tab_title("");
    if app.relaunch {
        // The terminal is restored and the host is down: replace this
        // process with the updated binary, continuing the same session.
        let cwd = std::env::current_dir()
            .unwrap_or_default()
            .display()
            .to_string();
        let args = vec!["-c".to_string()];
        if let Err(error) = relaunch_self(&cwd, &args, &std::collections::BTreeMap::new()) {
            eprintln!("relaunch failed: {error} — start e again by hand");
        }
    }
}

/// The channels App keeps to report back to the frame loop, and the one it
/// hands every extension host for `ui.*` / `session.*` requests.
struct Senders {
    jobs: Sender<String>,
    logins: Sender<LoginOutcome>,
    results: Sender<AppJob>,
    requests: Sender<HostRequest>,
}

/// The terminal side of the session and every channel the frame loop waits
/// on. Built once at launch, alive until the process exits.
struct FrameLoop {
    painter: Painter,
    cols: u16,
    rows: u16,
    anchor: usize,
    /// Terminal input is read on its own thread and can be paused: while an
    /// external editor owns the terminal (ctrl+g), nothing here may read
    /// it, or the editor's keystrokes land in e instead.
    input_paused: Arc<AtomicBool>,
    input: Receiver<std::io::Result<TermEvent>>,
    session_events: Receiver<SessionEvent>,
    requests: Receiver<HostRequest>,
    results: Receiver<AppJob>,
    jobs: Receiver<String>,
    logins: Receiver<LoginOutcome>,
    tick: tokio::time::Interval,
    sigterm: tokio::signal::unix::Signal,
    sighup: tokio::signal::unix::Signal,
    next_paint: tokio::time::Instant,
    /// A paint was skipped inside the frame interval.
    paint_deferred: bool,
    event_buf: Vec<SessionEvent>,
}

impl FrameLoop {
    /// Wait, handle, paint, until the input or the session ends, a signal
    /// arrives, or the app asks to quit.
    async fn run(&mut self, app: &mut App) {
        loop {
            if app.external_edit {
                app.external_edit = false;
                edit_externally(
                    app,
                    &mut self.painter,
                    &self.input_paused,
                    self.cols,
                    self.rows,
                    self.anchor,
                )
                .await;
            }
            if self.wait(app).await.is_break() {
                return;
            }
            self.note_paint_status(app);
            self.paint_when_due(app);
            if app.should_quit {
                return;
            }
        }
    }

    /// Wait for the next thing that happens and hand it to its handler.
    /// `Break` means the loop is over: input or the session closed, or a
    /// signal asked to exit.
    async fn wait(&mut self, app: &mut App) -> ControlFlow<()> {
        tokio::select! {
            maybe = self.input.recv() => {
                let Some(Ok(event)) = maybe else { return Break(()) };
                self.on_terminal_event(app, event);
            }
            count = self.session_events.recv_many(&mut self.event_buf, 128) => {
                if count == 0 {
                    return Break(());
                }
                // Apply the whole burst before building one frame — a fast
                // stream must not cost one paint per delta.
                for e in self.event_buf.drain(..) {
                    app.on_session_event(e);
                }
            }
            request = self.requests.recv() => {
                if let Some(request) = request {
                    app.on_host_request(request);
                }
            }
            job = self.results.recv() => {
                if let Some(job) = job {
                    app.on_job(job);
                }
            }
            message = self.jobs.recv() => {
                if let Some(message) = message {
                    app.notice(message);
                }
            }
            outcome = self.logins.recv() => {
                if let Some(outcome) = outcome {
                    app.on_login_outcome(outcome);
                }
            }
            _ = self.sigterm.recv() => return Break(()),
            _ = self.sighup.recv() => return Break(()),
            // A paint was skipped inside the frame interval; fire it when
            // the interval lapses.
            _ = tokio::time::sleep_until(self.next_paint), if self.paint_deferred => {}
            _ = self.tick.tick() => app.on_tick(),
        }
        Continue(())
    }

    /// Paste, mouse, resize, and key presses. Key releases are ignored.
    fn on_terminal_event(&mut self, app: &mut App, event: TermEvent) {
        let (cols, rows) = (self.cols as usize, self.rows as usize);
        match event {
            TermEvent::Paste(text) if app.viewer.is_none() => app.paste(&text),
            TermEvent::Mouse(event) => app.mouse(event, cols, rows),
            TermEvent::Resize(c, r) => {
                self.cols = c;
                self.rows = r;
                self.painter.resize(c, r);
            }
            TermEvent::Key(k) if k.kind != crossterm::event::KeyEventKind::Release => {
                app.on_key(k, cols, rows);
            }
            _ => {}
        }
    }

    /// Surface a slow or failing paint thread: the statusline's delay flag,
    /// and one notice per distinct failure.
    fn note_paint_status(&self, app: &mut App) {
        let paint_status = self.painter.status();
        app.rendering_delayed = paint_status.delayed(Duration::from_millis(500));
        match paint_status.failure.as_ref().map(|(_, error)| error) {
            Some(error) if app.last_paint_failure.as_ref() != Some(error) => {
                app.notice(format!("render failed: {error}"));
                app.last_paint_failure = Some(error.clone());
            }
            None => app.last_paint_failure = None,
            Some(_) => {}
        }
    }

    /// Paint now if the frame interval has lapsed (or the app is quitting),
    /// else defer the paint to the interval's end.
    fn paint_when_due(&mut self, app: &mut App) {
        let now = tokio::time::Instant::now();
        if now < self.next_paint && !app.should_quit {
            self.paint_deferred = true;
            return;
        }
        let (cols, rows) = (self.cols as usize, self.rows as usize);
        let frame = if app.viewer.is_some() {
            app.viewer_frame(cols, rows)
        } else {
            app.frame(cols, rows)
        };
        // Fixed-height reading preserves the inline screen and its native history.
        self.painter.frame_in_view(frame, app.fixed_view());
        self.next_paint = now + FRAME_INTERVAL;
        self.paint_deferred = false;
    }
}

impl App {
    /// The app at launch: the agent and look the caller resolved, the
    /// channels it reports on, and every surface closed.
    fn new(
        agent: Agent,
        host: Arc<ExtensionHost>,
        theme: Theme,
        keymap: crate::keybindings::Keymap,
        light_background: bool,
        images: Vec<e_core::providers::ImageInput>,
        senders: Senders,
    ) -> Self {
        App {
            theme,
            keymap,
            transcript: Transcript::default(),
            editor: Editor::new(),
            attachments: input::Attachments::default(),
            agent,
            active: None,
            overlay: None,
            armed_at: None,
            should_quit: false,
            context_tokens: 0,
            pending_key: None,
            menu: None,
            staged_scope: None,
            auth: None,
            settings: None,
            show_thinking: e_core::config::settings::show_thinking(),
            thinking_hint: String::new(),
            jobs: senders.jobs,
            logins: senders.logins,
            login_task: None,
            login_sequence: 0,
            host,
            results: senders.results,
            input_verdicts: PendingInputVerdicts::default(),
            compacting: false,
            held_prompts: Vec::new(),
            trust: None,
            pending_initial: None,
            pending_initial_images: images,
            shell_block: None,
            reloading: false,
            reload_block: None,
            outputs: Vec::new(),
            output_seq: 0,
            viewer: None,
            conversation_scroll: None,
            scroll_lines: 3,
            scroll_hint: String::new(),
            viewer_cache: None,
            queue_review: None,
            session_epoch: 0,
            update_installed: None,
            relaunch: false,
            rendering_delayed: false,
            last_paint_failure: None,
            light_background,
            bottom_pinned: false,
            live_preview_rows: 5,
            tool_label_rows: 2,
            tool_history_limit: 10,
            tool_history_hint: String::new(),
            signed_in: false,
            status_effort: None,
            requests: senders.requests,
            ui_queue: extui::UiQueue::new(),
            ui_prompt: None,
            ext_status: std::collections::BTreeMap::new(),
            ext_activity: std::collections::BTreeMap::new(),
            ext_panel: None,
            pane: None,
            pane_hidden: false,
            widgets: std::collections::BTreeMap::new(),
            layout: e_core::config::layout::load(),
            external_edit: false,
        }
    }

    /// Everything launch does before the first frame, in order: recall,
    /// `session_start`, the banner and launch notices, background refreshes,
    /// the sign-in check, the -r/-c session, and the tab title.
    fn start(&mut self, options: &AgentOptions, resume_session: bool, continue_session: bool) {
        self.editor
            .seed_history(crate::history::load(crate::history::RECALL));
        self.refresh_status_cache();
        self.emit(
            "session_start",
            serde_json::json!({
                "reason": "startup",
                "path": self.agent.session_path().map(|p| p.display().to_string()),
            }),
        );
        if self.layout.banner {
            self.transcript
                .push(Block::new(Kind::Banner, e_core::VERSION));
        }
        self.launch_notices(options);
        spawn_launch_refreshes(&self.results);
        self.check_sign_in();
        if resume_session {
            // The reference behavior: launch straight into the session picker.
            self.open_resume_menu();
        } else if continue_session {
            self.resume_recent();
        }
        // Terminal tab title: the custom glyph, a dot, the path — a named
        // session takes over the title when one lands (the reference prefers the
        // session name over the workspace).
        set_tab_title(&tab_title(
            &title_path(),
            self.agent.session_name().as_deref(),
        ));
    }

    /// Configuration warnings and run modes worth a line, and the first-visit
    /// trust question.
    fn launch_notices(&mut self, options: &AgentOptions) {
        for warning in model::config_warnings() {
            self.notice(format!("warning: {warning}"));
        }
        if !options.save_session {
            self.notice("session saving disabled for this run".into());
        }
        match options.tool_mode {
            e_core::cli::ToolMode::None => {
                self.notice("no-tools mode — provider requests contain no tool schemas".into())
            }
            e_core::cli::ToolMode::All => {}
        }
        if e_core::config::trust::status(&self.agent.cwd()).is_none() {
            self.trust = Some(TrustStage::new(&self.agent.cwd()));
        }
        // The trust lookup may be the first read of trust.json; drain afterward so
        // its recovery joins warnings collected while constructing the app.
        for warning in e_core::config::store::take_warnings() {
            self.notice(format!("warning: {warning}"));
        }
    }

    /// With no provider signed in, open sign-in; otherwise say when the
    /// saved model could not be used.
    fn check_sign_in(&mut self) {
        if e_core::auth::load().is_empty() {
            self.notice(
                "no provider signed in — use /login to sign in with an account or API key".into(),
            );
            // Route straight to sign-in instead of leaving a phantom model
            // implied on the status bar. Yields to the trust panel above it,
            // if that's showing too — this still renders once trust is settled.
            self.open_login_menu();
        } else if let Some(wanted) = e_core::config::settings::get_string("model") {
            let current = self.agent.model_slug();
            if wanted != current {
                self.notice(format!(
                    "{wanted} is unavailable (provider not signed in) — using {current}"
                ));
            }
        }
    }

    /// Submit the command-line prompt, or hold it while the trust question
    /// is open. A -r launch prompt must wait for the session pick, or it
    /// would start a turn whose reply splices into whichever session gets
    /// selected.
    fn stage_launch_prompt(&mut self, initial: String, resume_session: bool) {
        let hold_initial = self.trust.is_some() || (resume_session && self.menu.is_some());
        if let Some(initial) =
            stage_initial_prompt(initial, hold_initial, &mut self.pending_initial)
        {
            self.submit_initial(initial);
        }
    }

    /// Route one key press. The order is precedence: e's own chords first,
    /// then an extension surface, then whichever panel or picker is open,
    /// and the composer last.
    fn on_key(&mut self, k: KeyEvent, cols: usize, rows: usize) {
        if self.global_key(k, cols, rows) || self.extension_key(k) || self.panel_key(k, cols, rows)
        {
            return;
        }
        self.composer_key(k, cols, rows);
    }

    /// No picker or panel (menu, settings, sign-in, trust) holds the footer.
    fn panels_closed(&self) -> bool {
        self.menu.is_none()
            && self.settings.is_none()
            && self.auth.is_none()
            && self.trust.is_none()
    }

    /// ctrl+c, clipboard paste, and the ctrl+o viewer: e's before anything
    /// else sees the key. True when the key was taken.
    fn global_key(&mut self, k: KeyEvent, cols: usize, rows: usize) -> bool {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        let system_paste = k
            .modifiers
            .intersects(KeyModifiers::SUPER | KeyModifiers::META)
            && matches!(k.code, KeyCode::Char('v') | KeyCode::Char('V'));
        if ctrl && k.code == KeyCode::Char('c') {
            self.interrupt_or_exit();
            return true;
        }
        if ((ctrl && k.code == KeyCode::Char('v')) || system_paste)
            && self.composer_free()
            && !self.editor.mask
        {
            self.paste_clipboard();
            return true;
        }
        if self.viewer_key(k, cols, rows) {
            return true;
        }
        if ctrl && k.code == KeyCode::Char('o') {
            self.viewer = Some(Viewer::new());
            return true;
        }
        false
    }

    /// The side pane and the extension panel, when they own the keyboard.
    /// ctrl+c never reaches here: [`App::global_key`] keeps it e's. True
    /// when the key was taken.
    fn extension_key(&mut self, k: KeyEvent) -> bool {
        let free = self.panels_closed() && !self.ui_input_open();
        if self.pane.is_some()
            && free
            && self.pending_key.is_none()
            && extui::chord_of(&k).as_deref() == Some(self.layout.focus.as_str())
        {
            // The layout's focus chord moves between the
            // conversation and the pane.
            if let Some(pane) = self.pane.as_mut() {
                pane.focused = !pane.focused;
            }
        } else if self.pane.as_ref().is_some_and(|p| p.focused) && free {
            // The pane owns the keyboard: e navigates it,
            // and chords it does not use go to the owner.
            if let Some(pane) = self.pane.as_mut() {
                let action = pane.key(k);
                self.pane_action(action);
            }
        } else if self.ext_panel.as_ref().is_some_and(|p| p.interactive) && self.panels_closed() {
            // The panel owns the keyboard: Esc closes it
            // here, every other chord goes to its owner.
            if k.code == KeyCode::Esc {
                self.close_ext_panel(true);
            } else {
                self.forward_panel_key(&k);
            }
        } else if k.code == KeyCode::Esc && self.ext_panel.is_some() && free {
            self.close_ext_panel(true);
        } else if k.code == KeyCode::Esc && self.ui_input_open() {
            self.cancel_ui_prompt();
        } else {
            return false;
        }
        true
    }

    /// The trust question, /settings, sign-in, and the open picker, in that
    /// order. Each panel takes every key while it is open. True when the
    /// key was taken.
    fn panel_key(&mut self, k: KeyEvent, cols: usize, rows: usize) -> bool {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        if self.trust.is_some() {
            self.trust_key(k.code, cols, rows);
        } else if self.settings.is_some() {
            self.settings_key(k.code);
        } else if self.auth.is_some() {
            self.sign_in_key(k);
        } else if self.scoped_menu_takes(k.code, ctrl) {
            self.scoped_menu_key(k.code, ctrl);
        } else if self.menu_takes(&k, ctrl) {
            self.menu_key(k.code);
        } else {
            return false;
        }
        true
    }

    /// The first-visit trust question: move, page, or answer.
    fn trust_key(&mut self, code: KeyCode, cols: usize, rows: usize) {
        let Some(stage) = &mut self.trust else { return };
        match code {
            KeyCode::Up => stage.step(-1),
            KeyCode::Down => stage.step(1),
            KeyCode::PageUp => stage.page(-1, cols, rows.saturating_sub(1)),
            KeyCode::PageDown => stage.page(1, cols, rows.saturating_sub(1)),
            KeyCode::Enter => {
                let (parent, trusted) = stage.choice();
                self.answer_trust(parent, trusted);
            }
            _ => {}
        }
    }

    /// Persist the trust answer, or quit on a decline. The middle row (when
    /// offered) trusts the broader ancestor; trust propagates down, so the
    /// workspace is covered too.
    fn answer_trust(&mut self, parent: Option<std::path::PathBuf>, trusted: bool) {
        let target = parent.unwrap_or_else(|| self.agent.cwd().to_path_buf());
        if !trusted {
            // A decline is remembered nowhere: e
            // runs only trusted, so the next
            // launch asks again.
            if let Some(refusal) = e_core::config::trust::refusal(&target) {
                self.notice(refusal);
            }
            self.should_quit = true;
            return;
        }
        match e_core::config::trust::set(&target, true) {
            Err(e) => self.notice(format!("trust: {e}")),
            Ok(()) => {
                self.trust = None;
                self.install_project_packages();
                // An open -r picker still owns
                // the launch prompt; submitting
                // it now would start a turn the
                // session pick then refuses.
                if self.menu.is_none() {
                    if let Some(initial) = self.pending_initial.take() {
                        self.submit_initial(initial);
                    }
                }
            }
        }
    }

    /// A key while /settings is open: move, change the selected setting, or
    /// close. Every key re-reads what settings drive.
    fn settings_key(&mut self, code: KeyCode) {
        let Some(panel) = &mut self.settings else {
            return;
        };
        let mut setting_error = None;
        let changing_effort = panel.selected_key() == Some("effort")
            && matches!(code, KeyCode::Left | KeyCode::Right);
        match code {
            KeyCode::Up => panel.step(-1),
            KeyCode::Down => panel.step(1),
            KeyCode::Left => setting_error = panel.change(-1).err(),
            KeyCode::Right => setting_error = panel.change(1).err(),
            // Esc alone closes: Enter opens the panel
            // from the command menu, so it must not be
            // the same key that dismisses it.
            KeyCode::Esc => self.settings = None,
            _ => {}
        }
        if let Some(error) = setting_error {
            self.notice(format!("could not save setting: {error}"));
        } else if changing_effort {
            self.agent.use_saved_effort();
        }
        // A theme change applies immediately; settings can
        // also change what the statusline derives from disk.
        // The thinking toggle and keymap are file-backed
        // too — re-read them so a mid-session change
        // lands this frame.
        self.apply_theme();
        self.apply_keymap();
        self.refresh_status_cache();
    }

    /// A key while the sign-in panel is open: move through its lists, pick,
    /// step back with Backspace, or close with Esc.
    fn sign_in_key(&mut self, k: KeyEvent) {
        let Some(stage) = &mut self.auth else { return };
        match (&mut *stage, k.code) {
            (AuthStage::Choose { selected }, KeyCode::Up | KeyCode::Down) => {
                *selected = 1 - *selected;
            }
            (AuthStage::Choose { selected }, KeyCode::Enter) => {
                let choice = *selected;
                self.auth_choose(choice);
            }
            (AuthStage::Account { selected }, KeyCode::Up | KeyCode::Down) => {
                let n = e_core::providers::registry::oauth_providers().len();
                *selected = authpanel::step(*selected, n, k.code == KeyCode::Up);
            }
            (AuthStage::Key { selected }, KeyCode::Up | KeyCode::Down) => {
                let n = e_core::providers::registry::key_providers().len();
                *selected = authpanel::step(*selected, n, k.code == KeyCode::Up);
            }
            (AuthStage::Account { selected }, KeyCode::Enter) => {
                let choice = *selected;
                self.auth_account(choice);
            }
            (AuthStage::Key { selected }, KeyCode::Enter) => {
                let choice = *selected;
                self.auth_key(choice);
            }
            (_, KeyCode::Esc) => self.close_auth_panel(),
            (AuthStage::Account { .. }, KeyCode::Backspace) => {
                *stage = AuthStage::Choose { selected: 0 };
            }
            (AuthStage::Key { .. }, KeyCode::Backspace) => {
                *stage = AuthStage::Choose { selected: 1 };
            }
            // The entry keeps Backspace for editing while
            // there is text; an empty input navigates back.
            (AuthStage::ApiKey { provider }, KeyCode::Backspace) if self.editor.is_empty() => {
                let provider = provider.clone();
                self.pending_key = None;
                self.editor.mask = false;
                let selected = e_core::providers::registry::key_providers()
                    .iter()
                    .position(|p| p.name == provider)
                    .unwrap_or(0);
                *stage = AuthStage::Key { selected };
            }
            (AuthStage::Waiting { back }, KeyCode::Backspace) => {
                let back = *back;
                let cancelled = self.cancel_login();
                // Launched by `/login <provider>`: no
                // list to return to, so close.
                self.auth = back.map(|selected| AuthStage::Account { selected });
                if cancelled {
                    self.notice("login cancelled".into());
                }
            }
            (AuthStage::Done { back, .. }, KeyCode::Enter | KeyCode::Backspace) => {
                *stage = back.stage();
            }
            (AuthStage::ApiKey { .. }, _) => {
                if let Some(key) = key_of(&k, &self.keymap) {
                    if let EditorResult::Submit(text) = self.editor.key(key) {
                        self.submit(text);
                    }
                }
            }
            _ => {}
        }
    }

    /// Esc closes the whole sign-in panel from any depth; an in-flight flow
    /// is cancelled with it.
    fn close_auth_panel(&mut self) {
        let waiting = matches!(self.auth, Some(AuthStage::Waiting { .. }));
        let cancelled = self.cancel_login();
        self.auth = None;
        self.pending_key = None;
        self.editor.mask = false;
        self.editor.set_text("");
        self.discard_composer_images();
        if waiting && cancelled {
            self.notice("login cancelled".into());
        }
    }

    /// The scoped-models picker keeps Space (toggle), ctrl+x (reset), and
    /// ctrl+s (save) for itself.
    fn scoped_menu_takes(&self, code: KeyCode, ctrl: bool) -> bool {
        self.menu
            .as_ref()
            .map(|m| m.kind == MenuKind::Scoped)
            .unwrap_or(false)
            && ((code == KeyCode::Char(' ') && !ctrl)
                || (ctrl && matches!(code, KeyCode::Char('x') | KeyCode::Char('s'))))
    }

    /// Toggle, reset, or save the scoped-models picker's staged scope.
    fn scoped_menu_key(&mut self, code: KeyCode, ctrl: bool) {
        match (code, ctrl) {
            (KeyCode::Char('x'), true) => {
                // Reset: stage nothing — the picker
                // mirrors "no scope" and Ctrl+S saves it
                // (or Ctrl+X again is enough to walk
                // back). Nothing hits settings.json
                // until Ctrl+S.
                self.staged_scope = Some(Vec::new());
                self.open_scoped_menu();
            }
            (KeyCode::Char('s'), true) => self.save_scope(),
            _ => self.toggle_scoped(),
        }
    }

    /// An open picker takes navigation, Enter, Esc, and — when it has tabs —
    /// Tab and shift+tab. Shift+tab belongs to the picker while it is open:
    /// it steps the tabs backward. The effort shortcut stays a bare-composer
    /// key.
    fn menu_takes(&self, k: &KeyEvent, ctrl: bool) -> bool {
        let has_tabs = || self.menu.as_ref().is_some_and(|menu| menu.has_tabs());
        self.menu.is_some()
            && (matches!(
                k.code,
                KeyCode::Up | KeyCode::Down | KeyCode::Enter | KeyCode::Esc
            ) || (k.code == KeyCode::Tab
                && !k.modifiers.contains(KeyModifiers::SHIFT)
                && has_tabs())
                || (k.code == KeyCode::BackTab && has_tabs()))
            && !ctrl
    }

    /// Step, switch tabs, select, or close the open picker.
    fn menu_key(&mut self, code: KeyCode) {
        match code {
            KeyCode::Enter => {
                self.select_menu();
            }
            KeyCode::Esc => self.close_menu(),
            _ => {
                let Some(menu) = self.menu.as_mut() else {
                    return;
                };
                match code {
                    KeyCode::Up => menu.step(-1),
                    KeyCode::Down => menu.step(1),
                    KeyCode::Tab => menu.cycle_tab(),
                    KeyCode::BackTab => menu.cycle_tab_back(),
                    _ => {}
                }
            }
        }
    }

    /// Esc on an open picker: an extension's `select` is answered
    /// "cancelled", a picker typed without its trigger clears its filter.
    fn close_menu(&mut self) {
        if self
            .menu
            .as_ref()
            .is_some_and(|m| m.kind == MenuKind::Extension)
        {
            self.cancel_ui_prompt();
        }
        if self
            .menu
            .as_ref()
            .is_some_and(|m| m.filter_without_trigger && m.kind != MenuKind::Commands)
        {
            self.editor.set_text("");
        }
        self.menu = None;
        // Closing the scoped picker without
        // Ctrl+S discards its staged edits.
        self.staged_scope = None;
        // Declining the -r picker releases a
        // held launch prompt into the current
        // session.
        self.release_initial_prompt();
    }

    /// What's left once no surface took the key: cancels, the composer's own
    /// shortcuts, conversation scrolling, the queue review, editing, and
    /// finally an extension's declared shortcut.
    fn composer_key(&mut self, k: KeyEvent, cols: usize, rows: usize) {
        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
        if k.code == KeyCode::Esc && self.pending_key.is_some() {
            self.pending_key = None;
            self.editor.mask = false;
            self.editor.set_text("");
            self.discard_composer_images();
            self.notice("login cancelled".into());
        } else if k.code == KeyCode::Esc && self.agent.is_streaming() {
            self.agent.interrupt();
        } else if ctrl
            && k.code == KeyCode::Char('g')
            && self.panels_closed()
            && (!self.ui_input_open() || self.ui_editor_open())
            && self.pending_key.is_none()
        {
            // Deferred to the top of the loop: the terminal
            // hand-off needs the painter and the reader,
            // which the key handler does not own.
            self.external_edit = true;
        } else if ctrl && matches!(k.code, KeyCode::Char('p') | KeyCode::Char('P')) {
            let backward =
                k.code == KeyCode::Char('P') || k.modifiers.contains(KeyModifiers::SHIFT);
            self.cycle_model(!backward);
        } else if self.panels_closed()
            && (k.code == KeyCode::BackTab
                || (k.code == KeyCode::Tab && !ctrl && k.modifiers.contains(KeyModifiers::SHIFT)))
        {
            self.cycle_effort_key();
        } else if self.conversation_key(k, cols, rows) {
            // Consumed by conversation scrolling.
        } else if !ctrl && self.queue_review_key(k.code) {
            // Consumed by the queued-prompt review.
        } else if let Some(key) = key_of(&k, &self.keymap) {
            if let EditorResult::Submit(text) = self.editor.key(key) {
                self.submit_composer(text);
            }
            self.sync_menu();
        } else if let Some(chord) = extui::chord_of(&k)
            .filter(|chord| self.host.has_shortcut(chord))
            .filter(|_| !self.ui_input_open() && self.pending_key.is_none())
        {
            self.run_shortcut(chord);
        }
    }

    /// Shift+tab cycles the model's declared levels —
    /// a bare-composer shortcut. With a picker or
    /// panel open the keys belong to that surface;
    /// the effort setting must not mutate unseen
    /// beneath it. The statusline confirms the
    /// change; nothing lands in the transcript.
    fn cycle_effort_key(&mut self) {
        match self.agent.cycle_effort() {
            Ok(Some(level)) => {
                self.refresh_status_cache();
                self.emit("effort_change", serde_json::json!({"effort": level}));
            }
            Ok(None) => {}
            Err(error) => self.notice(format!("could not save reasoning effort: {error}")),
        }
    }

    /// A declared extension shortcut, answered like
    /// a command. Only a chord neither e nor the
    /// composer (built in, or the user's
    /// keybindings.json) took reaches this —
    /// so unbinding a chord there frees it for an
    /// extension.
    fn run_shortcut(&self, chord: String) {
        let host = self.host.clone();
        let results = self.results.clone();
        let epoch = self.session_epoch;
        e_core::config::home::spawn(async move {
            let result = host.run_shortcut(&chord).await;
            let _ = results.send(AppJob::Command { result, epoch }).await;
        });
    }

    /// Background work landing back in the frame loop.
    fn on_job(&mut self, job: AppJob) {
        match job {
            AppJob::Command { result, epoch } => self.deliver_command_result(result, epoch),
            AppJob::Completions {
                command,
                prefix,
                items,
            } => self.show_completions(&command, &prefix, items),
            AppJob::Rendered {
                target,
                show,
                epoch,
            } => self.apply_render(target, show, epoch),
            AppJob::InputVerdict {
                sequence,
                text,
                images,
                verdict,
            } => {
                // A later hook may finish first; hold it until every
                // earlier submission has a verdict, then apply the
                // contiguous ordered prefix.
                for (text, images, verdict) in self
                    .input_verdicts
                    .complete(sequence, text, images, verdict)
                {
                    self.apply_input_verdict(text, images, verdict);
                }
            }
            AppJob::CatalogRefreshed => self.rebuild_model_menu(),
            AppJob::Updated(version) => {
                self.notice(format!(
                    "e {version} installed — /reload to switch to it now"
                ));
                self.update_installed = Some(version);
            }
            AppJob::ClipboardPaste {
                generation,
                paste,
                fallback,
            } => self.apply_clipboard_paste(generation, paste, fallback),
            AppJob::Reloaded(host) => self.finish_reload(host),
            AppJob::Shell { cmd, output, epoch } => self.finish_shell(cmd, output, epoch),
        }
    }

    /// A catalog refresh rebuilds an open model picker, keeping its selection.
    fn rebuild_model_menu(&mut self) {
        let Some(menu) = &self.menu else { return };
        if menu.kind != MenuKind::Models {
            return;
        }
        let selected = menu.current().map(|item| item.value.clone());
        self.build_model_menu();
        if let (Some(menu), Some(value)) = (&mut self.menu, selected) {
            menu.select_value(&value);
        }
    }

    /// /reload finished: adopt the restarted host, re-read file-backed looks,
    /// and submit the prompts held while it ran.
    fn finish_reload(&mut self, host: Arc<ExtensionHost>) {
        self.reloading = false;
        self.host = host.clone();
        self.agent.set_host(host);
        // A narrowing the old host's extension installed
        // has no owner left to lift it.
        self.agent.set_active_tools(None);
        self.apply_theme();
        self.apply_keymap();
        self.refresh_status_cache();
        finish_reload_notice(&mut self.transcript, self.reload_block.take());
        for text in std::mem::take(&mut self.held_prompts) {
            self.prompt(text);
        }
    }

    /// A `!` command finished: fill its block, record it for the model, and
    /// submit the prompts held for it.
    fn finish_shell(&mut self, cmd: String, output: e_core::tools::ToolOutput, epoch: u64) {
        // A result from a command started in an earlier
        // session must not be recorded into this one.
        if epoch != self.session_epoch {
            self.notice(format!(
                "`{cmd}` finished after the session changed — output discarded"
            ));
            return;
        }
        // Display a trimmed tail in the live block;
        // history gets the full (tool-truncated) output.
        let display_output = e_core::tools::sanitize_display(&output.content);
        let shown = shell_tail(&display_output);
        let output_id = (!output.content.trim().is_empty())
            .then(|| self.remember_output(format!("$ {cmd}"), display_output));
        if let Some(idx) = self.shell_block.take() {
            if let Some(block) = self.transcript.blocks.get_mut(idx) {
                block.done = true;
                block.is_error = output.is_error();
                block.detail = Some(shown);
                block.output_id = output_id;
                block.touch();
            }
        }
        self.agent.record_user(format!(
            "I ran `{cmd}` in my shell. Output:\n```\n{}\n```",
            output.content
        ));
        // Prompts held for the shell result submit now,
        // ordered after it.
        for text in std::mem::take(&mut self.held_prompts) {
            self.prompt(text);
        }
    }

    /// How a login flow ended. Control flow hangs off the typed outcome; the
    /// human-readable notice arrives separately on `jobs`.
    fn on_login_outcome(&mut self, outcome: LoginOutcome) {
        match outcome {
            LoginOutcome::SignedIn { flow_id, provider }
                if self.login_outcome_is_current(flow_id) =>
            {
                self.signed_in_with(&provider)
            }
            LoginOutcome::Failed { flow_id } if self.login_outcome_is_current(Some(flow_id)) => {
                self.login_task.take();
                if let Some(AuthStage::Waiting { back }) = &self.auth {
                    let back = back.unwrap_or(0);
                    self.auth = Some(AuthStage::Done {
                        ok: false,
                        message: "sign-in did not complete — details in the notice below".into(),
                        back: authpanel::BackTarget::Account(back),
                    });
                } else {
                    self.auth = None;
                }
            }
            // A canceled flow can finish just before its task aborts.
            // Its queued outcome must not affect the replacement flow.
            _ => {}
        }
    }

    /// A sign-in completed: show the outcome beat, refresh the catalog, and
    /// move off a model whose provider is still signed out.
    fn signed_in_with(&mut self, provider: &str) {
        self.login_task.take();
        // Stay in the panel: show the outcome beat, then
        // land back on the account list.
        if let Some(AuthStage::Waiting { back }) = &self.auth {
            let back = back.unwrap_or(0);
            let display = e_core::providers::catalog::display_name(provider);
            self.auth = Some(AuthStage::Done {
                ok: true,
                message: format!("{display} connected"),
                back: authpanel::BackTarget::Account(back),
            });
        }
        e_core::config::home::spawn(e_core::providers::catalog::refresh_remote());
        // A fresh credential may make new models available:
        // if the current model's provider is still signed out,
        // fall back to the first available model.
        if !e_core::auth::signed_in(&e_core::auth::load(), &self.agent.model.provider) {
            if let Some(m) = e_core::providers::catalog::available().into_iter().next() {
                self.notice(format!(
                    "model set to {}",
                    e_core::providers::catalog::slug(&m)
                ));
                self.agent.model = m;
            }
        }
        self.refresh_status_cache();
    }

    /// The quarter-second tick: expire the ctrl+c exit arm and the
    /// "recovered" status beat.
    fn on_tick(&mut self) {
        if let Some(at) = self.armed_at {
            if at.elapsed() > Duration::from_millis(1600) {
                self.armed_at = None;
                self.overlay = None;
            }
        }
        if let Some(s) = &mut self.active {
            let expired = s
                .turn
                .recovered
                .is_some_and(|r| r.since.elapsed() > Duration::from_millis(RECOVERED_VISIBLE_MS));
            if expired {
                s.turn.recovered = None;
            }
        }
    }
}

/// The harness pattern: check for a newer release in the background at
/// launch, install it silently, and say so — the running session is
/// untouched until a restart. Dev builds and the opt-out are exempt.
/// Providers' model lists refresh in the background too (the reference
/// behavior, sourced from each gateway's own /models): a model a provider
/// ships today shows in /models today, no e release involved.
fn spawn_launch_refreshes(results: &Sender<AppJob>) {
    if !e_core::update::is_dev_build() && e_core::config::settings::auto_update() {
        let results = results.clone();
        e_core::config::home::spawn(async move {
            if let Ok(Some(version)) = e_core::update::self_update().await {
                let _ = results.send(AppJob::Updated(version)).await;
            }
        });
    }
    e_core::config::home::spawn(e_core::providers::catalog::refresh_remote());
}

/// The last 20 lines of a shell command's output, with a count of what was
/// cut above them.
fn shell_tail(output: &str) -> String {
    let lines: Vec<&str> = output.lines().collect();
    let tail = &lines[lines.len().saturating_sub(20)..];
    let mut text = tail.join("\n");
    if lines.len() > 20 {
        text = format!("… ({} more lines above)\n{text}", lines.len() - 20);
    }
    text
}

pub(super) fn arm(app: &mut App) {
    app.armed_at = Some(Instant::now());
    app.overlay = Some("press ctrl+c again to exit".into());
}

/// Read terminal events on a thread the frame loop can pause. Each poll
/// waits at most 100 ms, so a pause takes effect within that; the thread
/// ends when the receiver is dropped.
pub(super) fn spawn_input_reader(
    paused: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> tokio::sync::mpsc::Receiver<std::io::Result<TermEvent>> {
    let (tx, rx) = tokio::sync::mpsc::channel(64);
    std::thread::spawn(move || loop {
        if paused.load(std::sync::atomic::Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(50));
            continue;
        }
        match crossterm::event::poll(Duration::from_millis(100)) {
            Ok(true) => {
                let event = crossterm::event::read();
                if tx.blocking_send(event).is_err() {
                    break;
                }
            }
            Ok(false) => {}
            Err(error) => {
                let _ = tx.blocking_send(Err(error));
                break;
            }
        }
    });
    rx
}

/// ctrl+g: hand the terminal to `$VISUAL` / `$EDITOR` (or the `editor`
/// setting) with the draft in a private temp file, then take it back and
/// load what was saved. The painter is stopped and respawned around the
/// hand-off so the next frame repaints from a known-blank screen; the input
/// reader is paused so the editor gets every keystroke.
pub(super) async fn edit_externally(
    app: &mut App,
    painter: &mut Painter,
    input_paused: &std::sync::Arc<std::sync::atomic::AtomicBool>,
    cols: u16,
    rows: u16,
    anchor: usize,
) {
    let command = e_core::config::settings::external_editor();
    let Some(program) = command.first().cloned() else {
        app.notice("no editor: set `editor` in this channel's settings.json or $EDITOR".into());
        return;
    };
    let path = match stage_draft(&app.editor.expanded_text()) {
        Ok(path) => path,
        Err(error) => {
            app.notice(format!("could not stage the draft: {error}"));
            return;
        }
    };
    input_paused.store(true, Ordering::SeqCst);
    // The reader's poll in flight ends within its 100 ms window.
    tokio::time::sleep(Duration::from_millis(150)).await;
    painter.shutdown();
    let _ = pop_terminal_modes();
    let _ = terminal::disable_raw_mode();
    {
        use std::io::Write as _;
        let mut out = std::io::stdout();
        let _ = write!(out, "\x1b[?25h");
        let _ = out.flush();
    }
    let args: Vec<String> = command.iter().skip(1).cloned().collect();
    let edit_path = path.clone();
    let status = tokio::task::spawn_blocking(move || {
        std::process::Command::new(&program)
            .args(&args)
            .arg(&edit_path)
            .status()
    })
    .await;
    let _ = terminal::enable_raw_mode();
    let _ = push_terminal_modes();
    *painter = Painter::spawn(cols, rows, anchor);
    input_paused.store(false, Ordering::SeqCst);
    match status {
        Ok(Ok(status)) if status.success() => match std::fs::read_to_string(&path) {
            Ok(text) => {
                let text = text.trim_end_matches('\n').to_string();
                app.editor.set_text(&text);
                app.sync_menu();
            }
            Err(error) => app.notice(format!("could not read the edited draft: {error}")),
        },
        Ok(Ok(status)) => app.notice(format!("editor exited with {status}; draft unchanged")),
        Ok(Err(error)) => app.notice(format!("could not start the editor: {error}")),
        Err(_) => app.notice("the editor task failed; draft unchanged".into()),
    }
    let _ = std::fs::remove_file(&path);
    painter.frame_in_view(app.frame(cols as usize, rows as usize), app.fixed_view());
}

/// Write the draft to a fresh private temp file for the external editor.
/// An unguessable name and create_new: a pre-placed symlink in the shared
/// temp directory is refused rather than followed.
fn stage_draft(draft: &str) -> std::io::Result<std::path::PathBuf> {
    let path = std::env::temp_dir().join(format!(
        "e-draft-{}-{}.md",
        std::process::id(),
        uuid::Uuid::now_v7()
    ));
    let mut options = std::fs::OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(&path)
        .and_then(|mut f| std::io::Write::write_all(&mut f, draft.as_bytes()))?;
    Ok(path)
}

/// Restores every terminal mode the TUI enables — keyboard enhancement
/// flags, bracketed paste, raw mode, cursor visibility — on every exit
/// path, `?` returns and unwinds included. Popping a mode that never got
/// enabled is harmless; leaving one enabled corrupts the user's shell.
struct TerminalGuard;
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = pop_terminal_modes();
        let _ = terminal::disable_raw_mode();
        use std::io::Write as _;
        let mut out = std::io::stdout();
        let _ = write!(out, "\r\n\x1b[?25h");
        let _ = out.flush();
    }
}
