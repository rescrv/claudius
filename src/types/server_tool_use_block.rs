use serde::{Serialize, Deserialize};
use serde_json::Value;

/// A block representing a server-side tool use request from the model.
///
/// ServerToolUseBlocks indicate the model wants to use a server-side tool (like web search).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerToolUseBlock {
    /// A unique identifier for this tool use request.
    pub id: String,
    
    /// The input data for the tool, can be any valid JSON.
    pub input: Value,
    
    /// The name of the server tool being invoked.
    /// Currently only "web_search" is supported.
    #[serde(default = "default_name")]
    pub name: String,
    
    /// The type of content block, always "server_tool_use" for this struct.
    #[serde(default = "default_type")]
    pub r#type: String,
}

fn default_name() -> String {
    "web_search".to_string()
}

fn default_type() -> String {
    "server_tool_use".to_string()
}

impl ServerToolUseBlock {
    /// Creates a new ServerToolUseBlock with the specified id and input.
    /// The name is set to "web_search" as that's the only supported server tool.
    pub fn new<S: Into<String>>(id: S, input: Value) -> Self {
        Self {
            id: id.into(),
            input,
            name: default_name(),
            r#type: default_type(),
        }
    }
    
    /// Creates a new web search ServerToolUseBlock with the specified id and query.
    pub fn new_web_search<S1: Into<String>, S2: Into<String>>(id: S1, query: S2) -> Self {
        let input = serde_json::json!({
            "query": query.into()
        });
        
        Self::new(id, input)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_server_tool_use_block_serialization() {
        let input_json = serde_json::json!({
            "query": "weather in San Francisco"
        });
        
        let block = ServerToolUseBlock::new("tool_123", input_json);
        
        let json = serde_json::to_string(&block).unwrap();
        let expected = r#"{"id":"tool_123","input":{"query":"weather in San Francisco"},"name":"web_search","type":"server_tool_use"}"#;
        
        assert_eq!(json, expected);
    }
    
    #[test]
    fn test_new_web_search() {
        let block = ServerToolUseBlock::new_web_search("tool_123", "weather in San Francisco");
        
        let json = serde_json::to_string(&block).unwrap();
        let expected = r#"{"id":"tool_123","input":{"query":"weather in San Francisco"},"name":"web_search","type":"server_tool_use"}"#;
        
        assert_eq!(json, expected);
    }
    
    #[test]
    fn test_deserialization() {
        let json = r#"{"id":"tool_123","input":{"query":"weather in San Francisco"},"name":"web_search","type":"server_tool_use"}"#;
        let block: ServerToolUseBlock = serde_json::from_str(json).unwrap();
        
        assert_eq!(block.id, "tool_123");
        assert_eq!(block.name, "web_search");
        assert_eq!(block.r#type, "server_tool_use");
        
        let expected_input = serde_json::json!({
            "query": "weather in San Francisco"
        });
        assert_eq!(block.input, expected_input);
    }
}