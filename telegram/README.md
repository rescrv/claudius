# claudius-telegram

A durable Telegram Bot API transport for [`claudius`](https://github.com/rescrv/claudius)
agents. It lets any `claudius::Agent` converse over Telegram instead of a
terminal, with a turn loop that survives process restarts without losing or
double-processing messages.

## What it provides

- **`ChatTransport`** — a duplex transport trait with a split `recv`/`ack` cursor.
  - **`TelegramTransport`** — long-polling Bot API client (offset advancement,
    60s long polls, `429` `retry_after` backoff, `403` dead-channel detection,
    4096-char chunking, optional Telegram user-id allow-list).
  - **`StdinTransport`** — the terminal behavior behind the same trait, for
    token-free integration testing.
  - Transport `send` takes native `Vec<claudius::ContentBlock>` content; concrete
    transports flatten text blocks and discard thinking/non-text blocks at the
    chat boundary.
- **`run_agent_loop`** — a durable turn loop generic over any `claudius::Agent`
  and any `ChatTransport`.
- **`TransportState` + `StateStore`** — the single durable artifact holding the
  poll cursor, per-chat conversation history, the transactional outbox, and the
  dead-chat set, committed atomically (`FileStateStore` uses temp-file +
  `fsync` + rename).
- **`run_synthetic_user_turn` / `send_proactive` / `known_chats`** — the hooks a
  downstream proactive scheduler needs (see "Durability" below).

This crate is transport-only and agent-agnostic; it contains no application
logic.

## Durability model

Inbound delivery is at-least-once: Telegram only treats an update as consumed
once a later `getUpdates` passes a larger `offset`. We make that offset live
**inside the same durable blob** as the conversation history, and the turn loop
commits the conversation *before* it acks (advances the offset). The loop never
exits between commit and ack, so the only replay window is a crash strictly
between "turn produced output" and "state committed."

Telegram `sendMessage` is not idempotent. A reactive agent (`use_outbox: false`)
sends directly and tolerates the rare duplicate. A proactive agent
(`use_outbox: true`) routes sends through the transactional outbox: an intent is
recorded `Pending` and committed before the send, then marked `Sent`. The one
documented at-least-once window is a `Pending` record whose send outcome was
unknown at crash time; it is re-sent on restart.

For model-generated proactive check-ins, use `run_synthetic_user_turn` instead
of sending prebuilt text. The synthetic prompt is persisted as a real user
message, the model generates the assistant response, and later replies see the
full coherent conversation history:

```rust
use claudius_telegram::{known_chats, run_synthetic_user_turn, SyntheticTurnConfig};

let chats = {
    let state = state.lock().await;
    known_chats(&state)
};

for chat in chats {
    let prompt = format!(
        "Current timestamp: {now}\n\n\
         Proactive wakeup: the following scheduled items are due now:\n{items}\n\n\
         Check in with the user about these due items."
    );

    run_synthetic_user_turn(
        &mut agent,
        &transport,
        &client,
        store.as_ref(),
        &state,
        &budget,
        chat,
        prompt,
        SyntheticTurnConfig {
            max_history_messages: Some(80),
            reset_on_context_limit: true,
            use_outbox: true,
            source_label: Some("scheduler".to_string()),
        },
    )
    .await?;
}
```

## Quick start (terminal)

```bash
# Echo smoke test over stdin (no token, no network):
echo hello | cargo run --bin echo -- --stdin
# => HELLO
```

## Live test against a real bot

`api.telegram.org` egress is required and is **not** in the default CI sandbox
allowlist; CI uses an in-memory mock and needs no network.

1. Create a bot with [@BotFather](https://t.me/BotFather) and copy its token.
2. Run the echo bot:

   ```bash
   export TELEGRAM_BOT_TOKEN=123456:ABC-...
   cargo run --bin echo -- --state /tmp/echo-state.json
   ```

   To restrict access, pass Telegram user ids:

   ```bash
   cargo run --bin echo -- --state /tmp/echo-state.json --allowed-user-ids 123,456
   ```

3. Message the bot in Telegram; it replies with your text uppercased.
4. Validate durability: stop the process (Ctrl-C), restart with the same
   `--state` path, and confirm already-handled messages are not reprocessed.
5. Validate chunking: send a message longer than 4096 characters and observe it
   arrive as multiple messages.
6. Validate dead-channel handling: block the bot, send a proactive message, and
   confirm the chat is recorded in `dead_chats`.

Flags: `--token`, `--state`, `--poll-timeout` (default 50s),
`--allowed-user-ids`, `--delete-webhook-on-start`, `--stdin`.

## Testing

```bash
cargo test          # unit + integration; no live network
cargo clippy --all-targets
```

The `transport_contract` suite asserts the load-bearing invariants (cursor never
advances without `ack`, `ack` advances it, `403` returns without retry) against
an in-memory `MockTelegram`. `state_recovery` covers atomic-write integrity and
restart recovery. `runtime_loop` drives the full durable loop end-to-end.

## Non-goals (v1)

Webhooks (a `WebhookTransport` could implement the same trait later),
multi-tenant sharding, live token streaming to Telegram, and non-text content
(media, inline keyboards, polls). `Inbound` is `#[non_exhaustive]` so these can
be added without a breaking change.
