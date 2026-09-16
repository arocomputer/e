//! Own terminal startup, the event loop, and terminal cleanup.

use super::*;

/// Launch inputs after extension startup hooks have rewritten arguments and cwd.
pub struct RunOptions {
    pub initial: String,
    pub continue_session: bool,
    pub resume_session: bool,
    pub model: Model,
    pub agent: AgentOptions,
    pub images: Vec<crate::core::providers::ImageInput>,
}

/// The channel extensions' own requests travel on: created by the caller
/// before the host starts (so `initialize` can promise a UI), consumed here.
pub type Requests = (
    tokio::sync::mpsc::Sender<crate::core::extensions::HostRequest>,
    tokio::sync::mpsc::Receiver<crate::core::extensions::HostRequest>,
);

/// Open the terminal session and restore terminal state when its loop ends.
pub async fn run(
    options: RunOptions,
    host: std::sync::Arc<crate::core::extensions::ExtensionHost>,
    jobs_tx: tokio::sync::mpsc::Sender<String>,
    jobs_rx: tokio::sync::mpsc::Receiver<String>,
    requests: Requests,
) -> std::io::Result<()> {
    let home = options
        .agent
        .home
        .clone()
        .unwrap_or_else(crate::core::config::home::home);
    crate::core::config::home::scope(home, run_scoped(options, host, jobs_tx, jobs_rx, requests))
        .await
}

/// Run the terminal and its configuration reads within the selected home.
pub(super) async fn run_scoped(
    options: RunOptions,
    host: std::sync::Arc<crate::core::extensions::ExtensionHost>,
    jobs_tx: tokio::sync::mpsc::Sender<String>,
    mut jobs_rx: tokio::sync::mpsc::Receiver<String>,
    requests: Requests,
) -> std::io::Result<()> {
    let (requests_tx, mut requests_rx) = requests;
    let RunOptions {
        initial,
        continue_session,
        resume_session,
        model,
        agent: agent_options,
        images,
    } = options;
    // A panic mid-frame must not strand the shell in raw mode with a hidden
    // cursor or kitty keyboard flags — restore the terminal first, then
    // report as usual. (\x1b[<u pops the keyboard enhancement stack.) Only
    // a panic on this thread — the frame loop, driven by the runtime's
    // block_on — is fatal to the session; the paint thread, tool tasks and
    // the turn worker all run elsewhere and catch their own panics to keep
    // the session alive, so the hook must leave the terminal alone for them
    // (the hook fires before any catch_unwind gets its say).
    {
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

    // Raw mode first so the frame loop can take the terminal over. Theme
    // detection now queries the terminal (OSC 11 background color, then
    // COLORFGBG) so `auto` follows the real terminal theme instead of
    // defaulting to dark. The probe is timeout-bounded and runs here, where
    // the TUI owns the terminal reader, so it can't block startup or swallow
    // keystrokes (audit #93). The guard exists before any further mode
    // changes, so every exit path restores them.
    terminal::enable_raw_mode()?;
    let _guard = TerminalGuard;
    execute!(
        std::io::stdout(),
        EnableBracketedPaste,
        // The kitty keyboard protocol: without it, terminals send plain Enter
        // for shift+enter and multi-line entry is unreachable.
        PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
    )?;
    // detect_light() probes the terminal background over OSC 11 (short
    // timeout) and falls back to COLORFGBG, then dark.
    let detected = crate::tui::background::detect_light().unwrap_or(false);
    let theme = crate::tui::theme::resolve(&crate::core::config::settings::theme(), detected);
    let keymap = crate::tui::keybindings::load();

    let (mut cols, mut rows) = terminal::size()?;
    // The launch anchor: the frame paints below where the user launched e,
    // never over what came before. A terminal that doesn't answer DSR 6n — a raw pty —
    // falls back to the screen's bottom row, the common launch spot.
    let anchor = crate::tui::paint::background::query_cursor_row(rows)
        .unwrap_or(rows.saturating_sub(1)) as usize;
    let mut painter = Painter::spawn(cols, rows, anchor);
    let (mut agent, mut session_events) = Agent::with_options(model, agent_options.clone());
    let (logins_tx, mut logins_rx) =
        tokio::sync::mpsc::channel::<crate::core::auth::login::Outcome>(4);
    let (results_tx, mut results_rx) = tokio::sync::mpsc::channel::<AppJob>(16);
    agent.set_host(host.clone());
    let mut app = App {
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
        show_thinking: crate::core::config::settings::show_thinking(),
        jobs: jobs_tx,
        logins: logins_tx,
        login_task: None,
        login_sequence: 0,
        host,
        results: results_tx,
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
        viewer_cache: None,
        queue_review: None,
        session_epoch: 0,
        update_installed: None,
        relaunch: false,
        rendering_delayed: false,
        last_paint_failure: None,
        light_background: detected,
        bottom_pinned: false,
        live_preview_rows: 5,
        tool_label_rows: 2,
        signed_in: false,
        status_effort: None,
        requests: requests_tx,
        ui_queue: extui::UiQueue::new(),
        ui_prompt: None,
        ext_status: std::collections::BTreeMap::new(),
        ext_activity: std::collections::BTreeMap::new(),
        ext_panel: None,
        pane: None,
        pane_hidden: false,
        widgets: std::collections::BTreeMap::new(),
        layout: crate::core::config::layout::load(),
        external_edit: false,
    };
    app.editor
        .seed_history(crate::tui::history::load(crate::tui::history::RECALL));
    app.refresh_status_cache();
    app.emit(
        "session_start",
        serde_json::json!({
            "reason": "startup",
            "path": app.agent.session_path().map(|p| p.display().to_string()),
        }),
    );
    if app.layout.banner {
        app.transcript
            .push(Block::new(Kind::Banner, crate::VERSION));
    }
    for warning in model::config_warnings() {
        app.notice(format!("warning: {warning}"));
    }
    if !agent_options.save_session {
        app.notice("session saving disabled for this run".into());
    }
    match agent_options.tool_mode {
        crate::core::cli::ToolMode::None => {
            app.notice("no-tools mode — provider requests contain no tool schemas".into())
        }
        crate::core::cli::ToolMode::All => {}
    }
    if crate::core::config::trust::status(&app.agent.cwd()).is_none() {
        app.trust = Some(TrustStage::new(&app.agent.cwd()));
    }
    // The trust lookup may be the first read of trust.json; drain afterward so
    // its recovery joins warnings collected while constructing the app.
    for warning in crate::core::config::store::take_warnings() {
        app.notice(format!("warning: {warning}"));
    }
    // The harness pattern: check for a newer release in the background at
    // launch, install it silently, and say so — the running session is
    // untouched until a restart. Dev builds and the opt-out are exempt.
    if !crate::core::update::is_dev_build() && crate::core::config::settings::auto_update() {
        let results = app.results.clone();
        crate::core::config::home::spawn(async move {
            if let Ok(Some(version)) = crate::core::update::self_update().await {
                let _ = results.send(AppJob::Updated(version)).await;
            }
        });
    }
    // Providers' model lists refresh in the background (the reference
    // behavior, sourced from each gateway's own /models): a model a provider
    // ships today shows in /models today, no e release involved.
    crate::core::config::home::spawn(crate::core::providers::catalog::refresh_remote());
    if crate::core::auth::load().is_empty() {
        app.notice(
            "no provider signed in — use /login to sign in with an account or API key".into(),
        );
        // Route straight to sign-in instead of leaving a phantom model
        // implied on the status bar. Yields to the trust panel above it,
        // if that's showing too — this still renders once trust is settled.
        app.open_login_menu();
    } else if let Some(wanted) = crate::core::config::settings::get_string("model") {
        let current = app.agent.model_slug();
        if wanted != current {
            app.notice(format!(
                "{wanted} is unavailable (provider not signed in) — using {current}"
            ));
        }
    }
    if resume_session {
        // The reference behavior: launch straight into the session picker.
        app.open_resume_menu();
    } else if continue_session {
        app.resume_recent();
    }

    // Terminal tab title: the custom glyph, a dot, the path — a named
    // session takes over the title when one lands (the reference prefers the
    // session name over the workspace).
    set_tab_title(&tab_title(
        &title_path(),
        app.agent.session_name().as_deref(),
    ));
    // Terminal input is read on its own thread and can be paused: while an
    // external editor owns the terminal (ctrl+g), nothing here may read
    // it, or the editor's keystrokes land in e instead.
    let input_paused = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let mut input_rx = spawn_input_reader(input_paused.clone());
    let mut tick = tokio::time::interval(Duration::from_millis(250));
    // SIGTERM/SIGHUP (a kill, a closed tab) exit through the same cleanup as
    // /quit — the terminal is restored, the extension host shut down.
    let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let mut sighup = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::hangup())?;

    // A -r launch prompt must wait for the session pick, or it would start a
    // turn whose reply splices into whichever session gets selected.
    let hold_initial = app.trust.is_some() || (resume_session && app.menu.is_some());
    if let Some(initial) = stage_initial_prompt(initial, hold_initial, &mut app.pending_initial) {
        app.submit_initial(initial);
    }
    painter.frame_in_view(app.frame(cols as usize, rows as usize), false);

    // Frame pacing: every select arm may change what's on screen, but frames
    // are built at most once per interval — a token burst becomes one paint,
    // and a deferred paint fires when the interval lapses.
    const FRAME_INTERVAL: Duration = Duration::from_millis(33);
    let mut next_paint = tokio::time::Instant::now();
    let mut paint_deferred = false;
    let mut event_buf: Vec<SessionEvent> = Vec::with_capacity(128);
    let mut mouse_enabled = false;

    loop {
        if app.external_edit {
            app.external_edit = false;
            edit_externally(&mut app, &mut painter, &input_paused, cols, rows, anchor).await;
        }
        tokio::select! {
            maybe = input_rx.recv() => {
                let Some(Ok(event)) = maybe else { break };
                match event {
                    TermEvent::Paste(text) if app.viewer.is_none() => {
                        app.paste(&text);

                    }
                    TermEvent::Mouse(event) => {
                        if app.viewer.is_some() {
                            match event.kind {
                                crossterm::event::MouseEventKind::ScrollUp => app.scroll_viewer(false, 3, cols as usize, rows as usize),
                                crossterm::event::MouseEventKind::ScrollDown => app.scroll_viewer(true, 3, cols as usize, rows as usize),
                                _ => {}
                            }
                        } else {
                            app.pane_mouse(event, cols as usize);
                        }
                    },
                    TermEvent::Resize(c, r) => {
                        cols = c;
                        rows = r;
                        painter.resize(c, r);
                    }
                    TermEvent::Key(k) if k.kind != crossterm::event::KeyEventKind::Release => {
                        let ctrl = k.modifiers.contains(KeyModifiers::CONTROL);
                        let system_paste = k
                            .modifiers
                            .intersects(KeyModifiers::SUPER | KeyModifiers::META)
                            && matches!(k.code, KeyCode::Char('v') | KeyCode::Char('V'));
                        if ctrl && k.code == KeyCode::Char('c') {
                            app.interrupt_or_exit();
                        } else if ((ctrl && k.code == KeyCode::Char('v')) || system_paste)
                            && app.composer_free()
                            && !app.editor.mask
                        {
                            app.paste_clipboard();
                        } else if app.viewer_key(k, cols as usize, rows as usize) {

                        } else if ctrl && k.code == KeyCode::Char('o') {
                            app.viewer = Some(Viewer::new());
                        } else if app.pane.is_some()
                            && app.menu.is_none()
                            && app.settings.is_none()
                            && app.auth.is_none()
                            && app.trust.is_none()
                            && !app.ui_input_open()
                            && app.pending_key.is_none()
                            && extui::chord_of(&k).as_deref() == Some(app.layout.focus.as_str())
                        {
                            // The layout's focus chord moves between the
                            // conversation and the pane.
                            if let Some(pane) = app.pane.as_mut() {
                                pane.focused = !pane.focused;
                            }
                        } else if app.pane.as_ref().is_some_and(|p| p.focused)
                            && app.menu.is_none()
                            && app.settings.is_none()
                            && app.auth.is_none()
                            && app.trust.is_none()
                            && !app.ui_input_open()
                            && !(ctrl && k.code == KeyCode::Char('c'))
                        {
                            // The pane owns the keyboard: e navigates it,
                            // and chords it does not use go to the owner.
                            // ctrl+c stays e's.
                            let width = cols as usize;
                            if let Some(pane) = app.pane.as_mut() {
                                let action = pane.key(k, width);
                                app.pane_action(action);
                            }
                        } else if app.ext_panel.as_ref().is_some_and(|p| p.interactive)
                            && app.menu.is_none()
                            && app.settings.is_none()
                            && app.auth.is_none()
                            && app.trust.is_none()
                            && !(ctrl && k.code == KeyCode::Char('c'))
                        {
                            // The panel owns the keyboard: Esc closes it
                            // here, every other chord goes to its owner.
                            // ctrl+c stays e's.
                            if k.code == KeyCode::Esc {
                                app.close_ext_panel(true);
                            } else {
                                app.forward_panel_key(&k);
                            }
                        } else if k.code == KeyCode::Esc
                            && app.ext_panel.is_some()
                            && app.menu.is_none()
                            && app.settings.is_none()
                            && app.auth.is_none()
                            && app.trust.is_none()
                            && !app.ui_input_open()
                        {
                            app.close_ext_panel(true);
                        } else if k.code == KeyCode::Esc && app.ui_input_open() {
                            app.cancel_ui_prompt();
                        } else if let Some(stage) = &mut app.trust {
                            match k.code {
                                KeyCode::Up => stage.step(-1),
                                KeyCode::Down => stage.step(1),
                                KeyCode::PageUp => stage.page(-1, cols as usize, (rows as usize).saturating_sub(1)),
                                KeyCode::PageDown => stage.page(1, cols as usize, (rows as usize).saturating_sub(1)),
                                KeyCode::Enter => {
                                    // The middle row (when offered) trusts the
                                    // broader ancestor; trust propagates down,
                                    // so the workspace is covered too.
                                    let (parent, trusted) = stage.choice();
                                    let target = parent.unwrap_or_else(|| app.agent.cwd().to_path_buf());
                                    if !trusted {
                                        // A decline is remembered nowhere: e
                                        // runs only trusted, so the next
                                        // launch asks again.
                                        if let Some(refusal) = crate::core::config::trust::refusal(&target) {
                                            app.notice(refusal);
                                        }
                                        app.should_quit = true;
                                    } else {
                                        match crate::core::config::trust::set(&target, true) {
                                            Err(e) => app.notice(format!("trust: {e}")),
                                            Ok(()) => {
                                                app.trust = None;
                                                app.install_project_packages();
                                                // An open -r picker still owns
                                                // the launch prompt; submitting
                                                // it now would start a turn the
                                                // session pick then refuses.
                                                if app.menu.is_none() {
                                                    if let Some(initial) = app.pending_initial.take() {
                                                        app.submit_initial(initial);
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                                _ => {}
                            }
                        } else if let Some(panel) = &mut app.settings {
                            let mut setting_error = None;
                            let changing_effort = panel.selected_key() == Some("effort")
                                && matches!(k.code, KeyCode::Left | KeyCode::Right);
                            match k.code {
                                KeyCode::Up => panel.step(-1),
                                KeyCode::Down => panel.step(1),
                                KeyCode::Left => setting_error = panel.change(-1).err(),
                                KeyCode::Right => setting_error = panel.change(1).err(),
                                // Esc alone closes: Enter opens the panel
                                // from the command menu, so it must not be
                                // the same key that dismisses it.
                                KeyCode::Esc => app.settings = None,
                                _ => {}
                            }
                            if let Some(error) = setting_error {
                                app.notice(format!("could not save setting: {error}"));
                            } else if changing_effort {
                                app.agent.use_saved_effort();
                            }
                            // A theme change applies immediately; settings can
                            // also change what the statusline derives from disk.
                            // The thinking toggle and keymap are file-backed
                            // too — re-read them so a mid-session change
                            // lands this frame.
                            app.apply_theme();
                            app.apply_keymap();
                            app.show_thinking =
                                crate::core::config::settings::show_thinking();
                            app.refresh_status_cache();
                        } else if let Some(stage) = &mut app.auth {
                            match (&mut *stage, k.code) {
                                (AuthStage::Choose { selected }, KeyCode::Up | KeyCode::Down) => {
                                    *selected = 1 - *selected;
                                }
                                (AuthStage::Choose { selected }, KeyCode::Enter) => {
                                    let choice = *selected;
                                    app.auth_choose(choice);
                                }
                                (AuthStage::Account { selected }, KeyCode::Up | KeyCode::Down) => {
                                    let n = crate::core::providers::registry::oauth_providers().len();
                                    *selected = (*selected + 1) % n.max(1);
                                }
                                (AuthStage::Key { selected }, KeyCode::Up) => {
                                    let n = crate::core::providers::registry::key_providers().len();
                                    *selected = (*selected + n - 1) % n.max(1);
                                }
                                (AuthStage::Key { selected }, KeyCode::Down) => {
                                    let n = crate::core::providers::registry::key_providers().len();
                                    *selected = (*selected + 1) % n.max(1);
                                }
                                (AuthStage::Account { selected }, KeyCode::Enter) => {
                                    let choice = *selected;
                                    app.auth_account(choice);
                                }
                                (AuthStage::Key { selected }, KeyCode::Enter) => {
                                    let choice = *selected;
                                    app.auth_key(choice);
                                }
                                (_, KeyCode::Esc) => {
                                    // Esc closes the whole panel from any
                                    // depth; an in-flight flow is cancelled
                                    // with it.
                                    let waiting = matches!(&*stage, AuthStage::Waiting { .. });
                                    let cancelled = app.cancel_login();
                                    app.auth = None;
                                    app.pending_key = None;
                                    app.editor.mask = false;
                                    app.editor.set_text("");
                                    app.discard_composer_images();
                                    if waiting && cancelled {
                                        app.notice("login cancelled".into());
                                    }
                                }
                                (AuthStage::Account { .. }, KeyCode::Backspace) => {
                                    *stage = AuthStage::Choose { selected: 0 };
                                }
                                (AuthStage::Key { .. }, KeyCode::Backspace) => {
                                    *stage = AuthStage::Choose { selected: 1 };
                                }
                                // The entry keeps Backspace for editing while
                                // there is text; an empty input navigates back.
                                (AuthStage::ApiKey { .. }, KeyCode::Backspace)
                                    if app.editor.is_empty() =>
                                {
                                    // The outer guard matched ApiKey; if the
                                    // stage somehow moved on, fall through to
                                    // the default handling instead of panicking.
                                    if let AuthStage::ApiKey { provider } = &*stage {
                                        let provider = provider.clone();
                                        app.pending_key = None;
                                        app.editor.mask = false;
                                        let selected = crate::core::providers::registry::key_providers()
                                            .iter()
                                            .position(|p| p.name == provider)
                                            .unwrap_or(0);
                                        *stage = AuthStage::Key { selected };
                                    }
                                }
                                (AuthStage::Waiting { back }, KeyCode::Backspace) => {
                                    let back = *back;
                                    let cancelled = app.cancel_login();
                                    match back {
                                        Some(selected) => {
                                            app.auth = Some(AuthStage::Account { selected });
                                            if cancelled {
                                                app.notice("login cancelled".into());
                                            }
                                        }
                                        // Launched by `/login <provider>`: no
                                        // list to return to, so close.
                                        None => {
                                            app.auth = None;
                                            if cancelled {
                                                app.notice("login cancelled".into());
                                            }
                                        }
                                    }
                                }
                                (AuthStage::Done { back, .. }, KeyCode::Enter | KeyCode::Backspace) => {
                                    *stage = back.stage();
                                }
                                (AuthStage::ApiKey { .. }, _) => {
                                    if let Some(key) = key_of(&k, &app.keymap) {
                                        if let EditorResult::Submit(text) = app.editor.key(key) {
                                            app.submit(text);
                                        }
                                    }
                                }
                                _ => {}
                            }
                        } else if app
                            .menu
                            .as_ref()
                            .map(|m| m.kind == MenuKind::Scoped)
                            .unwrap_or(false)
                            && ((k.code == KeyCode::Char(' ') && !ctrl)
                                || (ctrl && matches!(k.code, KeyCode::Char('x') | KeyCode::Char('s'))))
                        {
                            match (k.code, ctrl) {
                                (KeyCode::Char('x'), true) => {
                                    // Reset: stage nothing — the picker
                                    // mirrors "no scope" and Ctrl+S saves it
                                    // (or Ctrl+X again is enough to walk
                                    // back). Nothing hits settings.json
                                    // until Ctrl+S.
                                    app.staged_scope = Some(Vec::new());
                                    app.open_scoped_menu();
                                }
                                (KeyCode::Char('s'), true) => app.save_scope(),
                                _ => app.toggle_scoped(),
                            }
                        } else if app.menu.is_some()
                            && (matches!(k.code, KeyCode::Up | KeyCode::Down | KeyCode::Enter | KeyCode::Esc)
                                || (k.code == KeyCode::Tab
                                    && !k.modifiers.contains(KeyModifiers::SHIFT)
                                    && app
                                        .menu
                                        .as_ref()
                                        .is_some_and(|menu| menu.has_tabs()))
                                // Shift+tab belongs to the picker while it
                                // is open: it steps the tabs backward. The
                                // effort shortcut stays a bare-composer key.
                                || (k.code == KeyCode::BackTab
                                    && app
                                        .menu
                                        .as_ref()
                                        .is_some_and(|menu| menu.has_tabs())))
                            && !ctrl
                        {
                            match k.code {
                                KeyCode::Up => {
                                    if let Some(menu) = app.menu.as_mut() {
                                        menu.step(-1);
                                    }
                                }
                                KeyCode::Down => {
                                    if let Some(menu) = app.menu.as_mut() {
                                        menu.step(1);
                                    }
                                }
                                KeyCode::Tab => {
                                    if let Some(menu) = app.menu.as_mut() {
                                        menu.cycle_tab();
                                    }
                                }
                                KeyCode::BackTab => {
                                    if let Some(menu) = app.menu.as_mut() {
                                        menu.cycle_tab_back();
                                    }
                                }
                                KeyCode::Enter => { app.select_menu(); }
                                KeyCode::Esc => {
                                    if app.menu.as_ref().is_some_and(|m| m.kind == MenuKind::Extension) {
                                        app.cancel_ui_prompt();
                                    }
                                    if app.menu.as_ref().is_some_and(|m| m.filter_without_trigger && m.kind != MenuKind::Commands) {
                                        app.editor.set_text("");
                                    }
                                    app.menu = None;
                                    // Closing the scoped picker without
                                    // Ctrl+S discards its staged edits.
                                    app.staged_scope = None;
                                    // Declining the -r picker releases a
                                    // held launch prompt into the current
                                    // session.
                                    app.release_initial_prompt();
                                }
                                _ => {}
                            }
                        } else if k.code == KeyCode::Esc && app.pending_key.is_some() {
                            app.pending_key = None;
                            app.editor.mask = false;
                            app.editor.set_text("");
                            app.discard_composer_images();
                            app.notice("login cancelled".into());
                        } else if k.code == KeyCode::Esc && app.agent.is_streaming() {
                            app.agent.interrupt();
                        } else if ctrl
                            && k.code == KeyCode::Char('g')
                            && app.menu.is_none()
                            && app.settings.is_none()
                            && app.auth.is_none()
                            && app.trust.is_none()
                            && (!app.ui_input_open() || app.ui_editor_open())
                            && app.pending_key.is_none()
                        {
                            // Deferred to the top of the loop: the terminal
                            // hand-off needs the painter and the reader,
                            // which the key handler does not own.
                            app.external_edit = true;
                        } else if ctrl && matches!(k.code, KeyCode::Char('p') | KeyCode::Char('P')) {
                            let backward = k.code == KeyCode::Char('P')
                                || k.modifiers.contains(KeyModifiers::SHIFT);
                            app.cycle_model(!backward);
                        } else if app.menu.is_none()
                            && app.settings.is_none()
                            && app.auth.is_none()
                            && app.trust.is_none()
                            && (k.code == KeyCode::BackTab
                                || (k.code == KeyCode::Tab
                                    && !ctrl
                                    && k.modifiers.contains(KeyModifiers::SHIFT)))
                        {
                            // Shift+tab cycles the model's declared levels —
                            // a bare-composer shortcut. With a picker or
                            // panel open the keys belong to that surface;
                            // the effort setting must not mutate unseen
                            // beneath it. The statusline confirms the
                            // change; nothing lands in the transcript.
                            match app.agent.cycle_effort() {
                                Ok(Some(level)) => {
                                    app.refresh_status_cache();
                                    app.emit("effort_change", serde_json::json!({"effort": level}));
                                }
                                Ok(None) => {}
                                Err(error) => app.notice(format!("could not save reasoning effort: {error}")),
                            }
                        } else if !ctrl && app.queue_review_key(k.code) {
                            // Consumed by the queued-prompt review.
                        } else if let Some(key) = key_of(&k, &app.keymap) {
                            if let EditorResult::Submit(text) = app.editor.key(key) {
                                app.submit_composer(text);
                            }
                            app.sync_menu();
                        } else if let Some(chord) = extui::chord_of(&k)
                            .filter(|chord| app.host.has_shortcut(chord))
                            .filter(|_| !app.ui_input_open() && app.pending_key.is_none())
                        {
                            // A declared extension shortcut, answered like
                            // a command. Only a chord neither e nor the
                            // composer (built in, or the user's
                            // keybindings.json) took reaches this branch —
                            // so unbinding a chord there frees it for an
                            // extension.
                            let host = app.host.clone();
                            let results = app.results.clone();
                            let epoch = app.session_epoch;
                            crate::core::config::home::spawn(async move {
                                let result = host.run_shortcut(&chord).await;
                                let _ = results.send(AppJob::Command { result, epoch }).await;
                            });
                        }
                    }
                    _ => {}
                }
            }
            count = session_events.recv_many(&mut event_buf, 128) => {
                if count == 0 {
                    break;
                }
                // Apply the whole burst before building one frame — a fast
                // stream must not cost one paint per delta.
                for e in event_buf.drain(..) {
                    app.on_session_event(e);
                }
            }
            request = requests_rx.recv() => {
                if let Some(request) = request {
                    app.on_host_request(request);
                }
            }
            job = results_rx.recv() => {
                match job {
                    Some(AppJob::Command { result, epoch }) => {
                        app.deliver_command_result(result, epoch);
                    }
                    Some(AppJob::Completions { command, prefix, items }) => {
                        app.show_completions(&command, &prefix, items);
                    }
                    Some(AppJob::Rendered { target, show, epoch }) => {
                        app.apply_render(target, show, epoch);
                    }
                    Some(AppJob::InputVerdict { sequence, text, images, verdict }) => {
                        // A later hook may finish first; hold it until every
                        // earlier submission has a verdict, then apply the
                        // contiguous ordered prefix.
                        for (text, images, verdict) in
                            app.input_verdicts.complete(sequence, text, images, verdict)
                        {
                            app.apply_input_verdict(text, images, verdict);
                        }
                    }
                    Some(AppJob::CatalogRefreshed) => {
                        if let Some(menu) = &app.menu {
                            if menu.kind == MenuKind::Models {
                                let selected =
                                    menu.current().map(|item| item.value.clone());
                                app.build_model_menu();
                                if let (Some(menu), Some(value)) =
                                    (&mut app.menu, selected)
                                {
                                    menu.select_value(&value);
                                }
                            }
                        }
                    }
                    Some(AppJob::Updated(version)) => {
                        app.notice(format!(
                            "e {version} installed — /reload to switch to it now"
                        ));
                        app.update_installed = Some(version);
                    }
                    Some(AppJob::ClipboardPaste {
                        generation,
                        paste,
                        fallback,
                    }) => {
                        app.apply_clipboard_paste(generation, paste, fallback);
                    }
                    Some(AppJob::Reloaded(host)) => {
                        app.reloading = false;
                        app.host = host.clone();
                        app.agent.set_host(host);
                        // A narrowing the old host's extension installed
                        // has no owner left to lift it.
                        app.agent.set_active_tools(None);
                        app.apply_theme();
                        app.apply_keymap();
                        app.refresh_status_cache();
                        finish_reload_notice(&mut app.transcript, app.reload_block.take());
                        for text in std::mem::take(&mut app.held_prompts) {
                            app.prompt(text);
                        }
                    }
                    Some(AppJob::Shell { cmd, output, epoch }) => {
                        // A result from a command started in an earlier
                        // session must not be recorded into this one.
                        if epoch != app.session_epoch {
                            app.notice(format!(
                                "`{cmd}` finished after the session changed — output discarded"
                            ));
                        } else {
                            // Display a trimmed tail in the live block;
                            // history gets the full (tool-truncated) output.
                            let display_output =
                                crate::core::tools::sanitize_display(&output.content);
                            let shown: String = {
                                let lines: Vec<&str> = display_output.lines().collect();
                                let tail = &lines[lines.len().saturating_sub(20)..];
                                let mut text = tail.join("\n");
                                if lines.len() > 20 {
                                    text = format!(
                                        "… ({} more lines above)\n{text}",
                                        lines.len() - 20
                                    );
                                }
                                text
                            };
                            let output_id = (!output.content.trim().is_empty()).then(|| app.remember_output(format!("$ {cmd}"), display_output));
                            if let Some(idx) = app.shell_block.take() {
                                if let Some(block) = app.transcript.blocks.get_mut(idx) {
                                    block.done = true;
                                    block.is_error = output.is_error();
                                    block.detail = Some(shown);
                                    block.output_id = output_id;
                                    block.touch();
                                }
                            }
                            app.agent.record_user(format!(
                                "I ran `{cmd}` in my shell. Output:\n```\n{}\n```",
                                output.content
                            ));
                            // Prompts held for the shell result submit now,
                            // ordered after it.
                            for text in std::mem::take(&mut app.held_prompts) {
                                app.prompt(text);
                            }
                        }
                    }
                    None => {}
                }
            }
            message = jobs_rx.recv() => {
                if let Some(message) = message {
                    app.notice(message);
                }
            }
            outcome = logins_rx.recv() => {
                // Control flow hangs off the typed outcome; the human-readable
                // notice arrives separately on `jobs`.
                match outcome {
                    Some(crate::core::auth::login::Outcome::SignedIn { flow_id, provider })
                        if app.login_outcome_is_current(flow_id) =>
                    {
                            app.login_task.take();
                            // Stay in the panel: show the outcome beat, then
                            // land back on the account list.
                            if let Some(AuthStage::Waiting { back }) = &app.auth {
                                let back = back.unwrap_or(0);
                                let display =
                                    crate::core::providers::catalog::display_name(&provider);
                                app.auth = Some(AuthStage::Done {
                                    ok: true,
                                    message: format!("{display} connected"),
                                    back: authpanel::BackTarget::Account(back),
                                });
                            }
                            crate::core::config::home::spawn(crate::core::providers::catalog::refresh_remote());
                            // A fresh credential may make new models available:
                            // if the current model's provider is still signed out,
                            // fall back to the first available model.
                            if !crate::core::auth::signed_in(&crate::core::auth::load(), &app.agent.model.provider) {
                                if let Some(m) = crate::core::providers::catalog::available().into_iter().next() {
                                    app.notice(format!("model set to {}", crate::core::providers::catalog::slug(&m)));
                                    app.agent.model = m;
                                }
                            }
                            app.refresh_status_cache();
                        }
                    Some(crate::core::auth::login::Outcome::Failed { flow_id })
                        if app.login_outcome_is_current(Some(flow_id)) => {
                            app.login_task.take();
                            if let Some(AuthStage::Waiting { back }) = &app.auth {
                                let back = back.unwrap_or(0);
                                app.auth = Some(AuthStage::Done {
                                    ok: false,
                                    message:
                                        "sign-in did not complete — details in the notice below"
                                            .into(),
                                    back: authpanel::BackTarget::Account(back),
                                });
                            } else {
                                app.auth = None;
                            }
                        }
                    // A canceled flow can finish just before its task aborts.
                    // Its queued outcome must not affect the replacement flow.
                    Some(_) => {}
                    None => {}
                }
            }
            _ = sigterm.recv() => break,
            _ = sighup.recv() => break,
            // A paint was skipped inside the frame interval; fire it when
            // the interval lapses.
            _ = tokio::time::sleep_until(next_paint), if paint_deferred => {}
            _ = tick.tick() => {
                if let Some(at) = app.armed_at {
                    if at.elapsed() > Duration::from_millis(1600) {
                        app.armed_at = None;
                        app.overlay = None;
                    }
                }
                if let Some(s) = &mut app.active {
                    let expired = s.turn.recovered
                        .is_some_and(|r| r.since.elapsed() > Duration::from_millis(RECOVERED_VISIBLE_MS));
                    if expired {
                        s.turn.recovered = None;
                    }
                }
            }
        }
        let capture_mouse = app.viewer.is_some() || app.pane.is_some();
        if capture_mouse != mouse_enabled {
            if capture_mouse {
                let _ = execute!(std::io::stdout(), EnableMouseCapture);
            } else {
                let _ = execute!(std::io::stdout(), DisableMouseCapture);
            }
            mouse_enabled = capture_mouse;
        }
        let paint_status = painter.status();
        app.rendering_delayed = paint_status.delayed(Duration::from_millis(500));
        match paint_status.failure.as_ref().map(|(_, error)| error) {
            Some(error) if app.last_paint_failure.as_ref() != Some(error) => {
                app.notice(format!("render failed: {error}"));
                app.last_paint_failure = Some(error.clone());
            }
            None => app.last_paint_failure = None,
            Some(_) => {}
        }

        let now = tokio::time::Instant::now();
        if now >= next_paint || app.should_quit {
            let frame = if app.viewer.is_some() {
                app.viewer_frame(cols as usize, rows as usize)
            } else {
                app.frame(cols as usize, rows as usize)
            };
            // A split beside a pane is a fixed-height frame: it paints on
            // the alternate screen, like the viewer, so the transcript and
            // the terminal's scrollback come back untouched when it closes.
            painter.frame_in_view(frame, app.viewer.is_some() || app.pane.is_some());
            next_paint = now + FRAME_INTERVAL;
            paint_deferred = false;
        } else {
            paint_deferred = true;
        }
        if app.should_quit {
            break;
        }
    }

    // Let the final frame land before the terminal is restored. Shells use
    // detached process groups, so stop them explicitly before this process
    // gives extensions their shutdown notification.
    painter.shutdown();
    crate::core::tools::kill_tracked_processes();
    app.host
        .event("session_shutdown", serde_json::json!({"reason": "quit"}))
        .await;
    app.host.shutdown().await;
    drop(_guard);
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
    Ok(())
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
    let command = crate::core::config::settings::external_editor();
    let Some(program) = command.first().cloned() else {
        app.notice("no editor: set `editor` in this channel's settings.json or $EDITOR".into());
        return;
    };
    let draft = app.editor.expanded_text();
    // An unguessable name and create_new: a pre-placed symlink in the shared
    // temp directory is refused rather than followed.
    let path = std::env::temp_dir().join(format!(
        "e-draft-{}-{}.md",
        std::process::id(),
        uuid::Uuid::now_v7()
    ));
    {
        let mut options = std::fs::OpenOptions::new();
        options.create_new(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let written = options
            .open(&path)
            .and_then(|mut f| std::io::Write::write_all(&mut f, draft.as_bytes()));
        if let Err(error) = written {
            app.notice(format!("could not stage the draft: {error}"));
            return;
        }
    }
    input_paused.store(true, std::sync::atomic::Ordering::SeqCst);
    // The reader's poll in flight ends within its 100 ms window.
    tokio::time::sleep(Duration::from_millis(150)).await;
    painter.shutdown();
    let _ = execute!(
        std::io::stdout(),
        PopKeyboardEnhancementFlags,
        DisableBracketedPaste
    );
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
    let _ = execute!(
        std::io::stdout(),
        EnableBracketedPaste,
        PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
    );
    *painter = Painter::spawn(cols, rows, anchor);
    input_paused.store(false, std::sync::atomic::Ordering::SeqCst);
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
    painter.frame(app.frame(cols as usize, rows as usize));
}

/// Restores every terminal mode the TUI enables — keyboard enhancement
/// flags, bracketed paste, raw mode, cursor visibility — on every exit
/// path, `?` returns and unwinds included. Popping a mode that never got
/// enabled is harmless; leaving one enabled corrupts the user's shell.
struct TerminalGuard;
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = execute!(
            std::io::stdout(),
            PopKeyboardEnhancementFlags,
            DisableBracketedPaste,
            DisableMouseCapture
        );
        let _ = terminal::disable_raw_mode();
        use std::io::Write as _;
        let mut out = std::io::stdout();
        let _ = write!(out, "\r\n\x1b[?25h");
        let _ = out.flush();
    }
}
