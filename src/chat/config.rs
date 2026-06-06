//! Configuration types for the chat application.
//!
//! This module provides CLI argument parsing via `arrrg` and configuration
//! structures for controlling chat behavior.

use std::path::PathBuf;

use arrrg_derive::CommandLine;

use crate::Budget;
use crate::types::{
    Effort, KnownModel, MessageCreateTemplate, Model, OutputConfig, SystemPrompt, ThinkingConfig,
};

/// The resolved thinking mode for a chat session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThinkingMode {
    /// Thinking is disabled.
    Disabled,
    /// Adaptive thinking with an effort level.
    Adaptive(Effort),
    /// Extended thinking with a fixed token budget.
    Budgeted(u32),
}

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

    /// Sampling temperature (0.0 to 1.0).
    #[arrrg(optional, "Sampling temperature (0.0 to 1.0)", "TEMP")]
    pub temperature: Option<String>,

    /// Top-p (nucleus) sampling (0.0 to 1.0).
    #[arrrg(optional, "Top-p (nucleus) sampling (0.0 to 1.0)", "TOP_P")]
    pub top_p: Option<String>,

    /// Top-k sampling.
    #[arrrg(optional, "Top-k sampling", "TOP_K")]
    pub top_k: Option<u32>,

    /// Thinking budget (enables extended thinking with given token budget).
    #[arrrg(
        optional,
        "Thinking budget in tokens (enables extended thinking)",
        "TOKENS"
    )]
    pub thinking: Option<u32>,

    /// Effort level for adaptive thinking (low, medium, high).
    #[arrrg(
        optional,
        "Effort level for adaptive thinking (low, medium, high)",
        "LEVEL"
    )]
    pub effort: Option<String>,

    /// Disable ANSI colors and styles.
    #[arrrg(flag, "Disable ANSI colors/styles")]
    pub no_color: bool,
}

/// Error type for parsing ChatArgs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChatArgsError {
    message: String,
}

impl std::fmt::Display for ChatArgsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for ChatArgsError {}

fn parse_f32_arg(value: &str, name: &str) -> Result<f32, ChatArgsError> {
    value.parse::<f32>().map_err(|_| ChatArgsError {
        message: format!(
            "invalid value for --{}: '{}' is not a valid number",
            name, value
        ),
    })
}

fn parse_effort(s: &str) -> Result<Effort, ChatArgsError> {
    match s.to_lowercase().as_str() {
        "low" => Ok(Effort::Low),
        "medium" | "med" => Ok(Effort::Medium),
        "high" => Ok(Effort::High),
        _ => Err(ChatArgsError {
            message: format!(
                "invalid value for --effort: '{}' (expected low, medium, or high)",
                s
            ),
        }),
    }
}

impl TryFrom<ChatArgs> for MessageCreateTemplate {
    type Error = ChatArgsError;

    fn try_from(args: ChatArgs) -> Result<Self, Self::Error> {
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

        if let Some(ref temp) = args.temperature {
            template.temperature = Some(parse_f32_arg(temp, "temperature")?);
        }

        if let Some(ref top_p) = args.top_p {
            template.top_p = Some(parse_f32_arg(top_p, "top-p")?);
        }

        template.top_k = args.top_k;

        if let Some(ref effort_str) = args.effort {
            let effort = parse_effort(effort_str)?;
            template.thinking = Some(ThinkingConfig::adaptive_summarized());
            template.output_config = Some(OutputConfig::new().with_effort(effort));
        }

        if let Some(thinking) = args.thinking {
            template.thinking = Some(ThinkingConfig::enabled_summarized(thinking));
        }

        Ok(template)
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
    /// Optional per-session spend limit.
    pub session_spend: Option<Budget>,
    /// Path to persist transcripts automatically after each assistant turn.
    pub transcript_path: Option<PathBuf>,
    /// Whether prompt caching is enabled for this session.
    pub caching_enabled: bool,
    /// Optional effort level for adaptive thinking.
    pub effort: Option<Effort>,
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
            session_spend: None,
            transcript_path: None,
            caching_enabled: true,
            effort: None,
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
    /// Clears any adaptive effort setting to avoid conflicting modes.
    pub fn with_thinking_budget(mut self, budget: Option<u32>) -> Self {
        self.template.thinking = budget.map(ThinkingConfig::enabled_summarized);
        self.template.output_config = None;
        self.effort = None;
        self
    }

    /// Sets adaptive thinking with an optional effort level.
    pub fn with_thinking_adaptive(mut self, effort: Option<Effort>) -> Self {
        self.template.thinking = Some(ThinkingConfig::adaptive_summarized());
        self.template.output_config = effort.map(|e| OutputConfig::new().with_effort(e));
        self.effort = effort;
        self
    }

    /// Sets the effort level for adaptive thinking.
    pub fn with_effort(mut self, effort: Option<Effort>) -> Self {
        self.template.output_config = effort.map(|e| OutputConfig::new().with_effort(e));
        self.effort = effort;
        self.ensure_adaptive_thinking_has_display();
        self
    }

    /// Sets the session spend limit in dollars, using the configured model's token rates.
    pub fn with_session_spend(mut self, dollars: Option<f64>) -> Self {
        self.session_spend = dollars.map(|d| self.dollar_budget(d));
        self
    }

    /// Sets the session spend budget object directly.
    ///
    /// Cloning a [`Budget`] preserves its remaining-spend state, so this can be
    /// used to carry spend accounting across rebuilt chat sessions.
    pub fn with_session_spend_budget(mut self, budget: Option<Budget>) -> Self {
        self.session_spend = budget;
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
            Some(
                ThinkingConfig::Enabled { budget_tokens }
                | ThinkingConfig::EnabledWithDisplay { budget_tokens, .. },
            ) => Some(budget_tokens),
            _ => None,
        }
    }

    /// Returns the full thinking configuration, if any.
    ///
    /// Use this to distinguish `Adaptive` from `Enabled { budget_tokens }`
    /// from `None`/`Disabled`.
    pub fn thinking_config(&self) -> Option<ThinkingConfig> {
        self.template.thinking
    }

    /// Returns the resolved thinking mode.
    pub fn thinking_mode(&self) -> ThinkingMode {
        match self.template.thinking {
            Some(ThinkingConfig::Adaptive | ThinkingConfig::AdaptiveWithDisplay { .. }) => {
                ThinkingMode::Adaptive(self.effort.unwrap_or(Effort::Medium))
            }
            Some(
                ThinkingConfig::Enabled { budget_tokens }
                | ThinkingConfig::EnabledWithDisplay { budget_tokens, .. },
            ) => ThinkingMode::Budgeted(budget_tokens),
            Some(ThinkingConfig::Disabled) | None => ThinkingMode::Disabled,
        }
    }

    /// Returns the configured effort level, if any.
    pub fn effort(&self) -> Option<Effort> {
        self.effort
    }

    /// Builds the `OutputConfig` for this session, if effort is set.
    pub fn output_config(&self) -> Option<OutputConfig> {
        self.effort
            .map(|effort| OutputConfig::new().with_effort(effort))
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
    /// Clears any adaptive effort setting to avoid conflicting modes.
    pub fn set_thinking_budget(&mut self, budget: Option<u32>) {
        self.template.thinking = budget.map(ThinkingConfig::enabled_summarized);
        self.template.output_config = None;
        self.effort = None;
    }

    /// Sets adaptive thinking with an optional effort level.
    pub fn set_thinking_adaptive(&mut self, effort: Option<Effort>) {
        self.template.thinking = Some(ThinkingConfig::adaptive_summarized());
        self.template.output_config = effort.map(|e| OutputConfig::new().with_effort(e));
        self.effort = effort;
    }

    /// Sets the effort level for adaptive thinking.
    pub fn set_effort(&mut self, effort: Option<Effort>) {
        self.template.output_config = effort.map(|e| OutputConfig::new().with_effort(e));
        self.effort = effort;
        self.ensure_adaptive_thinking_has_display();
    }

    /// Sets the session spend limit in dollars, using the configured model's token rates.
    pub fn set_session_spend(&mut self, dollars: Option<f64>) {
        self.session_spend = dollars.map(|d| self.dollar_budget(d));
    }

    /// Sets the session spend budget object directly.
    ///
    /// Cloning a [`Budget`] preserves its remaining-spend state, so this can be
    /// used to carry spend accounting across rebuilt chat sessions.
    pub fn set_session_spend_budget(&mut self, budget: Option<Budget>) {
        self.session_spend = budget;
    }

    fn dollar_budget(&self, dollars: f64) -> Budget {
        match self.model() {
            Model::Known(km) => Budget::from_dollars_with_model(dollars, km),
            Model::Custom(_) => {
                // Fall back to Sonnet 4.5 rates for custom/unknown models.
                Budget::from_dollars_with_model(dollars, KnownModel::ClaudeSonnet45)
            }
        }
    }

    fn ensure_adaptive_thinking_has_display(&mut self) {
        if matches!(
            self.template.thinking,
            Some(ThinkingConfig::Adaptive | ThinkingConfig::AdaptiveWithDisplay { .. })
        ) {
            self.template.thinking = Some(ThinkingConfig::adaptive_summarized());
        }
    }
}

impl Default for ChatConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl TryFrom<ChatArgs> for ChatConfig {
    type Error = ChatArgsError;

    fn try_from(args: ChatArgs) -> Result<Self, Self::Error> {
        let use_color = !args.no_color;
        let effort = match args.effort.as_deref() {
            Some(s) => Some(parse_effort(s)?),
            None => None,
        };
        let template = default_template().merge(MessageCreateTemplate::try_from(args)?);

        Ok(ChatConfig {
            template,
            use_color,
            session_spend: None,
            transcript_path: None,
            caching_enabled: true,
            effort,
        })
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
        assert!(config.thinking_config().is_none());
        assert!(config.effort().is_none());
        assert!(config.output_config().is_none());
        assert!(config.session_spend.is_none());
        assert!(config.transcript_path.is_none());
        assert!(config.caching_enabled);
    }

    #[test]
    fn config_from_args_defaults() {
        let args = ChatArgs::default();
        let config = ChatConfig::try_from(args).unwrap();
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
            temperature: Some("0.7".to_string()),
            top_p: Some("0.9".to_string()),
            top_k: Some(40),
            thinking: Some(2048),
            effort: None,
            no_color: true,
        };
        let config = ChatConfig::try_from(args).unwrap();
        assert_eq!(config.model(), Model::Known(KnownModel::ClaudeSonnet40));
        assert_eq!(config.system_prompt_text(), Some("You are helpful."));
        assert_eq!(config.max_tokens(), 8192);
        assert_eq!(config.template.temperature, Some(0.7));
        assert_eq!(config.template.top_p, Some(0.9));
        assert_eq!(config.template.top_k, Some(40));
        assert_eq!(config.thinking_budget(), Some(2048));
        assert!(!config.use_color);
    }

    #[test]
    fn config_from_args_effort_uses_adaptive_display() {
        let args = ChatArgs {
            effort: Some("high".to_string()),
            ..Default::default()
        };

        let config = ChatConfig::try_from(args).unwrap();

        assert_eq!(
            config.thinking_config(),
            Some(ThinkingConfig::adaptive_summarized())
        );
        assert_eq!(config.effort(), Some(Effort::High));
    }

    #[test]
    fn config_from_args_invalid_temperature() {
        let args = ChatArgs {
            temperature: Some("not-a-number".to_string()),
            ..Default::default()
        };
        let result = ChatConfig::try_from(args);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.message.contains("--temperature"));
        assert!(err.message.contains("not-a-number"));
    }

    #[test]
    fn config_from_args_invalid_top_p() {
        let args = ChatArgs {
            top_p: Some("invalid".to_string()),
            ..Default::default()
        };
        let result = ChatConfig::try_from(args);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.message.contains("--top-p"));
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
            .with_session_spend(Some(1.25))
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
            config.thinking_config(),
            Some(ThinkingConfig::EnabledWithDisplay {
                budget_tokens: 2048,
                display: crate::types::ThinkingDisplay::Summarized
            })
        );
        assert_eq!(config.effort(), None);
        assert_eq!(
            config.session_spend.as_ref().map(Budget::total_micro_cents),
            Some(125_000_000)
        );
        assert_eq!(
            config.transcript_path,
            Some(PathBuf::from("transcript.json"))
        );
        assert!(!config.caching_enabled);
    }

    #[test]
    fn config_adaptive_thinking() {
        let config = ChatConfig::new().with_thinking_adaptive(Some(Effort::High));

        assert_eq!(
            config.thinking_config(),
            Some(ThinkingConfig::adaptive_summarized())
        );
        assert_eq!(config.effort(), Some(Effort::High));
        assert_eq!(config.thinking_budget(), None);
        assert!(config.output_config().is_some());
        assert_eq!(config.output_config().unwrap().effort, Some(Effort::High));
    }

    #[test]
    fn set_thinking_budget_clears_effort() {
        let mut config = ChatConfig::new().with_thinking_adaptive(Some(Effort::Medium));
        assert_eq!(config.effort(), Some(Effort::Medium));

        config.set_thinking_budget(Some(4096));
        assert_eq!(config.thinking_budget(), Some(4096));
        assert_eq!(config.effort(), None);
    }

    #[test]
    fn set_thinking_adaptive_mutator() {
        let mut config = ChatConfig::new().with_thinking_budget(Some(4096));
        assert_eq!(config.thinking_budget(), Some(4096));

        config.set_thinking_adaptive(Some(Effort::Low));
        assert_eq!(
            config.thinking_config(),
            Some(ThinkingConfig::adaptive_summarized())
        );
        assert_eq!(config.effort(), Some(Effort::Low));
        assert_eq!(config.thinking_budget(), None);
    }

    #[test]
    fn effort_setters_normalize_adaptive_to_display_mode() {
        let config = ChatConfig {
            template: MessageCreateTemplate::new().with_thinking(ThinkingConfig::Adaptive),
            ..ChatConfig::new()
        }
        .with_effort(Some(Effort::Medium));

        assert_eq!(
            config.thinking_config(),
            Some(ThinkingConfig::adaptive_summarized())
        );

        let mut config = ChatConfig {
            template: MessageCreateTemplate::new().with_thinking(ThinkingConfig::Adaptive),
            ..ChatConfig::new()
        };
        config.set_effort(Some(Effort::High));

        assert_eq!(
            config.thinking_config(),
            Some(ThinkingConfig::adaptive_summarized())
        );
    }

    #[test]
    fn with_thinking_budget_clears_effort() {
        let config = ChatConfig::new()
            .with_thinking_adaptive(Some(Effort::High))
            .with_thinking_budget(Some(2048));

        assert_eq!(config.thinking_budget(), Some(2048));
        assert_eq!(config.effort(), None);
    }
}
