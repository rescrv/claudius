//! Configuration types for the chat application.
//!
//! This module provides CLI argument parsing via `arrrg` and configuration
//! structures for controlling chat behavior.

use std::path::PathBuf;

use arrrg_derive::CommandLine;

use crate::types::{KnownModel, Model};

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

/// Configuration for a chat session.
///
/// This struct holds the resolved configuration values after processing
/// command-line arguments with appropriate defaults.
#[derive(Debug, Clone)]
pub struct ChatConfig {
    /// The model to use for generating responses.
    pub model: Model,

    /// Optional system prompt to set conversation context.
    pub system_prompt: Option<String>,

    /// Maximum tokens per response.
    pub max_tokens: u32,

    /// Whether to use ANSI colors and styles in output.
    pub use_color: bool,

    /// Optional sampling temperature.
    pub temperature: Option<f32>,

    /// Optional top-p nucleus sampling value.
    pub top_p: Option<f32>,

    /// Optional top-k sampling limit.
    pub top_k: Option<u32>,

    /// Custom stop sequences supplied on every request.
    pub stop_sequences: Vec<String>,

    /// Extended thinking configuration.
    /// `None` means thinking is disabled, `Some(budget)` enables with the given token budget.
    pub thinking_budget: Option<u32>,

    /// Optional per-session token budget (input + output).
    pub session_budget_tokens: Option<u64>,

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
            model: Model::Known(KnownModel::ClaudeHaiku45),
            system_prompt: None,
            max_tokens: DEFAULT_MAX_TOKENS,
            use_color: true,
            temperature: None,
            top_p: None,
            top_k: None,
            stop_sequences: Vec::new(),
            thinking_budget: None,
            session_budget_tokens: None,
            transcript_path: None,
            caching_enabled: true,
        }
    }

    /// Sets the model to use.
    pub fn with_model(mut self, model: Model) -> Self {
        self.model = model;
        self
    }

    /// Sets the system prompt.
    pub fn with_system_prompt(mut self, prompt: String) -> Self {
        self.system_prompt = Some(prompt);
        self
    }

    /// Sets the maximum tokens per response.
    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = max_tokens;
        self
    }

    /// Disables ANSI color output.
    pub fn without_color(mut self) -> Self {
        self.use_color = false;
        self
    }

    /// Sets the sampling temperature.
    pub fn with_temperature(mut self, temperature: Option<f32>) -> Self {
        self.temperature = temperature;
        self
    }

    /// Sets the top-p value.
    pub fn with_top_p(mut self, top_p: Option<f32>) -> Self {
        self.top_p = top_p;
        self
    }

    /// Sets the top-k value.
    pub fn with_top_k(mut self, top_k: Option<u32>) -> Self {
        self.top_k = top_k;
        self
    }

    /// Sets the stop sequences.
    pub fn with_stop_sequences(mut self, stop_sequences: Vec<String>) -> Self {
        self.stop_sequences = stop_sequences;
        self
    }

    /// Sets the thinking budget.
    /// `None` disables thinking, `Some(budget)` enables with the given token budget.
    pub fn with_thinking_budget(mut self, budget: Option<u32>) -> Self {
        self.thinking_budget = budget;
        self
    }

    /// Sets the session token budget.
    pub fn with_session_budget(mut self, budget: Option<u64>) -> Self {
        self.session_budget_tokens = budget;
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
}

impl Default for ChatConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl From<ChatArgs> for ChatConfig {
    fn from(args: ChatArgs) -> Self {
        let model = args
            .model
            .map(|s| s.parse::<Model>().unwrap_or(Model::Custom(s)))
            .unwrap_or(Model::Known(KnownModel::ClaudeHaiku45));

        ChatConfig {
            model,
            system_prompt: args.system,
            max_tokens: args.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
            use_color: !args.no_color,
            ..ChatConfig::new()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config() {
        let config = ChatConfig::new();
        assert_eq!(config.model, Model::Known(KnownModel::ClaudeHaiku45));
        assert_eq!(config.max_tokens, 4096);
        assert!(config.use_color);
        assert!(config.system_prompt.is_none());
        assert!(config.temperature.is_none());
        assert!(config.top_p.is_none());
        assert!(config.top_k.is_none());
        assert!(config.stop_sequences.is_empty());
        assert!(config.thinking_budget.is_none());
        assert!(config.session_budget_tokens.is_none());
        assert!(config.transcript_path.is_none());
        assert!(config.caching_enabled);
    }

    #[test]
    fn config_from_args_defaults() {
        let args = ChatArgs::default();
        let config = ChatConfig::from(args);
        assert_eq!(config.model, Model::Known(KnownModel::ClaudeHaiku45));
        assert_eq!(config.max_tokens, 4096);
        assert!(config.use_color);
        assert!(config.thinking_budget.is_none());
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
        assert_eq!(config.model, Model::Known(KnownModel::ClaudeSonnet40));
        assert_eq!(config.system_prompt, Some("You are helpful.".to_string()));
        assert_eq!(config.max_tokens, 8192);
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

        assert_eq!(config.model, Model::Known(KnownModel::ClaudeSonnet40));
        assert_eq!(config.system_prompt, Some("Test prompt".to_string()));
        assert_eq!(config.max_tokens, 2048);
        assert!(!config.use_color);
        assert_eq!(config.temperature, Some(0.6));
        assert_eq!(config.top_p, Some(0.9));
        assert_eq!(config.top_k, Some(64));
        assert_eq!(config.stop_sequences, vec!["END".to_string()]);
        assert_eq!(config.thinking_budget, Some(2048));
        assert_eq!(config.session_budget_tokens, Some(10_000));
        assert_eq!(
            config.transcript_path,
            Some(PathBuf::from("transcript.json"))
        );
        assert!(!config.caching_enabled);
    }
}
