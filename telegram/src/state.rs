//! Durable transport state: the single atomic artifact that makes the turn loop
//! restart-safe.
//!
//! The load-bearing idea (see the crate-level PRD): the `getUpdates` offset lives
//! **inside the same durable blob** as the conversation history and the
//! transactional outbox, and they commit together via [`StateStore::commit`].
//! Recovery is "load one blob, resume polling from its embedded offset." There is
//! no cross-object consistency gap.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::{ChatId, Error};

one_two_eight::generate_id! {LogicalIdInner, "outbox:"}

/// A stable logical id for an intended send, used as an outbox dedup key.
///
/// Backed by a 128-bit [`one_two_eight`] identifier and serialized in its
/// human-readable form (e.g. `outbox:...`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LogicalId(LogicalIdInner);

impl LogicalId {
    /// Generates a fresh random logical id.
    pub fn generate() -> Self {
        // urandom only fails if the OS RNG is unavailable; fall back to BOTTOM
        // rather than panic, which keeps the outbox usable in degraded environments.
        LogicalId(LogicalIdInner::generate().unwrap_or(LogicalIdInner::BOTTOM))
    }

    /// Returns the human-readable encoding (with the `outbox:` prefix).
    pub fn human_readable(&self) -> String {
        self.0.human_readable()
    }
}

impl std::fmt::Display for LogicalId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Serialize for LogicalId {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.human_readable())
    }
}

impl<'de> Deserialize<'de> for LogicalId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        LogicalIdInner::from_human_readable(&s)
            .map(LogicalId)
            .ok_or_else(|| serde::de::Error::custom("invalid LogicalId"))
    }
}

/// The role of a [`ConversationEntry`], a serializable mirror of `MessageRole`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryRole {
    /// A message from the human user.
    User,
    /// A message from the assistant/agent.
    Assistant,
    /// A system message interleaved into the conversation.
    System,
}

/// A serializable projection of a [`claudius::MessageParam`] sufficient to
/// rebuild the message vector on restart.
///
/// Kept deliberately minimal (role + text). `#[non_exhaustive]` so richer block
/// projections can be added later without a breaking change.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct ConversationEntry {
    /// The role that produced this entry.
    pub role: EntryRole,
    /// The flattened text content.
    pub text: String,
}

impl ConversationEntry {
    /// Constructs a conversation entry.
    pub fn new(role: EntryRole, text: impl Into<String>) -> Self {
        Self {
            role,
            text: text.into(),
        }
    }
}

/// The delivery status of an [`OutboxRecord`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum OutboxStatus {
    /// Enqueued but not yet confirmed sent. Re-sent on restart (at-least-once).
    Pending,
    /// Confirmed sent; carries the server `message_id`.
    Sent {
        /// The Telegram `message_id` of the delivered message.
        message_id: i64,
    },
}

/// A transactional-outbox record for exactly-once-intent outbound delivery.
///
/// `sendMessage` is not idempotent, so a retry after an ambiguous network
/// timeout can produce a duplicate. The outbox bounds this to a single
/// documented window: a `Pending` record whose send outcome was unknown at
/// crash time is re-sent on restart (at-least-once).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct OutboxRecord {
    /// The stable dedup key for this intended send.
    pub logical_id: LogicalId,
    /// The destination chat.
    pub chat: ChatId,
    /// The message text.
    pub text: String,
    /// The current delivery status.
    pub status: OutboxStatus,
}

impl OutboxRecord {
    /// Constructs a `Pending` outbox record with a fresh logical id.
    pub fn pending(chat: ChatId, text: impl Into<String>) -> Self {
        Self {
            logical_id: LogicalId::generate(),
            chat,
            text: text.into(),
            status: OutboxStatus::Pending,
        }
    }
}

/// The single durable artifact for a transport.
///
/// Serialized to JSON and committed atomically via a [`StateStore`]. The
/// `next_offset` cursor commits together with `conversations` and `outbox`, which
/// is what collapses inbound dedup into one atomic state write.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransportState {
    /// The next offset to pass to `getUpdates`. Persisted on every `ack`.
    pub next_offset: i64,
    /// Per-chat conversation history, so an agent can resume context on restart.
    pub conversations: BTreeMap<i64, Vec<ConversationEntry>>,
    /// The transactional outbox for exactly-once-intent outbound delivery.
    pub outbox: Vec<OutboxRecord>,
    /// Chats known to be dead (`403`). Proactive sends skip these.
    pub dead_chats: BTreeSet<i64>,
}

impl TransportState {
    /// Records first contact with `chat`, ensuring it has a conversation slot.
    ///
    /// This is what later enables proactive sends: a chat is only addressable
    /// once it has messaged the bot at least once.
    pub fn note_chat(&mut self, chat: ChatId) {
        self.conversations.entry(chat.0).or_default();
    }

    /// Appends a conversation entry for `chat`.
    pub fn push_entry(&mut self, chat: ChatId, entry: ConversationEntry) {
        self.conversations.entry(chat.0).or_default().push(entry);
    }

    /// Returns the conversation history for `chat`, if any.
    pub fn conversation(&self, chat: ChatId) -> Option<&[ConversationEntry]> {
        self.conversations.get(&chat.0).map(|v| v.as_slice())
    }

    /// Marks `chat` dead (a `403` response). Idempotent.
    pub fn mark_dead(&mut self, chat: ChatId) {
        self.dead_chats.insert(chat.0);
    }

    /// Returns whether `chat` has been marked dead.
    pub fn is_dead(&self, chat: ChatId) -> bool {
        self.dead_chats.contains(&chat.0)
    }
}

/////////////////////////////////////////// StateStore ///////////////////////////////////////////

/// A pluggable durable backend for [`TransportState`].
///
/// v1 ships [`FileStateStore`] (atomic temp-file + rename) and
/// [`InMemoryStateStore`] (for tests). A future durable log (e.g. WAL-backed,
/// checksummed segments) can replace the file without touching the turn loop.
#[async_trait::async_trait]
pub trait StateStore: Send + Sync {
    /// Loads the persisted state, returning [`TransportState::default`] if none
    /// exists yet.
    async fn load(&self) -> Result<TransportState, Error>;

    /// Atomically commits `state`. The commit point is a single durable write.
    async fn commit(&self, state: &TransportState) -> Result<(), Error>;
}

/// A [`StateStore`] backed by a single JSON file, committed via atomic rename.
///
/// The write path is: serialize to a sibling temp file, `fsync` it, `rename` over
/// the target, then `fsync` the parent directory. A crash mid-write leaves the
/// prior committed file intact (no partial state is ever observed).
pub struct FileStateStore {
    path: PathBuf,
}

impl FileStateStore {
    /// Creates a file-backed store at `path`.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Returns the backing file path.
    pub fn path(&self) -> &std::path::Path {
        &self.path
    }
}

#[async_trait::async_trait]
impl StateStore for FileStateStore {
    async fn load(&self) -> Result<TransportState, Error> {
        let path = self.path.clone();
        tokio::task::spawn_blocking(move || {
            match std::fs::read(&path) {
                Ok(bytes) => {
                    let state: TransportState = serde_json::from_slice(&bytes)?;
                    Ok(state)
                }
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                    Ok(TransportState::default())
                }
                Err(err) => Err(Error::Io(err)),
            }
        })
        .await
        .map_err(|e| Error::Internal(format!("load join error: {e}")))?
    }

    async fn commit(&self, state: &TransportState) -> Result<(), Error> {
        let path = self.path.clone();
        let bytes = serde_json::to_vec_pretty(state)?;
        tokio::task::spawn_blocking(move || -> Result<(), Error> {
            let dir = path
                .parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| PathBuf::from("."));
            std::fs::create_dir_all(&dir)?;
            let tmp = path.with_extension("tmp");
            {
                let mut f = std::fs::File::create(&tmp)?;
                f.write_all(&bytes)?;
                f.sync_all()?;
            }
            std::fs::rename(&tmp, &path)?;
            // fsync the directory so the rename itself is durable.
            if let Ok(dirf) = std::fs::File::open(&dir) {
                let _ = dirf.sync_all();
            }
            Ok(())
        })
        .await
        .map_err(|e| Error::Internal(format!("commit join error: {e}")))?
    }
}

/// An in-memory [`StateStore`] for tests and ephemeral runs.
#[derive(Default)]
pub struct InMemoryStateStore {
    inner: tokio::sync::Mutex<TransportState>,
}

impl InMemoryStateStore {
    /// Creates an empty in-memory store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates an in-memory store seeded with `state`.
    pub fn with_state(state: TransportState) -> Self {
        Self {
            inner: tokio::sync::Mutex::new(state),
        }
    }
}

#[async_trait::async_trait]
impl StateStore for InMemoryStateStore {
    async fn load(&self) -> Result<TransportState, Error> {
        Ok(self.inner.lock().await.clone())
    }

    async fn commit(&self, state: &TransportState) -> Result<(), Error> {
        *self.inner.lock().await = state.clone();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logical_id_roundtrips_through_json() {
        let id = LogicalId::generate();
        let json = serde_json::to_string(&id).unwrap();
        let back: LogicalId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
    }

    #[test]
    fn transport_state_roundtrips() {
        let mut state = TransportState {
            next_offset: 42,
            ..Default::default()
        };
        state.note_chat(ChatId(7));
        state.push_entry(ChatId(7), ConversationEntry::new(EntryRole::User, "hi"));
        state.outbox.push(OutboxRecord::pending(ChatId(7), "yo"));
        state.mark_dead(ChatId(-100));

        let json = serde_json::to_string(&state).unwrap();
        let back: TransportState = serde_json::from_str(&json).unwrap();
        assert_eq!(state, back);
    }
}
