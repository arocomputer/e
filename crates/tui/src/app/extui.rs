//! The extension surface, frontend side: the `ui.*` and `session.*`
//! requests an extension sends, answered here on the
//! user's behalf, plus the lifecycle events the frontend alone can emit
//! (session start and shutdown, model and effort changes).
//!
//! Everything an extension shows is data painted by ulo through the theme:
//! a `show` becomes a transcript block, a `select` the ordinary picker, a
//! `panel` a footer surface framed like every other one, a `pane` a side
//! pane beside the conversation (`tui/surfaces/pane.rs`), a `widget` rows
//! above the composer, a `status` a bounded slot on the status row. Modal
//! requests (`select`, `confirm`, `input`) queue first-come across
//! extensions; one is open at a time, and Esc answers it "cancelled".
//! Text is sanitized before paint and styled only through theme tokens, so
//! an extension can neither emit an escape sequence nor use a colour the
//! user's theme does not define.

use std::collections::VecDeque;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde_json::{json, Value};

use super::*;
use crate::menu::{Menu, MenuItem, MenuKind, HINT_USE};
pub(crate) use crate::pane::Span;
use crate::pane::{spans_of, Action, Pane};
use ulo_core::extensions::{CommandResult, HostRequest, Show};

/// Widest a `ui.status` slot paints; longer text ends in an ellipsis.
const STATUS_COLUMNS: usize = 40;
/// Most rows every extension's widgets may take above the composer.
const WIDGET_MAX_ROWS: usize = 8;
/// Most rows a `ui.panel` may carry; the rest are dropped with a last row
/// saying so, since a panel is a glance, not a document.
const PANEL_MAX_LINES: usize = 200;
/// Widest a picker or panel title paints.
const TITLE_COLUMNS: usize = 60;

pub(super) const HINT_INPUT: &str = "Enter Answer     Esc Cancel";
pub(super) const HINT_EDITOR: &str =
    "Enter Answer     Shift+Enter Newline     Ctrl+G Editor     Esc Cancel";
pub(super) const HINT_PANEL: &str = "Esc Close";
pub(super) const HINT_PANEL_INTERACTIVE: &str = "Keys go to the extension     Esc Close";

/// An extension's footer panel: one slot, last writer wins. Interactive
/// panels receive keys as `ui.key` notifications until closed.
pub(super) struct ExtPanel {
    pub extension: String,
    pub title: String,
    pub lines: Vec<Vec<Span>>,
    pub interactive: bool,
}

impl ExtPanel {
    pub fn render(&self, theme: &Theme, width: usize) -> Vec<String> {
        let header = theme.fg("userMessageText", &self.title);
        let body = self
            .lines
            .iter()
            .map(|line| {
                let mut row = String::from("  ");
                for span in line {
                    match &span.token {
                        Some(token) => row.push_str(&theme.fg(token, &span.text)),
                        None => row.push_str(&span.text),
                    }
                }
                crate::markdown::clip_styled(&row, width)
            })
            .collect();
        crate::panel::frame(theme, width, header, body)
    }
}

/// The open modal request, waiting on the user.
pub(super) enum UiPrompt {
    /// The picker is open with `MenuKind::Extension`; Enter answers.
    Select(HostRequest),
    /// The picker holds Yes/No; Enter answers `confirmed`.
    Confirm(HostRequest),
    /// The composer is the answer field. `multiline` is `ui.editor`:
    /// shift+enter breaks a line, ctrl+g opens the external editor.
    Input {
        request: HostRequest,
        title: String,
        placeholder: String,
        multiline: bool,
    },
}

impl UiPrompt {
    fn cancel(self) {
        match self {
            UiPrompt::Select(request) | UiPrompt::Input { request, .. } => {
                request.respond(Ok(json!({"cancelled": true})))
            }
            UiPrompt::Confirm(request) => request.respond(Ok(json!({"confirmed": false}))),
        }
    }
}

/// A canonical chord for a key event — the keybindings grammar plus the
/// keys the composer never binds (`escape`, `tab`, `space`, paging) so an
/// interactive panel can see them. None for keys without a name.
pub(crate) fn chord_of(event: &KeyEvent) -> Option<String> {
    use crate::keybindings::base_name;
    use ulo_core::config::chord::chord_string;
    let ctrl = event.modifiers.contains(KeyModifiers::CONTROL);
    let alt = event.modifiers.contains(KeyModifiers::ALT);
    let mut shift = event.modifiers.contains(KeyModifiers::SHIFT);
    let base = match event.code {
        KeyCode::Esc => "escape".to_string(),
        KeyCode::Tab => "tab".to_string(),
        KeyCode::BackTab => {
            shift = true;
            "tab".to_string()
        }
        KeyCode::PageUp => "pageup".to_string(),
        KeyCode::PageDown => "pagedown".to_string(),
        KeyCode::Char(' ') => "space".to_string(),
        KeyCode::Char(c) => {
            if c.is_uppercase() {
                shift = true;
            }
            c.to_lowercase().to_string()
        }
        code => base_name(code)?,
    };
    Some(chord_string(ctrl, alt, shift, &base))
}

fn text_of(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(ulo_core::tools::sanitize_display)
        .unwrap_or_default()
}

fn one_line(text: &str, max: usize) -> String {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if line.chars().count() <= max {
        line
    } else {
        let head: String = line.chars().take(max.saturating_sub(1)).collect();
        format!("{head}…")
    }
}

/// A span's text on one row: control sequences stripped, newlines folded to
/// spaces, every other space kept — alignment is the extension's to draw.
fn flat(text: &str) -> String {
    ulo_core::tools::sanitize_display(text).replace('\n', " ")
}

/// Panel lines from JSON: a string paints plain, an array of
/// `{text, token}` spans paints each run through the theme.
fn panel_lines(value: &Value) -> Vec<Vec<Span>> {
    let Some(lines) = value.as_array() else {
        return Vec::new();
    };
    let mut out: Vec<Vec<Span>> = lines.iter().take(PANEL_MAX_LINES).map(spans_of).collect();
    if lines.len() > PANEL_MAX_LINES {
        out.push(vec![Span {
            text: format!("… {} more lines not shown", lines.len() - PANEL_MAX_LINES),
            token: Some("dim".into()),
        }]);
    }
    out
}

impl App {
    /// Emit a lifecycle event to subscribed extensions without waiting.
    pub(super) fn emit(&self, name: &'static str, params: Value) {
        let host = self.host.clone();
        ulo_core::config::home::spawn(async move { host.event(name, params).await });
    }

    /// Whether the composer is currently an extension's answer field.
    pub(super) fn ui_input_open(&self) -> bool {
        matches!(self.ui_prompt, Some(UiPrompt::Input { .. }))
    }

    /// Whether the open answer field is a `ui.editor`, which may hand the
    /// draft to the external editor.
    pub(super) fn ui_editor_open(&self) -> bool {
        matches!(
            self.ui_prompt,
            Some(UiPrompt::Input {
                multiline: true,
                ..
            })
        )
    }

    /// Whether some other surface owns the footer, so a modal must wait.
    fn surface_busy(&self) -> bool {
        self.ui_prompt.is_some()
            || self.menu.is_some()
            || self.settings.is_some()
            || self.auth.is_some()
            || self.trust.is_some()
            || self.pending_key.is_some()
            || self.viewer.is_some()
    }

    /// Open the next queued modal when the footer is free. Called from
    /// every frame, so a modal waits out a picker rather than fighting it.
    pub(super) fn pump_ui_queue(&mut self) {
        if self.surface_busy() {
            return;
        }
        let Some(request) = self.ui_queue.pop_front() else {
            return;
        };
        self.open_prompt(request);
    }

    /// Show a queued modal request: `select` and `confirm` open the
    /// picker, `input` and `editor` make the composer the answer field.
    fn open_prompt(&mut self, request: HostRequest) {
        let title = one_line(&text_of(&request.params, "title"), TITLE_COLUMNS);
        match request.method.as_str() {
            "ui.select" => self.open_select(request, title),
            "ui.confirm" => self.open_confirm(request, title),
            "ui.input" | "ui.editor" => self.open_input(request, title),
            _ => request.respond(Err("not a modal request".into())),
        }
    }

    /// `ui.select`: the offered options in the ordinary picker.
    fn open_select(&mut self, request: HostRequest, title: String) {
        let items = select_items(&request.params);
        if items.is_empty() {
            request.respond(Err("select needs at least one option".into()));
            return;
        }
        let title = or_default(title, "Choose");
        self.menu = Some(Menu::new(MenuKind::Extension, title, HINT_USE, items).without_trigger());
        self.ui_prompt = Some(UiPrompt::Select(request));
    }

    /// `ui.confirm`: a Yes/No picker, the message beside Yes.
    fn open_confirm(&mut self, request: HostRequest, title: String) {
        let message = one_line(&text_of(&request.params, "message"), TITLE_COLUMNS);
        let yes = MenuItem::new("Yes", &message, "yes");
        let no = MenuItem::new("No", "", "no");
        let title = or_default(title, "Confirm");
        self.menu =
            Some(Menu::new(MenuKind::Extension, title, HINT_USE, vec![yes, no]).without_trigger());
        self.ui_prompt = Some(UiPrompt::Confirm(request));
    }

    /// `ui.input` / `ui.editor`: the composer, seeded and possibly masked,
    /// becomes the answer field.
    fn open_input(&mut self, request: HostRequest, title: String) {
        let multiline = request.method == "ui.editor";
        let placeholder = one_line(&text_of(&request.params, "placeholder"), TITLE_COLUMNS);
        // `ui.editor` seeds the draft from `text`; `ui.input` from
        // `prefill`.
        let prefill = if multiline {
            text_of(&request.params, "text")
        } else {
            text_of(&request.params, "prefill")
        };
        let secret = !multiline && flag(&request.params, "secret", false);
        self.editor.set_text(&prefill);
        self.editor.mask = secret;
        self.ui_prompt = Some(UiPrompt::Input {
            request,
            title: or_default(title, if multiline { "Editor" } else { "Input" }),
            placeholder,
            multiline,
        });
    }

    /// Enter on an `Extension` picker: answer with the chosen row.
    pub(super) fn answer_ui_select(&mut self, item: &MenuItem) {
        match self.ui_prompt.take() {
            Some(UiPrompt::Select(request)) => {
                request.respond(Ok(json!({"value": item.value, "label": item.label})));
            }
            Some(UiPrompt::Confirm(request)) => {
                request.respond(Ok(json!({"confirmed": item.value == "yes"})));
            }
            Some(other) => self.ui_prompt = Some(other),
            None => {}
        }
    }

    /// The composer submitted while an extension's input is open: answer.
    /// Returns false when no input is open, so the line submits normally.
    pub(super) fn answer_ui_input(&mut self, text: &str) -> bool {
        match self.ui_prompt.take() {
            Some(UiPrompt::Input { request, .. }) => {
                self.editor.mask = false;
                self.editor.set_text("");
                request.respond(Ok(json!({"text": text})));
                true
            }
            Some(other) => {
                self.ui_prompt = Some(other);
                false
            }
            None => false,
        }
    }

    /// Esc on an open modal: answer "cancelled" and free the surface.
    pub(super) fn cancel_ui_prompt(&mut self) {
        if let Some(prompt) = self.ui_prompt.take() {
            if matches!(prompt, UiPrompt::Input { .. }) {
                self.editor.mask = false;
                self.editor.set_text("");
            }
            prompt.cancel();
        }
    }

    /// The panel below the composer while an extension asks for text.
    pub(super) fn render_ui_input(&self, width: usize) -> Vec<String> {
        let Some(UiPrompt::Input {
            title, placeholder, ..
        }) = &self.ui_prompt
        else {
            return Vec::new();
        };
        let hint = if !placeholder.is_empty() {
            placeholder.clone()
        } else if self.ui_editor_open() {
            "type or paste, shift+enter for a new line, then Enter".to_string()
        } else {
            "type an answer, then Enter".to_string()
        };
        crate::panel::frame(
            &self.theme,
            width,
            self.theme.fg("userMessageText", title),
            vec![format!("  {}", self.theme.fg("dim", &hint))],
        )
    }

    /// Close the side pane; `tell` notifies its owner (`pane.closed`).
    pub(super) fn close_pane(&mut self, tell: bool) {
        if let Some(pane) = self.pane.take() {
            if tell {
                self.host.notify_extension(
                    &pane.extension,
                    "pane.closed",
                    json!({"pane": pane.id}),
                );
            }
        }
    }

    /// What a pane's key or mouse asked of the app: an attachment lands in
    /// the composer, a selection or activation goes to the owner as data.
    pub(super) fn pane_action(&mut self, action: Action) {
        let Some(pane) = &self.pane else { return };
        let (extension, id) = (pane.extension.clone(), pane.id.clone());
        match action {
            Action::None => {}
            Action::Close => self.close_pane(true),
            Action::Attach { label, content } => {
                self.editor
                    .insert_attachment(&label, &format!("\n{content}\n"));
            }
            Action::Select { section, id: item } => self.host.notify_extension(
                &extension,
                "pane.select",
                json!({"pane": id, "section": section, "id": item}),
            ),
            Action::Activate { section, id: item } => self.host.notify_extension(
                &extension,
                "pane.activate",
                json!({"pane": id, "section": section, "id": item}),
            ),
            Action::Key(chord) => self.host.notify_extension(
                &extension,
                "pane.key",
                json!({"pane": id, "key": chord}),
            ),
        }
    }

    /// The widget rows above the composer, every extension's in name
    /// order, bounded.
    pub(super) fn widget_rows(&self, width: usize) -> Vec<String> {
        self.widgets
            .values()
            .flatten()
            .take(WIDGET_MAX_ROWS)
            .map(|spans| crate::pane::paint_spans(&self.theme, spans, width))
            .collect()
    }

    /// Close the extension panel; `tell` notifies its owner (`ui.panel_closed`).
    pub(super) fn close_ext_panel(&mut self, tell: bool) {
        if let Some(panel) = self.ext_panel.take() {
            if tell {
                self.host
                    .notify_extension(&panel.extension, "ui.panel_closed", json!({}));
            }
        }
    }

    /// A key while an interactive panel is open: forwarded as a chord.
    pub(super) fn forward_panel_key(&self, event: &KeyEvent) {
        let Some(panel) = &self.ext_panel else { return };
        if let Some(chord) = chord_of(event) {
            self.host
                .notify_extension(&panel.extension, "ui.key", json!({"key": chord}));
        }
    }

    /// Apply a command's (or shortcut's) result, guarding session-changing
    /// parts against a session that moved on since the command started.
    pub(super) fn deliver_command_result(&mut self, out: CommandResult, epoch: u64) {
        if let Some(notice) = out.notice {
            self.notice(notice);
        }
        if let Some(show) = out.show {
            self.transcript.push(Block::show(show));
        }
        if let Some(text) = out.prompt {
            if epoch == self.session_epoch {
                self.prompt(text);
            } else {
                self.notice(
                    "an extension command finished after the session changed — its prompt was discarded"
                        .into(),
                );
            }
        }
        if let Some(name) = out.session_name.filter(|n| !n.trim().is_empty()) {
            if epoch == self.session_epoch {
                self.agent.set_session_name(name.clone());
                self.notice(format!("session: {name}"));
                set_tab_title(&tab_title(&title_path(), Some(&name)));
            }
        }
    }

    /// One request from an extension, answered now or parked until the
    /// user can see it. Unknown methods are answered with an error so a
    /// newer extension on an older ulo learns what is missing.
    pub(super) fn on_host_request(&mut self, request: HostRequest) {
        match request.method.as_str() {
            "ui.notify" => self.ui_notify(request),
            "ui.show" => self.ui_show(request),
            "ui.select" | "ui.confirm" | "ui.input" | "ui.editor" => {
                self.ui_queue.push_back(request);
                self.pump_ui_queue();
            }
            // One slot per extension, or several under `key`; the
            // status template's `{status}` joins them all and
            // `{status:<name>}` picks one extension's.
            "ui.status" => {
                set_text_slot(&mut self.ext_status, &request);
                request.ok();
            }
            // The `{activity}` token of the row below the transcript,
            // one slot per extension or several under `key`.
            "ui.activity" => {
                set_text_slot(&mut self.ext_activity, &request);
                request.ok();
            }
            "ui.widget" => self.ui_widget(request),
            "ui.pane" => self.ui_pane(request),
            "ui.compose" => self.ui_compose(request),
            "ui.panel" => self.ui_panel(request),
            "session.send" => self.session_send(request),
            "session.info" => self.session_info(request),
            "session.name" => self.session_name(request),
            "session.model" => self.session_model(request),
            "session.effort" => self.session_effort(request),
            "session.tools" => {
                let names = request
                    .params
                    .get("names")
                    .and_then(Value::as_array)
                    .map(|names| {
                        names
                            .iter()
                            .filter_map(Value::as_str)
                            .map(str::to_string)
                            .collect::<Vec<_>>()
                    });
                self.agent.set_active_tools(names);
                request.ok();
            }
            "session.interrupt" => {
                if self.agent.is_streaming() {
                    self.agent.interrupt();
                }
                request.ok();
            }
            "session.compact" => {
                let focus = text_of(&request.params, "focus");
                self.compact_now((!focus.trim().is_empty()).then_some(focus));
                request.ok();
            }
            other => {
                let error = format!("unknown method {other}");
                request.respond(Err(error));
            }
        }
    }

    /// `ui.notify`: a notice, a warning, or an error block, by tone.
    fn ui_notify(&mut self, request: HostRequest) {
        let params = &request.params;
        let message = text_of(params, "message");
        match params.get("tone").and_then(Value::as_str).unwrap_or("info") {
            "error" => {
                self.transcript.push(Block::new(Kind::Error, message));
            }
            "warning" => self.notice(format!("warning: {message}")),
            _ => self.notice(message),
        }
        request.ok();
    }

    /// `ui.show`: a transcript block, which needs a body or a title.
    fn ui_show(&mut self, request: HostRequest) {
        let show: Show = serde_json::from_value(request.params.clone()).unwrap_or_default();
        if show.body.trim().is_empty() && show.title.trim().is_empty() {
            request.respond(Err("show needs a body or a title".into()));
            return;
        }
        self.transcript.push(Block::show(show));
        request.ok();
    }

    /// `ui.widget`: rows above the composer, keyed so an extension can keep
    /// several; null lines remove one. Bounded across all.
    fn ui_widget(&mut self, request: HostRequest) {
        let slot = slot_of(&request);
        match request.params.get("lines") {
            Some(Value::Array(lines)) if !lines.is_empty() => {
                let rows: Vec<Vec<Span>> =
                    lines.iter().take(WIDGET_MAX_ROWS).map(spans_of).collect();
                self.widgets.insert(slot, rows);
            }
            _ => {
                self.widgets.remove(&slot);
            }
        }
        request.ok();
    }

    /// `ui.pane`: open, update, or (with null) close the side pane.
    fn ui_pane(&mut self, request: HostRequest) {
        if request.params.is_null() {
            let owns = self
                .pane
                .as_ref()
                .is_some_and(|p| p.extension == request.extension);
            if owns {
                self.close_pane(false);
            }
            request.ok();
            return;
        }
        let Some(fresh) = Pane::from_request(&request.extension, &request.params) else {
            request.respond(Err("a pane needs at least one section".into()));
            return;
        };
        match self.pane.as_mut() {
            // The same pane again: new content, the user's place kept.
            Some(open) if open.extension == fresh.extension && open.id == fresh.id => {
                open.update(fresh);
            }
            _ => {
                // Another pane is replaced, and its owner told.
                if self.pane.is_some() {
                    self.close_pane(true);
                }
                self.pane = Some(fresh);
            }
        }
        request.ok();
    }

    /// `ui.compose`: replace the draft, unless the composer is answering a
    /// prompt.
    fn ui_compose(&mut self, request: HostRequest) {
        if self.ui_input_open() || self.pending_key.is_some() {
            request.respond(Err("the composer is answering a prompt".into()));
            return;
        }
        self.editor.set_text(&text_of(&request.params, "text"));
        self.sync_menu();
        request.ok();
    }

    /// `ui.panel`: open or (with null) close the footer panel.
    fn ui_panel(&mut self, request: HostRequest) {
        let params = &request.params;
        if params.is_null() {
            let owns = self
                .ext_panel
                .as_ref()
                .is_some_and(|p| p.extension == request.extension);
            if owns {
                self.close_ext_panel(false);
            }
            request.ok();
            return;
        }
        let title = one_line(&text_of(params, "title"), TITLE_COLUMNS);
        let panel = ExtPanel {
            extension: request.extension.clone(),
            title: or_default(title, &request.extension),
            lines: panel_lines(params.get("lines").unwrap_or(&Value::Null)),
            interactive: flag(params, "interactive", false),
        };
        // Another extension's panel is replaced, and told so.
        let replaced = self
            .ext_panel
            .as_ref()
            .is_some_and(|p| p.extension != panel.extension);
        if replaced {
            self.close_ext_panel(true);
        }
        self.ext_panel = Some(panel);
        request.ok();
    }

    /// `session.send`: a message into the session — as a prompt, a hidden
    /// steer, a record without a turn, or riding with the next turn.
    fn session_send(&mut self, request: HostRequest) {
        let params = &request.params;
        let content = text_of(params, "content");
        if content.trim().is_empty() {
            request.respond(Err("send needs content".into()));
            return;
        }
        let internal = flag(params, "internal", false);
        let run = flag(params, "run", !internal);
        if self.compacting || self.reloading {
            request.respond(Err("the session is busy — try again after the turn".into()));
            return;
        }
        if params.get("when").and_then(Value::as_str) == Some("next_turn") {
            // Ride with whatever the user says next; a visible
            // message cannot wait, so only internal ones may.
            if !internal {
                request.respond(Err("next_turn delivery needs internal: true".into()));
                return;
            }
            self.agent.attach_to_next_turn(content);
            request.ok();
            return;
        }
        if !run && self.agent.is_streaming() {
            // A turn is mid-flight: a commit now could land between
            // a tool call and its result, which every provider
            // rejects. Steering (run: true) queues correctly; so
            // does waiting for the next prompt.
            request.respond(Err(
                "a turn is running — steer with run: true, or use when: \"next_turn\"".into(),
            ));
            return;
        }
        if run && !internal {
            // A visible message that starts (or steers) a turn is a
            // prompt like any other.
            self.prompt(content);
        } else if run {
            // Hidden from the transcript, seen by the model, and it
            // starts the turn; mid-turn it steers like any prompt.
            self.close_queue_review();
            let mut message = ulo_core::providers::ChatMessage::user(content);
            message.mark_internal();
            self.agent.submit_message(message, system_prompt());
        } else if internal {
            self.agent.record_internal(content);
        } else {
            self.agent.record_user(content.clone());
            self.transcript.push(Block::new(Kind::User, content));
        }
        request.ok();
    }

    /// `session.info`: what an extension may know about the session.
    fn session_info(&mut self, request: HostRequest) {
        let info = json!({
            "path": self.agent.session_path().map(|p| p.display().to_string()),
            "id": self.agent.session_id(),
            "name": self.agent.session_name(),
            "cwd": self.agent.cwd().display().to_string(),
            "model": self.agent.model_slug(),
            "effort": self.agent.effort(),
            "running": self.active.is_some() || self.agent.is_streaming(),
            "tools": self.agent.active_tools(),
            "context_tokens": self.context_tokens,
            "context_window": self.agent.model.context_window,
        });
        request.respond(Ok(info));
    }

    /// `session.name`: name the session, or clear the name with an empty one.
    fn session_name(&mut self, request: HostRequest) {
        let name = one_line(&text_of(&request.params, "name"), TITLE_COLUMNS);
        if name.is_empty() {
            self.agent.clear_session_name();
            set_tab_title(&tab_title(&title_path(), None));
        } else {
            self.agent.set_session_name(name.clone());
            self.notice(format!("session: {name}"));
            set_tab_title(&tab_title(&title_path(), Some(&name)));
        }
        request.ok();
    }

    /// `session.model`: the same path /model takes — only a signed-in
    /// provider's model resolves, and the choice is persisted.
    fn session_model(&mut self, request: HostRequest) {
        let query = text_of(&request.params, "model");
        let Some(found) = model::resolve(&query) else {
            request.respond(Err(format!("no available model matches `{query}`")));
            return;
        };
        match persist_model(&found) {
            Ok(()) => {
                self.notice(format!("model set to {}", model::slug(&found)));
                self.agent.model = found;
                self.refresh_status_cache();
                self.emit("model_change", json!({"model": self.agent.model_slug()}));
                request.ok();
            }
            Err(error) => request.respond(Err(format!("could not save model choice: {error}"))),
        }
    }

    /// `session.effort`: one of the current model's declared levels, saved.
    fn session_effort(&mut self, request: HostRequest) {
        let level = text_of(&request.params, "effort");
        match self.agent.set_effort(&level) {
            Ok(true) => {
                self.refresh_status_cache();
                self.emit("effort_change", json!({"effort": level}));
                request.ok();
            }
            Ok(false) => request.respond(Err(format!(
                "`{level}` is not one of this model's effort levels"
            ))),
            Err(error) => request.respond(Err(format!("could not save effort: {error}"))),
        }
    }
}

/// The slot a keyed request writes: the extension's own, or
/// `extension/key` when it names one.
fn slot_of(request: &HostRequest) -> String {
    match request.params.get("key").and_then(Value::as_str) {
        Some(key) if !key.trim().is_empty() => format!("{}/{}", request.extension, flat(key)),
        _ => request.extension.clone(),
    }
}

/// Set a `ui.status` / `ui.activity` slot to the request's text, bounded to
/// one line; blank or missing text clears it.
fn set_text_slot(slots: &mut std::collections::BTreeMap<String, String>, request: &HostRequest) {
    let slot = slot_of(request);
    match request.params.get("text") {
        Some(Value::String(text)) if !text.trim().is_empty() => {
            slots.insert(
                slot,
                one_line(&ulo_core::tools::sanitize_display(text), STATUS_COLUMNS),
            );
        }
        _ => {
            slots.remove(&slot);
        }
    }
}

/// A picker row per usable `ui.select` option: a bare string, or an object
/// with a label (required), a description, and a value.
fn select_items(params: &Value) -> Vec<MenuItem> {
    params
        .get("options")
        .and_then(Value::as_array)
        .map(|options| options.iter().filter_map(select_item).collect())
        .unwrap_or_default()
}

/// One `ui.select` option as a picker row, or None when it has no label.
fn select_item(option: &Value) -> Option<MenuItem> {
    match option {
        Value::String(raw) => {
            // The answer is the offered string; only
            // the row's label is clipped.
            let value = ulo_core::tools::sanitize_display(raw);
            let label = one_line(&value, TITLE_COLUMNS);
            Some(MenuItem::new(&label, "", &value))
        }
        Value::Object(_) => {
            let full_label = text_of(option, "label");
            let label = one_line(&full_label, TITLE_COLUMNS);
            if label.is_empty() {
                return None;
            }
            let value = option
                .get("value")
                .map(|v| match v {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .unwrap_or(full_label);
            Some(MenuItem::new(
                &label,
                &one_line(&text_of(option, "description"), TITLE_COLUMNS),
                &value,
            ))
        }
        _ => None,
    }
}

/// A request's title, or `fallback` when it gave none.
fn or_default(title: String, fallback: &str) -> String {
    if title.is_empty() {
        fallback.to_string()
    } else {
        title
    }
}

/// A boolean parameter, or `default` when absent or not a boolean.
fn flag(params: &Value, key: &str, default: bool) -> bool {
    params.get(key).and_then(Value::as_bool).unwrap_or(default)
}

/// Notify extensions of shutdown followed by startup for a frontend transition.
pub(super) fn shutdown_then_start(app: &App, reason: &'static str) {
    let host = app.host.clone();
    let start = json!({
        "reason": reason,
        "path": app.agent.session_path().map(|p| p.display().to_string()),
    });
    // One task, two awaits: separate spawns could deliver them reordered.
    ulo_core::config::home::spawn(async move {
        host.event("session_shutdown", json!({"reason": reason}))
            .await;
        host.event("session_start", start).await;
    });
}

pub(super) type UiQueue = VecDeque<HostRequest>;
