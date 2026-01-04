//! Core chat session management.
//!
//! This module provides the `ChatSession` struct which manages conversation
//! state and handles streaming API interactions.

use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::{from_reader, to_writer_pretty};

use crate::Error;
use crate::chat::config::ChatConfig;
use crate::error::Result;
use crate::types::{MessageParam, MessageParamContent, MessageRole, Model, Usage};
use crate::{Agent, Anthropic, Budget, Renderer, SystemPrompt, ThinkingConfig, TurnOutcome};

/// Agent behavior expected by the chat session.
pub trait ChatAgent: Agent {
    /// Returns the active chat configuration.
    fn config(&self) -> &ChatConfig;

    /// Returns the active chat configuration for mutation.
    fn config_mut(&mut self) -> &mut ChatConfig;
}

/// Default chat agent that sources behavior from `ChatConfig`.
pub struct ConfigAgent {
    config: ChatConfig,
}

impl ConfigAgent {
    /// Creates a new chat agent from a configuration.
    pub fn new(config: ChatConfig) -> Self {
        Self { config }
    }
}

#[async_trait::async_trait]
impl Agent for ConfigAgent {
    async fn max_tokens(&self) -> u32 {
        self.config.max_tokens
    }

    async fn model(&self) -> Model {
        self.config.model.clone()
    }

    async fn stop_sequences(&self) -> Option<Vec<String>> {
        if self.config.stop_sequences.is_empty() {
            None
        } else {
            Some(self.config.stop_sequences.clone())
        }
    }

    async fn system(&self) -> Option<SystemPrompt> {
        self.config.system_prompt.clone().map(SystemPrompt::from)
    }

    async fn temperature(&self) -> Option<f32> {
        self.config.temperature
    }

    async fn thinking(&self) -> Option<ThinkingConfig> {
        self.config.thinking_budget.map(ThinkingConfig::enabled)
    }

    async fn top_k(&self) -> Option<u32> {
        self.config.top_k
    }

    async fn top_p(&self) -> Option<f32> {
        self.config.top_p
    }
}

impl ChatAgent for ConfigAgent {
    fn config(&self) -> &ChatConfig {
        &self.config
    }

    fn config_mut(&mut self) -> &mut ChatConfig {
        &mut self.config
    }
}

/// A chat session that manages conversation state and API interactions.
///
/// The session maintains message history and handles streaming responses
/// from the Anthropic API.
pub struct ChatSession<A: ChatAgent> {
    client: Anthropic,
    agent: A,
    messages: Vec<MessageParam>,
    usage_totals: Usage,
    last_turn_usage: Option<Usage>,
    request_count: u64,
    budget_spent_tokens: u64,
    budget: Arc<Budget>,
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
    /// Extended thinking budget (None = disabled, Some(n) = enabled with n tokens).
    pub thinking_budget: Option<u32>,
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

impl ChatSession<ConfigAgent> {
    /// Creates a new chat session with the given client and configuration.
    pub fn new(client: Anthropic, config: ChatConfig) -> Self {
        Self::with_agent(client, ConfigAgent::new(config))
    }
}

impl<A: ChatAgent> ChatSession<A> {
    /// Creates a new chat session with a custom agent.
    pub fn with_agent(client: Anthropic, agent: A) -> Self {
        let budget = Arc::new(Budget::new_flat_rate(u64::MAX, 1));
        Self {
            client,
            agent,
            messages: Vec::new(),
            usage_totals: Usage::new(0, 0),
            last_turn_usage: None,
            request_count: 0,
            budget_spent_tokens: 0,
            budget,
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
    /// # Errors
    ///
    /// Returns an error if the API request fails.
    pub async fn send_streaming(
        &mut self,
        user_input: &str,
        renderer: &mut dyn Renderer,
    ) -> Result<()> {
        let context = ();
        let session_budget_tokens = self.agent.config().session_budget_tokens;
        if let Some(limit) = session_budget_tokens
            && self.budget_spent_tokens >= limit
        {
            renderer.print_error(
                &context,
                "Session budget exhausted. Use /budget to increase or clear the limit.",
            );
            return Err(Error::bad_request(
                "session budget exhausted",
                Some("budget".to_string()),
            ));
        }

        let previous_len = self.messages.len();

        // Add user message to history
        self.messages.push(MessageParam {
            role: MessageRole::User,
            content: MessageParamContent::String(user_input.to_string()),
        });

        let outcome = self
            .agent
            .take_turn_streaming_root(&self.client, &mut self.messages, &self.budget, renderer)
            .await;

        match outcome {
            Ok(outcome) => {
                self.record_usage(outcome);
                self.auto_save_transcript()?;
                Ok(())
            }
            Err(err) => {
                self.messages.truncate(previous_len);
                Err(err)
            }
        }
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
        self.agent.config_mut().model = model;
    }

    /// Returns the current model.
    pub fn model(&self) -> &Model {
        &self.agent.config().model
    }

    /// Sets or clears the system prompt.
    pub fn set_system_prompt(&mut self, prompt: Option<String>) {
        self.agent.config_mut().system_prompt = prompt;
    }

    /// Returns the current system prompt, if any.
    pub fn system_prompt(&self) -> Option<&str> {
        self.agent.config().system_prompt.as_deref()
    }

    /// Sets the maximum tokens per response.
    pub fn set_max_tokens(&mut self, max_tokens: u32) {
        self.agent.config_mut().max_tokens = max_tokens;
    }

    /// Sets the sampling temperature.
    pub fn set_temperature(&mut self, temperature: Option<f32>) {
        self.agent.config_mut().temperature = temperature;
    }

    /// Sets the top-p value.
    pub fn set_top_p(&mut self, top_p: Option<f32>) {
        self.agent.config_mut().top_p = top_p;
    }

    /// Sets the top-k value.
    pub fn set_top_k(&mut self, top_k: Option<u32>) {
        self.agent.config_mut().top_k = top_k;
    }

    /// Adds a stop sequence to the persistent list.
    pub fn add_stop_sequence(&mut self, sequence: String) {
        if !self
            .agent
            .config()
            .stop_sequences
            .iter()
            .any(|existing| existing == &sequence)
        {
            self.agent.config_mut().stop_sequences.push(sequence);
        }
    }

    /// Clears all stop sequences.
    pub fn clear_stop_sequences(&mut self) {
        self.agent.config_mut().stop_sequences.clear();
    }

    /// Returns the configured stop sequences.
    pub fn stop_sequences(&self) -> &[String] {
        &self.agent.config().stop_sequences
    }

    /// Sets the extended thinking budget.
    /// `None` disables thinking, `Some(budget)` enables with the given token budget.
    pub fn set_thinking_budget(&mut self, budget: Option<u32>) {
        self.agent.config_mut().thinking_budget = budget;
    }

    /// Returns the extended thinking budget, if enabled.
    pub fn thinking_budget(&self) -> Option<u32> {
        self.agent.config().thinking_budget
    }

    /// Sets the session token budget.
    pub fn set_session_budget(&mut self, budget: Option<u64>) {
        self.agent.config_mut().session_budget_tokens = budget;
    }

    /// Returns the remaining session budget, if any.
    pub fn session_budget_remaining(&self) -> Option<i64> {
        self.agent.config().session_budget_tokens.map(|limit| {
            let spent = self.budget_spent_tokens as i64;
            limit as i64 - spent
        })
    }

    /// Sets the auto-save transcript path.
    pub fn set_transcript_path(&mut self, path: Option<PathBuf>) {
        self.agent.config_mut().transcript_path = path;
    }

    /// Returns the configured transcript path, if any.
    pub fn transcript_path(&self) -> Option<&Path> {
        self.agent.config().transcript_path.as_deref()
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
        let config = self.agent.config();
        SessionStats {
            model: config.model.clone(),
            message_count: self.message_count(),
            max_tokens: config.max_tokens,
            system_prompt: config.system_prompt.clone(),
            temperature: config.temperature,
            top_p: config.top_p,
            top_k: config.top_k,
            stop_sequences: config.stop_sequences.clone(),
            thinking_budget: config.thinking_budget,
            session_budget_tokens: config.session_budget_tokens,
            budget_spent_tokens: self.budget_spent_tokens,
            transcript_path: config.transcript_path.clone(),
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

    fn record_usage(&mut self, outcome: TurnOutcome) {
        self.last_turn_usage = Some(outcome.usage);
        self.usage_totals = self.usage_totals + outcome.usage;
        self.request_count = self.request_count.saturating_add(outcome.request_count);
        let turn_total =
            tokens_to_u64(outcome.usage.input_tokens) + tokens_to_u64(outcome.usage.output_tokens);
        self.budget_spent_tokens = self.budget_spent_tokens.saturating_add(turn_total);
    }

    fn auto_save_transcript(&self) -> Result<()> {
        if let Some(path) = &self.agent.config().transcript_path {
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
