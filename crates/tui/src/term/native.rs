//! A real terminal, through crossterm: raw mode and input modes, the input
//! thread, launch probes, and the signals that end a session.

use std::io::Write as _;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::{execute, terminal};
use tokio::sync::mpsc::Receiver;

use super::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use super::{MouseButton, MouseEvent, MouseEventKind};

/// Write to the terminal and flush.
pub fn write(bytes: &[u8]) -> std::io::Result<()> {
    let mut out = std::io::stdout().lock();
    out.write_all(bytes)?;
    out.flush()
}

/// Columns and rows.
pub fn size() -> std::io::Result<(u16, u16)> {
    terminal::size()
}

/// Whether a person is watching: escape codes into a pipe are garbage.
pub fn is_terminal() -> bool {
    crate::background::stdout_is_tty()
}

/// Whether the terminal's background is light, when it says.
pub fn light_background() -> Option<bool> {
    crate::background::detect_light()
}

/// The row the frame starts on: below where the user launched e, never over
/// what came before. A terminal that doesn't answer DSR 6n — a raw pty —
/// falls back to the screen's bottom row, the common launch spot.
pub fn launch_row(rows: u16) -> usize {
    crate::background::query_cursor_row(rows).unwrap_or(rows.saturating_sub(1)) as usize
}

/// Take the terminal: raw mode first, so the frame loop owns input, then the
/// input modes. The guard restores both on every exit path, `?` returns and
/// unwinds included. A panic mid-frame must not strand the shell in raw mode
/// either, so the panic hook restores it too.
pub fn enter() -> std::io::Result<Guard> {
    install_panic_hook();
    terminal::enable_raw_mode()?;
    let guard = Guard;
    push_modes()?;
    Ok(guard)
}

/// Restores every terminal mode the TUI enables — keyboard enhancement
/// flags, bracketed paste, raw mode, cursor visibility. Popping a mode that
/// never got enabled is harmless; leaving one enabled corrupts the user's
/// shell.
pub struct Guard;

impl Drop for Guard {
    fn drop(&mut self) {
        let _ = pop_modes();
        let _ = terminal::disable_raw_mode();
        let _ = write(b"\r\n\x1b[?25h");
    }
}

impl Guard {
    /// Hand the terminal to another program (the external editor): input
    /// modes off, raw mode off, cursor shown.
    pub fn suspend(&self) {
        let _ = pop_modes();
        let _ = terminal::disable_raw_mode();
        let _ = write(b"\x1b[?25h");
    }

    /// Take the terminal back after [`Guard::suspend`].
    pub fn resume(&self) {
        let _ = terminal::enable_raw_mode();
        let _ = push_modes();
    }
}

/// Bracketed paste, mouse capture, and the kitty keyboard protocol —
/// without it, terminals send plain Enter for shift+enter and multi-line
/// entry is unreachable.
fn push_modes() -> std::io::Result<()> {
    execute!(
        std::io::stdout(),
        EnableBracketedPaste,
        EnableMouseCapture,
        PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
    )
}

fn pop_modes() -> std::io::Result<()> {
    execute!(
        std::io::stdout(),
        PopKeyboardEnhancementFlags,
        DisableBracketedPaste,
        DisableMouseCapture
    )
}

/// Restore the terminal before reporting a panic. Only a panic on this
/// thread — the frame loop, driven by the runtime's block_on — is fatal to
/// the session; the paint thread, tool tasks and the turn worker all run
/// elsewhere and catch their own panics to keep the session alive, so the
/// hook must leave the terminal alone for them (the hook fires before any
/// catch_unwind gets its say). `\x1b[<u` pops the keyboard enhancement stack.
fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    let frame_thread = std::thread::current().id();
    std::panic::set_hook(Box::new(move |info| {
        if std::thread::current().id() == frame_thread {
            let _ = terminal::disable_raw_mode();
            let _ = write(b"\x1b[<u\x1b[?2004l\x1b[?25h\r\n");
        }
        default_hook(info);
    }));
}

/// Read terminal events on a thread the frame loop can pause. Each poll
/// waits at most 100 ms, so a pause takes effect within that; the thread
/// ends when the receiver is dropped.
pub fn input(paused: Arc<AtomicBool>) -> Receiver<std::io::Result<Event>> {
    let (tx, rx) = tokio::sync::mpsc::channel(64);
    std::thread::spawn(move || loop {
        if paused.load(Ordering::SeqCst) {
            std::thread::sleep(Duration::from_millis(50));
            continue;
        }
        match crossterm::event::poll(Duration::from_millis(100)) {
            Ok(true) => {
                let event = crossterm::event::read().map(Event::from);
                if tx.blocking_send(event).is_err() {
                    break;
                }
            }
            Ok(false) => {}
            Err(error) => {
                let _ = tx.blocking_send(Err(error));
                break;
            }
        }
    });
    rx
}

/// SIGTERM and SIGHUP — a kill, a closed tab — end the session through the
/// same cleanup as /quit.
pub struct Signals {
    terminate: tokio::signal::unix::Signal,
    hangup: tokio::signal::unix::Signal,
}

impl Signals {
    pub fn new() -> std::io::Result<Self> {
        use tokio::signal::unix::{signal, SignalKind};
        Ok(Signals {
            terminate: signal(SignalKind::terminate())?,
            hangup: signal(SignalKind::hangup())?,
        })
    }

    /// Resolve when either arrives.
    pub async fn recv(&mut self) {
        tokio::select! {
            _ = self.terminate.recv() => {}
            _ = self.hangup.recv() => {}
        }
    }
}

impl From<crossterm::event::Event> for Event {
    fn from(event: crossterm::event::Event) -> Self {
        use crossterm::event::Event as Crossterm;
        match event {
            Crossterm::Key(key) => Event::Key(KeyEvent {
                code: key.code.into(),
                modifiers: key.modifiers.into(),
                kind: match key.kind {
                    crossterm::event::KeyEventKind::Press => KeyEventKind::Press,
                    crossterm::event::KeyEventKind::Repeat => KeyEventKind::Repeat,
                    crossterm::event::KeyEventKind::Release => KeyEventKind::Release,
                },
            }),
            Crossterm::Mouse(mouse) => Event::Mouse(MouseEvent {
                kind: match mouse.kind {
                    crossterm::event::MouseEventKind::Down(button) => {
                        MouseEventKind::Down(button.into())
                    }
                    crossterm::event::MouseEventKind::Up(button) => {
                        MouseEventKind::Up(button.into())
                    }
                    crossterm::event::MouseEventKind::Drag(button) => {
                        MouseEventKind::Drag(button.into())
                    }
                    crossterm::event::MouseEventKind::Moved => MouseEventKind::Moved,
                    crossterm::event::MouseEventKind::ScrollDown => MouseEventKind::ScrollDown,
                    crossterm::event::MouseEventKind::ScrollUp => MouseEventKind::ScrollUp,
                    crossterm::event::MouseEventKind::ScrollLeft => MouseEventKind::ScrollLeft,
                    crossterm::event::MouseEventKind::ScrollRight => MouseEventKind::ScrollRight,
                },
                column: mouse.column,
                row: mouse.row,
                modifiers: mouse.modifiers.into(),
            }),
            Crossterm::Paste(text) => Event::Paste(text),
            Crossterm::Resize(cols, rows) => Event::Resize(cols, rows),
            Crossterm::FocusGained => Event::FocusGained,
            Crossterm::FocusLost => Event::FocusLost,
        }
    }
}

impl From<crossterm::event::KeyCode> for KeyCode {
    fn from(code: crossterm::event::KeyCode) -> Self {
        use crossterm::event::KeyCode as Crossterm;
        match code {
            Crossterm::Backspace => KeyCode::Backspace,
            Crossterm::Enter => KeyCode::Enter,
            Crossterm::Left => KeyCode::Left,
            Crossterm::Right => KeyCode::Right,
            Crossterm::Up => KeyCode::Up,
            Crossterm::Down => KeyCode::Down,
            Crossterm::Home => KeyCode::Home,
            Crossterm::End => KeyCode::End,
            Crossterm::PageUp => KeyCode::PageUp,
            Crossterm::PageDown => KeyCode::PageDown,
            Crossterm::Tab => KeyCode::Tab,
            Crossterm::BackTab => KeyCode::BackTab,
            Crossterm::Delete => KeyCode::Delete,
            Crossterm::Insert => KeyCode::Insert,
            Crossterm::F(n) => KeyCode::F(n),
            Crossterm::Char(c) => KeyCode::Char(c),
            Crossterm::Esc => KeyCode::Esc,
            _ => KeyCode::Other,
        }
    }
}

impl From<crossterm::event::KeyModifiers> for KeyModifiers {
    fn from(held: crossterm::event::KeyModifiers) -> Self {
        use crossterm::event::KeyModifiers as Crossterm;
        let mut modifiers = KeyModifiers::NONE;
        for (theirs, ours) in [
            (Crossterm::SHIFT, KeyModifiers::SHIFT),
            (Crossterm::ALT, KeyModifiers::ALT),
            (Crossterm::CONTROL, KeyModifiers::CONTROL),
            (Crossterm::SUPER, KeyModifiers::SUPER),
            (Crossterm::HYPER, KeyModifiers::HYPER),
            (Crossterm::META, KeyModifiers::META),
        ] {
            if held.contains(theirs) {
                modifiers.insert(ours);
            }
        }
        modifiers
    }
}

impl From<crossterm::event::MouseButton> for MouseButton {
    fn from(button: crossterm::event::MouseButton) -> Self {
        match button {
            crossterm::event::MouseButton::Left => MouseButton::Left,
            crossterm::event::MouseButton::Right => MouseButton::Right,
            crossterm::event::MouseButton::Middle => MouseButton::Middle,
        }
    }
}
