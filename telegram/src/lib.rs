#![deny(missing_docs)]

//! A durable Telegram Bot API transport for [`claudius`] agents.
//!
//! `claudius-telegram` lets any [`claudius::Agent`] converse over Telegram instead
//! of a terminal. It provides a [`ChatTransport`] abstraction with two
//! implementations -- [`StdinTransport`] (the terminal behavior, preserved for
//! testing) and [`TelegramTransport`] (a long-polling Bot API client) -- plus a
//! durable turn loop ([`run_agent_loop`]) that survives process restarts without
//! losing or double-processing messages.
//!
//! The headline correctness property: inbound updates and the turn side effects
//! (conversation history and the Telegram poll cursor) commit as one atomic unit.
//! Recovery is "load one state blob, resume polling from its embedded offset."
//!
//! This crate is transport-only and agent-agnostic. It contains no application
//! logic. A future `WebhookTransport` could implement the same [`ChatTransport`]
//! trait; v1 is long-polling only (see the trait docs).

mod chunk;
mod runtime;
mod state;
mod stdin;
mod telegram;
mod transport;

pub use chunk::{DEFAULT_CHUNK_LIMIT, chunk_message};
pub use runtime::{
    BufferingRenderer, LoopConfig, PreFilter, PreFilterFn, SyntheticTurnConfig, known_chats,
    run_agent_loop, run_synthetic_user_turn, send_proactive,
};
pub use state::{
    FileStateStore, InMemoryStateStore, LogicalId, OutboxRecord, OutboxStatus, StateStore,
    TransportState,
};
pub use stdin::StdinTransport;
pub use telegram::TelegramTransport;
pub use transport::{
    ChatTransport, Inbound, content_blocks_have_text, content_blocks_to_text, text_content,
};

use std::fmt;

/// A Telegram chat identifier.
///
/// Chat IDs are `i64` and can be negative for groups and channels. A bot must
/// capture and persist each `chat_id` from a user's first inbound message before
/// it can ever send to that user (Telegram returns `403` otherwise).
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct ChatId(pub i64);

impl fmt::Display for ChatId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A Telegram update identifier.
///
/// An update is consumed only when `getUpdates` is next called with an `offset`
/// strictly greater than this id. The cursor is this crate's durability ack.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct UpdateId(pub i64);

impl fmt::Display for UpdateId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A Telegram message identifier, returned by `sendMessage`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct MessageId(pub i64);

impl fmt::Display for MessageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/////////////////////////////////////////// Error ///////////////////////////////////////////

/// The error type for `claudius-telegram`.
#[derive(Debug)]
pub enum Error {
    /// An HTTP transport-level error from `reqwest`.
    Http(reqwest::Error),
    /// The Telegram API returned `ok: false`.
    ///
    /// `retry_after` is preserved from `parameters.retry_after` so the turn loop
    /// can honor `429` backoff. `code` carries the Telegram `error_code` (e.g.
    /// `403` for a dead channel, `429` for rate limiting).
    Api {
        /// The Telegram `error_code`.
        code: i64,
        /// The human-readable `description` from Telegram.
        description: String,
        /// Seconds to wait before retrying, from `parameters.retry_after`.
        retry_after: Option<u64>,
    },
    /// An I/O error, typically from durable state persistence.
    Io(std::io::Error),
    /// A JSON serialization or deserialization error.
    Json(serde_json::Error),
    /// An error originating in the underlying `claudius` SDK.
    Claudius(claudius::Error),
    /// An internal invariant was violated.
    Internal(String),
}

impl Error {
    /// Returns the `retry_after` hint, if this is an [`Error::Api`] carrying one.
    pub fn retry_after(&self) -> Option<u64> {
        match self {
            Error::Api { retry_after, .. } => *retry_after,
            _ => None,
        }
    }

    /// Returns the Telegram `error_code`, if this is an [`Error::Api`].
    pub fn api_code(&self) -> Option<i64> {
        match self {
            Error::Api { code, .. } => Some(*code),
            _ => None,
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Http(err) => write!(f, "HTTP error: {err}"),
            Error::Api {
                code,
                description,
                retry_after,
            } => {
                if let Some(retry_after) = retry_after {
                    write!(
                        f,
                        "Telegram API error {code}: {description} (retry after {retry_after}s)"
                    )
                } else {
                    write!(f, "Telegram API error {code}: {description}")
                }
            }
            Error::Io(err) => write!(f, "I/O error: {err}"),
            Error::Json(err) => write!(f, "JSON error: {err}"),
            Error::Claudius(err) => write!(f, "claudius error: {err}"),
            Error::Internal(msg) => write!(f, "internal error: {msg}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Http(err) => Some(err),
            Error::Io(err) => Some(err),
            Error::Json(err) => Some(err),
            Error::Claudius(err) => Some(err),
            _ => None,
        }
    }
}

impl From<reqwest::Error> for Error {
    fn from(err: reqwest::Error) -> Self {
        Error::Http(err)
    }
}

impl From<std::io::Error> for Error {
    fn from(err: std::io::Error) -> Self {
        Error::Io(err)
    }
}

impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Self {
        Error::Json(err)
    }
}

impl From<claudius::Error> for Error {
    fn from(err: claudius::Error) -> Self {
        Error::Claudius(err)
    }
}
