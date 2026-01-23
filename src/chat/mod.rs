//! Chat application module for interactive conversations with Claude.
//!
//! This module provides a streaming REPL chat interface built on top of the
//! claudius client library. It supports:
//!
//! - Streaming responses with real-time token display
//! - ANSI-styled output for thinking blocks
//! - Slash commands for session control
//! - Configurable model, system prompt, and parameters
//!
//! # Architecture
//!
//! The module is organized into several components:
//!
//! - [`config`]: CLI argument parsing and configuration
//! - [`session`]: Core chat session management and API interaction
//! - [`commands`]: Slash command parsing and handling

mod commands;
mod config;
mod session;

pub use crate::render::{PlainTextRenderer, Renderer, StreamContext};
pub use commands::{ChatCommand, help_text, parse_command};
pub use config::{ChatArgs, ChatArgsError, ChatConfig};
pub use session::{ChatAgent, ChatSession, ConfigAgent, SessionStats};
