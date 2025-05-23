use serde::{Deserialize, Serialize};

use crate::types::{WebSearchResultBlock, WebSearchToolRequestErrorParam};

/// Content for a web search tool result block, which can be either an array of
/// web search result blocks or a web search tool request error.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum WebSearchToolResultBlockParamContent {
    /// An array of web search result blocks.
    Results(Vec<WebSearchResultBlock>),

    /// A web search tool request error.
    Error(WebSearchToolRequestErrorParam),
}

impl WebSearchToolResultBlockParamContent {
    /// Create a new `WebSearchToolResultBlockParamContent` with the given results.
    pub fn new_with_results(results: Vec<WebSearchResultBlock>) -> Self {
        Self::Results(results)
    }

    /// Create a new `WebSearchToolResultBlockParamContent` with the given error.
    pub fn new_with_error(error: WebSearchToolRequestErrorParam) -> Self {
        Self::Error(error)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, to_value};

    #[test]
    fn test_web_search_tool_result_block_param_content_results() {
        let result =
            WebSearchResultBlock::new("encrypted-content", "Example Title", "https://example.com");

        let content = WebSearchToolResultBlockParamContent::new_with_results(vec![result]);
        let json = to_value(&content).unwrap();

        assert_eq!(
            json,
            json!([
                {
                    "encrypted_content": "encrypted-content",
                    "title": "Example Title",
                    "url": "https://example.com"
                }
            ])
        );
    }

    #[test]
    fn test_web_search_tool_result_block_param_content_error() {
        let error = WebSearchToolRequestErrorParam::new(
            crate::types::WebSearchToolRequestErrorCode::InvalidToolInput,
        );

        let content = WebSearchToolResultBlockParamContent::new_with_error(error);
        let json = to_value(&content).unwrap();

        assert_eq!(
            json,
            json!({
                "error_code": "invalid_tool_input"
            })
        );
    }

    #[test]
    fn test_web_search_tool_result_block_param_content_deserialization_results() {
        let json = json!([
            {
                "encrypted_content": "encrypted-content",
                "title": "Example Title",
                "url": "https://example.com"
            }
        ]);

        let content: WebSearchToolResultBlockParamContent = serde_json::from_value(json).unwrap();
        match content {
            WebSearchToolResultBlockParamContent::Results(results) => {
                assert_eq!(results.len(), 1);
                assert_eq!(results[0].title, "Example Title");
            }
            _ => panic!("Expected Results variant"),
        }
    }

    #[test]
    fn test_web_search_tool_result_block_param_content_deserialization_error() {
        let json = json!({
            "error_code": "invalid_tool_input"
        });

        let content: WebSearchToolResultBlockParamContent = serde_json::from_value(json).unwrap();
        match content {
            WebSearchToolResultBlockParamContent::Error(error) => {
                assert_eq!(
                    error.error_code,
                    crate::types::WebSearchToolRequestErrorCode::InvalidToolInput
                );
            }
            _ => panic!("Expected Error variant"),
        }
    }
}
