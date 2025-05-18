use serde::{Deserialize, Serialize};

use crate::types::{
    CitationsDelta,
    InputJsonDelta,
    SignatureDelta,
    TextDelta,
    ThinkingDelta,
};

/// A raw content block delta, representing a streaming update to a content block.
/// 
/// This type is used for streaming responses from the API, where content blocks
/// are updated incrementally.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum RawContentBlockDelta {
    /// A text delta.
    #[serde(rename = "text_delta")]
    TextDelta(TextDelta),
    
    /// An input JSON delta.
    #[serde(rename = "input_json_delta")]
    InputJsonDelta(InputJsonDelta),
    
    /// A citations delta.
    #[serde(rename = "citations_delta")]
    CitationsDelta(CitationsDelta),
    
    /// A thinking delta.
    #[serde(rename = "thinking_delta")]
    ThinkingDelta(ThinkingDelta),
    
    /// A signature delta.
    #[serde(rename = "signature_delta")]
    SignatureDelta(SignatureDelta),
}

impl RawContentBlockDelta {
    /// Create a new `RawContentBlockDelta` from a text delta.
    pub fn from_text_delta(text_delta: TextDelta) -> Self {
        RawContentBlockDelta::TextDelta(text_delta)
    }
    
    /// Create a new `RawContentBlockDelta` from an input JSON delta.
    pub fn from_input_json_delta(input_json_delta: InputJsonDelta) -> Self {
        RawContentBlockDelta::InputJsonDelta(input_json_delta)
    }
    
    /// Create a new `RawContentBlockDelta` from a citations delta.
    pub fn from_citations_delta(citations_delta: CitationsDelta) -> Self {
        RawContentBlockDelta::CitationsDelta(citations_delta)
    }
    
    /// Create a new `RawContentBlockDelta` from a thinking delta.
    pub fn from_thinking_delta(thinking_delta: ThinkingDelta) -> Self {
        RawContentBlockDelta::ThinkingDelta(thinking_delta)
    }
    
    /// Create a new `RawContentBlockDelta` from a signature delta.
    pub fn from_signature_delta(signature_delta: SignatureDelta) -> Self {
        RawContentBlockDelta::SignatureDelta(signature_delta)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, to_value, from_value};

    #[test]
    fn test_text_delta_serialization() {
        let text_delta = TextDelta::new("Hello world".to_string());
        let delta = RawContentBlockDelta::TextDelta(text_delta);
        
        let json = to_value(&delta).unwrap();
        assert_eq!(
            json,
            json!({
                "text": "Hello world",
                "type": "text_delta"
            })
        );
    }
    
    #[test]
    fn test_input_json_delta_serialization() {
        let input_json_delta = InputJsonDelta::new(r#"{"key":"#.to_string());
        let delta = RawContentBlockDelta::InputJsonDelta(input_json_delta);
        
        let json = to_value(&delta).unwrap();
        assert_eq!(
            json,
            json!({
                "partial_json": r#"{"key":"#,
                "type": "input_json_delta"
            })
        );
    }
    
    #[test]
    fn test_deserialization() {
        let json = json!({
            "text": "Hello world",
            "type": "text_delta"
        });
        
        let delta: RawContentBlockDelta = from_value(json).unwrap();
        match delta {
            RawContentBlockDelta::TextDelta(text_delta) => {
                assert_eq!(text_delta.text, "Hello world");
            },
            _ => panic!("Expected TextDelta variant"),
        }
    }
}