//! Transport-agnostic contract tests.
//!
//! The suite asserts the load-bearing `ChatTransport` invariants without any live
//! network:
//!   * `recv` does not advance the cursor; the same updates return until `ack`.
//!   * `ack` advances the cursor so consumed updates are not redelivered.
//!   * `send` returns a `MessageId` on success.
//!   * `send` returns `Error::Api { code: 403, .. }` without retrying.
//!
//! It runs against a `MockTelegram` that faithfully mirrors Telegram's offset
//! semantics, plus a couple of direct checks against `StdinTransport`.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use async_trait::async_trait;

use claudius_telegram::{
    ChatId, ChatTransport, Error, Inbound, MessageId, Outbound, StdinTransport, UpdateId,
};

/// How a scripted send should behave.
#[derive(Clone, Copy)]
enum SendScript {
    Ok(i64),
    Forbidden,
}

/// An in-memory fake mirroring real Telegram offset/ack semantics.
struct MockTelegram {
    /// Updates keyed by update_id, in delivery order.
    updates: Vec<Inbound>,
    /// The next offset to deliver from (mirrors `getUpdates` offset).
    next_offset: i64,
    /// What `send` should do.
    script: SendScript,
    /// Count of actual send attempts, to prove 403 does not retry.
    send_attempts: Arc<AtomicUsize>,
}

impl MockTelegram {
    fn new(updates: Vec<Inbound>, script: SendScript) -> Self {
        Self {
            updates,
            next_offset: 0,
            script,
            send_attempts: Arc::new(AtomicUsize::new(0)),
        }
    }
}

#[async_trait]
impl ChatTransport for MockTelegram {
    async fn recv(&mut self) -> Result<Vec<Inbound>, Error> {
        // Return every update with id >= next_offset, WITHOUT advancing.
        Ok(self
            .updates
            .iter()
            .filter(|u| u.update_id.0 >= self.next_offset)
            .cloned()
            .collect())
    }

    async fn ack(&mut self, up_to: UpdateId) -> Result<(), Error> {
        let candidate = up_to.0 + 1;
        if candidate > self.next_offset {
            self.next_offset = candidate;
        }
        Ok(())
    }

    async fn send(&self, _out: Outbound) -> Result<MessageId, Error> {
        self.send_attempts.fetch_add(1, Ordering::SeqCst);
        match self.script {
            SendScript::Ok(id) => Ok(MessageId(id)),
            SendScript::Forbidden => Err(Error::Api {
                code: 403,
                description: "bot was blocked by the user".to_string(),
                retry_after: None,
            }),
        }
    }
}

fn inbound(id: i64, text: &str) -> Inbound {
    Inbound::new(UpdateId(id), ChatId(1), text)
}

#[tokio::test]
async fn recv_does_not_advance_cursor_without_ack() {
    let mut t = MockTelegram::new(
        vec![inbound(0, "a"), inbound(1, "b")],
        SendScript::Ok(100),
    );

    // Repeated recv without ack returns the same updates every time.
    let first = t.recv().await.unwrap();
    let second = t.recv().await.unwrap();
    assert_eq!(first.len(), 2);
    assert_eq!(second.len(), 2);
    assert_eq!(first[0].update_id, second[0].update_id);
}

#[tokio::test]
async fn ack_advances_cursor() {
    let mut t = MockTelegram::new(
        vec![inbound(0, "a"), inbound(1, "b"), inbound(2, "c")],
        SendScript::Ok(100),
    );

    assert_eq!(t.recv().await.unwrap().len(), 3);
    t.ack(UpdateId(1)).await.unwrap(); // consume 0 and 1
    let remaining = t.recv().await.unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].update_id, UpdateId(2));
}

#[tokio::test]
async fn send_returns_message_id_on_success() {
    let t = MockTelegram::new(vec![], SendScript::Ok(777));
    let id = t.send(Outbound::new(ChatId(1), "hi")).await.unwrap();
    assert_eq!(id, MessageId(777));
}

#[tokio::test]
async fn send_403_returns_without_retry() {
    let t = MockTelegram::new(vec![], SendScript::Forbidden);
    let attempts = Arc::clone(&t.send_attempts);

    let err = t.send(Outbound::new(ChatId(1), "hi")).await.unwrap_err();
    assert_eq!(err.api_code(), Some(403));
    // Exactly one attempt: a 403 must not be retried.
    assert_eq!(attempts.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn stdin_ack_is_noop_and_send_succeeds() {
    let shutdown = Arc::new(AtomicBool::new(false));
    let mut t = StdinTransport::new(shutdown);
    // ack is a no-op on the terminal cursor.
    t.ack(UpdateId(42)).await.unwrap();
    // send prints and reports a synthetic message id.
    let id = t.send(Outbound::new(ChatId(0), "printed line")).await.unwrap();
    assert_eq!(id, MessageId(0));
}
