use serde::{Deserialize, Serialize};

/// Parameters for an "any" tool choice configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolChoiceAnyParam {
    /// The type, which is always "any".
    pub r#type: String,

    /// Whether to disable parallel tool use.
    ///
    /// Defaults to `false`. If set to `true`, the model will output exactly one tool use.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disable_parallel_tool_use: Option<bool>,
}

impl ToolChoiceAnyParam {
    /// Create a new `ToolChoiceAnyParam`.
    pub fn new() -> Self {
        Self {
            r#type: "any".to_string(),
            disable_parallel_tool_use: None,
        }
    }

    /// Set the disable_parallel_tool_use flag.
    pub fn with_disable_parallel_tool_use(mut self, disable: bool) -> Self {
        self.disable_parallel_tool_use = Some(disable);
        self
    }
}

impl Default for ToolChoiceAnyParam {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, to_value};

    #[test]
    fn test_tool_choice_any_param_minimal() {
        let param = ToolChoiceAnyParam::new();
        let json = to_value(&param).unwrap();

        assert_eq!(
            json,
            json!({
                "type": "any"
            })
        );
    }

    #[test]
    fn test_tool_choice_any_param_with_disable_parallel() {
        let param = ToolChoiceAnyParam::new().with_disable_parallel_tool_use(true);

        let json = to_value(&param).unwrap();
        assert_eq!(
            json,
            json!({
                "type": "any",
                "disable_parallel_tool_use": true
            })
        );
    }

    #[test]
    fn test_tool_choice_any_param_deserialization() {
        let json = json!({
            "type": "any",
            "disable_parallel_tool_use": true
        });

        let param: ToolChoiceAnyParam = serde_json::from_value(json).unwrap();
        assert_eq!(param.r#type, "any");
        assert_eq!(param.disable_parallel_tool_use, Some(true));
    }
}
