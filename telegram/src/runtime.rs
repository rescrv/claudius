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
    Agent, Anthropic, Budget, MessageParam, MessageParamContent, MessageRole, Renderer,
    StreamContext,
};

use crate::state::{
    ConversationEntry, EntryRole, OutboxRecord, OutboxStatus, StateStore, TransportState,
};
use crate::transport::{ChatTransport, Inbound, Outbound};
use crate::{ChatId, Error, MessageId};

/////////////////////////////////////// BufferingRenderer ///////////////////////////////////////

/// A [`Renderer`] that accumulates streamed assistant text instead of writing to
/// stdout, so the turn loop can hand a single buffered message to a transport.
///
/// Chunking to Telegram's size limit happens in the transport's `send`, not here:
/// this renderer stays dumb -- accumulate and hand back.
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

    fn start_tool_result(&mut self, _context: &dyn StreamContext, _tool_use_id: &str, _is_error: bool) {}

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
    /// The message was handled; send this text and skip the agent turn.
    Handled(String),
    /// Pass the message through to the agent.
    PassToAgent,
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
        }
    }
}

/////////////////////////////////////////// helpers ///////////////////////////////////////////

/// Returns the chats this transport knows about (the prerequisite for any
/// proactive send -- recall Telegram forbids initiating a conversation).
pub fn known_chats(state: &TransportState) -> Vec<ChatId> {
    state.conversations.keys().copied().map(ChatId).collect()
}

fn entry_to_message(entry: &ConversationEntry) -> MessageParam {
    let role = match entry.role {
        EntryRole::User => MessageRole::User,
        EntryRole::Assistant => MessageRole::Assistant,
        EntryRole::System => MessageRole::System,
    };
    MessageParam::new(MessageParamContent::String(entry.text.clone()), role)
}

fn build_messages(state: &TransportState, chat: ChatId) -> Vec<MessageParam> {
    state
        .conversation(chat)
        .map(|entries| entries.iter().map(entry_to_message).collect())
        .unwrap_or_default()
}

/// Drives a single `Pending` outbox record to `Sent`: sends it, then marks it and
/// commits. Returns the sent message id. On `403` marks the chat dead, commits,
/// and returns the error.
async fn drive_outbox_record(
    transport: &dyn ChatTransport,
    store: &dyn StateStore,
    state: &Arc<Mutex<TransportState>>,
    logical_id: crate::state::LogicalId,
) -> Result<MessageId, Error> {
    // Read the record's chat/text without holding the lock across the send.
    let (chat, text) = {
        let st = state.lock().await;
        let rec = st
            .outbox
            .iter()
            .find(|r| r.logical_id == logical_id && r.status == OutboxStatus::Pending)
            .ok_or_else(|| Error::Internal("outbox record not found or not pending".to_string()))?;
        (rec.chat, rec.text.clone())
    };

    match transport.send(Outbound::new(chat, text)).await {
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

/// Sends `text` to `chat` independently of any inbound message, via the outbox.
///
/// Respects `dead_chats` (returns an error without attempting a send) and records
/// the intent durably before sending. This is the §9 hook the downstream
/// proactive check-in scheduler calls on a timer.
pub async fn send_proactive(
    transport: &dyn ChatTransport,
    store: &dyn StateStore,
    state: &Arc<Mutex<TransportState>>,
    chat: ChatId,
    text: impl Into<String>,
) -> Result<MessageId, Error> {
    let text = text.into();
    let logical_id = {
        let mut st = state.lock().await;
        if st.is_dead(chat) {
            return Err(Error::Api {
                code: 403,
                description: format!("chat {chat} is marked dead"),
                retry_after: None,
            });
        }
        let record = OutboxRecord::pending(chat, text);
        let id = record.logical_id;
        st.outbox.push(record);
        let snapshot = st.clone();
        drop(st);
        store.commit(&snapshot).await?;
        id
    };
    drive_outbox_record(transport, store, state, logical_id).await
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
            let chat = inbound.chat;
            let update_id = inbound.update_id;

            // 1. First contact + record the user's message, then build context.
            let messages_seed = {
                let mut st = state.lock().await;
                st.note_chat(chat);
                st.push_entry(chat, ConversationEntry::new(EntryRole::User, &inbound.text));
                build_messages(&st, chat)
            };

            // 2. Pre-filter.
            let decision = pre_filter
                .as_ref()
                .map(|f| f(&inbound))
                .unwrap_or(PreFilter::PassToAgent);

            // 3. Produce the assistant text.
            let assistant_text = match decision {
                PreFilter::Handled(text) => text,
                PreFilter::PassToAgent => {
                    transport.typing(chat).await.ok();
                    let mut messages = messages_seed;
                    let mut renderer = BufferingRenderer::new();
                    agent
                        .take_turn_streaming_root(&client, &mut messages, &budget, &mut renderer)
                        .await?;
                    renderer.take()
                }
            };

            // 4. Commit point: record the assistant turn (+ enqueue outbox in the
            //    same commit when using the outbox). Offset is NOT advanced here.
            let outbox_id = {
                let mut st = state.lock().await;
                st.push_entry(
                    chat,
                    ConversationEntry::new(EntryRole::Assistant, &assistant_text),
                );
                let id = if use_outbox {
                    let record = OutboxRecord::pending(chat, &assistant_text);
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
            } else if !assistant_text.is_empty() {
                match transport.send(Outbound::new(chat, assistant_text)).await {
                    Ok(_) => {}
                    Err(err @ Error::Api { code: 403, .. }) => {
                        let snapshot = {
                            let mut st = state.lock().await;
                            st.mark_dead(chat);
                            st.clone()
                        };
                        store.commit(&snapshot).await?;
                        eprintln!("[telegram] 403 dead chat_id={chat}; marked dead");
                        let _ = err;
                    }
                    Err(other) => return Err(other),
                }
            }

            // 6. Ack: advances and persists the offset. Never skipped after a
            //    successful commit, so we never reprocess this update.
            transport.ack(update_id).await?;
        }
    }

    Ok(())
}
