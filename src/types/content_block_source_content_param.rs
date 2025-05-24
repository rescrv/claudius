use serde::{Deserialize, Serialize};

use crate::types::{ImageBlock, TextBlock};

/// Parameter for a content block source content, which can be either a text block or an image block.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum ContentBlockSourceContentParam {
    /// A text block source content.
    #[serde(rename = "text")]
    Text(TextBlock),

    /// An image block source content.
    #[serde(rename = "image")]
    Image(ImageBlock),
}

impl From<TextBlock> for ContentBlockSourceContentParam {
    fn from(param: TextBlock) -> Self {
        ContentBlockSourceContentParam::Text(param)
    }
}

impl From<ImageBlock> for ContentBlockSourceContentParam {
    fn from(param: ImageBlock) -> Self {
        ContentBlockSourceContentParam::Image(param)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::base64_image_source::{Base64ImageSource, ImageMediaType};
    use crate::types::UrlImageSource;
    use serde_json::{json, to_value};

    #[test]
    fn test_content_block_source_content_param_text() {
        let text_param = TextBlock::new("Sample text content".to_string());
        let content_param = ContentBlockSourceContentParam::Text(text_param);

        let json = to_value(&content_param).unwrap();

        assert_eq!(
            json,
            json!({
                "text": "Sample text content",
                "type": "text"
            })
        );
    }

    #[test]
    fn test_content_block_source_content_param_image() {
        let url_source = UrlImageSource::new("https://example.com/image.jpg".to_string());
        let image_param = ImageBlock::new_with_url(url_source);
        let content_param = ContentBlockSourceContentParam::Image(image_param);

        let json = to_value(&content_param).unwrap();

        assert_eq!(
            json,
            json!({
                "source": {
                    "type": "url",
                    "url": "https://example.com/image.jpg"
                },
                "type": "image"
            })
        );
    }

    #[test]
    fn test_from_text_block_param() {
        let text_param = TextBlock::new("Sample text content".to_string());
        let content_param: ContentBlockSourceContentParam = text_param.into();

        match content_param {
            ContentBlockSourceContentParam::Text(param) => {
                assert_eq!(param.text, "Sample text content");
            }
            _ => panic!("Expected Text variant"),
        }
    }

    #[test]
    fn test_from_image_block_param() {
        let base64_source = Base64ImageSource::new(
            "data:image/jpeg;base64,SGVsbG8gd29ybGQ=".to_string(),
            ImageMediaType::Jpeg,
        );
        let image_param = ImageBlock::new_with_base64(base64_source);
        let content_param: ContentBlockSourceContentParam = image_param.into();

        match content_param {
            ContentBlockSourceContentParam::Image(_) => {
                // Test passes if we got the Image variant
            }
            _ => panic!("Expected Image variant"),
        }
    }
}
