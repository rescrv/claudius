use serde::{Deserialize, Serialize};

use crate::types::ServerToolUsage;

/// Usage information for API calls.
///
/// Anthropic's API bills and rate-limits by token counts, as tokens represent the
/// underlying cost to their systems.
#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq)]
pub struct Usage {
    /// The number of input tokens used to create the cache entry.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_creation_input_tokens: Option<i32>,

    /// The number of input tokens read from the cache.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_read_input_tokens: Option<i32>,

    /// The number of input tokens which were used.
    pub input_tokens: i32,

    /// The number of output tokens which were used.
    pub output_tokens: i32,

    /// The number of server tool requests.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_tool_use: Option<ServerToolUsage>,
}

impl Usage {
    /// Create a new `Usage` with the given input and output tokens.
    pub fn new(input_tokens: i32, output_tokens: i32) -> Self {
        Self {
            cache_creation_input_tokens: None,
            cache_read_input_tokens: None,
            input_tokens,
            output_tokens,
            server_tool_use: None,
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
    fn usage_minimal() {
        let usage = Usage::new(50, 100);
        let json = to_value(usage).unwrap();

        assert_eq!(
            json,
            json!({
                "input_tokens": 50,
                "output_tokens": 100
            })
        );
    }

    #[test]
    fn usage_complete() {
        let server_tool_use = ServerToolUsage::new(5);
        let usage = Usage::new(50, 100)
            .with_cache_creation_input_tokens(20)
            .with_cache_read_input_tokens(30)
            .with_server_tool_use(server_tool_use);

        let json = to_value(usage).unwrap();

        assert_eq!(
            json,
            json!({
                "cache_creation_input_tokens": 20,
                "cache_read_input_tokens": 30,
                "input_tokens": 50,
                "output_tokens": 100,
                "server_tool_use": {
                    "web_search_requests": 5
                }
            })
        );
    }

    #[test]
    fn usage_deserialization() {
        let json = json!({
            "cache_creation_input_tokens": 20,
            "cache_read_input_tokens": 30,
            "input_tokens": 50,
            "output_tokens": 100,
            "server_tool_use": {
                "web_search_requests": 5
            }
        });

        let usage: Usage = serde_json::from_value(json).unwrap();
        assert_eq!(usage.cache_creation_input_tokens, Some(20));
        assert_eq!(usage.cache_read_input_tokens, Some(30));
        assert_eq!(usage.input_tokens, 50);
        assert_eq!(usage.output_tokens, 100);
        assert_eq!(usage.server_tool_use, Some(ServerToolUsage::new(5)));
    }
}
