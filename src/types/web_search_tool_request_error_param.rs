use serde::{Deserialize, Serialize};

/// The error code for a web search tool request error.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebSearchToolRequestErrorCode {
    /// The tool input was invalid.
    InvalidToolInput,

    /// The web search service is unavailable.
    Unavailable,

    /// The maximum number of uses for the web search tool has been exceeded.
    MaxUsesExceeded,

    /// Too many requests have been made to the web search service.
    TooManyRequests,

    /// The query is too long.
    QueryTooLong,
}

/// Parameters for a web search tool request error.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WebSearchToolRequestErrorParam {
    /// The error code for the web search tool request error.
    pub error_code: WebSearchToolRequestErrorCode,

    /// The type, which is always "web_search_tool_result_error".
    pub r#type: String,
}

impl WebSearchToolRequestErrorParam {
    /// Create a new `WebSearchToolRequestErrorParam` with the given error code.
    pub fn new(error_code: WebSearchToolRequestErrorCode) -> Self {
        Self {
            error_code,
            r#type: "web_search_tool_result_error".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, to_value};

    #[test]
    fn test_web_search_tool_request_error_param_serialization() {
        let error =
            WebSearchToolRequestErrorParam::new(WebSearchToolRequestErrorCode::InvalidToolInput);

        let json = to_value(&error).unwrap();
        assert_eq!(
            json,
            json!({
                "error_code": "invalid_tool_input",
                "type": "web_search_tool_result_error"
            })
        );
    }

    #[test]
    fn test_web_search_tool_request_error_param_deserialization() {
        let json = json!({
            "error_code": "max_uses_exceeded",
            "type": "web_search_tool_result_error"
        });

        let error: WebSearchToolRequestErrorParam = serde_json::from_value(json).unwrap();
        assert_eq!(
            error.error_code,
            WebSearchToolRequestErrorCode::MaxUsesExceeded
        );
        assert_eq!(error.r#type, "web_search_tool_result_error");
    }
}
