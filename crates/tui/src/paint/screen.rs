//! Bounded inline frames over Wove's renderer. e supplies the logical transcript;
//! rows that leave the screen are released to native terminal history. Growth
//! flows in batches, while resize and collapse repaint only the visible tail.
use std::io::{self, Write};
use wove::{Depth, Inline};

/// Owns e's logical row offset; Wove owns cells, clipping, diffing, and output.
pub struct Screen {
    inline: Inline,
    frame: super::wove::Frame,
    /// Logical row zero of the bounded frame. Earlier rows belong to history.
    base: usize,
    /// Only the reachable suffix is retained for the unchanged-frame fast path.
    prev: Vec<String>,
    len: usize,
    redraw_pending: bool,
    pub cols: u16,
    pub rows: u16,
    debug_frames: bool,
}

impl Screen {
    /// Start below the launch cursor; terminal dimensions are at least one.
    pub fn new(cols: u16, rows: u16, anchor: usize) -> Self {
        let (cols, rows) = (cols.max(1), rows.max(1));
        Self {
            inline: Inline::new(anchor.min(usize::from(rows - 1)) as u16, Depth::Rgb),
            frame: super::wove::Frame::default(),
            base: 0,
            prev: Vec::new(),
            len: 0,
            redraw_pending: false,
            cols,
            rows,
            debug_frames: std::env::var("E_DEBUG_FRAMES").is_ok(),
        }
    }

    /// Repaint only the new visible tail after physical positions become unknown.
    pub fn resize(&mut self, cols: u16, rows: u16) {
        self.cols = cols.max(1);
        self.rows = rows.max(1);
        self.redraw_pending = true;
    }

    /// Transfer a logical frame to bounded cell frames and release rows into
    /// native history. Every appended row is drawn before it can scroll away.
    fn paint_to(&mut self, lines: Vec<String>, out: &mut impl Write) -> io::Result<()> {
        let rows = usize::from(self.rows);
        let len = lines.len();
        if !self.redraw_pending && len == self.len && self.prev == lines[self.base..] {
            return Ok(());
        }
        if self.debug_frames {
            log_frame(&lines);
        }
        if self.redraw_pending || len < self.len && (len <= self.base || len >= rows) {
            self.inline = Inline::new(0, Depth::Rgb);
            self.inline.invalidate();
            self.base = len.saturating_sub(rows);
        }
        // The small batch keeps cell memory independent of session length and
        // leaves room for the still-visible rows from the preceding batch.
        let batch = rows.saturating_add(256).min(usize::from(u16::MAX));
        loop {
            let end = len.min(self.base.saturating_add(batch));
            let frame =
                self.frame
                    .layout(&lines[self.base..end], self.cols, (end - self.base) as u16)?;
            if let Err(error) = self.inline.draw(out, frame, self.rows) {
                self.redraw_pending = true;
                return Err(error);
            }
            if let Some(offscreen) = self.inline.frame_row(0) {
                // At the maximum buffer height, make room for the next row.
                // Wove keeps this released row visible until that draw scrolls it.
                let release = if offscreen == 0 && end < len {
                    1
                } else {
                    offscreen
                };
                self.inline.commit(release);
                self.base += usize::from(release);
            }
            if end == len {
                break;
            }
        }
        out.flush().inspect_err(|_| self.redraw_pending = true)?;
        self.prev = lines[self.base..].to_vec();
        self.len = len;
        self.redraw_pending = false;
        Ok(())
    }
}

/// `E_DEBUG_FRAMES`: append each painted frame's rows to /tmp/e-frames.log.
fn log_frame(lines: &[String]) {
    let f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("/tmp/e-frames.log");
    if let Ok(mut f) = f {
        let _ = writeln!(f, "== frame {} rows ==", lines.len());
        for l in lines {
            let _ = writeln!(f, "{:?}", l);
        }
    }
}

/// One sequenced frame in the paint thread's single-slot mailbox.
#[cfg(not(target_family = "wasm"))]
struct PendingFrame {
    sequence: u64,
    posted_at: e_core::rt::Instant,
    lines: Vec<String>,
    alternate: bool,
}

/// Paint progress as observed by the app loop. "Completed" means stdout
/// accepted and flushed the write; terminals do not acknowledge display.
#[derive(Clone, Debug)]
pub struct PaintStatus {
    pub posted: u64,
    pub completed: u64,
    pub pending_since: Option<e_core::rt::Instant>,
    pub failure: Option<(u64, String)>,
    pub stopped: bool,
}

impl PaintStatus {
    pub fn delayed(&self, threshold: std::time::Duration) -> bool {
        self.pending_since
            .is_some_and(|since| since.elapsed() >= threshold)
    }
}

/// The paint thread's single-slot mailbox: a newer frame replaces the
/// undelivered one, so a terminal blocked mid-write bounds the backlog to
/// exactly one pending frame — an unbounded queue would grow by a full
/// transcript copy per tick for as long as the write stalls.
#[cfg(not(target_family = "wasm"))]
#[derive(Default)]
struct PaintMailbox {
    frame: Option<PendingFrame>,
    /// Applied before the next frame, which must use these dimensions.
    /// Resize discards a queued frame at the previous width.
    resize: Option<(u16, u16)>,
    posted: u64,
    completed: u64,
    pending_since: Option<e_core::rt::Instant>,
    failure: Option<(u64, String)>,
    stopped: bool,
    shutdown: bool,
}

#[cfg(not(target_family = "wasm"))]
impl PaintMailbox {
    fn status(&self) -> PaintStatus {
        PaintStatus {
            posted: self.posted,
            completed: self.completed,
            pending_since: self.pending_since,
            failure: self.failure.clone(),
            stopped: self.stopped,
        }
    }

    fn complete(&mut self, sequence: u64) {
        self.completed = self.completed.max(sequence);
        self.failure = None;
        self.pending_since = if self.completed >= self.posted {
            None
        } else {
            self.frame.as_ref().map(|frame| frame.posted_at)
        };
    }

    fn fail(&mut self, sequence: u64, error: String) {
        self.failure = Some((sequence, error));
        self.pending_since
            .get_or_insert_with(e_core::rt::Instant::now);
    }
}

/// Marks an unexpected painter exit even when it unwinds outside `paint()`.
#[cfg(not(target_family = "wasm"))]
struct PaintThreadGuard {
    mailbox: std::sync::Arc<(std::sync::Mutex<PaintMailbox>, std::sync::Condvar)>,
}

#[cfg(not(target_family = "wasm"))]
impl Drop for PaintThreadGuard {
    fn drop(&mut self) {
        let (lock, wake) = &*self.mailbox;
        let mut mailbox = lock.lock().unwrap_or_else(|e| e.into_inner());
        mailbox.stopped = true;
        if !mailbox.shutdown && mailbox.failure.is_none() {
            let sequence = mailbox.posted;
            mailbox.fail(sequence, "paint worker stopped unexpectedly".into());
        }
        wake.notify_all();
    }
}

const ENTER_ALTERNATE: &[u8] = b"\x1b[?1049h";
const LEAVE_ALTERNATE: &[u8] = b"\x1b[?1049l";

/// Preserve inline history while Wove paints fullscreen conversation and review views.
/// Dropping the paint worker restores the main buffer even after a failed frame.
#[derive(Default)]
struct ReviewScreen {
    main: Option<Screen>,
    fullscreen: super::wove::Fullscreen,
}

impl ReviewScreen {
    fn switch(
        &mut self,
        screen: &mut Screen,
        alternate: bool,
        out: &mut impl Write,
    ) -> io::Result<()> {
        if alternate && self.main.is_none() {
            out.write_all(ENTER_ALTERNATE)?;
            out.flush()?;
            self.fullscreen.invalidate();
            let review = Screen::new(screen.cols, screen.rows, 0);
            self.main = Some(std::mem::replace(screen, review));
        } else if !alternate && self.main.is_some() {
            out.write_all(LEAVE_ALTERNATE)?;
            out.flush()?;
            if let Some(main) = self.main.take() {
                *screen = main;
                // The restored buffer's contents belong to the terminal, not
                // to this process's knowledge: a no-op frame must not skip
                // the repaint that proves what is now visible. Same contract
                // as resize and a failed write.
                screen.redraw_pending = true;
            }
        }
        Ok(())
    }

    /// Paint fullscreen views through Wove while retaining the inline history.
    fn paint_to(
        &mut self,
        screen: &mut Screen,
        lines: Vec<String>,
        out: &mut impl Write,
    ) -> io::Result<()> {
        if self.main.is_some() {
            if screen.debug_frames {
                log_frame(&lines);
            }
            self.fullscreen
                .paint_to(&lines, screen.cols, screen.rows, out)
        } else {
            screen.paint_to(lines, out)
        }
    }

    fn resize(&mut self, screen: &mut Screen, cols: u16, rows: u16) {
        self.fullscreen.invalidate();
        if let Some(main) = self.main.as_mut() {
            main.resize(cols, rows);
            *screen = Screen::new(cols, rows, 0);
        } else {
            screen.resize(cols, rows);
        }
    }
}

impl Drop for ReviewScreen {
    fn drop(&mut self) {
        if self.main.is_some() {
            let _ = crate::term::write(LEAVE_ALTERNATE);
        }
    }
}

/// The paint thread: owns the `Screen` and its blocking stdout writes so a
/// slow terminal can never stall the event loop. `anchor` is the launch
/// cursor row — the frame paints below it, never over what came before.
#[cfg(not(target_family = "wasm"))]
pub struct Painter {
    mailbox: std::sync::Arc<(std::sync::Mutex<PaintMailbox>, std::sync::Condvar)>,
    thread: Option<std::thread::JoinHandle<()>>,
    next_sequence: u64,
}

#[cfg(not(target_family = "wasm"))]
impl Painter {
    pub fn spawn(cols: u16, rows: u16, anchor: usize) -> Self {
        let mailbox = std::sync::Arc::new((
            std::sync::Mutex::new(PaintMailbox::default()),
            std::sync::Condvar::new(),
        ));
        let shared = mailbox.clone();
        let thread = std::thread::spawn(move || {
            let _guard = PaintThreadGuard {
                mailbox: shared.clone(),
            };
            let mut screen = Screen::new(cols, rows, anchor);
            let mut review = ReviewScreen::default();
            let (lock, wake) = &*shared;
            loop {
                let (frame, resize, shutdown) = {
                    let mut box_ = lock.lock().unwrap_or_else(|e| e.into_inner());
                    while box_.frame.is_none() && box_.resize.is_none() && !box_.shutdown {
                        box_ = wake.wait(box_).unwrap_or_else(|e| e.into_inner());
                    }
                    (box_.frame.take(), box_.resize.take(), box_.shutdown)
                };
                if let Some((cols, rows)) = resize {
                    review.resize(&mut screen, cols, rows);
                }
                // A panic in the paint path must cost one garbled frame —
                // not the session. The screen is marked unknown so the next
                // frame repaints everything over whatever the panicking
                // write left behind.
                if let Some(frame) = frame {
                    let sequence = frame.sequence;
                    let painted = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let mut out = io::stdout().lock();
                        review.switch(&mut screen, frame.alternate, &mut out)?;
                        review.paint_to(&mut screen, frame.lines, &mut out)
                    }));
                    let mut mailbox = lock.lock().unwrap_or_else(|e| e.into_inner());
                    match painted {
                        Ok(Ok(())) => mailbox.complete(sequence),
                        Ok(Err(error)) => {
                            let (cols, rows) = (screen.cols, screen.rows);
                            review.resize(&mut screen, cols, rows);
                            mailbox.fail(sequence, format!("terminal write failed: {error}"));
                        }
                        Err(_) => {
                            let (cols, rows) = (screen.cols, screen.rows);
                            review.resize(&mut screen, cols, rows);
                            mailbox.fail(sequence, "paint worker panicked".into());
                        }
                    }
                }
                if shutdown {
                    // The final frame (taken above) has landed; done.
                    break;
                }
            }
        });
        Painter {
            mailbox,
            thread: Some(thread),
            next_sequence: 0,
        }
    }

    fn post(&self, update: impl FnOnce(&mut PaintMailbox)) {
        let (lock, wake) = &*self.mailbox;
        update(&mut lock.lock().unwrap_or_else(|e| e.into_inner()));
        wake.notify_one();
    }

    pub fn frame(&mut self, lines: Vec<String>) {
        self.frame_in_view(lines, false);
    }

    /// Post the terminal-buffer mode with its frame so rapid toggles cannot mismatch them.
    pub fn frame_in_view(&mut self, lines: Vec<String>, alternate: bool) {
        self.next_sequence = self.next_sequence.wrapping_add(1);
        let sequence = self.next_sequence;
        let posted_at = e_core::rt::Instant::now();
        self.post(|mailbox| {
            mailbox.posted = sequence;
            mailbox.pending_since.get_or_insert(posted_at);
            mailbox.frame = Some(PendingFrame {
                sequence,
                posted_at,
                lines,
                alternate,
            });
        });
    }

    pub fn status(&self) -> PaintStatus {
        let (lock, _) = &*self.mailbox;
        lock.lock().unwrap_or_else(|e| e.into_inner()).status()
    }

    pub fn resize(&self, cols: u16, rows: u16) {
        self.post(|mailbox| {
            mailbox.resize = Some((cols, rows));
            mailbox.frame = None;
        });
    }

    /// Flush and stop: the pending frame lands before terminal teardown.
    pub fn shutdown(&mut self) {
        self.post(|mailbox| mailbox.shutdown = true);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The browser's painter. A page's terminal takes writes without blocking,
/// so each frame is painted as it is posted, on the page's one thread.
#[cfg(target_family = "wasm")]
pub struct Painter {
    screen: Screen,
    review: ReviewScreen,
    posted: u64,
    failure: Option<(u64, String)>,
}

#[cfg(target_family = "wasm")]
impl Painter {
    pub fn spawn(cols: u16, rows: u16, anchor: usize) -> Self {
        Painter {
            screen: Screen::new(cols, rows, anchor),
            review: ReviewScreen::default(),
            posted: 0,
            failure: None,
        }
    }

    pub fn frame(&mut self, lines: Vec<String>) {
        self.frame_in_view(lines, false);
    }

    pub fn frame_in_view(&mut self, lines: Vec<String>, alternate: bool) {
        self.posted = self.posted.wrapping_add(1);
        let mut bytes = Vec::new();
        let painted = self
            .review
            .switch(&mut self.screen, alternate, &mut bytes)
            .and_then(|()| self.review.paint_to(&mut self.screen, lines, &mut bytes))
            .and_then(|()| crate::term::write(&bytes));
        match painted {
            Ok(()) => self.failure = None,
            Err(error) => {
                let (cols, rows) = (self.screen.cols, self.screen.rows);
                self.review.resize(&mut self.screen, cols, rows);
                self.failure = Some((self.posted, format!("terminal write failed: {error}")));
            }
        }
    }

    pub fn status(&self) -> PaintStatus {
        PaintStatus {
            posted: self.posted,
            completed: self.posted,
            pending_since: None,
            failure: self.failure.clone(),
            stopped: false,
        }
    }

    pub fn resize(&mut self, cols: u16, rows: u16) {
        self.review.resize(&mut self.screen, cols, rows);
    }

    pub fn shutdown(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Measure a changing dock below cached Markdown, including transcript
    /// assembly and painting. Run explicitly in release mode for comparisons.
    #[test]
    #[ignore = "release-mode renderer benchmark"]
    fn long_session_frame_benchmark() {
        use crate::transcript::{Block, Kind, Transcript};
        let theme = crate::theme::load_bundled(false).unwrap();
        for count in [100, 1000, 10000] {
            let mut transcript = Transcript::default();
            for _ in 0..count {
                transcript.push(Block::new(
                    Kind::Assistant,
                    "A **finished** response with some text.\n\n```rust\nfn main() {}\n```",
                ));
            }
            let mut screen = Screen::new(100, 30, 0);
            let mut sink = io::sink();
            screen
                .paint_to(transcript.render(&theme, 100), &mut sink)
                .unwrap();
            let start = e_core::rt::Instant::now();
            for tick in 0..100 {
                let mut frame = transcript.render(&theme, 100);
                frame.push(format!("composer {tick}"));
                screen.paint_to(frame, &mut sink).unwrap();
            }
            eprintln!(
                "renderer {count} blocks: {:.3} ms/frame",
                start.elapsed().as_secs_f64() * 10.0
            );
        }
    }

    #[test]
    fn shrinking_a_scrolled_frame_keeps_the_composer_visible() {
        let mut screen = Screen::new(80, 10, 0);
        screen
            .paint_to(lines(40, "history"), &mut Vec::new())
            .unwrap();
        let mut output = Vec::new();
        screen
            .paint_to(vec!["finished tool".into(), "composer".into()], &mut output)
            .unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(
            output.contains("composer"),
            "the composer disappeared: {output:?}"
        );
        assert_eq!(screen.base, 0);
    }

    #[test]
    fn small_shrinks_keep_a_full_height_dock_on_the_last_row() {
        let mut screen = Screen::new(80, 10, 7);
        screen
            .paint_to(lines(40, "history"), &mut Vec::new())
            .unwrap();
        for count in [39, 35, 10] {
            let mut frame = lines(count, "history");
            frame[count - 1] = "dock".into();
            let mut bytes = Vec::new();
            screen.paint_to(frame, &mut bytes).unwrap();
            assert_eq!(count - screen.base, 10);
            assert_eq!(screen.prev[9], "dock");
            assert!(!bytes.windows(4).any(|b| b == b"\x1b[3J"));
        }
    }

    #[test]
    fn shrinking_a_near_bottom_frame_preserves_pre_launch_rows() {
        // A small overflow leaves pre-launch rows visible. Shrink in place so
        // those rows survive above the composer.
        let mut screen = Screen::new(80, 10, 8);
        screen
            .paint_to(lines(4, "history"), &mut Vec::new())
            .unwrap();
        assert_eq!(screen.base, 0);
        let mut output = Vec::new();
        screen
            .paint_to(vec!["tool".into(), "composer".into()], &mut output)
            .unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(
            output.contains("composer"),
            "the composer disappeared: {output:?}"
        );
        assert!(
            !output.contains("\x1b[1;1H"),
            "the shrink repainted from the top, clobbering pre-launch rows: {output:?}"
        );
        assert_eq!(screen.base, 0, "the shrink replayed history");
    }

    #[test]
    fn resize_preserves_scrollback_and_writes_only_the_visible_tail() {
        let mut screen = Screen::new(80, 10, 0);
        let frame = lines(40, "history");
        screen.paint_to(frame.clone(), &mut Vec::new()).unwrap();
        screen.resize(60, 8);
        let mut output = Vec::new();
        screen.paint_to(frame.clone(), &mut output).unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(!output.contains("\x1b[3J"), "resize erased native history");
        assert!(
            !output.contains("history0"),
            "resize duplicated offscreen history"
        );
        assert!(output.contains("history32") && output.contains("history39"));
    }

    #[test]
    fn an_edit_above_the_viewport_does_not_skip_a_large_append() {
        let mut screen = Screen::new(80, 10, 0);
        screen.paint_to(lines(40, "row"), &mut Vec::new()).unwrap();
        let mut next = lines(70, "row");
        next[0] = "edited heading".into();
        let mut output = Vec::new();
        screen.paint_to(next, &mut output).unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(
            output.contains("row40"),
            "new rows went into scrollback unpainted"
        );
        assert!(
            !output.contains("edited heading"),
            "committed history was replayed"
        );
    }

    /// Synthetic transcripts make row loss and replay visible in output.
    fn lines(n: usize, tag: &str) -> Vec<String> {
        (0..n).map(|i| format!("{tag}{i}")).collect()
    }

    #[test]
    fn transcripts_above_the_cell_height_limit_reach_history_in_order() {
        let mut screen = Screen::new(20, 10, 0);
        let mut output = Vec::new();
        screen.paint_to(lines(70_000, "row"), &mut output).unwrap();
        let text = e_core::tools::strip_ansi(&String::from_utf8(output).unwrap());
        let written: Vec<_> = text
            .lines()
            .map(|line| line.trim_matches('\r'))
            .filter(|line| line.starts_with("row"))
            .collect();
        assert_eq!(written.len(), 70_000, "growth lost or replayed rows");
        for (row, line) in written.iter().enumerate() {
            assert_eq!(
                *line,
                format!("row{row}"),
                "rows reached history out of order"
            );
        }
        assert_eq!(screen.prev.len(), 10);
        let mut next = lines(70_001, "row");
        next[0] = "historical edit".into();
        let mut output = Vec::new();
        screen.paint_to(next, &mut output).unwrap();
        let text = e_core::tools::strip_ansi(&String::from_utf8(output).unwrap());
        assert!(text.contains("row70000"));
        assert!(!text.contains("historical edit"));
    }

    #[test]
    fn a_maximum_height_screen_can_scroll_another_row() {
        let mut screen = Screen::new(1, u16::MAX, 0);
        let mut frame = vec!["x".into(); usize::from(u16::MAX)];
        frame.push("z".into());
        let mut output = Vec::new();
        screen.paint_to(frame, &mut output).unwrap();
        assert!(output.contains(&b'z'));
        assert_eq!(screen.base, 1);
    }

    #[test]
    fn a_failed_write_repaints_only_the_visible_tail() {
        /// Fail before accepting bytes, as a disconnected terminal would.
        struct Broken;
        impl Write for Broken {
            fn write(&mut self, _: &[u8]) -> io::Result<usize> {
                Err(io::ErrorKind::BrokenPipe.into())
            }
            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }
        let mut screen = Screen::new(20, 10, 0);
        screen.paint_to(lines(40, "row"), &mut Vec::new()).unwrap();
        assert!(screen.paint_to(lines(41, "row"), &mut Broken).is_err());
        let mut output = Vec::new();
        screen.paint_to(lines(41, "row"), &mut output).unwrap();
        let text = e_core::tools::strip_ansi(&String::from_utf8(output).unwrap());
        assert!(text.contains("row31") && text.contains("row40"));
        assert!(!text.contains("row30"));
    }

    #[test]
    fn unchanged_reachable_rows_emit_nothing() {
        let mut screen = Screen::new(20, 10, 0);
        screen.paint_to(lines(40, "row"), &mut Vec::new()).unwrap();
        let mut next = lines(40, "row");
        next[0] = "historical edit".into();
        let mut output = Vec::new();
        screen.paint_to(next, &mut output).unwrap();
        assert!(output.is_empty());
    }

    #[test]
    fn paint_progress_distinguishes_posted_completed_and_failed_frames() {
        let mut mailbox = PaintMailbox::default();
        let posted_at = e_core::rt::Instant::now();
        mailbox.posted = 2;
        mailbox.pending_since = Some(posted_at);
        mailbox.frame = Some(PendingFrame {
            sequence: 2,
            posted_at,
            lines: vec!["new".into()],
            alternate: false,
        });

        mailbox.complete(1);
        let pending = mailbox.status();
        assert_eq!((pending.posted, pending.completed), (2, 1));
        assert_eq!(pending.pending_since, Some(posted_at));

        mailbox.fail(2, "broken pipe".into());
        assert_eq!(mailbox.status().failure.unwrap().0, 2);
        mailbox.complete(2);
        let recovered = mailbox.status();
        assert_eq!(recovered.completed, 2);
        assert!(recovered.pending_since.is_none());
        assert!(recovered.failure.is_none());
    }
}
