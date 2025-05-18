use serde::{Deserialize, Serialize};

use crate::types::ContentBlockSourceContentParam;

/// Parameter for a content block source.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ContentBlockSourceParam {
    /// The content of the source, which can be either a string or an array of content items.
    #[serde(flatten)]
    pub content: ContentBlockSourceContent,
    
    /// The type, which is always "content".
    pub r#type: String,
}

/// The content of a content block source, which can be either a string or an array of content items.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum ContentBlockSourceContent {
    /// A simple string content.
    String(String),
    
    /// An array of content items.
    Array(Vec<ContentBlockSourceContentParam>),
}

impl ContentBlockSourceParam {
    /// Create a new `ContentBlockSourceParam` with a string content.
    pub fn new_with_string(content: String) -> Self {
        Self {
            content: ContentBlockSourceContent::String(content),
            r#type: "content".to_string(),
        }
    }
    
    /// Create a new `ContentBlockSourceParam` with string content from a str reference.
    pub fn from_str(content: &str) -> Self {
        Self::new_with_string(content.to_string())
    }
    
    /// Create a new `ContentBlockSourceParam` with an array of content items.
    pub fn new_with_array(content: Vec<ContentBlockSourceContentParam>) -> Self {
        Self {
            content: ContentBlockSourceContent::Array(content),
            r#type: "content".to_string(),
        }
    }
    
    /// Create a new `ContentBlockSourceParam` with a single content item.
    pub fn new_with_item(content: ContentBlockSourceContentParam) -> Self {
        Self::new_with_array(vec![content])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, to_value};
    use crate::types::{TextBlockParam, ImageBlockParam, UrlImageSource};

    #[test]
    fn test_content_block_source_param_with_string() {
        let source = ContentBlockSourceParam::new_with_string("Sample content".to_string());
        let json = to_value(&source).unwrap();
        
        assert_eq!(
            json,
            json!({
                "content": "Sample content",
                "type": "content"
            })
        );
    }

    #[test]
    fn test_content_block_source_param_with_array() {
        let text_param = TextBlockParam::new("Sample text content".to_string());
        let url_source = UrlImageSource::new("https://example.com/image.jpg".to_string());
        let image_param = ImageBlockParam::new_with_url(url_source);
        
        let content = vec![
            ContentBlockSourceContentParam::Text(text_param),
            ContentBlockSourceContentParam::Image(image_param),
        ];
        
        let source = ContentBlockSourceParam::new_with_array(content);
        let json = to_value(&source).unwrap();
        
        assert_eq!(
            json,
            json!({
                "content": [
                    {
                        "text": "Sample text content",
                        "type": "text"
                    },
                    {
                        "source": {
                            "url": "https://example.com/image.jpg",
                            "type": "url"
                        },
                        "type": "image"
                    }
                ],
                "type": "content"
            })
        );
    }
    
    #[test]
    fn test_content_block_source_param_with_item() {
        let text_param = TextBlockParam::new("Sample text content".to_string());
        let source = ContentBlockSourceParam::new_with_item(
            ContentBlockSourceContentParam::Text(text_param)
        );
        
        let json = to_value(&source).unwrap();
        
        assert_eq!(
            json,
            json!({
                "content": [
                    {
                        "text": "Sample text content",
                        "type": "text"
                    }
                ],
                "type": "content"
            })
        );
    }
    
    #[test]
    fn test_from_str() {
        let source = ContentBlockSourceParam::from_str("Sample content");
        
        match &source.content {
            ContentBlockSourceContent::String(content) => {
                assert_eq!(content, "Sample content");
            },
            _ => panic!("Expected String variant"),
        }
        
        assert_eq!(source.r#type, "content");
    }
}