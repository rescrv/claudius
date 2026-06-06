use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::types::{CitationsDelta, InputJsonDelta, SignatureDelta, TextDelta, ThinkingDelta};

/// A raw content block delta, representing a streaming update to a content block.
///
/// This type is used for streaming responses from the API, where content blocks
/// are updated incrementally.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "type")]
pub enum ContentBlockDelta {
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

impl<'de> Deserialize<'de> for ContentBlockDelta {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let delta_type = value
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| serde::de::Error::missing_field("type"))?
            .to_string();

        match delta_type.as_str() {
            "text_delta" => from_value(value).map(ContentBlockDelta::TextDelta),
            "input_json_delta" => from_value(value).map(ContentBlockDelta::InputJsonDelta),
            "citations_delta" => from_value(value).map(ContentBlockDelta::CitationsDelta),
            "thinking_delta" | "summary_delta" | "thinking_summary_delta" => {
                from_value(value).map(ContentBlockDelta::ThinkingDelta)
            }
            "signature_delta" => from_value(value).map(ContentBlockDelta::SignatureDelta),
            other if is_thinking_like_delta(other) => {
                let thinking_delta: ThinkingDelta = from_value(value)?;
                if thinking_delta.thinking.is_empty() {
                    Err(serde::de::Error::custom(format!(
                        "unknown thinking delta type {other:?} did not contain text"
                    )))
                } else {
                    Ok(ContentBlockDelta::ThinkingDelta(thinking_delta))
                }
            }
            other => Err(serde::de::Error::unknown_variant(
                other,
                &[
                    "text_delta",
                    "input_json_delta",
                    "citations_delta",
                    "thinking_delta",
                    "summary_delta",
                    "thinking_summary_delta",
                    "signature_delta",
                ],
            )),
        }
    }
}

fn from_value<T, E>(value: Value) -> Result<T, E>
where
    T: serde::de::DeserializeOwned,
    E: serde::de::Error,
{
    serde_json::from_value(value).map_err(E::custom)
}

fn is_thinking_like_delta(delta_type: &str) -> bool {
    delta_type.contains("thinking") || delta_type.contains("summary")
}

impl ContentBlockDelta {
    /// Create a new `ContentBlockDelta` from a text delta.
    pub fn from_text_delta(text_delta: TextDelta) -> Self {
        ContentBlockDelta::TextDelta(text_delta)
    }

    /// Create a new `ContentBlockDelta` from an input JSON delta.
    pub fn from_input_json_delta(input_json_delta: InputJsonDelta) -> Self {
        ContentBlockDelta::InputJsonDelta(input_json_delta)
    }

    /// Create a new `ContentBlockDelta` from a citations delta.
    pub fn from_citations_delta(citations_delta: CitationsDelta) -> Self {
        ContentBlockDelta::CitationsDelta(citations_delta)
    }

    /// Create a new `ContentBlockDelta` from a thinking delta.
    pub fn from_thinking_delta(thinking_delta: ThinkingDelta) -> Self {
        ContentBlockDelta::ThinkingDelta(thinking_delta)
    }

    /// Create a new `ContentBlockDelta` from a signature delta.
    pub fn from_signature_delta(signature_delta: SignatureDelta) -> Self {
        ContentBlockDelta::SignatureDelta(signature_delta)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{from_value, json, to_value};

    #[test]
    fn text_delta_serialization() {
        let text_delta = TextDelta::new("Hello world".to_string());
        let delta = ContentBlockDelta::TextDelta(text_delta);

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
    fn input_json_delta_serialization() {
        let input_json_delta = InputJsonDelta::new(r#"{"key":"#.to_string());
        let delta = ContentBlockDelta::InputJsonDelta(input_json_delta);

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
    fn citations_delta_serialization() {
        let char_location = crate::types::CitationCharLocation {
            cited_text: "example text".to_string(),
            document_index: 0,
            document_title: Some("Document Title".to_string()),
            end_char_index: 12,
            start_char_index: 0,
        };

        let citations_delta = CitationsDelta::with_char_location(char_location);
        let delta = ContentBlockDelta::CitationsDelta(citations_delta);

        let json = to_value(&delta).unwrap();
        assert_eq!(
            json,
            json!({
                "citation": {
                    "type": "char_location",
                    "cited_text": "example text",
                    "document_index": 0,
                    "document_title": "Document Title",
                    "end_char_index": 12,
                    "start_char_index": 0
                },
                "type": "citations_delta"
            })
        );
    }

    #[test]
    fn thinking_delta_serialization() {
        let thinking_delta = ThinkingDelta::new("Let me think about this...".to_string());
        let delta = ContentBlockDelta::ThinkingDelta(thinking_delta);

        let json = to_value(&delta).unwrap();
        assert_eq!(
            json,
            json!({
                "thinking": "Let me think about this...",
                "type": "thinking_delta"
            })
        );
    }

    #[test]
    fn signature_delta_serialization() {
        let signature_delta = SignatureDelta::new("Robert Paulson".to_string());
        let delta = ContentBlockDelta::SignatureDelta(signature_delta);

        let json = to_value(&delta).unwrap();
        assert_eq!(
            json,
            json!({
                "signature": "Robert Paulson",
                "type": "signature_delta"
            })
        );
    }

    #[test]
    fn deserialization() {
        let json = json!({
            "text": "Hello world",
            "type": "text_delta"
        });

        let delta: ContentBlockDelta = from_value(json).unwrap();
        match delta {
            ContentBlockDelta::TextDelta(text_delta) => {
                assert_eq!(text_delta.text, "Hello world");
            }
            _ => panic!("Expected TextDelta variant"),
        }
    }

    #[test]
    fn deserializes_summary_delta_as_thinking_delta() {
        let json = json!({
            "summary": "A compact thinking summary.",
            "type": "summary_delta"
        });

        let delta: ContentBlockDelta = from_value(json).unwrap();
        match delta {
            ContentBlockDelta::ThinkingDelta(thinking_delta) => {
                assert_eq!(thinking_delta.thinking, "A compact thinking summary.");
            }
            _ => panic!("Expected ThinkingDelta variant"),
        }
    }

    #[test]
    fn deserializes_thinking_summary_delta_as_thinking_delta() {
        let json = json!({
            "text": "Thinking summary text.",
            "type": "thinking_summary_delta"
        });

        let delta: ContentBlockDelta = from_value(json).unwrap();
        match delta {
            ContentBlockDelta::ThinkingDelta(thinking_delta) => {
                assert_eq!(thinking_delta.thinking, "Thinking summary text.");
            }
            _ => panic!("Expected ThinkingDelta variant"),
        }
    }
}
