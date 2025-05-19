use serde::{Deserialize, Serialize};

use crate::types::WebSearchToolResultBlockContent;

/// A block containing the results of a web search tool operation.
///
/// WebSearchToolResultBlock contains either a list of search results or an error.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebSearchToolResultBlock {
    /// The content of the web search tool result.
    pub content: WebSearchToolResultBlockContent,

    /// The ID of the tool use that this result is for.
    pub tool_use_id: String,

    /// The type of content block, always "web_search_tool_result" for this struct.
    #[serde(default = "default_type")]
    pub r#type: String,
}

fn default_type() -> String {
    "web_search_tool_result".to_string()
}

impl WebSearchToolResultBlock {
    /// Creates a new WebSearchToolResultBlock.
    pub fn new<S: Into<String>>(content: WebSearchToolResultBlockContent, tool_use_id: S) -> Self {
        Self {
            content,
            tool_use_id: tool_use_id.into(),
            r#type: default_type(),
        }
    }

    /// Returns true if the web search result contains successful results.
    pub fn has_results(&self) -> bool {
        self.content.is_results()
    }

    /// Returns true if the web search result contains an error.
    pub fn has_error(&self) -> bool {
        self.content.is_error()
    }

    /// Returns the number of search results, or 0 if this is an error result.
    pub fn result_count(&self) -> usize {
        self.content.result_count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{WebSearchErrorCode, WebSearchResultBlock, WebSearchToolResultError};

    #[test]
    fn test_results_serialization() {
        let results = vec![WebSearchResultBlock {
            encrypted_content: "encrypted-data-1".to_string(),
            page_age: Some("2 days ago".to_string()),
            title: "Example Page 1".to_string(),
            r#type: "web_search_result".to_string(),
            url: "https://example.com/page1".to_string(),
        }];

        let content = WebSearchToolResultBlockContent::with_results(results);
        let block = WebSearchToolResultBlock::new(content, "tool-123");

        let json = serde_json::to_string(&block).unwrap();
        let expected = r#"{"content":[{"encrypted_content":"encrypted-data-1","page_age":"2 days ago","title":"Example Page 1","type":"web_search_result","url":"https://example.com/page1"}],"tool_use_id":"tool-123","type":"web_search_tool_result"}"#;

        assert_eq!(json, expected);
    }

    #[test]
    fn test_error_serialization() {
        let error = WebSearchToolResultError {
            error_code: WebSearchErrorCode::InvalidToolInput,
            r#type: "web_search_tool_result_error".to_string(),
        };

        let content = WebSearchToolResultBlockContent::with_error(error);
        let block = WebSearchToolResultBlock::new(content, "tool-123");

        let json = serde_json::to_string(&block).unwrap();
        let expected = r#"{"content":{"error_code":"invalid_tool_input","type":"web_search_tool_result_error"},"tool_use_id":"tool-123","type":"web_search_tool_result"}"#;

        assert_eq!(json, expected);
    }

    #[test]
    fn test_deserialization() {
        let json = r#"{"content":[{"encrypted_content":"encrypted-data-1","page_age":"2 days ago","title":"Example Page 1","type":"web_search_result","url":"https://example.com/page1"}],"tool_use_id":"tool-123","type":"web_search_tool_result"}"#;
        let block: WebSearchToolResultBlock = serde_json::from_str(json).unwrap();

        assert_eq!(block.tool_use_id, "tool-123");
        assert_eq!(block.r#type, "web_search_tool_result");
        assert!(block.has_results());
        assert!(!block.has_error());
        assert_eq!(block.result_count(), 1);
    }
}
