use serde::{Deserialize, Serialize};

use crate::types::CacheControlEphemeral;
use crate::types::tool_param::{InputSchema, ToolParam};

/// Represents different types of tools that can be used for token counting.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageCountTokensToolParam {
    /// Custom tool with a basic structure.
    Tool(ToolParam),

    /// Bash tool for command execution.
    Bash {
        /// [JSON schema](https://json-schema.org/draft/2020-12) for this tool's input.
        input_schema: InputSchema,

        /// Name of the tool.
        name: String,

        /// Create a cache control breakpoint at this content block.
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<CacheControlEphemeral>,

        /// Description of what this tool does.
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,

        /// The type of the tool, always "bash_20250124" for this variant.
        r#type: String,
    },

    /// Text editor tool.
    TextEditor {
        /// [JSON schema](https://json-schema.org/draft/2020-12) for this tool's input.
        input_schema: InputSchema,

        /// Name of the tool.
        name: String,

        /// Create a cache control breakpoint at this content block.
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<CacheControlEphemeral>,

        /// Description of what this tool does.
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,

        /// The type of the tool, always "text_editor_20250124" for this variant.
        r#type: String,
    },

    /// Web search tool.
    WebSearch {
        /// [JSON schema](https://json-schema.org/draft/2020-12) for this tool's input.
        input_schema: InputSchema,

        /// Name of the tool.
        name: String,

        /// Create a cache control breakpoint at this content block.
        #[serde(skip_serializing_if = "Option::is_none")]
        cache_control: Option<CacheControlEphemeral>,

        /// Description of what this tool does.
        #[serde(skip_serializing_if = "Option::is_none")]
        description: Option<String>,

        /// The type of the tool, always "web_search_20250305" for this variant.
        r#type: String,
    },
}

impl MessageCountTokensToolParam {
    /// Create a new custom tool parameter.
    pub fn new_custom(name: String, input_schema: InputSchema) -> Self {
        Self::Tool(ToolParam::new(name, input_schema))
    }

    /// Create a new bash tool parameter.
    pub fn new_bash(name: String, input_schema: InputSchema) -> Self {
        Self::Bash {
            name,
            input_schema,
            cache_control: None,
            description: None,
            r#type: "bash_20250124".to_string(),
        }
    }

    /// Create a new text editor tool parameter.
    pub fn new_text_editor(name: String, input_schema: InputSchema) -> Self {
        Self::TextEditor {
            name,
            input_schema,
            cache_control: None,
            description: None,
            r#type: "text_editor_20250124".to_string(),
        }
    }

    /// Create a new web search tool parameter.
    pub fn new_web_search(name: String, input_schema: InputSchema) -> Self {
        Self::WebSearch {
            name,
            input_schema,
            cache_control: None,
            description: None,
            r#type: "web_search_20250305".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, to_value};
    use std::collections::HashMap;

    #[test]
    fn test_tool_param_serialization() {
        let input_schema = InputSchema::Typed {
            r#type: "object".to_string(),
            properties: Some(json!({
                "query": {
                    "type": "string",
                    "description": "The search query"
                }
            })),
            additional: HashMap::new(),
        };

        let tool = ToolParam::new("search".to_string(), input_schema)
            .with_description("Search for information".to_string());

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
                "description": "Search for information"
            })
        );
    }

    #[test]
    fn test_message_count_tokens_tool_param_bash() {
        let input_schema = InputSchema::Generic(
            serde_json::from_value(json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The bash command to execute"
                    }
                },
                "required": ["command"]
            }))
            .unwrap(),
        );

        let tool = MessageCountTokensToolParam::new_bash("bash".to_string(), input_schema);

        let json = to_value(&tool).unwrap();
        assert_eq!(
            json,
            json!({
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": "The bash command to execute"
                        }
                    },
                    "required": ["command"]
                },
                "name": "bash",
                "type": "bash_20250124"
            })
        );
    }
}
