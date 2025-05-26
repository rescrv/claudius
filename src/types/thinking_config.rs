use serde::{Deserialize, Serialize};

/// Configuration for enabling Claude's extended thinking capabilities.
///
/// This can be either enabled (with a token budget) or disabled.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum ThinkingConfig {
    /// Disabled thinking configuration.
    #[serde(rename = "disabled")]
    Disabled,

    /// Enabled thinking configuration with a token budget.
    #[serde(rename = "enabled")]
    Enabled {
        /// Determines how many tokens Claude can use for its internal reasoning process.
        ///
        /// Larger budgets can enable more thorough analysis for complex problems, improving
        /// response quality.
        ///
        /// Must be ≥1024 and less than `max_tokens`.
        #[serde(rename = "budget_tokens")]
        budget_tokens: i32,
    },
}

impl ThinkingConfig {
    /// Create a new enabled thinking configuration with the given budget tokens.
    ///
    /// Budget tokens must be ≥1024.
    pub fn enabled(budget_tokens: i32) -> Self {
        Self::Enabled { budget_tokens }
    }

    /// Create a new disabled thinking configuration.
    pub fn disabled() -> Self {
        Self::Disabled
    }
}

impl Default for ThinkingConfig {
    fn default() -> Self {
        Self::disabled()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, to_value};

    #[test]
    fn thinking_config_enabled_serialization() {
        let config = ThinkingConfig::enabled(2048);
        let json = to_value(&config).unwrap();

        assert_eq!(
            json,
            json!({
                "type": "enabled",
                "budget_tokens": 2048
            })
        );
    }

    #[test]
    fn thinking_config_disabled_serialization() {
        let config = ThinkingConfig::disabled();
        let json = to_value(&config).unwrap();

        assert_eq!(
            json,
            json!({
                "type": "disabled"
            })
        );
    }

    #[test]
    fn thinking_config_enabled_deserialization() {
        let json = json!({
            "type": "enabled",
            "budget_tokens": 2048
        });

        let config: ThinkingConfig = serde_json::from_value(json).unwrap();
        match config {
            ThinkingConfig::Enabled { budget_tokens } => {
                assert_eq!(budget_tokens, 2048);
            }
            _ => panic!("Expected Enabled variant"),
        }
    }

    #[test]
    fn thinking_config_disabled_deserialization() {
        let json = json!({
            "type": "disabled"
        });

        let config: ThinkingConfig = serde_json::from_value(json).unwrap();
        match config {
            ThinkingConfig::Disabled => {}
            _ => panic!("Expected Disabled variant"),
        }
    }
}
