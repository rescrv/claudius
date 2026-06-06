use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

/// A block containing model thinking details.
///
/// ThinkingBlocks contain internal reasoning or deliberation from the model.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ThinkingBlock {
    /// A signature for the thinking (typically a hash).
    pub signature: String,

    /// The thinking content.
    pub thinking: String,
}

impl<'de> Deserialize<'de> for ThinkingBlock {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let signature = value
            .get("signature")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let thinking = extract_thinking_text(&value).unwrap_or_default();

        Ok(Self {
            signature,
            thinking,
        })
    }
}

fn extract_thinking_text(value: &Value) -> Option<String> {
    for field in ["thinking", "summary", "text", "content", "summaries"] {
        if let Some(candidate) = value.get(field) {
            if let Some(text) = extract_textish(candidate) {
                return Some(text);
            }
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
                .join("\n");
            if text.is_empty() { None } else { Some(text) }
        }
        Value::Object(_) => extract_thinking_text(value),
        _ => None,
    }
}

impl ThinkingBlock {
    /// Creates a new ThinkingBlock with the specified thinking content and signature.
    pub fn new<S1: Into<String>, S2: Into<String>>(thinking: S1, signature: S2) -> Self {
        Self {
            thinking: thinking.into(),
            signature: signature.into(),
        }
    }

    /// Create a new `ThinkingBlock` from string references.
    pub fn from_str(signature: &str, thinking: &str) -> Self {
        Self::new(thinking, signature)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, to_value};

    #[test]
    fn thinking_block_serialization() {
        let thinking_block = ThinkingBlock::new(
            "Let me think through this problem step by step...",
            "abc123signature",
        );

        let json = serde_json::to_string(&thinking_block).unwrap();
        let expected = r#"{"signature":"abc123signature","thinking":"Let me think through this problem step by step..."}"#;

        assert_eq!(json, expected);
    }

    #[test]
    fn deserialization() {
        let json = r#"{"signature":"abc123signature","thinking":"Let me think through this problem step by step...","type":"thinking"}"#;
        let thinking_block: ThinkingBlock = serde_json::from_str(json).unwrap();

        assert_eq!(thinking_block.signature, "abc123signature");
        assert_eq!(
            thinking_block.thinking,
            "Let me think through this problem step by step..."
        );
    }

    #[test]
    fn deserialization_allows_missing_signature_on_stream_start() {
        let json = r#"{"thinking":"","type":"thinking"}"#;
        let thinking_block: ThinkingBlock = serde_json::from_str(json).unwrap();

        assert_eq!(thinking_block.signature, "");
        assert_eq!(thinking_block.thinking, "");
    }

    #[test]
    fn deserialization_accepts_summary_text() {
        let json = r#"{"summary":"Condensed thinking summary.","type":"thinking"}"#;
        let thinking_block: ThinkingBlock = serde_json::from_str(json).unwrap();

        assert_eq!(thinking_block.signature, "");
        assert_eq!(thinking_block.thinking, "Condensed thinking summary.");
    }

    #[test]
    fn thinking_block_with_string_references() {
        let block = ThinkingBlock::new("Let me think about this...", "Signature");
        let json = to_value(&block).unwrap();

        assert_eq!(
            json,
            json!({
                "signature": "Signature",
                "thinking": "Let me think about this..."
            })
        );
    }

    #[test]
    fn thinking_block_from_str() {
        let block = ThinkingBlock::from_str("Signature", "Let me think about this...");
        let json = to_value(&block).unwrap();

        assert_eq!(
            json,
            json!({
                "signature": "Signature",
                "thinking": "Let me think about this..."
            })
        );
    }
}
