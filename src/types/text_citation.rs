use serde::{Serialize, Deserialize};

use crate::types::{
    CitationCharLocation,
    CitationPageLocation,
    CitationContentBlockLocation,
    CitationWebSearchResultLocation,
};

/// A citation reference in a TextBlock.
///
/// This enum represents the different types of citations that can be included 
/// in a text block's content.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum TextCitation {
    /// A character-based location citation
    #[serde(rename = "char_location")]
    CharLocation(CitationCharLocation),
    
    /// A page-based location citation
    #[serde(rename = "page_location")]
    PageLocation(CitationPageLocation),
    
    /// A content block-based location citation
    #[serde(rename = "content_block_location")]
    ContentBlockLocation(CitationContentBlockLocation),
    
    /// A web search result location citation
    #[serde(rename = "web_search_result_location")]
    WebSearchResultLocation(CitationWebSearchResultLocation),
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_char_location_serialization() {
        let char_location = CitationCharLocation {
            cited_text: "example text".to_string(),
            document_index: 0,
            document_title: Some("Document Title".to_string()),
            end_char_index: 12,
            start_char_index: 0,
            r#type: "char_location".to_string(),
        };
        
        let citation = TextCitation::CharLocation(char_location);
        
        let json = serde_json::to_string(&citation).unwrap();
        let expected = r#"{"cited_text":"example text","document_index":0,"document_title":"Document Title","end_char_index":12,"start_char_index":0,"type":"char_location"}"#;
        
        assert_eq!(json, expected);
    }
    
    #[test]
    fn test_page_location_serialization() {
        let page_location = CitationPageLocation {
            cited_text: "example text".to_string(),
            document_index: 0,
            document_title: Some("Document Title".to_string()),
            end_page_number: 5,
            start_page_number: 3,
            r#type: "page_location".to_string(),
        };
        
        let citation = TextCitation::PageLocation(page_location);
        
        let json = serde_json::to_string(&citation).unwrap();
        let expected = r#"{"cited_text":"example text","document_index":0,"document_title":"Document Title","end_page_number":5,"start_page_number":3,"type":"page_location"}"#;
        
        assert_eq!(json, expected);
    }
    
    #[test]
    fn test_deserialization() {
        let json = r#"{"cited_text":"example text","document_index":0,"document_title":"Document Title","end_char_index":12,"start_char_index":0,"type":"char_location"}"#;
        let citation: TextCitation = serde_json::from_str(json).unwrap();
        
        match citation {
            TextCitation::CharLocation(location) => {
                assert_eq!(location.cited_text, "example text");
                assert_eq!(location.document_index, 0);
                assert_eq!(location.document_title, Some("Document Title".to_string()));
                assert_eq!(location.end_char_index, 12);
                assert_eq!(location.start_char_index, 0);
            },
            _ => panic!("Expected CharLocation"),
        }
    }
}