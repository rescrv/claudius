//! Tests for the transactional outbox and the §9 proactive-send hooks.

use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::Mutex;

use claudius_telegram::{
    ChatId, ChatTransport, Error, InMemoryStateStore, Inbound, MessageId, Outbound, OutboxStatus,
    StateStore, TransportState, UpdateId, known_chats, send_proactive,
};

/// A transport whose `send` outcome is scripted.
struct ScriptedTransport {
    forbidden: bool,
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
    async fn send(&self, out: Outbound) -> Result<MessageId, Error> {
        if self.forbidden {
            return Err(Error::Api {
                code: 403,
                description: "blocked".to_string(),
                retry_after: None,
            });
        }
        self.sent.lock().await.push((out.chat.0, out.text));
        Ok(MessageId(555))
    }
}

#[tokio::test]
async fn send_proactive_marks_record_sent() {
    let store: Arc<dyn StateStore> = Arc::new(InMemoryStateStore::new());
    let state = Arc::new(Mutex::new(TransportState::default()));
    let sent = Arc::new(Mutex::new(Vec::new()));
    let transport = ScriptedTransport {
        forbidden: false,
        sent: Arc::clone(&sent),
    };

    let id = send_proactive(&transport, store.as_ref(), &state, ChatId(9), "ping")
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
        forbidden: false,
        sent: Arc::clone(&sent),
    };

    let err = send_proactive(&transport, store.as_ref(), &state, ChatId(9), "ping")
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
        forbidden: true,
        sent: Arc::clone(&sent),
    };

    let err = send_proactive(&transport, store.as_ref(), &state, ChatId(9), "ping")
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
async fn known_chats_lists_chats_with_history() {
    let mut state = TransportState::default();
    state.note_chat(ChatId(1));
    state.note_chat(ChatId(-100));
    let chats = known_chats(&state);
    assert!(chats.contains(&ChatId(1)));
    assert!(chats.contains(&ChatId(-100)));
    assert_eq!(chats.len(), 2);
}
