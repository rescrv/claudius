use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::types::CacheControlEphemeral;

/// Parameters for a tool use block.
///
/// This represents a block that describes the use of a tool by the model.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolUseBlockParam {
    /// The ID of the tool use.
    pub id: String,

    /// The input to the tool, which can be any JSON value.
    pub input: Value,

    /// The name of the tool.
    pub name: String,

    /// The type, which is always "tool_use".
    pub r#type: String,

    /// Create a cache control breakpoint at this content block.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControlEphemeral>,
}

impl ToolUseBlockParam {
    /// Create a new `ToolUseBlockParam` with the given ID, input, and name.
    pub fn new(id: String, input: Value, name: String) -> Self {
        Self {
            id,
            input,
            name,
            r#type: "tool_use".to_string(),
            cache_control: None,
        }
    }

    /// Add a cache control to this tool use block.
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
    fn test_tool_use_block_param_serialization() {
        let input = json!({
            "query": "weather in San Francisco"
        });

        let block = ToolUseBlockParam::new("tool_1".to_string(), input, "web_search".to_string());

        let json = to_value(&block).unwrap();
        assert_eq!(
            json,
            json!({
                "id": "tool_1",
                "input": {
                    "query": "weather in San Francisco"
                },
                "name": "web_search",
                "type": "tool_use"
            })
        );
    }

    #[test]
    fn test_tool_use_block_param_with_cache_control() {
        let input = json!({
            "query": "weather in San Francisco"
        });

        let cache_control = CacheControlEphemeral::new();
        let block = ToolUseBlockParam::new("tool_1".to_string(), input, "web_search".to_string())
            .with_cache_control(cache_control);

        let json = to_value(&block).unwrap();
        assert_eq!(
            json,
            json!({
                "id": "tool_1",
                "input": {
                    "query": "weather in San Francisco"
                },
                "name": "web_search",
                "type": "tool_use",
                "cache_control": {
                    "type": "ephemeral"
                }
            })
        );
    }

    #[test]
    fn test_tool_use_block_param_deserialization() {
        let json = json!({
            "id": "tool_1",
            "input": {
                "query": "weather in San Francisco"
            },
            "name": "web_search",
            "type": "tool_use"
        });

        let block: ToolUseBlockParam = serde_json::from_value(json).unwrap();
        assert_eq!(block.id, "tool_1");
        assert_eq!(block.input, json!({ "query": "weather in San Francisco" }));
        assert_eq!(block.name, "web_search");
        assert_eq!(block.r#type, "tool_use");
        assert!(block.cache_control.is_none());
    }
}
