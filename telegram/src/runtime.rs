//! The durable turn loop and its supporting pieces.
//!
//! [`run_agent_loop`] is the orchestrator that replaces a terminal agent's
//! `main()` loop body. It is generic over any [`claudius::Agent`] and any
//! [`ChatTransport`]. The load-bearing ordering invariant is: the conversation
//! and offset commit must land *before* the transport ack, and the loop never
//! exits between commit and ack.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tokio::sync::Mutex;

use claudius::{
    Agent, Anthropic, Budget, ContentBlock, MessageParam, MessageParamContent, MessageRole,
    Renderer, StopReason, StreamContext, TurnOutcome,
};

use crate::state::{OutboxRecord, OutboxStatus, StateStore, TransportState};
use crate::transport::{ChatTransport, Inbound, content_blocks_have_text, text_content};
use crate::{ChatId, Error, MessageId};

/////////////////////////////////////// BufferingRenderer ///////////////////////////////////////

/// A [`Renderer`] that accumulates streamed assistant text instead of writing to
/// stdout.
///
/// Chunking to Telegram's size limit happens in the transport's `send`, not here.
#[derive(Default)]
pub struct BufferingRenderer {
    buffer: String,
    last_error: Option<String>,
    debug: bool,
}

impl BufferingRenderer {
    /// Creates an empty buffering renderer.
    pub fn new() -> Self {
        Self::default()
    }

    /// Enables echoing streamed text and errors to stderr (for local runs).
    pub fn with_debug(mut self, debug: bool) -> Self {
        self.debug = debug;
        self
    }

    /// Drains and returns the accumulated turn output.
    pub fn take(&mut self) -> String {
        std::mem::take(&mut self.buffer)
    }

    /// Returns the most recent error recorded during the turn, if any.
    pub fn last_error(&self) -> Option<&str> {
        self.last_error.as_deref()
    }
}

impl Renderer for BufferingRenderer {
    fn print_text(&mut self, _context: &dyn StreamContext, text: &str) {
        if self.debug {
            eprint!("{text}");
        }
        self.buffer.push_str(text);
    }

    fn print_thinking(&mut self, _context: &dyn StreamContext, _text: &str) {
        // Thinking is not surfaced to chat in v1.
    }

    fn print_error(&mut self, _context: &dyn StreamContext, error: &str) {
        if self.debug {
            eprintln!("[error] {error}");
        }
        self.last_error = Some(error.to_string());
    }

    fn print_info(&mut self, _context: &dyn StreamContext, _info: &str) {}

    fn start_tool_use(&mut self, _context: &dyn StreamContext, _name: &str, _id: &str) {}

    fn print_tool_input(&mut self, _context: &dyn StreamContext, _partial_json: &str) {}

    fn finish_tool_use(&mut self, _context: &dyn StreamContext) {}

    fn start_tool_result(
        &mut self,
        _context: &dyn StreamContext,
        _tool_use_id: &str,
        _is_error: bool,
    ) {
    }

    fn print_tool_result_text(&mut self, _context: &dyn StreamContext, _text: &str) {}

    fn finish_tool_result(&mut self, _context: &dyn StreamContext) {}

    fn finish_response(&mut self, _context: &dyn StreamContext) {
        // Record terminal state without writing to stdout.
    }
}

/////////////////////////////////////////// PreFilter ///////////////////////////////////////////

/// The result of an inbound pre-filter.
///
/// Lets the binary intercept slash-commands (or, for the echo smoke test, every
/// message) before the agent is invoked.
pub enum PreFilter {
    /// The message was handled; send this content and skip the agent turn.
    Handled(Vec<ContentBlock>),
    /// Pass the message through to the agent.
    PassToAgent,
}

impl PreFilter {
    /// Builds a handled pre-filter response from plain text.
    pub fn handled_text(text: impl Into<String>) -> Self {
        Self::Handled(text_content(text))
    }
}

/////////////////////////////////////////// LoopConfig ///////////////////////////////////////////

/// An inbound pre-filter: maps an [`Inbound`] to a [`PreFilter`] decision.
///
/// Used for slash-command interception and the echo smoke-test bypass.
pub type PreFilterFn = Box<dyn Fn(&Inbound) -> PreFilter + Send>;

/// Configuration for [`run_agent_loop`].
pub struct LoopConfig {
    /// The durable store. Must be the same store the transport was built with.
    pub store: Arc<dyn StateStore>,
    /// The shared state. Must be the same `Arc` the transport was built with.
    pub state: Arc<Mutex<TransportState>>,
    /// The per-turn token/cost budget passed to the agent.
    pub budget: Arc<Budget>,
    /// `false` for a reactive agent (duplicates harmless); `true` to route sends
    /// through the transactional outbox (needed once proactive sends occur).
    pub use_outbox: bool,
    /// Set on SIGINT/EOF; checked at the top of each iteration and after `recv`.
    pub interrupted: Arc<AtomicBool>,
    /// Optional inbound pre-filter (slash-commands, echo bypass, ...).
    pub pre_filter: Option<PreFilterFn>,
    /// Reset a chat's model-visible history before an agent turn when its
    /// persisted message count exceeds this threshold.
    ///
    /// The current inbound user message is retained so the agent can still
    /// answer the active request. A value of `Some(0)` resets before every
    /// agent turn.
    pub max_history_messages: Option<usize>,
    /// On context-length failures, reset the chat history to the current user
    /// message and retry the agent turn once.
    pub reset_on_context_limit: bool,
}

impl LoopConfig {
    /// Builds a config with sensible defaults (no outbox, no pre-filter).
    pub fn new(
        store: Arc<dyn StateStore>,
        state: Arc<Mutex<TransportState>>,
        budget: Arc<Budget>,
        interrupted: Arc<AtomicBool>,
    ) -> Self {
        Self {
            store,
            state,
            budget,
            use_outbox: false,
            interrupted,
            pre_filter: None,
            max_history_messages: None,
            reset_on_context_limit: false,
        }
    }
}

////////////////////////////////////// SyntheticTurnConfig //////////////////////////////////////

/// Configuration for [`run_synthetic_user_turn`].
#[derive(Debug, Clone)]
pub struct SyntheticTurnConfig {
    /// Reset a chat's model-visible history before the synthetic agent turn when
    /// its persisted message count exceeds this threshold.
    ///
    /// The synthetic user message is retained so the agent can still answer the
    /// proactive prompt. A value of `Some(0)` resets before every turn.
    pub max_history_messages: Option<usize>,
    /// On context-length failures, reset the chat history to the synthetic user
    /// message and retry the agent turn once.
    pub reset_on_context_limit: bool,
    /// When true, commit the assistant response to the transactional outbox
    /// before sending it.
    pub use_outbox: bool,
    /// Optional operational label included in diagnostic log messages.
    pub source_label: Option<String>,
}

impl Default for SyntheticTurnConfig {
    fn default() -> Self {
        Self {
            max_history_messages: None,
            reset_on_context_limit: false,
            use_outbox: true,
            source_label: None,
        }
    }
}

/////////////////////////////////////////// helpers ///////////////////////////////////////////

/// Returns the chats this transport knows about (the prerequisite for any
/// proactive send -- recall Telegram forbids initiating a conversation).
pub fn known_chats(state: &TransportState) -> Vec<ChatId> {
    state.conversations.keys().copied().map(ChatId).collect()
}

fn build_messages(state: &TransportState, chat: ChatId) -> Vec<MessageParam> {
    state
        .conversation(chat)
        .map(|messages| messages.to_vec())
        .unwrap_or_default()
}

fn content_to_blocks(content: &MessageParamContent) -> Vec<ContentBlock> {
    match content {
        MessageParamContent::String(text) => text_content(text.clone()),
        MessageParamContent::Array(blocks) => blocks.clone(),
    }
}

fn assistant_content(messages: &[MessageParam]) -> Vec<ContentBlock> {
    messages
        .iter()
        .filter(|message| message.role == MessageRole::Assistant)
        .flat_map(|message| content_to_blocks(&message.content))
        .collect()
}

fn latest_user_message(messages: &[MessageParam]) -> Option<MessageParam> {
    messages
        .iter()
        .rev()
        .find(|message| message.role == MessageRole::User)
        .cloned()
}

fn reset_to_latest_user_message(messages: &mut Vec<MessageParam>) -> bool {
    let Some(message) = latest_user_message(messages) else {
        return false;
    };
    messages.clear();
    messages.push(message);
    true
}

fn reset_history_if_over_threshold(
    messages: &mut Vec<MessageParam>,
    threshold: usize,
) -> Option<usize> {
    let previous_len = messages.len();
    if previous_len <= threshold {
        return None;
    }
    reset_to_latest_user_message(messages).then_some(previous_len)
}

fn is_context_length_error(err: &claudius::Error) -> bool {
    fn text_matches(text: &str) -> bool {
        let lower = text.to_ascii_lowercase();
        (lower.contains("context") && lower.contains("long"))
            || lower.contains("context window")
            || lower.contains("model_context_window_exceeded")
            || lower.contains("prompt is too long")
            || lower.contains("messages is too long")
            || lower.contains("input is too long")
            || lower.contains("too many tokens")
            || lower.contains("token limit")
    }

    match err {
        claudius::Error::BadRequest { message, param } => {
            text_matches(message) || param.as_deref().is_some_and(text_matches)
        }
        claudius::Error::Validation { message, param } => {
            text_matches(message) || param.as_deref().is_some_and(text_matches)
        }
        claudius::Error::Api {
            status_code,
            error_type,
            message,
            ..
        } => {
            *status_code == 400
                && (text_matches(message) || error_type.as_deref().is_some_and(text_matches))
        }
        claudius::Error::Streaming { message, .. } => text_matches(message),
        _ => text_matches(&err.to_string()),
    }
}

async fn take_agent_turn_with_reset<A: Agent>(
    agent: &mut A,
    client: &Anthropic,
    messages: &mut Vec<MessageParam>,
    budget: &Arc<Budget>,
    reset_on_context_limit: bool,
) -> Result<(usize, TurnOutcome), claudius::Error> {
    let turn_start = messages.len();
    if !reset_on_context_limit {
        let mut renderer = BufferingRenderer::new();
        let outcome = agent
            .take_turn_streaming_root(client, messages, budget, &mut renderer)
            .await?;
        return Ok((turn_start, outcome));
    }

    let Some(current_user_message) = latest_user_message(messages) else {
        let mut renderer = BufferingRenderer::new();
        let outcome = agent
            .take_turn_streaming_root(client, messages, budget, &mut renderer)
            .await?;
        return Ok((turn_start, outcome));
    };

    let mut renderer = BufferingRenderer::new();
    let first = agent
        .take_turn_streaming_root(client, messages, budget, &mut renderer)
        .await;

    match first {
        Ok(outcome) if outcome.stop_reason != StopReason::ModelContextWindowExceeded => {
            Ok((turn_start, outcome))
        }
        Ok(_) => {
            eprintln!("[telegram] context window exceeded; resetting chat history and retrying");
            messages.clear();
            messages.push(current_user_message);
            let retry_turn_start = messages.len();
            let mut renderer = BufferingRenderer::new();
            let outcome = agent
                .take_turn_streaming_root(client, messages, budget, &mut renderer)
                .await?;
            Ok((retry_turn_start, outcome))
        }
        Err(err) if is_context_length_error(&err) => {
            eprintln!(
                "[telegram] context-length error; resetting chat history and retrying: {err}"
            );
            messages.clear();
            messages.push(current_user_message);
            let retry_turn_start = messages.len();
            let mut renderer = BufferingRenderer::new();
            let outcome = agent
                .take_turn_streaming_root(client, messages, budget, &mut renderer)
                .await?;
            Ok((retry_turn_start, outcome))
        }
        Err(err) => Err(err),
    }
}

/// Drives a single `Pending` outbox record to `Sent`: sends it, then marks it and
/// commits. Returns the sent message id. On `403` marks the chat dead, commits,
/// and returns the error.
async fn drive_outbox_record(
    transport: &(impl ChatTransport + ?Sized),
    store: &dyn StateStore,
    state: &Arc<Mutex<TransportState>>,
    logical_id: crate::state::LogicalId,
) -> Result<MessageId, Error> {
    // Read the record's chat/content without holding the lock across the send.
    let (chat, content) = {
        let st = state.lock().await;
        let rec = st
            .outbox
            .iter()
            .find(|r| r.logical_id == logical_id && r.status == OutboxStatus::Pending)
            .ok_or_else(|| Error::Internal("outbox record not found or not pending".to_string()))?;
        (rec.chat, rec.content.clone())
    };

    match transport.send(chat, content).await {
        Ok(message_id) => {
            let snapshot = {
                let mut st = state.lock().await;
                if let Some(rec) = st.outbox.iter_mut().find(|r| r.logical_id == logical_id) {
                    rec.status = OutboxStatus::Sent {
                        message_id: message_id.0,
                    };
                }
                st.clone()
            };
            store.commit(&snapshot).await?;
            Ok(message_id)
        }
        Err(err @ Error::Api { code: 403, .. }) => {
            let snapshot = {
                let mut st = state.lock().await;
                st.mark_dead(chat);
                st.clone()
            };
            store.commit(&snapshot).await?;
            eprintln!("[telegram] 403 dead chat_id={chat}; marked dead");
            Err(err)
        }
        Err(other) => Err(other),
    }
}

async fn send_direct_and_mark_dead(
    transport: &(impl ChatTransport + ?Sized),
    store: &dyn StateStore,
    state: &Arc<Mutex<TransportState>>,
    chat: ChatId,
    content: Vec<ContentBlock>,
) -> Result<MessageId, Error> {
    match transport.send(chat, content).await {
        Ok(message_id) => Ok(message_id),
        Err(err @ Error::Api { code: 403, .. }) => {
            let snapshot = {
                let mut st = state.lock().await;
                st.mark_dead(chat);
                st.clone()
            };
            store.commit(&snapshot).await?;
            eprintln!("[telegram] 403 dead chat_id={chat}; marked dead");
            Err(err)
        }
        Err(other) => Err(other),
    }
}

/// Sends `content` to `chat` independently of any inbound message, via the outbox.
///
/// Respects `dead_chats` (returns an error without attempting a send) and records
/// the intent durably before sending. This is the §9 hook the downstream
/// proactive check-in scheduler calls on a timer.
pub async fn send_proactive(
    transport: &dyn ChatTransport,
    store: &dyn StateStore,
    state: &Arc<Mutex<TransportState>>,
    chat: ChatId,
    content: Vec<ContentBlock>,
) -> Result<MessageId, Error> {
    let logical_id = {
        let mut st = state.lock().await;
        if st.is_dead(chat) {
            return Err(Error::Api {
                code: 403,
                description: format!("chat {chat} is marked dead"),
                retry_after: None,
            });
        }
        let record = OutboxRecord::pending(chat, content);
        let id = record.logical_id;
        st.outbox.push(record);
        let snapshot = st.clone();
        drop(st);
        store.commit(&snapshot).await?;
        id
    };
    drive_outbox_record(transport, store, state, logical_id).await
}

/// Runs a model-generated synthetic user turn for a known chat.
///
/// This is the proactive-agent counterpart to one inbound Telegram update: it
/// appends `user_text` as a real user message, runs the supplied agent through
/// the same native history/reset path as [`run_agent_loop`], commits the full
/// conversation turn, then delivers the assistant text directly or through the
/// transactional outbox according to `config.use_outbox`.
///
/// The helper refuses dead chats with a `403`-shaped [`Error::Api`] and refuses
/// unknown chats with [`Error::Internal`]. On transient send failure with
/// `use_outbox` enabled, the pending outbox record remains durable for a later
/// retry.
#[allow(clippy::too_many_arguments)]
pub async fn run_synthetic_user_turn<A, T>(
    agent: &mut A,
    transport: &T,
    client: &Anthropic,
    store: &dyn StateStore,
    state: &Arc<Mutex<TransportState>>,
    budget: &Arc<Budget>,
    chat: ChatId,
    user_text: String,
    config: SyntheticTurnConfig,
) -> Result<(), Error>
where
    A: Agent,
    T: ChatTransport + Sync + ?Sized,
{
    let source = config.source_label.as_deref().unwrap_or("synthetic");
    let mut messages = {
        let st = state.lock().await;
        if st.is_dead(chat) {
            return Err(Error::Api {
                code: 403,
                description: format!("chat {chat} is marked dead"),
                retry_after: None,
            });
        }
        if st.conversation(chat).is_none() {
            return Err(Error::Internal(format!(
                "cannot run synthetic turn for unknown chat {chat}"
            )));
        }
        let mut messages = build_messages(&st, chat);
        messages.push(MessageParam::new(
            MessageParamContent::String(user_text),
            MessageRole::User,
        ));
        messages
    };

    if let Some(threshold) = config.max_history_messages
        && let Some(previous_len) = reset_history_if_over_threshold(&mut messages, threshold)
    {
        eprintln!(
            "[telegram] reset chat history for chat_id={chat}; source={source}; \
             previous messages={previous_len}, threshold={threshold}"
        );
    }

    transport.typing(chat).await.ok();
    let (turn_start, _outcome) = take_agent_turn_with_reset(
        agent,
        client,
        &mut messages,
        budget,
        config.reset_on_context_limit,
    )
    .await?;
    let send_content = assistant_content(&messages[turn_start..]);

    let outbox_id = {
        let mut st = state.lock().await;
        if st.is_dead(chat) {
            return Err(Error::Api {
                code: 403,
                description: format!("chat {chat} is marked dead"),
                retry_after: None,
            });
        }
        st.set_conversation(chat, messages);
        let id = if config.use_outbox && content_blocks_have_text(&send_content) {
            let record = OutboxRecord::pending(chat, send_content.clone());
            let id = record.logical_id;
            st.outbox.push(record);
            Some(id)
        } else {
            None
        };
        let snapshot = st.clone();
        drop(st);
        store.commit(&snapshot).await?;
        id
    };

    if let Some(id) = outbox_id {
        drive_outbox_record(transport, store, state, id).await?;
    } else if content_blocks_have_text(&send_content) {
        send_direct_and_mark_dead(transport, store, state, chat, send_content).await?;
    }

    Ok(())
}

/////////////////////////////////////////// the loop ///////////////////////////////////////////

/// Runs the durable turn loop until interrupted or the transport signals EOF.
///
/// Per inbound message (serialized per chat):
/// 1. record first contact and load the chat's history,
/// 2. apply the optional pre-filter,
/// 3. run the agent turn into a [`BufferingRenderer`] (unless pre-filtered),
/// 4. **commit** the updated conversation (offset not yet advanced),
/// 5. send (directly, or via the outbox when `use_outbox`),
/// 6. **ack**, advancing and persisting the offset.
///
/// The loop never exits between commit and ack.
pub async fn run_agent_loop<A, T>(
    mut agent: A,
    mut transport: T,
    client: Anthropic,
    config: LoopConfig,
) -> Result<(), Error>
where
    A: Agent,
    T: ChatTransport + Sync,
{
    let LoopConfig {
        store,
        state,
        budget,
        use_outbox,
        interrupted,
        pre_filter,
        max_history_messages,
        reset_on_context_limit,
    } = config;

    loop {
        if interrupted.load(Ordering::SeqCst) {
            break;
        }

        let batch = transport.recv().await?;

        if interrupted.load(Ordering::SeqCst) {
            break;
        }
        if batch.is_empty() {
            continue;
        }

        for inbound in batch {
            if !transport.is_user_allowed(inbound.from_user_id) {
                eprintln!(
                    "[telegram] rejecting update_id={} from user_id={:?}; not in allow-list",
                    inbound.update_id, inbound.from_user_id
                );
                transport.ack(inbound.update_id).await?;
                continue;
            }

            let chat = inbound.chat;
            let update_id = inbound.update_id;

            // 1. First contact + record the user's message, then build context.
            let mut messages = {
                let mut st = state.lock().await;
                st.note_chat(chat);
                st.push_message(
                    chat,
                    MessageParam::new(
                        MessageParamContent::String(inbound.text.clone()),
                        MessageRole::User,
                    ),
                );
                build_messages(&st, chat)
            };
            // 2. Pre-filter.
            let decision = pre_filter
                .as_ref()
                .map(|f| f(&inbound))
                .unwrap_or(PreFilter::PassToAgent);

            // 3. Produce the assistant content.
            let send_content = match decision {
                PreFilter::Handled(content) => {
                    messages.push(MessageParam::new(
                        MessageParamContent::Array(content.clone()),
                        MessageRole::Assistant,
                    ));
                    content
                }
                PreFilter::PassToAgent => {
                    if let Some(threshold) = max_history_messages
                        && let Some(previous_len) =
                            reset_history_if_over_threshold(&mut messages, threshold)
                    {
                        eprintln!(
                            "[telegram] reset chat history for chat_id={chat}; \
                             previous messages={previous_len}, threshold={threshold}"
                        );
                    }
                    transport.typing(chat).await.ok();
                    let (turn_start, _outcome) = take_agent_turn_with_reset(
                        &mut agent,
                        &client,
                        &mut messages,
                        &budget,
                        reset_on_context_limit,
                    )
                    .await?;
                    assistant_content(&messages[turn_start..])
                }
            };

            // 4. Commit point: record the full native turn (+ enqueue outbox in the
            //    same commit when using the outbox). Offset is NOT advanced here.
            let outbox_id = {
                let mut st = state.lock().await;
                st.set_conversation(chat, messages);
                let id = if use_outbox && content_blocks_have_text(&send_content) {
                    let record = OutboxRecord::pending(chat, send_content.clone());
                    let id = record.logical_id;
                    st.outbox.push(record);
                    Some(id)
                } else {
                    None
                };
                let snapshot = st.clone();
                drop(st);
                store.commit(&snapshot).await?;
                id
            };

            // 5. Send.
            if let Some(id) = outbox_id {
                // Outbox path: 403 marks dead inside drive_outbox_record; we
                // swallow it so the loop continues and still acks the update.
                if let Err(err) = drive_outbox_record(&transport, store.as_ref(), &state, id).await
                    && err.api_code() != Some(403)
                {
                    return Err(err);
                }
            } else if content_blocks_have_text(&send_content)
                && let Err(err) = send_direct_and_mark_dead(
                    &transport,
                    store.as_ref(),
                    &state,
                    chat,
                    send_content,
                )
                .await
                && err.api_code() != Some(403)
            {
                return Err(err);
            }

            // 6. Ack: advances and persists the offset. Never skipped after a
            //    successful commit, so we never reprocess this update.
            transport.ack(update_id).await?;
        }
    }

    Ok(())
}
