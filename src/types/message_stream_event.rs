use serde::{Deserialize, Serialize};

use crate::types::{
    ContentBlockDeltaEvent, ContentBlockStartEvent, ContentBlockStopEvent, MessageDeltaEvent,
    MessageStartEvent, MessageStopEvent,
};

/// An event in a message stream.
///
/// This enum represents all possible events that can occur when streaming
/// messages from the Anthropic API. Events are delivered in a specific order:
/// message_start, then potentially multiple content_block events, and finally
/// message_stop.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum MessageStreamEvent {
    /// A periodic ping event to keep the connection alive.
    ///
    /// These events have no payload and can be safely ignored.
    #[serde(rename = "ping")]
    Ping,

    /// Indicates the start of a new message in the stream.
    ///
    /// This event contains the initial message metadata including ID, model,
    /// role, and initial usage statistics.
    #[serde(rename = "message_start")]
    MessageStart(MessageStartEvent),

    /// Provides incremental updates to the message being generated.
    ///
    /// This includes updates to stop_reason, stop_sequence, and usage statistics.
    #[serde(rename = "message_delta")]
    MessageDelta(MessageDeltaEvent),

    /// Marks the beginning of a new content block within the message.
    ///
    /// Content blocks can be text, tool_use, or other content types.
    #[serde(rename = "content_block_start")]
    ContentBlockStart(ContentBlockStartEvent),

    /// Provides incremental updates to the current content block.
    ///
    /// For text blocks, this contains partial text. For tool_use blocks,
    /// this contains partial JSON input.
    #[serde(rename = "content_block_delta")]
    ContentBlockDelta(ContentBlockDeltaEvent),

    /// Indicates that the current content block is complete.
    ///
    /// After this event, either a new content_block_start or message_stop will follow.
    #[serde(rename = "content_block_stop")]
    ContentBlockStop(ContentBlockStopEvent),

    /// Marks the end of the message stream.
    ///
    /// This is always the final event in a successful stream.
    #[serde(rename = "message_stop")]
    MessageStop(MessageStopEvent),
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{from_value, json};

    #[test]
    fn message_stream_event_deserialization_message_start() {
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
    fn message_stream_event_deserialization_message_delta() {
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
    fn message_stream_event_deserialization_message_stop() {
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
    fn message_stream_event_deserialization_content_block_start() {
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
    fn message_stream_event_deserialization_content_block_delta() {
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
    fn message_stream_event_deserialization_content_block_stop() {
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
