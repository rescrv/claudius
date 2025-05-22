use serde::{Deserialize, Serialize};

/// A thinking delta, representing a piece of thinking in a streaming response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThinkingDelta {
    /// The thinking content.
    pub thinking: String,
}

impl ThinkingDelta {
    /// Create a new `ThinkingDelta` with the given thinking text.
    pub fn new(thinking: String) -> Self {
        Self { thinking }
    }

    /// Create a new `ThinkingDelta` from a string reference.
    // TODO(claude): no from_string_ref
    pub fn from_string_ref(thinking: &str) -> Self {
        Self::new(thinking.to_string())
    }
}

impl std::str::FromStr for ThinkingDelta {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::from_string_ref(s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, to_value};

    #[test]
    fn test_thinking_delta_serialization() {
        let delta = ThinkingDelta::new("Let me think about this...".to_string());
        let json = to_value(&delta).unwrap();

        assert_eq!(
            json,
            json!({
                "thinking": "Let me think about this..."
            })
        );
    }

    #[test]
    fn test_thinking_delta_deserialization() {
        let json = json!({
            "thinking": "Let me think about this..."
        });

        let delta: ThinkingDelta = serde_json::from_value(json).unwrap();
        assert_eq!(delta.thinking, "Let me think about this...");
    }

    #[test]
    fn test_from_str() {
        let delta = "Let me think about this..."
            .parse::<ThinkingDelta>()
            .unwrap();
        assert_eq!(delta.thinking, "Let me think about this...");
    }
}
