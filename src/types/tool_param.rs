// Our tool_param.rs is just a re-export file

/// A tool parameter that can be used to specify a custom tool.
///
/// This is a re-export of the `ToolParam` defined in `message_count_tokens_tool_param.rs`.
/// It provides the same functionality but is available at a more logical location
/// in the module hierarchy.
pub use crate::types::message_count_tokens_tool_param::ToolParam;

/// A tool input schema.
///
/// This is a re-export of the `InputSchema` defined in `message_count_tokens_tool_param.rs`.
pub use crate::types::message_count_tokens_tool_param::InputSchema;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::CacheControlEphemeral;
    use serde_json::{json, to_value};
    use std::collections::HashMap;

    #[test]
    fn test_tool_param_complete() {
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

        let cache_control = CacheControlEphemeral::new();

        let tool = ToolParam::new("search".to_string(), input_schema)
            .with_description("Search for information".to_string())
            .with_cache_control(cache_control)
            .with_custom_type();

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
                "description": "Search for information",
                "type": "custom"
            })
        );
    }
}
