use serde::de;
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Controls how extended thinking content is returned in API responses.
#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ThinkingDisplay {
    /// Return summarized thinking text in `thinking` blocks.
    Summarized,
    /// Return `thinking` blocks with empty thinking text and only the signature.
    Omitted,
}

/// Configuration for enabling Claude's extended thinking capabilities.
///
/// This can be disabled, adaptive, or enabled with a token budget. Display-aware
/// variants add the API's `display` field without changing the JSON `type`.
#[derive(Debug, Copy, Clone, PartialEq)]
pub enum ThinkingConfig {
    /// Disabled thinking configuration.
    Disabled,

    /// Adaptive thinking configuration.
    ///
    /// When enabled, Claude automatically determines the appropriate amount of thinking
    /// based on the task complexity. Use with the effort parameter in `OutputConfig`
    /// to control thinking depth.
    Adaptive,

    /// Adaptive thinking configuration with explicit response display behavior.
    AdaptiveWithDisplay {
        /// Controls how thinking content is returned.
        display: ThinkingDisplay,
    },

    /// Enabled thinking configuration with a token budget.
    Enabled {
        /// Determines how many tokens Claude can use for its internal reasoning process.
        ///
        /// Larger budgets can enable more thorough analysis for complex problems, improving
        /// response quality.
        ///
        /// Must be ≥1024 and less than `max_tokens`.
        budget_tokens: u32,
    },

    /// Enabled thinking configuration with a token budget and explicit response display behavior.
    EnabledWithDisplay {
        /// Determines how many tokens Claude can use for its internal reasoning process.
        ///
        /// Must be ≥1024 and less than `max_tokens`.
        budget_tokens: u32,
        /// Controls how thinking content is returned.
        display: ThinkingDisplay,
    },
}

impl ThinkingConfig {
    /// Returns the number of budget tokens configured for thinking.
    ///
    /// Returns 0 if thinking is disabled or adaptive.
    pub fn num_tokens(&self) -> u32 {
        match self {
            ThinkingConfig::Disabled
            | ThinkingConfig::Adaptive
            | ThinkingConfig::AdaptiveWithDisplay { .. } => 0,
            ThinkingConfig::Enabled { budget_tokens }
            | ThinkingConfig::EnabledWithDisplay { budget_tokens, .. } => *budget_tokens,
        }
    }

    /// Create a new enabled thinking configuration with the given budget tokens.
    ///
    /// Budget tokens must be ≥1024.
    pub fn enabled(budget_tokens: u32) -> Self {
        Self::Enabled { budget_tokens }
    }

    /// Create a new enabled thinking configuration that requests summarized thinking.
    ///
    /// Budget tokens must be ≥1024.
    pub fn enabled_summarized(budget_tokens: u32) -> Self {
        Self::enabled(budget_tokens).with_display(ThinkingDisplay::Summarized)
    }

    /// Create a new enabled thinking configuration that omits thinking text.
    ///
    /// Budget tokens must be ≥1024.
    pub fn enabled_omitted(budget_tokens: u32) -> Self {
        Self::enabled(budget_tokens).with_display(ThinkingDisplay::Omitted)
    }

    /// Create a new disabled thinking configuration.
    pub fn disabled() -> Self {
        Self::Disabled
    }

    /// Create a new adaptive thinking configuration.
    pub fn adaptive() -> Self {
        Self::Adaptive
    }

    /// Create a new adaptive thinking configuration that requests summarized thinking.
    pub fn adaptive_summarized() -> Self {
        Self::adaptive().with_display(ThinkingDisplay::Summarized)
    }

    /// Create a new adaptive thinking configuration that omits thinking text.
    pub fn adaptive_omitted() -> Self {
        Self::adaptive().with_display(ThinkingDisplay::Omitted)
    }

    /// Set how thinking content should be returned.
    ///
    /// `display` is only valid for enabled or adaptive thinking. Calling this on
    /// disabled thinking leaves it unchanged.
    pub fn with_display(self, display: ThinkingDisplay) -> Self {
        match self {
            ThinkingConfig::Disabled => ThinkingConfig::Disabled,
            ThinkingConfig::Adaptive | ThinkingConfig::AdaptiveWithDisplay { .. } => {
                ThinkingConfig::AdaptiveWithDisplay { display }
            }
            ThinkingConfig::Enabled { budget_tokens }
            | ThinkingConfig::EnabledWithDisplay { budget_tokens, .. } => {
                ThinkingConfig::EnabledWithDisplay {
                    budget_tokens,
                    display,
                }
            }
        }
    }

    /// Returns the configured thinking display mode, if any.
    pub fn display(&self) -> Option<ThinkingDisplay> {
        match self {
            ThinkingConfig::AdaptiveWithDisplay { display }
            | ThinkingConfig::EnabledWithDisplay { display, .. } => Some(*display),
            ThinkingConfig::Disabled
            | ThinkingConfig::Adaptive
            | ThinkingConfig::Enabled { .. } => None,
        }
    }
}

impl Serialize for ThinkingConfig {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            ThinkingConfig::Disabled => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("type", "disabled")?;
                map.end()
            }
            ThinkingConfig::Adaptive => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("type", "adaptive")?;
                map.end()
            }
            ThinkingConfig::AdaptiveWithDisplay { display } => {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("type", "adaptive")?;
                map.serialize_entry("display", display)?;
                map.end()
            }
            ThinkingConfig::Enabled { budget_tokens } => {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("type", "enabled")?;
                map.serialize_entry("budget_tokens", budget_tokens)?;
                map.end()
            }
            ThinkingConfig::EnabledWithDisplay {
                budget_tokens,
                display,
            } => {
                let mut map = serializer.serialize_map(Some(3))?;
                map.serialize_entry("type", "enabled")?;
                map.serialize_entry("budget_tokens", budget_tokens)?;
                map.serialize_entry("display", display)?;
                map.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for ThinkingConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawThinkingConfig {
            #[serde(rename = "type")]
            kind: String,
            budget_tokens: Option<u32>,
            display: Option<ThinkingDisplay>,
        }

        let raw = RawThinkingConfig::deserialize(deserializer)?;
        match raw.kind.as_str() {
            "disabled" => Ok(ThinkingConfig::Disabled),
            "adaptive" => Ok(match raw.display {
                Some(display) => ThinkingConfig::AdaptiveWithDisplay { display },
                None => ThinkingConfig::Adaptive,
            }),
            "enabled" => {
                let budget_tokens = raw
                    .budget_tokens
                    .ok_or_else(|| de::Error::missing_field("budget_tokens"))?;
                Ok(match raw.display {
                    Some(display) => ThinkingConfig::EnabledWithDisplay {
                        budget_tokens,
                        display,
                    },
                    None => ThinkingConfig::Enabled { budget_tokens },
                })
            }
            other => Err(de::Error::unknown_variant(
                other,
                &["disabled", "adaptive", "enabled"],
            )),
        }
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
        let json = to_value(config).unwrap();

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
        let json = to_value(config).unwrap();

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

    #[test]
    fn thinking_config_adaptive_serialization() {
        let config = ThinkingConfig::adaptive();
        let json = to_value(config).unwrap();

        assert_eq!(
            json,
            json!({
                "type": "adaptive"
            })
        );
    }

    #[test]
    fn thinking_config_adaptive_deserialization() {
        let json = json!({
            "type": "adaptive"
        });

        let config: ThinkingConfig = serde_json::from_value(json).unwrap();
        match config {
            ThinkingConfig::Adaptive => {}
            _ => panic!("Expected Adaptive variant"),
        }
    }

    #[test]
    fn thinking_display_serialization() {
        assert_eq!(
            to_value(ThinkingDisplay::Summarized).unwrap(),
            json!("summarized")
        );
        assert_eq!(
            to_value(ThinkingDisplay::Omitted).unwrap(),
            json!("omitted")
        );
    }

    #[test]
    fn thinking_config_enabled_summarized_serialization() {
        let config = ThinkingConfig::enabled_summarized(2048);
        let json = to_value(config).unwrap();

        assert_eq!(
            json,
            json!({
                "type": "enabled",
                "budget_tokens": 2048,
                "display": "summarized"
            })
        );
    }

    #[test]
    fn thinking_config_adaptive_summarized_serialization() {
        let config = ThinkingConfig::adaptive_summarized();
        let json = to_value(config).unwrap();

        assert_eq!(
            json,
            json!({
                "type": "adaptive",
                "display": "summarized"
            })
        );
    }

    #[test]
    fn thinking_config_display_deserialization() {
        let json = json!({
            "type": "enabled",
            "budget_tokens": 2048,
            "display": "omitted"
        });

        let config: ThinkingConfig = serde_json::from_value(json).unwrap();
        assert_eq!(config.display(), Some(ThinkingDisplay::Omitted));
        match config {
            ThinkingConfig::EnabledWithDisplay {
                budget_tokens,
                display,
            } => {
                assert_eq!(budget_tokens, 2048);
                assert_eq!(display, ThinkingDisplay::Omitted);
            }
            _ => panic!("Expected EnabledWithDisplay variant"),
        }
    }
}
