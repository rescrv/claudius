//! The [`ChatTransport`] trait and its inbound message type.
//!
//! A transport is the seam between an agent's turn loop and the outside world.
//! Two implementations ship with this crate: [`crate::StdinTransport`] (terminal
//! I/O for testing) and [`crate::TelegramTransport`] (long-polling Bot API). A
//! `WebhookTransport` could implement the same trait in the future; v1 is
//! long-polling only.

use time::OffsetDateTime;

use claudius::{ContentBlock, TextBlock};

use crate::{ChatId, Error, MessageId, UpdateId};

/// An inbound message delivered to the agent.
///
/// `#[non_exhaustive]` so media, inline keyboards, and other Telegram features
/// can be added without a breaking change.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Inbound {
    /// The Telegram update id; the durability cursor advances past this on `ack`.
    pub update_id: UpdateId,
    /// The chat the message arrived in. Persisted on first contact to enable
    /// later proactive sends (Telegram forbids initiating a conversation).
    pub chat: ChatId,
    /// The message text.
    pub text: String,
    /// The sending user's id, if present.
    pub from_user_id: Option<i64>,
    /// The message timestamp, in UTC.
    pub timestamp: OffsetDateTime,
}

impl Inbound {
    /// Constructs an [`Inbound`] with the required fields.
    pub fn new(update_id: UpdateId, chat: ChatId, text: impl Into<String>) -> Self {
        Self {
            update_id,
            chat,
            text: text.into(),
            from_user_id: None,
            timestamp: OffsetDateTime::now_utc(),
        }
    }
}

/// Builds a single text [`ContentBlock`].
pub fn text_content(text: impl Into<String>) -> Vec<ContentBlock> {
    vec![ContentBlock::Text(TextBlock::new(text))]
}

/// Extracts chat-visible text from content blocks.
///
/// Telegram can only send text in this transport, so only text blocks are
/// concatenated. Thinking, redacted thinking, tool use, and other non-text blocks
/// are intentionally ignored.
pub fn content_blocks_to_text(content: &[ContentBlock]) -> String {
    content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::Text(text) => Some(text.text.as_str()),
            _ => None,
        })
        .collect()
}

/// Returns whether any text block contains visible text.
pub fn content_blocks_have_text(content: &[ContentBlock]) -> bool {
    content.iter().any(|block| match block {
        ContentBlock::Text(text) => !text.text.is_empty(),
        _ => false,
    })
}

/// A duplex chat transport with a split receive/ack cursor.
///
/// **Design rationale:** [`recv`](ChatTransport::recv) and
/// [`ack`](ChatTransport::ack) are split precisely so the durable cursor never
/// advances until the caller has committed the turn's side effects. This is what
/// makes the turn loop restart-safe. Do not "optimize" by advancing the offset
/// inside `recv`.
#[async_trait::async_trait]
pub trait ChatTransport: Send {
    /// Returns the Telegram user ids allowed to drive this transport.
    ///
    /// An empty slice means "allow every user". Transports that cannot identify
    /// a sender should keep the default unless they intentionally want the
    /// shared loop to reject messages without `from_user_id`.
    fn allowed_user_ids(&self) -> &[i64] {
        &[]
    }

    /// Returns whether an inbound sender is allowed to drive the agent.
    ///
    /// The default policy allows everyone when [`allowed_user_ids`](Self::allowed_user_ids)
    /// is empty, and otherwise requires a present user id in that allow-list.
    fn is_user_allowed(&self, from_user_id: Option<i64>) -> bool {
        let allowed = self.allowed_user_ids();
        allowed.is_empty() || from_user_id.is_some_and(|id| allowed.contains(&id))
    }

    /// Blocks until at least one inbound message is available, or the poll window
    /// elapses (returning an empty `Vec`).
    ///
    /// Implementations MUST NOT advance any durable cursor here -- only
    /// [`ack`](ChatTransport::ack) may do that.
    async fn recv(&mut self) -> Result<Vec<Inbound>, Error>;

    /// Durably marks all updates with id `<= up_to` as consumed.
    ///
    /// For Telegram this advances the persisted `getUpdates` offset (realized
    /// lazily on the next `recv`); for stdin it is a no-op.
    async fn ack(&mut self, up_to: UpdateId) -> Result<(), Error>;

    /// Sends one message, returning the server message id of the last chunk sent.
    ///
    /// The transport extracts text from `content` and discards thinking and other
    /// non-text blocks at the chat boundary.
    ///
    /// On `429` the implementation handles `retry_after` internally and retries.
    /// On `403` it returns [`Error::Api`] with `code: 403` WITHOUT retrying, so
    /// the caller can mark the channel dead.
    async fn send(&self, chat: ChatId, content: Vec<ContentBlock>) -> Result<MessageId, Error>;

    /// Best-effort presence cue (Telegram "typing" for ~5s). Default: `Ok(())`.
    async fn typing(&self, _chat: ChatId) -> Result<(), Error> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use claudius::ThinkingBlock;

    #[test]
    fn flattening_keeps_text_and_discards_thinking() {
        let content = vec![
            ContentBlock::Thinking(ThinkingBlock::new("internal", "signature")),
            ContentBlock::Text(TextBlock::new("visible")),
            ContentBlock::Text(TextBlock::new(" text")),
        ];

        assert_eq!(content_blocks_to_text(&content), "visible text");
        assert!(content_blocks_have_text(&content));
    }

    #[test]
    fn thinking_only_content_has_no_visible_text() {
        let content = vec![ContentBlock::Thinking(ThinkingBlock::new(
            "internal",
            "signature",
        ))];

        assert_eq!(content_blocks_to_text(&content), "");
        assert!(!content_blocks_have_text(&content));
    }
}
