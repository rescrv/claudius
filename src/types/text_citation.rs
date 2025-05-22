use serde::{Deserialize, Serialize};

use crate::types::{
    CitationCharLocation, CitationContentBlockLocation, CitationPageLocation,
    CitationWebSearchResultLocation,
};

/// A citation reference in a TextBlock.
///
/// This enum represents the different types of citations that can be included
/// in a text block's content.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
        };

        let citation = TextCitation::CharLocation(char_location);

        let json = serde_json::to_string(&citation).unwrap();
        let expected = r#"{"type":"char_location","cited_text":"example text","document_index":0,"document_title":"Document Title","end_char_index":12,"start_char_index":0}"#;

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
        };

        let citation = TextCitation::PageLocation(page_location);

        let json = serde_json::to_string(&citation).unwrap();
        let expected = r#"{"type":"page_location","cited_text":"example text","document_index":0,"document_title":"Document Title","end_page_number":5,"start_page_number":3}"#;

        assert_eq!(json, expected);
    }

    #[test]
    fn test_content_block_location_serialization() {
        let content_block_location = CitationContentBlockLocation {
            cited_text: "example text".to_string(),
            document_index: 0,
            document_title: Some("Document Title".to_string()),
            start_block_index: 1,
            end_block_index: 5,
        };

        let citation = TextCitation::ContentBlockLocation(content_block_location);

        let json = serde_json::to_string(&citation).unwrap();
        let expected = r#"{"type":"content_block_location","cited_text":"example text","document_index":0,"document_title":"Document Title","end_block_index":5,"start_block_index":1}"#;

        assert_eq!(json, expected);
    }

    #[test]
    fn test_web_search_result_location_serialization() {
        let web_search_location = CitationWebSearchResultLocation {
            cited_text: "example text".to_string(),
            encrypted_index: "abc123".to_string(),
            title: Some("Example Website".to_string()),
            url: "https://example.com/page".to_string(),
        };

        let citation = TextCitation::WebSearchResultLocation(web_search_location);

        let json = serde_json::to_string(&citation).unwrap();
        let expected = r#"{"type":"web_search_result_location","cited_text":"example text","encrypted_index":"abc123","title":"Example Website","url":"https://example.com/page"}"#;

        assert_eq!(json, expected);
    }

    #[test]
    fn test_deserialization() {
        let json = r#"{"type":"char_location","cited_text":"example text","document_index":0,"document_title":"Document Title","end_char_index":12,"start_char_index":0}"#;
        let citation: TextCitation = serde_json::from_str(json).unwrap();

        match citation {
            TextCitation::CharLocation(location) => {
                assert_eq!(location.cited_text, "example text");
                assert_eq!(location.document_index, 0);
                assert_eq!(location.document_title, Some("Document Title".to_string()));
                assert_eq!(location.end_char_index, 12);
                assert_eq!(location.start_char_index, 0);
            }
            _ => panic!("Expected CharLocation"),
        }
    }
}
