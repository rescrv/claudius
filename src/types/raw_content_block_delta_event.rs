use serde::{Deserialize, Serialize};

use crate::types::RawContentBlockDelta;

/// An event that represents a delta update to a content block in a streaming response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RawContentBlockDeltaEvent {
    /// The delta update to the content block.
    pub delta: RawContentBlockDelta,

    /// The index of the content block being updated.
    pub index: usize,

    /// The type, which is always "content_block_delta".
    pub r#type: String,
}

impl RawContentBlockDeltaEvent {
    /// Create a new `RawContentBlockDeltaEvent` with the given delta and index.
    pub fn new(delta: RawContentBlockDelta, index: usize) -> Self {
        Self {
            delta,
            index,
            r#type: "content_block_delta".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::TextDelta;
    use serde_json::{json, to_value};

    #[test]
    fn test_raw_content_block_delta_event_serialization() {
        let text_delta = TextDelta::new("Hello world".to_string());
        let delta = RawContentBlockDelta::TextDelta(text_delta);
        let event = RawContentBlockDeltaEvent::new(delta, 0);

        let json = to_value(&event).unwrap();
        assert_eq!(
            json,
            json!({
                "delta": {
                    "text": "Hello world",
                    "type": "text_delta"
                },
                "index": 0,
                "type": "content_block_delta"
            })
        );
    }

    #[test]
    fn test_raw_content_block_delta_event_deserialization() {
        let json = json!({
            "delta": {
                "text": "Hello world",
                "type": "text_delta"
            },
            "index": 0,
            "type": "content_block_delta"
        });

        let event: RawContentBlockDeltaEvent = serde_json::from_value(json).unwrap();
        assert_eq!(event.index, 0);
        assert_eq!(event.r#type, "content_block_delta");

        match event.delta {
            RawContentBlockDelta::TextDelta(text_delta) => {
                assert_eq!(text_delta.text, "Hello world");
            }
            _ => panic!("Expected TextDelta variant"),
        }
    }
}
