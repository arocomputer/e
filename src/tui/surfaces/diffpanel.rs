//! Live read-only diff panel. File navigation and patch selection are separate
//! from the composer; refreshes preserve the file and position being reviewed.

use crate::core::diff::{File, Snapshot};
use crate::core::tools::sanitize_display;
use crate::tui::{
    markdown::{clip_styled, visible_width},
    panel,
    render::bold,
    theme::Theme,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Files,
    Patch,
}

/// Results that need the app's composer or lifecycle rather than panel state.
pub enum Action {
    None,
    Close,
    Attach { label: String, content: String },
}

pub struct DiffPanel {
    pub files: Vec<File>,
    pub selected: Option<PathBuf>,
    pub focused: bool,
    pub focus: Focus,
    pub error: Option<String>,
    pub loading: bool,
    pub dirty: bool,
    pub next_refresh: std::time::Instant,
    pub task: Option<tokio::task::JoinHandle<()>>,
    pub generation: u64,
    pub min_width: usize,
    pub percent: usize,
    pub refresh_ms: u64,
    patch: Vec<String>,
    horizontal: usize,
    close_column: usize,
    cursor: usize,
    anchor: Option<usize>,
    scroll: usize,
    file_scroll: usize,
    file_rows: usize,
    patch_start: usize,
    patch_rows: usize,
    title: String,
    hint: String,
}

impl Drop for DiffPanel {
    fn drop(&mut self) {
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

impl DiffPanel {
    pub fn new(generation: u64) -> Self {
        use crate::core::config::settings::{get_string, get_u64};
        Self {
            files: Vec::new(),
            selected: None,
            focused: true,
            focus: Focus::Files,
            error: None,
            loading: false,
            dirty: true,
            next_refresh: std::time::Instant::now(),
            task: None,
            generation,
            min_width: get_u64("diff_min_width").unwrap_or(110).max(60) as usize,
            percent: get_u64("diff_width_percent").unwrap_or(50).clamp(30, 70) as usize,
            refresh_ms: get_u64("diff_refresh_ms").unwrap_or(1000).max(250),
            patch: Vec::new(),
            horizontal: 0,
            close_column: 0,
            cursor: 0,
            anchor: None,
            scroll: 0,
            file_scroll: 0,
            file_rows: 0,
            patch_start: 0,
            patch_rows: 1,
            title: get_string("diff_title").unwrap_or_else(|| "Diff · workspace vs HEAD".into()),
            hint: get_string("diff_hint")
                .unwrap_or_else(|| "↑↓ move · Enter · Ctrl+D focus · Esc back".into()),
        }
    }

    /// A wide terminal splits; a narrow one shows only the focused pane.
    pub fn left_width(&self, width: usize) -> Option<usize> {
        (width >= self.min_width).then(|| width.saturating_sub(3 + width * self.percent / 100))
    }

    fn selected_index(&self) -> usize {
        self.files
            .iter()
            .position(|f| Some(&f.path) == self.selected.as_ref())
            .unwrap_or(0)
    }

    /// Apply only the requested file's response. A refresh must not steal selection.
    pub fn apply(&mut self, requested: Option<PathBuf>, result: Result<Snapshot, String>) {
        self.loading = false;
        self.task = None;
        if requested != self.selected {
            self.dirty = true;
            return;
        }
        match result {
            Ok(snapshot) => {
                let changed_file = self.selected != snapshot.selected;
                let mut in_hunk = false;
                let patch: Vec<String> = snapshot
                    .patch
                    .lines()
                    .filter(|line| {
                        if line.starts_with("@@") {
                            in_hunk = true;
                        }
                        in_hunk
                            || !(line.starts_with("diff --git ")
                                || line.starts_with("index ")
                                || line.starts_with("--- ")
                                || line.starts_with("+++ "))
                    })
                    .map(str::to_string)
                    .collect();
                if changed_file {
                    self.cursor = 0;
                    self.scroll = 0;
                }
                if patch != self.patch {
                    self.anchor = None;
                }
                self.files = snapshot.files;
                self.selected = snapshot.selected;
                self.patch = patch;
                self.cursor = self.cursor.min(self.patch.len().saturating_sub(1));
                self.scroll = self.scroll.min(self.patch.len().saturating_sub(1));
                self.error = None;
            }
            Err(error) => {
                self.error = Some(error);
            }
        }
    }

    fn select_file(&mut self, index: usize) {
        if let Some(file) = self.files.get(index) {
            if self.selected.as_ref() != Some(&file.path) {
                self.selected = Some(file.path.clone());
                self.patch.clear();
                self.horizontal = 0;
                self.cursor = 0;
                self.scroll = 0;
                self.anchor = None;
                self.dirty = true;
            }
        }
    }

    /// Copy a selection now, so later edits cannot change the next prompt's attachment.
    fn attach(&mut self) -> Action {
        let Some(path) = self.selected.as_ref() else {
            return Action::None;
        };
        if self.patch.is_empty() || self.error.is_some() {
            return Action::None;
        }
        let anchor = self.anchor.unwrap_or(self.cursor);
        let lo = anchor.min(self.cursor);
        let hi = anchor.max(self.cursor).min(self.patch.len() - 1);
        let hunk = self.patch[..lo].iter().rfind(|line| line.starts_with("@@"));
        let mut content = format!("Diff for {} (workspace vs HEAD):\n", path.display());
        if let Some(hunk) = hunk {
            content.push_str(hunk);
            content.push('\n');
        }
        content.push_str(&self.patch[lo..=hi].join("\n"));
        let path = clip_styled(&display_path(path), 36);
        let label = format!("[Diff {path}, {} lines]", hi - lo + 1);
        self.anchor = None;
        self.focused = false;
        Action::Attach { label, content }
    }

    /// Keys are consumed only while the panel owns focus. Ctrl+C stays global.
    pub fn key(&mut self, key: KeyEvent) -> Action {
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        if key.code == KeyCode::Esc {
            if self.focus == Focus::Patch {
                self.focus = Focus::Files;
                self.anchor = None;
            } else {
                return Action::Close;
            }
            return Action::None;
        }
        if key.code == KeyCode::Enter {
            if self.focus == Focus::Files {
                self.focus = Focus::Patch;
            } else {
                return self.attach();
            }
            return Action::None;
        }
        if self.focus == Focus::Files {
            let index = self.selected_index();
            match key.code {
                KeyCode::Up | KeyCode::Char('k') => self.select_file(index.saturating_sub(1)),
                KeyCode::Down | KeyCode::Char('j') => {
                    self.select_file((index + 1).min(self.files.len().saturating_sub(1)))
                }
                KeyCode::PageDown => self.select_file(
                    (index + self.file_rows.max(1)).min(self.files.len().saturating_sub(1)),
                ),
                KeyCode::PageUp => self.select_file(index.saturating_sub(self.file_rows.max(1))),
                KeyCode::Home => self.select_file(0),
                KeyCode::End => self.select_file(self.files.len().saturating_sub(1)),
                _ => {}
            }
        } else {
            let before = self.cursor;
            let step = self.patch_rows.max(1);
            match key.code {
                KeyCode::Up | KeyCode::Char('k') => self.cursor = self.cursor.saturating_sub(1),
                KeyCode::Down | KeyCode::Char('j') => self.cursor += 1,
                KeyCode::PageUp => self.cursor = self.cursor.saturating_sub(step),
                KeyCode::PageDown => self.cursor += step,
                KeyCode::Left => {
                    self.horizontal = self.horizontal.saturating_sub(8);
                    return Action::None;
                }
                KeyCode::Right => {
                    self.horizontal = (self.horizontal + 8).min(4096);
                    return Action::None;
                }
                KeyCode::Home => self.cursor = 0,
                KeyCode::End => self.cursor = self.patch.len().saturating_sub(1),
                _ => return Action::None,
            }
            self.cursor = self.cursor.min(self.patch.len().saturating_sub(1));
            if shift {
                self.anchor.get_or_insert(before);
            } else {
                self.anchor = None;
            }
        }
        Action::None
    }

    /// Mouse coordinates are local to the full-height panel.
    pub fn mouse(&mut self, event: MouseEvent) -> Action {
        let row = event.row as usize;
        match event.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                self.focused = true;
                if row == 1 && event.column as usize >= self.close_column {
                    return Action::Close;
                }
                if (3..3 + self.file_rows).contains(&row) {
                    self.focus = Focus::Files;
                    self.select_file(self.file_scroll + row - 3);
                } else if row >= self.patch_start
                    && row < self.patch_start + self.patch_rows
                    && !self.patch.is_empty()
                {
                    self.focus = Focus::Patch;
                    self.cursor = (self.scroll + row - self.patch_start).min(self.patch.len() - 1);
                    self.anchor = Some(self.cursor);
                }
            }
            MouseEventKind::Drag(MouseButton::Left) if self.anchor.is_some() => {
                self.cursor = (self.scroll + row.saturating_sub(self.patch_start))
                    .min(self.patch.len().saturating_sub(1));
            }
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                if row < self.patch_start {
                    let index = self.selected_index();
                    self.select_file(if event.kind == MouseEventKind::ScrollUp {
                        index.saturating_sub(1)
                    } else {
                        (index + 1).min(self.files.len().saturating_sub(1))
                    });
                } else {
                    self.cursor = if event.kind == MouseEventKind::ScrollUp {
                        self.cursor.saturating_sub(3)
                    } else {
                        (self.cursor + 3).min(self.patch.len().saturating_sub(1))
                    };
                    self.anchor = None;
                }
            }
            _ => {}
        }
        Action::None
    }

    /// Fixed-height layout using the shared panel frame and theme tokens.
    pub fn render(&mut self, theme: &Theme, width: usize, height: usize) -> Vec<String> {
        let available = height.saturating_sub(5);
        self.file_rows = self.files.len().min(6).min(available / 3);
        let selected = self.selected_index();
        if selected < self.file_scroll {
            self.file_scroll = selected;
        }
        if selected >= self.file_scroll + self.file_rows.max(1) {
            self.file_scroll = selected + 1 - self.file_rows.max(1);
        }
        let mut body = Vec::new();
        for (index, file) in self
            .files
            .iter()
            .enumerate()
            .skip(self.file_scroll)
            .take(self.file_rows)
        {
            let count = |v: Option<usize>| v.map(|n| n.to_string()).unwrap_or_else(|| "?".into());
            let stats = format!("+{} -{}", count(file.added), count(file.removed));
            let name = clip_styled(
                &display_path(&file.path),
                width.saturating_sub(stats.len() + 2),
            );
            let pad = width.saturating_sub(visible_width(&name) + stats.len());
            let name = if index == selected {
                bold(&theme.fg("userMessageText", &name))
            } else {
                theme.fg("dim", &name)
            };
            let added = theme.fg(
                Theme::diff_marker_token(true),
                &format!("+{}", count(file.added)),
            );
            let removed = theme.fg(
                Theme::diff_marker_token(false),
                &format!("-{}", count(file.removed)),
            );
            body.push(format!("{name}{}{added} {removed}", " ".repeat(pad)));
        }
        body.push(theme.fg("border", &"─".repeat(width)));
        let path = self
            .selected
            .as_ref()
            .map(|p| display_path(p))
            .unwrap_or_else(|| "No changes".into());
        body.push(theme.fg("dim", &clip_styled(&path, width)));
        body.push(String::new());
        self.patch_start = 3 + body.len();
        self.patch_rows = available.saturating_sub(body.len());
        if self.cursor < self.scroll {
            self.scroll = self.cursor;
        }
        if self.cursor >= self.scroll + self.patch_rows.max(1) {
            self.scroll = self.cursor + 1 - self.patch_rows.max(1);
        }
        let range = self
            .anchor
            .map(|a| (a.min(self.cursor), a.max(self.cursor)));
        if let Some(error) = &self.error {
            body.push(theme.fg("error", &clip_styled(&sanitize_display(error), width)));
        } else if self.patch.is_empty() {
            body.push(theme.fg(
                "dim",
                if self.loading || self.dirty {
                    "Loading…"
                } else {
                    "Working tree clean"
                },
            ));
        } else {
            for (i, line) in self
                .patch
                .iter()
                .enumerate()
                .skip(self.scroll)
                .take(self.patch_rows)
            {
                let text = clip_styled(
                    &sanitize_display(line)
                        .chars()
                        .skip(self.horizontal)
                        .collect::<String>(),
                    width,
                );
                let token = if line.starts_with('+') && !line.starts_with("+++") {
                    Theme::diff_marker_token(true)
                } else if line.starts_with('-') && !line.starts_with("---") {
                    Theme::diff_marker_token(false)
                } else if line.starts_with("@@") {
                    "accent"
                } else {
                    "dim"
                };
                let mut text = theme.fg(token, &text);
                if range.is_some_and(|(lo, hi)| i >= lo && i <= hi) {
                    text = format!("\x1b[7m{text}\x1b[27m");
                } else if self.focused && self.focus == Focus::Patch && i == self.cursor {
                    text = bold(&text);
                }
                body.push(text);
            }
        }
        body.truncate(available);
        body.resize(available, String::new());
        let title = clip_styled(
            &format!("{} · {} files", self.title, self.files.len()),
            width.saturating_sub(2),
        );
        let header = format!(
            "{title}{}×",
            " ".repeat(width.saturating_sub(visible_width(&title) + 1))
        );
        self.close_column = width.saturating_sub(1);
        let header = theme.fg(
            if self.focused {
                "userMessageText"
            } else {
                "dim"
            },
            &header,
        );
        let mut rows = panel::frame(theme, width, header, body);
        rows.push(theme.fg("dim", &clip_styled(&self.hint, width)));
        rows.truncate(height);
        rows.into_iter()
            .map(|row| clip_styled(&row, width))
            .collect()
    }
}

/// Keep unusual Git path bytes from creating extra terminal rows.
fn display_path(path: &std::path::Path) -> String {
    sanitize_display(&path.to_string_lossy())
        .replace('\n', "\\n")
        .replace('\t', "\\t")
}
