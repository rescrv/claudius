use serde::{Deserialize, Serialize};

use crate::types::{ThinkingConfigDisabledParam, ThinkingConfigEnabledParam};

/// Parameter for configuring Claude's extended thinking capabilities.
///
/// This can be either enabled (with a token budget) or disabled.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum ThinkingConfigParam {
    /// Enabled thinking configuration.
    Enabled(ThinkingConfigEnabledParam),

    /// Disabled thinking configuration.
    Disabled(ThinkingConfigDisabledParam),
}

impl ThinkingConfigParam {
    /// Create a new enabled `ThinkingConfigParam` with the given budget tokens.
    ///
    /// Budget tokens must be ≥1024.
    pub fn enabled(budget_tokens: i32) -> Self {
        Self::Enabled(ThinkingConfigEnabledParam::new(budget_tokens))
    }

    /// Create a new disabled `ThinkingConfigParam`.
    pub fn disabled() -> Self {
        Self::Disabled(ThinkingConfigDisabledParam::new())
    }
}

impl Default for ThinkingConfigParam {
    fn default() -> Self {
        Self::disabled()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, to_value};

    #[test]
    fn test_thinking_config_param_enabled_serialization() {
        let param = ThinkingConfigParam::enabled(2048);
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
    fn test_thinking_config_param_disabled_serialization() {
        let param = ThinkingConfigParam::disabled();
        let json = to_value(&param).unwrap();

        assert_eq!(
            json,
            json!({
                "type": "disabled"
            })
        );
    }

    #[test]
    fn test_thinking_config_param_enabled_deserialization() {
        let json = json!({
            "budget_tokens": 2048,
            "type": "enabled"
        });

        let param: ThinkingConfigParam = serde_json::from_value(json).unwrap();
        match param {
            ThinkingConfigParam::Enabled(enabled) => {
                assert_eq!(enabled.budget_tokens, 2048);
            }
            _ => panic!("Expected Enabled variant"),
        }
    }

    #[test]
    fn test_thinking_config_param_disabled_deserialization() {
        let json = json!({
            "type": "disabled"
        });

        let param: ThinkingConfigParam = serde_json::from_value(json).unwrap();
        match param {
            ThinkingConfigParam::Disabled(_) => {}
            _ => panic!("Expected Disabled variant"),
        }
    }
}
