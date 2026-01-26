//! Prompt testing utilities for the Claudius library.
//!
//! This module provides structures and functions for testing prompts against
//! the Anthropic API, with support for file-based configurations, inheritance,
//! file reference resolution, and comprehensive unit testing capabilities.
//!
//! ## File Reference Resolution
//!
//! The configuration system automatically loads content from external files when:
//! - `prompt` field contains a relative path ending with "prompt.yaml"
//! - `system` field contains a relative path ending with "system.md"
//!
//! This enables clean separation of configuration from content while maintaining
//! security through filename restrictions.
//!
//! ## Security Model
//!
//! File operations are restricted for security:
//! - Only specific filenames ("prompt.yaml", "system.md") are automatically resolved
//! - Absolute paths are treated as literal strings, not file references
//! - Configuration inheritance only allows parent directory traversal for "base.yaml" files

use crate::{
    Anthropic, ContentBlock, KnownModel, Message, MessageCreateParams, MessageParam, MessageRole,
    Model, OutputFormat, ToolChoice, ToolUnionParam,
};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::time::{Duration, Instant};

/// Configuration for a prompt test with support for inheritance and file references.
///
/// This structure represents a complete prompt test configuration that can be loaded
/// from YAML files, inherit from base configurations, and automatically resolve
/// file references for content.
///
/// # File Reference Resolution
///
/// When loading from files, the following fields support automatic file resolution:
/// - `prompt`: If the value is a relative path ending with "prompt.yaml", the file content is loaded
/// - `system`: If the value is a relative path ending with "system.md", the file content is loaded
///
/// # Security Considerations
///
/// File resolution is restricted for security:
/// - Only relative paths are resolved (absolute paths remain as literal strings)
/// - Only specific filenames ("prompt.yaml", "system.md") trigger file loading
/// - Files are resolved relative to the configuration file's directory
///
/// # Examples
///
/// ## Creating a basic configuration:
/// ```rust
/// # use claudius::PromptTestConfig;
/// let config = PromptTestConfig::new("What is the capital of France?")
///     .with_model("claude-3-5-haiku-latest")
///     .expect_contains("Paris");
/// ```
///
/// ## Loading from file with automatic file references:
/// If `test.yaml` contains:
/// ```yaml
/// name: "Geography Test"
/// prompt: "prompt.yaml"    # Content loaded from prompt.yaml
/// system: "system.md"      # Content loaded from system.md
/// model: "claude-3-5-haiku-latest"
/// expected_contains:
///   - "capital"
/// ```
///
/// Then:
/// ```rust,no_run
/// # use claudius::PromptTestConfig;
/// # use std::error::Error;
/// # fn main() -> Result<(), Box<dyn Error>> {
/// let config = PromptTestConfig::from_file("test.yaml")?;
/// // config.prompt now contains the contents of prompt.yaml
/// // config.system now contains the contents of system.md
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptTestConfig {
    /// Base configuration to inherit from (filename within prompts directory).
    pub inherits: Option<String>,

    /// Name of the test (optional).
    pub name: Option<String>,

    /// The prompt text to send (for single-turn conversations).
    ///
    /// When loading from files, if this field contains a relative path ending with
    /// "prompt.yaml", the content will be automatically loaded from that file.
    pub prompt: Option<String>,

    /// Multi-turn conversation messages (alternative to prompt).
    pub messages: Option<Vec<MessageParam>>,

    /// Optional system prompt.
    ///
    /// When loading from files, if this field contains a relative path ending with
    /// "system.md", the content will be automatically loaded from that file.
    pub system: Option<String>,

    /// Model to use for testing.
    pub model: Option<String>,

    /// Maximum tokens to generate.
    pub max_tokens: Option<u32>,

    /// Temperature setting (0.0 to 1.0).
    pub temperature: Option<f32>,

    /// Top-p setting (0.0 to 1.0).
    pub top_p: Option<f32>,

    /// Top-k setting.
    pub top_k: Option<u32>,

    /// Stop sequences.
    pub stop_sequences: Option<Vec<String>>,

    /// Tools available for the conversation.
    pub tools: Option<Vec<ToolUnionParam>>,

    /// How the model should use the provided tools.
    pub tool_choice: Option<ToolChoice>,

    /// Expected content that should appear in the response.
    pub expected_contains: Option<Vec<String>>,

    /// Expected content that should NOT appear in the response.
    pub expected_not_contains: Option<Vec<String>>,

    /// Minimum expected response length.
    pub min_response_length: Option<usize>,

    /// Maximum expected response length.
    pub max_response_length: Option<usize>,

    /// Expected tool calls (name of tools that should be called).
    pub expected_tool_calls: Option<Vec<String>>,

    /// Whether this test is expected to fail with an API error.
    pub expect_error: Option<bool>,

    /// Expected error message (substring match).
    pub expected_error_message: Option<String>,

    /// Output format for structured outputs.
    ///
    /// When set, constrains Claude's response to follow a specific JSON schema,
    /// ensuring valid, parseable output for downstream processing.
    pub output_format: Option<OutputFormat>,
}

/// Default model to use for prompt tests when none is specified.
const DEFAULT_MODEL: &str = "claude-3-5-haiku-latest";

/// Default maximum tokens for prompt tests when none is specified.
const DEFAULT_MAX_TOKENS: u32 = 1000;

/// Result of running a prompt test.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptTestResult {
    /// The test configuration that was run.
    pub config: PromptTestConfig,

    /// The response text from the API (empty if API call failed).
    pub response: String,

    /// Duration of the API call.
    pub duration: Duration,

    /// Input tokens used (0 if API call failed).
    pub input_tokens: u32,

    /// Output tokens used (0 if API call failed).
    pub output_tokens: u32,

    /// Whether the API call succeeded.
    pub api_success: bool,

    /// Error message if API call failed.
    pub error_message: Option<String>,

    /// Whether all assertions passed.
    pub assertions_passed: bool,

    /// List of assertion failures, if any.
    pub assertion_failures: Vec<String>,

    /// The full message object from the API (None if API call failed).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<Message>,
}

impl PromptTestConfig {
    /// Create a new prompt test configuration with just a prompt.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use claudius::PromptTestConfig;
    /// let config = PromptTestConfig::new("What is the capital of France?");
    /// assert_eq!(config.prompt, Some("What is the capital of France?".to_string()));
    /// ```
    pub fn new(prompt: impl Into<String>) -> Self {
        Self {
            inherits: None,
            name: None,
            prompt: Some(prompt.into()),
            messages: None,
            system: None,
            model: None,
            max_tokens: None,
            temperature: None,
            top_p: None,
            top_k: None,
            stop_sequences: None,
            tools: None,
            tool_choice: None,
            expected_contains: None,
            expected_not_contains: None,
            min_response_length: None,
            max_response_length: None,
            expected_tool_calls: None,
            expect_error: None,
            expected_error_message: None,
            output_format: None,
        }
    }

    /// Create a new multi-turn conversation test.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use claudius::{PromptTestConfig, MessageParam};
    /// let messages = vec![
    ///     MessageParam::user("Hello"),
    ///     MessageParam::assistant("Hi there! How can I help you?"),
    ///     MessageParam::user("What's the weather like?"),
    /// ];
    /// let config = PromptTestConfig::new_conversation(messages);
    /// assert!(config.messages.is_some());
    /// assert!(config.prompt.is_none());
    /// ```
    pub fn new_conversation(messages: Vec<MessageParam>) -> Self {
        Self {
            inherits: None,
            name: None,
            prompt: None,
            messages: Some(messages),
            system: None,
            model: None,
            max_tokens: None,
            temperature: None,
            top_p: None,
            top_k: None,
            stop_sequences: None,
            tools: None,
            tool_choice: None,
            expected_contains: None,
            expected_not_contains: None,
            min_response_length: None,
            max_response_length: None,
            expected_tool_calls: None,
            expect_error: None,
            expected_error_message: None,
            output_format: None,
        }
    }

    /// Set the test name.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    /// Set the system prompt.
    pub fn with_system(mut self, system: impl Into<String>) -> Self {
        self.system = Some(system.into());
        self
    }

    /// Set the model to use.
    pub fn with_model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Set the maximum tokens.
    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }

    /// Set the temperature.
    pub fn with_temperature(mut self, temperature: f32) -> Self {
        self.temperature = Some(temperature);
        self
    }

    /// Add expected content that should appear in the response.
    pub fn expect_contains(mut self, content: impl Into<String>) -> Self {
        self.expected_contains
            .get_or_insert_with(Vec::new)
            .push(content.into());
        self
    }

    /// Add content that should NOT appear in the response.
    pub fn expect_not_contains(mut self, content: impl Into<String>) -> Self {
        self.expected_not_contains
            .get_or_insert_with(Vec::new)
            .push(content.into());
        self
    }

    /// Set minimum expected response length.
    pub fn with_min_length(mut self, min_length: usize) -> Self {
        self.min_response_length = Some(min_length);
        self
    }

    /// Set maximum expected response length.
    pub fn with_max_length(mut self, max_length: usize) -> Self {
        self.max_response_length = Some(max_length);
        self
    }

    /// Add a tool to the configuration.
    pub fn with_tool(mut self, tool: ToolUnionParam) -> Self {
        self.tools.get_or_insert_with(Vec::new).push(tool);
        self
    }

    /// Set the tool choice configuration.
    pub fn with_tool_choice(mut self, tool_choice: ToolChoice) -> Self {
        self.tool_choice = Some(tool_choice);
        self
    }

    /// Expect a specific tool to be called.
    pub fn expect_tool_call(mut self, tool_name: impl Into<String>) -> Self {
        self.expected_tool_calls
            .get_or_insert_with(Vec::new)
            .push(tool_name.into());
        self
    }

    /// Expect the API call to fail with an error.
    pub fn expect_error(mut self) -> Self {
        self.expect_error = Some(true);
        self
    }

    /// Expect a specific error message (substring match).
    pub fn expect_error_message(mut self, message: impl Into<String>) -> Self {
        self.expected_error_message = Some(message.into());
        self.expect_error = Some(true);
        self
    }

    /// Set the output format for structured outputs.
    ///
    /// When set, constrains Claude's response to follow a specific JSON schema,
    /// ensuring valid, parseable output for downstream processing.
    ///
    /// # Examples
    ///
    /// ```rust
    /// # use claudius::{PromptTestConfig, OutputFormat};
    /// # use serde_json::json;
    /// let config = PromptTestConfig::new("Extract the person's name and age")
    ///     .with_output_format(OutputFormat::json_schema(json!({
    ///         "type": "object",
    ///         "properties": {
    ///             "name": { "type": "string" },
    ///             "age": { "type": "integer" }
    ///         },
    ///         "required": ["name", "age"],
    ///         "additionalProperties": false
    ///     })));
    /// ```
    pub fn with_output_format(mut self, output_format: OutputFormat) -> Self {
        self.output_format = Some(output_format);
        self
    }

    /// Load a prompt test configuration from a YAML file with inheritance and file reference support.
    ///
    /// This method provides several key features:
    ///
    /// ## Configuration Inheritance
    /// Supports configuration inheritance via the `inherits` field, allowing you to build
    /// configuration hierarchies. Security restrictions apply: only relative paths are allowed,
    /// and parent directory traversal is only permitted for `base.yaml` files.
    ///
    /// ## File Reference Resolution
    /// Automatically loads content from external files when:
    /// - `prompt` field contains a relative path ending with "prompt.yaml"
    /// - `system` field contains a relative path ending with "system.md"
    ///
    /// Files are resolved relative to the configuration file's directory. Absolute paths
    /// are treated as literal strings for security reasons.
    ///
    /// # Examples
    ///
    /// ## Basic usage:
    /// ```rust,no_run
    /// # use claudius::PromptTestConfig;
    /// # use std::error::Error;
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// let config = PromptTestConfig::from_file("test_config.yaml")?;
    /// println!("Loaded test: {:?}", config.name);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// ## File references:
    /// If your config file contains:
    /// ```yaml
    /// name: "My Test"
    /// prompt: "prompt.yaml"     # This file will be loaded
    /// system: "system.md"       # This file will be loaded
    /// model: "claude-3-5-haiku-latest"
    /// ```
    ///
    /// The content of `prompt.yaml` and `system.md` (relative to the config file)
    /// will be automatically loaded into the `prompt` and `system` fields.
    ///
    /// ## Inheritance with file references:
    /// ```yaml
    /// inherits: "../base.yaml"   # Inheritance (base.yaml only for parent dirs)
    /// name: "Specialized Test"
    /// prompt: "custom_prompt.yaml"  # File reference
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The file cannot be read
    /// - The YAML is invalid
    /// - Referenced `prompt.yaml` or `system.md` files cannot be read
    /// - Inheritance files use absolute paths or unsafe traversal
    /// - Inherited files cannot be found or loaded
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self, Box<dyn std::error::Error>> {
        Self::from_file_with_base_dir(path, None)
    }

    /// Load a prompt test configuration from a YAML file with a specific base directory.
    ///
    /// This is the core method that handles both configuration inheritance and file
    /// reference resolution. The `base_dir` parameter allows you to override the
    /// directory used for resolving relative file paths.
    ///
    /// ## File Reference Resolution
    /// Files are automatically loaded when:
    /// - The `prompt` field contains a relative path ending with "prompt.yaml"
    /// - The `system` field contains a relative path ending with "system.md"
    ///
    /// Only these specific filenames are resolved for security reasons. Other filenames
    /// or absolute paths are treated as literal strings.
    ///
    /// ## Base Directory Resolution
    /// - If `base_dir` is provided, all relative paths are resolved relative to it
    /// - If `base_dir` is None, paths are resolved relative to the config file's directory
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use claudius::PromptTestConfig;
    /// # use std::path::Path;
    /// # use std::error::Error;
    /// # fn main() -> Result<(), Box<dyn Error>> {
    /// // Load config with custom base directory
    /// let config = PromptTestConfig::from_file_with_base_dir(
    ///     "config.yaml",
    ///     Some(Path::new("/custom/base/dir"))
    /// )?;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Security
    ///
    /// File resolution is restricted to specific patterns for security:
    /// - Only relative paths ending with "prompt.yaml" or "system.md" are resolved
    /// - Absolute paths are treated as literal strings
    /// - Parent directory traversal in inheritance is only allowed for "base.yaml" files
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The configuration file cannot be read
    /// - Referenced prompt.yaml or system.md files cannot be read
    /// - The YAML syntax is invalid
    /// - Inheritance security restrictions are violated
    pub fn from_file_with_base_dir<P: AsRef<Path>>(
        path: P,
        base_dir: Option<&Path>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let path = path.as_ref();
        let content = std::fs::read_to_string(path)?;
        let mut config: Self = serde_yaml::from_str(&content)?;

        // Determine the directory containing the current file for relative path resolution
        let current_dir = if let Some(base) = base_dir {
            base.to_path_buf()
        } else {
            path.parent()
                .unwrap_or_else(|| Path::new("."))
                .to_path_buf()
        };

        // Handle prompt file reference (if prompt basename is "prompt.yaml", resolve relative to containing file)
        if let Some(ref prompt_value) = config.prompt {
            let prompt_path = Path::new(prompt_value);
            if prompt_path.file_name().and_then(|n| n.to_str()) == Some("prompt.yaml")
                && !prompt_path.is_absolute()
            {
                let prompt_file_path = current_dir.join(prompt_value);
                config.prompt = Some(std::fs::read_to_string(&prompt_file_path)?);
            }
        }

        // Handle system file reference (if system basename is "system.md", resolve relative to containing file)
        if let Some(ref system_value) = config.system {
            let system_path = Path::new(system_value);
            if system_path.file_name().and_then(|n| n.to_str()) == Some("system.md")
                && !system_path.is_absolute()
            {
                let system_file_path = current_dir.join(system_value);
                config.system = Some(std::fs::read_to_string(&system_file_path)?);
            }
        }

        // Handle inheritance
        if let Some(ref inherits_file) = config.inherits {
            // Security check: prevent absolute paths, and parent directory traversal except for base.yaml
            let path_obj = Path::new(inherits_file);
            let filename = path_obj.file_name().and_then(|n| n.to_str());

            if Path::new(inherits_file).is_absolute() {
                return Err(format!(
                    "Inheritance file '{}' cannot use absolute paths for security",
                    inherits_file
                )
                .into());
            }

            // Allow parent directory traversal only for base.yaml files
            if inherits_file.contains("..") && filename != Some("base.yaml") {
                return Err(format!(
                    "Inheritance file '{}' cannot use parent directory traversal for security (only base.yaml is allowed)",
                    inherits_file
                )
                .into());
            }

            let inherit_path = current_dir.join(inherits_file);
            let base_config = Self::from_file_with_base_dir(&inherit_path, Some(&current_dir))?;

            // Merge base config with current config (current takes precedence)
            config = base_config.merge_with(config);
        }

        Ok(config)
    }

    /// Merge this configuration with another, giving precedence to the other config's values.
    /// This is intended for use during inheritance - the other config is the child that inherits from self.
    fn merge_with(mut self, other: Self) -> Self {
        // The 'other' config takes precedence for all specified values
        if other.inherits.is_some() {
            self.inherits = other.inherits;
        }
        if other.name.is_some() {
            self.name = other.name;
        }
        if other.prompt.is_some() {
            self.prompt = other.prompt;
        }
        if other.messages.is_some() {
            self.messages = other.messages;
        }
        if other.system.is_some() {
            self.system = other.system;
        }
        if other.model.is_some() {
            self.model = other.model;
        }
        if other.max_tokens.is_some() {
            self.max_tokens = other.max_tokens;
        }
        if other.temperature.is_some() {
            self.temperature = other.temperature;
        }
        if other.top_p.is_some() {
            self.top_p = other.top_p;
        }
        if other.top_k.is_some() {
            self.top_k = other.top_k;
        }
        if other.stop_sequences.is_some() {
            self.stop_sequences = other.stop_sequences;
        }
        if other.tools.is_some() {
            self.tools = other.tools;
        }
        if other.tool_choice.is_some() {
            self.tool_choice = other.tool_choice;
        }
        if other.expected_contains.is_some() {
            self.expected_contains = other.expected_contains;
        }
        if other.expected_not_contains.is_some() {
            self.expected_not_contains = other.expected_not_contains;
        }
        if other.min_response_length.is_some() {
            self.min_response_length = other.min_response_length;
        }
        if other.max_response_length.is_some() {
            self.max_response_length = other.max_response_length;
        }
        if other.expected_tool_calls.is_some() {
            self.expected_tool_calls = other.expected_tool_calls;
        }
        if other.expect_error.is_some() {
            self.expect_error = other.expect_error;
        }
        if other.expected_error_message.is_some() {
            self.expected_error_message = other.expected_error_message;
        }
        if other.output_format.is_some() {
            self.output_format = other.output_format;
        }
        self
    }

    /// Save a prompt test configuration to a YAML file.
    pub fn to_file<P: AsRef<Path>>(&self, path: P) -> Result<(), Box<dyn std::error::Error>> {
        let content = serde_yaml::to_string(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }

    /// Run the prompt test using the provided Anthropic client.
    ///
    /// This method executes the prompt against the Anthropic API and validates
    /// all configured assertions. It handles both successful responses and API errors
    /// gracefully, allowing tests to verify error conditions.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use claudius::{Anthropic, PromptTestConfig};
    /// # #[tokio::main]
    /// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// let client = Anthropic::new(None)?;
    /// let config = PromptTestConfig::new("Hello, world!")
    ///     .expect_contains("hello")
    ///     .with_min_length(5);
    ///
    /// let result = config.run(&client).await?;
    /// assert!(result.api_success);
    /// println!("Response: {}", result.response);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns a [`crate::Error`] if:
    /// - Neither `prompt` nor `messages` is provided
    /// - Invalid parameter values (e.g., temperature out of range)
    /// - Other validation errors during request building
    pub async fn run(&self, client: &Anthropic) -> Result<PromptTestResult, crate::Error> {
        let start = Instant::now();

        // Parse the model
        let model_str = self.model.as_deref().unwrap_or(DEFAULT_MODEL);
        let model = if let Ok(known) = model_str.parse::<KnownModel>() {
            Model::Known(known)
        } else {
            Model::Custom(model_str.to_string())
        };

        // Build messages from either prompt or messages
        let messages = if let Some(ref prompt) = self.prompt {
            vec![MessageParam::new_with_string(
                prompt.clone(),
                MessageRole::User,
            )]
        } else if let Some(ref test_messages) = self.messages {
            test_messages.clone()
        } else {
            return Err(crate::Error::validation(
                "Must provide either 'prompt' or 'messages'",
                None,
            ));
        };

        // Build the request parameters
        let max_tokens = self.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS);
        let mut params = MessageCreateParams::new(max_tokens, messages, model);

        // Add system prompt if provided
        if let Some(ref system) = self.system {
            params = params.with_system_string(system.clone());
        }

        // Add optional parameters
        if let Some(temp) = self.temperature {
            params = params.with_temperature(temp)?;
        }

        if let Some(top_p) = self.top_p {
            params = params.with_top_p(top_p)?;
        }

        if let Some(top_k) = self.top_k {
            params = params.with_top_k(top_k);
        }

        if let Some(ref stop_seqs) = self.stop_sequences {
            params = params.with_stop_sequences(stop_seqs.clone());
        }

        // Add tools if provided
        if let Some(ref tools) = self.tools {
            params = params.with_tools(tools.clone());
        }

        // Add tool choice if provided
        if let Some(ref tool_choice) = self.tool_choice {
            params = params.with_tool_choice(tool_choice.clone());
        }

        // Add output format for structured outputs
        if let Some(ref output_format) = self.output_format {
            params = params.with_output_format(output_format.clone());
        }

        // Make the API call and handle errors gracefully
        let api_result = client.send(params).await;
        let duration = start.elapsed();

        let (
            response_text,
            tool_calls,
            api_success,
            error_message,
            input_tokens,
            output_tokens,
            message,
        ) = match api_result {
            Ok(response) => {
                // Extract response text and tool calls
                let mut response_text = String::new();
                let mut tool_calls = Vec::new();

                for block in &response.content {
                    match block {
                        ContentBlock::Text(text_block) => {
                            if !response_text.is_empty() {
                                response_text.push('\n');
                            }
                            response_text.push_str(&text_block.text);
                        }
                        ContentBlock::ToolUse(tool_use_block) => {
                            tool_calls.push(tool_use_block.name.clone());
                        }
                        _ => {}
                    }
                }

                (
                    response_text,
                    tool_calls,
                    true,
                    None,
                    response.usage.input_tokens as u32,
                    response.usage.output_tokens as u32,
                    Some(response),
                )
            }
            Err(error) => (
                String::new(),
                Vec::new(),
                false,
                Some(error.to_string()),
                0,
                0,
                None,
            ),
        };

        // Run assertions
        let mut assertion_failures = Vec::new();

        // Check if we expected an error
        if let Some(true) = self.expect_error {
            if api_success {
                assertion_failures.push("Expected API call to fail, but it succeeded".to_string());
            }
        } else if !api_success {
            assertion_failures.push(format!(
                "API call failed unexpectedly: {}",
                error_message
                    .as_ref()
                    .unwrap_or(&"Unknown error".to_string())
            ));
        }

        // Check expected error message
        if let Some(ref expected_msg) = self.expected_error_message {
            if let Some(ref actual_error) = error_message {
                if !actual_error
                    .to_lowercase()
                    .contains(&expected_msg.to_lowercase())
                {
                    assertion_failures.push(format!(
                        "Expected error message to contain '{}', but got: '{}'",
                        expected_msg, actual_error
                    ));
                }
            } else {
                assertion_failures.push(format!(
                    "Expected error message containing '{}', but API call succeeded",
                    expected_msg
                ));
            }
        }

        // Only run content-based assertions if API call succeeded
        if api_success {
            // Check expected_contains
            if let Some(ref expected) = self.expected_contains {
                for expected_content in expected {
                    if !response_text
                        .to_lowercase()
                        .contains(&expected_content.to_lowercase())
                    {
                        assertion_failures.push(format!(
                            "Expected response to contain '{}', but it didn't",
                            expected_content
                        ));
                    }
                }
            }

            // Check expected_not_contains
            if let Some(ref not_expected) = self.expected_not_contains {
                for not_expected_content in not_expected {
                    if response_text
                        .to_lowercase()
                        .contains(&not_expected_content.to_lowercase())
                    {
                        assertion_failures.push(format!(
                            "Expected response NOT to contain '{}', but it did",
                            not_expected_content
                        ));
                    }
                }
            }

            // Check minimum length
            if let Some(min_len) = self.min_response_length
                && response_text.len() < min_len
            {
                assertion_failures.push(format!(
                    "Expected response length >= {}, but got {}",
                    min_len,
                    response_text.len()
                ));
            }

            // Check maximum length
            if let Some(max_len) = self.max_response_length
                && response_text.len() > max_len
            {
                assertion_failures.push(format!(
                    "Expected response length <= {}, but got {}",
                    max_len,
                    response_text.len()
                ));
            }

            // Check expected tool calls
            if let Some(ref expected_tools) = self.expected_tool_calls {
                for expected_tool in expected_tools {
                    if !tool_calls.contains(expected_tool) {
                        assertion_failures.push(format!(
                            "Expected tool '{}' to be called, but it wasn't. Called tools: {:?}",
                            expected_tool, tool_calls
                        ));
                    }
                }
            }
        }

        Ok(PromptTestResult {
            config: self.clone(),
            response: response_text,
            duration,
            input_tokens,
            output_tokens,
            api_success,
            error_message,
            assertions_passed: assertion_failures.is_empty(),
            assertion_failures,
            message,
        })
    }
}

/// Helper function for unit tests - runs a prompt test and returns the result.
///
/// The input is treated as a literal prompt string. This is a convenience function
/// that creates a client, builds a basic test configuration, and runs it.
///
/// # Examples
///
/// ```rust,no_run
/// # use claudius::test_prompt;
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
/// let result = test_prompt("What is 2 + 2?").await?;
/// assert!(result.api_success);
/// assert!(result.response.len() > 0);
/// # Ok(())
/// # }
/// ```
///
/// # Errors
///
/// Returns an error if the client cannot be created or the API call fails.
pub async fn test_prompt(
    input: &str,
) -> Result<PromptTestResult, Box<dyn std::error::Error + Send + Sync>> {
    let client = Anthropic::new(None)
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;

    let config = PromptTestConfig::new(input);

    let result = config
        .run(&client)
        .await
        .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
    Ok(result)
}

/// Assert that a prompt test result contains specific text.
///
/// # Examples
///
/// ```rust,no_run
/// # use claudius::{test_prompt, assert_contains};
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
/// let result = test_prompt("What is the capital of France?").await?;
/// assert_contains(&result, "Paris");
/// # Ok(())
/// # }
/// ```
///
/// # Panics
///
/// Panics if the response does not contain the expected text.
pub fn assert_contains(result: &PromptTestResult, expected: &str) {
    assert!(
        result.response.contains(expected),
        "Expected response to contain '{}', but response was: '{}'",
        expected,
        result.response
    );
}

/// Assert that a prompt test result does not contain specific text.
///
/// # Examples
///
/// ```rust,no_run
/// # use claudius::{test_prompt, assert_not_contains};
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
/// let result = test_prompt("What is the capital of France?").await?;
/// assert_not_contains(&result, "London");
/// # Ok(())
/// # }
/// ```
///
/// # Panics
///
/// Panics if the response contains the unexpected text.
pub fn assert_not_contains(result: &PromptTestResult, unexpected: &str) {
    assert!(
        !result.response.contains(unexpected),
        "Expected response NOT to contain '{}', but response was: '{}'",
        unexpected,
        result.response
    );
}

/// Assert that a prompt test result has a minimum length.
///
/// # Examples
///
/// ```rust,no_run
/// # use claudius::{test_prompt, assert_min_length};
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
/// let result = test_prompt("Write a short story").await?;
/// assert_min_length(&result, 100);
/// # Ok(())
/// # }
/// ```
///
/// # Panics
///
/// Panics if the response is shorter than the minimum length.
pub fn assert_min_length(result: &PromptTestResult, min_length: usize) {
    assert!(
        result.response.len() >= min_length,
        "Expected response length >= {}, but got {} characters: '{}'",
        min_length,
        result.response.len(),
        result.response
    );
}

/// Assert that a prompt test result has a maximum length.
///
/// # Examples
///
/// ```rust,no_run
/// # use claudius::{test_prompt, assert_max_length};
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
/// let result = test_prompt("Say hello").await?;
/// assert_max_length(&result, 50);
/// # Ok(())
/// # }
/// ```
///
/// # Panics
///
/// Panics if the response is longer than the maximum length.
pub fn assert_max_length(result: &PromptTestResult, max_length: usize) {
    assert!(
        result.response.len() <= max_length,
        "Expected response length <= {}, but got {} characters: '{}'",
        max_length,
        result.response.len(),
        result.response
    );
}

/// Assert that all built-in assertions in the test config passed.
///
/// This is a convenience function for checking that all assertions configured
/// in the test (like `expected_contains`, `min_response_length`, etc.) passed.
///
/// # Examples
///
/// ```rust,no_run
/// # use claudius::{PromptTestConfig, Anthropic, assert_test_passed};
/// # #[tokio::main]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let client = Anthropic::new(None)?;
/// let config = PromptTestConfig::new("What is 2 + 2?")
///     .expect_contains("4");
/// let result = config.run(&client).await?;
/// assert_test_passed(&result);
/// # Ok(())
/// # }
/// ```
///
/// # Panics
///
/// Panics if any of the built-in assertions failed.
pub fn assert_test_passed(result: &PromptTestResult) {
    if !result.assertions_passed {
        panic!(
            "Prompt test failed with {} assertion failures:\n{}",
            result.assertion_failures.len(),
            result.assertion_failures.join("\n")
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_basic_config() {
        let config = PromptTestConfig::new("Hello, world!");
        assert_eq!(config.prompt, Some("Hello, world!".to_string()));
        assert_eq!(config.model, None); // Should be None since we didn't set it
        assert_eq!(config.max_tokens, None); // Should be None since we didn't set it
    }

    #[test]
    fn builder_pattern() {
        let config = PromptTestConfig::new("Test prompt")
            .with_name("My Test")
            .with_system("You are helpful")
            .with_model("claude-3-opus-latest")
            .with_max_tokens(500)
            .with_temperature(0.7)
            .expect_contains("hello")
            .expect_not_contains("goodbye")
            .with_min_length(10)
            .with_max_length(100)
            .expect_tool_call("search");

        assert_eq!(config.name, Some("My Test".to_string()));
        assert_eq!(config.system, Some("You are helpful".to_string()));
        assert_eq!(config.model, Some("claude-3-opus-latest".to_string()));
        assert_eq!(config.max_tokens, Some(500));
        assert_eq!(config.temperature, Some(0.7));
        assert_eq!(config.expected_contains, Some(vec!["hello".to_string()]));
        assert_eq!(
            config.expected_not_contains,
            Some(vec!["goodbye".to_string()])
        );
        assert_eq!(config.min_response_length, Some(10));
        assert_eq!(config.max_response_length, Some(100));
        assert_eq!(config.expected_tool_calls, Some(vec!["search".to_string()]));
        assert_eq!(config.prompt, Some("Test prompt".to_string()));
        assert!(config.messages.is_none());
    }

    #[test]
    fn multi_turn_conversation() {
        let messages = vec![
            MessageParam::user("Hello"),
            MessageParam::assistant("Hi there! How can I help you?"),
            MessageParam::user("What's the weather like?"),
        ];

        let config =
            PromptTestConfig::new_conversation(messages.clone()).with_name("Multi-turn test");

        assert_eq!(config.name, Some("Multi-turn test".to_string()));
        assert_eq!(config.messages, Some(messages));
        assert!(config.prompt.is_none());
    }

    #[test]
    fn yaml_serialization() {
        let config = PromptTestConfig::new("Test prompt")
            .with_name("YAML Test")
            .with_system("System prompt")
            .expect_contains("test");

        let yaml = serde_yaml::to_string(&config).expect("Should serialize to YAML");
        let deserialized: PromptTestConfig =
            serde_yaml::from_str(&yaml).expect("Should deserialize from YAML");

        assert_eq!(config.name, deserialized.name);
        assert_eq!(config.prompt, deserialized.prompt);
        assert_eq!(config.system, deserialized.system);
        assert_eq!(config.expected_contains, deserialized.expected_contains);
    }

    #[test]
    fn output_format_builder() {
        use serde_json::json;

        let schema = json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" },
                "age": { "type": "integer" }
            },
            "required": ["name", "age"],
            "additionalProperties": false
        });

        let config = PromptTestConfig::new("Extract info")
            .with_output_format(OutputFormat::json_schema(schema.clone()));

        assert!(config.output_format.is_some());
        match config.output_format.unwrap() {
            OutputFormat::JsonSchema {
                schema: inner_schema,
            } => {
                assert_eq!(inner_schema, schema);
            }
        }
    }

    #[test]
    fn output_format_yaml_serialization() {
        use serde_json::json;

        let schema = json!({
            "type": "object",
            "properties": {
                "result": { "type": "string" }
            },
            "required": ["result"],
            "additionalProperties": false
        });

        let config = PromptTestConfig::new("Test prompt")
            .with_name("Structured Output Test")
            .with_output_format(OutputFormat::json_schema(schema.clone()));

        let yaml = serde_yaml::to_string(&config).expect("Should serialize to YAML");
        let deserialized: PromptTestConfig =
            serde_yaml::from_str(&yaml).expect("Should deserialize from YAML");

        assert_eq!(config.name, deserialized.name);
        assert!(deserialized.output_format.is_some());
        match deserialized.output_format.unwrap() {
            OutputFormat::JsonSchema {
                schema: inner_schema,
            } => {
                assert_eq!(inner_schema, schema);
            }
        }
    }

    #[test]
    fn inheritance_system() {
        // Create temporary directory for test files
        let temp_dir = std::env::temp_dir();
        let test_dir = temp_dir.join("test_inheritance_system");
        std::fs::create_dir_all(&test_dir).unwrap();

        // Create base config file
        let base_yaml = r#"
name: "Base Config"
prompt: "Base prompt"
system: "Base system"
model: "claude-3-5-haiku-latest"
max_tokens: 100
temperature: 0.5
expected_contains:
  - "base"
"#;
        let base_file = test_dir.join("base.yaml");
        std::fs::write(&base_file, base_yaml).unwrap();

        // Create child config file that inherits from base
        let child_yaml = r#"
inherits: "base.yaml"
name: "Child Config"
prompt: "Child prompt"
temperature: 0.7
"#;
        let child_file = test_dir.join("child.yaml");
        std::fs::write(&child_file, child_yaml).unwrap();

        // Load the child config (which should inherit from base)
        let loaded = PromptTestConfig::from_file(&child_file).unwrap();

        // Child values should override base values
        assert_eq!(loaded.name, Some("Child Config".to_string()));
        assert_eq!(loaded.prompt, Some("Child prompt".to_string()));
        assert_eq!(loaded.temperature, Some(0.7));

        // Base values should be inherited where child doesn't specify
        assert_eq!(loaded.system, Some("Base system".to_string()));
        assert_eq!(loaded.max_tokens, Some(100));
        assert_eq!(loaded.expected_contains, Some(vec!["base".to_string()]));

        // Clean up
        std::fs::remove_dir_all(&test_dir).ok();
    }

    #[test]
    fn inheritance_security_check() {
        let temp_dir = std::env::temp_dir();

        // Test that parent directory traversal with non-base.yaml files is rejected
        let yaml_with_traversal = r#"
inherits: "../secrets.yaml"
name: "Malicious Test"
prompt: "test"
"#;

        let test_file = temp_dir.join("test_inheritance_security.yaml");
        std::fs::write(&test_file, yaml_with_traversal).unwrap();

        let load_result = PromptTestConfig::from_file(&test_file);
        assert!(load_result.is_err());
        assert!(load_result.unwrap_err().to_string().contains(
            "cannot use parent directory traversal for security (only base.yaml is allowed)"
        ));

        // Test that parent directory traversal with base.yaml IS allowed
        let yaml_with_base_traversal = r#"
inherits: "../base.yaml"
name: "Base Traversal Test"
prompt: "test"
"#;

        let test_file2 = temp_dir.join("test_base_traversal.yaml");
        std::fs::write(&test_file2, yaml_with_base_traversal).unwrap();

        // This should NOT fail (but might fail due to missing file, which is OK for this test)
        let load_result2 = PromptTestConfig::from_file(&test_file2);
        // We expect this to fail because the base.yaml doesn't exist, not because of security
        if let Err(error) = load_result2 {
            let error_msg = error.to_string();
            assert!(!error_msg.contains("cannot use parent directory traversal"));
        }

        // Clean up
        std::fs::remove_file(&test_file).ok();
        std::fs::remove_file(&test_file2).ok();
    }

    #[test]
    fn inheritance_allows_subdirectories() {
        // Create temporary directory structure for test files
        let temp_dir = std::env::temp_dir();
        let test_dir = temp_dir.join("test_inheritance_subdirs");
        let subdir = test_dir.join("configs");
        std::fs::create_dir_all(&subdir).unwrap();

        // Create base config file in subdirectory
        let base_yaml = r#"
name: "Subdir Base Config"
system: "Base system"
model: "claude-3-5-haiku-latest"
max_tokens: 100
"#;
        let base_file = subdir.join("base.yaml");
        std::fs::write(&base_file, base_yaml).unwrap();

        // Create child config file that inherits from subdirectory
        let child_yaml = r#"
inherits: "configs/base.yaml"
name: "Child Config"
prompt: "Child prompt"
"#;
        let child_file = test_dir.join("child.yaml");
        std::fs::write(&child_file, child_yaml).unwrap();

        // Load the child config (which should inherit from subdirectory)
        let loaded = PromptTestConfig::from_file(&child_file).unwrap();

        // Child values should override base values
        assert_eq!(loaded.name, Some("Child Config".to_string()));
        assert_eq!(loaded.prompt, Some("Child prompt".to_string()));

        // Base values should be inherited from subdirectory
        assert_eq!(loaded.system, Some("Base system".to_string()));
        assert_eq!(loaded.max_tokens, Some(100));

        // Test with ./relative/path syntax too
        let child2_yaml = r#"
inherits: "./configs/base.yaml"
name: "Child Config 2"
prompt: "Child prompt 2"
"#;
        let child2_file = test_dir.join("child2.yaml");
        std::fs::write(&child2_file, child2_yaml).unwrap();

        let loaded2 = PromptTestConfig::from_file(&child2_file).unwrap();
        assert_eq!(loaded2.name, Some("Child Config 2".to_string()));
        assert_eq!(loaded2.system, Some("Base system".to_string()));

        // Clean up
        std::fs::remove_dir_all(&test_dir).ok();
    }

    #[test]
    fn relative_prompt_yaml_resolution() {
        // Create temporary directory for test files
        let temp_dir = std::env::temp_dir();
        let test_dir = temp_dir.join("test_relative_prompt_resolution");
        std::fs::create_dir_all(&test_dir).unwrap();

        // Create prompt.yaml file
        let prompt_content = "This is the content from prompt.yaml file";
        let prompt_file = test_dir.join("prompt.yaml");
        std::fs::write(&prompt_file, prompt_content).unwrap();

        // Create config file that references prompt.yaml as basename
        let config_yaml = r#"
name: "Relative Prompt Test"
prompt: "prompt.yaml"
model: "claude-3-5-haiku-latest"
max_tokens: 100
"#;
        let config_file = test_dir.join("config.yaml");
        std::fs::write(&config_file, config_yaml).unwrap();

        // Load the config (should resolve prompt.yaml relative to config file)
        let loaded = PromptTestConfig::from_file(&config_file).unwrap();

        assert_eq!(loaded.name, Some("Relative Prompt Test".to_string()));
        assert_eq!(loaded.prompt, Some(prompt_content.to_string()));

        // Clean up
        std::fs::remove_dir_all(&test_dir).ok();
    }

    #[test]
    fn relative_system_md_resolution() {
        // Create temporary directory for test files
        let temp_dir = std::env::temp_dir();
        let test_dir = temp_dir.join("test_relative_system_resolution");
        std::fs::create_dir_all(&test_dir).unwrap();

        // Create system.md file
        let system_content = "You are a helpful assistant from system.md file";
        let system_file = test_dir.join("system.md");
        std::fs::write(&system_file, system_content).unwrap();

        // Create config file that references system.md as basename
        let config_yaml = r#"
name: "Relative System Test"
prompt: "Hello world"
system: "system.md"
model: "claude-3-5-haiku-latest"
max_tokens: 100
"#;
        let config_file = test_dir.join("config.yaml");
        std::fs::write(&config_file, config_yaml).unwrap();

        // Load the config (should resolve system.md relative to config file)
        let loaded = PromptTestConfig::from_file(&config_file).unwrap();

        assert_eq!(loaded.name, Some("Relative System Test".to_string()));
        assert_eq!(loaded.prompt, Some("Hello world".to_string()));
        assert_eq!(loaded.system, Some(system_content.to_string()));

        // Clean up
        std::fs::remove_dir_all(&test_dir).ok();
    }

    #[test]
    fn relative_path_resolution_with_subdirectory() {
        // Create temporary directory structure for test files
        let temp_dir = std::env::temp_dir();
        let test_dir = temp_dir.join("test_relative_path_subdirs");
        let subdir = test_dir.join("configs");
        std::fs::create_dir_all(&subdir).unwrap();

        // Create prompt.yaml in subdirectory
        let prompt_content = "Prompt from subdirectory";
        let prompt_file = subdir.join("prompt.yaml");
        std::fs::write(&prompt_file, prompt_content).unwrap();

        // Create system.md in subdirectory
        let system_content = "System from subdirectory";
        let system_file = subdir.join("system.md");
        std::fs::write(&system_file, system_content).unwrap();

        // Create config file in subdirectory that references both
        let config_yaml = r#"
name: "Subdirectory Test"
prompt: "prompt.yaml"
system: "system.md"
model: "claude-3-5-haiku-latest"
"#;
        let config_file = subdir.join("config.yaml");
        std::fs::write(&config_file, config_yaml).unwrap();

        // Load the config (should resolve files relative to subdirectory)
        let loaded = PromptTestConfig::from_file(&config_file).unwrap();

        assert_eq!(loaded.name, Some("Subdirectory Test".to_string()));
        assert_eq!(loaded.prompt, Some(prompt_content.to_string()));
        assert_eq!(loaded.system, Some(system_content.to_string()));

        // Clean up
        std::fs::remove_dir_all(&test_dir).ok();
    }

    #[test]
    fn absolute_paths_not_resolved() {
        // Test that absolute paths in prompt/system fields are not resolved
        let temp_dir = std::env::temp_dir();
        let test_dir = temp_dir.join("test_absolute_paths_not_resolved");
        std::fs::create_dir_all(&test_dir).unwrap();

        // Create config file with absolute path references (should NOT be resolved)
        let config_yaml = r#"
name: "Absolute Path Test"
prompt: "/absolute/path/prompt.yaml"
system: "/absolute/path/system.md"
model: "claude-3-5-haiku-latest"
"#;
        let config_file = test_dir.join("config.yaml");
        std::fs::write(&config_file, config_yaml).unwrap();

        // Load the config (absolute paths should remain as literal strings)
        let loaded = PromptTestConfig::from_file(&config_file).unwrap();

        assert_eq!(loaded.name, Some("Absolute Path Test".to_string()));
        assert_eq!(
            loaded.prompt,
            Some("/absolute/path/prompt.yaml".to_string())
        );
        assert_eq!(loaded.system, Some("/absolute/path/system.md".to_string()));

        // Clean up
        std::fs::remove_dir_all(&test_dir).ok();
    }

    #[test]
    fn include_system_md_from_parent_directory() {
        // Create temporary directory structure for test files
        let temp_dir = std::env::temp_dir();
        let test_dir = temp_dir.join("test_parent_system_include");
        let subdir = test_dir.join("prompts");
        std::fs::create_dir_all(&subdir).unwrap();

        // Create system.md in parent directory
        let system_content = "# Parent System\n\nYou are an AI from the parent directory.";
        let system_file = test_dir.join("system.md");
        std::fs::write(&system_file, system_content).unwrap();

        // Create config file in subdirectory that references parent system.md
        let config_yaml = r#"
name: "Parent System Test"
prompt: "Hello world"
system: "../system.md"
model: "claude-3-5-haiku-latest"
max_tokens: 100
"#;
        let config_file = subdir.join("test.yaml");
        std::fs::write(&config_file, config_yaml).unwrap();

        // Load the config (should resolve system.md from parent directory)
        let loaded = PromptTestConfig::from_file(&config_file).unwrap();

        assert_eq!(loaded.name, Some("Parent System Test".to_string()));
        assert_eq!(loaded.prompt, Some("Hello world".to_string()));
        assert_eq!(loaded.system, Some(system_content.to_string()));

        // Clean up
        std::fs::remove_dir_all(&test_dir).ok();
    }
}
