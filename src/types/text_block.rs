use serde::{Serialize, Deserialize};

use crate::types::TextCitation;

/// A block of text content in a message.
///
/// TextBlocks contain plain text content and optional citations.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextBlock {
    /// Optional citations supporting the text block.
    ///
    /// The type of citation returned will depend on the type of document being cited.
    /// Citing a PDF results in `page_location`, plain text results in `char_location`,
    /// and content document results in `content_block_location`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub citations: Option<Vec<TextCitation>>,
    
    /// The text content.
    pub text: String,
    
    /// The type of content block, always "text" for this struct.
    #[serde(default = "default_type")]
    pub r#type: String,
}

fn default_type() -> String {
    "text".to_string()
}

impl TextBlock {
    /// Creates a new TextBlock with the specified text.
    pub fn new<S: Into<String>>(text: S) -> Self {
        Self {
            text: text.into(),
            citations: None,
            r#type: default_type(),
        }
    }
    
    /// Creates a new TextBlock with the specified text and citations.
    pub fn with_citations<S: Into<String>>(text: S, citations: Vec<TextCitation>) -> Self {
        Self {
            text: text.into(),
            citations: Some(citations),
            r#type: default_type(),
        }
    }
    
    /// Returns the number of citations if any, or 0 if there are none.
    pub fn citation_count(&self) -> usize {
        self.citations.as_ref().map_or(0, |c| c.len())
    }
    
    /// Returns true if this text block has citations.
    pub fn has_citations(&self) -> bool {
        self.citation_count() > 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::CitationCharLocation;
    
    #[test]
    fn test_text_block_serialization() {
        let text_block = TextBlock::new("This is some text content.");
        
        let json = serde_json::to_string(&text_block).unwrap();
        let expected = r#"{"text":"This is some text content.","type":"text"}"#;
        
        assert_eq!(json, expected);
    }
    
    #[test]
    fn test_text_block_with_citations_serialization() {
        let char_location = CitationCharLocation {
            cited_text: "example text".to_string(),
            document_index: 0,
            document_title: Some("Document Title".to_string()),
            end_char_index: 12,
            start_char_index: 0,
            r#type: "char_location".to_string(),
        };
        
        let citation = TextCitation::CharLocation(char_location);
        
        let text_block = TextBlock::with_citations(
            "This is some text content with a citation.",
            vec![citation],
        );
        
        let json = serde_json::to_string(&text_block).unwrap();
        let expected = r#"{"citations":[{"cited_text":"example text","document_index":0,"document_title":"Document Title","end_char_index":12,"start_char_index":0,"type":"char_location"}],"text":"This is some text content with a citation.","type":"text"}"#;
        
        assert_eq!(json, expected);
    }
    
    #[test]
    fn test_deserialization() {
        let json = r#"{"text":"This is some text content.","type":"text"}"#;
        let text_block: TextBlock = serde_json::from_str(json).unwrap();
        
        assert_eq!(text_block.text, "This is some text content.");
        assert_eq!(text_block.r#type, "text");
        assert!(text_block.citations.is_none());
    }
    
    #[test]
    fn test_helper_methods() {
        let text_block = TextBlock::new("Simple text");
        assert_eq!(text_block.citation_count(), 0);
        assert!(!text_block.has_citations());
        
        let char_location = CitationCharLocation {
            cited_text: "example text".to_string(),
            document_index: 0,
            document_title: Some("Document Title".to_string()),
            end_char_index: 12,
            start_char_index: 0,
            r#type: "char_location".to_string(),
        };
        
        let citation = TextCitation::CharLocation(char_location);
        
        let text_block = TextBlock::with_citations(
            "Text with citation",
            vec![citation],
        );
        
        assert_eq!(text_block.citation_count(), 1);
        assert!(text_block.has_citations());
    }
}