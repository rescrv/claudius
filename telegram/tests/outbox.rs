//! Tests for the transactional outbox and the §9 proactive-send hooks.

use std::sync::Arc;

use async_trait::async_trait;
use claudius::{
    Agent, Anthropic, Budget, ContentBlock, MessageParam, MessageParamContent, MessageRole,
    Renderer, StopReason, TextBlock, TurnOutcome, Usage,
};
use tokio::sync::Mutex;

use claudius_telegram::{
    ChatId, ChatTransport, Error, InMemoryStateStore, Inbound, MessageId, OutboxStatus, StateStore,
    SyntheticTurnConfig, TransportState, UpdateId, content_blocks_to_text, known_chats,
    run_synthetic_user_turn, send_proactive, text_content,
};

enum SendScript {
    Ok,
    Forbidden,
    Transient,
}

/// A transport whose `send` outcome is scripted.
struct ScriptedTransport {
    script: SendScript,
    sent: Arc<Mutex<Vec<(i64, String)>>>,
}

#[async_trait]
impl ChatTransport for ScriptedTransport {
    async fn recv(&mut self) -> Result<Vec<Inbound>, Error> {
        Ok(Vec::new())
    }
    async fn ack(&mut self, _up_to: UpdateId) -> Result<(), Error> {
        Ok(())
    }
    async fn send(&self, chat: ChatId, content: Vec<ContentBlock>) -> Result<MessageId, Error> {
        match self.script {
            SendScript::Ok => {}
            SendScript::Forbidden => {
                return Err(Error::Api {
                    code: 403,
                    description: "blocked".to_string(),
                    retry_after: None,
                });
            }
            SendScript::Transient => {
                return Err(Error::Api {
                    code: 500,
                    description: "try again".to_string(),
                    retry_after: None,
                });
            }
        }
        self.sent
            .lock()
            .await
            .push((chat.0, content_blocks_to_text(&content)));
        Ok(MessageId(555))
    }
}

struct ReplyAgent {
    response: &'static str,
    seen_lengths: Arc<std::sync::Mutex<Vec<usize>>>,
}

#[async_trait]
impl Agent for ReplyAgent {
    async fn take_turn_streaming_root(
        &mut self,
        _client: &Anthropic,
        messages: &mut Vec<MessageParam>,
        _budget: &Arc<Budget>,
        _renderer: &mut dyn Renderer,
    ) -> Result<TurnOutcome, claudius::Error> {
        self.seen_lengths.lock().unwrap().push(messages.len());
        messages.push(MessageParam::new(
            MessageParamContent::Array(vec![ContentBlock::Text(TextBlock::new(self.response))]),
            MessageRole::Assistant,
        ));

        Ok(TurnOutcome {
            stop_reason: StopReason::EndTurn,
            usage: Usage::new(0, 0),
            request_count: 0,
        })
    }
}

fn message_text(message: &MessageParam) -> String {
    match &message.content {
        MessageParamContent::String(text) => text.clone(),
        MessageParamContent::Array(blocks) => content_blocks_to_text(blocks),
    }
}

#[tokio::test]
async fn send_proactive_marks_record_sent() {
    let store: Arc<dyn StateStore> = Arc::new(InMemoryStateStore::new());
    let state = Arc::new(Mutex::new(TransportState::default()));
    let sent = Arc::new(Mutex::new(Vec::new()));
    let transport = ScriptedTransport {
        script: SendScript::Ok,
        sent: Arc::clone(&sent),
    };

    let id = send_proactive(
        &transport,
        store.as_ref(),
        &state,
        ChatId(9),
        text_content("ping"),
    )
    .await
    .unwrap();
    assert_eq!(id, MessageId(555));
    assert_eq!(sent.lock().await.clone(), vec![(9, "ping".to_string())]);

    let committed = store.load().await.unwrap();
    assert_eq!(committed.outbox.len(), 1);
    assert_eq!(
        committed.outbox[0].status,
        OutboxStatus::Sent { message_id: 555 }
    );
}

#[tokio::test]
async fn send_proactive_to_dead_chat_is_refused_without_sending() {
    let store: Arc<dyn StateStore> = Arc::new(InMemoryStateStore::new());
    let mut seed = TransportState::default();
    seed.mark_dead(ChatId(9));
    let state = Arc::new(Mutex::new(seed));
    let sent = Arc::new(Mutex::new(Vec::new()));
    let transport = ScriptedTransport {
        script: SendScript::Ok,
        sent: Arc::clone(&sent),
    };

    let err = send_proactive(
        &transport,
        store.as_ref(),
        &state,
        ChatId(9),
        text_content("ping"),
    )
    .await
    .unwrap_err();
    assert_eq!(err.api_code(), Some(403));
    // No send was attempted and no Pending record was enqueued.
    assert!(sent.lock().await.is_empty());
}

#[tokio::test]
async fn send_proactive_403_marks_chat_dead() {
    let store: Arc<dyn StateStore> = Arc::new(InMemoryStateStore::new());
    let state = Arc::new(Mutex::new(TransportState::default()));
    let sent = Arc::new(Mutex::new(Vec::new()));
    let transport = ScriptedTransport {
        script: SendScript::Forbidden,
        sent: Arc::clone(&sent),
    };

    let err = send_proactive(
        &transport,
        store.as_ref(),
        &state,
        ChatId(9),
        text_content("ping"),
    )
    .await
    .unwrap_err();
    assert_eq!(err.api_code(), Some(403));

    let committed = store.load().await.unwrap();
    assert!(committed.is_dead(ChatId(9)));
    // The record was enqueued but remains Pending (delivery unknown).
    assert_eq!(committed.outbox.len(), 1);
    assert_eq!(committed.outbox[0].status, OutboxStatus::Pending);
}

#[tokio::test]
async fn synthetic_turn_persists_model_response_and_marks_outbox_sent() {
    let mut seed = TransportState::default();
    seed.set_conversation(
        ChatId(9),
        vec![
            MessageParam::user("previous"),
            MessageParam::assistant("old"),
        ],
    );
    let store: Arc<dyn StateStore> = Arc::new(InMemoryStateStore::with_state(seed.clone()));
    let state = Arc::new(Mutex::new(seed));
    let sent = Arc::new(Mutex::new(Vec::new()));
    let transport = ScriptedTransport {
        script: SendScript::Ok,
        sent: Arc::clone(&sent),
    };
    let seen_lengths = Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut agent = ReplyAgent {
        response: "check-in",
        seen_lengths: Arc::clone(&seen_lengths),
    };
    let client = Anthropic::new(Some("dummy".to_string())).unwrap();
    let budget = Arc::new(Budget::from_dollars_flat_rate(0.0, 1000));

    run_synthetic_user_turn(
        &mut agent,
        &transport,
        &client,
        store.as_ref(),
        &state,
        &budget,
        ChatId(9),
        "scheduled wakeup".to_string(),
        SyntheticTurnConfig {
            source_label: Some("test-scheduler".to_string()),
            ..Default::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(seen_lengths.lock().unwrap().as_slice(), &[3]);
    assert_eq!(sent.lock().await.clone(), vec![(9, "check-in".to_string())]);

    let committed = store.load().await.unwrap();
    let convo = committed.conversation(ChatId(9)).unwrap();
    assert_eq!(convo.len(), 4);
    assert_eq!(convo[2].role, MessageRole::User);
    assert_eq!(message_text(&convo[2]), "scheduled wakeup");
    assert_eq!(convo[3].role, MessageRole::Assistant);
    assert_eq!(message_text(&convo[3]), "check-in");
    assert_eq!(committed.outbox.len(), 1);
    assert_eq!(
        committed.outbox[0].status,
        OutboxStatus::Sent { message_id: 555 }
    );
}

#[tokio::test]
async fn synthetic_turn_applies_history_limit_to_model_visible_context() {
    let mut seed = TransportState::default();
    seed.set_conversation(
        ChatId(9),
        vec![
            MessageParam::user("old user 1"),
            MessageParam::assistant("old assistant 1"),
            MessageParam::user("old user 2"),
            MessageParam::assistant("old assistant 2"),
        ],
    );
    let store: Arc<dyn StateStore> = Arc::new(InMemoryStateStore::with_state(seed.clone()));
    let state = Arc::new(Mutex::new(seed));
    let sent = Arc::new(Mutex::new(Vec::new()));
    let transport = ScriptedTransport {
        script: SendScript::Ok,
        sent: Arc::clone(&sent),
    };
    let seen_lengths = Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut agent = ReplyAgent {
        response: "limited",
        seen_lengths: Arc::clone(&seen_lengths),
    };
    let client = Anthropic::new(Some("dummy".to_string())).unwrap();
    let budget = Arc::new(Budget::from_dollars_flat_rate(0.0, 1000));

    run_synthetic_user_turn(
        &mut agent,
        &transport,
        &client,
        store.as_ref(),
        &state,
        &budget,
        ChatId(9),
        "current synthetic prompt".to_string(),
        SyntheticTurnConfig {
            max_history_messages: Some(2),
            use_outbox: false,
            ..Default::default()
        },
    )
    .await
    .unwrap();

    assert_eq!(seen_lengths.lock().unwrap().as_slice(), &[1]);
    assert_eq!(sent.lock().await.clone(), vec![(9, "limited".to_string())]);

    let committed = store.load().await.unwrap();
    let convo = committed.conversation(ChatId(9)).unwrap();
    assert_eq!(convo.len(), 2);
    assert_eq!(message_text(&convo[0]), "current synthetic prompt");
    assert_eq!(message_text(&convo[1]), "limited");
    assert!(committed.outbox.is_empty());
}

#[tokio::test]
async fn synthetic_turn_leaves_pending_outbox_record_on_transient_send_failure() {
    let mut seed = TransportState::default();
    seed.note_chat(ChatId(9));
    let store: Arc<dyn StateStore> = Arc::new(InMemoryStateStore::with_state(seed.clone()));
    let state = Arc::new(Mutex::new(seed));
    let sent = Arc::new(Mutex::new(Vec::new()));
    let transport = ScriptedTransport {
        script: SendScript::Transient,
        sent: Arc::clone(&sent),
    };
    let seen_lengths = Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut agent = ReplyAgent {
        response: "retry later",
        seen_lengths: Arc::clone(&seen_lengths),
    };
    let client = Anthropic::new(Some("dummy".to_string())).unwrap();
    let budget = Arc::new(Budget::from_dollars_flat_rate(0.0, 1000));

    let err = run_synthetic_user_turn(
        &mut agent,
        &transport,
        &client,
        store.as_ref(),
        &state,
        &budget,
        ChatId(9),
        "wake".to_string(),
        SyntheticTurnConfig::default(),
    )
    .await
    .unwrap_err();

    assert_eq!(err.api_code(), Some(500));
    assert!(sent.lock().await.is_empty());
    let committed = store.load().await.unwrap();
    assert_eq!(committed.conversation(ChatId(9)).unwrap().len(), 2);
    assert_eq!(committed.outbox.len(), 1);
    assert_eq!(committed.outbox[0].status, OutboxStatus::Pending);
}

#[tokio::test]
async fn synthetic_turn_refuses_dead_chat_without_running_agent() {
    let mut seed = TransportState::default();
    seed.note_chat(ChatId(9));
    seed.mark_dead(ChatId(9));
    let store: Arc<dyn StateStore> = Arc::new(InMemoryStateStore::with_state(seed.clone()));
    let state = Arc::new(Mutex::new(seed));
    let sent = Arc::new(Mutex::new(Vec::new()));
    let transport = ScriptedTransport {
        script: SendScript::Ok,
        sent: Arc::clone(&sent),
    };
    let seen_lengths = Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut agent = ReplyAgent {
        response: "should not run",
        seen_lengths: Arc::clone(&seen_lengths),
    };
    let client = Anthropic::new(Some("dummy".to_string())).unwrap();
    let budget = Arc::new(Budget::from_dollars_flat_rate(0.0, 1000));

    let err = run_synthetic_user_turn(
        &mut agent,
        &transport,
        &client,
        store.as_ref(),
        &state,
        &budget,
        ChatId(9),
        "wake".to_string(),
        SyntheticTurnConfig::default(),
    )
    .await
    .unwrap_err();

    assert_eq!(err.api_code(), Some(403));
    assert!(seen_lengths.lock().unwrap().is_empty());
    assert!(sent.lock().await.is_empty());
    let committed = store.load().await.unwrap();
    assert!(committed.conversation(ChatId(9)).unwrap().is_empty());
    assert!(committed.outbox.is_empty());
}

#[tokio::test]
async fn known_chats_lists_chats_with_history() {
    let mut state = TransportState::default();
    state.note_chat(ChatId(1));
    state.note_chat(ChatId(-100));
    let chats = known_chats(&state);
    assert!(chats.contains(&ChatId(1)));
    assert!(chats.contains(&ChatId(-100)));
    assert_eq!(chats.len(), 2);
}
