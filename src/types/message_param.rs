use serde::{Deserialize, Serialize};

use crate::types::ContentBlock;

/// The content of a message, which can be either a string or an array of content blocks.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum MessageParamContent {
    /// A simple string content.
    String(String),

    /// An array of content blocks.
    Array(Vec<ContentBlock>),
}

/// Parameters for a message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MessageParam {
    /// The content of the message.
    pub content: MessageParamContent,

    /// The role of the message.
    pub role: MessageRole,
}

/// Role type for a message parameter.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    /// User role.
    User,

    /// Assistant role.
    Assistant,
}

impl MessageParam {
    /// Create a new `MessageParam` with the given content and role.
    pub fn new(content: MessageParamContent, role: MessageRole) -> Self {
        Self { content, role }
    }

    /// Create a new `MessageParam` with a string content.
    pub fn new_with_string(content: String, role: MessageRole) -> Self {
        Self::new(MessageParamContent::String(content), role)
    }

    /// Create a new `MessageParam` with an array of content blocks.
    pub fn new_with_blocks(blocks: Vec<ContentBlock>, role: MessageRole) -> Self {
        Self::new(MessageParamContent::Array(blocks), role)
    }

    /// Create a new user `MessageParam` with a string content.
    pub fn user(content: String) -> Self {
        Self::new_with_string(content, MessageRole::User)
    }

    /// Create a new assistant `MessageParam` with a string content.
    pub fn assistant(content: String) -> Self {
        Self::new_with_string(content, MessageRole::Assistant)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ImageBlock, TextBlock};
    use serde_json::{json, to_value};

    #[test]
    fn message_param_with_string() {
        let message = MessageParam::user("Hello, Claude!".to_string());
        let json = to_value(&message).unwrap();

        assert_eq!(
            json,
            json!({
                "content": "Hello, Claude!",
                "role": "user"
            })
        );
    }

    #[test]
    fn message_param_with_blocks() {
        let text_block = TextBlock::new("Hello, Claude!".to_string());
        let blocks = vec![ContentBlock::Text(text_block)];

        let message = MessageParam::new_with_blocks(blocks, MessageRole::User);
        let json = to_value(&message).unwrap();

        assert_eq!(
            json,
            json!({
                "content": [
                    {
                        "text": "Hello, Claude!",
                        "type": "text"
                    }
                ],
                "role": "user"
            })
        );
    }

    #[test]
    fn message_param_with_mixed_blocks() {
        let text_block = TextBlock::new("Check out this image:".to_string());

        let image_source =
            crate::types::UrlImageSource::new("https://example.com/image.jpg".to_string());
        let image_block = ImageBlock::new_with_url(image_source);

        let blocks = vec![
            ContentBlock::Text(text_block),
            ContentBlock::Image(image_block),
        ];

        let message = MessageParam::new_with_blocks(blocks, MessageRole::User);
        let json = to_value(&message).unwrap();

        assert_eq!(
            json,
            json!({
                "content": [
                    {
                        "text": "Check out this image:",
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
                "role": "user"
            })
        );
    }

    #[test]
    fn message_param_deserialization() {
        let json = json!({
            "content": "Hello, Claude!",
            "role": "user"
        });

        let message: MessageParam = serde_json::from_value(json).unwrap();
        match message.content {
            MessageParamContent::String(s) => assert_eq!(s, "Hello, Claude!"),
            _ => panic!("Expected String variant"),
        }
        assert_eq!(message.role, MessageRole::User);

        let json = json!({
            "content": [
                {
                    "text": "Hello, Claude!",
                    "type": "text"
                }
            ],
            "role": "assistant"
        });

        let message: MessageParam = serde_json::from_value(json).unwrap();
        match message.content {
            MessageParamContent::Array(blocks) => {
                assert_eq!(blocks.len(), 1);
                match &blocks[0] {
                    ContentBlock::Text(text) => assert_eq!(text.text, "Hello, Claude!"),
                    _ => panic!("Expected Text variant"),
                }
            }
            _ => panic!("Expected Array variant"),
        }
        assert_eq!(message.role, MessageRole::Assistant);
    }
}
