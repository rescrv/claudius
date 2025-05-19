use serde::{Deserialize, Serialize};

/// An event that represents the end of a message in a streaming response.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RawMessageStopEvent {
    /// The type, which is always "message_stop".
    pub r#type: String,
}

impl RawMessageStopEvent {
    /// Create a new `RawMessageStopEvent`.
    pub fn new() -> Self {
        Self {
            r#type: "message_stop".to_string(),
        }
    }
}

impl Default for RawMessageStopEvent {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, to_value};

    #[test]
    fn test_raw_message_stop_event_serialization() {
        let event = RawMessageStopEvent::new();

        let json = to_value(&event).unwrap();
        assert_eq!(
            json,
            json!({
                "type": "message_stop"
            })
        );
    }

    #[test]
    fn test_raw_message_stop_event_deserialization() {
        let json = json!({
            "type": "message_stop"
        });

        let event: RawMessageStopEvent = serde_json::from_value(json).unwrap();
        assert_eq!(event.r#type, "message_stop");
    }
}
