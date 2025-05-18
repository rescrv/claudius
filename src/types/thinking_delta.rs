use serde::{Deserialize, Serialize};

/// A thinking delta, representing a piece of thinking in a streaming response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ThinkingDelta {
    /// The thinking content.
    pub thinking: String,
    
    /// The type, which is always "thinking_delta".
    pub r#type: String,
}

impl ThinkingDelta {
    /// Create a new `ThinkingDelta` with the given thinking text.
    pub fn new(thinking: String) -> Self {
        Self {
            thinking,
            r#type: "thinking_delta".to_string(),
        }
    }
    
    /// Create a new `ThinkingDelta` from a string reference.
    pub fn from_str(thinking: &str) -> Self {
        Self::new(thinking.to_string())
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
                "thinking": "Let me think about this...",
                "type": "thinking_delta"
            })
        );
    }

    #[test]
    fn test_thinking_delta_deserialization() {
        let json = json!({
            "thinking": "Let me think about this...",
            "type": "thinking_delta"
        });
        
        let delta: ThinkingDelta = serde_json::from_value(json).unwrap();
        assert_eq!(delta.thinking, "Let me think about this...");
        assert_eq!(delta.r#type, "thinking_delta");
    }
}