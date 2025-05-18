use serde::{Deserialize, Serialize};
use std::collections::Vec;

use crate::types::{
    CacheControlEphemeral,
    TextCitation,
};

/// Parameters for a text block.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TextBlockParam {
    /// The text content.
    pub text: String,
    
    /// The type, which is always "text".
    pub r#type: String,
    
    /// Create a cache control breakpoint at this content block.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControlEphemeral>,
    
    /// Citations for this text block.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub citations: Option<Vec<TextCitation>>,
}

impl TextBlockParam {
    /// Create a new `TextBlockParam` with the given text.
    pub fn new(text: String) -> Self {
        Self {
            text,
            r#type: "text".to_string(),
            cache_control: None,
            citations: None,
        }
    }
    
    /// Create a new `TextBlockParam` from a string reference.
    pub fn from_str(text: &str) -> Self {
        Self::new(text.to_string())
    }
    
    /// Add a cache control to this text block.
    pub fn with_cache_control(mut self, cache_control: CacheControlEphemeral) -> Self {
        self.cache_control = Some(cache_control);
        self
    }
    
    /// Add citations to this text block.
    pub fn with_citations(mut self, citations: Vec<TextCitation>) -> Self {
        self.citations = Some(citations);
        self
    }
    
    /// Add a single citation to this text block.
    pub fn with_citation(mut self, citation: TextCitation) -> Self {
        if let Some(citations) = &mut self.citations {
            citations.push(citation);
        } else {
            self.citations = Some(vec![citation]);
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, to_value};
    use crate::types::CitationCharLocation;

    #[test]
    fn test_text_block_param_serialization() {
        let text_block = TextBlockParam::new("Sample text content".to_string());
        let json = to_value(&text_block).unwrap();
        
        assert_eq!(
            json,
            json!({
                "text": "Sample text content",
                "type": "text"
            })
        );
    }

    #[test]
    fn test_text_block_param_with_cache_control() {
        let cache_control = CacheControlEphemeral::new();
        let text_block = TextBlockParam::new("Sample text content".to_string())
            .with_cache_control(cache_control);
        
        let json = to_value(&text_block).unwrap();
        
        assert_eq!(
            json,
            json!({
                "text": "Sample text content",
                "type": "text",
                "cache_control": {
                    "type": "ephemeral"
                }
            })
        );
    }

    #[test]
    fn test_text_block_param_with_citation() {
        let citation = TextCitation::CharLocation(CitationCharLocation {
            cited_text: "example text".to_string(),
            document_index: 0,
            document_title: Some("Document Title".to_string()),
            end_char_index: 12,
            start_char_index: 0,
            r#type: "char_location".to_string(),
        });
        
        let text_block = TextBlockParam::new("Sample text content".to_string())
            .with_citation(citation);
        
        let json = to_value(&text_block).unwrap();
        
        assert_eq!(
            json,
            json!({
                "text": "Sample text content",
                "type": "text",
                "citations": [
                    {
                        "cited_text": "example text",
                        "document_index": 0,
                        "document_title": "Document Title",
                        "end_char_index": 12,
                        "start_char_index": 0,
                        "type": "char_location"
                    }
                ]
            })
        );
    }
}