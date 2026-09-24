//! Provider requests, tool batches, steering, and context maintenance for one run.
//! The supervisor owns terminal events; this worker stops only at a safe boundary.
//!
//! A run is a loop of steps, and [`Turn::step`] reads as the whole story:
//! steer, compact if needed, stream one reply, commit it, run its tools.
//! Every phase returns a [`Flow`]: `Continue` carries on and `Break` ends the
//! run with its [`Outcome`], so `?` is how a phase stops the run.

use std::collections::HashSet;
use std::iter::Peekable;
use std::ops::ControlFlow::{self, Break, Continue};
use std::time::{SystemTime, UNIX_EPOCH};

use super::*;

/// Keep going with a `T`, or end the run with this outcome.
type Flow<T = ()> = ControlFlow<Outcome, T>;

/// A worker failure cannot restart just because prompts arrived during shutdown.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Outcome {
    Complete,
    Cancelled,
    Failed,
}

impl Outcome {
    fn stopped(cancel: &AtomicBool) -> Self {
        if cancel.load(Ordering::SeqCst) {
            Self::Cancelled
        } else {
            Self::Failed
        }
    }
}

/// Everything a worker needs, captured once at submission. Continuations share
/// the log and cancellation token while retaining the selected model and policy.
#[derive(Clone)]
pub(super) struct Context {
    pub(super) log: TurnLog,
    pub(super) events: mpsc::Sender<SessionEvent>,
    pub(super) history: Arc<Mutex<Vec<ChatMessage>>>,
    pub(super) cancel: Arc<AtomicBool>,
    pub(super) model: Model,
    pub(super) cwd: PathBuf,
    pub(super) effort: Option<String>,
    pub(super) pending: Arc<Mutex<PendingQueue>>,
    pub(super) host: Option<Arc<crate::extensions::ExtensionHost>>,
    pub(super) tool_seq: Arc<AtomicU64>,
    pub(super) active_tools: Arc<Mutex<Option<Vec<String>>>>,
    pub(super) wake: wake::Shared,
    pub(super) system: String,
    pub(super) allowed_tools: Option<Arc<Vec<String>>>,
    pub(super) compact_requested: Arc<AtomicBool>,
    pub(super) compact_focus: Arc<Mutex<Option<String>>>,
    pub(super) instructions_loaded: Arc<Mutex<std::collections::HashSet<PathBuf>>>,
    pub(super) tool_runtime: Arc<tools::ToolRuntime>,
    pub(super) tool_mode: ToolMode,
}

/// Execute requests until an answer, cancellation, or failure. Compaction
/// happens after complete tool batches and never requires frontend intervention.
pub(super) async fn run(context: Context, compact_only: bool) -> Outcome {
    let mut turn = Turn::new(context, compact_only);
    loop {
        if let Break(outcome) = turn.step().await {
            return outcome;
        }
    }
}

/// A run's state across its steps.
struct Turn {
    ctx: Context,
    compact_only: bool,
    /// This run's system prompt; a `before_turn` hook may extend it.
    system: String,
    /// Steps taken, one provider request each. [`MAX_STEPS`] is a runaway
    /// backstop far above real work, not a working budget.
    steps: u32,
    /// The sleep policy, read once per run.
    window_secs: u64,
    max_continuations: u32,
    sleep_continuations: u32,
    /// Whether this run spent its one quiet retry of a blank reply.
    empty_retried: bool,
    settled_tools: usize,
    failed_tools: usize,
    /// The current step's context size in tokens: an estimate until the
    /// provider reports usage, then grown by each tool result. Drives
    /// compaction after a tool batch.
    context_tokens: u64,
}

/// What one step's stream produced, and why it stopped early if it did.
#[derive(Default)]
struct Reply {
    text: String,
    calls: Vec<ToolCall>,
    reasoning: Vec<String>,
    /// Thought deltas arrived. A dialect can stream thoughts without ever
    /// committing a reasoning item (Gemini's empty-text thought chunks), so
    /// this alone still counts as output: a thinking-only reply is neither
    /// replayed by a retry nor called empty.
    reasoning_streamed: bool,
    /// The stream's last usage frame. Dialects may report usage cumulatively
    /// mid-stream, so only the final frame is this step's.
    usage: Option<providers::Usage>,
    interrupted: Option<Interruption>,
}

impl Reply {
    /// Nothing reached the user or the model: no text, calls, or reasoning.
    fn is_blank(&self) -> bool {
        self.text.is_empty()
            && self.calls.is_empty()
            && self.reasoning.is_empty()
            && !self.reasoning_streamed
    }
}

/// Why a step's stream stopped before the provider finished it.
enum Interruption {
    /// Esc mid-stream.
    Cancelled,
    /// The provider failed; the error is already reported.
    Failed,
    /// The device slept past the resume window or the continuation cap. The
    /// stop is already reported, so the run ends as a stop, not an error.
    SleptTooLong,
    /// The device slept mid-reply within the window: the run continues and
    /// asks the model to finish its own sentence.
    SleptMidReply(Duration),
}

/// One provider request in flight for the current step.
struct Attempt {
    /// 1-based: attempt 1 is the first try.
    number: u32,
    /// Requests allowed for the step, the first included
    /// (`retry_max_attempts`). A request budget, not a retry budget.
    limit: u32,
    /// When this stream opened. A sleep that ended after it happened with
    /// the request in flight, so the loss is the sleep's, not the provider's.
    started: Instant,
    rx: mpsc::Receiver<ProviderEvent>,
    handle: tokio::task::JoinHandle<()>,
    /// Tool-argument bytes streamed so far, for the liveness row.
    assembly_bytes: u64,
    /// A retried attempt already reported that it recovered.
    recovered: bool,
}

impl Attempt {
    fn open(request: &Request) -> Self {
        let limit = retry::max_attempts();
        let started = Instant::now();
        let (rx, handle) = providers::stream(clone_request(request));
        Self {
            number: 1,
            limit,
            started,
            rx,
            handle,
            assembly_bytes: 0,
            recovered: false,
        }
    }

    /// Send the same request again. The new stream starts its tool arguments
    /// and usage from scratch: counts the dead stream reported aren't its own.
    fn reopen(&mut self, request: &Request, reply: &mut Reply) {
        self.started = Instant::now();
        (self.rx, self.handle) = providers::stream(clone_request(request));
        self.assembly_bytes = 0;
        reply.usage = None;
    }

    /// Nothing streamed this attempt, not even part of a tool call, so
    /// replaying the request can't duplicate anything.
    fn nothing_produced(&self, reply: &Reply) -> bool {
        reply.is_blank() && self.assembly_bytes == 0
    }
}

impl Turn {
    fn new(ctx: Context, compact_only: bool) -> Self {
        Self {
            system: ctx.system.clone(),
            ctx,
            compact_only,
            steps: 0,
            window_secs: wake::policy::window_secs(),
            max_continuations: wake::policy::max_continuations(),
            sleep_continuations: 0,
            empty_retried: false,
            settled_tools: 0,
            failed_tools: 0,
            context_tokens: 0,
        }
    }

    /// One provider request and whatever it asks for. `Continue` means the
    /// run needs another step.
    async fn step(&mut self) -> Flow {
        if self.cancelled() {
            return Break(Outcome::Cancelled);
        }
        self.compact_on_request().await?;
        self.steps += 1;
        if self.steps > MAX_STEPS {
            self.fail(format!(
                "turn stopped after {MAX_STEPS} steps — send a message to continue"
            ))
            .await;
            return Break(Outcome::Failed);
        }
        self.take_steering().await;
        // The first request is the extensions' moment. Continuations and
        // compaction-only runs are not turns.
        if self.steps == 1 && !self.compact_only {
            self.start_turn().await;
        }
        let active_tools = lock(&self.ctx.active_tools).clone().map(Arc::new);
        let request = self.build_request(&active_tools).await?;

        let mut reply = self.stream_reply(&request).await?;
        let response = providers::ResponseMeta::new(
            &self.ctx.model,
            providers::ResponsePurpose::Turn,
            reply.usage,
        );
        self.count_reply(&reply).await;
        if let Some(interruption) = reply.interrupted.take() {
            return self.end_interrupted(interruption, reply, response).await;
        }
        if reply.is_blank() && !self.has_pending() {
            return self.retry_blank(response).await;
        }

        let answered = !reply.text.trim().is_empty();
        let calls = reply.calls.clone();
        self.commit_reply(reply, response).await;
        if calls.is_empty() {
            return self.end_without_tools(answered).await;
        }
        self.run_tool_batch(&calls, active_tools).await?;
        // Nested instructions for the paths the batch touched join after its
        // results, so the provider's call/result pairing holds.
        load_nested_instructions(
            &calls,
            &self.ctx.cwd,
            &self.ctx.instructions_loaded,
            &self.ctx.log,
            &self.ctx.events,
        )
        .await;
        self.compact_after_tools().await
    }

    /// Run the compaction `/compact` asked for. A compaction-only run ends here.
    async fn compact_on_request(&mut self) -> Flow {
        if !self.ctx.compact_requested.swap(false, Ordering::SeqCst) {
            return Continue(());
        }
        match self.compact().await {
            Ok(true) => {}
            Ok(false) => {
                self.warn("recent context already fits; nothing to compact")
                    .await
            }
            Err(error) => {
                self.fail(error).await;
                return Break(Outcome::stopped(&self.ctx.cancel));
            }
        }
        if self.compact_only && self.steps == 0 {
            return Break(Outcome::Complete);
        }
        Continue(())
    }

    /// Fold messages queued while the run worked into the conversation. The
    /// queue review edits entries by key concurrently; whatever is taken
    /// here is gone from it.
    async fn take_steering(&self) {
        let steered: Vec<ChatMessage> = lock(&self.ctx.pending)
            .items
            .drain(..)
            .map(|(_, message)| message)
            .collect();
        for mut message in steered {
            // An extension's internal message rides the queue too: the model
            // sees it, the transcript doesn't. Images stay attached either
            // way; a steering message is the user's intent, not a text echo.
            if !message.is_internal() {
                self.emit(SessionEvent::Steered(message.content.clone()))
                    .await;
            }
            message.mark_internal();
            self.ctx.log.commit_async(message).await;
        }
    }

    /// The run's first request: `before_turn` hooks may extend the system
    /// prompt and add messages, and `turn_start` carries the prompt.
    async fn start_turn(&mut self) {
        let Some(host) = self.ctx.host.clone() else {
            return;
        };
        let prompt = lock(&self.ctx.history)
            .iter()
            .rev()
            .find(|m| {
                matches!(m.kind, crate::providers::MessageKind::User { .. }) && !m.is_internal()
            })
            .map(|m| m.content.clone())
            .unwrap_or_default();
        if host.has_hook("before_turn") {
            let added = host.hook_before_turn(&prompt).await;
            for suffix in added.system_suffixes {
                self.system.push_str("\n\n");
                self.system.push_str(&suffix);
            }
            for message in added.messages {
                let mut recorded = ChatMessage::user(message.content.clone());
                if message.internal {
                    recorded.mark_internal();
                } else {
                    self.emit(SessionEvent::Steered(message.content)).await;
                }
                self.ctx.log.commit_async(recorded).await;
            }
        }
        host.event("turn_start", serde_json::json!({"prompt": prompt}))
            .await;
    }

    /// This step's request, compacting first if history no longer fits.
    async fn build_request(&mut self, active_tools: &Option<Arc<Vec<String>>>) -> Flow<Request> {
        let mut messages = self.history();
        // Some gateways never report usage, so start from a conservative
        // local estimate; a real usage frame replaces it. Sized before the
        // image strip below, so it errs on the side of what's sent.
        self.context_tokens = compact::estimate_request_tokens(&self.system, &messages);
        if compact::should_compact(self.context_tokens, self.ctx.model.context_window) {
            match self.compact().await {
                Ok(true) => {
                    messages = self.history();
                    self.context_tokens = compact::estimate_request_tokens(&self.system, &messages);
                }
                result => {
                    let message = result.err().unwrap_or_else(|| {
                        "context is full and cannot be reduced; history was preserved".into()
                    });
                    self.fail(message).await;
                    return Break(Outcome::stopped(&self.ctx.cancel));
                }
            }
        }
        // A resumed session or a model switch can carry images to a model
        // that can't take them. Every frontend's request passes here, so this
        // is the one place to strip them — from this copy only; the session
        // keeps its images.
        providers::strip_incompatible_images(&mut messages, &self.ctx.model);
        // The stable conversation id (empty for an unsaved session), sent as
        // a session header by providers that opt in.
        let session_id = lock(&self.ctx.log.session)
            .as_ref()
            .map(|s| s.id().to_string())
            .unwrap_or_default();
        Continue(Request {
            model: self.ctx.model.clone(),
            system: self.system.clone(),
            messages,
            effort: self.ctx.effort.clone(),
            session_id,
            tools: self.tool_schemas(active_tools),
        })
    }

    /// The tools a request offers: every schema the mode allows, narrowed by
    /// the request allowlist, then by an extension's narrowing for this turn.
    fn tool_schemas(&self, active_tools: &Option<Arc<Vec<String>>>) -> Vec<serde_json::Value> {
        let ctx = &self.ctx;
        // A request allowlist names built-ins, so extension tools and
        // overrides stay out whenever one is set.
        let all = match (&ctx.host, ctx.tool_mode, ctx.allowed_tools.is_some()) {
            (Some(host), ToolMode::All, false) => host.merged_tool_schemas(),
            _ => tools::schemas(),
        };
        let allowed = tools::restrict_to(
            tools::filter_schemas(all, ctx.tool_mode),
            ctx.allowed_tools.as_deref().map(Vec::as_slice),
        );
        tools::restrict_to(allowed, active_tools.as_deref().map(Vec::as_slice))
    }

    /// Stream one reply, retrying failures that are safe to replay. Esc wins
    /// even when the provider goes silent: a stalled stream never yields
    /// another event to check it on.
    async fn stream_reply(&mut self, request: &Request) -> Flow<Reply> {
        let mut reply = Reply::default();
        let mut attempt = Attempt::open(request);
        loop {
            let event = tokio::select! {
                event = attempt.rx.recv() => event,
                _ = wait_cancelled(&self.ctx.cancel) => {
                    attempt.handle.abort();
                    reply.interrupted = Some(Interruption::Cancelled);
                    break;
                }
            };
            let Some(event) = event else {
                break;
            };
            // A retry worked once its stream yields anything but another error.
            if attempt.number > 1 && !attempt.recovered && !matches!(event, ProviderEvent::Error(_))
            {
                attempt.recovered = true;
                self.emit(SessionEvent::Recovered {
                    attempt: attempt.number,
                    limit: attempt.limit,
                })
                .await;
            }
            match event {
                ProviderEvent::TextDelta(delta) => {
                    reply.text.push_str(&delta);
                    self.emit(SessionEvent::TextDelta(delta)).await;
                }
                ProviderEvent::ReasoningDelta(delta) => {
                    reply.reasoning_streamed = true;
                    self.emit(SessionEvent::ReasoningDelta(delta)).await;
                }
                ProviderEvent::ToolArgumentsDelta { delta, .. } => {
                    attempt.assembly_bytes += delta.len() as u64;
                    self.emit(SessionEvent::ToolCallAssembly {
                        bytes: attempt.assembly_bytes,
                    })
                    .await;
                }
                ProviderEvent::ToolCallStart { .. } | ProviderEvent::ToolCallEnd { .. } => {}
                ProviderEvent::ToolCall(call) => reply.calls.push(call),
                ProviderEvent::ReasoningItem(item) => reply.reasoning.push(item),
                ProviderEvent::Usage(usage) => reply.usage = Some(usage),
                ProviderEvent::Done(end) => self.warn_abnormal_finish(&end).await,
                // The provider sends an error as its last event, so a
                // reported failure ends the stream.
                ProviderEvent::Error(error) => {
                    match self
                        .on_stream_error(error, &mut attempt, &mut reply, request)
                        .await?
                    {
                        None => continue,
                        Some(interruption) => {
                            reply.interrupted = Some(interruption);
                            break;
                        }
                    }
                }
            }
        }
        Continue(reply)
    }

    /// Decide what a provider error means: `None` when the stream was
    /// reopened, otherwise how the stream ends. Esc during a retry backoff
    /// ends the run.
    async fn on_stream_error(
        &mut self,
        err: providers::ProviderError,
        attempt: &mut Attempt,
        reply: &mut Reply,
        request: &Request,
    ) -> Flow<Option<Interruption>> {
        let nothing_produced = attempt.nothing_produced(reply);
        if let Some(gap) = wake::gap_since(&self.ctx.wake, attempt.started) {
            let slept = gap.duration.as_secs();
            // Past the window the run stops, whatever the request died of.
            if slept >= self.window_secs {
                let reason = format!("window {}s", self.window_secs);
                return Continue(Some(self.stop_for_sleep(&gap, &reason, attempt).await));
            }
            // Nothing streamed: replay at once. The machine lost the
            // request, so it costs no attempt and gets no backoff.
            if nothing_produced {
                self.emit(SessionEvent::Slept {
                    duration_secs: slept,
                })
                .await;
                attempt.reopen(request, reply);
                return Continue(None);
            }
            // Mid-reply: commit the partial and let the model finish its
            // sentence, up to the cap, so a flapping lid can't chain turns
            // unattended.
            if self.sleep_continuations < self.max_continuations {
                return Continue(Some(Interruption::SleptMidReply(gap.duration)));
            }
            let reason = format!("continuation cap {} reached", self.max_continuations);
            return Continue(Some(self.stop_for_sleep(&gap, &reason, attempt).await));
        }
        // Replay only a retryable cause (never auth or a rejection) when
        // nothing streamed: a request that produced output or ran tools
        // can't be replayed without risking a duplicate.
        if err.cause.is_retryable() && nothing_produced && attempt.number < attempt.limit {
            let delay = retry::delay_for(attempt.number, err.retry_after);
            attempt.number += 1;
            self.emit(SessionEvent::Retry {
                attempt: attempt.number,
                limit: attempt.limit,
                delay_secs: delay.as_secs(),
                cause: err.cause,
                reason: err.short.clone(),
            })
            .await;
            if !sleep_cancellable(delay, &self.ctx.cancel).await {
                attempt.handle.abort();
                return Break(Outcome::Cancelled);
            }
            attempt.reopen(request, reply);
            return Continue(None);
        }
        self.report_failure(err, attempt, reply, nothing_produced)
            .await;
        Continue(Some(Interruption::Failed))
    }

    /// Stop the run over a sleep it can't resume from, reported as a stop
    /// rather than an error. The partial reply is still committed.
    async fn stop_for_sleep(
        &self,
        gap: &wake::SleepGap,
        reason: &str,
        attempt: &Attempt,
    ) -> Interruption {
        self.emit(SessionEvent::SleepStopped {
            duration_secs: gap.duration.as_secs(),
        })
        .await;
        self.warn(format!(
            "run stopped — the device was asleep for {} ({reason})",
            gap.label()
        ))
        .await;
        attempt.handle.abort();
        Interruption::SleptTooLong
    }

    /// Record and report a failure the step can't recover from.
    async fn report_failure(
        &self,
        err: providers::ProviderError,
        attempt: &Attempt,
        reply: &Reply,
        nothing_produced: bool,
    ) {
        let retry_decision =
            failure::ErrorDetails::retry_decision(&err, !nothing_produced, attempt.limit);
        let details = failure::ErrorDetails {
            summary: failure::ErrorDetails::summary(&err).await,
            detail: err.diagnostic(),
            cause: err.cause,
            stage: err.stage,
            response: err.response.as_ref().clone(),
            provider_code: err.provider_code.clone(),
            provider: self.ctx.model.provider.clone(),
            model: self.ctx.model.id.clone(),
            timestamp_ms: millis(
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default(),
            ),
            attempt_elapsed_ms: millis(attempt.started.elapsed()),
            step: self.steps,
            attempt: attempt.number,
            max_attempts: attempt.limit,
            retry_after_secs: err.retry_after,
            partial_tool_argument_bytes: attempt.assembly_bytes,
            retry_decision,
            partial_text_bytes: reply.text.len(),
            reasoning_received: reply.reasoning_streamed || !reply.reasoning.is_empty(),
            unexecuted_tool_calls: reply.calls.len(),
            settled_tools: self.settled_tools,
            failed_tools: self.failed_tools,
            recovery: failure::ErrorDetails::recovery(err.cause, retry_decision),
        };
        self.ctx.log.record_error(details.clone()).await;
        self.emit(SessionEvent::ErrorDetails(Box::new(details)))
            .await;
        // "Gave up after N/M" only when retries were genuinely exhausted; a
        // failure that could never be retried would misreport why it stopped.
        let message = if attempt.limit > 0 && err.cause.is_retryable() && nothing_produced {
            format!(
                "{} — gave up after {}/{} attempts: {}",
                err.cause.label(),
                attempt.number,
                attempt.limit,
                err.message
            )
        } else if err.cause == FailureCause::QuotaExhausted {
            // Lead with the why: the raw body behind it is provider JSON the
            // row never shows.
            format!("{} — {}", err.cause.label(), err.message)
        } else {
            err.message
        };
        self.fail(message).await;
    }

    /// A finished stream isn't necessarily a full answer: a truncated,
    /// refused, or filtered reply arrives as an HTTP success and must not
    /// pass silently.
    async fn warn_abnormal_finish(&self, end: &providers::StreamEnd) {
        let warning = match &end.finish {
            FinishReason::Normal | FinishReason::ToolCalls => None,
            FinishReason::Length => {
                Some("reply truncated: the provider hit its output limit".to_string())
            }
            FinishReason::Refusal => Some("the model refused to answer".to_string()),
            FinishReason::ContentFilter => {
                Some("output blocked by the provider's content filter".to_string())
            }
            FinishReason::Other(reason) => Some(format!("turn ended abnormally: {reason}")),
        };
        if let Some(warning) = warning {
            self.warn(warning).await;
        }
        if end.malformed > 0 {
            let plural = if end.malformed == 1 { "" } else { "s" };
            self.warn(format!(
                "{} malformed stream event{plural} skipped",
                end.malformed
            ))
            .await;
        }
    }

    /// Report the step's usage once, from the stream's final frame, even when
    /// the stream then failed: the tokens were still spent. Without a frame,
    /// grow the estimate by the reply itself.
    async fn count_reply(&mut self, reply: &Reply) {
        if let Some(usage) = reply.usage {
            self.context_tokens = usage.prompt_tokens().saturating_add(usage.output);
            self.emit(SessionEvent::Usage {
                usage,
                pricing: self.ctx.model.pricing.clone(),
            })
            .await;
        } else {
            let provisional = ChatMessage::assistant(reply.text.clone(), reply.calls.clone());
            self.context_tokens = self
                .context_tokens
                .saturating_add(compact::estimate_message_tokens(&provisional));
        }
    }

    /// End a stream that didn't finish. What already streamed is committed —
    /// the user watched it arrive, and the model must not contradict its own
    /// visible words — and each call that never ran gets a synthetic result,
    /// since a dangling tool call fails the next request on every dialect.
    /// A sleep within the window continues the run instead of ending it.
    async fn end_interrupted(
        &mut self,
        interruption: Interruption,
        reply: Reply,
        response: providers::ResponseMeta,
    ) -> Flow {
        if !reply.text.is_empty() || !reply.calls.is_empty() {
            for item in reply.reasoning {
                self.ctx
                    .log
                    .commit_async(ChatMessage::reasoning(item))
                    .await;
            }
            let (note, outcome, summary) = match interruption {
                Interruption::Cancelled => (
                    "not executed — the turn was cancelled before this call ran",
                    tools::ToolOutcome::Cancelled,
                    "cancelled",
                ),
                _ => (
                    "not executed — the provider stream failed before this call ran",
                    tools::ToolOutcome::Failed,
                    "error",
                ),
            };
            let unrun = reply.calls.clone();
            let partial = ChatMessage::assistant(reply.text, reply.calls).with_response(response);
            self.ctx.log.commit_async(partial).await;
            for call in unrun {
                self.ctx
                    .log
                    .commit_async(ChatMessage::tool_result_with_meta(
                        call.id, note, outcome, summary,
                    ))
                    .await;
            }
        }
        match interruption {
            // A sleep stop already reported itself, so it ends quietly, as
            // Esc does.
            Interruption::Cancelled | Interruption::SleptTooLong => Break(Outcome::Cancelled),
            Interruption::Failed => Break(Outcome::Failed),
            Interruption::SleptMidReply(duration) => {
                self.sleep_continuations += 1;
                self.emit(SessionEvent::Slept {
                    duration_secs: duration.as_secs(),
                })
                .await;
                // Harness-authored: it fills the history but is not a user turn.
                let mut recorded = ChatMessage::user(SLEEP_CONTINUATION.to_string());
                recorded.mark_internal();
                self.ctx.log.commit_async(recorded).await;
                self.emit(SessionEvent::Steered(SLEEP_CONTINUATION.to_string()))
                    .await;
                Continue(())
            }
        }
    }

    /// A reply with no text, calls, reasoning, or error. Committing it would
    /// strand the run in silence, and it's the most transient failure a
    /// provider produces, so it gets one quiet retry per run when the attempt
    /// budget allows. The request may still have been billed, so its response
    /// is recorded outside model history either way.
    async fn retry_blank(&mut self, response: providers::ResponseMeta) -> Flow {
        self.ctx.log.record_response(response).await;
        let limit = retry::max_attempts();
        if !self.empty_retried && limit > 1 {
            self.empty_retried = true;
            self.emit(SessionEvent::Retry {
                attempt: 2,
                limit,
                delay_secs: 1,
                cause: FailureCause::ProviderUnavailable,
                reason: "empty response".into(),
            })
            .await;
            if !sleep_cancellable(Duration::from_secs(1), &self.ctx.cancel).await {
                return Break(Outcome::Cancelled);
            }
            return Continue(());
        }
        self.fail("the model returned an empty response").await;
        Break(Outcome::Failed)
    }

    /// Commit a finished reply. Reasoning goes first: the dialect that
    /// produced it replays it ahead of the assistant message. The response
    /// metadata rides along; compaction carries it forward without sending
    /// it back to the provider.
    async fn commit_reply(&self, reply: Reply, response: providers::ResponseMeta) {
        for item in reply.reasoning {
            self.ctx
                .log
                .commit_async(ChatMessage::reasoning(item))
                .await;
        }
        let message = ChatMessage::assistant(reply.text, reply.calls).with_response(response);
        self.ctx.log.commit_async(message).await;
    }

    /// A reply without tool calls ends the run, unless a message landed
    /// mid-reply: then one more step delivers it.
    async fn end_without_tools(&self, answered: bool) -> Flow {
        if self.has_pending() {
            return Continue(());
        }
        if !answered {
            let message =
                crate::config::settings::get_string("no_answer_message").unwrap_or_else(|| {
                    "The model finished without an answer. Retry or ask it to continue.".into()
                });
            self.warn(message).await;
        }
        Break(Outcome::Complete)
    }

    /// Run a reply's tool calls and commit a result for each, in provider
    /// order. Calls run concurrently in waves of up to `tool_concurrency`.
    async fn run_tool_batch(
        &mut self,
        calls: &[ToolCall],
        active_tools: Option<Arc<Vec<String>>>,
    ) -> Flow {
        let batch = self.announce_batch(calls).await;
        let concurrency = crate::config::settings::get_u64("tool_concurrency")
            .unwrap_or(8)
            .clamp(1, 64) as usize;
        let mut remaining = batch.into_iter().peekable();
        while remaining.peek().is_some() {
            if self.cancelled() {
                // Every advertised call needs a result, even if it never ran.
                for (_, call) in remaining.by_ref() {
                    self.ctx
                        .log
                        .commit_async(ChatMessage::tool_result_with_meta(
                            call.id,
                            "tool cancelled before execution",
                            tools::ToolOutcome::Cancelled,
                            "cancelled",
                        ))
                        .await;
                }
                break;
            }
            let wave = self.next_wave(&mut remaining, concurrency).await;
            let tasks: Vec<_> = wave
                .into_iter()
                .map(|(id, call)| {
                    let context = self.tool_context(id, &active_tools);
                    (call.clone(), tokio::spawn(execute_tool(context, call)))
                })
                .collect();
            for (call, task) in tasks {
                let output = settle_tool(task, &self.ctx.cancel).await;
                self.settled_tools += 1;
                self.failed_tools += usize::from(output.outcome.is_error());
                // About four characters per token.
                self.context_tokens = self
                    .context_tokens
                    .saturating_add((output.content.chars().count() as u64).div_ceil(4));
                self.ctx
                    .log
                    .commit_async(ChatMessage::tool_result_with_meta(
                        call.id,
                        output.content,
                        output.outcome,
                        output.summary,
                    ))
                    .await;
            }
        }
        if self.cancelled() {
            return Break(Outcome::Cancelled);
        }
        Continue(())
    }

    /// Number the batch and announce it whole before anything runs, so the
    /// transcript shows one stable group from the first call.
    async fn announce_batch(&self, calls: &[ToolCall]) -> Vec<(u64, ToolCall)> {
        let mut batch = Vec::with_capacity(calls.len());
        let mut shown = Vec::with_capacity(calls.len());
        for call in calls {
            let args: serde_json::Value =
                serde_json::from_str(&call.arguments).unwrap_or(serde_json::Value::Null);
            let presentation = match self
                .ctx
                .host
                .as_ref()
                .and_then(|h| h.tool_label(&call.name))
            {
                Some(label) => tools::present_labeled(
                    &call.name,
                    &label.category,
                    &label.running,
                    &label.completed,
                    &label.target,
                    &args,
                ),
                None => tools::present(&call.name, &args),
            };
            let id = self.ctx.tool_seq.fetch_add(1, Ordering::SeqCst) + 1;
            shown.push(ToolCallPresentation {
                id,
                name: call.name.clone(),
                arguments: call.arguments.clone(),
                category: presentation.category,
                running: presentation.running,
                completed: presentation.completed,
                target: presentation.target,
            });
            batch.push((id, call.clone()));
        }
        self.emit(SessionEvent::ToolBatchStart { calls: shown })
            .await;
        batch
    }

    /// Take up to `limit` calls off the batch, stopping before a call whose
    /// file an earlier call in the wave already names, so mutations of one
    /// file follow provider order.
    async fn next_wave(
        &self,
        remaining: &mut Peekable<std::vec::IntoIter<(u64, ToolCall)>>,
        limit: usize,
    ) -> Vec<(u64, ToolCall)> {
        let mut files = HashSet::new();
        let mut wave = Vec::new();
        while wave.len() < limit {
            let Some((_, call)) = remaining.peek() else {
                break;
            };
            if let Some(file) = file_target(call, &self.ctx.cwd, &self.ctx.cancel).await {
                if !files.insert(file) {
                    break;
                }
            }
            wave.extend(remaining.next());
        }
        wave
    }

    fn tool_context(&self, id: u64, active_tools: &Option<Arc<Vec<String>>>) -> ToolRunContext {
        ToolRunContext {
            tools: self.ctx.tool_runtime.clone(),
            host: self.ctx.host.clone(),
            tool_mode: self.ctx.tool_mode,
            allowed_tools: self.ctx.allowed_tools.clone(),
            active_tools: active_tools.clone(),
            cwd: self.ctx.cwd.clone(),
            cancel: self.ctx.cancel.clone(),
            id,
            events: self.ctx.events.clone(),
        }
    }

    /// Compact once a whole tool batch is committed, if the context is nearly
    /// full. The next step continues on the same event stream.
    async fn compact_after_tools(&mut self) -> Flow {
        if self.context_tokens == 0
            || !compact::should_compact(self.context_tokens, self.ctx.model.context_window)
        {
            return Continue(());
        }
        self.warn("context nearly full — compacting before continuing")
            .await;
        match self.compact().await {
            Ok(true) => Continue(()),
            Ok(false) => {
                self.fail("context is full and cannot be reduced; history was preserved")
                    .await;
                Break(Outcome::Failed)
            }
            Err(error) => {
                self.fail(error).await;
                Break(Outcome::stopped(&self.ctx.cancel))
            }
        }
    }

    async fn compact(&self) -> Result<bool, String> {
        compact_log(
            &self.ctx.log,
            &self.system,
            &self.ctx.cancel,
            self.ctx.host.as_ref(),
            take_focus(&self.ctx.compact_focus),
        )
        .await
    }

    /// Send one event to the frontend. A closed receiver means nobody is
    /// listening, which never stops the run.
    async fn emit(&self, event: SessionEvent) {
        let _ = self.ctx.events.send(event).await;
    }

    async fn warn(&self, message: impl Into<String>) {
        self.emit(SessionEvent::Warning(message.into())).await;
    }

    async fn fail(&self, message: impl Into<String>) {
        self.emit(SessionEvent::Error(message.into())).await;
    }

    fn cancelled(&self) -> bool {
        self.ctx.cancel.load(Ordering::SeqCst)
    }

    fn history(&self) -> Vec<ChatMessage> {
        lock(&self.ctx.history).clone()
    }

    fn has_pending(&self) -> bool {
        !lock(&self.ctx.pending).items.is_empty()
    }
}

/// Run one tool call as its own task: announce it, run it, let a
/// `tool_result` hook redact the result, then report the end.
async fn execute_tool(context: ToolRunContext, call: ToolCall) -> tools::ToolOutput {
    let id = context.id;
    let host = context.host.clone();
    let events = context.events.clone();
    let _ = events.send(SessionEvent::ToolStart { id }).await;
    if let Some(host) = &host {
        let args: serde_json::Value =
            serde_json::from_str(&call.arguments).unwrap_or(serde_json::Value::Null);
        host.event(
            "tool_start",
            serde_json::json!({"id": id, "name": call.name, "arguments": args}),
        )
        .await;
    }
    let mut output = run_tool(context, &call.name, &call.arguments).await;
    if let Some(host) = &host {
        // Redaction and trimming happen before the result is shown, stored,
        // or sent anywhere. The hook sees only `content`, so a rewrite also
        // drops the richer `display`: the viewer must never show more than
        // the hook let through.
        if host.has_hook("tool_result") {
            if let Some(content) = host
                .hook_tool_result(&call.name, &output.content, output.is_error())
                .await
            {
                if content != output.content {
                    output.display = None;
                }
                output.content = content;
            }
        }
        host.event(
            "tool_end",
            serde_json::json!({
                "id": id,
                "name": call.name,
                "outcome": format!("{:?}", output.outcome).to_lowercase(),
                "content": output.content,
            }),
        )
        .await;
    }
    let _ = events
        .send(SessionEvent::ToolEnd {
            id,
            outcome: output.outcome,
            summary: output.summary.clone(),
            // The viewer gets the rich detail (full diffs); history keeps the
            // lean content.
            content: output.display_text().to_string(),
        })
        .await;
    output
}

/// Wait for a tool task. A blocked filesystem call can't be interrupted in
/// place, so on Esc stop waiting, record the call as cancelled, and detach
/// the task: the run must end promptly even over a stalled FIFO or NFS mount.
async fn settle_tool(
    task: tokio::task::JoinHandle<tools::ToolOutput>,
    cancel: &AtomicBool,
) -> tools::ToolOutput {
    tokio::select! {
        biased;
        joined = task => joined.unwrap_or_else(|_| tools::ToolOutput {
            content: "tool panicked".into(),
            outcome: tools::ToolOutcome::Failed,
            summary: "error".into(),
            display: None,
        }),
        _ = wait_cancelled(cancel) => tools::ToolOutput {
            // Honest record: the blocked operation is only detached, so it
            // may still complete after this.
            content: "tool cancelled — the underlying operation \
                      may still complete in the background"
                .into(),
            outcome: tools::ToolOutcome::Cancelled,
            summary: "cancelled".into(),
            display: None,
        },
    }
}

/// Lock a mutex, recovering the data if a panicking holder poisoned it.
fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|ulo| ulo.into_inner())
}

/// Whole milliseconds, saturating at `u64::MAX`.
fn millis(duration: Duration) -> u64 {
    duration.as_millis().min(u64::MAX as u128) as u64
}

/// The focus a `/compact <focus>` left for the next compaction, whichever
/// path performs it: an automatic one that lands first honours it rather
/// than racing it, and the manual one then finds nothing left to do.
fn take_focus(focus: &Arc<Mutex<Option<String>>>) -> Option<String> {
    focus.lock().unwrap_or_else(|ulo| ulo.into_inner()).take()
}

/// Largest nested `AGENTS.md` that is read in full; the rest of a longer
/// file is never read at all.
const NESTED_INSTRUCTIONS_CAP: usize = 32 * 1024;

/// The first line of a nested-instructions message; the directory it names
/// is what [`instruction_dirs`] recovers from a history.
const INSTRUCTIONS_HEAD: &str = "Instructions for files under ";

/// The directories whose nested `AGENTS.md` a history already carries, so a
/// resumed, rewound, or forked session neither reloads them nor forgets
/// them. The set is the loader's own key: the lexical directory the message
/// names.
pub(super) fn instruction_dirs(messages: &[ChatMessage]) -> std::collections::HashSet<PathBuf> {
    messages
        .iter()
        .filter(|m| m.is_internal() && m.role() == "user")
        .filter_map(|m| {
            let head = m.content.lines().next()?;
            let dir = head.strip_prefix(INSTRUCTIONS_HEAD)?.strip_suffix(':')?;
            Some(PathBuf::from(dir))
        })
        .collect()
}

/// Read at most the cap plus one byte of a regular file, off the async
/// worker: a FIFO or a multi-gigabyte `AGENTS.md` must neither hang the
/// turn nor be read whole. `None` for anything that is not a regular file
/// inside `root` once links are resolved.
async fn read_instructions(file: PathBuf, root: PathBuf) -> Option<String> {
    tokio::task::spawn_blocking(move || {
        use std::io::Read as _;
        let real = file.canonicalize().ok()?;
        if !real.starts_with(&root) || !std::fs::metadata(&real).ok()?.is_file() {
            return None;
        }
        let mut bytes = Vec::new();
        std::fs::File::open(&real)
            .ok()?
            .take(NESTED_INSTRUCTIONS_CAP as u64 + 1)
            .read_to_end(&mut bytes)
            .ok()?;
        Some(String::from_utf8_lossy(&bytes).into_owned())
    })
    .await
    .ok()
    .flatten()
}

/// For every path a batch's calls named, walk from its directory up to (not
/// including) the workspace and add each `AGENTS.md` not yet loaded this
/// session as an internal user message, outermost first so the nearest
/// reads as the most specific. Only in a trusted workspace, only for paths
/// inside it — lexically, and again after resolving links, so a symlink
/// under the checkout cannot reach instructions outside it
/// (docs/guides/customize/instructions.md).
async fn load_nested_instructions(
    calls: &[ToolCall],
    cwd: &std::path::Path,
    loaded: &Arc<Mutex<std::collections::HashSet<PathBuf>>>,
    log: &TurnLog,
    events: &mpsc::Sender<SessionEvent>,
) {
    if !crate::config::trust::trusted(cwd) {
        return;
    }
    let Ok(root) = cwd.canonicalize() else {
        return;
    };
    let mut pending: Vec<PathBuf> = Vec::new();
    for call in calls {
        if !matches!(call.name.as_str(), "read" | "write" | "edit" | "grep") {
            continue;
        }
        let Ok(args) = serde_json::from_str::<serde_json::Value>(&call.arguments) else {
            continue;
        };
        let Some(path) = args.get("path").and_then(|p| p.as_str()) else {
            continue;
        };
        let target = std::path::Path::new(path);
        if target
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir))
        {
            continue;
        }
        let full = if target.is_absolute() {
            target.to_path_buf()
        } else {
            cwd.join(target)
        };
        if !full.starts_with(cwd) {
            continue;
        }
        let mut chain: Vec<PathBuf> = Vec::new();
        // A grep path is the directory searched, so its own AGENTS.md is
        // in scope; the file tools name a file, whose parent is.
        let mut dir = if call.name == "grep" {
            Some(full.as_path())
        } else {
            full.parent()
        };
        while let Some(d) = dir {
            if d == cwd || !d.starts_with(cwd) {
                break;
            }
            chain.push(d.to_path_buf());
            dir = d.parent();
        }
        for d in chain.into_iter().rev() {
            if !pending.contains(&d) {
                pending.push(d);
            }
        }
    }
    for dir in pending {
        let fresh = loaded
            .lock()
            .unwrap_or_else(|ulo| ulo.into_inner())
            .insert(dir.clone());
        if !fresh {
            continue;
        }
        let file = dir.join("AGENTS.md");
        let Some(text) = read_instructions(file.clone(), root.clone()).await else {
            continue;
        };
        let mut text = text.trim().to_string();
        if text.is_empty() {
            continue;
        }
        if text.len() > NESTED_INSTRUCTIONS_CAP {
            let mut cut = NESTED_INSTRUCTIONS_CAP;
            while !text.is_char_boundary(cut) {
                cut -= 1;
            }
            text.truncate(cut);
            text.push_str("\n… [instructions clipped at 32 KiB]");
        }
        let shown = file.display().to_string();
        let mut message = ChatMessage::user(format!(
            "{INSTRUCTIONS_HEAD}{}:\n<project_instructions path=\"{}\">\n{text}\n</project_instructions>",
            dir.display(),
            context::xml_escape(&shown)
        ));
        message.mark_internal();
        log.commit_async(message).await;
        let _ = events
            .send(SessionEvent::Instructions { path: shown })
            .await;
    }
}

/// Identify explicit filesystem targets, including aliases and new paths.
/// Metadata lookup stays off the async worker and cancellation stops waiting.
async fn file_target(
    call: &ToolCall,
    cwd: &std::path::Path,
    cancel: &AtomicBool,
) -> Option<FileTarget> {
    if !matches!(call.name.as_str(), "read" | "write" | "edit") {
        return None;
    }
    if cancel.load(Ordering::SeqCst) {
        return None;
    }
    let args: serde_json::Value = serde_json::from_str(&call.arguments).ok()?;
    let path = cwd.join(args.get("path")?.as_str()?);
    let lookup = tokio::task::spawn_blocking(move || {
        let path = tools::stable_path_key(&path);
        #[cfg(unix)]
        if let Ok(metadata) = std::fs::metadata(&path) {
            use std::os::unix::fs::MetadataExt;
            return FileTarget::Inode(metadata.dev(), metadata.ino());
        }
        FileTarget::Path(path)
    });
    tokio::select! {
        result = lookup => result.ok(),
        _ = wait_cancelled(cancel) => None,
    }
}

/// Existing Unix aliases share an inode; missing targets share a resolved path.
#[derive(PartialEq, Eq, Hash)]
enum FileTarget {
    Path(PathBuf),
    #[cfg(unix)]
    Inode(u64, u64),
}
