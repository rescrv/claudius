use serde::{Deserialize, Serialize};

/// A source for an image from a URL.
///
/// This type is used to provide an image to the model from a URL.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UrlImageSource {
    /// The URL of the image.
    pub url: String,
}

impl UrlImageSource {
    /// Creates a new UrlImageSource with the specified URL.
    pub fn new<S: Into<String>>(url: S) -> Self {
        Self { url: url.into() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialization() {
        let source = UrlImageSource {
            url: "https://example.com/image.jpg".to_string(),
        };

        let json = serde_json::to_string(&source).unwrap();
        let expected = r#"{"url":"https://example.com/image.jpg"}"#;

        assert_eq!(json, expected);
    }

    #[test]
    fn test_deserialization() {
        let json = r#"{"url":"https://example.com/image.jpg"}"#;
        let source: UrlImageSource = serde_json::from_str(json).unwrap();

        assert_eq!(source.url, "https://example.com/image.jpg");
    }
}
