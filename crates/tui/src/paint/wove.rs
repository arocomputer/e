//! Terminal painting through Wove. The row adapter accepts only the SGR and
//! hyperlink metadata emitted by e's existing presenters. Wove receives typed
//! styles and text, and owns clipping, grapheme layout, and terminal output.
use std::io::{self, Write};
use wove::{
    text::{Span, TextLayout, Wrap},
    Buffer, Color, Rect, Renderer, Style,
};

/// Reuses a typed frame for rows whose positions e has already chosen.
#[derive(Default)]
pub(super) struct Frame {
    buffer: Option<Buffer>,
}

impl Frame {
    /// Lay out preformatted rows without changing application-owned wrapping.
    pub fn layout(&mut self, lines: &[String], cols: u16, rows: u16) -> &Buffer {
        let area = Rect::new(0, 0, cols, rows);
        let frame = self.buffer.get_or_insert_with(|| Buffer::new(cols, rows));
        if frame.area() != area {
            *frame = Buffer::new(cols, rows);
        }
        frame.clear();
        {
            let mut canvas = frame.canvas(area);
            for (row, line) in lines.iter().take(usize::from(rows)).enumerate() {
                TextLayout::new(&spans(line), Some(cols), Wrap::None)
                    .paint_at(&mut canvas, row as i32);
            }
        }
        frame
    }
}

/// A fixed-height view whose appearance remains owned by e's presenters.
#[derive(Default)]
pub(super) struct Fullscreen {
    frame: Frame,
    renderer: Renderer,
}

impl Fullscreen {
    /// Forget display contents after screen changes, resize, or a failed write.
    pub fn invalidate(&mut self) {
        self.renderer.invalidate();
    }

    /// Draw the visible tail, leaving the rest of the screen blank.
    pub fn paint_to(
        &mut self,
        lines: &[String],
        cols: u16,
        rows: u16,
        out: &mut impl Write,
    ) -> io::Result<()> {
        let lines = &lines[lines.len().saturating_sub(usize::from(rows))..];
        self.renderer
            .draw(out, self.frame.layout(lines, cols, rows))
    }
}

/// Translate existing styled rows to data. Other control sequences are consumed
/// without being passed to Wove or the terminal. State starts fresh on each row.
fn spans(line: &str) -> Vec<Span> {
    let mut result = Vec::new();
    let mut style = Style::default();
    let mut link = None;
    let mut at = 0;
    while at < line.len() {
        let remaining = &line[at..];
        if !remaining.starts_with('\x1b') {
            let end = remaining.find('\x1b').unwrap_or(remaining.len());
            result.push(Span {
                text: remaining[..end].into(),
                style,
                link: link.clone(),
            });
            at += end;
            continue;
        }
        at += 1;
        let remaining = &line[at..];
        if let Some(csi) = remaining.strip_prefix('[') {
            if let Some(end) = csi.find(|c: char| ('@'..='~').contains(&c)) {
                if csi.as_bytes()[end] == b'm' {
                    sgr(&mut style, &csi[..end]);
                }
                at += end + 2;
            } else {
                break;
            }
        } else if let Some(osc) = remaining.strip_prefix(']') {
            let end = osc
                .find('\x07')
                .map(|n| (n, 1))
                .into_iter()
                .chain(osc.find("\x1b\\").map(|n| (n, 2)))
                .min_by_key(|(n, _)| *n);
            let Some((end, terminator)) = end else {
                break;
            };
            if let Some((_, destination)) = osc[..end]
                .strip_prefix("8;")
                .and_then(|s| s.split_once(';'))
            {
                link = (!destination.is_empty()).then(|| std::sync::Arc::from(destination));
            }
            at += 1 + end + terminator;
        } else if let Some(c) = remaining.chars().next() {
            at += c.len_utf8();
        }
    }
    result
}

/// Fold only e's supported SGR attributes and palette colors into a cell style.
fn sgr(style: &mut Style, parameters: &str) {
    let values: Vec<_> = parameters
        .split(';')
        .map(|p| {
            if p.is_empty() {
                Some(0)
            } else {
                p.parse::<u16>().ok()
            }
        })
        .collect();
    let mut at = 0;
    while at < values.len() {
        match values[at] {
            Some(0) => *style = Style::default(),
            Some(1) => style.bold = true,
            Some(2) => style.dim = true,
            Some(3) => style.italic = true,
            Some(4) => style.underline = true,
            Some(7) => style.reverse = true,
            Some(9) => style.strikethrough = true,
            Some(22) => {
                style.bold = false;
                style.dim = false;
            }
            Some(23) => style.italic = false,
            Some(24) => style.underline = false,
            Some(27) => style.reverse = false,
            Some(29) => style.strikethrough = false,
            Some(39) => style.fg = Color::Default,
            Some(49) => style.bg = Color::Reset,
            Some(code @ (38 | 48)) => {
                let color = match values.get(at + 1).copied().flatten() {
                    Some(5) => {
                        at += 2;
                        values
                            .get(at)
                            .copied()
                            .flatten()
                            .and_then(|n| u8::try_from(n).ok())
                            .map(Color::Indexed)
                    }
                    Some(2) => {
                        at += 4;
                        let rgb = values.get(at.saturating_sub(2)..=at).and_then(|v| {
                            Some((
                                u8::try_from(v[0]?).ok()?,
                                u8::try_from(v[1]?).ok()?,
                                u8::try_from(v[2]?).ok()?,
                            ))
                        });
                        rgb.map(|(r, g, b)| Color::Rgb(r, g, b))
                    }
                    _ => None,
                };
                if let Some(color) = color {
                    if code == 38 {
                        style.fg = color;
                    } else {
                        style.bg = color;
                    }
                }
            }
            _ => {}
        }
        at += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fullscreen_adapter_preserves_palette_links_and_composer_cursor() {
        let mut view = Fullscreen::default();
        let lines = vec!["\x1b[38;5;245m\x1b[1mDim \x1b[22m\x1b]8;id=test;https://example.com\x1b\\\x1b[4mlink\x1b[24m\x1b]8;;\x1b\\\x1b[39m \x1b[7m界\x1b[27m".into()];
        let mut bytes = Vec::new();
        view.paint_to(&lines, 20, 3, &mut bytes).unwrap();
        let frame = view.frame.buffer.as_ref().unwrap();
        assert_eq!(frame.cell(0, 0).unwrap().style().fg, Color::Indexed(245));
        assert!(frame.cell(0, 0).unwrap().style().bold);
        assert!(frame.cell(4, 0).unwrap().style().underline);
        assert_eq!(
            frame.cell(4, 0).unwrap().link(),
            Some("https://example.com")
        );
        assert!(frame.cell(9, 0).unwrap().style().reverse);
        assert_eq!(frame.cell(10, 0).unwrap().symbol(), "");
        bytes.clear();
        view.paint_to(&lines, 20, 3, &mut bytes).unwrap();
        assert!(bytes.is_empty(), "an unchanged view should write nothing");
        view.paint_to(&[], 20, 3, &mut bytes).unwrap();
        assert!(view
            .frame
            .buffer
            .as_ref()
            .unwrap()
            .lines()
            .iter()
            .all(|line| line.trim().is_empty()));
    }

    #[test]
    fn unsupported_terminal_commands_never_enter_the_frame() {
        let parsed = spans("a\x1b[2Jb\x1b]52;c;payload\x07c\x1b[38;2;1;2;3m\x1b[7md");
        assert_eq!(
            parsed.iter().map(|s| s.text.as_str()).collect::<String>(),
            "abcd"
        );
        assert_eq!(parsed.last().unwrap().style.fg, Color::Rgb(1, 2, 3));
        assert!(parsed.last().unwrap().style.reverse);
    }
}
