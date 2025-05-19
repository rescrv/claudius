use base64::Engine;
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::Read;
use std::path::Path;

/// Represents a base64-encoded image source.
///
/// This can be created from either a base64-encoded string or from a file path.
/// The media_type must be one of the supported image formats: "image/jpeg", "image/png", "image/gif", or "image/webp".
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Base64ImageSource {
    /// The base64-encoded data of the image
    pub data: String,

    /// The media type of the image (jpeg, png, gif, or webp)
    pub media_type: ImageMediaType,

    /// The source type (always "base64" for this struct)
    #[serde(default = "default_type")]
    pub r#type: String,
}

/// Supported image media types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum ImageMediaType {
    #[serde(rename = "image/jpeg")]
    Jpeg,

    #[serde(rename = "image/png")]
    Png,

    #[serde(rename = "image/gif")]
    Gif,

    #[serde(rename = "image/webp")]
    Webp,
}

fn default_type() -> String {
    "base64".to_string()
}

impl Base64ImageSource {
    /// Create a new Base64ImageSource from a base64-encoded string
    pub fn new(data: String, media_type: ImageMediaType) -> Self {
        Self {
            data,
            media_type,
            r#type: "base64".to_string(),
        }
    }

    /// Create a Base64ImageSource from a file path
    ///
    /// This will read the file and encode it as base64.
    /// The media_type will be determined from the file extension if possible.
    pub fn from_path<P: AsRef<Path>>(path: P) -> Result<Self, std::io::Error> {
        let path = path.as_ref();

        // Try to determine media type from extension
        let media_type = match path.extension().and_then(|ext| ext.to_str()) {
            Some("jpg") | Some("jpeg") => ImageMediaType::Jpeg,
            Some("png") => ImageMediaType::Png,
            Some("gif") => ImageMediaType::Gif,
            Some("webp") => ImageMediaType::Webp,
            _ => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "Unsupported file extension. Must be jpeg, png, gif, or webp",
                ));
            }
        };

        // Read the file
        let mut file = File::open(path)?;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)?;

        // Encode as base64
        let data = base64::engine::general_purpose::STANDARD.encode(&buffer);

        Ok(Self {
            data,
            media_type,
            r#type: "base64".to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialization() {
        let source = Base64ImageSource {
            data: "SGVsbG8gV29ybGQ=".to_string(), // "Hello World" in base64
            media_type: ImageMediaType::Jpeg,
            r#type: "base64".to_string(),
        };

        let json = serde_json::to_string(&source).unwrap();
        let expected = r#"{"data":"SGVsbG8gV29ybGQ=","media_type":"image/jpeg","type":"base64"}"#;

        assert_eq!(json, expected);
    }

    #[test]
    fn test_deserialization() {
        let json = r#"{"data":"SGVsbG8gV29ybGQ=","media_type":"image/png","type":"base64"}"#;
        let source: Base64ImageSource = serde_json::from_str(json).unwrap();

        assert_eq!(source.data, "SGVsbG8gV29ybGQ=");
        matches!(source.media_type, ImageMediaType::Png);
        assert_eq!(source.r#type, "base64");
    }
}
