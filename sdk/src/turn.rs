//! `Turn`: one prompt's run as a stream of events ending in a reply.
//!
//! The turn is lazy — nothing is submitted until the first poll — and it
//! reads the core's single ordered `SessionEvent` channel directly, so
//! backpressure is the channel's: an unread event holds the model. Core
//! events that exist only to keep a terminal lively (`TurnStart`, argument
//! byte counts, retry recovery) are folded away; everything else maps to
//! one [`Event`]. The core's `Error` is not an event: it ends the turn, so
//! it comes back from [`Turn::finish`] as a [`TurnError`].

use std::collections::VecDeque;
use std::future::{Future, IntoFuture};
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use futures::Stream;

use e::core::agent::SessionEvent;
use e::core::providers::ChatMessage;
use e::core::tools::{OutputStream, ToolOutcome};

use crate::{Session, TurnError};

/// What a turn reports while it runs, in the order it happened.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum Event {
    /// A fragment of the assistant's reply.
    Text(String),
    /// A fragment of visible reasoning, when the provider streams it.
    Reasoning(String),
    /// The model asked for a tool. `arguments` is the raw JSON it sent.
    /// Every call in one assistant message is announced before any runs.
    ToolCall {
        id: u64,
        name: String,
        arguments: String,
    },
    /// The call is now executing.
    ToolStart { id: u64 },
    /// A chunk of a running command's output, as it appears. A preview: a
    /// slow reader may miss chunks, but `ToolEnd` carries the retained
    /// output either way.
    ToolOutput {
        id: u64,
        stream: OutputStream,
        chunk: String,
    },
    /// The call finished. `content` is what the model reads.
    ToolEnd {
        id: u64,
        outcome: ToolOutcome,
        summary: String,
        content: String,
    },
    /// Token accounting for one provider request within the turn.
    Usage(Usage),
    /// Older history is being summarized into a checkpoint.
    Compacting,
    /// The checkpoint is installed; the turn continues on it.
    Compacted {
        summary: String,
        context_tokens: u64,
    },
    /// A retryable provider failure; the next attempt follows `delay`.
    Retry {
        attempt: u32,
        limit: u32,
        delay: Duration,
        reason: String,
    },
    /// A steering message was taken up by the turn.
    Steered(String),
    /// Steering messages that never ran because the turn was cancelled or
    /// failed first.
    Discarded(Vec<String>),
    /// An extension named the session.
    Named(String),
    /// A non-fatal problem: a refused or truncated reply the provider
    /// returned as success, skipped malformed frames, a session that could
    /// not be saved. Also collected into the reply.
    Warning(String),
    /// An extension's `notify` message. Only with extensions enabled.
    Notice(String),
}

/// Tokens across the turn's provider requests. `input` counts each
/// request's full context, cached tokens included.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
}

/// How many tools the turn ran and how many of those did not complete.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ToolStats {
    pub calls: u64,
    pub failures: u64,
}

/// Why a turn stopped. Cancellation is a stop, not a failure: the host
/// asked for it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Stop {
    #[default]
    Complete,
    Cancelled,
}

/// What a finished turn produced. `text` is every assistant fragment of
/// the turn joined, across tool rounds.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Reply {
    pub text: String,
    pub usage: Usage,
    /// An estimate from the model's declared pricing, when it has one.
    pub cost_usd: Option<f64>,
    pub tools: ToolStats,
    pub stop: Stop,
    pub warnings: Vec<String>,
}

/// What the turn will do when first polled.
pub(crate) enum Start {
    Prompt(ChatMessage),
    Compact,
    /// Refused before any request — the reason becomes the `TurnError`.
    Refused(String),
}

enum State {
    Pending(Start),
    Running,
    Done,
}

/// One prompt's run. Iterate it for events with [`Turn::next`] (or as a
/// `futures::Stream`), then call [`Turn::finish`] for the reply — or await
/// the turn directly to skip the events. Dropping a running turn interrupts
/// it.
pub struct Turn<'s> {
    session: &'s mut Session,
    state: State,
    reply: Reply,
    error: Option<String>,
    /// Steering requested before the first poll, delivered right after
    /// the turn starts.
    early_steers: Vec<String>,
    /// Events already translated but not yet handed out: a tool batch
    /// arrives as one core event and leaves as one `ToolCall` per call.
    pending_events: VecDeque<Event>,
}

impl<'s> Turn<'s> {
    pub(crate) fn new(session: &'s mut Session, start: Start) -> Self {
        Turn {
            session,
            state: State::Pending(start),
            reply: Reply::default(),
            error: None,
            early_steers: Vec::new(),
            pending_events: VecDeque::new(),
        }
    }

    /// The next event, or None once the turn has ended.
    pub async fn next(&mut self) -> Option<Event> {
        futures::future::poll_fn(|cx| Pin::new(&mut *self).poll_next(cx)).await
    }

    /// Add a message to the running turn. It is delivered at the next step
    /// (before the next provider request), and echoed as `Event::Steered`.
    /// Returns false if the turn had already ended, in which case the text
    /// was not delivered.
    pub fn steer(&mut self, text: impl Into<String>) -> bool {
        let text = text.into();
        match self.state {
            State::Pending(_) => {
                self.early_steers.push(text);
                true
            }
            State::Running => self.submit_steer(text),
            State::Done => false,
        }
    }

    /// `submit` steers only while the core still counts the turn as running.
    /// If the turn had ended before we read its `TurnEnd`, the message would
    /// have started a fresh turn instead — undo that so the session's
    /// event stream stays one turn at a time.
    fn submit_steer(&mut self, text: String) -> bool {
        let system = self.session.system_prompt();
        if self.session.agent.submit(text, system) {
            return true;
        }
        self.session.agent.interrupt();
        self.session.stale = true;
        false
    }

    /// Stop the turn. Like Esc: the current request is abandoned, tools
    /// that have not started are skipped, and everything already produced
    /// stays in history. The stream still ends with `None`, and the reply's
    /// `stop` reads `Cancelled`.
    pub fn cancel(&mut self) {
        match self.state {
            State::Pending(_) => {
                // Nothing was sent, no history changed: the turn simply
                // reports itself cancelled.
                self.reply.stop = Stop::Cancelled;
                self.state = State::Done;
            }
            State::Running => self.session.agent.interrupt(),
            State::Done => {}
        }
    }

    /// Run the turn to its end, discarding any events not yet read, and
    /// return the reply. A turn that failed returns the error with its
    /// partial reply inside.
    pub async fn finish(mut self) -> Result<Reply, TurnError> {
        while self.next().await.is_some() {}
        let mut reply = std::mem::take(&mut self.reply);
        reply.cost_usd = self.session.agent.model.pricing.as_ref().map(|rates| {
            rates.estimate(
                reply.usage.input,
                reply.usage.output,
                reply.usage.cache_read,
            )
        });
        match self.error.take() {
            Some(message) => Err(TurnError { message, reply }),
            None => Ok(reply),
        }
    }

    fn start(&mut self, start: Start) {
        let system = self.session.system_prompt();
        match start {
            Start::Prompt(message) => {
                self.session.agent.submit_message(message, system);
            }
            Start::Compact => self.session.agent.request_compaction(system),
            Start::Refused(reason) => {
                self.error = Some(reason);
                self.state = State::Done;
                return;
            }
        }
        self.state = State::Running;
        for text in std::mem::take(&mut self.early_steers) {
            if !self.submit_steer(text) {
                break;
            }
        }
    }

    /// Fold one core event into the reply's totals, then translate it.
    /// None means the event is not part of the SDK's vocabulary; `TurnEnd`
    /// additionally closes the turn.
    fn observe(&mut self, event: SessionEvent) -> Option<Event> {
        match event {
            SessionEvent::TextDelta(delta) => {
                self.reply.text.push_str(&delta);
                Some(Event::Text(delta))
            }
            SessionEvent::ReasoningDelta(delta) => Some(Event::Reasoning(delta)),
            SessionEvent::ToolBatchStart { calls } => {
                self.reply.tools.calls += calls.len() as u64;
                // One announcement per call keeps the stream flat; the
                // batch boundary is visible as the run of `ToolCall`s
                // before the first `ToolStart`.
                let mut announced = calls.into_iter().map(|call| Event::ToolCall {
                    id: call.id,
                    name: call.name,
                    arguments: call.arguments,
                });
                let first = announced.next();
                self.pending_events.extend(announced);
                first
            }
            SessionEvent::ToolStart { id } => Some(Event::ToolStart { id }),
            SessionEvent::ToolOutput { id, stream, chunk } => {
                Some(Event::ToolOutput { id, stream, chunk })
            }
            SessionEvent::ToolEnd {
                id,
                outcome,
                summary,
                content,
            } => {
                if outcome.is_error() {
                    self.reply.tools.failures += 1;
                }
                Some(Event::ToolEnd {
                    id,
                    outcome,
                    summary,
                    content,
                })
            }
            SessionEvent::Usage {
                input,
                output,
                cache_read,
            } => {
                let usage = Usage {
                    input,
                    output,
                    cache_read,
                };
                self.reply.usage.input = self.reply.usage.input.saturating_add(input);
                self.reply.usage.output = self.reply.usage.output.saturating_add(output);
                self.reply.usage.cache_read =
                    self.reply.usage.cache_read.saturating_add(cache_read);
                Some(Event::Usage(usage))
            }
            SessionEvent::Compacting => Some(Event::Compacting),
            SessionEvent::Compacted {
                summary,
                context_tokens,
            } => Some(Event::Compacted {
                summary,
                context_tokens,
            }),
            SessionEvent::Retry {
                attempt,
                limit,
                delay_secs,
                cause,
                reason,
            } => {
                let reason = format!("{}: {reason}", cause.label());
                self.reply.warnings.push(format!(
                    "{reason} — retrying ({attempt}/{limit}) in {delay_secs}s"
                ));
                Some(Event::Retry {
                    attempt,
                    limit,
                    delay: Duration::from_secs(delay_secs),
                    reason,
                })
            }
            SessionEvent::Steered(text) => Some(Event::Steered(text)),
            SessionEvent::Discarded(texts) => Some(Event::Discarded(texts)),
            SessionEvent::Named(name) => Some(Event::Named(name)),
            SessionEvent::Warning(warning) => {
                self.reply.warnings.push(warning.clone());
                Some(Event::Warning(warning))
            }
            SessionEvent::SleepStopped { duration_secs } => {
                let warning = format!(
                    "the process slept for {duration_secs}s; the turn stopped with its partial work committed"
                );
                self.reply.warnings.push(warning.clone());
                Some(Event::Warning(warning))
            }
            SessionEvent::Error(message) => {
                self.error = Some(message);
                None
            }
            SessionEvent::TurnEnd { aborted } => {
                self.reply.stop = if aborted {
                    Stop::Cancelled
                } else {
                    Stop::Complete
                };
                self.state = State::Done;
                None
            }
            SessionEvent::TurnStart
            | SessionEvent::ToolCallAssembly { .. }
            | SessionEvent::Recovered { .. }
            | SessionEvent::Slept { .. } => None,
        }
    }
}

impl Stream for Turn<'_> {
    type Item = Event;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Event>> {
        loop {
            if let Some(event) = self.pending_events.pop_front() {
                return Poll::Ready(Some(event));
            }
            match self.state {
                State::Done => return Poll::Ready(None),
                State::Pending(_) => {
                    // A dropped turn's tail comes first: the core will not
                    // start a new turn while it still counts the old one as
                    // running, so drain to its `TurnEnd`.
                    if self.session.stale {
                        match self.session.events.poll_recv(cx) {
                            Poll::Ready(Some(SessionEvent::TurnEnd { .. })) => {
                                self.session.stale = false;
                            }
                            Poll::Ready(Some(_)) => continue,
                            Poll::Ready(None) => {
                                self.error = Some(CLOSED.into());
                                self.state = State::Done;
                            }
                            Poll::Pending => return Poll::Pending,
                        }
                        continue;
                    }
                    let State::Pending(start) = std::mem::replace(&mut self.state, State::Running)
                    else {
                        continue;
                    };
                    self.start(start);
                }
                State::Running => {
                    // Extension notices are out-of-band diagnostics; deliver
                    // them ahead of model output, never instead of it.
                    if let Some(notices) = self.session.notices.as_mut() {
                        if let Poll::Ready(Some(notice)) = notices.poll_recv(cx) {
                            return Poll::Ready(Some(Event::Notice(notice)));
                        }
                    }
                    match self.session.events.poll_recv(cx) {
                        Poll::Ready(Some(event)) => {
                            if let Some(event) = self.observe(event) {
                                return Poll::Ready(Some(event));
                            }
                        }
                        Poll::Ready(None) => {
                            self.error = Some(CLOSED.into());
                            self.state = State::Done;
                        }
                        Poll::Pending => return Poll::Pending,
                    }
                }
            }
        }
    }
}

/// The core's turn worker stopped without publishing `TurnEnd`.
const CLOSED: &str = "agent event stream closed before turn completion";

impl<'s> IntoFuture for Turn<'s> {
    type Output = Result<Reply, TurnError>;
    type IntoFuture = Pin<Box<dyn Future<Output = Self::Output> + Send + 's>>;

    fn into_future(self) -> Self::IntoFuture {
        Box::pin(self.finish())
    }
}

impl Drop for Turn<'_> {
    fn drop(&mut self) {
        if matches!(self.state, State::Running) {
            self.session.agent.interrupt();
            self.session.stale = true;
        }
    }
}
