//! The long-polling [`TelegramTransport`].

mod api;
mod types;

pub use api::TelegramApi;

use std::sync::Arc;
use std::time::Duration;

use time::OffsetDateTime;
use tokio::sync::Mutex;

use crate::chunk::{DEFAULT_CHUNK_LIMIT, chunk_message};
use crate::state::{StateStore, TransportState};
use claudius::ContentBlock;

use crate::transport::{ChatTransport, Inbound, content_blocks_to_text};
use crate::{ChatId, Error, MessageId, UpdateId};

/// The default long-poll timeout, in seconds.
const DEFAULT_POLL_TIMEOUT_SECS: u64 = 50;
/// The per-`getUpdates` cap (Telegram's maximum and default).
const UPDATE_LIMIT: u32 = 100;
/// Maximum send retries before giving up on a `429`.
const MAX_SEND_RETRIES: u32 = 5;
/// Per-chat pacing delay between chunks (Telegram allows ~1 msg/s/chat).
const PER_CHAT_DELAY: Duration = Duration::from_millis(1000);

/// A durable Telegram transport over the long-polling Bot API.
///
/// Shares its [`TransportState`] and [`StateStore`] with the turn loop so that
/// [`ack`](ChatTransport::ack) can advance and persist the `getUpdates` offset
/// atomically. The actual server-side consumption happens lazily on the next
/// [`recv`](ChatTransport::recv).
pub struct TelegramTransport {
    api: TelegramApi,
    poll_timeout_secs: u64,
    state: Arc<Mutex<TransportState>>,
    store: Arc<dyn StateStore>,
    allowed_updates: Vec<String>,
    allowed_user_ids: Vec<i64>,
}

impl TelegramTransport {
    /// Builds a transport for `token`, sharing `state` and `store` with the loop.
    pub fn new(
        token: impl Into<String>,
        state: Arc<Mutex<TransportState>>,
        store: Arc<dyn StateStore>,
    ) -> Result<Self, Error> {
        let poll_timeout_secs = DEFAULT_POLL_TIMEOUT_SECS;
        let api = TelegramApi::new(token, poll_timeout_secs)?;
        Ok(Self {
            api,
            poll_timeout_secs,
            state,
            store,
            allowed_updates: vec!["message".to_string()],
            allowed_user_ids: Vec::new(),
        })
    }

    /// Overrides the long-poll timeout (seconds).
    ///
    /// Rebuilds the underlying HTTP client so its read timeout stays larger than
    /// the poll window.
    pub fn with_poll_timeout(mut self, secs: u64, token: impl Into<String>) -> Result<Self, Error> {
        self.poll_timeout_secs = secs;
        self.api = TelegramApi::new(token, secs)?;
        Ok(self)
    }

    /// Restricts inbound messages to the given Telegram user ids.
    ///
    /// Passing an empty iterator restores the default allow-all policy. Messages
    /// from other users, or messages without a sender id, are acknowledged and
    /// ignored by the shared turn loop.
    pub fn with_allowed_user_ids<I>(mut self, user_ids: I) -> Self
    where
        I: IntoIterator<Item = i64>,
    {
        self.allowed_user_ids = user_ids.into_iter().collect();
        self
    }

    /// Returns the bot's own `@username` (a connectivity/auth check).
    ///
    /// Falls back to the bot's first name if no username is set.
    pub async fn get_me(&self) -> Result<String, Error> {
        let me = self.api.get_me().await?;
        Ok(me.username.unwrap_or(me.first_name))
    }

    /// Defensively deletes any configured webhook (avoids `getUpdates` `409`).
    pub async fn delete_webhook(&self) -> Result<(), Error> {
        self.api.delete_webhook().await
    }

    /// Sends `text` to `chat_id` with chunking and `429`/`403` handling.
    ///
    /// Returns the [`MessageId`] of the last chunk sent. On `403` it returns the
    /// error immediately without retrying so the caller can mark the channel dead.
    async fn send_text(&self, chat_id: i64, text: &str) -> Result<MessageId, Error> {
        let chunks = chunk_message(text, DEFAULT_CHUNK_LIMIT);
        if chunks.is_empty() {
            // Nothing to send; surface as an internal no-op error rather than
            // silently returning a bogus message id.
            return Err(Error::Internal(
                "refusing to send empty message".to_string(),
            ));
        }
        let mut last_id = MessageId(0);
        for (i, chunk) in chunks.iter().enumerate() {
            if i > 0 {
                tokio::time::sleep(PER_CHAT_DELAY).await;
            }
            let mut attempt = 0u32;
            loop {
                match self.api.send_message(chat_id, chunk).await {
                    Ok(msg) => {
                        last_id = MessageId(msg.message_id);
                        break;
                    }
                    Err(Error::Api {
                        code: 403,
                        description,
                        retry_after,
                    }) => {
                        // Permanent for this chat: do not retry.
                        return Err(Error::Api {
                            code: 403,
                            description,
                            retry_after,
                        });
                    }
                    Err(Error::Api {
                        code: 429,
                        description,
                        retry_after,
                    }) => {
                        attempt += 1;
                        if attempt > MAX_SEND_RETRIES {
                            return Err(Error::Api {
                                code: 429,
                                description,
                                retry_after,
                            });
                        }
                        let wait = retry_after.unwrap_or(1);
                        eprintln!(
                            "[telegram] 429 on sendMessage chat_id={chat_id} retry_after={wait}s attempt={attempt}"
                        );
                        // Sleep at least the advised window; premature retry
                        // resets Telegram's penalty and doubles the next wait.
                        tokio::time::sleep(Duration::from_secs(wait) + Duration::from_millis(250))
                            .await;
                    }
                    Err(other) => return Err(other),
                }
            }
        }
        Ok(last_id)
    }
}

#[async_trait::async_trait]
impl ChatTransport for TelegramTransport {
    fn allowed_user_ids(&self) -> &[i64] {
        &self.allowed_user_ids
    }

    async fn recv(&mut self) -> Result<Vec<Inbound>, Error> {
        let offset = self.state.lock().await.next_offset;
        let updates = self
            .api
            .get_updates(
                offset,
                UPDATE_LIMIT,
                self.poll_timeout_secs,
                &self.allowed_updates,
            )
            .await?;

        let mut inbound = Vec::new();
        for update in updates {
            // Defensively skip non-text and non-message updates: allowed_updates
            // is not retroactive, so stragglers can still arrive.
            let Some(message) = update.message else {
                continue;
            };
            let Some(text) = message.text else {
                continue;
            };
            let timestamp = OffsetDateTime::from_unix_timestamp(message.date)
                .unwrap_or_else(|_| OffsetDateTime::now_utc());
            inbound.push(Inbound {
                update_id: UpdateId(update.update_id),
                chat: ChatId(message.chat.id),
                text,
                from_user_id: message.from.map(|u| u.id),
                timestamp,
            });
        }
        // Do NOT advance state.next_offset here; only ack may do that.
        Ok(inbound)
    }

    async fn ack(&mut self, up_to: UpdateId) -> Result<(), Error> {
        let snapshot = {
            let mut state = self.state.lock().await;
            // Advance only forward; an out-of-order ack must not rewind.
            let candidate = up_to.0 + 1;
            if candidate > state.next_offset {
                state.next_offset = candidate;
            }
            state.clone()
        };
        self.store.commit(&snapshot).await
    }

    async fn send(&self, chat: ChatId, content: Vec<ContentBlock>) -> Result<MessageId, Error> {
        let text = content_blocks_to_text(&content);
        self.send_text(chat.0, &text).await
    }

    async fn typing(&self, chat: ChatId) -> Result<(), Error> {
        match self.api.send_chat_action(chat.0, "typing").await {
            Ok(()) => Ok(()),
            // Surface 403 (dead channel); swallow everything else best-effort.
            Err(Error::Api {
                code: 403,
                description,
                retry_after,
            }) => Err(Error::Api {
                code: 403,
                description,
                retry_after,
            }),
            Err(_) => Ok(()),
        }
    }
}
