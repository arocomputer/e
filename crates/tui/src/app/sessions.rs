//! Resume, navigate, fork, and export the current conversation through the core.

use super::*;

impl App {
    pub(super) fn open_resume_menu(&mut self) {
        // Both checks: `active` covers a turn whose TurnStart has been seen,
        // `is_streaming` covers the gap between submit and that event.
        if self.active.is_some() || self.agent.is_streaming() {
            self.notice("a turn is running — press Esc to stop it, then /resume".into());
            return;
        }
        let cwd = e_core::session::normalized_cwd(&self.agent.cwd());
        // Every workspace's sessions, with a Tab-cycled scope filter that
        // opens on the current workspace. Each row carries the reference's
        // dim right cluster: `workspace · age · N turns`. A workspace's own
        // directory name stands in for it when unique across the list; two
        // `proj` directories fall back to the full `~`-collapsed path so
        // the rows stay distinguishable.
        let listed = e_core::session::list_all();
        let mut tail_counts = std::collections::HashMap::<String, usize>::new();
        for info in &listed {
            if let Some(tail) = info.cwd.file_name() {
                *tail_counts
                    .entry(tail.to_string_lossy().into_owned())
                    .or_default() += 1;
            }
        }
        let items: Vec<MenuItem> = listed
            .into_iter()
            .map(|info| {
                let mut item = MenuItem::new(
                    if info.title.is_empty() {
                        "(untitled)"
                    } else {
                        &info.title
                    },
                    "",
                    &info.path.to_string_lossy(),
                );
                let tail = info
                    .cwd
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let workspace = if tail_counts.get(&tail).copied().unwrap_or(0) > 1 {
                    collapse_home(&info.cwd)
                } else {
                    tail
                };
                let turns = format!(
                    "{} {}",
                    info.user_turns,
                    if info.user_turns == 1 {
                        "turn"
                    } else {
                        "turns"
                    }
                );
                item.meta = format!("{workspace} · {} · {turns}", ago(info.modified));
                // Tab index space: 0 = current workspace, 1 = the
                // "All workspaces" tab itself. Tagging other workspaces 1
                // works because `tab_admits` checks all_tab before item
                // tags — reordering these tabs breaks that silently.
                item.tab = Some(if e_core::session::normalized_cwd(&info.cwd) == cwd {
                    0
                } else {
                    1
                });
                item
            })
            .collect();
        if items.is_empty() {
            self.notice("no saved sessions".into());
            return;
        }
        self.menu = Some(
            Menu::new(MenuKind::Sessions, "Sessions", HINT_SESSIONS, items)
                .without_trigger()
                .with_tabs(
                    vec!["Current workspace".into(), "All workspaces".into()],
                    Some(1),
                    0,
                    "",
                ),
        );
    }

    pub(super) fn resume_recent(&mut self) {
        let cwd = self.agent.cwd();
        match e_core::session::most_recent(&cwd) {
            Some(path) => self.resume_path(path),
            None => self.notice("no saved sessions for this workspace".into()),
        }
    }

    /// Rebuild the transcript from a linear message history: clear what's
    /// showing and replay `messages` as blocks, reconstructing tool-call
    /// groups from their recorded outcomes. Shared by /resume (the whole
    /// file) and /tree (the path from root to a rewind point) — both end up
    /// wanting exactly the same replay, just fed a different list. A resumed
    /// transcript carries no welcome banner — the reference reserves it for
    /// a fresh session.
    pub(super) fn rebuild_transcript(&mut self, messages: &[e_core::providers::ChatMessage]) {
        self.transcript.clear();
        self.outputs.clear();
        self.viewer = None;
        // The projection is keyed by the transcript's shape, which another
        // session can share; the cache must not outlive the transcript.
        self.viewer_cache = None;
        let mut restored_calls = std::collections::HashMap::<String, (usize, u64)>::new();
        let mut restored_id = 0u64;
        // Consecutive tool batches with no assistant voice between them were
        // one growing tree live — the replay keeps them one tree.
        let mut open_group: Option<usize> = None;
        for m in messages {
            match m.role() {
                "user" => {
                    open_group = None;
                    let count = m.images().len();
                    let content = if count == 0 {
                        m.content.clone()
                    } else {
                        display_image_prompt(&m.content, count)
                    };
                    self.transcript
                        .push(Block::new(Kind::User, content).with_images(count));
                }
                "assistant" => {
                    if !m.content.trim().is_empty() {
                        open_group = None;
                        self.transcript
                            .push(Block::new(Kind::Assistant, m.content.clone()));
                    }
                    if !m.tool_calls().is_empty() {
                        let mut children = Vec::with_capacity(m.tool_calls().len());
                        let mut ids = Vec::with_capacity(m.tool_calls().len());
                        for call in m.tool_calls() {
                            restored_id += 1;
                            let args = serde_json::from_str(&call.arguments)
                                .unwrap_or(serde_json::Value::Null);
                            let shown = e_core::tools::present(&call.name, &args);
                            children.push(crate::transcript::ToolChild::pending(
                                restored_id,
                                shown.category,
                                shown.running,
                                shown.completed,
                                shown.target,
                            ));
                            ids.push((call.id.clone(), restored_id));
                        }
                        let block = match open_group {
                            Some(idx) => {
                                if let Some(group) = self.transcript.blocks.get_mut(idx) {
                                    group.tool_children.extend(children);
                                    group.touch();
                                }
                                idx
                            }
                            None => self.transcript.push(Block::tool_group(children)),
                        };
                        open_group = Some(block);
                        for (call_id, id) in ids {
                            restored_calls.insert(call_id, (block, id));
                        }
                    }
                }
                "tool" => {
                    let Some(call_id) = m.tool_call_id() else {
                        continue;
                    };
                    let Some(&(block, id)) = restored_calls.get(call_id) else {
                        continue;
                    };
                    let (outcome, summary) = m
                        .tool_meta()
                        .as_ref()
                        .map(|meta| (meta.outcome, meta.summary.clone()))
                        .unwrap_or((e_core::tools::ToolOutcome::Completed, "done".into()));
                    let mut title = None;
                    if let Some(group) = self.transcript.blocks.get_mut(block) {
                        group.start_tool(id);
                        if let Some(child) = group.tool_children.iter().find(|child| child.id == id)
                        {
                            title = Some(if child.target.is_empty() {
                                child.completed.clone()
                            } else {
                                format!("{} {}", child.completed, child.target)
                            });
                        }
                        group.finish_tool(id, outcome, summary, &m.content);
                    }
                    // Recorded results come back to the ctrl+o review
                    // screen, the same store the live session fills.
                    if !m.content.trim().is_empty() {
                        let detail = self.remember_output(
                            title.unwrap_or_else(|| "tool output".into()),
                            e_core::tools::sanitize_display(&m.content),
                        );
                        if let Some(group) = self.transcript.blocks.get_mut(block) {
                            if let Some(child) =
                                group.tool_children.iter_mut().find(|child| child.id == id)
                            {
                                child.detail = Some(detail);
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        // Seal every restored group: no more results are coming, so a child
        // still pending renders (and tallies) as unreported instead of
        // silently vanishing, and a later live batch starts its own tree
        // instead of splicing into a restored one.
        for block in &mut self.transcript.blocks {
            if block.kind == Kind::ToolGroup {
                block.tool_label_rows = self.tool_label_rows;
                block.seal();
            }
        }
        // Seed the context gauge from the restored history so the statusline
        // and the auto-compact check don't see an empty context until the
        // first real usage report lands.
        self.context_tokens =
            e_core::agent::compact::estimate_request_tokens(&system_prompt(), messages);
    }

    pub(super) fn resume_path(&mut self, path: std::path::PathBuf) {
        // The picker can already be open when a turn starts (queued prompt);
        // re-check here so a selection can never splice a running turn's
        // output into the resumed session. `is_streaming` also covers the
        // gap between a submit and its TurnStart event.
        if self.active.is_some() || self.agent.is_streaming() {
            self.notice("a turn is running — press Esc to stop it, then resume".into());
            return;
        }
        // Ownership first: a session another e is appending to must not be
        // replayed into a second, diverging history.
        let session = match e_core::session::SessionLog::reopen(&path) {
            Ok(s) => s,
            Err(e) => {
                self.notice(format!("could not resume session: {e}"));
                self.release_initial_prompt();
                return;
            }
        };
        let messages = match e_core::session::SessionLog::load(&path) {
            Ok(m) => m,
            Err(e) => {
                self.notice(format!("could not open session: {e}"));
                self.release_initial_prompt();
                return;
            }
        };
        // The old transcript's shell block index and held prompts die with
        // it; a still-running `!` command's result is epoch-discarded, and a
        // /compact still summarizing the old session must not land its swap
        // on the resumed one.
        self.shell_block = None;
        self.held_prompts.clear();
        self.compacting = false;
        self.discard_composer_images();
        self.rebuild_transcript(&messages);
        self.agent.load_history(messages);
        self.agent.set_session(Some(session));
        // Identity travels together: the resumed log's persisted name
        // replaces whatever the previous session was called.
        self.agent
            .adopt_session_name(e_core::session::name_of(&path));
        self.session_epoch += 1;
        extui::shutdown_then_start(self, "resume");
        self.notice(format!(
            "resumed {}",
            path.file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        ));
        // A -r launch prompt waits for this selection; deliver it against
        // the loaded history.
        if let Some(initial) = self.pending_initial.take() {
            self.submit_initial(initial);
        }
    }

    /// /tree: list every earlier user turn in the active session as a
    /// rewind point. Picking one means "go back to just before this and try
    /// something different" — the new branch replaces it, not extends it.
    pub(super) fn open_tree_menu(&mut self) {
        if self.active.is_some() || self.agent.is_streaming() {
            self.notice("a turn is running — press Esc to stop it, then /tree".into());
            return;
        }
        let Some(path) = self.agent.session_path() else {
            self.notice("nothing to rewind yet — send a message first".into());
            return;
        };
        let nodes = match e_core::session::SessionLog::nodes(&path) {
            Ok(n) => n,
            Err(e) => {
                self.notice(format!("could not read session: {e}"));
                return;
            }
        };
        let items: Vec<MenuItem> = tree_items(&nodes)
            .into_iter()
            .map(|(id, preview, branched)| {
                let mut item = MenuItem::new(
                    if preview.is_empty() {
                        "(empty)"
                    } else {
                        &preview
                    },
                    "",
                    &id,
                );
                if branched {
                    item.meta = "⑂ branch point".into();
                }
                item
            })
            .collect();
        if items.is_empty() {
            self.notice("nothing to rewind to yet".into());
            return;
        }
        self.menu = Some(Menu::new(MenuKind::Tree, "Rewind to", HINT_USE, items).without_trigger());
    }

    /// Apply a /tree choice: rewind to just before the chosen user message,
    /// restore its text in the composer, and replay everything before it
    /// into the transcript and agent history. The file stays untouched. The
    /// edited or resent message grows a sibling branch beside the old tail.
    pub(super) fn rewind_to_node(&mut self, node_id: &str) {
        if self.active.is_some() || self.agent.is_streaming() {
            self.notice("a turn is running — press Esc to stop it, then /tree".into());
            return;
        }
        let Some(path) = self.agent.session_path() else {
            return;
        };
        let nodes = match e_core::session::SessionLog::nodes(&path) {
            Ok(n) => n,
            Err(e) => {
                self.notice(format!("could not read session: {e}"));
                return;
            }
        };
        let Some((head, messages, prompt)) = rewind_target(&nodes, node_id) else {
            self.notice("that point no longer exists".into());
            return;
        };
        self.shell_block = None;
        self.held_prompts.clear();
        self.compacting = false;
        self.discard_composer_images();
        self.rebuild_transcript(&messages);
        self.agent.rewind_to(head, messages);
        self.editor.set_text(&prompt);
        self.session_epoch += 1;
        self.notice("edit or resend the restored prompt to branch".into());
    }

    /// Ctrl+C is global, including during trust and login. The first press
    /// cancels work and clears transient input; a second press exits without
    /// recording a trust decision or waiting for cancellation to finish.
    /// `/usage [24h|7d|30d|all]`: tokens and estimated cost by model over a
    /// period, from the sessions on disk.
    pub(super) fn show_usage(&mut self, period: &str) {
        let Some(report) = e_core::usage::report_for(period) else {
            self.notice("usage periods: 24h, 7d (default), 30d, all".into());
            return;
        };
        let label = match period.trim() {
            "24h" | "day" | "today" => "last 24 hours",
            "30d" | "month" => "last 30 days",
            "all" => "whole history",
            _ => "last 7 days",
        };
        self.transcript.push(Block::show(e_core::extensions::Show {
            title: format!("Usage · {label}"),
            body: e_core::usage::markdown(&report, label),
            format: e_core::extensions::Format::Markdown,
        }));
    }

    /// `/undo`: put back what the last write or edit replaced.
    pub(super) fn undo_change(&mut self) {
        if self.active.is_some() || self.agent.is_streaming() {
            self.notice("a turn is running — press Esc to stop it, then /undo".into());
            return;
        }
        match self.agent.undo_last_change() {
            Ok(Some(label)) => {
                let left = self.agent.undo_depth();
                self.notice(format!(
                    "undid {label}{}",
                    if left > 0 {
                        format!(" — {left} more to undo")
                    } else {
                        String::new()
                    }
                ));
            }
            Ok(None) => self.notice("nothing to undo".into()),
            Err(error) => self.notice(error),
        }
    }

    /// `/fork [name]`: continue in a new session file seeded with the
    /// current branch. The transcript and history stay as they are; only
    /// where the next messages land changes. The original file is untouched.
    pub(super) fn fork_session(&mut self, name: Option<String>) {
        if self.shell_block.is_some() {
            self.notice("a shell command is running — fork after it finishes".into());
            return;
        }
        if self.active.is_some() || self.agent.is_streaming() {
            self.notice("a turn is running — press Esc to stop it, then /fork".into());
            return;
        }
        let messages = self.agent.history_snapshot();
        if messages.is_empty() {
            self.notice("nothing to fork yet — send a message first".into());
            return;
        }
        let model = self.agent.model_slug();
        let mut log =
            match e_core::session::SessionLog::create_with(&self.agent.cwd(), &model, &messages) {
                Ok(log) => log,
                Err(error) => {
                    self.notice(format!("could not fork: {error}"));
                    return;
                }
            };
        let name = name
            .filter(|n| !n.is_empty())
            .or_else(|| self.agent.session_name().map(|n| format!("{n} (fork)")));
        if let Some(name) = &name {
            if let Err(error) = log.set_name(name) {
                self.notice(format!("could not name the fork: {error}"));
            }
        }
        let file = log
            .path()
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.agent.set_session(Some(log));
        self.agent.adopt_session_name(name.clone());
        self.session_epoch += 1;
        set_tab_title(&tab_title(&title_path(), name.as_deref()));
        extui::shutdown_then_start(self, "fork");
        self.notice(format!(
            "forked into {file} — the original session is unchanged"
        ));
    }

    /// `/export [path]`: write the current branch as a self-contained HTML
    /// page. Default: `e-session-<id>.html` in the working directory.
    pub(super) fn export_session(&mut self, path: Option<String>) {
        let messages = self.agent.history_snapshot();
        if messages.is_empty() {
            self.notice("nothing to export yet".into());
            return;
        }
        let title = self
            .agent
            .session_name()
            .or_else(|| {
                self.agent
                    .session_path()
                    .and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
            })
            .unwrap_or_else(|| "e session".into());
        let target = match path.filter(|p| !p.is_empty()) {
            Some(path) => {
                let path = std::path::PathBuf::from(path);
                if path.is_absolute() {
                    path
                } else {
                    self.agent.cwd().join(path)
                }
            }
            None => {
                let id = self
                    .agent
                    .session_id()
                    .map(|id| id.chars().take(8).collect::<String>())
                    .unwrap_or_else(|| "unsaved".into());
                self.agent.cwd().join(format!("e-session-{id}.html"))
            }
        };
        let page = e_core::export::html(&title, &self.agent.model_slug(), &messages);
        match std::fs::write(&target, page) {
            Ok(()) => self.notice(format!("exported to {}", target.display())),
            Err(error) => self.notice(format!("could not export: {error}")),
        }
    }

    pub(super) fn compact_now(&mut self, focus: Option<String>) {
        if self.shell_block.is_some() {
            self.notice("a shell command is running — compact after it finishes".into());
            return;
        }
        self.agent.request_compaction_with(system_prompt(), focus);
    }
}
