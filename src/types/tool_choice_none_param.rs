use serde::{Deserialize, Serialize};

/// Parameters for a "none" tool choice configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolChoiceNoneParam {
    /// The type, which is always "none".
    pub r#type: String,
}

impl ToolChoiceNoneParam {
    /// Create a new `ToolChoiceNoneParam`.
    pub fn new() -> Self {
        Self {
            r#type: "none".to_string(),
        }
    }
}

impl Default for ToolChoiceNoneParam {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, to_value};

    #[test]
    fn test_tool_choice_none_param_serialization() {
        let param = ToolChoiceNoneParam::new();
        let json = to_value(&param).unwrap();
        
        assert_eq!(
            json,
            json!({
                "type": "none"
            })
        );
    }
    
    #[test]
    fn test_tool_choice_none_param_deserialization() {
        let json = json!({
            "type": "none"
        });
        
        let param: ToolChoiceNoneParam = serde_json::from_value(json).unwrap();
        assert_eq!(param.r#type, "none");
    }
}