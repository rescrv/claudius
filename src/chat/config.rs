//! Configuration types for the chat application.
//!
//! This module provides CLI argument parsing via `arrrg` and configuration
//! structures for controlling chat behavior.

use std::path::PathBuf;

use arrrg_derive::CommandLine;

use crate::Budget;
use crate::types::{KnownModel, MessageCreateTemplate, Model, SystemPrompt, ThinkingConfig};

/// Default maximum tokens per response.
const DEFAULT_MAX_TOKENS: u32 = 4096;

/// Command-line arguments for the claudius-chat tool.
#[derive(CommandLine, Debug, Default, PartialEq, Eq)]
pub struct ChatArgs {
    /// Model to use for chat.
    #[arrrg(optional, "Model to use (default: claude-haiku-4-5)", "MODEL")]
    pub model: Option<String>,

    /// System prompt to set context for the conversation.
    #[arrrg(optional, "System prompt for the conversation", "PROMPT")]
    pub system: Option<String>,

    /// Maximum tokens per response.
    #[arrrg(optional, "Max tokens per response (default: 4096)", "TOKENS")]
    pub max_tokens: Option<u32>,

    /// Disable ANSI colors and styles.
    #[arrrg(flag, "Disable ANSI colors/styles")]
    pub no_color: bool,
}

impl From<ChatArgs> for MessageCreateTemplate {
    fn from(args: ChatArgs) -> Self {
        let mut template = MessageCreateTemplate::new();

        if let Some(model) = args.model {
            let parsed = model.parse::<Model>().unwrap_or(Model::Custom(model));
            template = template.with_model(parsed);
        }

        if let Some(system) = args.system {
            template = template.with_system(system);
        }

        if let Some(max_tokens) = args.max_tokens {
            template = template.with_max_tokens(max_tokens);
        }

        template
    }
}

/// Configuration for a chat session.
///
/// This struct holds the resolved configuration values after processing
/// command-line arguments with appropriate defaults.
#[derive(Debug, Clone)]
pub struct ChatConfig {
    /// Template applied to message creation parameters.
    pub template: MessageCreateTemplate,
    /// Whether to use ANSI colors and styles in output.
    pub use_color: bool,
    /// Optional per-session token budget (input + output).
    pub session_budget: Option<Budget>,
    /// Path to persist transcripts automatically after each assistant turn.
    pub transcript_path: Option<PathBuf>,
    /// Whether to enable prompt caching for the system prompt.
    /// When enabled, the system prompt will include cache_control markers.
    pub caching_enabled: bool,
}

impl ChatConfig {
    /// Creates a new ChatConfig with default values.
    ///
    /// Defaults:
    /// - Model: claude-haiku-4-5
    /// - Max tokens: 4096
    /// - Color: enabled
    /// - Thinking: disabled
    /// - Caching: enabled
    pub fn new() -> Self {
        Self {
            template: default_template(),
            use_color: true,
            session_budget: None,
            transcript_path: None,
            caching_enabled: true,
        }
    }

    /// Sets the model to use.
    pub fn with_model(mut self, model: Model) -> Self {
        self.template.model = Some(model);
        self
    }

    /// Sets the system prompt.
    pub fn with_system_prompt(mut self, prompt: String) -> Self {
        self.template.system = Some(SystemPrompt::from(prompt));
        self
    }

    /// Sets the maximum tokens per response.
    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.template.max_tokens = Some(max_tokens);
        self
    }

    /// Disables ANSI color output.
    pub fn without_color(mut self) -> Self {
        self.use_color = false;
        self
    }

    /// Sets the sampling temperature.
    pub fn with_temperature(mut self, temperature: Option<f32>) -> Self {
        self.template.temperature = temperature;
        self
    }

    /// Sets the top-p value.
    pub fn with_top_p(mut self, top_p: Option<f32>) -> Self {
        self.template.top_p = top_p;
        self
    }

    /// Sets the top-k value.
    pub fn with_top_k(mut self, top_k: Option<u32>) -> Self {
        self.template.top_k = top_k;
        self
    }

    /// Sets the stop sequences.
    pub fn with_stop_sequences(mut self, stop_sequences: Vec<String>) -> Self {
        self.template.stop_sequences = Some(stop_sequences);
        self
    }

    /// Sets the thinking budget.
    /// `None` disables thinking, `Some(budget)` enables with the given token budget.
    pub fn with_thinking_budget(mut self, budget: Option<u32>) -> Self {
        self.template.thinking = budget.map(ThinkingConfig::enabled);
        self
    }

    /// Sets the session token budget.
    pub fn with_session_budget(mut self, budget: Option<u64>) -> Self {
        self.session_budget = budget.map(Self::token_budget);
        self
    }

    /// Sets the transcript auto-save path.
    pub fn with_transcript_path(mut self, path: Option<PathBuf>) -> Self {
        self.transcript_path = path;
        self
    }

    /// Sets whether prompt caching is enabled.
    pub fn with_caching(mut self, enabled: bool) -> Self {
        self.caching_enabled = enabled;
        self
    }

    /// Returns the configured model.
    pub fn model(&self) -> Model {
        self.template
            .model
            .clone()
            .unwrap_or(Model::Known(KnownModel::ClaudeHaiku45))
    }

    /// Returns the configured max tokens value.
    pub fn max_tokens(&self) -> u32 {
        self.template.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS)
    }

    /// Returns the system prompt as a string, if configured.
    pub fn system_prompt_text(&self) -> Option<&str> {
        match self.template.system.as_ref()? {
            SystemPrompt::String(text) => Some(text.as_str()),
            SystemPrompt::Blocks(_) => None,
        }
    }

    /// Returns the configured stop sequences, if any.
    pub fn stop_sequences(&self) -> &[String] {
        self.template.stop_sequences.as_deref().unwrap_or(&[])
    }

    /// Returns the configured thinking budget, if enabled.
    pub fn thinking_budget(&self) -> Option<u32> {
        match self.template.thinking {
            Some(ThinkingConfig::Enabled { budget_tokens }) => Some(budget_tokens),
            _ => None,
        }
    }

    /// Sets the model.
    pub fn set_model(&mut self, model: Model) {
        self.template.model = Some(model);
    }

    /// Sets or clears the system prompt.
    pub fn set_system_prompt(&mut self, prompt: Option<String>) {
        self.template.system = prompt.map(SystemPrompt::from);
    }

    /// Sets the maximum tokens per response.
    pub fn set_max_tokens(&mut self, max_tokens: u32) {
        self.template.max_tokens = Some(max_tokens);
    }

    /// Sets the sampling temperature.
    pub fn set_temperature(&mut self, temperature: Option<f32>) {
        self.template.temperature = temperature;
    }

    /// Sets the top-p value.
    pub fn set_top_p(&mut self, top_p: Option<f32>) {
        self.template.top_p = top_p;
    }

    /// Sets the top-k value.
    pub fn set_top_k(&mut self, top_k: Option<u32>) {
        self.template.top_k = top_k;
    }

    /// Sets the thinking budget.
    pub fn set_thinking_budget(&mut self, budget: Option<u32>) {
        self.template.thinking = budget.map(ThinkingConfig::enabled);
    }

    /// Sets the session token budget.
    pub fn set_session_budget(&mut self, budget: Option<u64>) {
        self.session_budget = budget.map(Self::token_budget);
    }

    fn token_budget(limit_tokens: u64) -> Budget {
        Budget::new_with_rates(limit_tokens, 1, 1, 1, 1)
    }
}

impl Default for ChatConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl From<ChatArgs> for ChatConfig {
    fn from(args: ChatArgs) -> Self {
        let use_color = !args.no_color;
        let template = default_template().merge(MessageCreateTemplate::from(args));

        ChatConfig {
            template,
            use_color,
            session_budget: None,
            transcript_path: None,
            caching_enabled: true,
        }
    }
}

fn default_template() -> MessageCreateTemplate {
    let mut template = MessageCreateTemplate::new();
    template.model = Some(Model::Known(KnownModel::ClaudeHaiku45));
    template.max_tokens = Some(DEFAULT_MAX_TOKENS);
    template
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config() {
        let config = ChatConfig::new();
        assert_eq!(config.model(), Model::Known(KnownModel::ClaudeHaiku45));
        assert_eq!(config.max_tokens(), 4096);
        assert!(config.use_color);
        assert!(config.template.system.is_none());
        assert!(config.template.temperature.is_none());
        assert!(config.template.top_p.is_none());
        assert!(config.template.top_k.is_none());
        assert!(config.stop_sequences().is_empty());
        assert!(config.thinking_budget().is_none());
        assert!(config.session_budget.is_none());
        assert!(config.transcript_path.is_none());
        assert!(config.caching_enabled);
    }

    #[test]
    fn config_from_args_defaults() {
        let args = ChatArgs::default();
        let config = ChatConfig::from(args);
        assert_eq!(config.model(), Model::Known(KnownModel::ClaudeHaiku45));
        assert_eq!(config.max_tokens(), 4096);
        assert!(config.use_color);
        assert!(config.thinking_budget().is_none());
    }

    #[test]
    fn config_from_args_custom() {
        let args = ChatArgs {
            model: Some("claude-sonnet-4-0".to_string()),
            system: Some("You are helpful.".to_string()),
            max_tokens: Some(8192),
            no_color: true,
        };
        let config = ChatConfig::from(args);
        assert_eq!(config.model(), Model::Known(KnownModel::ClaudeSonnet40));
        assert_eq!(config.system_prompt_text(), Some("You are helpful."));
        assert_eq!(config.max_tokens(), 8192);
        assert!(!config.use_color);
    }

    #[test]
    fn config_builder_pattern() {
        let config = ChatConfig::new()
            .with_model(Model::Known(KnownModel::ClaudeSonnet40))
            .with_system_prompt("Test prompt".to_string())
            .with_max_tokens(2048)
            .without_color()
            .with_temperature(Some(0.6))
            .with_top_p(Some(0.9))
            .with_top_k(Some(64))
            .with_stop_sequences(vec!["END".to_string()])
            .with_thinking_budget(Some(2048))
            .with_session_budget(Some(10_000))
            .with_transcript_path(Some(PathBuf::from("transcript.json")))
            .with_caching(false);

        assert_eq!(config.model(), Model::Known(KnownModel::ClaudeSonnet40));
        assert_eq!(config.system_prompt_text(), Some("Test prompt"));
        assert_eq!(config.max_tokens(), 2048);
        assert!(!config.use_color);
        assert_eq!(config.template.temperature, Some(0.6));
        assert_eq!(config.template.top_p, Some(0.9));
        assert_eq!(config.template.top_k, Some(64));
        assert_eq!(config.stop_sequences(), vec!["END".to_string()]);
        assert_eq!(config.thinking_budget(), Some(2048));
        assert_eq!(
            config
                .session_budget
                .as_ref()
                .map(Budget::total_micro_cents),
            Some(10_000)
        );
        assert_eq!(
            config.transcript_path,
            Some(PathBuf::from("transcript.json"))
        );
        assert!(!config.caching_enabled);
    }
}
