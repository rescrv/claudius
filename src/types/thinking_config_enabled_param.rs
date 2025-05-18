use serde::{Deserialize, Serialize};

/// Parameters for an enabled thinking configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ThinkingConfigEnabledParam {
    /// Determines how many tokens Claude can use for its internal reasoning process.
    ///
    /// Larger budgets can enable more thorough analysis for complex problems, improving
    /// response quality.
    ///
    /// Must be ≥1024 and less than `max_tokens`.
    #[serde(rename = "budget_tokens")]
    pub budget_tokens: i32,
    
    /// The type, which is always "enabled".
    pub r#type: String,
}

impl ThinkingConfigEnabledParam {
    /// Create a new `ThinkingConfigEnabledParam` with the given budget tokens.
    ///
    /// Budget tokens must be ≥1024.
    pub fn new(budget_tokens: i32) -> Self {
        Self {
            budget_tokens,
            r#type: "enabled".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, to_value};

    #[test]
    fn test_thinking_config_enabled_param_serialization() {
        let param = ThinkingConfigEnabledParam::new(2048);
        let json = to_value(&param).unwrap();
        
        assert_eq!(
            json,
            json!({
                "budget_tokens": 2048,
                "type": "enabled"
            })
        );
    }

    #[test]
    fn test_thinking_config_enabled_param_deserialization() {
        let json = json!({
            "budget_tokens": 2048,
            "type": "enabled"
        });
        
        let param: ThinkingConfigEnabledParam = serde_json::from_value(json).unwrap();
        assert_eq!(param.budget_tokens, 2048);
        assert_eq!(param.r#type, "enabled");
    }
}