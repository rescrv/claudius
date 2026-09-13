//! A thin, typed `reqwest` wrapper over the Telegram Bot API.
//!
//! Every call is `POST https://api.telegram.org/bot<TOKEN>/<method>` with a JSON
//! body and a JSON response envelope. On `ok: false` the call returns
//! [`Error::Api`] carrying the code, description, and any `retry_after`.
//!
//! **The bot token is never logged.** It lives only inside the per-request URL,
//! is never placed in an error string, and the [`TelegramApi`] `Debug` impl
//! redacts it.

use std::fmt;
use std::time::Duration;

use serde_json::json;

use super::types::{Envelope, Message, Update, User};
use crate::Error;

const API_BASE: &str = "https://api.telegram.org";

/// Resolves a bot token value, handling `file://` URLs.
///
/// Mirrors `claudius`'s `Anthropic::resolve_api_key`: a value beginning with
/// `file://` names a file (absolute or relative) whose contents are the token;
/// the contents are trimmed. Any other value is used as-is, with surrounding
/// whitespace trimmed so a stray trailing newline never reaches the request URL.
fn resolve_token(value: &str) -> Result<String, Error> {
    if let Some(path) = value.strip_prefix("file://") {
        std::fs::read_to_string(path)
            .map(|content| content.trim().to_string())
            .map_err(|e| {
                Error::Internal(format!("failed to read bot token from file '{path}': {e}"))
            })
    } else {
        Ok(value.trim().to_string())
    }
}

/// A typed Telegram Bot API client.
pub struct TelegramApi {
    client: reqwest::Client,
    token: String,
}

impl fmt::Debug for TelegramApi {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Never leak the token.
        f.debug_struct("TelegramApi")
            .field("token", &"<redacted>")
            .finish()
    }
}

impl TelegramApi {
    /// Builds a client whose HTTP read timeout exceeds the long-poll window.
    ///
    /// The read timeout MUST be larger than `poll_timeout_secs` or long polls
    /// would be cancelled by the client before Telegram responds; here it is
    /// `poll_timeout_secs + 10`.
    pub fn new(token: impl Into<String>, poll_timeout_secs: u64) -> Result<Self, Error> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(poll_timeout_secs + 10))
            .build()?;
        // Resolve the token the same way `claudius` resolves `CLAUDIUS_API_KEY`
        // (see `Anthropic::resolve_api_key`): a `file://` value names a file
        // whose (trimmed) contents are the token. Trimming also strips the
        // trailing `\n` that an env var or `$(cat token)` often carries, which
        // would otherwise land in the request URL and make Telegram answer `404`.
        let token = resolve_token(&token.into())?;
        Ok(Self { client, token })
    }

    fn url(&self, method: &str) -> String {
        format!("{API_BASE}/bot{}/{method}", self.token)
    }

    /// POSTs `body` to `method` and unwraps the response envelope.
    async fn call<T: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        body: serde_json::Value,
    ) -> Result<T, Error> {
        let resp = self
            .client
            .post(self.url(method))
            .json(&body)
            .send()
            .await?;
        let envelope: Envelope<T> = resp.json().await?;
        if envelope.ok {
            envelope.result.ok_or_else(|| {
                Error::Internal(format!("Telegram {method}: ok=true but missing result"))
            })
        } else {
            Err(Error::Api {
                code: envelope.error_code.unwrap_or(0),
                description: envelope
                    .description
                    .unwrap_or_else(|| "unknown error".to_string()),
                retry_after: envelope.parameters.and_then(|p| p.retry_after),
            })
        }
    }

    /// Long-polls for updates.
    ///
    /// An update is consumed only once a later call passes an `offset` strictly
    /// greater than its `update_id`; failing to advance re-delivers forever.
    pub async fn get_updates(
        &self,
        offset: i64,
        limit: u32,
        timeout: u64,
        allowed_updates: &[String],
    ) -> Result<Vec<Update>, Error> {
        let body = json!({
            "offset": offset,
            "limit": limit,
            "timeout": timeout,
            "allowed_updates": allowed_updates,
        });
        self.call("getUpdates", body).await
    }

    /// Sends a text message and returns the resulting [`Message`].
    pub async fn send_message(&self, chat_id: i64, text: &str) -> Result<Message, Error> {
        let body = json!({ "chat_id": chat_id, "text": text });
        self.call("sendMessage", body).await
    }

    /// Sends a chat action (e.g. `"typing"`).
    pub async fn send_chat_action(&self, chat_id: i64, action: &str) -> Result<(), Error> {
        let body = json!({ "chat_id": chat_id, "action": action });
        // result is `true`; we discard it.
        let _: bool = self.call("sendChatAction", body).await?;
        Ok(())
    }

    /// Returns the bot's own [`User`] record (a connectivity/auth check).
    pub async fn get_me(&self) -> Result<User, Error> {
        self.call("getMe", json!({})).await
    }

    /// Defensively removes any configured webhook so `getUpdates` won't return
    /// `409`.
    pub async fn delete_webhook(&self) -> Result<(), Error> {
        let _: bool = self.call("deleteWebhook", json!({})).await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_token_plain_value() {
        assert_eq!(resolve_token("123:ABC").unwrap(), "123:ABC");
    }

    #[test]
    fn resolve_token_trims_plain_value() {
        // A trailing newline (env var / `$(cat token)`) must not reach the URL.
        assert_eq!(resolve_token("123:ABC\n").unwrap(), "123:ABC");
    }

    #[test]
    fn resolve_token_file_url_absolute() {
        let test_dir =
            std::env::temp_dir().join(format!("claudius_tg_test_{}", std::process::id()));
        std::fs::create_dir_all(&test_dir).unwrap();
        let test_file = test_dir.join("token.txt");
        std::fs::write(&test_file, "123:ABC-from-file\n").unwrap();

        let file_url = format!("file://{}", test_file.display());
        let result = resolve_token(&file_url);

        std::fs::remove_dir_all(&test_dir).unwrap();

        assert_eq!(result.unwrap(), "123:ABC-from-file");
    }

    #[test]
    fn resolve_token_file_url_trims_whitespace() {
        let test_file =
            std::env::temp_dir().join(format!("claudius_tg_ws_{}.txt", std::process::id()));
        std::fs::write(&test_file, "  123:ABC-ws  \n  ").unwrap();

        let file_url = format!("file://{}", test_file.display());
        let result = resolve_token(&file_url);

        std::fs::remove_file(&test_file).unwrap();

        assert_eq!(result.unwrap(), "123:ABC-ws");
    }

    #[test]
    fn resolve_token_file_url_nonexistent_errors_without_leaking_token() {
        let result = resolve_token("file:///nonexistent/path/to/token.txt");
        let err = result.unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("failed to read bot token from file"));
        assert!(msg.contains("/nonexistent/path/to/token.txt"));
    }
}
