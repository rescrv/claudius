use serde::{Deserialize, Serialize};

use crate::types::{OutputTokensDetails, ServerToolUsage, UsageIteration};

/// Message delta usage information.
///
/// Anthropic's API bills and rate-limits by token counts, as tokens represent the
/// underlying cost to their systems.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MessageDeltaUsage {
    /// The cumulative number of input tokens used to create the cache entry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_creation_input_tokens: Option<i32>,

    /// The cumulative number of input tokens read from the cache.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_read_input_tokens: Option<i32>,

    /// The cumulative number of input tokens which were used.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<i32>,

    /// The cumulative number of output tokens which were used.
    pub output_tokens: i32,

    /// Breakdown of output tokens by category.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens_details: Option<OutputTokensDetails>,

    /// The number of server tool requests.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_tool_use: Option<ServerToolUsage>,

    /// Per-model attempts made by fallback routing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iterations: Option<Vec<UsageIteration>>,
}

impl MessageDeltaUsage {
    /// Create a new `MessageDeltaUsage` with the given output tokens.
    pub fn new(output_tokens: i32) -> Self {
        Self {
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
            input_tokens: None,
            output_tokens,
            output_tokens_details: None,
            server_tool_use: None,
            iterations: None,
        }
    }

    /// Set the cache creation input tokens.
    pub fn with_cache_creation_input_tokens(mut self, tokens: i32) -> Self {
        self.cache_creation_input_tokens = Some(tokens);
        self
    }

    /// Set the cache read input tokens.
    pub fn with_cache_read_input_tokens(mut self, tokens: i32) -> Self {
        self.cache_read_input_tokens = Some(tokens);
        self
    }

    /// Set the input tokens.
    pub fn with_input_tokens(mut self, tokens: i32) -> Self {
        self.input_tokens = Some(tokens);
        self
    }

    /// Set the output-token breakdown.
    pub fn with_output_tokens_details(mut self, details: OutputTokensDetails) -> Self {
        self.output_tokens_details = Some(details);
        self
    }

    /// Set the thinking-token count in the output-token breakdown.
    pub fn with_thinking_tokens(self, thinking_tokens: i32) -> Self {
        self.with_output_tokens_details(OutputTokensDetails::new(thinking_tokens))
    }

    /// Set the server tool usage.
    pub fn with_server_tool_use(mut self, server_tool_use: ServerToolUsage) -> Self {
        self.server_tool_use = Some(server_tool_use);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, to_value};

    #[test]
    fn message_delta_usage_minimal() {
        let usage = MessageDeltaUsage::new(100);
        let json = to_value(&usage).unwrap();

        assert_eq!(
            json,
            json!({
                "output_tokens": 100
            })
        );
    }

    #[test]
    fn message_delta_usage_complete() {
        let server_tool_use = ServerToolUsage::new(5);
        let usage = MessageDeltaUsage::new(100)
            .with_cache_creation_input_tokens(20)
            .with_cache_read_input_tokens(30)
            .with_input_tokens(50)
            .with_thinking_tokens(40)
            .with_server_tool_use(server_tool_use);

        let json = to_value(&usage).unwrap();

        assert_eq!(
            json,
            json!({
                "cache_creation_input_tokens": 20,
                "cache_read_input_tokens": 30,
                "input_tokens": 50,
                "output_tokens": 100,
                "output_tokens_details": {
                    "thinking_tokens": 40
                },
                "server_tool_use": {
                    "web_search_requests": 5
                }
            })
        );
    }

    #[test]
    fn message_delta_usage_deserialization() {
        let json = json!({
            "cache_creation_input_tokens": 20,
            "cache_read_input_tokens": 30,
            "input_tokens": 50,
            "output_tokens": 100,
            "output_tokens_details": {
                "thinking_tokens": 40
            },
            "server_tool_use": {
                "web_search_requests": 5
            }
        });

        let usage: MessageDeltaUsage = serde_json::from_value(json).unwrap();
        assert_eq!(usage.cache_creation_input_tokens, Some(20));
        assert_eq!(usage.cache_read_input_tokens, Some(30));
        assert_eq!(usage.input_tokens, Some(50));
        assert_eq!(usage.output_tokens, 100);
        assert_eq!(
            usage.output_tokens_details,
            Some(OutputTokensDetails::new(40))
        );
        assert_eq!(usage.server_tool_use, Some(ServerToolUsage::new(5)));
    }
}
