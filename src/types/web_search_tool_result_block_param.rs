use serde::{Deserialize, Serialize};

use crate::types::{
    CacheControlEphemeral, WebSearchResultBlock, WebSearchToolRequestErrorParam,
    WebSearchToolResultBlockParamContent,
};

/// Parameters for a web search tool result block.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WebSearchToolResultBlockParam {
    /// The content of the web search tool result.
    pub content: WebSearchToolResultBlockParamContent,

    /// The ID of the tool use that this result is for.
    #[serde(rename = "tool_use_id")]
    pub tool_use_id: String,

    /// Create a cache control breakpoint at this content block.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControlEphemeral>,
}

impl WebSearchToolResultBlockParam {
    /// Create a new `WebSearchToolResultBlockParam` with the given content and tool use ID.
    pub fn new(content: WebSearchToolResultBlockParamContent, tool_use_id: String) -> Self {
        Self {
            content,
            tool_use_id,
            cache_control: None,
        }
    }

    /// Create a new `WebSearchToolResultBlockParam` with the given results and tool use ID.
    pub fn new_with_results(results: Vec<WebSearchResultBlock>, tool_use_id: String) -> Self {
        Self::new(
            WebSearchToolResultBlockParamContent::new_with_results(results),
            tool_use_id,
        )
    }

    /// Create a new `WebSearchToolResultBlockParam` with the given error and tool use ID.
    pub fn new_with_error(error: WebSearchToolRequestErrorParam, tool_use_id: String) -> Self {
        Self::new(
            WebSearchToolResultBlockParamContent::new_with_error(error),
            tool_use_id,
        )
    }

    /// Add a cache control to this web search tool result block.
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
    fn test_web_search_tool_result_block_param_with_results() {
        let result = WebSearchResultBlock::new(
            "encrypted-content",
            "Example Title",
            "https://example.com",
        );

        let block =
            WebSearchToolResultBlockParam::new_with_results(vec![result], "tool_1".to_string());

        let json = to_value(&block).unwrap();
        assert_eq!(
            json,
            json!({
                "content": [
                    {
                        "encrypted_content": "encrypted-content",
                        "title": "Example Title",
                        "type": "web_search_result",
                        "url": "https://example.com"
                    }
                ],
                "tool_use_id": "tool_1"
            })
        );
    }

    #[test]
    fn test_web_search_tool_result_block_param_with_error() {
        let error = WebSearchToolRequestErrorParam::new(
            crate::types::WebSearchToolRequestErrorCode::InvalidToolInput,
        );

        let block = WebSearchToolResultBlockParam::new_with_error(error, "tool_1".to_string());

        let json = to_value(&block).unwrap();
        assert_eq!(
            json,
            json!({
                "content": {
                    "error_code": "invalid_tool_input",
                    "type": "web_search_tool_result_error"
                },
                "tool_use_id": "tool_1"
            })
        );
    }

    #[test]
    fn test_web_search_tool_result_block_param_with_cache_control() {
        let result = WebSearchResultBlock::new(
            "encrypted-content",
            "Example Title",
            "https://example.com",
        );

        let cache_control = CacheControlEphemeral::new();
        let block =
            WebSearchToolResultBlockParam::new_with_results(vec![result], "tool_1".to_string())
                .with_cache_control(cache_control);

        let json = to_value(&block).unwrap();
        assert_eq!(
            json,
            json!({
                "content": [
                    {
                        "encrypted_content": "encrypted-content",
                        "title": "Example Title",
                        "type": "web_search_result",
                        "url": "https://example.com"
                    }
                ],
                "tool_use_id": "tool_1",
                "cache_control": {
                    "type": "ephemeral"
                }
            })
        );
    }

    #[test]
    fn test_web_search_tool_result_block_param_deserialization() {
        let json = json!({
            "content": [
                {
                    "encrypted_content": "encrypted-content",
                    "title": "Example Title",
                    "type": "web_search_result",
                    "url": "https://example.com"
                }
            ],
            "tool_use_id": "tool_1",
            "type": "web_search_tool_result",
            "cache_control": {
                "type": "ephemeral"
            }
        });

        let block: WebSearchToolResultBlockParam = serde_json::from_value(json).unwrap();
        assert_eq!(block.tool_use_id, "tool_1");
        assert!(block.cache_control.is_some());

        match &block.content {
            WebSearchToolResultBlockParamContent::Results(results) => {
                assert_eq!(results.len(), 1);
                assert_eq!(results[0].title, "Example Title");
            }
            _ => panic!("Expected Results variant"),
        }
    }
}
