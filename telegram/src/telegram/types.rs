//! Serde structs mirroring the subset of the Telegram Bot API this crate uses.
//!
//! Only the fields the transport reads are modeled; unknown fields are ignored.
//! All inbound types are deserialize-only; request bodies are built ad hoc in
//! [`super::api`].

// Some fields are modeled to document the protocol shape even when the transport
// does not yet read them (e.g. group migration, bot flag).
#![allow(dead_code)]

use serde::Deserialize;

/// The generic Telegram response envelope.
///
/// Every Bot API call returns `{ ok, result?, description?, error_code?,
/// parameters? }`. On `ok: false` the error fields carry the diagnosis.
#[derive(Debug, Deserialize)]
pub struct Envelope<T> {
    /// Whether the call succeeded.
    pub ok: bool,
    /// The result payload, present when `ok` is `true`.
    pub result: Option<T>,
    /// A human-readable error description, present when `ok` is `false`.
    pub description: Option<String>,
    /// The Telegram error code, present when `ok` is `false`.
    pub error_code: Option<i64>,
    /// Extra parameters, notably `retry_after` on `429`.
    pub parameters: Option<ResponseParameters>,
}

/// Auxiliary parameters returned alongside some errors.
#[derive(Debug, Deserialize)]
pub struct ResponseParameters {
    /// Seconds to wait before retrying, on `429`.
    pub retry_after: Option<u64>,
    /// A chat's new id after a group-to-supergroup migration.
    pub migrate_to_chat_id: Option<i64>,
}

/// A single incoming update.
#[derive(Debug, Deserialize)]
pub struct Update {
    /// The update's unique, monotonically increasing id.
    pub update_id: i64,
    /// The new message, when this update is a `message` update.
    pub message: Option<Message>,
}

/// A Telegram message.
#[derive(Debug, Deserialize)]
pub struct Message {
    /// The message id, unique within its chat.
    pub message_id: i64,
    /// The sender, absent for messages sent to channels.
    pub from: Option<User>,
    /// The chat the message belongs to.
    pub chat: Chat,
    /// The send date as a Unix timestamp.
    pub date: i64,
    /// The message text, for text messages.
    pub text: Option<String>,
}

/// A Telegram user or bot.
#[derive(Debug, Deserialize)]
pub struct User {
    /// The user's unique id.
    pub id: i64,
    /// Whether this user is a bot.
    #[serde(default)]
    pub is_bot: bool,
    /// The user's first name.
    #[serde(default)]
    pub first_name: String,
    /// The user's `@username`, if set.
    pub username: Option<String>,
}

/// A Telegram chat.
#[derive(Debug, Deserialize)]
pub struct Chat {
    /// The chat's unique id. Negative for groups and channels.
    pub id: i64,
    /// The chat type (`private`, `group`, `supergroup`, `channel`).
    #[serde(rename = "type", default)]
    pub kind: String,
}
