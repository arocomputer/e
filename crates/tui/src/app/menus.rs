//! The footer menus: commands, models, scoped models, skills, files —
//! building them, syncing them to the composer, and applying a selection.

use super::*;

/// The built-in commands as the `/` picker lists them, before grouping by
/// category: (command, description). Selecting one runs the command.
const BUILTIN_COMMANDS: &[(&str, &str)] = &[
    ("/login", "sign in to a provider — account or API key"),
    ("/models", "switch the model"),
    ("/effort", "show or set reasoning effort"),
    ("/scoped-models", "choose which models ctrl+p cycles"),
    ("/reload", "reload extensions, themes, and config"),
    ("/resume", "resume a saved session"),
    (
        "/tree",
        "rewind to an earlier point in this session and branch",
    ),
    ("/new", "start a fresh session"),
    (
        "/fork",
        "continue in a new session file seeded with this one — /fork <name>",
    ),
    (
        "/export",
        "write this session as a self-contained HTML page — /export <path>",
    ),
    ("/copy", "copy the last reply"),
    (
        "/compact",
        "summarize into a fresh session — /compact <focus> steers what it keeps",
    ),
    (
        "/usage",
        "tokens and estimated cost by model — /usage 24h|7d|30d|all",
    ),
    ("/undo", "put back what the last write or edit replaced"),
    (
        "/trust",
        "trust this directory (loads its AGENTS.md, .ulo resources)",
    ),
    ("/settings", "change preferences"),
    ("/help", "show commands"),
    ("/version", "show the version"),
    ("/quit", "exit"),
];

impl App {
    /// The `/` picker's rows: the built-ins grouped by category, then prompt
    /// templates, then extension commands.
    pub(super) fn command_items(&self) -> Vec<MenuItem> {
        let mut items: Vec<MenuItem> = BUILTIN_COMMANDS
            .iter()
            .map(|(command, description)| MenuItem::new(command, description, command))
            .collect();
        // Every row carries a right-aligned category, brightening with the
        // selected row: a built-in gets its functional group, a prompt
        // template reads `Prompt`, an extension command `Extension`.
        for item in &mut items {
            item.meta = builtin_category(&item.value).into();
        }
        // Group by tag: a stable sort on the category keeps each group
        // contiguous (Account, Model, Session, Workspace, General) without
        // disturbing the order inside a group, and leaves the Prompt and
        // Extension rows — pushed below — trailing the built-ins.
        items.sort_by_key(|item| category_rank(&item.meta));
        // Built-in dispatch wins name clashes, so a template or extension
        // command shadowed by a built-in is unreachable — listing it would
        // show a duplicate row that runs the built-in anyway.
        for template in ulo_core::resources::prompts::list(&self.agent.cwd()) {
            if is_builtin_command(&template.name) {
                continue;
            }
            let slash = format!("/{}", template.name);
            let description = if template.argument_hint.is_empty() {
                template.description.clone()
            } else {
                format!("{} — {}", template.description, template.argument_hint)
            };
            let mut item = MenuItem::new(&slash, &description, &slash);
            item.meta = "Prompt".into();
            items.push(item);
        }
        for (name, description) in self.host.commands() {
            if is_builtin_command(&name) {
                continue;
            }
            let slash = format!("/{name}");
            let description = match self.host.command_hint(&name) {
                Some(hint) if !hint.trim().is_empty() => format!("{description} — {hint}"),
                _ => description,
            };
            let mut item = MenuItem::new(&slash, &description, &slash);
            item.meta = "Extension".into();
            items.push(item);
        }
        items
    }

    /// Whether picking `/name` should leave `/name ` in the composer for
    /// the user to finish, rather than run it bare: a prompt template with
    /// an argument hint, or an extension command that declared arguments.
    fn command_takes_arguments(&self, slashed: &str) -> bool {
        let name = slashed.trim_start_matches('/');
        if let Some(template) = ulo_core::resources::prompts::find(name, &self.agent.cwd()) {
            return !template.argument_hint.trim().is_empty();
        }
        self.host
            .command_hint(name)
            .is_some_and(|hint| !hint.trim().is_empty())
    }

    /// `/name prefix` in the composer, when `name` is an extension command
    /// with completions: the part typed after the name, else None.
    fn completion_prefix(&self, text: &str) -> Option<(String, String)> {
        let rest = text.strip_prefix('/')?;
        if rest.contains('\n') {
            return None;
        }
        let (name, args) = rest.split_once(' ')?;
        if !self.host.has_completions(name) {
            return None;
        }
        // The prefix is the last word being typed; earlier words are done.
        let prefix = args.rsplit(' ').next().unwrap_or("").to_string();
        Some((name.to_string(), prefix))
    }

    /// Ask the extension for completions of what is being typed. The
    /// answer is applied only if the composer still says the same thing.
    fn request_completions(&mut self, command: String, prefix: String) {
        let host = self.host.clone();
        let results = self.results.clone();
        ulo_core::config::home::spawn(async move {
            let items = host.complete_command(&command, &prefix).await;
            let _ = results
                .send(AppJob::Completions {
                    command,
                    prefix,
                    items,
                })
                .await;
        });
    }

    /// Completions arrived: open (or refresh) the arguments picker if the
    /// composer still ends with the prefix they were asked for.
    pub(super) fn show_completions(
        &mut self,
        command: &str,
        prefix: &str,
        items: Vec<ulo_core::extensions::Completion>,
    ) {
        let text = self.editor.text();
        match self.completion_prefix(&text) {
            Some((name, current)) if name == command && current == prefix => {}
            _ => return,
        }
        if items.is_empty() {
            if self
                .menu
                .as_ref()
                .is_some_and(|m| m.kind == MenuKind::Arguments)
            {
                self.menu = None;
            }
            return;
        }
        let items: Vec<MenuItem> = items
            .into_iter()
            .map(|c| {
                let label = c.label.clone().unwrap_or_else(|| c.value.clone());
                MenuItem::new(&label, c.description.as_deref().unwrap_or(""), &c.value)
            })
            .collect();
        self.menu = Some(Menu::new(
            MenuKind::Arguments,
            format!("/{command}"),
            HINT_USE,
            items,
        ));
    }

    /// The scoped-models multi-select: available models plus saved unavailable
    /// entries, Space toggling membership, Ctrl+S saving. The reference semantics:
    /// no scope stored = everything in scope; the first toggle narrows the
    /// scope to just that model. Edits stay staged until Ctrl+S — closing
    /// without it (Enter or Esc) leaves the saved scope untouched.
    pub(super) fn open_scoped_menu(&mut self) {
        let available = model::available();
        // Stage a copy of the saved scope; nothing persists until Ctrl+S.
        // A rebuild while the picker is open (Space promoting a row) keeps
        // the buffer — only a fresh open seeds from what's saved.
        let rebuilding = self.menu.as_ref().map(|m| m.kind) == Some(MenuKind::Scoped);
        if !rebuilding {
            self.staged_scope = model::scope();
        }
        let staged = self.staged_scope.clone().unwrap_or_default();
        if available.is_empty() && staged.is_empty() && !rebuilding {
            self.notice("no models available — use /login to sign in to a provider".into());
            return;
        }
        let available_ids = available
            .iter()
            .map(model::slug)
            .collect::<std::collections::HashSet<_>>();
        let mut available = model::provider_grouped(available);
        // The staged entries lead the list — what you curated, not a hunt —
        // most recently staged first, so what you just picked is at the very
        // top. Unstaged entries keep the provider grouping.
        if !staged.is_empty() {
            let rank = |m: &_| {
                staged
                    .iter()
                    .rposition(|id| *id == model::slug(m))
                    .map(|pos| staged.len() - pos)
                    .unwrap_or(usize::MAX)
            };
            available.sort_by_key(rank);
        }
        let mut items: Vec<MenuItem> = available
            .iter()
            .map(|m| {
                let slug = model::slug(m);
                let mut item = MenuItem::new(&m.id, &m.provider, &slug);
                // Marks reflect the staging buffer, not what's saved — the
                // picker shows the draft until Ctrl+S commits it.
                if staged.contains(&slug) {
                    item.meta = "in scope".into();
                }
                item
            })
            .collect();
        // Keep signed-out, removed, or renamed choices visible and removable.
        // Their IDs stay persisted until the user explicitly toggles them off.
        items.extend(
            staged
                .iter()
                .filter(|id| !available_ids.contains(*id))
                .map(|id| {
                    let mut item = MenuItem::new(id, "unavailable", id);
                    item.meta = "in scope".into();
                    item
                }),
        );
        self.menu = Some(Menu::new(
            MenuKind::Scoped,
            "Scoped models",
            HINT_SCOPED,
            items,
        ));
    }

    /// Space on the scoped picker: toggle membership in the staging buffer —
    /// reference semantics, no scope yet means the first toggle starts a
    /// scope of exactly that model. A model that enters the buffer moves to
    /// the top of the list, so what you curated leads and you never hunt for
    /// what you just picked. Nothing is saved until Ctrl+S.
    pub(super) fn toggle_scoped(&mut self) {
        let Some(slug) = self
            .menu
            .as_ref()
            .and_then(|menu| menu.current().map(|item| item.value.clone()))
        else {
            return;
        };
        let mut ids = self.staged_scope.take().unwrap_or_default();
        let entering = !ids.contains(&slug);
        if entering {
            ids.push(slug.clone());
        } else {
            ids.retain(|id| id != &slug);
        }
        self.staged_scope = Some(ids);
        // Rebuild the picker so the promoted row lands at the top, and keep
        // the selection on the model just toggled.
        self.open_scoped_menu();
        if entering {
            if let Some(menu) = self.menu.as_mut() {
                menu.select_value(&slug);
            }
        }
    }

    /// Ctrl+S on the scoped picker: commit the staging buffer as the saved
    /// scope. Empty commits as no scope at all — back to everything cycling.
    /// A failed write keeps the draft and the picker, so a retry commits the
    /// intended selection, never a cleared scope.
    pub(super) fn save_scope(&mut self) {
        let ids = self.staged_scope.clone().unwrap_or_default();
        if let Err(error) = model::set_scope(&ids) {
            self.notice(format!("could not save model scope: {error}"));
            return;
        }
        self.staged_scope = None;
        self.menu = None;
        self.notice(if ids.is_empty() {
            "scope cleared — ctrl+p cycles every model again".into()
        } else {
            let available = model::available()
                .into_iter()
                .map(|entry| model::slug(&entry))
                .collect::<std::collections::HashSet<_>>();
            let available_count = ids.iter().filter(|id| available.contains(*id)).count();
            format!(
                "scope saved: {available_count} available, {} unavailable",
                ids.len() - available_count
            )
        });
    }

    pub(super) fn open_model_menu(&mut self) {
        // Instant discovery where it matters: show the cached list now, ask
        // the gateways in the background (60s floor), and pop new rows into
        // the open picker when the answer lands.
        let results = self.results.clone();
        ulo_core::config::home::spawn(async move {
            ulo_core::providers::catalog::refresh_remote_within(60_000).await;
            let _ = results.send(AppJob::CatalogRefreshed).await;
        });
        self.build_model_menu();
    }

    /// The picker itself, from the current catalog — no refresh side effects,
    /// so the rebuild-on-refresh arm cannot loop.
    pub(super) fn build_model_menu(&mut self) {
        let available = model::provider_grouped(model::available());
        if available.is_empty() {
            self.notice("no models available — use /login to sign in to a provider".into());
            return;
        }
        let current = self.agent.model_slug();
        // Provider tabs: All, then each provider with models available, in
        // the grouped order the rows themselves use.
        let mut tabs = vec!["All".to_string()];
        for m in &available {
            let display = ulo_core::providers::catalog::display_name(&m.provider);
            if !tabs[1..].contains(&display) {
                tabs.push(display);
            }
        }
        // The reference's model rows carry a dim compact-facts column —
        // `200K context · 8K output` — two columns past the longest id; the
        // current model is where the selection starts, not a marker.
        let items = available
            .iter()
            .map(|m| {
                let mut item = MenuItem::new(&m.id, &model_facts(m), &model::slug(m));
                let display = ulo_core::providers::catalog::display_name(&m.provider);
                item.tab = tabs.iter().position(|t| *t == display);
                item
            })
            .collect();
        let mut menu = Menu::new(MenuKind::Models, "Models", HINT_MODELS, items).with_tabs(
            tabs,
            Some(0),
            0,
            "",
        );
        menu.select_value(&current);
        self.menu = Some(menu);
    }

    pub(super) fn open_skills_menu(&mut self, query: &str) {
        // Single-line rows, the reference's grammar: the skill name with a
        // dim source scope beside it, no description — Tab cycles the
        // source filter.
        let global_root = ulo_core::config::home::skills_dir();
        let items: Vec<MenuItem> = ulo_core::resources::skills::list(&self.agent.cwd())
            .into_iter()
            .map(|s| {
                let (scope, tab) = if s.dir.starts_with(&global_root) {
                    ("Global", 1)
                } else if ulo_core::resources::packages::is_packaged(&s.dir) {
                    ("Package", 3)
                } else {
                    ("Workspace", 2)
                };
                let mut item = MenuItem::new(&s.name, scope, &s.name);
                item.tab = Some(tab);
                item
            })
            .collect();
        if items.is_empty() {
            return;
        }
        let mut menu = Menu::new(MenuKind::Skills, "Skills", HINT_SKILLS, items).with_tabs(
            vec![
                "All".into(),
                "Global".into(),
                "Workspace".into(),
                "Package".into(),
            ],
            Some(0),
            0,
            "Source",
        );
        menu.set_query(query);
        self.menu = Some(menu);
    }

    pub(super) fn open_file_menu(&mut self, query: &str) {
        let cwd = std::env::current_dir().unwrap_or_default();
        let items = ulo_core::workspace::list_files(&cwd)
            .into_iter()
            .map(|path| MenuItem::new(&path, "", &path))
            .collect();
        let mut menu = Menu::new(MenuKind::Files, "Files", HINT_USE, items);
        menu.set_query(query);
        self.menu = Some(menu);
    }

    /// Keep pickers in sync with the composer text: `/` at the start opens
    /// the command picker, an `@word` under the cursor the file picker.
    pub(super) fn sync_menu(&mut self) {
        let text = self.editor.text();
        // A secret or an answer to an extension's question is not a
        // trigger for any picker.
        if self.pending_key.is_some() || self.ui_input_open() {
            return;
        }
        // A picker a command opened (/help, /resume, /tree, an extension's
        // select) has no trigger text in the composer: everything typed
        // from here is its filter.
        if let Some(menu) = self
            .menu
            .as_mut()
            .filter(|menu| menu.filter_without_trigger)
        {
            let query = if menu.kind == MenuKind::Commands {
                text.strip_prefix('/').unwrap_or(&text)
            } else {
                text.as_str()
            };
            menu.set_query(query);
            return;
        }
        // Slash picker: leading '/', no space yet.
        if text.starts_with('/') && !text.contains(' ') && !text.contains('\n') {
            let query = text[1..].to_string();
            match &mut self.menu {
                Some(m) if m.kind == MenuKind::Commands => m.set_query(&query),
                _ => {
                    let mut menu = Menu::new(
                        MenuKind::Commands,
                        "Commands",
                        HINT_USE,
                        self.command_items(),
                    );
                    menu.set_query(&query);
                    self.menu = Some(menu);
                }
            }
            return;
        }
        // Argument picker: `/name …` where the extension completes
        // arguments. Every change re-asks; the answer lands through
        // `AppJob::Completions` and is dropped if the text moved on.
        if let Some((name, prefix)) = self.completion_prefix(&text) {
            let already = self
                .menu
                .as_ref()
                .is_some_and(|m| m.kind == MenuKind::Arguments && m.title == format!("/{name}"));
            if !already {
                self.menu = None;
            }
            self.request_completions(name, prefix);
            return;
        }
        if self
            .menu
            .as_ref()
            .is_some_and(|m| m.kind == MenuKind::Arguments)
        {
            self.menu = None;
        }
        if self.sync_token_picker(&text) {
            return;
        }
        // Auto pickers close when their trigger text is gone.
        if matches!(
            self.menu.as_ref().map(|m| m.kind),
            Some(MenuKind::Commands) | Some(MenuKind::Files) | Some(MenuKind::Skills)
        ) {
            self.menu = None;
        }
    }

    /// The file picker when the last token starts with '@', the skills
    /// picker when it starts with '$'. True when one of them is open.
    fn sync_token_picker(&mut self, text: &str) -> bool {
        let Some(token) = text.split_whitespace().last() else {
            return false;
        };
        if let Some(query) = token.strip_prefix('@') {
            match &mut self.menu {
                Some(m) if m.kind == MenuKind::Files => m.set_query(query),
                _ => self.open_file_menu(query),
            }
            return true;
        }
        if let Some(query) = token.strip_prefix('$') {
            match &mut self.menu {
                Some(m) if m.kind == MenuKind::Skills => m.set_query(query),
                _ => self.open_skills_menu(query),
            }
            return true;
        }
        false
    }

    /// Enter on an open picker. Returns true when the key was consumed.
    pub(super) fn select_menu(&mut self) -> bool {
        let Some(menu) = &self.menu else { return false };
        let Some(item) = menu.current().cloned() else {
            // Enter on a picker the filter emptied closes it; an
            // extension's picker owes its owner an answer.
            if menu.kind == MenuKind::Extension {
                self.cancel_ui_prompt();
            }
            self.menu = None;
            return true;
        };
        let kind = menu.kind;
        // Whatever was typed to filter a command-opened picker was the
        // filter, not a draft.
        if menu.filter_without_trigger && kind != MenuKind::Commands {
            self.editor.set_text("");
        }
        self.menu = None;
        match kind {
            MenuKind::Commands if self.command_takes_arguments(&item.value) => {
                // A command that takes arguments is started, not run: the
                // composer holds `/name ` for the user to complete.
                self.editor.set_text(&format!("{} ", item.value));
                self.sync_menu();
            }
            MenuKind::Arguments => {
                // Replace the typed prefix with the chosen value, keeping
                // earlier words and leaving a space for the next.
                let text = self.editor.text();
                let cut = text.rfind(' ').map(|at| at + 1).unwrap_or(text.len());
                self.editor
                    .set_text(&format!("{}{} ", &text[..cut], item.value));
                self.sync_menu();
            }
            MenuKind::Commands => {
                // The picker consumed the draft line; any attachments were
                // tied to it and go with it.
                self.discard_composer_images();
                self.editor.set_text("");
                self.dispatch_command(item.value);
            }
            MenuKind::Files => {
                // Replace the @token under construction with the chosen path.
                let text = self.editor.text();
                let start = text
                    .rfind('@')
                    .map(|at| text[..at].chars().count())
                    .unwrap_or(0);
                self.editor.replace_suffix(start, &item.value);
            }
            MenuKind::Sessions => {
                self.resume_path(std::path::PathBuf::from(item.value));
            }
            MenuKind::Scoped => {
                // Closing without Ctrl+S discards the staged edits.
                self.staged_scope = None;
            }
            MenuKind::Skills => self.use_skill(&item.value),
            MenuKind::Models => self.select_model(&item.value),
            MenuKind::Tree => {
                self.rewind_to_node(&item.value);
            }
            MenuKind::Extension => self.answer_ui_select(&item),
        }
        true
    }
    /// A skill picked from the `$` picker: replace the $token, then send the
    /// skill body as context ahead of the rest of the draft.
    fn use_skill(&mut self, name: &str) {
        let text = self.editor.text();
        let start = text
            .rfind('$')
            .map(|at| text[..at].chars().count())
            .unwrap_or(0);
        self.editor.replace_suffix(start, "");
        let rest = self.editor.expanded_text().trim_end().to_string();
        self.discard_composer_images();

        self.editor.set_text("");
        let Some(skill) = ulo_core::resources::skills::get(name, &self.agent.cwd()) else {
            return;
        };
        // The directory rides along, exactly as the system-prompt
        // catalog carries it: a body that says "see reference.md"
        // strands the model without the path it lives at.
        let body = format!(
            "{}\n\n[skill directory: {} — files this skill references live there]",
            skill.body,
            skill.dir.display()
        );
        let combined = if rest.is_empty() {
            body
        } else {
            format!("{body}\n\n{rest}")
        };
        self.prompt(combined);
    }

    /// A model picked from /models: switch to it and persist the choice.
    fn select_model(&mut self, slug: &str) {
        let Some(found) = model::resolve(slug) else {
            return;
        };
        if let Err(error) = persist_model(&found) {
            self.notice(format!("could not save model choice: {error}"));
            return;
        }
        self.notice(format!("model set to {}", model::slug(&found)));
        self.agent.model = found;
        self.refresh_status_cache();
        self.emit(
            "model_change",
            serde_json::json!({"model": self.agent.model_slug()}),
        );
    }
}

/// `200K context · 8K output` — exact multiples compact to K/M,
/// anything else stays raw, the reference's fact grammar.
fn model_facts(m: &ulo_core::providers::catalog::Model) -> String {
    let mut facts = Vec::new();
    if m.context_window > 0 {
        facts.push(token_fact(m.context_window, "context"));
    }
    if let Some(output) = m.max_output {
        facts.push(token_fact(output, "output"));
    }
    facts.join(" · ")
}

/// One model fact: a token count, compacted to K/M when exact.
fn token_fact(tokens: u64, suffix: &str) -> String {
    if tokens >= 1_000_000 && tokens.is_multiple_of(1_000_000) {
        format!("{}M {suffix}", tokens / 1_000_000)
    } else if tokens >= 1_000 && tokens.is_multiple_of(1_000) {
        format!("{}K {suffix}", tokens / 1_000)
    } else {
        format!("{tokens} {suffix}")
    }
}
