//! [`StdinTransport`]: the terminal behavior behind [`ChatTransport`].
//!
//! Reimplementing the terminal loop behind the same trait lets the generic
//! [`run_agent_loop`](crate::run_agent_loop) drive a stdin REPL unchanged, which
//! is useful as a token-free integration harness.
//!
//! Slash-command handling (`/today`, `/help`, ...) is intentionally **not** here:
//! that is application-specific and belongs in the binary, which can intercept it
//! via [`LoopConfig::pre_filter`](crate::LoopConfig).

use std::io::{BufRead, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};

use crate::transport::{ChatTransport, Inbound, Outbound};
use crate::{ChatId, Error, MessageId, UpdateId};

/// The fixed chat id used for the single terminal "chat".
const STDIN_CHAT: ChatId = ChatId(0);

/// A terminal transport: line-buffered stdin in, `println!` out.
pub struct StdinTransport {
    next_update_id: AtomicI64,
    shutdown: Arc<AtomicBool>,
    prompt: String,
}

impl StdinTransport {
    /// Creates a stdin transport that signals `shutdown` on EOF.
    pub fn new(shutdown: Arc<AtomicBool>) -> Self {
        Self {
            next_update_id: AtomicI64::new(1),
            shutdown,
            prompt: "> ".to_string(),
        }
    }

    /// Sets the input prompt printed before each read (default `"> "`).
    pub fn with_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.prompt = prompt.into();
        self
    }
}

#[async_trait::async_trait]
impl ChatTransport for StdinTransport {
    async fn recv(&mut self) -> Result<Vec<Inbound>, Error> {
        let prompt = self.prompt.clone();
        let line = tokio::task::spawn_blocking(move || {
            print!("{prompt}");
            let _ = std::io::stdout().flush();
            let mut buf = String::new();
            let n = std::io::stdin().lock().read_line(&mut buf)?;
            Ok::<Option<String>, std::io::Error>(if n == 0 { None } else { Some(buf) })
        })
        .await
        .map_err(|e| Error::Internal(format!("stdin join error: {e}")))??;

        match line {
            None => {
                // EOF: signal shutdown and return an empty batch.
                self.shutdown.store(true, Ordering::SeqCst);
                Ok(Vec::new())
            }
            Some(text) => {
                let text = text.trim_end_matches(['\n', '\r']).to_string();
                if text.is_empty() {
                    return Ok(Vec::new());
                }
                let id = self.next_update_id.fetch_add(1, Ordering::SeqCst);
                Ok(vec![Inbound::new(UpdateId(id), STDIN_CHAT, text)])
            }
        }
    }

    async fn ack(&mut self, _up_to: UpdateId) -> Result<(), Error> {
        // The terminal has no durable cursor.
        Ok(())
    }

    async fn send(&self, out: Outbound) -> Result<MessageId, Error> {
        // Terminals don't care about the 4096 limit; print verbatim.
        println!("{}", out.text);
        let _ = std::io::stdout().flush();
        Ok(MessageId(0))
    }

    async fn typing(&self, _chat: ChatId) -> Result<(), Error> {
        Ok(())
    }
}
