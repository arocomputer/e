//! Session-event handling: the one ordered stream from the agent —
//! text, thinking, tool lifecycle, usage, retries — projected onto App
//! state and the transcript.
//!
//! [`App::on_session_event`] is the table of contents: one arm per event,
//! each handing off to a named handler. Events that arrive with no turn
//! open (a late tool, an idle /compact) touch only what they can.

use super::*;
use ulo_core::agent::ToolCallPresentation;
use ulo_core::providers::catalog::Pricing;

impl App {
    /// The single session stream, in order. Turn bookkeeping hangs off it.
    pub(super) fn on_session_event(&mut self, event: SessionEvent) {
        match event {
            SessionEvent::Discarded(prompts) => {
                for text in prompts {
                    self.notice(format!(
                        "queued message discarded when the run stopped: {text}"
                    ));
                }
            }
            SessionEvent::Compacting => self.on_compacting(),
            SessionEvent::Compacted {
                summary,
                context_tokens,
                response,
                pricing,
            } => self.on_compacted(summary, context_tokens, response.usage, pricing),
            SessionEvent::TurnStart => self.active = Some(ActiveTurn::new()),
            SessionEvent::Steered(text) => self.on_steered(text),
            SessionEvent::TextDelta(delta) => self.on_text(&delta),
            SessionEvent::ReasoningDelta(delta) => self.on_reasoning(&delta),
            // The model is streaming tool-call arguments. The tool row
            // appears only when the complete call starts executing.
            SessionEvent::ToolCallAssembly { bytes: _ } => self.set_phase(TurnPhase::ToolCall),
            SessionEvent::ToolBatchStart { calls } => self.on_tool_batch(calls),
            SessionEvent::ToolStart { id } => self.on_tool_start(id),
            SessionEvent::ToolOutput { id, chunk, .. } => self.on_tool_output(id, &chunk),
            SessionEvent::Instructions { path } => {
                self.notice(format!("loaded instructions from {path}"));
            }
            SessionEvent::Named(name) => {
                self.agent.set_session_name(name.clone());
                self.notice(format!("session: {name}"));
                set_tab_title(&tab_title(&title_path(), Some(&name)));
            }
            SessionEvent::ToolEnd {
                id,
                outcome,
                summary,
                content,
            } => self.on_tool_end(id, outcome, summary, content),
            SessionEvent::Usage { usage, pricing } => self.on_usage(usage, pricing),
            SessionEvent::Retry {
                attempt,
                limit,
                delay_secs,
                cause,
                reason,
            } => self.on_retry(RetryStatus {
                attempt,
                limit,
                delay_secs,
                since: Instant::now(),
                cause,
                reason,
            }),
            SessionEvent::Recovered { attempt, limit } => self.on_recovered(attempt, limit),
            SessionEvent::ErrorDetails(details) => {
                if let Some(s) = &mut self.active {
                    s.error_summary = Some(details.summary);
                }
            }
            SessionEvent::Error(message) => self.on_error(message),
            SessionEvent::Warning(message) => {
                self.notice(format!("warning: {message}"));
            }
            SessionEvent::Slept { duration_secs } => {
                // The device slept mid-run and woke inside the window: say
                // so where the work happened, then the continuation follows
                // as its own user turn.
                self.transcript.push(Block::new(
                    Kind::System,
                    format!(
                        "the device was asleep for {} — continuing",
                        ulo_core::output::format_elapsed(duration_secs)
                    ),
                ));
            }
            SessionEvent::SleepStopped { duration_secs } => self.on_sleep_stopped(duration_secs),
            SessionEvent::TurnEnd { aborted } => self.on_turn_end(aborted),
        }
    }

    /// Set the open turn's activity phase; no turn, no change.
    fn set_phase(&mut self, phase: TurnPhase) {
        if let Some(s) = &mut self.active {
            s.turn.phase = phase;
        }
    }

    /// Compaction began: the live bursts end where they are.
    fn on_compacting(&mut self) {
        self.compacting = true;
        self.end_thinking_burst();
        self.end_assistant_burst();
        // Mid-turn the activity row says so instead of "Thinking";
        // idle (a /compact between turns) the notice is the record.
        match &mut self.active {
            Some(s) => s.turn.phase = TurnPhase::Compacting,
            None => self.notice("compacting…".into()),
        }
    }

    /// Compaction landed: the new context size, its cost, and the summary.
    fn on_compacted(
        &mut self,
        summary: String,
        context_tokens: u64,
        usage: Option<ulo_core::providers::Usage>,
        pricing: Option<Pricing>,
    ) {
        self.compacting = false;
        self.context_tokens = context_tokens;
        if let Some(s) = &mut self.active {
            if s.turn.phase == TurnPhase::Compacting {
                s.turn.phase = TurnPhase::Waiting;
            }
        }
        if let (Some(usage), Some(active), Some(pricing)) =
            (usage, self.active.as_mut(), pricing.as_ref())
        {
            *active.cost_usd.get_or_insert(0.0) += pricing.estimate(usage);
        }
        // Compaction changes model context, not the user's scrollback.
        // Keep prior tool details and their live block references valid.
        self.transcript.push(Block::new(
            Kind::Notice,
            "compacted — recent messages kept, the full session is under /resume",
        ));
        self.transcript.push(Block::new(Kind::Summary, summary));
    }

    /// A mid-turn message: show it as a user turn where it landed.
    fn on_steered(&mut self, text: String) {
        self.transcript.push(Block::new(Kind::User, text));
        self.set_phase(TurnPhase::Waiting);
        // The next assistant text opens a fresh block; the burst
        // that was live keeps its source and display mode.
        self.end_thinking_burst();
        self.end_assistant_burst();
    }

    /// Reply text streams into the turn's open assistant block.
    fn on_text(&mut self, delta: &str) {
        // Reply text starting ends the live thinking burst — the
        // thought stays above the reply; the next burst,
        // if any, opens its own block.
        self.end_thinking_burst();
        let Some(s) = &mut self.active else { return };
        s.turn.phase = TurnPhase::AssistantText;
        // Model output is untrusted: strip control sequences
        // before it can reach the paint stream. (The raw text
        // still goes to the model's own history in core.)
        let idx = open_block(&mut self.transcript, &mut s.block, Kind::Assistant);
        if let Some(block) = self.transcript.blocks.get_mut(idx) {
            block.append_streaming(delta);
        }
    }

    /// Retain reasoning even when collapsed so review and later expansion
    /// can reveal it. Block ingestion sanitizes provider text.
    fn on_reasoning(&mut self, delta: &str) {
        let Some(s) = &mut self.active else { return };
        s.turn.phase = TurnPhase::Thinking;
        let idx = open_block(&mut self.transcript, &mut s.thinking_block, Kind::Thinking);
        if let Some(block) = self.transcript.blocks.get_mut(idx) {
            block.collapsed = (!self.show_thinking).then(|| self.thinking_hint.clone());
            block.append_streaming(delta);
        }
    }

    /// A tool batch starts: its calls join a tool group as pending rows.
    fn on_tool_batch(&mut self, calls: Vec<ToolCallPresentation>) {
        // End the pre-batch reasoning where it sits. A tool tree
        // continues only when no reply or retained thinking
        // separates this batch from the previous one.
        self.end_thinking_burst();
        self.end_assistant_burst();
        let Some(s) = &mut self.active else { return };
        s.turn.phase = TurnPhase::Tool;
        s.pending_tools += calls.len();
        let children = calls
            .iter()
            .map(|call| {
                crate::transcript::ToolChild::pending(
                    call.id,
                    call.category.clone(),
                    call.running.clone(),
                    call.completed.clone(),
                    call.target.clone(),
                )
            })
            .collect();
        let idx = self.transcript.extend_tool_group(children);
        let block = &mut self.transcript.blocks[idx];
        block.live_preview_rows = self.live_preview_rows;
        block.tool_label_rows = self.tool_label_rows;
        block.tool_history_limit = self.tool_history_limit;
        block.tool_history_hint = self.tool_history_hint.clone();
        for call in calls {
            s.tool_blocks.insert(call.id, idx);
            s.tool_names.insert(call.id, call.name.clone());
        }
    }

    /// A pending tool of this turn begins running.
    fn on_tool_start(&mut self, id: u64) {
        let Some(s) = &mut self.active else { return };
        let Some(&idx) = s.tool_blocks.get(&id) else {
            return;
        };
        s.turn.phase = TurnPhase::Tool;
        if let Some(block) = self.transcript.blocks.get_mut(idx) {
            block.start_tool(id);
        }
    }

    /// Live output from one of this turn's tools.
    fn on_tool_output(&mut self, id: u64, chunk: &str) {
        let Some(&idx) = self.active.as_ref().and_then(|s| s.tool_blocks.get(&id)) else {
            return;
        };
        if let Some(block) = self.transcript.blocks.get_mut(idx) {
            block.append_tool_output(id, chunk);
        }
    }

    /// A tool finished: settle its row, then store any output for the
    /// review screen and offer it to rendering extensions.
    fn on_tool_end(
        &mut self,
        id: u64,
        outcome: ulo_core::tools::ToolOutcome,
        summary: String,
        content: String,
    ) {
        // Detached tools can finish after a new turn has started.
        // Their events must not change that turn or its saved outputs.
        let Some(s) = &mut self.active else { return };
        let Some(&idx) = s.tool_blocks.get(&id) else {
            return;
        };
        let mut title = None;
        if let Some(block) = self.transcript.blocks.get_mut(idx) {
            title = block
                .tool_children
                .iter()
                .find(|child| child.id == id)
                .map(|child| {
                    if child.target.is_empty() {
                        child.completed.clone()
                    } else {
                        format!("{} {}", child.completed, child.target)
                    }
                });
            block.finish_tool(id, outcome, summary, &content);
        }
        s.pending_tools = s.pending_tools.saturating_sub(1);
        s.turn.phase = if s.pending_tools == 0 {
            TurnPhase::Waiting
        } else {
            TurnPhase::Tool
        };
        let name = s.tool_names.get(&id).cloned().unwrap_or_default();
        if content.trim().is_empty() {
            return;
        }
        let detail = self.remember_output(
            title.unwrap_or_else(|| "tool output".into()),
            ulo_core::tools::sanitize_display(&content),
        );
        // An extension that renders this tool's results gets
        // the stored output to rewrite.
        self.request_render(
            &format!("tool:{name}"),
            &name,
            &content,
            RenderTarget::Tool(detail),
        );
        // Link the stored detail to its row for the review
        // screen.
        if let Some(child) = self
            .transcript
            .blocks
            .get_mut(idx)
            .and_then(|block| block.tool_children.iter_mut().find(|child| child.id == id))
        {
            child.detail = Some(detail);
        }
    }

    /// A usage frame: the context gauge, the turn's cost, and its tokens.
    fn on_usage(&mut self, usage: ulo_core::providers::Usage, pricing: Option<Pricing>) {
        let prompt = usage.prompt_tokens();
        self.context_tokens = prompt.saturating_add(usage.output);
        let Some(s) = &mut self.active else { return };
        if let Some(pricing) = pricing {
            *s.cost_usd.get_or_insert(0.0) += pricing.estimate(usage);
        }
        // Every step resends the whole context, so prompt size is
        // latest-wins while generated output accumulates.
        s.turn.note_usage(prompt, usage.output);
    }

    /// A retry replaces the activity row in place — a live status, not a
    /// scrollback notice: it's transient by nature and would otherwise
    /// leave one permanent line per attempt behind.
    fn on_retry(&mut self, retry: RetryStatus) {
        if let Some(s) = &mut self.active {
            s.turn.phase = TurnPhase::Retrying;
            s.turn.retry = Some(retry);
            s.turn.recovered = None;
        }
        // The abandoned attempt's thinking burst ends where it sits;
        // the retry streams a fresh burst.
        self.end_thinking_burst();
    }

    /// A retry worked: the activity row says so briefly.
    fn on_recovered(&mut self, attempt: u32, limit: u32) {
        let Some(s) = &mut self.active else { return };
        s.turn.phase = TurnPhase::Waiting;
        s.turn.retry = None;
        s.turn.recovered = Some(RecoveredStatus {
            attempt,
            limit,
            since: Instant::now(),
        });
    }

    /// A turn's error waits for its end (preferring the classified
    /// summary); an error outside a turn is a notice now.
    fn on_error(&mut self, message: String) {
        if let Some(s) = &mut self.active {
            s.error = Some(s.error_summary.take().unwrap_or(message));
        } else {
            self.notice(format!("error: {message}"));
        }
    }

    /// Past the resume window: a stop in the cancelled family.
    /// The TurnEnd row is suppressed; this line is the record.
    fn on_sleep_stopped(&mut self, duration_secs: u64) {
        if let Some(s) = &mut self.active {
            s.sleep_stopped = true;
        }
        self.transcript.push(Block::new(
            Kind::System,
            format!(
                "run stopped — the device was asleep for {}",
                ulo_core::output::format_elapsed(duration_secs)
            ),
        ));
    }

    /// The turn ended: settle what it left open, write its trailer and any
    /// error, and release or discard prompts the frontend held.
    fn on_turn_end(&mut self, aborted: bool) {
        self.compacting = false;
        // The queue the review was editing died with the turn.
        self.close_queue_review();
        if aborted {
            self.cancel_unfinished_tools();
        }
        // The turn's final burst ends with it — same moment, not
        // early. Every other burst end (reply text, tools, retry,
        // steer) ended where it happened; this catches the one that
        // ran to the turn's end.
        self.end_thinking_burst();
        self.end_assistant_burst();
        let Some(s) = self.active.take() else { return };
        // The reference grammar: a completed turn ends with a dim
        // duration-and-tokens row; a cancelled one says so instead —
        // unless the sleep stop already said it its own way.
        if aborted && !s.sleep_stopped {
            self.transcript.push(Block::new(Kind::System, "cancelled"));
        } else if !aborted {
            self.transcript
                .push(Block::new(Kind::Summary, turn_trailer(&s)));
        }
        if let Some(message) = s.error {
            // A failed turn ends visibly: the error persists in error
            // color below the trailer, never a vanishing status blip.
            self.transcript.push(Block::new(Kind::Error, message));
        }
        // Release prompts held by frontend work such as shell passthrough.
        // Prompts queued in the agent are consumed by the core itself.
        for text in std::mem::take(&mut self.held_prompts) {
            if aborted {
                self.notice(format!("queued message discarded by Esc: {text}"));
            } else {
                self.prompt(text);
            }
        }
    }

    /// Every started or pending member reaches a terminal state; no ghost
    /// Running row survives Esc.
    fn cancel_unfinished_tools(&mut self) {
        for block in &mut self.transcript.blocks {
            if block.kind == Kind::ToolGroup {
                block.cancel_unfinished_tools();
            } else if block.kind == Kind::Tool && !block.done {
                block.cancelled = true;
                block.touch();
            }
        }
    }
}

impl ActiveTurn {
    /// A turn that just started: nothing streamed, no tools, no cost yet.
    fn new() -> Self {
        ActiveTurn {
            block: None,
            thinking_block: None,
            turn: Turn::new(),
            started: Instant::now(),
            error: None,
            error_summary: None,
            sleep_stopped: false,
            tool_blocks: std::collections::HashMap::new(),
            tool_names: std::collections::HashMap::new(),
            pending_tools: 0,
            // The first usage event supplies the request model's rates;
            // the selected model may change while that request is live.
            cost_usd: None,
        }
    }
}

/// A completed turn's trailer: its duration, then its tokens and cost when
/// it has them.
fn turn_trailer(s: &ActiveTurn) -> String {
    let tokens = if s.turn.input == 0 && s.turn.output == 0 {
        String::new()
    } else {
        format!(
            " (↑{} ↓{})",
            format_tokens(s.turn.input),
            format_tokens(s.turn.output)
        )
    };
    let cost = s
        .cost_usd
        .filter(|cost| *cost > 0.0)
        .map(|cost| format!(" {}", ulo_core::output::format_cost(cost)))
        .unwrap_or_default();
    format!(
        "{}{}{}",
        format_duration(s.started.elapsed().as_millis() as u64),
        tokens,
        cost
    )
}

/// The currently open block for `index`, or a freshly started one — the
/// same shape `TextDelta`'s assistant block and `ReasoningDelta`'s thinking
/// block both need, each with a different `kind`.
fn open_block(transcript: &mut Transcript, index: &mut Option<usize>, kind: Kind) -> usize {
    match *index {
        Some(idx) => idx,
        None => {
            let idx = transcript.push(Block::new(kind, ""));
            *index = Some(idx);
            idx
        }
    }
}

impl App {
    /// End a thinking burst without changing its height or discarding its
    /// source. The next burst gets its own block, whether thinking is
    /// expanded or collapsed.
    pub(super) fn end_thinking_burst(&mut self) {
        let Some(index) = self
            .active
            .as_mut()
            .and_then(|turn| turn.thinking_block.take())
        else {
            return;
        };
        if let Some(block) = self.transcript.blocks.get_mut(index) {
            block.finish_streaming();
        }
    }

    /// Seal the current reply segment so its final delta renders immediately.
    fn end_assistant_burst(&mut self) {
        let Some(index) = self.active.as_mut().and_then(|turn| turn.block.take()) else {
            return;
        };
        let mut finished = None;
        if let Some(block) = self.transcript.blocks.get_mut(index) {
            block.finish_streaming();
            finished = Some(block.text.clone());
        }
        // A completed reply is an entry an extension may render.
        if let Some(text) = finished {
            self.request_render(
                "assistant",
                "",
                &text,
                RenderTarget::Assistant {
                    index,
                    len: text.len(),
                },
            );
        }
    }
}
