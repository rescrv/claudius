use serde::{Deserialize, Serialize};

/// Parameters for a web search result block.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WebSearchResultBlockParam {
    /// The encrypted content of the web search result.
    pub encrypted_content: String,
    
    /// The title of the web search result.
    pub title: String,
    
    /// The type, which is always "web_search_result".
    pub r#type: String,
    
    /// The URL of the web search result.
    pub url: String,
    
    /// The age of the web page, if available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_age: Option<String>,
}

impl WebSearchResultBlockParam {
    /// Create a new `WebSearchResultBlockParam` with the given parameters.
    pub fn new(
        encrypted_content: String,
        title: String,
        url: String,
    ) -> Self {
        Self {
            encrypted_content,
            title,
            r#type: "web_search_result".to_string(),
            url,
            page_age: None,
        }
    }
    
    /// Add page age to this web search result block.
    pub fn with_page_age(mut self, page_age: String) -> Self {
        self.page_age = Some(page_age);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, to_value};

    #[test]
    fn test_web_search_result_block_param_minimal() {
        let block = WebSearchResultBlockParam::new(
            "encrypted-content".to_string(),
            "Example Title".to_string(),
            "https://example.com".to_string(),
        );
        
        let json = to_value(&block).unwrap();
        assert_eq!(
            json,
            json!({
                "encrypted_content": "encrypted-content",
                "title": "Example Title",
                "type": "web_search_result",
                "url": "https://example.com"
            })
        );
    }
    
    #[test]
    fn test_web_search_result_block_param_with_page_age() {
        let block = WebSearchResultBlockParam::new(
            "encrypted-content".to_string(),
            "Example Title".to_string(),
            "https://example.com".to_string(),
        ).with_page_age("1 day ago".to_string());
        
        let json = to_value(&block).unwrap();
        assert_eq!(
            json,
            json!({
                "encrypted_content": "encrypted-content",
                "title": "Example Title",
                "type": "web_search_result",
                "url": "https://example.com",
                "page_age": "1 day ago"
            })
        );
    }
    
    #[test]
    fn test_web_search_result_block_param_deserialization() {
        let json = json!({
            "encrypted_content": "encrypted-content",
            "title": "Example Title",
            "type": "web_search_result",
            "url": "https://example.com",
            "page_age": "1 day ago"
        });
        
        let block: WebSearchResultBlockParam = serde_json::from_value(json).unwrap();
        assert_eq!(block.encrypted_content, "encrypted-content");
        assert_eq!(block.title, "Example Title");
        assert_eq!(block.r#type, "web_search_result");
        assert_eq!(block.url, "https://example.com");
        assert_eq!(block.page_age, Some("1 day ago".to_string()));
    }
}