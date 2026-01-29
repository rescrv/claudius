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
use crate::cache_control::apply_cache_control_to_messages;
use crate::chat::config::ChatConfig;
use crate::error::Result;
use crate::types::{
    CacheControlEphemeral, MessageCreateTemplate, MessageParam, Model, SystemPrompt, TextBlock,
    Usage,
};
use crate::{Agent, Anthropic, Budget, Renderer, ThinkingConfig, TurnOutcome};

const BUDGET_BUFFER_MICRO_CENTS: u64 = 1;

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
        self.config.max_tokens()
    }

    async fn model(&self) -> Model {
        self.config.model()
    }

    async fn stop_sequences(&self) -> Option<Vec<String>> {
        let sequences = self.config.stop_sequences();
        if sequences.is_empty() {
            None
        } else {
            Some(sequences.to_vec())
        }
    }

    async fn system(&self) -> Option<SystemPrompt> {
        let prompt = self.config.template.system.as_ref()?;

        if self.config.caching_enabled {
            let mut blocks = match prompt {
                SystemPrompt::String(text) => vec![TextBlock::new(text.clone())],
                SystemPrompt::Blocks(existing) => {
                    existing.iter().map(|b| b.block.clone()).collect()
                }
            };
            if let Some(last) = blocks.last_mut() {
                last.cache_control = Some(CacheControlEphemeral::new());
            }
            Some(SystemPrompt::from_blocks(blocks))
        } else {
            Some(prompt.clone())
        }
    }

    async fn temperature(&self) -> Option<f32> {
        self.config.template.temperature
    }

    async fn thinking(&self) -> Option<ThinkingConfig> {
        self.config.template.thinking
    }

    async fn top_k(&self) -> Option<u32> {
        self.config.template.top_k
    }

    async fn top_p(&self) -> Option<f32> {
        self.config.template.top_p
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
    /// Whether prompt caching is enabled.
    pub caching_enabled: bool,
    /// Total cache creation tokens across all requests.
    pub total_cache_creation_tokens: u64,
    /// Total cache read tokens across all requests.
    pub total_cache_read_tokens: u64,
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
            budget,
        }
    }

    /// Sends a user message with content blocks and streams the response.
    ///
    /// This method accepts a `MessageParam` directly, allowing content blocks
    /// such as documents, images, and text to be included.
    ///
    /// # Errors
    ///
    /// Returns an error if the API request fails.
    pub async fn send_message(
        &mut self,
        message: MessageParam,
        renderer: &mut dyn Renderer,
    ) -> Result<()> {
        let context = ();
        if let Some(budget) = self.agent.config().session_budget.as_ref()
            && !budget_allows_next_turn(budget, self.last_turn_usage.as_ref())
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
        self.messages.push(message);

        // Apply cache_control markers to recent user messages if caching is enabled
        if self.agent.config().caching_enabled {
            apply_cache_control_to_messages(&mut self.messages);
        }

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

    /// Returns the chat configuration.
    pub fn config(&self) -> &ChatConfig {
        self.agent.config()
    }

    /// Returns the chat configuration for mutation.
    pub fn config_mut(&mut self) -> &mut ChatConfig {
        self.agent.config_mut()
    }

    /// Returns the message template used for requests.
    pub fn template(&self) -> &MessageCreateTemplate {
        &self.agent.config().template
    }

    /// Returns the message template used for requests for mutation.
    pub fn template_mut(&mut self) -> &mut MessageCreateTemplate {
        &mut self.agent.config_mut().template
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
        let (session_budget_tokens, budget_spent_tokens) = match config.session_budget.as_ref() {
            Some(budget) => {
                let total = budget.total_micro_cents();
                let remaining = budget.remaining_micro_cents();
                (Some(total), total.saturating_sub(remaining))
            }
            None => (None, 0),
        };
        SessionStats {
            model: config.model(),
            message_count: self.message_count(),
            max_tokens: config.max_tokens(),
            system_prompt: config.system_prompt_text().map(str::to_string),
            temperature: config.template.temperature,
            top_p: config.template.top_p,
            top_k: config.template.top_k,
            stop_sequences: config.template.stop_sequences.clone().unwrap_or_default(),
            thinking_budget: config.thinking_budget(),
            session_budget_tokens,
            budget_spent_tokens,
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
            caching_enabled: config.caching_enabled,
            total_cache_creation_tokens: self
                .usage_totals
                .cache_creation_input_tokens
                .map(|t| t.max(0) as u64)
                .unwrap_or(0),
            total_cache_read_tokens: self
                .usage_totals
                .cache_read_input_tokens
                .map(|t| t.max(0) as u64)
                .unwrap_or(0),
        }
    }

    fn record_usage(&mut self, outcome: TurnOutcome) {
        self.last_turn_usage = Some(outcome.usage);
        self.usage_totals = self.usage_totals + outcome.usage;
        self.request_count = self.request_count.saturating_add(outcome.request_count);
        if let Some(budget) = self.agent.config().session_budget.as_ref() {
            budget.consume_usage_saturating(&outcome.usage);
        }
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

fn budget_allows_next_turn(budget: &Budget, last_turn_usage: Option<&Usage>) -> bool {
    let remaining = budget.remaining_micro_cents();
    if remaining == 0 {
        return false;
    }
    let Some(usage) = last_turn_usage else {
        return true;
    };
    let cost = budget.calculate_cost(usage);
    cost.saturating_add(BUDGET_BUFFER_MICRO_CENTS) < remaining
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache_control::apply_cache_control_to_message;
    use crate::types::{KnownModel, SystemPrompt};
    use crate::{ContentBlock, MessageParamContent, MessageRole};

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
    fn template_updates_model() {
        let client = Anthropic::new(None).unwrap();
        let config = ChatConfig::default();
        let mut session = ChatSession::new(client, config);

        assert_eq!(
            session.template().model,
            Some(Model::Known(KnownModel::ClaudeHaiku45))
        );

        session.template_mut().model = Some(Model::Known(KnownModel::ClaudeSonnet40));
        assert_eq!(
            session.template().model,
            Some(Model::Known(KnownModel::ClaudeSonnet40))
        );
    }

    #[test]
    fn template_updates_system_prompt() {
        let client = Anthropic::new(None).unwrap();
        let config = ChatConfig::default();
        let mut session = ChatSession::new(client, config);

        assert!(session.template().system.is_none());

        session.template_mut().system = Some(SystemPrompt::from("Be helpful"));
        assert!(matches!(
            session.template().system,
            Some(SystemPrompt::String(ref text)) if text == "Be helpful"
        ));

        session.template_mut().system = None;
        assert!(session.template().system.is_none());
    }

    #[test]
    fn budget_allows_next_turn_without_usage() {
        let budget = Budget::new_with_rates(1000, 1, 1, 0, 0);
        assert!(budget_allows_next_turn(&budget, None));
    }

    #[test]
    fn budget_allows_next_turn_with_usage() {
        let budget = Budget::new_with_rates(1000, 1, 1, 0, 0);
        let usage = Usage::new(400, 0);
        assert!(budget_allows_next_turn(&budget, Some(&usage)));
    }

    #[test]
    fn budget_blocks_next_turn_when_over_grace() {
        let budget = Budget::new_with_rates(100, 1, 1, 0, 0);
        let usage = Usage::new(100, 0);
        assert!(!budget_allows_next_turn(&budget, Some(&usage)));
    }

    #[test]
    fn apply_cache_control_to_string_content() {
        let mut message = MessageParam {
            role: MessageRole::User,
            content: MessageParamContent::String("hello".to_string()),
        };

        apply_cache_control_to_message(&mut message);

        // Should have converted to array with cache_control
        match &message.content {
            MessageParamContent::Array(blocks) => {
                assert_eq!(blocks.len(), 1);
                if let ContentBlock::Text(text_block) = &blocks[0] {
                    assert_eq!(text_block.text, "hello");
                    assert!(text_block.cache_control.is_some());
                } else {
                    panic!("Expected Text block");
                }
            }
            _ => panic!("Expected Array content"),
        }
    }

    #[test]
    fn apply_cache_control_to_array_content() {
        let mut message = MessageParam {
            role: MessageRole::User,
            content: MessageParamContent::Array(vec![
                ContentBlock::Text(TextBlock::new("first")),
                ContentBlock::Text(TextBlock::new("second")),
            ]),
        };

        apply_cache_control_to_message(&mut message);

        // Should have cache_control only on the last block
        match &message.content {
            MessageParamContent::Array(blocks) => {
                assert_eq!(blocks.len(), 2);
                if let ContentBlock::Text(first) = &blocks[0] {
                    assert!(first.cache_control.is_none());
                }
                if let ContentBlock::Text(second) = &blocks[1] {
                    assert!(second.cache_control.is_some());
                }
            }
            _ => panic!("Expected Array content"),
        }
    }

    #[test]
    fn apply_cache_control_to_messages_selects_user_messages() {
        let mut messages = vec![
            MessageParam {
                role: MessageRole::User,
                content: MessageParamContent::String("user1".to_string()),
            },
            MessageParam {
                role: MessageRole::Assistant,
                content: MessageParamContent::String("assistant1".to_string()),
            },
            MessageParam {
                role: MessageRole::User,
                content: MessageParamContent::String("user2".to_string()),
            },
            MessageParam {
                role: MessageRole::Assistant,
                content: MessageParamContent::String("assistant2".to_string()),
            },
            MessageParam {
                role: MessageRole::User,
                content: MessageParamContent::String("user3".to_string()),
            },
        ];

        apply_cache_control_to_messages(&mut messages);

        // Should apply cache_control to last 3 user messages (MAX_CACHE_BREAKPOINTS - 1)
        // User messages are at indices 0, 2, 4
        for (idx, msg) in messages.iter().enumerate() {
            let has_cache = match &msg.content {
                MessageParamContent::Array(blocks) => blocks.last().is_some_and(|b| {
                    if let ContentBlock::Text(t) = b {
                        t.cache_control.is_some()
                    } else {
                        false
                    }
                }),
                MessageParamContent::String(_) => false,
            };

            let is_user = msg.role == MessageRole::User;
            // All user messages should have cache_control (we have 3 users, limit is 3)
            if is_user {
                assert!(
                    has_cache,
                    "User message at index {idx} should have cache_control"
                );
            } else {
                // Assistant messages should not be modified
                assert!(
                    !has_cache,
                    "Assistant message at index {idx} should not have cache_control"
                );
            }
        }
    }

    #[test]
    fn apply_cache_control_respects_max_breakpoints() {
        // Create 5 user messages - only last 3 should get cache_control
        let mut messages: Vec<MessageParam> = (0..5)
            .map(|i| MessageParam {
                role: MessageRole::User,
                content: MessageParamContent::String(format!("user{i}")),
            })
            .collect();

        apply_cache_control_to_messages(&mut messages);

        let cached_count = messages
            .iter()
            .filter(|msg| {
                matches!(
                    &msg.content,
                    MessageParamContent::Array(blocks)
                    if blocks.last().is_some_and(|b| {
                        matches!(b, ContentBlock::Text(t) if t.cache_control.is_some())
                    })
                )
            })
            .count();

        // MAX_CACHE_BREAKPOINTS - 1 = 3
        assert_eq!(cached_count, 3);

        // Verify it's the LAST 3 messages that got cache_control
        for (idx, msg) in messages.iter().enumerate() {
            let has_cache = matches!(
                &msg.content,
                MessageParamContent::Array(blocks)
                if blocks.last().is_some_and(|b| {
                    matches!(b, ContentBlock::Text(t) if t.cache_control.is_some())
                })
            );

            if idx < 2 {
                assert!(!has_cache, "Message {idx} should NOT have cache_control");
            } else {
                assert!(has_cache, "Message {idx} should have cache_control");
            }
        }
    }

    #[test]
    fn apply_cache_control_clears_old_markers() {
        // Simulate a conversation that grows over multiple turns.
        // Initially we have 3 user messages with cache_control set on all of them.
        let mut messages: Vec<MessageParam> = (0..3)
            .map(|i| MessageParam {
                role: MessageRole::User,
                content: MessageParamContent::Array(vec![ContentBlock::Text(
                    TextBlock::new(format!("user{i}"))
                        .with_cache_control(CacheControlEphemeral::new()),
                )]),
            })
            .collect();

        // Add 2 more user messages (simulating additional turns)
        for i in 3..5 {
            messages.push(MessageParam {
                role: MessageRole::User,
                content: MessageParamContent::String(format!("user{i}")),
            });
        }

        // At this point, messages 0, 1, 2 have cache_control from before.
        // After apply_cache_control_to_messages, only the last 3 (2, 3, 4) should have it.
        apply_cache_control_to_messages(&mut messages);

        let cached_count = messages
            .iter()
            .filter(|msg| {
                matches!(
                    &msg.content,
                    MessageParamContent::Array(blocks)
                    if blocks.last().is_some_and(|b| {
                        matches!(b, ContentBlock::Text(t) if t.cache_control.is_some())
                    })
                )
            })
            .count();

        // Only 3 messages should have cache_control (MAX_CACHE_BREAKPOINTS - 1)
        // DEBUG: Print cached count
        println!("cached_count: {cached_count}");
        assert_eq!(cached_count, 3, "Only 3 messages should have cache_control");

        // Verify the FIRST 2 messages no longer have cache_control (they were cleared)
        for (idx, msg) in messages.iter().enumerate() {
            let has_cache = matches!(
                &msg.content,
                MessageParamContent::Array(blocks)
                if blocks.last().is_some_and(|b| {
                    matches!(b, ContentBlock::Text(t) if t.cache_control.is_some())
                })
            );

            if idx < 2 {
                assert!(
                    !has_cache,
                    "Message {idx} should have cache_control CLEARED"
                );
            } else {
                assert!(has_cache, "Message {idx} should have cache_control");
            }
        }
    }
}
