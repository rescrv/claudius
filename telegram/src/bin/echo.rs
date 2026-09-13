//! A non-LLM smoke test that exercises the whole transport without burning
//! tokens.
//!
//! The "agent" echoes the user's text uppercased. It is wired via a pre-filter
//! that [`Handled`](claudius_telegram::PreFilter::Handled)s every message, so the
//! real agent is never invoked and no network/token is required.
//!
//! # Usage
//!
//! ```bash
//! # Terminal mode (no Telegram, no token):
//! echo --stdin
//!
//! # Telegram mode (requires TELEGRAM_BOT_TOKEN and api.telegram.org egress):
//! TELEGRAM_BOT_TOKEN=... echo --state /tmp/echo-state.json
//! ```
//!
//! This validates: long-poll receive, offset advancement across restart, 4096
//! chunking (echo a >4096 input), and 403 dead-chat marking (block the bot).

use std::env;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use arrrg::CommandLine;
use arrrg_derive::CommandLine;
use tokio::sync::Mutex;

use claudius::{Agent, Anthropic, Budget};
use claudius_telegram::{
    Error, FileStateStore, InMemoryStateStore, LoopConfig, PreFilter, StateStore, StdinTransport,
    TelegramTransport, TransportState, run_agent_loop,
};

/// A trivial agent that is never actually invoked (the pre-filter handles every
/// message). It exists only to satisfy `run_agent_loop`'s `A: Agent` bound.
struct EchoAgent;

impl Agent for EchoAgent {}

/// Command-line arguments for the echo smoke-test bot.
#[derive(CommandLine, Debug, Default, Eq, PartialEq)]
struct EchoArgs {
    /// Use the terminal transport instead of Telegram.
    #[arrrg(flag, "Use StdinTransport instead of Telegram")]
    stdin: bool,

    /// Path to the durable state file.
    #[arrrg(optional, "Path to durable state JSON (default: ephemeral)", "PATH")]
    state: Option<String>,

    /// Bot token (otherwise read from TELEGRAM_BOT_TOKEN).
    #[arrrg(optional, "Telegram bot token (default: $TELEGRAM_BOT_TOKEN)", "TOKEN")]
    token: Option<String>,

    /// Long-poll timeout, in seconds.
    #[arrrg(optional, "Long-poll timeout in seconds (default: 50)", "SECS")]
    poll_timeout: Option<u64>,

    /// Delete any active webhook on startup.
    #[arrrg(flag, "Call deleteWebhook on startup")]
    delete_webhook_on_start: bool,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (args, _) = EchoArgs::from_command_line_relaxed("echo [OPTIONS]");

    // Shared interrupt flag, wired to SIGINT.
    let interrupted = Arc::new(AtomicBool::new(false));
    {
        let interrupted = Arc::clone(&interrupted);
        ctrlc::set_handler(move || interrupted.store(true, Ordering::SeqCst)).ok();
    }

    // Durable store + shared state.
    let store: Arc<dyn StateStore> = match &args.state {
        Some(path) => Arc::new(FileStateStore::new(path)),
        None => Arc::new(InMemoryStateStore::new()),
    };
    let loaded: TransportState = store.load().await?;
    let state = Arc::new(Mutex::new(loaded));

    // The echo "agent": every message is handled by the pre-filter, uppercased.
    let pre_filter: Box<dyn Fn(&_) -> PreFilter + Send> =
        Box::new(|inb: &claudius_telegram::Inbound| PreFilter::Handled(inb.text.to_uppercase()));

    let budget = Arc::new(Budget::from_dollars_flat_rate(0.0, 1000));
    let config = LoopConfig {
        store: Arc::clone(&store),
        state: Arc::clone(&state),
        budget,
        use_outbox: false,
        interrupted: Arc::clone(&interrupted),
        pre_filter: Some(pre_filter),
    };

    // A dummy client; never used because the pre-filter handles every message.
    let client = Anthropic::new(Some("dummy-key".to_string()))?;

    if args.stdin {
        let transport = StdinTransport::new(Arc::clone(&interrupted));
        run_agent_loop(EchoAgent, transport, client, config).await?;
    } else {
        let token = args
            .token
            .clone()
            .or_else(|| env::var("TELEGRAM_BOT_TOKEN").ok())
            .ok_or_else(|| {
                Error::Internal(
                    "no token: pass --token or set TELEGRAM_BOT_TOKEN (or use --stdin)".to_string(),
                )
            })?;
        let mut transport = TelegramTransport::new(token.clone(), Arc::clone(&state), Arc::clone(&store))?;
        if let Some(secs) = args.poll_timeout {
            transport = transport.with_poll_timeout(secs, token.clone())?;
        }
        if args.delete_webhook_on_start {
            transport.delete_webhook().await?;
        }
        match transport.get_me().await {
            Ok(username) => eprintln!("[echo] connected as @{username}"),
            Err(err) => eprintln!("[echo] getMe failed: {err}"),
        }
        run_agent_loop(EchoAgent, transport, client, config).await?;
    }

    Ok(())
}
