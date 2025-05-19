use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::types::CacheControlEphemeral;

/// Parameters for a server tool use block.
///
/// This represents a block that describes the use of a server-side tool by the model.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ServerToolUseBlockParam {
    /// The ID of the server tool use.
    pub id: String,

    /// The input to the server tool, which can be any JSON value.
    pub input: Value,

    /// The name of the server tool, which is always "web_search".
    pub name: String,

    /// The type, which is always "server_tool_use".
    pub r#type: String,

    /// Create a cache control breakpoint at this content block.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControlEphemeral>,
}

impl ServerToolUseBlockParam {
    /// Create a new `ServerToolUseBlockParam` with the given ID and input.
    pub fn new(id: String, input: Value) -> Self {
        Self {
            id,
            input,
            name: "web_search".to_string(),
            r#type: "server_tool_use".to_string(),
            cache_control: None,
        }
    }

    /// Add a cache control to this server tool use block.
    pub fn with_cache_control(mut self, cache_control: CacheControlEphemeral) -> Self {
        self.cache_control = Some(cache_control);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, to_value};

    #[test]
    fn test_server_tool_use_block_param_serialization() {
        let input = json!({
            "query": "weather in San Francisco"
        });

        let block = ServerToolUseBlockParam::new("server_tool_1".to_string(), input);

        let json = to_value(&block).unwrap();
        assert_eq!(
            json,
            json!({
                "id": "server_tool_1",
                "input": {
                    "query": "weather in San Francisco"
                },
                "name": "web_search",
                "type": "server_tool_use"
            })
        );
    }

    #[test]
    fn test_server_tool_use_block_param_with_cache_control() {
        let input = json!({
            "query": "weather in San Francisco"
        });

        let cache_control = CacheControlEphemeral::new();
        let block = ServerToolUseBlockParam::new("server_tool_1".to_string(), input)
            .with_cache_control(cache_control);

        let json = to_value(&block).unwrap();
        assert_eq!(
            json,
            json!({
                "id": "server_tool_1",
                "input": {
                    "query": "weather in San Francisco"
                },
                "name": "web_search",
                "type": "server_tool_use",
                "cache_control": {
                    "type": "ephemeral"
                }
            })
        );
    }

    #[test]
    fn test_server_tool_use_block_param_deserialization() {
        let json = json!({
            "id": "server_tool_1",
            "input": {
                "query": "weather in San Francisco"
            },
            "name": "web_search",
            "type": "server_tool_use"
        });

        let block: ServerToolUseBlockParam = serde_json::from_value(json).unwrap();
        assert_eq!(block.id, "server_tool_1");
        assert_eq!(block.input, json!({ "query": "weather in San Francisco" }));
        assert_eq!(block.name, "web_search");
        assert_eq!(block.r#type, "server_tool_use");
        assert!(block.cache_control.is_none());
    }
}
