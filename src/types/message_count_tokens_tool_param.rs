use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::types::CacheControlEphemeral;

/// Represents the schema for a tool input.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum InputSchema {
    /// A typed input schema with an object type.
    Typed {
        /// The type of the schema is always "object".
        r#type: String,

        /// Optional properties of the schema.
        #[serde(skip_serializing_if = "Option::is_none")]
        properties: Option<serde_json::Value>,

        /// Additional fields.
        #[serde(flatten)]
        additional: HashMap<String, serde_json::Value>,
    },

    /// A generic input schema represented as a map.
    Generic(HashMap<String, serde_json::Value>),
}

/// Common parameters for a custom tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolParam {
    /// [JSON schema](https://json-schema.org/draft/2020-12) for this tool's input.
    ///
    /// This defines the shape of the `input` that your tool accepts and that the model
    /// will produce.
    pub input_schema: InputSchema,

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

    /// The type of the tool, always "custom" when specified.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,
}

impl ToolParam {
    /// Create a new `ToolParam` with the required fields.
    pub fn new(name: String, input_schema: InputSchema) -> Self {
        Self {
            name,
            input_schema,
            cache_control: None,
            description: None,
            r#type: None,
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

    /// Set the type to "custom".
    pub fn with_custom_type(mut self) -> Self {
        self.r#type = Some("custom".to_string());
        self
    }
}

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
