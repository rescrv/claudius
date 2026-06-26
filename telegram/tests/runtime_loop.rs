//! End-to-end tests for the durable turn loop.
//!
//! Uses an in-memory transport that mirrors Telegram's offset/ack semantics and
//! shares its `TransportState`/`StateStore` with the loop, so we can assert:
//!   * the loop commits conversation history and advances the offset,
//!   * a "restart" (reload from the store) does not reprocess acked updates.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use claudius::{
    Agent, Anthropic, Budget, ContentBlock, MessageParam, MessageParamContent, MessageRole,
    Renderer, StopReason, TextBlock, ThinkingBlock, TurnOutcome, Usage,
};
use tokio::sync::Mutex;

use claudius_telegram::{
    ChatId, ChatTransport, Error, InMemoryStateStore, Inbound, LoopConfig, MessageId, PreFilter,
    StateStore, TransportState, UpdateId, content_blocks_to_text, run_agent_loop,
};

/// A trivial agent; never invoked because the pre-filter handles every message.
struct EchoAgent;
impl Agent for EchoAgent {}

/// An agent that appends native assistant blocks, including thinking.
struct NativeBlockAgent;

#[async_trait]
impl Agent for NativeBlockAgent {
    async fn take_turn_streaming_root(
        &mut self,
        _client: &Anthropic,
        messages: &mut Vec<MessageParam>,
        _budget: &Arc<Budget>,
        _renderer: &mut dyn Renderer,
    ) -> Result<TurnOutcome, claudius::Error> {
        messages.push(MessageParam::new(
            MessageParamContent::Array(vec![
                ContentBlock::Thinking(ThinkingBlock::new("internal", "signature")),
                ContentBlock::Text(TextBlock::new("answer")),
            ]),
            MessageRole::Assistant,
        ));

        Ok(TurnOutcome {
            stop_reason: StopReason::EndTurn,
            usage: Usage::new(0, 0),
            request_count: 0,
        })
    }
}

/// An in-memory transport sharing durable state with the loop.
struct InMemTransport {
    updates: Vec<Inbound>,
    state: Arc<Mutex<TransportState>>,
    store: Arc<dyn StateStore>,
    interrupted: Arc<AtomicBool>,
    sent: Arc<Mutex<Vec<(i64, String)>>>,
    polled: bool,
}

#[async_trait]
impl ChatTransport for InMemTransport {
    async fn recv(&mut self) -> Result<Vec<Inbound>, Error> {
        if self.polled {
            // Second poll: nothing new; signal shutdown and return empty.
            self.interrupted.store(true, Ordering::SeqCst);
            return Ok(Vec::new());
        }
        self.polled = true;
        let offset = self.state.lock().await.next_offset;
        Ok(self
            .updates
            .iter()
            .filter(|u| u.update_id.0 >= offset)
            .cloned()
            .collect())
    }

    async fn ack(&mut self, up_to: UpdateId) -> Result<(), Error> {
        let snapshot = {
            let mut st = self.state.lock().await;
            let candidate = up_to.0 + 1;
            if candidate > st.next_offset {
                st.next_offset = candidate;
            }
            st.clone()
        };
        self.store.commit(&snapshot).await
    }

    async fn send(&self, chat: ChatId, content: Vec<ContentBlock>) -> Result<MessageId, Error> {
        self.sent
            .lock()
            .await
            .push((chat.0, content_blocks_to_text(&content)));
        Ok(MessageId(1))
    }
}

fn echo_filter() -> Box<dyn Fn(&Inbound) -> PreFilter + Send> {
    Box::new(|inb: &Inbound| PreFilter::handled_text(inb.text.to_uppercase()))
}

#[tokio::test]
async fn loop_commits_history_and_advances_offset() {
    let store: Arc<dyn StateStore> = Arc::new(InMemoryStateStore::new());
    let state = Arc::new(Mutex::new(store.load().await.unwrap()));
    let interrupted = Arc::new(AtomicBool::new(false));
    let sent = Arc::new(Mutex::new(Vec::new()));

    let transport = InMemTransport {
        updates: vec![
            Inbound::new(UpdateId(0), ChatId(1), "hello"),
            Inbound::new(UpdateId(1), ChatId(1), "world"),
        ],
        state: Arc::clone(&state),
        store: Arc::clone(&store),
        interrupted: Arc::clone(&interrupted),
        sent: Arc::clone(&sent),
        polled: false,
    };

    let config = LoopConfig {
        store: Arc::clone(&store),
        state: Arc::clone(&state),
        budget: Arc::new(Budget::from_dollars_flat_rate(0.0, 1000)),
        use_outbox: false,
        interrupted: Arc::clone(&interrupted),
        pre_filter: Some(echo_filter()),
    };
    let client = Anthropic::new(Some("dummy".to_string())).unwrap();

    run_agent_loop(EchoAgent, transport, client, config)
        .await
        .unwrap();

    // Both messages were echoed uppercased.
    let sent = sent.lock().await.clone();
    assert_eq!(
        sent,
        vec![(1, "HELLO".to_string()), (1, "WORLD".to_string())]
    );

    // The committed state advanced the offset past update 1 and recorded the
    // full user+assistant history.
    let committed = store.load().await.unwrap();
    assert_eq!(committed.next_offset, 2);
    let convo = committed.conversation(ChatId(1)).unwrap();
    assert_eq!(convo.len(), 4); // user, assistant, user, assistant
}

#[tokio::test]
async fn loop_persists_native_blocks_but_sends_text_only() {
    let store: Arc<dyn StateStore> = Arc::new(InMemoryStateStore::new());
    let state = Arc::new(Mutex::new(store.load().await.unwrap()));
    let interrupted = Arc::new(AtomicBool::new(false));
    let sent = Arc::new(Mutex::new(Vec::new()));

    let transport = InMemTransport {
        updates: vec![Inbound::new(UpdateId(0), ChatId(1), "hello")],
        state: Arc::clone(&state),
        store: Arc::clone(&store),
        interrupted: Arc::clone(&interrupted),
        sent: Arc::clone(&sent),
        polled: false,
    };

    let config = LoopConfig {
        store: Arc::clone(&store),
        state: Arc::clone(&state),
        budget: Arc::new(Budget::from_dollars_flat_rate(0.0, 1000)),
        use_outbox: false,
        interrupted: Arc::clone(&interrupted),
        pre_filter: None,
    };
    let client = Anthropic::new(Some("dummy".to_string())).unwrap();

    run_agent_loop(NativeBlockAgent, transport, client, config)
        .await
        .unwrap();

    assert_eq!(sent.lock().await.clone(), vec![(1, "answer".to_string())]);

    let committed = store.load().await.unwrap();
    let convo = committed.conversation(ChatId(1)).unwrap();
    assert_eq!(convo.len(), 2);
    let MessageParamContent::Array(blocks) = &convo[1].content else {
        panic!("assistant message should use native content blocks");
    };
    assert!(matches!(blocks[0], ContentBlock::Thinking(_)));
    assert!(matches!(blocks[1], ContentBlock::Text(_)));
}

#[tokio::test]
async fn restart_does_not_reprocess_acked_updates() {
    let store: Arc<dyn StateStore> = Arc::new(InMemoryStateStore::new());
    let interrupted = Arc::new(AtomicBool::new(false));

    // ---- First run: process one update. ----
    {
        let state = Arc::new(Mutex::new(store.load().await.unwrap()));
        let sent = Arc::new(Mutex::new(Vec::new()));
        let transport = InMemTransport {
            updates: vec![Inbound::new(UpdateId(0), ChatId(1), "only")],
            state: Arc::clone(&state),
            store: Arc::clone(&store),
            interrupted: Arc::clone(&interrupted),
            sent: Arc::clone(&sent),
            polled: false,
        };
        let config = LoopConfig {
            store: Arc::clone(&store),
            state: Arc::clone(&state),
            budget: Arc::new(Budget::from_dollars_flat_rate(0.0, 1000)),
            use_outbox: false,
            interrupted: Arc::clone(&interrupted),
            pre_filter: Some(echo_filter()),
        };
        let client = Anthropic::new(Some("dummy".to_string())).unwrap();
        run_agent_loop(EchoAgent, transport, client, config)
            .await
            .unwrap();
        assert_eq!(sent.lock().await.len(), 1);
    }

    // ---- Restart: same update still present, but offset has advanced. ----
    interrupted.store(false, Ordering::SeqCst);
    {
        let state = Arc::new(Mutex::new(store.load().await.unwrap()));
        assert_eq!(state.lock().await.next_offset, 1);
        let sent = Arc::new(Mutex::new(Vec::new()));
        let transport = InMemTransport {
            updates: vec![Inbound::new(UpdateId(0), ChatId(1), "only")],
            state: Arc::clone(&state),
            store: Arc::clone(&store),
            interrupted: Arc::clone(&interrupted),
            sent: Arc::clone(&sent),
            polled: false,
        };
        let config = LoopConfig {
            store: Arc::clone(&store),
            state: Arc::clone(&state),
            budget: Arc::new(Budget::from_dollars_flat_rate(0.0, 1000)),
            use_outbox: false,
            interrupted: Arc::clone(&interrupted),
            pre_filter: Some(echo_filter()),
        };
        let client = Anthropic::new(Some("dummy".to_string())).unwrap();
        run_agent_loop(EchoAgent, transport, client, config)
            .await
            .unwrap();
        // The already-acked update is filtered out: no reprocessing.
        assert_eq!(sent.lock().await.len(), 0);
    }
}
