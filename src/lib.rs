#![deny(missing_docs)]

//! Claudius: A Rust client for the Anthropic API.
//!
//! This crate provides a comprehensive client implementation for interacting with
//! Anthropic's Claude AI models, including support for streaming responses, tool use,
//! and agent-based interactions.

pub mod chat;

mod accumulating_stream;
mod agent;
mod backoff;
mod cache_control;
mod client;
mod client_logger;
mod error;
mod json_schema;
mod observability;
mod prompt;
mod render;
mod sse;
mod types;

pub use accumulating_stream::AccumulatingStream;
pub use agent::{
    Agent, Budget, FileSystem, IntermediateToolResult, Mount, MountHierarchy, Permissions,
    TokenKind, Tool, ToolCallback, ToolResult, ToolSearchFileSystem, TurnOutcome, TurnStep,
};
pub use client::{Anthropic, LoggingStream};
pub use client_logger::ClientLogger;
pub use error::{Error, Result};
pub use json_schema::JsonSchema;
pub use observability::register_biometrics;
pub use prompt::{
    PromptTestConfig, PromptTestResult, assert_contains, assert_max_length, assert_min_length,
    assert_not_contains, assert_test_passed, test_prompt,
};
pub use render::{AgentStreamContext, PlainTextRenderer, Renderer, StreamContext};
pub use types::*;

/// Pushes a message to the messages vector, or merges it with the last message if they have the same role.
///
/// This function helps maintain a clean message history by combining consecutive messages
/// from the same role into a single message entry.
pub fn push_or_merge_message(messages: &mut Vec<MessageParam>, to_push: MessageParam) {
    if let Some(last) = messages.last_mut() {
        if last.role != to_push.role {
            messages.push(to_push);
        } else {
            merge_message_content(&mut last.content, to_push.content);
        }
    } else {
        messages.push(to_push);
    }
}

/// Merges new message content into existing message content.
///
/// Handles all combinations of string and array content types, converting between
/// them as necessary to produce a unified message content.
pub fn merge_message_content(existing: &mut MessageParamContent, new: MessageParamContent) {
    match (&mut *existing, new) {
        (MessageParamContent::Array(existing_blocks), MessageParamContent::Array(new_blocks)) => {
            existing_blocks.extend(new_blocks);
        }
        (MessageParamContent::Array(existing_blocks), MessageParamContent::String(new_string)) => {
            existing_blocks.push(ContentBlock::Text(crate::TextBlock::new(new_string)));
        }
        (MessageParamContent::String(existing_string), MessageParamContent::Array(new_blocks)) => {
            let mut combined = vec![ContentBlock::Text(crate::TextBlock::new(
                existing_string.clone(),
            ))];
            combined.extend(new_blocks);
            *existing = MessageParamContent::Array(combined);
        }
        (MessageParamContent::String(existing_string), MessageParamContent::String(new_string)) => {
            existing_string.push_str(&new_string);
        }
    }
}
