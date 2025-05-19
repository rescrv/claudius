use serde::{Deserialize, Serialize};

/// A type that represents a citation in a text block or document.
///
/// This enum is used as a parameter when creating content with citations,
/// allowing different types of reference locations to be specified.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum TextCitationParam {
    /// A character-based location citation
    #[serde(rename = "char_location")]
    CharLocation {
        /// The text that was cited
        cited_text: String,

        /// The index of the document in the input context
        document_index: i32,

        /// Optional title of the document
        #[serde(skip_serializing_if = "Option::is_none")]
        document_title: Option<String>,

        /// The end character index (exclusive) of the citation in the document
        end_char_index: i32,

        /// The start character index (inclusive) of the citation in the document
        start_char_index: i32,
    },

    /// A page-based location citation
    #[serde(rename = "page_location")]
    PageLocation {
        /// The text that was cited
        cited_text: String,

        /// The index of the document in the input context
        document_index: i32,

        /// Optional title of the document
        #[serde(skip_serializing_if = "Option::is_none")]
        document_title: Option<String>,

        /// The end page number (inclusive) of the citation in the document
        end_page_number: i32,

        /// The start page number (inclusive) of the citation in the document
        start_page_number: i32,
    },

    /// A content block-based location citation
    #[serde(rename = "content_block_location")]
    ContentBlockLocation {
        /// The text that was cited
        cited_text: String,

        /// The index of the document in the input context
        document_index: i32,

        /// Optional title of the document
        #[serde(skip_serializing_if = "Option::is_none")]
        document_title: Option<String>,

        /// The end block index (exclusive) of the citation in the document
        end_block_index: i32,

        /// The start block index (inclusive) of the citation in the document
        start_block_index: i32,
    },

    /// A web search result location citation
    #[serde(rename = "web_search_result_location")]
    WebSearchResultLocation {
        /// The text that was cited
        cited_text: String,

        /// The encrypted index of the web search result
        encrypted_index: String,

        /// Optional title of the web page
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,

        /// The URL of the web page
        url: String,
    },
}

impl TextCitationParam {
    /// Creates a new character-based location citation parameter
    pub fn char_location(
        cited_text: String,
        document_index: i32,
        start_char_index: i32,
        end_char_index: i32,
        document_title: Option<String>,
    ) -> Self {
        Self::CharLocation {
            cited_text,
            document_index,
            document_title,
            end_char_index,
            start_char_index,
        }
    }

    /// Creates a new page-based location citation parameter
    pub fn page_location(
        cited_text: String,
        document_index: i32,
        start_page_number: i32,
        end_page_number: i32,
        document_title: Option<String>,
    ) -> Self {
        Self::PageLocation {
            cited_text,
            document_index,
            document_title,
            end_page_number,
            start_page_number,
        }
    }

    /// Creates a new content block-based location citation parameter
    pub fn content_block_location(
        cited_text: String,
        document_index: i32,
        start_block_index: i32,
        end_block_index: i32,
        document_title: Option<String>,
    ) -> Self {
        Self::ContentBlockLocation {
            cited_text,
            document_index,
            document_title,
            end_block_index,
            start_block_index,
        }
    }

    /// Creates a new web search result location citation parameter
    pub fn web_search_result_location(
        cited_text: String,
        encrypted_index: String,
        url: String,
        title: Option<String>,
    ) -> Self {
        Self::WebSearchResultLocation {
            cited_text,
            encrypted_index,
            title,
            url,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_char_location_serialization() {
        let citation = TextCitationParam::char_location(
            "example text".to_string(),
            0,
            0,
            12,
            Some("Document Title".to_string()),
        );

        let json_value = serde_json::to_value(&citation).unwrap();
        let expected = json!({
            "cited_text": "example text",
            "document_index": 0,
            "document_title": "Document Title",
            "end_char_index": 12,
            "start_char_index": 0,
            "type": "char_location"
        });

        assert_eq!(json_value, expected);
    }

    #[test]
    fn test_page_location_serialization() {
        let citation = TextCitationParam::page_location(
            "example text".to_string(),
            0,
            3,
            5,
            Some("Document Title".to_string()),
        );

        let json_value = serde_json::to_value(&citation).unwrap();
        let expected = json!({
            "cited_text": "example text",
            "document_index": 0,
            "document_title": "Document Title",
            "end_page_number": 5,
            "start_page_number": 3,
            "type": "page_location"
        });

        assert_eq!(json_value, expected);
    }

    #[test]
    fn test_content_block_location_serialization() {
        let citation = TextCitationParam::content_block_location(
            "example text".to_string(),
            0,
            2,
            4,
            Some("Document Title".to_string()),
        );

        let json_value = serde_json::to_value(&citation).unwrap();
        let expected = json!({
            "cited_text": "example text",
            "document_index": 0,
            "document_title": "Document Title",
            "end_block_index": 4,
            "start_block_index": 2,
            "type": "content_block_location"
        });

        assert_eq!(json_value, expected);
    }

    #[test]
    fn test_web_search_result_location_serialization() {
        let citation = TextCitationParam::web_search_result_location(
            "example text".to_string(),
            "encrypted123".to_string(),
            "https://example.com".to_string(),
            Some("Example Website".to_string()),
        );

        let json_value = serde_json::to_value(&citation).unwrap();
        let expected = json!({
            "cited_text": "example text",
            "encrypted_index": "encrypted123",
            "title": "Example Website",
            "url": "https://example.com",
            "type": "web_search_result_location"
        });

        assert_eq!(json_value, expected);
    }

    #[test]
    fn test_optional_fields_are_omitted() {
        let citation = TextCitationParam::char_location("example text".to_string(), 0, 0, 12, None);

        let json_value = serde_json::to_value(&citation).unwrap();
        let expected = json!({
            "cited_text": "example text",
            "document_index": 0,
            "end_char_index": 12,
            "start_char_index": 0,
            "type": "char_location"
        });

        assert_eq!(json_value, expected);
    }

    #[test]
    fn test_deserialization() {
        let json_str = r#"{
            "cited_text": "example text",
            "document_index": 0,
            "document_title": "Document Title",
            "end_char_index": 12,
            "start_char_index": 0,
            "type": "char_location"
        }"#;

        let citation: TextCitationParam = serde_json::from_str(json_str).unwrap();

        match citation {
            TextCitationParam::CharLocation {
                cited_text,
                document_index,
                document_title,
                end_char_index,
                start_char_index,
            } => {
                assert_eq!(cited_text, "example text");
                assert_eq!(document_index, 0);
                assert_eq!(document_title, Some("Document Title".to_string()));
                assert_eq!(end_char_index, 12);
                assert_eq!(start_char_index, 0);
            }
            _ => panic!("Incorrect variant deserialized"),
        }
    }
}
