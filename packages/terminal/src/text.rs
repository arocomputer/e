//! Terminal-column measurement and clipping for styled rows.
use unicode_width::UnicodeWidthChar;
const OSC8_CLOSE: &str = "\x1b]8;;\x1b\\";

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

/// Remove ANSI escape sequences — CSI (colours, cursor moves), OSC (titles,
/// hyperlinks), and two-byte ESC forms — leaving the plain text. Applied to
/// the model-facing capture and the display path alike: neither should pay
/// for (or render) colour codes.
pub fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        match chars.peek() {
            Some('[') => {
                chars.next();
                // CSI: parameter and intermediate bytes, then one final byte.
                for n in chars.by_ref() {
                    if ('\u{40}'..='\u{7e}').contains(&n) {
                        break;
                    }
                }
            }
            Some(']') => {
                chars.next();
                // OSC: runs to BEL or the ESC \ string terminator.
                while let Some(n) = chars.next() {
                    if n == '\u{07}' {
                        break;
                    }
                    if n == '\u{1b}' {
                        if chars.peek() == Some(&'\\') {
                            chars.next();
                        }
                        break;
                    }
                }
            }
            Some(_) => {
                chars.next(); // ESC plus one byte (ESC 7, ESC c, …)
            }
            None => {}
        }
    }
    out
}

/// Remove control sequences before untrusted process output reaches the TUI.
pub fn sanitize_display(text: &str) -> String {
    let mut clean = String::with_capacity(text.len());
    for character in strip_ansi(text).chars() {
        match character {
            '\n' => clean.push('\n'),
            '\t' => clean.push_str("    "),
            character if !character.is_control() => clean.push(character),
            _ => {}
        }
    }
    clean
}
