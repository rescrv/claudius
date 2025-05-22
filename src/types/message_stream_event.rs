use serde::{Deserialize, Serialize};

use crate::types::{
    ContentBlockDeltaEvent, ContentBlockStartEvent, ContentBlockStopEvent, MessageDeltaEvent,
    MessageStartEvent, MessageStopEvent,
};

/// An event in a message stream.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum MessageStreamEvent {
    /// Ping event.
    #[serde(rename = "ping")]
    Ping,

    /// Message start event.
    #[serde(rename = "message_start")]
    MessageStart(MessageStartEvent),

    /// Message delta event.
    #[serde(rename = "message_delta")]
    MessageDelta(MessageDeltaEvent),

    /// Content block start event.
    #[serde(rename = "content_block_start")]
    ContentBlockStart(ContentBlockStartEvent),

    /// Content block delta event.
    #[serde(rename = "content_block_delta")]
    ContentBlockDelta(ContentBlockDeltaEvent),

    /// Content block stop event.
    #[serde(rename = "content_block_stop")]
    ContentBlockStop(ContentBlockStopEvent),

    /// Message stop event.
    #[serde(rename = "message_stop")]
    MessageStop(MessageStopEvent),
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{from_value, json};

    #[test]
    fn test_message_stream_event_deserialization_message_start() {
        let json = json!({
            "type": "message_start",
            "message": {
                "id": "msg_012345",
                "content": [],
                "model": "claude-3-sonnet-20240229",
                "role": "assistant",
                "type": "message",
                "usage": {
                    "input_tokens": 50,
                    "output_tokens": 100
                }
            }
        });

        let event: MessageStreamEvent = from_value(json).unwrap();
        match event {
            MessageStreamEvent::MessageStart(_) => {}
            _ => panic!("Expected MessageStart variant"),
        }
    }

    #[test]
    fn test_message_stream_event_deserialization_message_delta() {
        let json = json!({
            "type": "message_delta",
            "delta": {
                "stop_reason": "end_turn"
            },
            "usage": {
                "input_tokens": 50,
                "output_tokens": 100
            }
        });

        let event: MessageStreamEvent = from_value(json).unwrap();
        match event {
            MessageStreamEvent::MessageDelta(_) => {}
            _ => panic!("Expected MessageDelta variant"),
        }
    }

    #[test]
    fn test_message_stream_event_deserialization_message_stop() {
        let json = json!({
            "type": "message_stop"
        });

        let event: MessageStreamEvent = from_value(json).unwrap();
        match event {
            MessageStreamEvent::MessageStop(_) => {}
            _ => panic!("Expected MessageStop variant"),
        }
    }

    #[test]
    fn test_message_stream_event_deserialization_content_block_start() {
        let json = json!({
            "type": "content_block_start",
            "content_block": {
                "text": "Hello, I'm Claude.",
                "type": "text"
            },
            "index": 0
        });

        let event: MessageStreamEvent = from_value(json).unwrap();
        match event {
            MessageStreamEvent::ContentBlockStart(_) => {}
            _ => panic!("Expected ContentBlockStart variant"),
        }
    }

    #[test]
    fn test_message_stream_event_deserialization_content_block_delta() {
        let json = json!({
            "type": "content_block_delta",
            "delta": {
                "text": "Hello, I'm Claude.",
                "type": "text_delta"
            },
            "index": 0
        });

        let event: MessageStreamEvent = from_value(json).unwrap();
        match event {
            MessageStreamEvent::ContentBlockDelta(_) => {}
            _ => panic!("Expected ContentBlockDelta variant"),
        }
    }

    #[test]
    fn test_message_stream_event_deserialization_content_block_stop() {
        let json = json!({
            "type": "content_block_stop",
            "index": 0
        });

        let event: MessageStreamEvent = from_value(json).unwrap();
        match event {
            MessageStreamEvent::ContentBlockStop(_) => {}
            _ => panic!("Expected ContentBlockStop variant"),
        }
    }
}
