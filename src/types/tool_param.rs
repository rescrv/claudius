use serde::{Deserialize, Serialize};

use crate::types::CacheControlEphemeral;

/// Common parameters for a custom tool.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolParam {
    /// [JSON schema](https://json-schema.org/draft/2020-12) for this tool's input.
    ///
    /// This defines the shape of the `input` that your tool accepts and that the model
    /// will produce.
    pub input_schema: serde_json::Value,

    /// Name of the tool.
    ///
    /// This is how the tool will be called by the model and in `tool_use` blocks.
    pub name: String,

    /// Create a cache control breakpoint at this content block.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControlEphemeral>,

    /// Description of what this tool does.
    ///
    /// Tool descriptions should be as detailed as possible. The more information that
    /// the model has about what the tool is and how to use it, the better it will
    /// perform. You can use natural language descriptions to reinforce important
    /// aspects of the tool input JSON schema.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl ToolParam {
    /// Create a new `ToolParam` with the required fields.
    pub fn new(name: String, input_schema: serde_json::Value) -> Self {
        Self {
            name,
            input_schema,
            cache_control: None,
            description: None,
        }
    }

    /// Add a description to the tool.
    pub fn with_description(mut self, description: String) -> Self {
        self.description = Some(description);
        self
    }

    /// Add cache control to the tool.
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
    fn test_tool_param_complete() {
        let input_schema = json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "The search query"
                }
            }
        });

        let cache_control = CacheControlEphemeral::new();

        let tool = ToolParam::new("search".to_string(), input_schema)
            .with_description("Search for information".to_string())
            .with_cache_control(cache_control);

        let json = to_value(&tool).unwrap();
        assert_eq!(
            json,
            json!({
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "The search query"
                        }
                    }
                },
                "name": "search",
                "cache_control": {
                    "type": "ephemeral"
                },
                "description": "Search for information"
            })
        );
    }
}
