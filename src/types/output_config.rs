use serde::{Deserialize, Serialize};

use crate::types::OutputFormat;

/// Effort level for controlling thinking depth with adaptive thinking.
///
/// Used with `ThinkingConfig::Adaptive` to control how much thinking
/// Claude applies to a task.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effort {
    /// Low effort - minimal thinking for simple tasks
    Low,
    /// Medium effort - moderate thinking (default)
    Medium,
    /// High effort - thorough thinking for complex tasks
    High,
}

/// Output configuration for API requests.
///
/// This is the newer configuration format that replaces the deprecated `output_format` field.
/// It supports both structured output format and the effort parameter for adaptive thinking.
///
/// # Example
///
/// ```
/// use claudius::{OutputConfig, Effort};
///
/// // Control thinking effort
/// let config = OutputConfig::new().with_effort(Effort::High);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct OutputConfig {
    /// Output format configuration for structured outputs.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<OutputFormat>,

    /// Effort level for controlling thinking depth.
    ///
    /// Used with `ThinkingConfig::Adaptive` to control how much thinking
    /// Claude applies to a task.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub effort: Option<Effort>,
}

impl OutputConfig {
    /// Create a new empty output configuration.
    pub fn new() -> Self {
        Self {
            format: None,
            effort: None,
        }
    }

    /// Set the output format.
    pub fn with_format(mut self, format: OutputFormat) -> Self {
        self.format = Some(format);
        self
    }

    /// Set the effort level.
    pub fn with_effort(mut self, effort: Effort) -> Self {
        self.effort = Some(effort);
        self
    }
}

impl Default for OutputConfig {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, to_value};

    #[test]
    fn effort_serialization() {
        assert_eq!(to_value(Effort::Low).unwrap(), json!("low"));
        assert_eq!(to_value(Effort::Medium).unwrap(), json!("medium"));
        assert_eq!(to_value(Effort::High).unwrap(), json!("high"));
    }

    #[test]
    fn effort_deserialization() {
        let low: Effort = serde_json::from_value(json!("low")).unwrap();
        assert_eq!(low, Effort::Low);

        let medium: Effort = serde_json::from_value(json!("medium")).unwrap();
        assert_eq!(medium, Effort::Medium);

        let high: Effort = serde_json::from_value(json!("high")).unwrap();
        assert_eq!(high, Effort::High);
    }

    #[test]
    fn output_config_with_effort_only() {
        let config = OutputConfig::new().with_effort(Effort::High);
        let json = to_value(&config).unwrap();

        assert_eq!(
            json,
            json!({
                "effort": "high"
            })
        );
    }

    #[test]
    fn output_config_with_format_only() {
        let config = OutputConfig::new().with_format(OutputFormat::json_schema(json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" }
            },
            "required": ["name"],
            "additionalProperties": false
        })));

        let json = to_value(&config).unwrap();

        assert_eq!(
            json,
            json!({
                "format": {
                    "type": "json_schema",
                    "schema": {
                        "type": "object",
                        "properties": {
                            "name": { "type": "string" }
                        },
                        "required": ["name"],
                        "additionalProperties": false
                    }
                }
            })
        );
    }

    #[test]
    fn output_config_with_both_format_and_effort() {
        let config = OutputConfig::new()
            .with_format(OutputFormat::json_schema(json!({
                "type": "object",
                "properties": {
                    "answer": { "type": "boolean" }
                },
                "required": ["answer"],
                "additionalProperties": false
            })))
            .with_effort(Effort::Medium);

        let json = to_value(&config).unwrap();

        assert_eq!(
            json,
            json!({
                "format": {
                    "type": "json_schema",
                    "schema": {
                        "type": "object",
                        "properties": {
                            "answer": { "type": "boolean" }
                        },
                        "required": ["answer"],
                        "additionalProperties": false
                    }
                },
                "effort": "medium"
            })
        );
    }

    #[test]
    fn output_config_empty_skips_none_fields() {
        let config = OutputConfig::new();
        let json = to_value(&config).unwrap();

        // Both fields are None, so they should be omitted
        assert_eq!(json, json!({}));
        assert!(!json.as_object().unwrap().contains_key("format"));
        assert!(!json.as_object().unwrap().contains_key("effort"));
    }

    #[test]
    fn output_config_deserialization_effort_only() {
        let json = json!({
            "effort": "low"
        });

        let config: OutputConfig = serde_json::from_value(json).unwrap();
        assert_eq!(config.effort, Some(Effort::Low));
        assert_eq!(config.format, None);
    }

    #[test]
    fn output_config_deserialization_format_only() {
        let json = json!({
            "format": {
                "type": "json_schema",
                "schema": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string" }
                    },
                    "required": ["name"],
                    "additionalProperties": false
                }
            }
        });

        let config: OutputConfig = serde_json::from_value(json).unwrap();
        assert!(config.format.is_some());
        assert_eq!(config.effort, None);
    }

    #[test]
    fn output_config_deserialization_both() {
        let json = json!({
            "format": {
                "type": "json_schema",
                "schema": {
                    "type": "object",
                    "properties": {
                        "result": { "type": "string" }
                    },
                    "required": ["result"],
                    "additionalProperties": false
                }
            },
            "effort": "high"
        });

        let config: OutputConfig = serde_json::from_value(json).unwrap();
        assert!(config.format.is_some());
        assert_eq!(config.effort, Some(Effort::High));
    }

    #[test]
    fn output_config_deserialization_empty() {
        let json = json!({});

        let config: OutputConfig = serde_json::from_value(json).unwrap();
        assert_eq!(config.format, None);
        assert_eq!(config.effort, None);
    }

    #[test]
    fn output_config_default() {
        let config = OutputConfig::default();
        assert_eq!(config.format, None);
        assert_eq!(config.effort, None);
    }
}
