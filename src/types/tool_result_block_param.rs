use serde::{Deserialize, Serialize};

use crate::types::{CacheControlEphemeral, ImageBlockParam, TextBlockParam};

/// Content type for tool result blocks, which can be either a text block or an image block.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum ToolResultContent {
    /// A text block content.
    Text(TextBlockParam),

    /// An image block content.
    Image(ImageBlockParam),
}

impl From<TextBlockParam> for ToolResultContent {
    fn from(param: TextBlockParam) -> Self {
        ToolResultContent::Text(param)
    }
}

impl From<ImageBlockParam> for ToolResultContent {
    fn from(param: ImageBlockParam) -> Self {
        ToolResultContent::Image(param)
    }
}

/// Parameters for a tool result block.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolResultBlockParam {
    /// The ID of the tool use that this result is for.
    #[serde(rename = "tool_use_id")]
    pub tool_use_id: String,

    /// The type, which is always "tool_result".
    pub r#type: String,

    /// Create a cache control breakpoint at this content block.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControlEphemeral>,

    /// The content of the tool result, which can be either a string or an array of content items.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<ToolResultBlockParamContent>,

    /// Whether this tool result represents an error.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_error: Option<bool>,
}

/// The content of a tool result block, which can be either a string or an array of content items.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum ToolResultBlockParamContent {
    /// A simple string content.
    String(String),

    /// An array of content items.
    Array(Vec<ToolResultContent>),
}

impl ToolResultBlockParam {
    /// Create a new `ToolResultBlockParam` with the given tool use ID.
    pub fn new(tool_use_id: String) -> Self {
        Self {
            tool_use_id,
            r#type: "tool_result".to_string(),
            cache_control: None,
            content: None,
            is_error: None,
        }
    }

    /// Add a cache control to this tool result block.
    pub fn with_cache_control(mut self, cache_control: CacheControlEphemeral) -> Self {
        self.cache_control = Some(cache_control);
        self
    }

    /// Add string content to this tool result block.
    pub fn with_string_content(mut self, content: String) -> Self {
        self.content = Some(ToolResultBlockParamContent::String(content));
        self
    }

    /// Add array content to this tool result block.
    pub fn with_array_content(mut self, content: Vec<ToolResultContent>) -> Self {
        self.content = Some(ToolResultBlockParamContent::Array(content));
        self
    }

    /// Add a single text content item to this tool result block.
    pub fn with_text_content(mut self, text: TextBlockParam) -> Self {
        let content = match self.content {
            Some(ToolResultBlockParamContent::Array(mut items)) => {
                items.push(ToolResultContent::Text(text));
                ToolResultBlockParamContent::Array(items)
            }
            Some(ToolResultBlockParamContent::String(s)) => {
                ToolResultBlockParamContent::Array(vec![
                    ToolResultContent::Text(TextBlockParam::new(s)),
                    ToolResultContent::Text(text),
                ])
            }
            None => ToolResultBlockParamContent::Array(vec![ToolResultContent::Text(text)]),
        };
        self.content = Some(content);
        self
    }

    /// Set this tool result block as an error.
    pub fn with_error(mut self, is_error: bool) -> Self {
        self.is_error = Some(is_error);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, to_value};

    #[test]
    fn test_tool_result_block_param_with_string_content() {
        let block = ToolResultBlockParam::new("tool_1".to_string())
            .with_string_content("Result of tool execution".to_string());

        let json = to_value(&block).unwrap();
        assert_eq!(
            json,
            json!({
                "tool_use_id": "tool_1",
                "type": "tool_result",
                "content": "Result of tool execution"
            })
        );
    }

    #[test]
    fn test_tool_result_block_param_with_array_content() {
        let text_param = TextBlockParam::new("Sample text content".to_string());
        let content = vec![ToolResultContent::Text(text_param)];

        let block = ToolResultBlockParam::new("tool_1".to_string()).with_array_content(content);

        let json = to_value(&block).unwrap();
        assert_eq!(
            json,
            json!({
                "tool_use_id": "tool_1",
                "type": "tool_result",
                "content": [
                    {
                        "text": "Sample text content",
                        "type": "text"
                    }
                ]
            })
        );
    }

    #[test]
    fn test_tool_result_block_param_with_error() {
        let block = ToolResultBlockParam::new("tool_1".to_string())
            .with_string_content("Error executing tool".to_string())
            .with_error(true);

        let json = to_value(&block).unwrap();
        assert_eq!(
            json,
            json!({
                "tool_use_id": "tool_1",
                "type": "tool_result",
                "content": "Error executing tool",
                "is_error": true
            })
        );
    }

    #[test]
    fn test_tool_result_block_param_deserialization() {
        let json = json!({
            "tool_use_id": "tool_1",
            "type": "tool_result",
            "content": "Result of tool execution",
            "is_error": false
        });

        let block: ToolResultBlockParam = serde_json::from_value(json).unwrap();
        assert_eq!(block.tool_use_id, "tool_1");
        assert_eq!(block.r#type, "tool_result");

        match &block.content {
            Some(ToolResultBlockParamContent::String(s)) => {
                assert_eq!(s, "Result of tool execution");
            }
            _ => panic!("Expected String variant"),
        }

        assert_eq!(block.is_error, Some(false));
    }
}
