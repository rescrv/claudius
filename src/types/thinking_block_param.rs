use serde::{Deserialize, Serialize};

/// Parameters for a thinking block.
///
/// This represents a thinking block which contains the model's reasoning process.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ThinkingBlockParam {
    /// The signature for the thinking block.
    pub signature: String,
    
    /// The thinking content.
    pub thinking: String,
    
    /// The type, which is always "thinking".
    pub r#type: String,
}

impl ThinkingBlockParam {
    /// Create a new `ThinkingBlockParam` with the given signature and thinking content.
    pub fn new(signature: String, thinking: String) -> Self {
        Self {
            signature,
            thinking,
            r#type: "thinking".to_string(),
        }
    }
    
    /// Create a new `ThinkingBlockParam` from string references.
    pub fn from_str(signature: &str, thinking: &str) -> Self {
        Self::new(signature.to_string(), thinking.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, to_value};

    #[test]
    fn test_thinking_block_param_serialization() {
        let block = ThinkingBlockParam::new(
            "Signature".to_string(),
            "Let me think about this...".to_string()
        );
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
    fn test_thinking_block_param_deserialization() {
        let json = json!({
            "signature": "Signature",
            "thinking": "Let me think about this...",
            "type": "thinking"
        });
        
        let block: ThinkingBlockParam = serde_json::from_value(json).unwrap();
        assert_eq!(block.signature, "Signature");
        assert_eq!(block.thinking, "Let me think about this...");
        assert_eq!(block.r#type, "thinking");
    }
}