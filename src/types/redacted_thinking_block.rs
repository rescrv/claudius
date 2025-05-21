use serde::{Deserialize, Serialize};

/// A redacted thinking block that contains encoded/obscured thinking data.
///
/// This block is used when the full thinking contents are not directly accessible.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RedactedThinkingBlock {
    /// The encoded thinking data (redacted from normal display).
    pub data: String,

    /// The type of content block, always "redacted_thinking" for this struct.
    #[serde(default = "default_type", rename = "type")]
    pub r#type: String,
}

fn default_type() -> String {
    "redacted_thinking".to_string()
}

impl RedactedThinkingBlock {
    /// Creates a new RedactedThinkingBlock with the specified data.
    pub fn new<S: Into<String>>(data: S) -> Self {
        Self {
            data: data.into(),
            r#type: default_type(),
        }
    }
}

impl std::str::FromStr for RedactedThinkingBlock {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::new(s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, to_value};

    #[test]
    fn test_redacted_thinking_block_serialization() {
        let block = RedactedThinkingBlock::new("encoded-thinking-data-123");

        let json = serde_json::to_string(&block).unwrap();
        let expected = r#"{"data":"encoded-thinking-data-123","type":"redacted_thinking"}"#;

        assert_eq!(json, expected);
    }

    #[test]
    fn test_deserialization() {
        let json = r#"{"data":"encoded-thinking-data-123","type":"redacted_thinking"}"#;
        let block: RedactedThinkingBlock = serde_json::from_str(json).unwrap();

        assert_eq!(block.data, "encoded-thinking-data-123");
        assert_eq!(block.r#type, "redacted_thinking");
    }

    #[test]
    fn test_from_string_ref() {
        let block = RedactedThinkingBlock::from_string_ref("Redacted thinking content");
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
    fn test_from_str() {
        let block = "Redacted thinking content"
            .parse::<RedactedThinkingBlock>()
            .unwrap();
        assert_eq!(block.data, "Redacted thinking content");
        assert_eq!(block.r#type, "redacted_thinking");
    }
}
