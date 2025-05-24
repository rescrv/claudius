use serde::{Deserialize, Serialize};

use crate::types::WebSearchErrorCode;

/// Parameters for a web search tool request error.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WebSearchToolRequestErrorParam {
    /// The error code for the web search tool request error.
    pub error_code: WebSearchErrorCode,
}

impl WebSearchToolRequestErrorParam {
    /// Create a new `WebSearchToolRequestErrorParam` with the given error code.
    pub fn new(error_code: WebSearchErrorCode) -> Self {
        Self { error_code }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, to_value};

    #[test]
    fn test_web_search_tool_request_error_param_serialization() {
        let error =
            WebSearchToolRequestErrorParam::new(WebSearchErrorCode::InvalidToolInput);

        let json = to_value(&error).unwrap();
        assert_eq!(
            json,
            json!({
                "error_code": "invalid_tool_input"
            })
        );
    }

    #[test]
    fn test_web_search_tool_request_error_param_deserialization() {
        let json = json!({
            "error_code": "max_uses_exceeded"
        });

        let error: WebSearchToolRequestErrorParam = serde_json::from_value(json).unwrap();
        assert_eq!(
            error.error_code,
            WebSearchErrorCode::MaxUsesExceeded
        );
    }
}
