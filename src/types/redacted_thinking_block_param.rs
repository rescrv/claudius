use serde::{Deserialize, Serialize};

/// Parameters for a redacted thinking block.
///
/// This represents a thinking block where the content has been redacted.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RedactedThinkingBlockParam {
    /// The redacted thinking data.
    pub data: String,

    /// The type, which is always "redacted_thinking".
    pub r#type: String,
}

impl RedactedThinkingBlockParam {
    /// Create a new `RedactedThinkingBlockParam` with the given data.
    pub fn new(data: String) -> Self {
        Self {
            data,
            r#type: "redacted_thinking".to_string(),
        }
    }

    /// Create a new `RedactedThinkingBlockParam` from a string reference.
    pub fn from_string_ref(data: &str) -> Self {
        Self::new(data.to_string())
    }
}

impl std::str::FromStr for RedactedThinkingBlockParam {
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
    fn test_redacted_thinking_block_param_serialization() {
        let block = RedactedThinkingBlockParam::new("Redacted thinking content".to_string());
        let json = to_value(&block).unwrap();

        assert_eq!(
            json,
            json!({
                "data": "Redacted thinking content",
                "type": "redacted_thinking"
            })
        );
    }

    #[test]
    fn test_redacted_thinking_block_param_deserialization() {
        let json = json!({
            "data": "Redacted thinking content",
            "type": "redacted_thinking"
        });

        let block: RedactedThinkingBlockParam = serde_json::from_value(json).unwrap();
        assert_eq!(block.data, "Redacted thinking content");
        assert_eq!(block.r#type, "redacted_thinking");
    }

    #[test]
    fn test_from_str() {
        let block = "Redacted thinking content"
            .parse::<RedactedThinkingBlockParam>()
            .unwrap();
        assert_eq!(block.data, "Redacted thinking content");
        assert_eq!(block.r#type, "redacted_thinking");
    }
}
