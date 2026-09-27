//! Decode a terminal's input bytes into [`Event`]s, the way crossterm does
//! on a real terminal: control bytes, `ESC`-prefixed Alt keys, CSI and SS3
//! key sequences (with xterm's modifier parameter), the kitty keyboard
//! protocol's `CSI … u`, SGR mouse reports, focus, and bracketed paste.
//!
//! The browser build feeds it what the page's terminal emulator sends. Input
//! may arrive in pieces, so a sequence cut off mid-way waits for the rest;
//! a lone `ESC` at the end of a piece is the Escape key, because an emulator
//! sends each key press whole.

use super::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use super::{MouseButton, MouseEvent, MouseEventKind};

const PASTE_START: &[u8] = b"\x1b[200~";
const PASTE_END: &[u8] = b"\x1b[201~";
/// An unterminated sequence longer than this is noise, not a slow key.
const MAX_SEQUENCE: usize = 64;

/// Input bytes not yet decoded, and whether a paste is open.
#[derive(Debug, Default)]
pub struct Parser {
    pending: Vec<u8>,
    paste: Option<Vec<u8>>,
}

impl Parser {
    /// Decode `bytes`, together with whatever the last call left undecoded.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<Event> {
        self.pending.extend_from_slice(bytes);
        let mut events = Vec::new();
        loop {
            if let Some(paste) = self.paste.as_mut() {
                match find(&self.pending, PASTE_END) {
                    Some(end) => {
                        paste.extend_from_slice(&self.pending[..end]);
                        self.pending.drain(..end + PASTE_END.len());
                        events.push(Event::Paste(String::from_utf8_lossy(paste).into_owned()));
                        self.paste = None;
                        continue;
                    }
                    None => {
                        // Keep a tail that could be the start of the end
                        // marker; everything before it is pasted text.
                        let keep = PASTE_END.len() - 1;
                        let take = self.pending.len().saturating_sub(keep);
                        paste.extend(self.pending.drain(..take));
                        return events;
                    }
                }
            }
            if self.pending.is_empty() {
                return events;
            }
            if self.pending.starts_with(PASTE_START) {
                self.pending.drain(..PASTE_START.len());
                self.paste = Some(Vec::new());
                continue;
            }
            match decode(&self.pending) {
                Decoded::Event(event, used) => {
                    self.pending.drain(..used);
                    if let Some(event) = event {
                        events.push(event);
                    }
                }
                Decoded::Incomplete if self.pending == b"\x1b" => {
                    self.pending.clear();
                    events.push(key(KeyCode::Esc, KeyModifiers::NONE));
                }
                Decoded::Incomplete if self.pending.len() > MAX_SEQUENCE => {
                    self.pending.clear();
                }
                Decoded::Incomplete => return events,
            }
        }
    }
}

enum Decoded {
    /// An event (or a sequence to ignore) and the bytes it used.
    Event(Option<Event>, usize),
    Incomplete,
}

fn key(code: KeyCode, modifiers: KeyModifiers) -> Event {
    Event::Key(KeyEvent::new(code, modifiers))
}

fn decode(bytes: &[u8]) -> Decoded {
    match bytes[0] {
        0x1b => decode_escape(bytes),
        _ => match decode_plain(bytes) {
            Some((event, used)) => Decoded::Event(Some(event), used),
            None => Decoded::Incomplete,
        },
    }
}

/// One byte or UTF-8 character outside an escape sequence.
fn decode_plain(bytes: &[u8]) -> Option<(Event, usize)> {
    let none = KeyModifiers::NONE;
    let control = KeyModifiers::CONTROL;
    let event = match bytes[0] {
        b'\r' => key(KeyCode::Enter, none),
        b'\n' => key(KeyCode::Char('j'), control),
        b'\t' => key(KeyCode::Tab, none),
        0x7f => key(KeyCode::Backspace, none),
        0x00 => key(KeyCode::Char(' '), control),
        byte @ 0x01..=0x1a => key(KeyCode::Char((byte - 1 + b'a') as char), control),
        byte @ 0x1c..=0x1f => key(KeyCode::Char((byte - 0x1c + b'4') as char), control),
        _ => {
            let (c, used) = utf8_char(bytes)?;
            let modifiers = if c.is_uppercase() {
                KeyModifiers::SHIFT
            } else {
                none
            };
            return Some((key(KeyCode::Char(c), modifiers), used));
        }
    };
    Some((event, 1))
}

/// The first character and its length; `None` while it is cut off. An
/// invalid byte decodes as U+FFFD rather than stalling the stream.
fn utf8_char(bytes: &[u8]) -> Option<(char, usize)> {
    let width = match bytes[0] {
        0x00..=0x7f => 1,
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf7 => 4,
        _ => return Some((char::REPLACEMENT_CHARACTER, 1)),
    };
    if bytes.len() < width {
        return None;
    }
    match std::str::from_utf8(&bytes[..width]) {
        Ok(text) => text.chars().next().map(|c| (c, width)),
        Err(_) => Some((char::REPLACEMENT_CHARACTER, 1)),
    }
}

fn decode_escape(bytes: &[u8]) -> Decoded {
    let Some(&next) = bytes.get(1) else {
        return Decoded::Incomplete;
    };
    match next {
        b'[' => decode_csi(bytes),
        b'O' => match bytes.get(2) {
            None => Decoded::Incomplete,
            Some(&last) => Decoded::Event(ss3(last, KeyModifiers::NONE), 3),
        },
        0x1b => Decoded::Event(Some(key(KeyCode::Esc, KeyModifiers::ALT)), 2),
        _ => match decode_plain(&bytes[1..]) {
            // ESC before a key is Alt held with it.
            Some((Event::Key(mut event), used)) => {
                event.modifiers.insert(KeyModifiers::ALT);
                Decoded::Event(Some(Event::Key(event)), used + 1)
            }
            Some((_, used)) => Decoded::Event(None, used + 1),
            None => Decoded::Incomplete,
        },
    }
}

/// `ESC O x`: F1–F4, and the cursor keys in application mode.
fn ss3(last: u8, modifiers: KeyModifiers) -> Option<Event> {
    let code = match last {
        b'P' => KeyCode::F(1),
        b'Q' => KeyCode::F(2),
        b'R' => KeyCode::F(3),
        b'S' => KeyCode::F(4),
        other => cursor_key(other)?,
    };
    Some(key(code, modifiers))
}

fn cursor_key(last: u8) -> Option<KeyCode> {
    Some(match last {
        b'A' => KeyCode::Up,
        b'B' => KeyCode::Down,
        b'C' => KeyCode::Right,
        b'D' => KeyCode::Left,
        b'H' => KeyCode::Home,
        b'F' => KeyCode::End,
        _ => return None,
    })
}

/// `ESC [ params final`. Parameters are digits, `;`, `:`, and a leading
/// `<`, `?`, or `>`; the final byte is in `@`–`~`.
fn decode_csi(bytes: &[u8]) -> Decoded {
    let Some(end) = bytes[2..].iter().position(|b| (0x40..=0x7e).contains(b)) else {
        return Decoded::Incomplete;
    };
    let end = end + 2;
    let used = end + 1;
    let params = String::from_utf8_lossy(&bytes[2..end]);
    let last = bytes[end];
    if let Some(mouse) = params.strip_prefix('<') {
        return Decoded::Event(sgr_mouse(mouse, last), used);
    }
    if params.starts_with(['?', '>']) {
        return Decoded::Event(None, used);
    }
    let fields: Vec<&str> = params.split(';').collect();
    let number = |i: usize| -> Option<u32> {
        fields
            .get(i)
            .and_then(|field| field.split(':').next())
            .filter(|field| !field.is_empty())
            .and_then(|field| field.parse().ok())
    };
    let modifiers = modifier_param(number(1));
    let event = match last {
        b'Z' => Some(key(KeyCode::BackTab, KeyModifiers::SHIFT | modifiers)),
        b'I' => Some(Event::FocusGained),
        b'O' => Some(Event::FocusLost),
        b'P' | b'Q' | b'R' | b'S' => ss3(last, modifiers),
        b'~' => tilde_key(number(0).unwrap_or(0)).map(|code| key(code, modifiers)),
        b'u' => number(0).map(|code| kitty_key(code, modifiers, number_kind(&fields))),
        other => cursor_key(other).map(|code| key(code, modifiers)),
    };
    Decoded::Event(event, used)
}

/// xterm's modifier parameter: one more than a bit set of shift, alt,
/// control, super, hyper, and meta.
fn modifier_param(value: Option<u32>) -> KeyModifiers {
    let bits = value.unwrap_or(1).saturating_sub(1);
    let mut modifiers = KeyModifiers::NONE;
    for (bit, modifier) in [
        (1, KeyModifiers::SHIFT),
        (2, KeyModifiers::ALT),
        (4, KeyModifiers::CONTROL),
        (8, KeyModifiers::SUPER),
        (16, KeyModifiers::HYPER),
        (32, KeyModifiers::META),
    ] {
        if bits & bit != 0 {
            modifiers.insert(modifier);
        }
    }
    modifiers
}

fn tilde_key(number: u32) -> Option<KeyCode> {
    Some(match number {
        1 | 7 => KeyCode::Home,
        2 => KeyCode::Insert,
        3 => KeyCode::Delete,
        4 | 8 => KeyCode::End,
        5 => KeyCode::PageUp,
        6 => KeyCode::PageDown,
        11..=15 => KeyCode::F((number - 10) as u8),
        17..=21 => KeyCode::F((number - 11) as u8),
        23..=26 => KeyCode::F((number - 12) as u8),
        28 | 29 => KeyCode::F((number - 15) as u8),
        31..=34 => KeyCode::F((number - 17) as u8),
        _ => return None,
    })
}

/// The kitty protocol's event type, the second subfield of the modifiers.
fn number_kind(fields: &[&str]) -> KeyEventKind {
    match fields.get(1).and_then(|field| field.split(':').nth(1)) {
        Some("2") => KeyEventKind::Repeat,
        Some("3") => KeyEventKind::Release,
        _ => KeyEventKind::Press,
    }
}

/// `CSI code ; modifiers u`: a key the legacy encoding cannot tell apart,
/// such as shift+enter.
fn kitty_key(code: u32, modifiers: KeyModifiers, kind: KeyEventKind) -> Event {
    let code = match code {
        13 => KeyCode::Enter,
        27 => KeyCode::Esc,
        9 if modifiers.contains(KeyModifiers::SHIFT) => KeyCode::BackTab,
        9 => KeyCode::Tab,
        127 | 8 => KeyCode::Backspace,
        other => char::from_u32(other).map_or(KeyCode::Other, KeyCode::Char),
    };
    Event::Key(KeyEvent {
        code,
        modifiers,
        kind,
    })
}

/// `CSI < button ; column ; row M` (press, motion) or `m` (release).
fn sgr_mouse(params: &str, last: u8) -> Option<Event> {
    let mut fields = params.split(';').map(|field| field.parse::<u16>().ok());
    let (Some(Some(code)), Some(Some(column)), Some(Some(row))) =
        (fields.next(), fields.next(), fields.next())
    else {
        return None;
    };
    let mut modifiers = KeyModifiers::NONE;
    if code & 4 != 0 {
        modifiers.insert(KeyModifiers::SHIFT);
    }
    if code & 8 != 0 {
        modifiers.insert(KeyModifiers::ALT);
    }
    if code & 16 != 0 {
        modifiers.insert(KeyModifiers::CONTROL);
    }
    let button = match code & 3 {
        0 => Some(MouseButton::Left),
        1 => Some(MouseButton::Middle),
        2 => Some(MouseButton::Right),
        _ => None,
    };
    let kind = if code & 64 != 0 {
        match code & 3 {
            0 => MouseEventKind::ScrollUp,
            1 => MouseEventKind::ScrollDown,
            2 => MouseEventKind::ScrollLeft,
            _ => MouseEventKind::ScrollRight,
        }
    } else if code & 32 != 0 {
        match button {
            Some(button) => MouseEventKind::Drag(button),
            None => MouseEventKind::Moved,
        }
    } else if last == b'm' {
        MouseEventKind::Up(button.unwrap_or(MouseButton::Left))
    } else {
        MouseEventKind::Down(button?)
    };
    Some(Event::Mouse(MouseEvent {
        kind,
        column: column.saturating_sub(1),
        row: row.saturating_sub(1),
        modifiers,
    }))
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(input: &[u8]) -> Vec<(KeyCode, KeyModifiers)> {
        Parser::default()
            .feed(input)
            .into_iter()
            .map(|event| match event {
                Event::Key(key) => (key.code, key.modifiers),
                other => panic!("expected a key, got {other:?}"),
            })
            .collect()
    }

    #[test]
    fn control_bytes_decode_like_crossterm() {
        use KeyCode::*;
        let ctrl = KeyModifiers::CONTROL;
        assert_eq!(
            keys(b"\r\n\t\x7f\x03"),
            vec![
                (Enter, KeyModifiers::NONE),
                (Char('j'), ctrl),
                (Tab, KeyModifiers::NONE),
                (Backspace, KeyModifiers::NONE),
                (Char('c'), ctrl),
            ]
        );
    }

    #[test]
    fn text_keeps_its_characters_and_shift() {
        assert_eq!(
            keys("aZé".as_bytes()),
            vec![
                (KeyCode::Char('a'), KeyModifiers::NONE),
                (KeyCode::Char('Z'), KeyModifiers::SHIFT),
                (KeyCode::Char('é'), KeyModifiers::NONE),
            ]
        );
    }

    #[test]
    fn escape_alone_and_escape_as_alt() {
        assert_eq!(keys(b"\x1b"), vec![(KeyCode::Esc, KeyModifiers::NONE)]);
        assert_eq!(
            keys(b"\x1b\r\x1bb"),
            vec![
                (KeyCode::Enter, KeyModifiers::ALT),
                (KeyCode::Char('b'), KeyModifiers::ALT),
            ]
        );
    }

    #[test]
    fn cursor_and_editing_keys_carry_their_modifiers() {
        use KeyCode::*;
        assert_eq!(
            keys(b"\x1b[A\x1b[1;5D\x1b[3~\x1b[5;2~\x1b[Z\x1bOH"),
            vec![
                (Up, KeyModifiers::NONE),
                (Left, KeyModifiers::CONTROL),
                (Delete, KeyModifiers::NONE),
                (PageUp, KeyModifiers::SHIFT),
                (BackTab, KeyModifiers::SHIFT),
                (Home, KeyModifiers::NONE),
            ]
        );
    }

    #[test]
    fn kitty_keys_tell_shift_enter_from_enter() {
        assert_eq!(
            keys(b"\x1b[13;2u\x1b[13u"),
            vec![
                (KeyCode::Enter, KeyModifiers::SHIFT),
                (KeyCode::Enter, KeyModifiers::NONE),
            ]
        );
    }

    #[test]
    fn sgr_mouse_reports_decode_to_cells() {
        let events = Parser::default().feed(b"\x1b[<64;10;5M\x1b[<0;3;4M\x1b[<32;4;4M");
        let kinds: Vec<_> = events
            .iter()
            .map(|event| match event {
                Event::Mouse(mouse) => (mouse.kind, mouse.column, mouse.row),
                other => panic!("expected mouse, got {other:?}"),
            })
            .collect();
        assert_eq!(
            kinds,
            vec![
                (MouseEventKind::ScrollUp, 9, 4),
                (MouseEventKind::Down(MouseButton::Left), 2, 3),
                (MouseEventKind::Drag(MouseButton::Left), 3, 3),
            ]
        );
    }

    #[test]
    fn a_paste_arrives_whole_even_across_pieces() {
        let mut parser = Parser::default();
        assert!(parser.feed(b"\x1b[200~line one\r\nline").is_empty());
        assert!(parser.feed(b" two\x1b[20").is_empty());
        assert_eq!(
            parser.feed(b"1~x"),
            vec![
                Event::Paste("line one\r\nline two".into()),
                key(KeyCode::Char('x'), KeyModifiers::NONE),
            ]
        );
    }

    #[test]
    fn a_sequence_cut_mid_way_waits_for_the_rest() {
        let mut parser = Parser::default();
        assert!(parser.feed(b"\x1b[1;5").is_empty());
        assert_eq!(
            parser.feed(b"C"),
            vec![key(KeyCode::Right, KeyModifiers::CONTROL)]
        );
        assert!(parser.feed(&"é".as_bytes()[..1]).is_empty());
        assert_eq!(
            parser.feed(&"é".as_bytes()[1..]),
            vec![key(KeyCode::Char('é'), KeyModifiers::NONE)]
        );
    }
}
