use serde::{Deserialize, Serialize};

use crate::types::Message;

/// An event that represents the start of a message in a streaming response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RawMessageStartEvent {
    /// The message that is starting.
    pub message: Message,
    
    /// The type, which is always "message_start".
    pub r#type: String,
}

impl RawMessageStartEvent {
    /// Create a new `RawMessageStartEvent` with the given message.
    pub fn new(message: Message) -> Self {
        Self {
            message,
            r#type: "message_start".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, to_value};
    use crate::types::{ContentBlock, Model, TextBlock, Usage};

    #[test]
    fn test_raw_message_start_event_serialization() {
        let text_block = TextBlock::new("Hello, I'm Claude.".to_string());
        let content = vec![ContentBlock::Text(text_block)];
        let model = Model::Known(crate::types::KnownModel::Claude3Sonnet);
        let usage = Usage::new(50, 100);
        
        let message = Message::new(
            "msg_012345".to_string(),
            content,
            model,
            usage,
        );
        
        let event = RawMessageStartEvent::new(message);
        
        let json = to_value(&event).unwrap();
        assert_eq!(
            json,
            json!({
                "message": {
                    "id": "msg_012345",
                    "content": [
                        {
                            "text": "Hello, I'm Claude.",
                            "type": "text"
                        }
                    ],
                    "model": "claude-3-sonnet-20240229",
                    "role": "assistant",
                    "type": "message",
                    "usage": {
                        "input_tokens": 50,
                        "output_tokens": 100
                    }
                },
                "type": "message_start"
            })
        );
    }
    
    #[test]
    fn test_raw_message_start_event_deserialization() {
        let json = json!({
            "message": {
                "id": "msg_012345",
                "content": [
                    {
                        "text": "Hello, I'm Claude.",
                        "type": "text"
                    }
                ],
                "model": "claude-3-sonnet-20240229",
                "role": "assistant",
                "type": "message",
                "usage": {
                    "input_tokens": 50,
                    "output_tokens": 100
                }
            },
            "type": "message_start"
        });
        
        let event: RawMessageStartEvent = serde_json::from_value(json).unwrap();
        assert_eq!(event.r#type, "message_start");
        assert_eq!(event.message.id, "msg_012345");
        assert_eq!(event.message.role, "assistant");
    }
}