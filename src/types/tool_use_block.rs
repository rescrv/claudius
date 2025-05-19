use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A block representing a tool use request from the model.
///
/// ToolUseBlocks indicate the model wants to use a specific tool with certain inputs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolUseBlock {
    /// A unique identifier for this tool use request.
    pub id: String,

    /// The input data for the tool, can be any valid JSON.
    pub input: Value,

    /// The name of the tool being invoked.
    pub name: String,

    /// The type of content block, always "tool_use" for this struct.
    #[serde(default = "default_type")]
    pub r#type: String,
}

fn default_type() -> String {
    "tool_use".to_string()
}

impl ToolUseBlock {
    /// Creates a new ToolUseBlock with the specified id, name, and input.
    pub fn new<S1: Into<String>, S2: Into<String>>(id: S1, name: S2, input: Value) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            input,
            r#type: default_type(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_use_block_serialization() {
        let input_json = serde_json::json!({
            "query": "weather in San Francisco",
            "limit": 5
        });

        let block = ToolUseBlock::new("tool_123", "search", input_json);

        let json = serde_json::to_string(&block).unwrap();
        let expected = r#"{"id":"tool_123","input":{"limit":5,"query":"weather in San Francisco"},"name":"search","type":"tool_use"}"#;

        assert_eq!(json, expected);
    }

    #[test]
    fn test_deserialization() {
        let json = r#"{"id":"tool_123","input":{"query":"weather in San Francisco","limit":5},"name":"search","type":"tool_use"}"#;
        let block: ToolUseBlock = serde_json::from_str(json).unwrap();

        assert_eq!(block.id, "tool_123");
        assert_eq!(block.name, "search");
        assert_eq!(block.r#type, "tool_use");

        let expected_input = serde_json::json!({
            "query": "weather in San Francisco",
            "limit": 5
        });
        assert_eq!(block.input, expected_input);
    }
}
