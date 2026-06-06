use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

/// A thinking delta, representing a piece of thinking in a streaming response.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ThinkingDelta {
    /// The thinking content.
    pub thinking: String,
}

impl<'de> Deserialize<'de> for ThinkingDelta {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        Ok(Self {
            thinking: extract_thinking_text(&value).unwrap_or_default(),
        })
    }
}

fn extract_thinking_text(value: &Value) -> Option<String> {
    for field in ["thinking", "summary", "text", "content"] {
        if let Some(candidate) = value.get(field)
            && let Some(text) = extract_textish(candidate)
        {
            return Some(text);
        }
    }
    None
}

fn extract_textish(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Array(items) => {
            let text = items
                .iter()
                .filter_map(extract_textish)
                .collect::<Vec<_>>()
                .join("");
            if text.is_empty() { None } else { Some(text) }
        }
        Value::Object(_) => extract_thinking_text(value),
        _ => None,
    }
}

impl ThinkingDelta {
    /// Create a new `ThinkingDelta` with the given thinking text.
    pub fn new(thinking: String) -> Self {
        Self { thinking }
    }
}

impl std::str::FromStr for ThinkingDelta {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::new(s.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, to_value};

    #[test]
    fn thinking_delta_serialization() {
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
    fn thinking_delta_deserialization() {
        let json = json!({
            "thinking": "Let me think about this..."
        });

        let delta: ThinkingDelta = serde_json::from_value(json).unwrap();
        assert_eq!(delta.thinking, "Let me think about this...");
    }

    #[test]
    fn thinking_delta_deserialization_accepts_summary() {
        let json = json!({
            "summary": "Summarized thinking..."
        });

        let delta: ThinkingDelta = serde_json::from_value(json).unwrap();
        assert_eq!(delta.thinking, "Summarized thinking...");
    }

    #[test]
    fn thinking_delta_deserialization_accepts_text() {
        let json = json!({
            "text": "Text-shaped thinking..."
        });

        let delta: ThinkingDelta = serde_json::from_value(json).unwrap();
        assert_eq!(delta.thinking, "Text-shaped thinking...");
    }

    #[test]
    fn from_str() {
        let delta = "Let me think about this..."
            .parse::<ThinkingDelta>()
            .unwrap();
        assert_eq!(delta.thinking, "Let me think about this...");
    }
}
