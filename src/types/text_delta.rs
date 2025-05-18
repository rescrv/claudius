use serde::{Deserialize, Serialize};

/// A text delta, representing a piece of text in a streaming response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextDelta {
    /// The text content.
    pub text: String,
    
    /// The type, which is always "text_delta".
    pub r#type: String,
}

impl TextDelta {
    /// Create a new `TextDelta` with the given text.
    pub fn new(text: String) -> Self {
        Self {
            text,
            r#type: "text_delta".to_string(),
        }
    }
    
    /// Create a new `TextDelta` from a string reference.
    pub fn from_str(text: &str) -> Self {
        Self::new(text.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, to_value};

    #[test]
    fn test_text_delta_serialization() {
        let delta = TextDelta::new("Hello world".to_string());
        let json = to_value(&delta).unwrap();
        
        assert_eq!(
            json,
            json!({
                "text": "Hello world",
                "type": "text_delta"
            })
        );
    }

    #[test]
    fn test_text_delta_deserialization() {
        let json = json!({
            "text": "Hello world",
            "type": "text_delta"
        });
        
        let delta: TextDelta = serde_json::from_value(json).unwrap();
        assert_eq!(delta.text, "Hello world");
        assert_eq!(delta.r#type, "text_delta");
    }
}