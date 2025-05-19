use serde::{Deserialize, Serialize};

/// Parameters for a "tool" tool choice configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolChoiceToolParam {
    /// The name of the tool to use.
    pub name: String,

    /// The type, which is always "tool".
    pub r#type: String,

    /// Whether to disable parallel tool use.
    ///
    /// Defaults to `false`. If set to `true`, the model will output exactly one tool use.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disable_parallel_tool_use: Option<bool>,
}

impl ToolChoiceToolParam {
    /// Create a new `ToolChoiceToolParam` with the specified tool name.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            r#type: "tool".to_string(),
            disable_parallel_tool_use: None,
        }
    }

    /// Set the disable_parallel_tool_use flag.
    pub fn with_disable_parallel_tool_use(mut self, disable: bool) -> Self {
        self.disable_parallel_tool_use = Some(disable);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, to_value};

    #[test]
    fn test_tool_choice_tool_param_minimal() {
        let param = ToolChoiceToolParam::new("my_tool");
        let json = to_value(&param).unwrap();

        assert_eq!(
            json,
            json!({
                "name": "my_tool",
                "type": "tool"
            })
        );
    }

    #[test]
    fn test_tool_choice_tool_param_with_disable_parallel() {
        let param = ToolChoiceToolParam::new("my_tool").with_disable_parallel_tool_use(true);

        let json = to_value(&param).unwrap();
        assert_eq!(
            json,
            json!({
                "name": "my_tool",
                "type": "tool",
                "disable_parallel_tool_use": true
            })
        );
    }

    #[test]
    fn test_tool_choice_tool_param_deserialization() {
        let json = json!({
            "name": "my_tool",
            "type": "tool",
            "disable_parallel_tool_use": true
        });

        let param: ToolChoiceToolParam = serde_json::from_value(json).unwrap();
        assert_eq!(param.r#type, "tool");
        assert_eq!(param.name, "my_tool");
        assert_eq!(param.disable_parallel_tool_use, Some(true));
    }
}
