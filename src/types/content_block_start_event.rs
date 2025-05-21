use serde::{Deserialize, Serialize};

use crate::types::ContentBlock;

/// An event that represents the start of a content block in a streaming response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContentBlockStartEvent {
    /// The content block that is starting.
    pub content_block: ContentBlock,

    /// The index of the content block.
    pub index: usize,

    /// The type, which is always "content_block_start".
    #[serde(rename = "type")]
    pub r#type: String,
}

impl ContentBlockStartEvent {
    /// Create a new `ContentBlockStartEvent` with the given content block and index.
    pub fn new(content_block: ContentBlock, index: usize) -> Self {
        Self {
            content_block,
            index,
            r#type: "content_block_start".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::TextBlock;
    use serde_json::{json, to_value};

    #[test]
    fn test_content_block_start_event_serialization() {
        let text_block = TextBlock::new("Hello world".to_string());
        let content_block = ContentBlock::Text(text_block);
        let event = ContentBlockStartEvent::new(content_block, 0);

        let json = to_value(&event).unwrap();
        assert_eq!(
            json,
            json!({
                "content_block": {
                    "text": "Hello world",
                    "type": "text"
                },
                "index": 0,
                "type": "content_block_start"
            })
        );
    }

    #[test]
    fn test_content_block_start_event_deserialization() {
        let json = json!({
            "content_block": {
                "text": "Hello world",
                "type": "text"
            },
            "index": 0,
            "type": "content_block_start"
        });

        let event: ContentBlockStartEvent = serde_json::from_value(json).unwrap();
        assert_eq!(event.index, 0);
        assert_eq!(event.r#type, "content_block_start");

        match event.content_block {
            ContentBlock::Text(text_block) => {
                assert_eq!(text_block.text, "Hello world");
            }
            _ => panic!("Expected Text variant"),
        }
    }
}
