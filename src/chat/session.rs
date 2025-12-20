//! Core chat session management.
//!
//! This module provides the `ChatSession` struct which manages conversation
//! state and handles streaming API interactions.

use std::collections::HashSet;
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use futures::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::{from_reader, to_string_pretty, to_writer_pretty};
use tokio::pin;

use crate::Error;
use crate::chat::config::ChatConfig;
use crate::chat::render::Renderer;
use crate::client::Anthropic;
use crate::error::Result;
use crate::types::{
    ContentBlock, ContentBlockDelta, MessageCreateParams, MessageDeltaUsage, MessageParam,
    MessageParamContent, MessageRole, MessageStreamEvent, Model, ToolResultBlockContent, Usage,
};

/// A chat session that manages conversation state and API interactions.
///
/// The session maintains message history and handles streaming responses
/// from the Anthropic API.
pub struct ChatSession {
    client: Anthropic,
    messages: Vec<MessageParam>,
    config: ChatConfig,
    usage_totals: Usage,
    last_turn_usage: Option<Usage>,
    request_count: u64,
    budget_spent_tokens: u64,
}

/// Aggregated stats for a chat session.
#[derive(Debug, Clone)]
pub struct SessionStats {
    /// The model used for the session.
    pub model: Model,
    /// The number of messages in the conversation.
    pub message_count: usize,
    /// The maximum tokens per response.
    pub max_tokens: u32,
    /// The system prompt, if any.
    pub system_prompt: Option<String>,
    /// The sampling temperature, if set.
    pub temperature: Option<f32>,
    /// The top-p value, if set.
    pub top_p: Option<f32>,
    /// The top-k value, if set.
    pub top_k: Option<u32>,
    /// The configured stop sequences.
    pub stop_sequences: Vec<String>,
    /// Whether thinking blocks are displayed.
    pub show_thinking: bool,
    /// The session token budget limit, if set.
    pub session_budget_tokens: Option<u64>,
    /// Total tokens spent against the budget.
    pub budget_spent_tokens: u64,
    /// The auto-save transcript path, if set.
    pub transcript_path: Option<PathBuf>,
    /// Total input tokens across all requests.
    pub total_input_tokens: u64,
    /// Total output tokens across all requests.
    pub total_output_tokens: u64,
    /// Total number of API requests made.
    pub total_requests: u64,
    /// Input tokens for the last turn, if available.
    pub last_turn_input_tokens: Option<u64>,
    /// Output tokens for the last turn, if available.
    pub last_turn_output_tokens: Option<u64>,
}

impl ChatSession {
    /// Creates a new chat session with the given client and configuration.
    pub fn new(client: Anthropic, config: ChatConfig) -> Self {
        Self {
            client,
            messages: Vec::new(),
            config,
            usage_totals: Usage::new(0, 0),
            last_turn_usage: None,
            request_count: 0,
            budget_spent_tokens: 0,
        }
    }

    /// Sends a user message and streams the response.
    ///
    /// This method:
    /// 1. Adds the user message to history
    /// 2. Sends a streaming request to the API
    /// 3. Renders response chunks as they arrive
    /// 4. Adds the complete assistant response to history
    ///
    /// The `interrupted` flag can be set to `true` to cancel the stream.
    ///
    /// # Errors
    ///
    /// Returns an error if the API request fails.
    pub async fn send_streaming(
        &mut self,
        user_input: &str,
        renderer: &mut dyn Renderer,
        interrupted: Arc<AtomicBool>,
    ) -> Result<()> {
        if let Some(limit) = self.config.session_budget_tokens
            && self.budget_spent_tokens >= limit
        {
            renderer.print_error(
                "Session budget exhausted. Use /budget to increase or clear the limit.",
            );
            return Err(Error::bad_request(
                "session budget exhausted",
                Some("budget".to_string()),
            ));
        }

        // Add user message to history
        self.messages.push(MessageParam {
            role: MessageRole::User,
            content: MessageParamContent::String(user_input.to_string()),
        });

        // Build request parameters
        let mut params = MessageCreateParams::new(
            self.config.max_tokens,
            self.messages.clone(),
            self.config.model.clone(),
        )
        .with_stream(true);

        if let Some(ref system) = self.config.system_prompt {
            params = params.with_system_string(system.clone());
        }
        if let Some(temp) = self.config.temperature {
            params = params.with_temperature(temp)?;
        }
        if let Some(top_p) = self.config.top_p {
            params = params.with_top_p(top_p)?;
        }
        if let Some(top_k) = self.config.top_k {
            params = params.with_top_k(top_k);
        }
        if !self.config.stop_sequences.is_empty() {
            params = params.with_stop_sequences(self.config.stop_sequences.clone());
        }

        let mut accumulated_text = String::new();
        let mut was_interrupted = false;
        let mut active_tool_uses: HashSet<usize> = HashSet::new();
        let mut active_tool_results: HashSet<usize> = HashSet::new();
        let mut turn_usage: Option<Usage> = None;

        {
            let stream = self.client.stream(params).await?;
            pin!(stream);

            // Process stream events
            while let Some(event) = stream.next().await {
                // Check for interrupt
                if interrupted.load(Ordering::Relaxed) {
                    was_interrupted = true;
                    renderer.print_interrupted();
                    break;
                }

                match event {
                    Ok(event) => match &event {
                        MessageStreamEvent::Ping | MessageStreamEvent::MessageStart(_) => {}
                        MessageStreamEvent::MessageDelta(delta_event) => {
                            turn_usage = Some(usage_from_delta(&delta_event.usage));
                        }
                        MessageStreamEvent::ContentBlockStart(start_event) => {
                            match &start_event.content_block {
                                ContentBlock::ToolUse(tool_use) => {
                                    active_tool_uses.insert(start_event.index);
                                    renderer.start_tool_use(&tool_use.name, &tool_use.id);
                                }
                                ContentBlock::ToolResult(tool_result) => {
                                    active_tool_results.insert(start_event.index);
                                    renderer.start_tool_result(
                                        &tool_result.tool_use_id,
                                        tool_result.is_error.unwrap_or(false),
                                    );
                                    if let Some(content) = &tool_result.content {
                                        render_tool_result_content(renderer, content);
                                    }
                                }
                                _ => {}
                            }
                        }
                        MessageStreamEvent::ContentBlockDelta(delta_event) => {
                            match &delta_event.delta {
                                ContentBlockDelta::InputJsonDelta(json_delta) => {
                                    if active_tool_uses.contains(&delta_event.index) {
                                        renderer.print_tool_input(&json_delta.partial_json);
                                    }
                                }
                                ContentBlockDelta::TextDelta(text_delta) => {
                                    if active_tool_results.contains(&delta_event.index) {
                                        renderer.print_tool_result_text(&text_delta.text);
                                    } else {
                                        renderer.print_text(&text_delta.text);
                                        accumulated_text.push_str(&text_delta.text);
                                    }
                                }
                                ContentBlockDelta::ThinkingDelta(thinking_delta) => {
                                    if self.config.show_thinking {
                                        renderer.print_thinking(&thinking_delta.thinking);
                                    }
                                }
                                _ => {}
                            }
                        }
                        MessageStreamEvent::ContentBlockStop(stop_event) => {
                            if active_tool_uses.remove(&stop_event.index) {
                                renderer.finish_tool_use();
                            }
                            if active_tool_results.remove(&stop_event.index) {
                                renderer.finish_tool_result();
                            }
                        }
                        MessageStreamEvent::MessageStop(_) => break,
                    },
                    Err(e) => {
                        renderer.print_error(&e.to_string());
                        // Remove the user message since the request failed
                        self.messages.pop();
                        return Err(e);
                    }
                }
            }
        }

        // Finish the response (newline, reset styling)
        if !was_interrupted {
            renderer.finish_response();
        }

        // Add assistant response to history (even if interrupted, include what we got)
        if !accumulated_text.is_empty() {
            self.messages.push(MessageParam {
                role: MessageRole::Assistant,
                content: MessageParamContent::String(accumulated_text),
            });
        } else if was_interrupted {
            // If interrupted with no text, remove the user message
            self.messages.pop();
        }

        if let Some(usage) = turn_usage {
            self.record_usage(usage);
        } else {
            self.last_turn_usage = None;
        }

        self.auto_save_transcript()?;

        Ok(())
    }

    /// Clears the conversation history.
    pub fn clear(&mut self) {
        self.messages.clear();
    }

    /// Returns the number of messages in the conversation.
    pub fn message_count(&self) -> usize {
        self.messages.len()
    }

    /// Changes the model used for responses.
    pub fn set_model(&mut self, model: Model) {
        self.config.model = model;
    }

    /// Returns the current model.
    pub fn model(&self) -> &Model {
        &self.config.model
    }

    /// Sets or clears the system prompt.
    pub fn set_system_prompt(&mut self, prompt: Option<String>) {
        self.config.system_prompt = prompt;
    }

    /// Returns the current system prompt, if any.
    pub fn system_prompt(&self) -> Option<&str> {
        self.config.system_prompt.as_deref()
    }

    /// Sets the maximum tokens per response.
    pub fn set_max_tokens(&mut self, max_tokens: u32) {
        self.config.max_tokens = max_tokens;
    }

    /// Sets the sampling temperature.
    pub fn set_temperature(&mut self, temperature: Option<f32>) {
        self.config.temperature = temperature;
    }

    /// Sets the top-p value.
    pub fn set_top_p(&mut self, top_p: Option<f32>) {
        self.config.top_p = top_p;
    }

    /// Sets the top-k value.
    pub fn set_top_k(&mut self, top_k: Option<u32>) {
        self.config.top_k = top_k;
    }

    /// Adds a stop sequence to the persistent list.
    pub fn add_stop_sequence(&mut self, sequence: String) {
        if !self
            .config
            .stop_sequences
            .iter()
            .any(|existing| existing == &sequence)
        {
            self.config.stop_sequences.push(sequence);
        }
    }

    /// Clears all stop sequences.
    pub fn clear_stop_sequences(&mut self) {
        self.config.stop_sequences.clear();
    }

    /// Returns the configured stop sequences.
    pub fn stop_sequences(&self) -> &[String] {
        &self.config.stop_sequences
    }

    /// Controls whether thinking blocks are rendered.
    pub fn set_show_thinking(&mut self, show: bool) {
        self.config.show_thinking = show;
    }

    /// Returns whether thinking blocks are rendered.
    pub fn show_thinking(&self) -> bool {
        self.config.show_thinking
    }

    /// Sets the session token budget.
    pub fn set_session_budget(&mut self, budget: Option<u64>) {
        self.config.session_budget_tokens = budget;
    }

    /// Returns the remaining session budget, if any.
    pub fn session_budget_remaining(&self) -> Option<i64> {
        self.config.session_budget_tokens.map(|limit| {
            let spent = self.budget_spent_tokens as i64;
            limit as i64 - spent
        })
    }

    /// Sets the auto-save transcript path.
    pub fn set_transcript_path(&mut self, path: Option<PathBuf>) {
        self.config.transcript_path = path;
    }

    /// Returns the configured transcript path, if any.
    pub fn transcript_path(&self) -> Option<&Path> {
        self.config.transcript_path.as_deref()
    }

    /// Saves the transcript to the specified path.
    pub fn save_transcript_to<P: AsRef<Path>>(&self, path: P) -> Result<()> {
        let transcript = TranscriptFile::new(&self.messages);
        let file = File::create(path.as_ref())
            .map_err(|err| Error::io("failed to create transcript file", err))?;
        let writer = BufWriter::new(file);
        to_writer_pretty(writer, &transcript).map_err(|err| {
            Error::serialization("failed to serialize transcript", Some(Box::new(err)))
        })
    }

    /// Loads a transcript from disk, replacing the current conversation history.
    pub fn load_transcript_from<P: AsRef<Path>>(&mut self, path: P) -> Result<()> {
        let file = File::open(path.as_ref())
            .map_err(|err| Error::io("failed to open transcript file", err))?;
        let reader = BufReader::new(file);
        let transcript: TranscriptFile = from_reader(reader).map_err(|err| {
            Error::serialization("failed to parse transcript", Some(Box::new(err)))
        })?;
        self.messages = transcript.messages;
        Ok(())
    }

    /// Returns the current session statistics snapshot.
    pub fn stats(&self) -> SessionStats {
        SessionStats {
            model: self.config.model.clone(),
            message_count: self.message_count(),
            max_tokens: self.config.max_tokens,
            system_prompt: self.config.system_prompt.clone(),
            temperature: self.config.temperature,
            top_p: self.config.top_p,
            top_k: self.config.top_k,
            stop_sequences: self.config.stop_sequences.clone(),
            show_thinking: self.config.show_thinking,
            session_budget_tokens: self.config.session_budget_tokens,
            budget_spent_tokens: self.budget_spent_tokens,
            transcript_path: self.config.transcript_path.clone(),
            total_input_tokens: tokens_to_u64(self.usage_totals.input_tokens),
            total_output_tokens: tokens_to_u64(self.usage_totals.output_tokens),
            total_requests: self.request_count,
            last_turn_input_tokens: self
                .last_turn_usage
                .map(|usage| tokens_to_u64(usage.input_tokens)),
            last_turn_output_tokens: self
                .last_turn_usage
                .map(|usage| tokens_to_u64(usage.output_tokens)),
        }
    }

    fn record_usage(&mut self, usage: Usage) {
        self.last_turn_usage = Some(usage);
        self.usage_totals = self.usage_totals + usage;
        self.request_count = self.request_count.saturating_add(1);
        let turn_total = tokens_to_u64(usage.input_tokens) + tokens_to_u64(usage.output_tokens);
        self.budget_spent_tokens = self.budget_spent_tokens.saturating_add(turn_total);
    }

    fn auto_save_transcript(&self) -> Result<()> {
        if let Some(path) = &self.config.transcript_path {
            self.save_transcript_to(path)
        } else {
            Ok(())
        }
    }
}

#[derive(Serialize, Deserialize)]
struct TranscriptFile {
    version: u8,
    messages: Vec<MessageParam>,
}

impl TranscriptFile {
    fn new(messages: &[MessageParam]) -> Self {
        Self {
            version: 1,
            messages: messages.to_vec(),
        }
    }
}

fn render_tool_result_content(renderer: &mut dyn Renderer, content: &ToolResultBlockContent) {
    match content {
        ToolResultBlockContent::String(text) => renderer.print_tool_result_text(text),
        _ => match to_string_pretty(content) {
            Ok(json) => renderer.print_tool_result_text(&json),
            Err(_) => renderer.print_tool_result_text("<unrenderable tool result>"),
        },
    }
}

fn usage_from_delta(delta_usage: &MessageDeltaUsage) -> Usage {
    let mut usage = Usage::new(
        delta_usage.input_tokens.unwrap_or(0),
        delta_usage.output_tokens,
    );
    if let Some(cache) = delta_usage.cache_creation_input_tokens {
        usage = usage.with_cache_creation_input_tokens(cache);
    }
    if let Some(cache_read) = delta_usage.cache_read_input_tokens {
        usage = usage.with_cache_read_input_tokens(cache_read);
    }
    if let Some(server_tool) = &delta_usage.server_tool_use {
        usage = usage.with_server_tool_use(*server_tool);
    }
    usage
}

fn tokens_to_u64(value: i32) -> u64 {
    value.max(0) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::KnownModel;

    #[test]
    fn new_session_empty() {
        let client = Anthropic::new(None).unwrap();
        let config = ChatConfig::default();
        let session = ChatSession::new(client, config);
        assert_eq!(session.message_count(), 0);
    }

    #[test]
    fn clear_session() {
        let client = Anthropic::new(None).unwrap();
        let config = ChatConfig::default();
        let mut session = ChatSession::new(client, config);

        // Manually add a message for testing
        session.messages.push(MessageParam {
            role: MessageRole::User,
            content: MessageParamContent::String("test".to_string()),
        });
        assert_eq!(session.message_count(), 1);

        session.clear();
        assert_eq!(session.message_count(), 0);
    }

    #[test]
    fn set_model() {
        let client = Anthropic::new(None).unwrap();
        let config = ChatConfig::default();
        let mut session = ChatSession::new(client, config);

        assert_eq!(session.model(), &Model::Known(KnownModel::ClaudeHaiku45));

        session.set_model(Model::Known(KnownModel::ClaudeSonnet40));
        assert_eq!(session.model(), &Model::Known(KnownModel::ClaudeSonnet40));
    }

    #[test]
    fn set_system_prompt() {
        let client = Anthropic::new(None).unwrap();
        let config = ChatConfig::default();
        let mut session = ChatSession::new(client, config);

        assert!(session.system_prompt().is_none());

        session.set_system_prompt(Some("Be helpful".to_string()));
        assert_eq!(session.system_prompt(), Some("Be helpful"));

        session.set_system_prompt(None);
        assert!(session.system_prompt().is_none());
    }
}
