//! A terminal emulator in a web page. The embedding installs the sink that
//! carries output to the emulator ([`install`]), then forwards what the
//! emulator reports: typed bytes ([`feed`]), size changes ([`resize`]), and
//! the end of the page's session ([`close`]). Everything runs on the page's
//! one thread.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use tokio::sync::mpsc::{Receiver, Sender};

use super::{vt, Event};

/// The modes the frame loop reads input through: bracketed paste, click and
/// drag reports in SGR coordinates, and the kitty keyboard protocol's
/// disambiguated keys. Plain motion reports stay off; the app has no use
/// for them and each would cost a frame.
const ENTER_MODES: &[u8] = b"\x1b[?2004h\x1b[?1000h\x1b[?1002h\x1b[?1006h\x1b[>1u";
const LEAVE_MODES: &[u8] = b"\x1b[<1u\x1b[?1006l\x1b[?1002l\x1b[?1000l\x1b[?2004l\r\n\x1b[?25h";

/// Where decoded input goes: the frame loop's receiver.
type Input = Sender<std::io::Result<Event>>;
/// Where e's output goes: the page's terminal.
type Output = Rc<dyn Fn(&[u8])>;

struct Page {
    output: Output,
    size: (u16, u16),
    light: bool,
    parser: vt::Parser,
    input: Option<Input>,
}

thread_local! {
    static PAGE: RefCell<Option<Page>> = const { RefCell::new(None) };
}

/// Connect the page's terminal: `output` receives every byte e writes, and
/// the size and background shape the first frame.
pub fn install(output: impl Fn(&[u8]) + 'static, cols: u16, rows: u16, light: bool) {
    PAGE.set(Some(Page {
        output: Rc::new(output),
        size: (cols, rows),
        light,
        parser: vt::Parser::default(),
        input: None,
    }));
}

/// Bytes the user typed or pasted into the page's terminal.
pub fn feed(bytes: &[u8]) {
    let (events, input) = PAGE.with_borrow_mut(|page| match page {
        Some(page) => (page.parser.feed(bytes), page.input.clone()),
        None => (Vec::new(), None),
    });
    if let Some(input) = input {
        for event in events {
            let _ = input.try_send(Ok(event));
        }
    }
}

/// The page's terminal changed size.
pub fn resize(cols: u16, rows: u16) {
    let input = PAGE.with_borrow_mut(|page| {
        page.as_mut().and_then(|page| {
            page.size = (cols, rows);
            page.input.clone()
        })
    });
    if let Some(input) = input {
        let _ = input.try_send(Ok(Event::Resize(cols, rows)));
    }
}

/// The page is done with the session: input ends, and the frame loop with it.
pub fn close() {
    PAGE.with_borrow_mut(|page| {
        if let Some(page) = page {
            page.input = None;
        }
    });
}

/// Write to the page's terminal. Nothing is installed: nothing to write to.
pub fn write(bytes: &[u8]) -> std::io::Result<()> {
    let output = PAGE.with_borrow(|page| page.as_ref().map(|page| page.output.clone()));
    match output {
        Some(output) => {
            output(bytes);
            Ok(())
        }
        None => Err(std::io::Error::new(
            std::io::ErrorKind::NotConnected,
            "no terminal installed",
        )),
    }
}

/// Columns and rows.
pub fn size() -> std::io::Result<(u16, u16)> {
    PAGE.with_borrow(|page| page.as_ref().map(|page| page.size))
        .ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotConnected, "no terminal installed")
        })
}

/// The page shows the terminal, so escape codes always land.
pub fn is_terminal() -> bool {
    true
}

/// The background the page chose.
pub fn light_background() -> Option<bool> {
    PAGE.with_borrow(|page| page.as_ref().map(|page| page.light))
}

/// The page's terminal starts empty: the frame begins at the top.
pub fn launch_row(_rows: u16) -> usize {
    0
}

/// Turn on the input modes; the guard turns them off.
pub fn enter() -> std::io::Result<Guard> {
    write(ENTER_MODES)?;
    Ok(Guard)
}

/// Restores the page's terminal when the session ends.
pub struct Guard;

impl Drop for Guard {
    fn drop(&mut self) {
        let _ = write(LEAVE_MODES);
    }
}

/// The page's input as events. Nothing pauses it: there is no external
/// editor to hand the terminal to.
pub fn input(_paused: Arc<AtomicBool>) -> Receiver<std::io::Result<Event>> {
    let (sender, receiver) = tokio::sync::mpsc::channel(1024);
    PAGE.with_borrow_mut(|page| {
        if let Some(page) = page {
            page.input = Some(sender);
        }
    });
    receiver
}

/// A page sends no signals; the session ends through input instead.
pub struct Signals;

impl Signals {
    pub fn new() -> std::io::Result<Self> {
        Ok(Signals)
    }

    pub async fn recv(&mut self) {
        std::future::pending::<()>().await;
    }
}
