use serde::{Deserialize, Serialize};

/// Count of tokens in a message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MessageTokensCount {
    /// The total number of tokens across the provided list of messages, system prompt,
    /// and tools.
    pub input_tokens: u32,
}

impl MessageTokensCount {
    /// Create a new `MessageTokensCount` with the given input tokens.
    pub fn new(input_tokens: u32) -> Self {
        Self { input_tokens }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, to_value, from_value};

    #[test]
    fn test_message_tokens_count_serialization() {
        let count = MessageTokensCount::new(123);
        
        let json = to_value(&count).unwrap();
        assert_eq!(
            json,
            json!({
                "input_tokens": 123
            })
        );
    }
    
    #[test]
    fn test_message_tokens_count_deserialization() {
        let json = json!({
            "input_tokens": 456
        });
        
        let count: MessageTokensCount = from_value(json).unwrap();
        assert_eq!(count.input_tokens, 456);
    }
}