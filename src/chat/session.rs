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
use crate::types::{Effort, MessageCreateTemplate, MessageParam, Model, SystemPrompt, Usage};
use crate::{Agent, Anthropic, Budget, OutputConfig, Renderer, ThinkingConfig, TurnOutcome};

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
        self.config.template.system.clone()
    }

    fn caching_enabled(&self) -> bool {
        self.config.caching_enabled
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

    async fn output_config(&self) -> Option<OutputConfig> {
        self.config.output_config()
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
    session_spend: Option<Arc<Budget>>,
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
    /// The full thinking configuration (Adaptive, Enabled, Disabled, or None).
    pub thinking_config: Option<ThinkingConfig>,
    /// Whether adaptive thinking is enabled.
    pub thinking_adaptive: bool,
    /// The configured effort level for adaptive thinking, if any.
    pub effort: Option<Effort>,
    /// The session spend limit, in micro-cents, if set.
    pub session_spend_micro_cents: Option<u64>,
    /// Total spend used against the session limit, in micro-cents.
    pub spend_used_micro_cents: u64,
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
        let session_spend = agent.config().session_spend.clone().map(Arc::new);
        Self {
            client,
            agent,
            messages: Vec::new(),
            usage_totals: Usage::new(0, 0),
            last_turn_usage: None,
            request_count: 0,
            budget,
            session_spend,
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
        self.sync_session_spend_from_config();
        self.ensure_session_spend_for_next_turn(renderer)?;

        let previous_len = self.messages.len();

        // Add user message to history
        self.messages.push(message);

        let budget = self.turn_budget();
        let outcome = self
            .agent
            .take_turn_streaming_root(&self.client, &mut self.messages, &budget, renderer)
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

    /// Returns a snapshot of the current conversation history.
    pub fn clone_messages(&self) -> Vec<MessageParam> {
        self.messages.clone()
    }

    /// Replaces the current conversation history with `messages`.
    pub fn replace_messages(&mut self, messages: Vec<MessageParam>) {
        self.messages = messages;
    }

    /// Continues a turn against an arbitrary transcript without mutating the session transcript.
    ///
    /// The provided `messages` transcript is used for the request, while usage totals,
    /// last-turn usage, request counts, and session-spend accounting are recorded on
    /// the parent session on success. If the turn fails, `messages` is restored to its
    /// original state.
    pub async fn continue_turn_streaming_on(
        &mut self,
        messages: &mut Vec<MessageParam>,
        renderer: &mut dyn Renderer,
    ) -> Result<()> {
        self.sync_session_spend_from_config();
        self.ensure_session_spend_for_next_turn(renderer)?;

        let previous_messages = messages.clone();
        let budget = self.turn_budget();
        let outcome = self
            .agent
            .take_turn_streaming_root(&self.client, messages, &budget, renderer)
            .await;

        match outcome {
            Ok(outcome) => {
                self.record_usage(outcome);
                Ok(())
            }
            Err(err) => {
                *messages = previous_messages;
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

    /// Sets or clears the session spend limit in dollars.
    ///
    /// This updates both the visible configuration and the active session
    /// budget used to cap model requests.
    pub fn set_session_spend(&mut self, dollars: Option<f64>) {
        self.agent.config_mut().set_session_spend(dollars);
        self.session_spend = self.agent.config().session_spend.clone().map(Arc::new);
    }

    /// Sets or clears the active session spend budget.
    ///
    /// Passing a cloned [`Budget`] preserves its remaining-spend state, which is
    /// useful when rebuilding or switching [`ChatSession`] instances.
    pub fn set_session_spend_budget(&mut self, budget: Option<Budget>) {
        self.agent
            .config_mut()
            .set_session_spend_budget(budget.clone());
        self.session_spend = budget.map(Arc::new);
    }

    /// Returns a clone of the active session spend budget.
    ///
    /// The returned [`Budget`] shares the same remaining-spend state as this
    /// session. Pass it to [`ChatConfig::with_session_spend_budget`] or
    /// [`ChatSession::set_session_spend_budget`] to preserve spend accounting
    /// across rebuilt sessions.
    pub fn session_spend_budget(&self) -> Option<Budget> {
        self.session_spend.as_deref().cloned()
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
        let (session_spend_micro_cents, spend_used_micro_cents) = match self.session_spend.as_ref()
        {
            Some(spend) => {
                let total = spend.total_micro_cents();
                let remaining = spend.remaining_micro_cents();
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
            thinking_config: config.thinking_config(),
            thinking_adaptive: matches!(
                config.thinking_mode(),
                crate::chat::config::ThinkingMode::Adaptive(_)
            ),
            effort: config.effort(),
            session_spend_micro_cents,
            spend_used_micro_cents,
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
    }

    fn auto_save_transcript(&self) -> Result<()> {
        if let Some(path) = &self.agent.config().transcript_path {
            self.save_transcript_to(path)
        } else {
            Ok(())
        }
    }

    fn turn_budget(&self) -> Arc<Budget> {
        self.session_spend
            .as_ref()
            .map(Arc::clone)
            .unwrap_or_else(|| Arc::clone(&self.budget))
    }

    fn sync_session_spend_from_config(&mut self) {
        if !same_budget_state(
            self.session_spend.as_deref(),
            self.agent.config().session_spend.as_ref(),
        ) {
            self.session_spend = self.agent.config().session_spend.clone().map(Arc::new);
        }
    }

    fn ensure_session_spend_for_next_turn(&self, renderer: &mut dyn Renderer) -> Result<()> {
        let context = ();
        if let Some(spend) = self.session_spend.as_ref()
            && spend.remaining_output_tokens() == 0
        {
            renderer.print_error(
                &context,
                "Session spend limit exhausted. Use /spend to increase or /spend clear to remove the limit.",
            );
            return Err(Error::bad_request(
                "session spend exhausted",
                Some("spend".to_string()),
            ));
        }
        Ok(())
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

fn same_budget_state(left: Option<&Budget>, right: Option<&Budget>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => {
            left.total_micro_cents() == right.total_micro_cents()
                && left.remaining_micro_cents() == right.remaining_micro_cents()
                && left.calculate_cost(&Usage::new(1, 0)) == right.calculate_cost(&Usage::new(1, 0))
                && left.calculate_cost(&Usage::new(0, 1)) == right.calculate_cost(&Usage::new(0, 1))
                && left.calculate_cost(&Usage::new(0, 0).with_cache_creation_input_tokens(1))
                    == right.calculate_cost(&Usage::new(0, 0).with_cache_creation_input_tokens(1))
                && left.calculate_cost(&Usage::new(0, 0).with_cache_read_input_tokens(1))
                    == right.calculate_cost(&Usage::new(0, 0).with_cache_read_input_tokens(1))
        }
        (None, None) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MessageParamContent;
    use crate::types::{KnownModel, SystemPrompt, Usage};

    struct TestRenderer;

    impl Renderer for TestRenderer {
        fn print_text(&mut self, _context: &dyn crate::StreamContext, _text: &str) {}

        fn print_thinking(&mut self, _context: &dyn crate::StreamContext, _text: &str) {}

        fn print_error(&mut self, _context: &dyn crate::StreamContext, _error: &str) {}

        fn print_info(&mut self, _context: &dyn crate::StreamContext, _info: &str) {}

        fn start_tool_use(&mut self, _context: &dyn crate::StreamContext, _name: &str, _id: &str) {}

        fn print_tool_input(&mut self, _context: &dyn crate::StreamContext, _partial_json: &str) {}

        fn finish_tool_use(&mut self, _context: &dyn crate::StreamContext) {}

        fn start_tool_result(
            &mut self,
            _context: &dyn crate::StreamContext,
            _tool_use_id: &str,
            _is_error: bool,
        ) {
        }

        fn print_tool_result_text(&mut self, _context: &dyn crate::StreamContext, _text: &str) {}

        fn finish_tool_result(&mut self, _context: &dyn crate::StreamContext) {}

        fn finish_response(&mut self, _context: &dyn crate::StreamContext) {}
    }

    struct StubAgent {
        config: ChatConfig,
        append: Option<MessageParam>,
        outcome: Result<TurnOutcome>,
        budget_usage: Option<Usage>,
    }

    impl StubAgent {
        fn new(
            config: ChatConfig,
            append: Option<MessageParam>,
            outcome: Result<TurnOutcome>,
        ) -> Self {
            Self {
                config,
                append,
                outcome,
                budget_usage: None,
            }
        }

        fn with_budget_usage(mut self, usage: Usage) -> Self {
            self.budget_usage = Some(usage);
            self
        }
    }

    #[async_trait::async_trait]
    impl Agent for StubAgent {
        async fn take_turn_streaming_root(
            &mut self,
            _client: &Anthropic,
            messages: &mut Vec<MessageParam>,
            budget: &Arc<Budget>,
            _renderer: &mut dyn Renderer,
        ) -> Result<TurnOutcome> {
            if let Some(usage) = self.budget_usage {
                let mut allocation = budget.allocate_available(self.config.max_tokens()).unwrap();
                assert!(allocation.consume_response(&usage));
            }
            if let Some(message) = self.append.clone() {
                crate::push_or_merge_message(messages, message);
            }
            self.outcome.clone()
        }
    }

    impl ChatAgent for StubAgent {
        fn config(&self) -> &ChatConfig {
            &self.config
        }

        fn config_mut(&mut self) -> &mut ChatConfig {
            &mut self.config
        }
    }

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

        session.messages.push(MessageParam {
            role: crate::MessageRole::User,
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
    fn clone_and_replace_messages_round_trip() {
        let client = Anthropic::new(None).unwrap();
        let config = ChatConfig::default();
        let mut session = ChatSession::new(client, config);

        let original = vec![
            MessageParam::user("hello"),
            MessageParam::assistant("world"),
        ];
        session.replace_messages(original.clone());
        assert_eq!(session.clone_messages(), original);
        assert_eq!(session.message_count(), 2);

        let replacement = vec![MessageParam::user("replacement")];
        session.replace_messages(replacement.clone());
        assert_eq!(session.clone_messages(), replacement);
        assert_eq!(session.message_count(), 1);
    }

    #[tokio::test]
    async fn continue_turn_streaming_on_updates_stats_without_mutating_session_messages() {
        let client = Anthropic::new(None).unwrap();
        let agent = StubAgent::new(
            ChatConfig::default(),
            Some(MessageParam::assistant("branched response")),
            Ok(TurnOutcome {
                stop_reason: crate::StopReason::EndTurn,
                usage: Usage::new(12, 34),
                request_count: 2,
            }),
        );
        let mut session = ChatSession::with_agent(client, agent);
        session.replace_messages(vec![MessageParam::user("live transcript")]);

        let session_snapshot = session.clone_messages();
        let mut branch = vec![MessageParam::user("resume transcript")];
        let mut renderer = TestRenderer;

        session
            .continue_turn_streaming_on(&mut branch, &mut renderer)
            .await
            .unwrap();

        assert_eq!(session.clone_messages(), session_snapshot);
        assert_eq!(
            branch,
            vec![
                MessageParam::user("resume transcript"),
                MessageParam::assistant("branched response")
            ]
        );

        let stats = session.stats();
        assert_eq!(stats.message_count, 1);
        assert_eq!(stats.total_input_tokens, 12);
        assert_eq!(stats.total_output_tokens, 34);
        assert_eq!(stats.total_requests, 2);
        assert_eq!(stats.last_turn_input_tokens, Some(12));
        assert_eq!(stats.last_turn_output_tokens, Some(34));
    }

    #[tokio::test]
    async fn continue_turn_streaming_on_restores_branch_on_error() {
        let client = Anthropic::new(None).unwrap();
        let agent = StubAgent::new(
            ChatConfig::default(),
            Some(MessageParam::assistant(" merged")),
            Err(Error::bad_request(
                "synthetic failure",
                Some("messages".to_string()),
            )),
        );
        let mut session = ChatSession::with_agent(client, agent);
        session.replace_messages(vec![MessageParam::user("live transcript")]);

        let mut branch = vec![MessageParam::assistant("original")];
        let original_branch = branch.clone();
        let mut renderer = TestRenderer;

        let err = session
            .continue_turn_streaming_on(&mut branch, &mut renderer)
            .await
            .unwrap_err();

        assert!(matches!(err, Error::BadRequest { .. }));
        assert_eq!(branch, original_branch);
        assert_eq!(
            session.clone_messages(),
            vec![MessageParam::user("live transcript")]
        );

        let stats = session.stats();
        assert_eq!(stats.total_input_tokens, 0);
        assert_eq!(stats.total_output_tokens, 0);
        assert_eq!(stats.total_requests, 0);
        assert_eq!(stats.last_turn_input_tokens, None);
        assert_eq!(stats.last_turn_output_tokens, None);
    }

    #[test]
    fn same_budget_state_matches_shared_budget_clones() {
        let budget = Budget::new_with_rates(1000, 1, 2, 3, 4);
        let cloned = budget.clone();
        budget.consume_usage_saturating(&Usage::new(10, 20));

        assert!(same_budget_state(Some(&budget), Some(&cloned)));
    }

    #[test]
    fn same_budget_state_detects_different_rates() {
        let left = Budget::new_with_rates(1000, 1, 2, 3, 4);
        let right = Budget::new_with_rates(1000, 1, 5, 3, 4);

        assert!(!same_budget_state(Some(&left), Some(&right)));
    }

    #[tokio::test]
    async fn session_spend_budget_survives_rebuild() {
        let spend = Budget::new_flat_rate(1000, 10);
        let usage = Usage::new(0, 40);
        let client = Anthropic::new(None).unwrap();
        let agent = StubAgent::new(
            ChatConfig::default().with_session_spend_budget(Some(spend)),
            Some(MessageParam::assistant("charged response")),
            Ok(TurnOutcome {
                stop_reason: crate::StopReason::EndTurn,
                usage,
                request_count: 1,
            }),
        )
        .with_budget_usage(usage);
        let mut session = ChatSession::with_agent(client, agent);
        let mut renderer = TestRenderer;

        session
            .send_message(MessageParam::user("charge spend"), &mut renderer)
            .await
            .unwrap();

        let stats = session.stats();
        assert_eq!(stats.spend_used_micro_cents, 400);

        let preserved_spend = session.session_spend_budget().unwrap();
        let rebuilt_client = Anthropic::new(None).unwrap();
        let rebuilt_agent = StubAgent::new(
            ChatConfig::default().with_session_spend_budget(Some(preserved_spend)),
            None,
            Ok(TurnOutcome {
                stop_reason: crate::StopReason::EndTurn,
                usage: Usage::new(0, 0),
                request_count: 0,
            }),
        );
        let rebuilt = ChatSession::with_agent(rebuilt_client, rebuilt_agent);

        assert_eq!(rebuilt.stats().spend_used_micro_cents, 400);
    }
}
