use serde::{Deserialize, Serialize};

/// An event that represents the end of a content block in a streaming response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContentBlockStopEvent {
    /// The index of the content block that is ending.
    pub index: usize,

    /// The type, which is always "content_block_stop".
    #[serde(rename = "type")]
    pub r#type: String,
}

impl ContentBlockStopEvent {
    /// Create a new `ContentBlockStopEvent` with the given index.
    pub fn new(index: usize) -> Self {
        Self {
            index,
            r#type: "content_block_stop".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, to_value};

    #[test]
    fn test_content_block_stop_event_serialization() {
        let event = ContentBlockStopEvent::new(0);

        let json = to_value(&event).unwrap();
        assert_eq!(
            json,
            json!({
                "index": 0,
                "type": "content_block_stop"
            })
        );
    }

    #[test]
    fn test_content_block_stop_event_deserialization() {
        let json = json!({
            "index": 0,
            "type": "content_block_stop"
        });

        let event: ContentBlockStopEvent = serde_json::from_value(json).unwrap();
        assert_eq!(event.index, 0);
        assert_eq!(event.r#type, "content_block_stop");
    }
}
