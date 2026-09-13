//! Continuous, mouse-driven Git review. Selection becomes owned prompt context;
//! keyboard input always stays with the composer.
use crate::diff::{File, Review};
use crate::style::sanitize_display;
use crate::style::{
    bold, panel,
    text::{clip_styled, visible_width},
    theme::Theme,
};
use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use std::path::{Path, PathBuf};
use unicode_width::UnicodeWidthChar;
mod patch;

/// Actions that belong to the app rather than the read-only review document.
pub enum Action {
    None,
    Close,
    Attach { label: String, content: String },
}

/// Immutable source and width-independent syntax for one file.
struct Section {
    path: PathBuf,
    source: String,
    rows: Vec<patch::Row>,
    styled: Vec<String>,
    digits: usize,
}

/// Visual rows retain source coordinates so wrapped lines copy only once.
#[derive(Clone, Copy)]
enum Line {
    File(usize),
    Divider,
    Heading(usize),
    Source {
        section: usize,
        row: usize,
        column: usize,
    },
    Omitted,
}

pub struct DiffPanel {
    pub files: Vec<File>,
    pub error: Option<String>,
    pub loading: bool,
    pub min_width: usize,
    pub percent: usize,
    pub refresh_ms: u64,
    pub height: usize,
    sections: Vec<Section>,
    lines: Vec<Line>,
    layout_width: Option<usize>,
    palette: Vec<String>,
    truncated: bool,
    scroll: usize,
    cursor: usize,
    anchor: Option<usize>,
    dragging: bool,
    close_column: usize,
    selection_label: String,
    title: String,
}

impl DiffPanel {
    pub fn new(config: &serde_json::Value) -> Self {
        let get_string = |key: &str| {
            config
                .get(key.strip_prefix("diff_").unwrap_or(key))
                .and_then(serde_json::Value::as_str)
                .map(String::from)
        };
        let get_u64 = |key: &str| {
            config
                .get(key.strip_prefix("diff_").unwrap_or(key))
                .and_then(serde_json::Value::as_u64)
        };
        Self {
            files: Vec::new(),
            error: None,
            loading: false,
            min_width: get_u64("diff_min_width").unwrap_or(110).max(60) as usize,
            percent: get_u64("diff_width_percent").unwrap_or(40).clamp(30, 70) as usize,
            refresh_ms: get_u64("diff_refresh_ms").unwrap_or(1000).max(250),
            height: 0,
            sections: Vec::new(),
            lines: Vec::new(),
            layout_width: None,
            palette: Vec::new(),
            truncated: false,
            scroll: 0,
            cursor: 0,
            anchor: None,
            dragging: false,
            close_column: 0,
            selection_label: get_string("diff_selection_label")
                .map(|label| sanitize_display(&label).replace('\n', " "))
                .filter(|label| !label.trim().is_empty())
                .unwrap_or_else(|| "⧉ {count} {lines} from diff".into()),
            title: get_string("diff_title").unwrap_or_else(|| "{count} {files} changed".into()),
        }
    }

    /// Narrow terminals keep the diff above the same full-width composer.
    pub fn left_width(&self, width: usize) -> Option<usize> {
        (width >= self.min_width).then(|| width.saturating_sub(2 + width * self.percent / 100))
    }

    /// Changed source clears pointer selection, never the snapshot owned by the draft.
    pub fn apply(&mut self, result: Result<Review, String>) {
        self.loading = false;
        match result {
            Ok(review) => {
                let changed = self.files != review.files
                    || self.truncated != review.truncated
                    || self.sections.len() != review.patches.len()
                    || self.sections.iter().zip(&review.patches).any(
                        |(section, (path, source))| {
                            section.path != *path || section.source != *source
                        },
                    );
                if changed {
                    self.anchor = None;
                    self.dragging = false;
                    self.layout_width = None;
                    self.sections = review
                        .patches
                        .into_iter()
                        .map(|(path, source)| {
                            let rows = patch::parse(&source);
                            let digits = rows
                                .iter()
                                .filter_map(|row| row.number)
                                .max()
                                .unwrap_or(0)
                                .to_string()
                                .len()
                                .max(2);
                            Section {
                                path,
                                source,
                                rows,
                                styled: Vec::new(),
                                digits,
                            }
                        })
                        .collect();
                }
                self.files = review.files;
                self.truncated = review.truncated;
                self.error = None;
            }
            Err(error) => {
                self.error = Some(error);
                self.anchor = None;
                self.dragging = false;
            }
        }
    }

    /// Reflow the scrolling document only after a source or width change.
    fn layout(&mut self, width: usize) {
        if self.layout_width == Some(width) {
            return;
        }
        self.anchor = None;
        self.dragging = false;
        self.layout_width = Some(width);
        self.lines = (0..self.files.len()).map(Line::File).collect();
        for (section, file) in self.sections.iter().enumerate() {
            self.lines
                .extend([Line::Divider, Line::Heading(section), Line::Divider]);
            let code_width = width.saturating_sub(file.digits + 4).max(1);
            for (row, source) in file.rows.iter().enumerate() {
                let (mut column, mut start) = (0, 0);
                self.lines.push(Line::Source {
                    section,
                    row,
                    column: 0,
                });
                for c in source.display.chars() {
                    let cells = c.width().unwrap_or(0);
                    if cells > 0 && column > start && column + cells - start > code_width {
                        start = column;
                        self.lines.push(Line::Source {
                            section,
                            row,
                            column,
                        });
                    }
                    column += cells;
                }
            }
        }
        if self.truncated {
            self.lines.push(Line::Omitted);
        }
        if !self.lines.is_empty() {
            self.lines.push(Line::Divider);
        }
    }

    /// Count logical source rows, excluding headers and repeated wrapped fragments.
    fn selected_source(&self) -> Vec<(usize, usize)> {
        let Some(anchor) = self.anchor else {
            return Vec::new();
        };
        let mut selected = Vec::new();
        for line in self
            .lines
            .iter()
            .take(anchor.max(self.cursor) + 1)
            .skip(anchor.min(self.cursor))
        {
            if let Line::Source { section, row, .. } = *line {
                if self.sections[section].rows[row].number.is_some()
                    && selected.last() != Some(&(section, row))
                {
                    selected.push((section, row));
                }
            }
        }
        selected
    }

    /// Mouse release replaces the draft's diff attachment with this source snapshot.
    fn attach(&self) -> Action {
        if self.error.is_some() {
            return Action::None;
        }
        let selected = self.selected_source();
        if selected.is_empty() {
            return Action::None;
        }
        let count = selected.len();
        let label = sanitize_display(&self.selection_label)
            .replace("{count}", &count.to_string())
            .replace("{lines}", if count == 1 { "line" } else { "lines" })
            .replace('\n', " ");
        let mut content = String::new();
        let mut previous = None;
        for (section, row) in selected {
            if previous != Some(section) {
                if previous.is_some() {
                    content.push('\n');
                }
                content.push_str(&format!(
                    "Selected lines from {}:\n",
                    display_path(&self.sections[section].path)
                ));
                previous = Some(section);
            }
            content.push_str(&self.sections[section].rows[row].text);
            content.push('\n');
        }
        Action::Attach { label, content }
    }

    /// Scroll without taking composer focus. File summaries jump to their source;
    /// selecting and releasing source updates prompt context without an Enter action.
    pub fn mouse(&mut self, event: MouseEvent) -> Action {
        let row = event.row as usize;
        if matches!(event.kind, MouseEventKind::Up(MouseButton::Left)) {
            let was_dragging = std::mem::take(&mut self.dragging);
            return if was_dragging {
                self.attach()
            } else {
                Action::None
            };
        }
        if row >= self.height {
            return Action::None;
        }
        let index = self.scroll + row.saturating_sub(1);
        match event.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                self.dragging = false;
                if row == 0 {
                    if event.column as usize >= self.close_column {
                        return Action::Close;
                    }
                    return Action::None;
                }
                self.anchor = None;
                match self.lines.get(index).copied() {
                    Some(Line::File(file)) => {
                        if let Some(section) = self
                            .sections
                            .iter()
                            .position(|section| section.path == self.files[file].path)
                        {
                            if let Some(at) = self
                                .lines
                                .iter()
                                .position(|line| matches!(line, Line::Heading(n) if *n == section))
                            {
                                self.scroll = at.saturating_sub(1);
                            }
                        } else if self.truncated {
                            self.scroll = self
                                .lines
                                .len()
                                .saturating_sub(self.height.saturating_sub(1));
                        }
                    }
                    Some(Line::Source { section, row, .. })
                        if self.sections[section].rows[row].number.is_some() =>
                    {
                        self.cursor = index;
                        self.anchor = Some(index);
                        self.dragging = true;
                    }
                    _ => {}
                }
            }
            MouseEventKind::Drag(MouseButton::Left) if self.dragging && row > 0 => {
                self.cursor = index.min(self.lines.len().saturating_sub(1));
            }
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown if row > 0 => {
                let step = if event.modifiers.contains(KeyModifiers::SHIFT) {
                    self.height.saturating_sub(1).max(1)
                } else {
                    3
                };
                self.scroll = if event.kind == MouseEventKind::ScrollUp {
                    self.scroll.saturating_sub(step)
                } else {
                    (self.scroll + step).min(
                        self.lines
                            .len()
                            .saturating_sub(self.height.saturating_sub(1)),
                    )
                };
            }
            _ => {}
        }
        Action::None
    }

    /// Only visible files pay for syntax highlighting; caches survive scrolling.
    pub fn render(&mut self, theme: &Theme, width: usize, height: usize) -> Vec<String> {
        self.height = height;
        self.layout(width);
        self.scroll = self
            .scroll
            .min(self.lines.len().saturating_sub(height.saturating_sub(1)));
        let palette = [
            "diffSyntaxKeyword",
            "diffSyntaxString",
            "diffSyntaxNumber",
            "diffSyntaxComment",
            "diffSyntaxFunction",
            "diffSyntaxType",
        ]
        .map(|token| theme.fg_prefix(token).to_string())
        .to_vec();
        if self.palette != palette {
            for section in &mut self.sections {
                section.styled.clear();
            }
            self.palette = palette;
        }
        let title = self.document_title(theme, width);
        let header = format!(
            "{title}{}{} ",
            " ".repeat(width.saturating_sub(visible_width(&title) + 2)),
            theme.fg("dim", "×")
        );
        self.close_column = width.saturating_sub(2);
        let mut body = Vec::new();
        if let Some(error) = &self.error {
            body.push(theme.fg("error", &sanitize_display(error).replace('\n', " ")));
        } else if self.files.is_empty() {
            body.push(theme.fg(
                "dim",
                if self.loading {
                    "Loading…"
                } else {
                    "Working tree clean"
                },
            ));
        } else {
            let first = self.scroll;
            let last = self
                .lines
                .len()
                .min(first + height.saturating_sub(1).max(1));
            for index in first..last {
                let selected = self.anchor.is_some_and(|anchor| {
                    index >= anchor.min(self.cursor) && index <= anchor.max(self.cursor)
                });
                body.push(self.line_row(theme, width, index, selected));
            }
        }
        panel::review_frame(theme, width, header, body, height)
    }

    /// The whole review document as styled rows for transcript output: every
    /// line, no viewport scroll, no pane background. The command surface of
    /// the extension — the transcript re-wraps rows to the terminal width.
    pub fn document(&mut self, theme: &Theme, width: usize) -> Vec<String> {
        self.layout(width);
        let palette: Vec<String> = [
            "diffSyntaxKeyword",
            "diffSyntaxString",
            "diffSyntaxNumber",
            "diffSyntaxComment",
            "diffSyntaxFunction",
            "diffSyntaxType",
        ]
        .map(|token| theme.fg_prefix(token).to_string())
        .to_vec();
        if self.palette != palette {
            for section in &mut self.sections {
                section.styled.clear();
            }
            self.palette = palette;
        }
        if let Some(error) = &self.error {
            return vec![theme.fg("error", &sanitize_display(error).replace('\n', " "))];
        }
        if self.files.is_empty() {
            return vec![theme.fg("dim", "Working tree clean")];
        }
        let mut rows = vec![self.document_title(theme, width)];
        for index in 0..self.lines.len() {
            rows.push(self.line_row(theme, width, index, false));
        }
        rows
    }

    /// Bold count line shared by the pane header and the transcript document.
    fn document_title(&self, theme: &Theme, width: usize) -> String {
        let count = self.files.len();
        let title = sanitize_display(&self.title)
            .replace('\n', " ")
            .replace("{count}", &count.to_string())
            .replace("{files}", if count == 1 { "file" } else { "files" });
        let added = self
            .files
            .iter()
            .filter_map(|file| file.added)
            .fold(0usize, usize::saturating_add);
        let removed = self
            .files
            .iter()
            .filter_map(|file| file.removed)
            .fold(0usize, usize::saturating_add);
        clip_styled(
            &format!(
                " {}  {}",
                bold(&theme.fg("diffText", &title)),
                stats(theme, Some(added), Some(removed))
            ),
            width.saturating_sub(3),
        )
    }

    /// One document row by index. Highlighting is computed lazily per
    /// section and cached, so a transcript dump pays only for what it shows.
    fn line_row(&mut self, theme: &Theme, width: usize, index: usize, selected: bool) -> String {
        match self.lines[index] {
            Line::File(file) => {
                let file = &self.files[file];
                let stats = stats(theme, file.added, file.removed);
                let name = crate::style::clip_plain(
                    &display_path(&file.path),
                    width.saturating_sub(visible_width(&stats) + 3),
                );
                format!(
                    " {}{}{stats} ",
                    theme.fg("dim", &name),
                    " ".repeat(
                        width.saturating_sub(visible_width(&name) + visible_width(&stats) + 2)
                    )
                )
            }
            Line::Divider => theme.fg(
                "border",
                &format!(" {} ", "─".repeat(width.saturating_sub(2))),
            ),
            Line::Heading(section) => format!(
                " {}",
                bold(&theme.fg("diffText", &display_path(&self.sections[section].path)))
            ),
            Line::Omitted => theme.fg(
                "dim",
                " More files omitted: review size or time limit reached",
            ),
            Line::Source {
                section,
                row,
                column,
            } => {
                let section = &mut self.sections[section];
                if section.styled.is_empty() {
                    let lang = section
                        .path
                        .extension()
                        .and_then(|s| s.to_str())
                        .unwrap_or("");
                    section.styled = patch::syntax(&section.rows, theme, lang);
                }
                source_row(theme, section, row, column, width, selected)
            }
        }
    }
}

/// Drop zero counts; keep unknown binary counts explicit.
fn stats(theme: &Theme, added: Option<usize>, removed: Option<usize>) -> String {
    [(added, '+', "diffAdded"), (removed, '-', "diffRemoved")]
        .into_iter()
        .filter(|(count, _, _)| *count != Some(0))
        .map(|(count, sign, token)| {
            theme.fg(
                token,
                &format!(
                    "{sign}{}",
                    count.map(|n| n.to_string()).unwrap_or_else(|| "?".into())
                ),
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Wrapped fragments repeat their sign but print the line number only once.
fn source_row(
    theme: &Theme,
    section: &Section,
    index: usize,
    column: usize,
    width: usize,
    selected: bool,
) -> String {
    let row = &section.rows[index];
    let selected = selected && row.number.is_some();
    let bg = if selected {
        "diffSelectedBg"
    } else {
        match row.kind {
            '+' => "diffAddedBg",
            '-' => "diffRemovedBg",
            _ => "diffPaneBg",
        }
    };
    let marker = match row.kind {
        '+' => "diffAdded",
        '-' => "diffRemoved",
        _ => "diffLineNumber",
    };
    let number = if column == 0 {
        row.number.map(|n| n.to_string()).unwrap_or_default()
    } else {
        String::new()
    };
    let sign = if row.number.is_some() { row.kind } else { ' ' };
    let digits = section.digits;
    let gutter = clip_styled(
        &format!(
            "{}{}",
            theme.fg(
                if selected {
                    "diffText"
                } else {
                    "diffLineNumber"
                },
                &format!(" {number:>digits$} ")
            ),
            theme.fg(marker, &format!("{sign} "))
        ),
        width,
    );
    let text = patch::code(
        theme,
        &section.styled[index],
        &row.words,
        column,
        width.saturating_sub(visible_width(&gutter)),
        bg,
        if row.kind == '-' {
            "diffRemovedWordBg"
        } else {
            "diffAddedWordBg"
        },
    );
    theme.bg(bg, &theme.fg("diffText", &format!("{gutter}{text}")))
}

/// Keep unusual Git path bytes from creating extra terminal rows.
fn display_path(path: &Path) -> String {
    sanitize_display(&path.to_string_lossy())
        .replace('\n', "\\n")
        .replace('\t', "\\t")
}
