use serde::{Deserialize, Serialize};

/// A block containing model thinking details.
///
/// ThinkingBlocks contain internal reasoning or deliberation from the model.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ThinkingBlock {
    /// A signature for the thinking (typically a hash).
    pub signature: String,

    /// The thinking content.
    pub thinking: String,

    /// The type of content block, always "thinking" for this struct.
    #[serde(default = "default_type", rename = "type")]
    pub r#type: String,
}

fn default_type() -> String {
    "thinking".to_string()
}

impl ThinkingBlock {
    /// Creates a new ThinkingBlock with the specified thinking content and signature.
    pub fn new<S1: Into<String>, S2: Into<String>>(thinking: S1, signature: S2) -> Self {
        Self {
            thinking: thinking.into(),
            signature: signature.into(),
            r#type: default_type(),
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
    fn test_thinking_block_serialization() {
        let thinking_block = ThinkingBlock::new(
            "Let me think through this problem step by step...",
            "abc123signature",
        );

        let json = serde_json::to_string(&thinking_block).unwrap();
        let expected = r#"{"signature":"abc123signature","thinking":"Let me think through this problem step by step...","type":"thinking"}"#;

        assert_eq!(json, expected);
    }

    #[test]
    fn test_deserialization() {
        let json = r#"{"signature":"abc123signature","thinking":"Let me think through this problem step by step...","type":"thinking"}"#;
        let thinking_block: ThinkingBlock = serde_json::from_str(json).unwrap();

        assert_eq!(thinking_block.signature, "abc123signature");
        assert_eq!(
            thinking_block.thinking,
            "Let me think through this problem step by step..."
        );
        assert_eq!(thinking_block.r#type, "thinking");
    }

    #[test]
    fn test_thinking_block_with_string_references() {
        let block = ThinkingBlock::new("Let me think about this...", "Signature");
        let json = to_value(&block).unwrap();

        assert_eq!(
            json,
            json!({
                "signature": "Signature",
                "thinking": "Let me think about this...",
                "type": "thinking"
            })
        );
    }

    #[test]
    fn test_thinking_block_from_str() {
        let block = ThinkingBlock::from_str("Signature", "Let me think about this...");
        let json = to_value(&block).unwrap();

        assert_eq!(
            json,
            json!({
                "signature": "Signature",
                "thinking": "Let me think about this...",
                "type": "thinking"
            })
        );
    }
}
