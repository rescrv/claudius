use serde::{Deserialize, Serialize};

use crate::types::CacheControlEphemeral;
use crate::types::tool_param::{InputSchema, ToolParam};

/// Represents different types of tools that can be used for token counting.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum MessageCountTokensToolParam {
    /// Custom tool with a basic structure.
    #[serde(rename = "custom")]
    Custom {
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
    },

    /// Bash tool for command execution.
    #[serde(rename = "bash_20250124")]
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
    },

    /// Text editor tool.
    #[serde(rename = "text_editor_20250124")]
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
    },

    /// Web search tool.
    #[serde(rename = "web_search_20250305")]
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
    },
}

impl MessageCountTokensToolParam {
    /// Create a new custom tool parameter.
    pub fn new_custom(name: String, input_schema: InputSchema) -> Self {
        Self::Custom {
            name,
            input_schema,
            cache_control: None,
            description: None,
        }
    }

    /// Create a new bash tool parameter.
    pub fn new_bash(name: String, input_schema: InputSchema) -> Self {
        Self::Bash {
            name,
            input_schema,
            cache_control: None,
            description: None,
        }
    }

    /// Create a new text editor tool parameter.
    pub fn new_text_editor(name: String, input_schema: InputSchema) -> Self {
        Self::TextEditor {
            name,
            input_schema,
            cache_control: None,
            description: None,
        }
    }

    /// Create a new web search tool parameter.
    pub fn new_web_search(name: String, input_schema: InputSchema) -> Self {
        Self::WebSearch {
            name,
            input_schema,
            cache_control: None,
            description: None,
        }
    }

    /// Add a description to the tool.
    pub fn with_description(self, description: String) -> Self {
        match self {
            Self::Custom {
                name,
                input_schema,
                cache_control,
                ..
            } => Self::Custom {
                name,
                input_schema,
                cache_control,
                description: Some(description),
            },
            Self::Bash {
                name,
                input_schema,
                cache_control,
                ..
            } => Self::Bash {
                name,
                input_schema,
                cache_control,
                description: Some(description),
            },
            Self::TextEditor {
                name,
                input_schema,
                cache_control,
                ..
            } => Self::TextEditor {
                name,
                input_schema,
                cache_control,
                description: Some(description),
            },
            Self::WebSearch {
                name,
                input_schema,
                cache_control,
                ..
            } => Self::WebSearch {
                name,
                input_schema,
                cache_control,
                description: Some(description),
            },
        }
    }

    /// Add cache control to the tool.
    pub fn with_cache_control(self, cache_control: CacheControlEphemeral) -> Self {
        match self {
            Self::Custom {
                name,
                input_schema,
                description,
                ..
            } => Self::Custom {
                name,
                input_schema,
                cache_control: Some(cache_control),
                description,
            },
            Self::Bash {
                name,
                input_schema,
                description,
                ..
            } => Self::Bash {
                name,
                input_schema,
                cache_control: Some(cache_control),
                description,
            },
            Self::TextEditor {
                name,
                input_schema,
                description,
                ..
            } => Self::TextEditor {
                name,
                input_schema,
                cache_control: Some(cache_control),
                description,
            },
            Self::WebSearch {
                name,
                input_schema,
                description,
                ..
            } => Self::WebSearch {
                name,
                input_schema,
                cache_control: Some(cache_control),
                description,
            },
        }
    }
}

impl From<ToolParam> for MessageCountTokensToolParam {
    fn from(param: ToolParam) -> Self {
        Self::Custom {
            name: param.name,
            input_schema: param.input_schema,
            cache_control: param.cache_control,
            description: param.description,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, to_value};
    use std::collections::HashMap;

    #[test]
    fn test_message_count_tokens_tool_param_custom() {
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

        let tool = MessageCountTokensToolParam::new_custom("search".to_string(), input_schema)
            .with_description("Search for information".to_string());

        let json = to_value(&tool).unwrap();
        assert_eq!(
            json,
            json!({
                "type": "custom",
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
                "type": "bash_20250124",
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
                "name": "bash"
            })
        );
    }

    #[test]
    fn test_message_count_tokens_tool_param_text_editor() {
        let input_schema = InputSchema::Generic(
            serde_json::from_value(json!({
                "type": "object",
                "properties": {
                    "file_path": {
                        "type": "string",
                        "description": "The file path to edit"
                    },
                    "content": {
                        "type": "string",
                        "description": "The content to write to the file"
                    }
                },
                "required": ["file_path", "content"]
            }))
            .unwrap(),
        );

        let tool = MessageCountTokensToolParam::new_text_editor("editor".to_string(), input_schema)
            .with_description("Edit text files".to_string());

        let json = to_value(&tool).unwrap();
        assert_eq!(
            json,
            json!({
                "type": "text_editor_20250124",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "file_path": {
                            "type": "string",
                            "description": "The file path to edit"
                        },
                        "content": {
                            "type": "string",
                            "description": "The content to write to the file"
                        }
                    },
                    "required": ["file_path", "content"]
                },
                "name": "editor",
                "description": "Edit text files"
            })
        );
    }

    #[test]
    fn test_message_count_tokens_tool_param_web_search() {
        let input_schema = InputSchema::Generic(
            serde_json::from_value(json!({
                "type": "object",
                "properties": {
                    "query": {
                        "type": "string",
                        "description": "The search query"
                    }
                },
                "required": ["query"]
            }))
            .unwrap(),
        );

        let cache_control = CacheControlEphemeral::new();

        let tool = MessageCountTokensToolParam::new_web_search("search".to_string(), input_schema)
            .with_cache_control(cache_control);

        let json = to_value(&tool).unwrap();
        assert_eq!(
            json,
            json!({
                "type": "web_search_20250305",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "query": {
                            "type": "string",
                            "description": "The search query"
                        }
                    },
                    "required": ["query"]
                },
                "name": "search",
                "cache_control": {
                    "type": "ephemeral"
                }
            })
        );
    }

    #[test]
    fn test_message_count_tokens_tool_param_from_tool_param() {
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

        let tool_param = ToolParam::new("search".to_string(), input_schema.clone())
            .with_description("Search for information".to_string());

        let tool = MessageCountTokensToolParam::from(tool_param);

        let json = to_value(&tool).unwrap();
        assert_eq!(
            json,
            json!({
                "type": "custom",
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
}
