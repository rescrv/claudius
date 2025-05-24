use serde::{Deserialize, Serialize};

/// A block containing a web search result.
///
/// WebSearchResultBlock represents a single result from a web search operation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WebSearchResultBlock {
    /// Encrypted content from the web search result.
    pub encrypted_content: String,

    /// Optional age of the page, if available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_age: Option<String>,

    /// The title of the web page.
    pub title: String,

    /// The URL of the web page.
    pub url: String,
}

impl WebSearchResultBlock {
    /// Creates a new WebSearchResultBlock.
    pub fn new<S1: Into<String>, S2: Into<String>, S3: Into<String>>(
        encrypted_content: S1,
        title: S2,
        url: S3,
    ) -> Self {
        Self {
            encrypted_content: encrypted_content.into(),
            page_age: None,
            title: title.into(),
            url: url.into(),
        }
    }

    /// Add page age to this web search result block.
    pub fn with_page_age(mut self, page_age: String) -> Self {
        self.page_age = Some(page_age);
        self
    }

    /// Returns the domain (host) part of the URL if it can be parsed.
    pub fn domain(&self) -> Option<String> {
        url::Url::parse(&self.url)
            .ok()
            .and_then(|url| url.host_str().map(|s| s.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialization() {
        let block = WebSearchResultBlock {
            encrypted_content: "encrypted-data-123".to_string(),
            page_age: Some("2 days ago".to_string()),
            title: "Example Page Title".to_string(),
            url: "https://example.com/page".to_string(),
        };

        let json = serde_json::to_string(&block).unwrap();
        let expected = r#"{"encrypted_content":"encrypted-data-123","page_age":"2 days ago","title":"Example Page Title","url":"https://example.com/page"}"#;

        assert_eq!(json, expected);
    }

    #[test]
    fn test_serialization_without_page_age() {
        let block = WebSearchResultBlock {
            encrypted_content: "encrypted-data-123".to_string(),
            page_age: None,
            title: "Example Page Title".to_string(),
            url: "https://example.com/page".to_string(),
        };

        let json = serde_json::to_string(&block).unwrap();
        let expected = r#"{"encrypted_content":"encrypted-data-123","title":"Example Page Title","url":"https://example.com/page"}"#;

        assert_eq!(json, expected);
    }

    #[test]
    fn test_deserialization() {
        let json = r#"{"encrypted_content":"encrypted-data-123","page_age":"2 days ago","title":"Example Page Title","url":"https://example.com/page"}"#;
        let block: WebSearchResultBlock = serde_json::from_str(json).unwrap();

        assert_eq!(block.encrypted_content, "encrypted-data-123");
        assert_eq!(block.page_age, Some("2 days ago".to_string()));
        assert_eq!(block.title, "Example Page Title");
        assert_eq!(block.url, "https://example.com/page");
    }

    #[test]
    fn test_domain() {
        let block = WebSearchResultBlock::new(
            "encrypted-data-123",
            "Example Page Title",
            "https://example.com/page",
        );

        assert_eq!(block.domain(), Some("example.com".to_string()));
    }
}
