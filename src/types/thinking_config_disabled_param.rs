use serde::{Deserialize, Serialize};

/// Parameters for a disabled thinking configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ThinkingConfigDisabledParam {
    /// The type, which is always "disabled".
    pub r#type: String,
}

impl ThinkingConfigDisabledParam {
    /// Create a new `ThinkingConfigDisabledParam`.
    pub fn new() -> Self {
        Self {
            r#type: "disabled".to_string(),
        }
    }
}

impl Default for ThinkingConfigDisabledParam {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, to_value};

    #[test]
    fn test_thinking_config_disabled_param_serialization() {
        let param = ThinkingConfigDisabledParam::new();
        let json = to_value(&param).unwrap();
        
        assert_eq!(
            json,
            json!({
                "type": "disabled"
            })
        );
    }

    #[test]
    fn test_thinking_config_disabled_param_deserialization() {
        let json = json!({
            "type": "disabled"
        });
        
        let param: ThinkingConfigDisabledParam = serde_json::from_value(json).unwrap();
        assert_eq!(param.r#type, "disabled");
    }
}