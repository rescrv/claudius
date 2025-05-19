use serde::{Deserialize, Serialize};

use crate::types::{Base64ImageSource, CacheControlEphemeral, UrlImageSource};

/// The source type for an image block, which can be either Base64 encoded or a URL.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum ImageSource {
    /// A Base64 encoded image source.
    Base64(Base64ImageSource),

    /// A URL image source.
    Url(UrlImageSource),
}

/// Parameters for an image block.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ImageBlockParam {
    /// The source of the image.
    pub source: ImageSource,

    /// The type, which is always "image".
    pub r#type: String,

    /// Create a cache control breakpoint at this content block.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_control: Option<CacheControlEphemeral>,
}

impl ImageBlockParam {
    /// Create a new `ImageBlockParam` with the given source.
    pub fn new(source: ImageSource) -> Self {
        Self {
            source,
            r#type: "image".to_string(),
            cache_control: None,
        }
    }

    /// Create a new `ImageBlockParam` with a Base64 image source.
    pub fn new_with_base64(source: Base64ImageSource) -> Self {
        Self::new(ImageSource::Base64(source))
    }

    /// Create a new `ImageBlockParam` with a URL image source.
    pub fn new_with_url(source: UrlImageSource) -> Self {
        Self::new(ImageSource::Url(source))
    }

    /// Add a cache control to this image block.
    pub fn with_cache_control(mut self, cache_control: CacheControlEphemeral) -> Self {
        self.cache_control = Some(cache_control);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::base64_image_source::ImageMediaType;
    use serde_json::{json, to_value};

    #[test]
    fn test_image_block_param_with_base64() {
        let base64_source = Base64ImageSource::new(
            "data:image/jpeg;base64,SGVsbG8gd29ybGQ=".to_string(),
            ImageMediaType::Jpeg,
        );

        let image_block = ImageBlockParam::new_with_base64(base64_source);
        let json = to_value(&image_block).unwrap();

        assert_eq!(
            json,
            json!({
                "source": {
                    "data": "data:image/jpeg;base64,SGVsbG8gd29ybGQ=",
                    "type": "base64"
                },
                "type": "image"
            })
        );
    }

    #[test]
    fn test_image_block_param_with_url() {
        let url_source = UrlImageSource::new("https://example.com/image.jpg".to_string());

        let image_block = ImageBlockParam::new_with_url(url_source);
        let json = to_value(&image_block).unwrap();

        assert_eq!(
            json,
            json!({
                "source": {
                    "url": "https://example.com/image.jpg",
                    "type": "url"
                },
                "type": "image"
            })
        );
    }

    #[test]
    fn test_image_block_param_with_cache_control() {
        let url_source = UrlImageSource::new("https://example.com/image.jpg".to_string());
        let cache_control = CacheControlEphemeral::new();

        let image_block =
            ImageBlockParam::new_with_url(url_source).with_cache_control(cache_control);

        let json = to_value(&image_block).unwrap();

        assert_eq!(
            json,
            json!({
                "source": {
                    "url": "https://example.com/image.jpg",
                    "type": "url"
                },
                "type": "image",
                "cache_control": {
                    "type": "ephemeral"
                }
            })
        );
    }
}
