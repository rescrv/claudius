use serde::{Deserialize, Serialize};

use crate::types::{
    ContentBlockDeltaEvent, ContentBlockStartEvent, ContentBlockStopEvent, RawMessageDeltaEvent,
    RawMessageStartEvent, RawMessageStopEvent,
};

/// An event in a message stream.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum RawMessageStreamEvent {
    /// Message start event.
    #[serde(rename = "message_start")]
    MessageStart(RawMessageStartEvent),

    /// Message delta event.
    #[serde(rename = "message_delta")]
    MessageDelta(RawMessageDeltaEvent),

    /// Message stop event.
    #[serde(rename = "message_stop")]
    MessageStop(RawMessageStopEvent),

    /// Content block start event.
    #[serde(rename = "content_block_start")]
    ContentBlockStart(ContentBlockStartEvent),

    /// Content block delta event.
    #[serde(rename = "content_block_delta")]
    ContentBlockDelta(ContentBlockDeltaEvent),

    /// Content block stop event.
    #[serde(rename = "content_block_stop")]
    ContentBlockStop(ContentBlockStopEvent),
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{from_value, json};

    #[test]
    fn test_message_stream_event_deserialization_message_start() {
        let json = json!({
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
            },
            "type": "message_start"
        });

        let event: RawMessageStreamEvent = from_value(json).unwrap();
        match event {
            RawMessageStreamEvent::MessageStart(_) => {}
            _ => panic!("Expected MessageStart variant"),
        }
    }

    #[test]
    fn test_message_stream_event_deserialization_message_delta() {
        let json = json!({
            "delta": {
                "stop_reason": "end_turn"
            },
            "type": "message_delta",
            "usage": {
                "input_tokens": 50,
                "output_tokens": 100
            }
        });

        let event: RawMessageStreamEvent = from_value(json).unwrap();
        match event {
            RawMessageStreamEvent::MessageDelta(_) => {}
            _ => panic!("Expected MessageDelta variant"),
        }
    }

    #[test]
    fn test_message_stream_event_deserialization_message_stop() {
        let json = json!({
            "type": "message_stop"
        });

        let event: RawMessageStreamEvent = from_value(json).unwrap();
        match event {
            RawMessageStreamEvent::MessageStop(_) => {}
            _ => panic!("Expected MessageStop variant"),
        }
    }

    #[test]
    fn test_message_stream_event_deserialization_content_block_start() {
        let json = json!({
            "content_block": {
                "text": "Hello, I'm Claude.",
                "type": "text"
            },
            "index": 0,
            "type": "content_block_start"
        });

        let event: RawMessageStreamEvent = from_value(json).unwrap();
        match event {
            RawMessageStreamEvent::ContentBlockStart(_) => {}
            _ => panic!("Expected ContentBlockStart variant"),
        }
    }

    #[test]
    fn test_message_stream_event_deserialization_content_block_delta() {
        let json = json!({
            "delta": {
                "text": "Hello, I'm Claude.",
                "type": "text_delta"
            },
            "index": 0,
            "type": "content_block_delta"
        });

        let event: RawMessageStreamEvent = from_value(json).unwrap();
        match event {
            RawMessageStreamEvent::ContentBlockDelta(_) => {}
            _ => panic!("Expected ContentBlockDelta variant"),
        }
    }

    #[test]
    fn test_message_stream_event_deserialization_content_block_stop() {
        let json = json!({
            "index": 0,
            "type": "content_block_stop"
        });

        let event: RawMessageStreamEvent = from_value(json).unwrap();
        match event {
            RawMessageStreamEvent::ContentBlockStop(_) => {}
            _ => panic!("Expected ContentBlockStop variant"),
        }
    }
}
