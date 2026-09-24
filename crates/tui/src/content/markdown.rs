//! Markdown → styled terminal lines, in the reference design's shapes.
//!
//! Visual contract, byte-pinned by the parity tests:
//!   headings   level-specific SGR (see `ansi::heading_style`), inner bold
//!              markers stripped, underline restored after links
//!   bullets    dim `• ` (the space rides inside the dim run), two columns
//!              of indent per level; ordered markers dim, source numbers kept
//!   tasks      dim `☐` pending, accent `✓` done — the marker replaces the
//!              bullet
//!   code       a dim box `┌─ lang ─┐` with `│` side rails around
//!              flush-left code; unboxed below six columns
//!   quotes     dim `│ ` rail per nesting level, body upright
//!   rules      fixed 60 columns, SGR dim
//!   tables     a boxed `┌┬┐` grid with a bold header and `:---:`
//!              alignment honored; a vertical `header: value` box when the
//!              grid doesn't fit
//!   inline     bold/italic/strike as SGR; code spans in the palette's
//!              inline-code gray; links underline-only with OSC 8; bare
//!              http(s) URLs autolink with trailing punctuation trimmed
//!
//! Parsing uses pulldown-cmark; rendering owns the width, so blocks land on
//! their final lines directly.

use pulldown_cmark::{Alignment, Event, Options, Parser, Tag, TagEnd};
use unicode_width::UnicodeWidthChar;

use crate::highlight::highlight_block;
use crate::render::*;
use crate::theme::Theme;

/// A link open carrying a document-scoped id, so a link split across
/// wrapped rows stays one link in id-aware terminals. Whitespace — legal in
/// a `<…>` destination — is percent-encoded: the word-wrapper splits on
/// spaces and must never find one inside the sequence.
fn osc8_id(id: u64, url: &str) -> String {
    let url: String = url
        .chars()
        .map(|c| {
            if c.is_whitespace() {
                c.encode_utf8(&mut [0; 4])
                    .bytes()
                    .map(|b| format!("%{b:02X}"))
                    .collect()
            } else {
                c.to_string()
            }
        })
        .collect();
    format!("\x1b]8;id=e-{id};{url}\x1b\\")
}
const OSC8_CLOSE: &str = "\x1b]8;;\x1b\\";

/// A URL safe to embed in an OSC 8 sequence: bounded, and free of control
/// bytes that would terminate or corrupt the sequence (a `\x07` or `\x1b`
/// inside the payload breaks out of the hyperlink and leaks the rest as
/// terminal input). Anything else renders as plain text instead.
fn valid_link_url(url: &str) -> bool {
    url.len() <= 2083 && !url.chars().any(|c| c.is_control())
}

// Terminal-column measurement and clipping for styled rows.

/// Visible width of a styled string (ANSI SGR and OSC sequences are zero).
pub fn visible_width(styled: &str) -> usize {
    let mut width = 0;
    let mut chars = styled.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            match chars.peek() {
                Some('[') => {
                    while let Some(&n) = chars.peek() {
                        chars.next();
                        if n.is_ascii_alphabetic() {
                            break;
                        }
                    }
                }
                Some(']') => {
                    // OSC … terminated by BEL or ST (ESC \)
                    while let Some(n) = chars.next() {
                        if n == '\x07' {
                            break;
                        }
                        if n == '\x1b' {
                            chars.next();
                            break;
                        }
                    }
                }
                _ => {}
            }
            continue;
        }
        width += c.width().unwrap_or(0);
    }
    width
}

/// Clip a styled line to `max` visible columns, passing escape sequences
/// through untouched. Width is display columns (CJK is two, combining
/// zero), and OSC sequences copy through their BEL/ST terminator — cutting
/// an OSC 8 link mid-URL would leak the rest as visible text. A clipped
/// line closes any hyperlink and SGR run so nothing bleeds past it.
pub fn clip_styled(styled: &str, max: usize) -> String {
    if visible_width(styled) <= max {
        return styled.to_string();
    }
    let mut out = String::new();
    let mut visible = 0usize;
    let mut chars = styled.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            out.push(c);
            if chars.peek() == Some(&']') {
                // OSC: runs to BEL or ST (ESC \).
                while let Some(n) = chars.next() {
                    out.push(n);
                    if n == '\x07' {
                        break;
                    }
                    if n == '\x1b' {
                        if let Some(t) = chars.next() {
                            out.push(t);
                        }
                        break;
                    }
                }
            } else {
                // CSI and friends: runs to the alphabetic final byte.
                for e in chars.by_ref() {
                    out.push(e);
                    if e.is_ascii_alphabetic() || e == '\\' {
                        break;
                    }
                }
            }
            continue;
        }
        let w = c.width().unwrap_or(0);
        if visible + w > max {
            break;
        }
        out.push(c);
        visible += w;
    }
    out.push_str(OSC8_CLOSE);
    out.push_str("\x1b[m");
    out
}

/// The inline styling open at some point in a line: SGR attributes, the
/// foreground, and any OSC 8 hyperlink. The reference closes everything at a
/// wrap seam (`\x1b[0m`, link terminator) and reopens it on the next row, so
/// a repainted row never depends on the row above it.
#[derive(Default, Clone)]
struct StyleState {
    bold: bool,
    dim: bool,
    italic: bool,
    underline: bool,
    strike: bool,
    fg: Option<String>,
    /// The open hyperlink's raw OSC 8 payload (`params;uri`), kept whole so
    /// a reopen carries the same id and the halves stay one link.
    link: Option<String>,
}

impl StyleState {
    /// Scan `text` and fold its escape sequences into the state.
    fn advance(&mut self, text: &str) {
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            if c != '\x1b' {
                continue;
            }
            match chars.peek() {
                Some('[') => {
                    chars.next();
                    let mut body = String::new();
                    for n in chars.by_ref() {
                        if n.is_ascii_alphabetic() {
                            if n == 'm' {
                                self.apply_sgr(&body);
                            }
                            break;
                        }
                        body.push(n);
                    }
                }
                Some(']') => {
                    let mut body = String::new();
                    chars.next();
                    while let Some(n) = chars.next() {
                        if n == '\x07' {
                            break;
                        }
                        if n == '\x1b' {
                            chars.next();
                            break;
                        }
                        body.push(n);
                    }
                    // OSC 8: `8;params;uri` — an empty uri closes the link.
                    if let Some(rest) = body.strip_prefix("8;") {
                        let uri = rest.split_once(';').map(|(_, u)| u).unwrap_or("");
                        self.link = if uri.is_empty() {
                            None
                        } else {
                            Some(rest.to_string())
                        };
                    }
                }
                _ => {}
            }
        }
    }

    fn apply_sgr(&mut self, body: &str) {
        let mut params = body.split(';');
        while let Some(p) = params.next() {
            match p {
                "" | "0" => {
                    let link = self.link.take();
                    *self = StyleState::default();
                    self.link = link;
                }
                "1" => self.bold = true,
                "2" => self.dim = true,
                "3" => self.italic = true,
                "4" => self.underline = true,
                "9" => self.strike = true,
                "22" => {
                    self.bold = false;
                    self.dim = false;
                }
                "23" => self.italic = false,
                "24" => self.underline = false,
                "29" => self.strike = false,
                "39" => self.fg = None,
                "38" => {
                    let rest: Vec<&str> = params.collect();
                    self.fg = Some(format!("38;{}", rest.join(";")));
                    break;
                }
                _ => {}
            }
        }
    }

    /// The sequences reopening this state at a row start.
    fn opens(&self) -> String {
        let mut out = String::new();
        if let Some(payload) = &self.link {
            out.push_str(&format!("\x1b]8;{payload}\x1b\\"));
        }
        if self.bold {
            out.push_str(BOLD_ON);
        }
        if self.dim {
            out.push_str(DIM_ON);
        }
        if self.italic {
            out.push_str(ITALIC_ON);
        }
        if self.underline {
            out.push_str(UNDERLINE_ON);
        }
        if self.strike {
            out.push_str(STRIKE_ON);
        }
        if let Some(fg) = &self.fg {
            out.push_str(&format!("\x1b[{fg}m"));
        }
        out
    }

    /// The sequences closing this state at a row end.
    fn closes(&self) -> String {
        let mut out = String::new();
        if self.link.is_some() {
            out.push_str(OSC8_CLOSE);
        }
        if self.bold
            || self.dim
            || self.italic
            || self.underline
            || self.strike
            || self.fg.is_some()
        {
            out.push_str("\x1b[0m");
        }
        out
    }
}

/// One word of a hard line, measured once for the row assignment.
struct WrapTok {
    text: String,
    width: usize,
    /// A piece of a force-broken over-long word: always ends its row.
    breaks_after: bool,
    /// Continues the previous token with no joining space.
    glue: bool,
}

/// Word-wrap a styled string the reference way: styling closes at every
/// seam and reopens on the next row (a repainted row stands alone), and a
/// single-word last line pulls the previous word down with it when it fits —
/// no orphans. A single token wider than the line (URL, hash, path)
/// hard-wraps across rows.
pub fn wrap_styled(styled: &str, width: usize) -> Vec<String> {
    let mut rows = Vec::new();
    for hard in styled.split('\n') {
        let toks = wrap_tokens(hard, width);
        let mut lines = assign_rows(&toks, width);
        avoid_orphan(&mut lines, &toks, width);
        emit_rows(&mut rows, &toks, lines);
    }
    if rows.is_empty() {
        rows.push(String::new());
    }
    rows
}

/// Split one hard line into words, with over-long words pre-split into
/// row pieces.
fn wrap_tokens(hard: &str, width: usize) -> Vec<WrapTok> {
    let mut toks: Vec<WrapTok> = Vec::new();
    for word in hard.split(' ') {
        let w = visible_width(word);
        if w > width && width > 0 {
            let pieces = hard_wrap(word, width);
            let count = pieces.len();
            for (k, piece) in pieces.into_iter().enumerate() {
                toks.push(WrapTok {
                    width: visible_width(&piece),
                    text: piece,
                    breaks_after: k + 1 < count,
                    glue: k > 0,
                });
            }
        } else {
            toks.push(WrapTok {
                text: word.to_string(),
                width: w,
                breaks_after: false,
                glue: false,
            });
        }
    }
    toks
}

/// Greedy assignment of token indices to rows of at most `width` columns.
fn assign_rows(toks: &[WrapTok], width: usize) -> Vec<Vec<usize>> {
    let mut lines: Vec<Vec<usize>> = Vec::new();
    let mut current: Vec<usize> = Vec::new();
    let mut cur_width = 0usize;
    for (i, tok) in toks.iter().enumerate() {
        let needed = if current.is_empty() || tok.glue {
            tok.width
        } else {
            1 + tok.width
        };
        if !current.is_empty() && !tok.glue && cur_width + needed > width {
            lines.push(std::mem::take(&mut current));
            cur_width = 0;
        }
        cur_width += if current.is_empty() {
            tok.width
        } else {
            needed
        };
        current.push(i);
        if tok.breaks_after {
            lines.push(std::mem::take(&mut current));
            cur_width = 0;
        }
    }
    lines.push(current);
    if lines.last().map(|l| l.is_empty()).unwrap_or(false) && lines.len() > 1 {
        lines.pop();
    }
    lines
}

/// Orphan avoidance: a lone word on the last row pulls the previous row's
/// final word down when the pair fits.
fn avoid_orphan(lines: &mut [Vec<usize>], toks: &[WrapTok], width: usize) {
    if lines.len() < 2 {
        return;
    }
    let last = lines.len() - 1;
    let lone = lines[last].len() == 1 && !toks[lines[last][0]].glue && lines[last - 1].len() >= 2;
    if !lone {
        return;
    }
    if let Some(&moved) = lines[last - 1].last() {
        let orphan = lines[last][0];
        if !toks[moved].breaks_after
            && !toks[moved].glue
            && toks[moved].width + 1 + toks[orphan].width <= width
        {
            lines[last - 1].pop();
            lines[last].insert(0, moved);
        }
    }
}

/// Join each row's tokens onto `rows`, carrying the style state across
/// seams: every row but the last closes it, every row but the first reopens it.
fn emit_rows(rows: &mut Vec<String>, toks: &[WrapTok], lines: Vec<Vec<usize>>) {
    let mut state = StyleState::default();
    let line_count = lines.len();
    for (r, line) in lines.into_iter().enumerate() {
        let mut row = String::new();
        if r > 0 {
            row.push_str(&state.opens());
        }
        for (j, ti) in line.into_iter().enumerate() {
            if j > 0 && !toks[ti].glue {
                row.push(' ');
            }
            row.push_str(&toks[ti].text);
            state.advance(&toks[ti].text);
        }
        if r + 1 < line_count {
            row.push_str(&state.closes());
        }
        rows.push(row);
    }
}

/// Hard-wrap one code line, closing and reopening any open color at the seam.
fn hard_wrap(line: &str, width: usize) -> Vec<String> {
    wrap_code_line(line, width, "")
}

/// Hard-wrap one code line with the reference's continuation indent: rows
/// after the first re-emit `indent` (the line's own leading whitespace) and
/// wrap the remaining columns. Open colors close at each seam (`\x1b[0m`)
/// and reopen on the next row.
fn wrap_code_line(line: &str, width: usize, indent: &str) -> Vec<String> {
    if visible_width(line) <= width || width == 0 {
        return vec![line.to_string()];
    }
    let indent_width = indent.chars().count();
    let mut rows = Vec::new();
    let mut state = StyleState::default();
    let mut row = String::new();
    let mut row_width = 0usize;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            let mut seq = String::from("\x1b");
            while let Some(&n) = chars.peek() {
                seq.push(n);
                chars.next();
                if n.is_ascii_alphabetic() {
                    break;
                }
            }
            state.advance(&seq);
            row.push_str(&seq);
            continue;
        }
        if c == '\x1b' && chars.peek() == Some(&']') {
            // OSC (hyperlinks): zero width and copied whole — a split
            // mid-sequence would count the payload as visible columns and
            // leave the terminal parsing rows as OSC data.
            let mut seq = String::from("\x1b");
            while let Some(n) = chars.next() {
                seq.push(n);
                if n == '\x07' {
                    break;
                }
                if n == '\x1b' {
                    if let Some(t) = chars.next() {
                        seq.push(t);
                    }
                    break;
                }
            }
            state.advance(&seq);
            row.push_str(&seq);
            continue;
        }
        let w = c.width().unwrap_or(0);
        let cap = if rows.is_empty() {
            width
        } else {
            width.saturating_sub(indent_width).max(1)
        };
        if row_width + w > cap {
            row.push_str(&state.closes());
            let finished = if rows.is_empty() {
                std::mem::take(&mut row)
            } else {
                format!("{indent}{}", std::mem::take(&mut row))
            };
            rows.push(finished);
            row.push_str(&state.opens());
            row_width = 0;
        }
        row.push(c);
        row_width += w;
    }
    if row_width > 0 || rows.is_empty() {
        row.push_str(&state.closes());
        let finished = if rows.is_empty() {
            row
        } else {
            format!("{indent}{row}")
        };
        rows.push(finished);
    }
    rows
}

/// A compact boxed code block with the language in its top border. Code stays
/// flush-left inside dim side rails; below six columns the border disappears
/// and code wraps bare.
pub fn code_panel(theme: &Theme, code: &str, language: &str, cols: usize) -> Vec<String> {
    let source = code.trim_end_matches('\n');
    // An unlabeled fence renders raw — the highlighter colors nothing it
    // cannot name, and nothing guesses a language the author didn't give.
    let lines = highlight_block(theme, language, source);

    // The reference renders bare wrapped code when the frame can't hold a
    // six-column rule.
    if cols <= 5 {
        let mut out = Vec::new();
        for line in &lines {
            let indent: String = line.chars().take_while(|c| c.is_whitespace()).collect();
            out.extend(wrap_code_line(line, cols.max(1), &indent));
        }
        return out;
    }

    let max_code_width = lines.iter().map(|l| visible_width(l)).max().unwrap_or(0);
    let label_width = language
        .chars()
        .map(|c| c.width().unwrap_or(0))
        .sum::<usize>();
    let panel_width = (max_code_width + 2)
        .max(if label_width > 0 { label_width + 6 } else { 0 })
        .max(6)
        .min(cols);
    let inner_width = panel_width - 2;

    let mut out = Vec::new();
    if label_width > 0 {
        // Label truncated by display width to the top border's inner width.
        let mut shown = String::new();
        let mut used = 0usize;
        for c in language.chars() {
            let w = c.width().unwrap_or(0);
            if used + w > inner_width.saturating_sub(3) {
                break;
            }
            shown.push(c);
            used += w;
        }
        if shown.is_empty() {
            shown.push('?');
            used = 1;
        }
        let tail = "─".repeat(inner_width.saturating_sub(3 + used));
        out.push(format!("{DIM_ON}┌─ {shown} {tail}┐{WEIGHT_OFF}"));
    } else {
        out.push(format!("{DIM_ON}┌{}┐{WEIGHT_OFF}", "─".repeat(inner_width)));
    }
    for line in &lines {
        let indent: String = line.chars().take_while(|c| c.is_whitespace()).collect();
        for wrapped in wrap_code_line(line, inner_width, &indent) {
            let padding = " ".repeat(inner_width.saturating_sub(visible_width(&wrapped)));
            out.push(format!(
                "{DIM_ON}│{WEIGHT_OFF}{wrapped}{padding}{DIM_ON}│{WEIGHT_OFF}"
            ));
        }
    }
    out.push(format!("{DIM_ON}└{}┘{WEIGHT_OFF}", "─".repeat(inner_width)));
    out
}

/// Append text to the inline run, autolinking bare http(s) URLs the
/// reference way: underline + OSC 8, trailing `.,;:!?` left outside.
fn push_text_autolinked(inline: &mut String, text: &str, link_seq: &mut u64) {
    let mut rest = text;
    loop {
        let Some(found) = rest.match_indices("http").map(|(i, _)| i).find(|&i| {
            let bounded = i == 0
                || rest[..i]
                    .chars()
                    .next_back()
                    .map(|c| !c.is_alphanumeric())
                    .unwrap_or(true);
            bounded && (rest[i..].starts_with("https://") || rest[i..].starts_with("http://"))
        }) else {
            inline.push_str(rest);
            return;
        };
        inline.push_str(&rest[..found]);
        let tail = &rest[found..];
        let end = tail.find(|c: char| c.is_whitespace()).unwrap_or(tail.len());
        let mut url = &tail[..end];
        while let Some(last) = url.chars().next_back() {
            if matches!(last, '.' | ',' | ';' | ':' | '!' | '?') {
                url = &url[..url.len() - last.len_utf8()];
            } else {
                break;
            }
        }
        let scheme_len = if url.starts_with("https://") { 8 } else { 7 };
        if url.len() <= scheme_len || !valid_link_url(url) {
            // A bare scheme is text, not a link; so is an invalid URL.
            inline.push_str(&tail[..end]);
        } else {
            *link_seq += 1;
            inline.push_str(&osc8_id(*link_seq, url));
            inline.push_str(UNDERLINE_ON);
            inline.push_str(url);
            inline.push_str(UNDERLINE_OFF);
            inline.push_str(OSC8_CLOSE);
            inline.push_str(&tail[url.len()..end]);
        }
        rest = &tail[end..];
    }
}

/// A finished block joins the message flow, with the blank separator row.
fn push_block(out: &mut Vec<String>, lines: Vec<String>) {
    if lines.is_empty() {
        return;
    }
    if !out.is_empty() {
        out.push(String::new());
    }
    out.extend(lines);
}

/// One open list: its numbering when ordered.
struct ListState {
    ordered: Option<u64>,
    /// The item number as written in the source — the reference echoes the
    /// author's markers instead of renumbering.
    source: Option<u64>,
}

/// A table being collected cell by cell until its end renders it whole.
struct TableState {
    header: Vec<String>,
    rows: Vec<Vec<String>>,
    aligns: Vec<Alignment>,
    in_header: bool,
}

/// Render a markdown document to lines at `width`, one blank row between blocks.
pub fn render_markdown(theme: &Theme, markdown: &str, width: usize) -> Vec<String> {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_TASKLISTS);
    // Footnote syntax stays inert: `[^label]` renders as the literal text
    // the author wrote and `[^a]: note` as an ordinary paragraph. The
    // reference's footnote grammar was ported once and retired — a coding
    // session's prose doesn't carry academic apparatus.
    let mut doc = Document::new(theme, markdown, width);
    for (event, range) in Parser::new_ext(markdown, opts).into_offset_iter() {
        doc.event(event, range.start);
    }
    doc.out
}

/// One document's rendering state as parser events stream through it.
/// Inline events accumulate styled text in `inline`; a block's end wraps
/// that text at `width` and pushes the finished block onto `out`.
struct Document<'a> {
    theme: &'a Theme,
    source: &'a str,
    width: usize,
    out: Vec<String>,
    inline: String,
    heading: Option<u8>,
    lists: Vec<ListState>,
    /// Rendered rows of the current top-level list block.
    list_rows: Vec<String>,
    /// One flag per open item: has its own inline text been emitted yet?
    item_stack: Vec<bool>,
    current_task: Option<bool>,
    quote_depth: usize,
    /// Each open link or image keeps its OSC 8 opener. Images may nest inside
    /// links, so closing one must restore the parent's hyperlink and inline state.
    link_stack: Vec<Option<String>>,
    link_seq: u64,
    image_mark: Option<usize>,
    /// The open code block's language and buffered source.
    code: Option<(String, String)>,
    table: Option<TableState>,
}

impl<'a> Document<'a> {
    fn new(theme: &'a Theme, source: &'a str, width: usize) -> Self {
        Self {
            theme,
            source,
            width,
            out: Vec::new(),
            inline: String::new(),
            heading: None,
            lists: Vec::new(),
            list_rows: Vec::new(),
            item_stack: Vec::new(),
            current_task: None,
            quote_depth: 0,
            link_stack: Vec::new(),
            link_seq: 0,
            image_mark: None,
            code: None,
            table: None,
        }
    }

    /// Fold one parser event in; `start` is its byte offset in the source.
    fn event(&mut self, event: Event, start: usize) {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                self.heading = Some(level as u8);
                self.inline.clear();
            }
            Event::End(TagEnd::Heading(_)) => self.end_heading(),
            Event::Start(Tag::Paragraph) => self.start_paragraph(),
            Event::End(TagEnd::Paragraph) => self.end_paragraph(),
            Event::Start(Tag::BlockQuote(_)) => self.quote_depth += 1,
            Event::End(TagEnd::BlockQuote(_)) => {
                self.quote_depth = self.quote_depth.saturating_sub(1)
            }
            Event::Start(Tag::List(first)) => self.start_list(first),
            Event::End(TagEnd::List(_)) => self.end_list(),
            Event::Start(Tag::Item) => self.start_item(start),
            Event::End(TagEnd::Item) => self.end_item(),
            Event::TaskListMarker(done) => self.current_task = Some(done),
            Event::Start(Tag::CodeBlock(kind)) => {
                let lang = match kind {
                    pulldown_cmark::CodeBlockKind::Fenced(l) => {
                        l.split_whitespace().next().unwrap_or("").to_string()
                    }
                    _ => String::new(),
                };
                self.code = Some((lang, String::new()));
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some((lang, buffer)) = self.code.take() {
                    let lines = code_panel(self.theme, &buffer, &lang, self.width);
                    push_block(&mut self.out, lines);
                }
            }
            Event::Start(Tag::Table(aligns)) => {
                self.table = Some(TableState {
                    header: Vec::new(),
                    rows: Vec::new(),
                    aligns,
                    in_header: false,
                });
            }
            Event::Start(Tag::TableHead) => self.set_table_head(true),
            Event::End(TagEnd::TableHead) => self.set_table_head(false),
            Event::Start(Tag::TableRow) => {
                if let Some(t) = &mut self.table {
                    if !t.in_header {
                        t.rows.push(Vec::new());
                    }
                }
            }
            Event::Start(Tag::TableCell) => self.inline.clear(),
            Event::End(TagEnd::TableCell) => self.end_table_cell(),
            Event::End(TagEnd::Table) => {
                if let Some(t) = self.table.take() {
                    let lines = render_table(&t.header, &t.rows, &t.aligns, self.width);
                    push_block(&mut self.out, lines);
                }
            }
            Event::Rule => push_block(&mut self.out, vec![rule()]),
            // The reference strips bold/italic markers inside a heading
            // rather than nesting SGR into the level style.
            Event::Start(Tag::Strong)
            | Event::End(TagEnd::Strong)
            | Event::Start(Tag::Emphasis)
            | Event::End(TagEnd::Emphasis)
                if self.heading.is_some() => {}
            Event::Start(Tag::Strong) => self.inline.push_str(BOLD_ON),
            Event::End(TagEnd::Strong) => self.inline.push_str(WEIGHT_OFF),
            Event::Start(Tag::Emphasis) => self.inline.push_str(ITALIC_ON),
            Event::End(TagEnd::Emphasis) => self.inline.push_str(ITALIC_OFF),
            Event::Start(Tag::Strikethrough) => self.inline.push_str(STRIKE_ON),
            Event::End(TagEnd::Strikethrough) => self.inline.push_str(STRIKE_OFF),
            Event::Start(Tag::Link { dest_url, .. }) => self.open_link(&dest_url),
            Event::End(TagEnd::Link) => {
                if !self.close_link() && matches!(self.heading, Some(1) | Some(3) | Some(5)) {
                    // An underlined heading level reopens its underline after
                    // the link closes its own.
                    self.inline.push_str(UNDERLINE_ON);
                }
            }
            Event::Start(Tag::Image { dest_url, .. }) => {
                self.open_link(&dest_url);
                self.inline.push_str("▧ ");
                self.image_mark = Some(self.inline.len());
            }
            Event::End(TagEnd::Image) => {
                // Empty alt text names the thing for what it is.
                if self.image_mark.take() == Some(self.inline.len()) {
                    self.inline.push_str("image");
                }
                self.close_link();
            }
            Event::Code(text) => self.inline.push_str(&self.theme.fg("mdCode", &text)),
            Event::Text(text) => self.text(&text),
            // The reference preserves the author's line breaks: a soft break
            // is a real row boundary, not a joining space.
            Event::SoftBreak => self.inline.push('\n'),
            Event::HardBreak => self.inline.push('\n'),
            Event::Html(html) | Event::InlineHtml(html) => self.inline.push_str(&html),
            _ => {}
        }
    }

    /// Headings wrap with the level SGR reopened on every row.
    fn end_heading(&mut self) {
        let level = self.heading.take().unwrap_or(2);
        let rows = wrap_styled(&self.inline, self.width)
            .into_iter()
            .map(|row| heading_style(level, &row))
            .collect();
        push_block(&mut self.out, rows);
        self.inline.clear();
    }

    /// A loose item's later paragraphs continue the item: its text waits for
    /// `End(Item)`, so clearing here would drop every paragraph but the last.
    fn start_paragraph(&mut self) {
        if self.item_stack.is_empty() {
            self.inline.clear();
        } else if !self.inline.trim().is_empty() {
            self.inline.push('\n');
        }
    }

    /// A paragraph becomes a block of its own, railed inside a quote; inside
    /// an item it waits for the item's end, and inside a table for its cell.
    fn end_paragraph(&mut self) {
        if !self.item_stack.is_empty() {
            // handled at item end via `inline`
        } else if self.quote_depth > 0 {
            // One dim rail per nesting level, the reference way.
            let rail = quote_rail().repeat(self.quote_depth);
            let body_width = self.width.saturating_sub(2 * self.quote_depth).max(8);
            let rows: Vec<String> = wrap_styled(&self.inline, body_width)
                .into_iter()
                .map(|r| format!("{rail}{r}"))
                .collect();
            push_block(&mut self.out, rows);
            self.inline.clear();
        } else if self.table.is_none() {
            push_block(&mut self.out, wrap_styled(&self.inline, self.width));
            self.inline.clear();
        }
    }

    /// A list opening inside an item means the item's own text is done —
    /// emit it now so children render below their parent.
    fn start_list(&mut self, first: Option<u64>) {
        if self.item_stack.last() == Some(&false) {
            self.flush_item();
            if let Some(flushed) = self.item_stack.last_mut() {
                *flushed = true;
            }
        }
        self.lists.push(ListState {
            ordered: first,
            source: None,
        });
        if self.lists.len() == 1 {
            self.list_rows.clear();
        }
    }

    /// The outermost list's end pushes every row its items rendered as one block.
    fn end_list(&mut self) {
        self.lists.pop();
        if self.lists.is_empty() {
            let lines = std::mem::take(&mut self.list_rows);
            push_block(&mut self.out, lines);
        }
    }

    /// The reference echoes the source's ordered markers; read the number as
    /// the author wrote it at the item's `start` offset.
    fn start_item(&mut self, start: usize) {
        self.item_stack.push(false);
        self.current_task = None;
        if let Some(state) = self.lists.last_mut() {
            if state.ordered.is_some() {
                let digits: String = self.source[start..]
                    .chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect();
                state.source = digits.parse().ok();
            }
        }
        self.inline.clear();
    }

    fn end_item(&mut self) {
        let flushed = self.item_stack.pop().unwrap_or(false);
        if !flushed {
            self.flush_item();
        }
        self.inline.clear();
    }

    /// Emit the current item's inline text as glyph-prefixed, hanging-indented
    /// rows, consuming its task marker.
    fn flush_item(&mut self) {
        let task = self.current_task.take();
        let depth = self.lists.len().saturating_sub(1);
        let pad = "  ".repeat(depth);
        let Some(state) = self.lists.last_mut() else {
            return;
        };
        let (glyph, glyph_width) = item_marker(self.theme, state, task);
        let hanging = format!("{pad}{}", " ".repeat(glyph_width));
        let body_width = self
            .width
            .saturating_sub(pad.chars().count() + glyph_width)
            .max(8);
        for (i, row) in wrap_styled(self.inline.trim_end(), body_width)
            .into_iter()
            .enumerate()
        {
            if i == 0 {
                self.list_rows.push(format!("{pad}{glyph}{row}"));
            } else {
                self.list_rows.push(format!("{hanging}{row}"));
            }
        }
        self.inline.clear();
    }

    fn set_table_head(&mut self, in_header: bool) {
        if let Some(t) = &mut self.table {
            t.in_header = in_header;
        }
    }

    /// A finished cell moves the inline run into the header or the last body row.
    fn end_table_cell(&mut self) {
        if let Some(t) = &mut self.table {
            if t.in_header {
                t.header.push(std::mem::take(&mut self.inline));
            } else if let Some(last) = t.rows.last_mut() {
                last.push(std::mem::take(&mut self.inline));
            }
        }
    }

    /// Open a link or image: an oversized or control-laden URL never enters an
    /// OSC 8 sequence, and its label renders as plain underlined text. An
    /// enclosing link's hyperlink is closed first.
    fn open_link(&mut self, url: &str) {
        let opener = if valid_link_url(url) {
            self.link_seq += 1;
            Some(osc8_id(self.link_seq, url))
        } else {
            None
        };
        if self.link_stack.last().is_some_and(Option::is_some) {
            self.inline.push_str(OSC8_CLOSE);
        }
        if let Some(open) = &opener {
            self.inline.push_str(open);
        }
        self.inline.push_str(UNDERLINE_ON);
        self.link_stack.push(opener);
    }

    /// Close the innermost link or image and reopen its parent's hyperlink
    /// and underline. Returns whether a parent was open.
    fn close_link(&mut self) -> bool {
        self.inline.push_str(UNDERLINE_OFF);
        if self.link_stack.pop().flatten().is_some() {
            self.inline.push_str(OSC8_CLOSE);
        }
        let Some(parent) = self.link_stack.last() else {
            return false;
        };
        if let Some(open) = parent {
            self.inline.push_str(open);
        }
        self.inline.push_str(UNDERLINE_ON);
        true
    }

    /// Text lands in the open code block, verbatim inside links and headings,
    /// and autolinked everywhere else.
    fn text(&mut self, text: &str) {
        if let Some((_, buffer)) = &mut self.code {
            buffer.push_str(text);
        } else if !self.link_stack.is_empty() || self.heading.is_some() {
            self.inline.push_str(text);
        } else {
            push_text_autolinked(&mut self.inline, text, &mut self.link_seq);
        }
    }
}

/// An item's leading marker and its width in columns: the dim source number
/// or bullet, and any task checkbox. Advances an ordered list's count.
fn item_marker(theme: &Theme, state: &mut ListState, task: Option<bool>) -> (String, usize) {
    let checkbox = task.map(|done| {
        if done {
            format!("{} ", theme.fg("accent", "✓"))
        } else {
            format!("{DIM_ON}☐ {WEIGHT_OFF}")
        }
    });
    match &mut state.ordered {
        Some(n) => {
            let shown = state.source.take().unwrap_or(*n);
            let marker = format!("{shown}.");
            let w = marker.chars().count() + 1;
            *n = shown + 1;
            let lead = format!("{DIM_ON}{marker}{WEIGHT_OFF} ");
            match checkbox {
                Some(mark) => (format!("{lead}{mark}"), w + 2),
                None => (lead, w),
            }
        }
        // The reference's checkbox replaces the bullet glyph outright.
        None => match checkbox {
            Some(mark) => (mark, 2),
            None => (format!("{DIM_ON}• {WEIGHT_OFF}"), 2),
        },
    }
}

/// The header cell's bold, the reference way: the cell's own inline
/// `\x1b[22m` re-asserts bold so a nested span cannot switch the rest of
/// the header off; padding stays outside the bold run.
fn table_header_cell(cell: &str) -> String {
    format!(
        "{BOLD_ON}{}{WEIGHT_OFF}",
        cell.replace(WEIGHT_OFF, "\x1b[22m\x1b[1m")
    )
}

fn table_border(left: &str, middle: &str, right: &str, widths: &[usize]) -> String {
    let mut out = String::from(left);
    for (index, width) in widths.iter().enumerate() {
        if index > 0 {
            out.push_str(middle);
        }
        out.push_str(&"─".repeat(width + 2));
    }
    out.push_str(right);
    out
}

/// One `│…│` field row of the vertical layout, wrapping at the inner width
/// (escapes ride along at zero columns); a glyph too wide for the whole
/// column renders as `?`.
fn table_vertical_lines(out: &mut Vec<String>, content: &str, inner_width: usize) {
    if content.is_empty() {
        out.push(format!("│{}│", " ".repeat(inner_width)));
        return;
    }
    let chars: Vec<char> = content.chars().collect();
    let mut i = 0usize;
    while i < chars.len() {
        let mut row = String::new();
        let mut used = 0usize;
        let start = i;
        while i < chars.len() {
            let c = chars[i];
            if c == '\x1b' {
                // Copy the whole escape sequence at zero columns: CSI to its
                // final letter, OSC (hyperlinks) to BEL or ST — a split
                // mid-sequence would count the URI as visible columns and
                // leave the terminal reading the box as OSC data.
                row.push(c);
                i += 1;
                let osc = chars.get(i) == Some(&']');
                while i < chars.len() {
                    let n = chars[i];
                    row.push(n);
                    i += 1;
                    if osc {
                        if n == '\x07' {
                            break;
                        }
                        if n == '\x1b' {
                            if let Some(&t) = chars.get(i) {
                                row.push(t);
                                i += 1;
                            }
                            break;
                        }
                    } else if n.is_ascii_alphabetic() {
                        break;
                    }
                }
                continue;
            }
            let w = c.width().unwrap_or(0);
            if used + w > inner_width {
                break;
            }
            row.push(c);
            used += w;
            i += 1;
        }
        if i == start {
            // A single glyph wider than the whole column: substitute.
            row.push('?');
            used = 1.min(inner_width);
            i += 1;
        }
        out.push(format!(
            "│{row}{}│",
            " ".repeat(inner_width.saturating_sub(used))
        ));
    }
}

/// The boxed grid: `┌┬┐`, padded cells, `├┼┤` after the header and between
/// every body row, `└┴┘`. Body cells honor their column's alignment; the
/// header is always left and bold.
fn table_grid(
    out: &mut Vec<String>,
    header: &[String],
    rows: &[Vec<String>],
    aligns: &[Alignment],
    widths: &[usize],
) {
    out.push(table_border("┌", "┬", "┐", widths));
    let all_rows: Vec<&[String]> = std::iter::once(header)
        .chain(rows.iter().map(Vec::as_slice))
        .collect();
    for (row_index, row) in all_rows.iter().enumerate() {
        let mut line = String::from("│");
        for (col, width) in widths.iter().enumerate() {
            let cell = row.get(col).map(String::as_str).unwrap_or("");
            let pad = width.saturating_sub(visible_width(cell));
            let align = if row_index == 0 {
                Alignment::Left
            } else {
                aligns.get(col).copied().unwrap_or(Alignment::None)
            };
            let left_pad = match align {
                Alignment::Right => pad,
                Alignment::Center => pad / 2,
                _ => 0,
            };
            line.push(' ');
            line.push_str(&" ".repeat(left_pad));
            if row_index == 0 {
                line.push_str(&table_header_cell(cell));
            } else {
                line.push_str(cell);
            }
            line.push_str(&" ".repeat(pad - left_pad));
            line.push_str(" │");
        }
        out.push(line);
        let last = row_index + 1 == all_rows.len();
        if (row_index == 0 && all_rows.len() > 1) || (row_index > 0 && !last) {
            out.push(table_border("├", "┼", "┤", widths));
        }
    }
    out.push(table_border("└", "┴", "┘", widths));
}

/// The reference's transcript table ladder: a boxed grid (`┌┬┐`, padded
/// cells, `├┼┤` after the header and between every body row, `└┴┘`) when it
/// fits the frame; a vertical `header: value` box at exactly the frame's
/// width when it doesn't; bare clipped `header: value` lines below three
/// columns. `:---:` alignment holds in the grid, the header always left.
fn render_table(
    header: &[String],
    rows: &[Vec<String>],
    aligns: &[Alignment],
    cols: usize,
) -> Vec<String> {
    let ncols = header.len();
    let mut out = Vec::new();
    if cols <= 2 {
        for row in rows {
            for (col, name) in header.iter().enumerate().take(ncols) {
                let value = row.get(col).map(String::as_str).unwrap_or("");
                let field = format!("{name}: {value}");
                out.push(clip_styled(&field, cols));
            }
        }
        return out;
    }
    let mut widths = vec![0usize; ncols];
    for row in std::iter::once(header).chain(rows.iter().map(Vec::as_slice)) {
        for (col, cell) in row.iter().enumerate() {
            if col < ncols {
                widths[col] = widths[col].max(visible_width(cell));
            }
        }
    }
    let grid_width = ncols * 3 + 1 + widths.iter().sum::<usize>();
    if ncols > 0 && grid_width <= cols {
        table_grid(&mut out, header, rows, aligns, &widths);
        return out;
    }
    // Vertical fallback: one record per body row, `header: value` fields
    // boxed at exactly the frame's width.
    let inner_width = cols - 2;
    out.push(format!("┌{}┐", "─".repeat(inner_width)));
    for (row_index, row) in rows.iter().enumerate() {
        for (col, name) in header.iter().enumerate().take(ncols) {
            let value = row.get(col).map(String::as_str).unwrap_or("");
            let field = format!("{name}: {value}");
            table_vertical_lines(&mut out, &field, inner_width);
        }
        if row_index + 1 < rows.len() {
            out.push(format!("├{}┤", "─".repeat(inner_width)));
        }
    }
    out.push(format!("└{}┘", "─".repeat(inner_width)));
    out
}
