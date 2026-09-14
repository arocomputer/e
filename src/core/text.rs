//! Control-sequence stripping for untrusted text: what a process printed,
//! what a model streamed, what an extension sent. Terminal-free, so the core
//! and the frontend sanitize the same way.

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
