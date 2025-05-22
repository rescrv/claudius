use serde::{Deserialize, Serialize};

use crate::types::{
    ContentBlock, DocumentBlockParam, ImageBlockParam, RedactedThinkingBlock,
    ServerToolUseBlockParam, TextBlock, ThinkingBlock, ToolResultBlockParam,
    ToolUseBlockParam, WebSearchToolResultBlockParam,
};

/// The content of a message, which can be either a string or an array of content blocks.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum MessageParamContent {
    /// A simple string content.
    String(String),

    /// An array of content blocks.
    Array(Vec<MessageContentBlock>),
}

/// A content block that can be part of a message parameter.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum MessageContentBlock {
    /// A text block parameter.
    #[serde(rename = "text")]
    Text(TextBlock),

    /// An image block parameter.
    #[serde(rename = "image")]
    Image(ImageBlockParam),

    /// A tool use block parameter.
    #[serde(rename = "tool_use")]
    ToolUse(ToolUseBlockParam),

    /// A server tool use block parameter.
    #[serde(rename = "server_tool_use")]
    ServerToolUse(ServerToolUseBlockParam),

    /// A web search tool result block parameter.
    #[serde(rename = "web_search_tool_result")]
    WebSearchToolResult(WebSearchToolResultBlockParam),

    /// A tool result block parameter.
    #[serde(rename = "tool_result")]
    ToolResult(ToolResultBlockParam),

    /// A document block parameter.
    #[serde(rename = "document")]
    Document(DocumentBlockParam),

    /// A thinking block parameter.
    #[serde(rename = "thinking")]
    Thinking(ThinkingBlock),

    /// A redacted thinking block parameter.
    #[serde(rename = "redacted_thinking")]
    RedactedThinking(RedactedThinkingBlock),

    /// A content block (for backward compatibility).
    #[serde(rename = "content_block")]
    ContentBlock(ContentBlock),
}

/// Parameters for a message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MessageParam {
    /// The content of the message.
    pub content: MessageParamContent,

    /// The role of the message, which is either "user" or "assistant".
    // TODO(claude): Convert this to MessageRole.
    pub role: String,
}

/// Role type for a message parameter.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MessageRole {
    /// User role.
    User,

    /// Assistant role.
    Assistant,
}

impl From<MessageRole> for String {
    fn from(role: MessageRole) -> Self {
        match role {
            MessageRole::User => "user".to_string(),
            MessageRole::Assistant => "assistant".to_string(),
        }
    }
}

impl MessageParam {
    /// Create a new `MessageParam` with the given content and role.
    pub fn new(content: MessageParamContent, role: MessageRole) -> Self {
        Self {
            content,
            role: role.into(),
        }
    }

    /// Create a new `MessageParam` with a string content.
    pub fn new_with_string(content: String, role: MessageRole) -> Self {
        Self::new(MessageParamContent::String(content), role)
    }

    /// Create a new `MessageParam` with an array of content blocks.
    pub fn new_with_blocks(blocks: Vec<MessageContentBlock>, role: MessageRole) -> Self {
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

impl From<TextBlock> for MessageContentBlock {
    fn from(param: TextBlock) -> Self {
        MessageContentBlock::Text(param)
    }
}

impl From<ImageBlockParam> for MessageContentBlock {
    fn from(param: ImageBlockParam) -> Self {
        MessageContentBlock::Image(param)
    }
}

impl From<ToolUseBlockParam> for MessageContentBlock {
    fn from(param: ToolUseBlockParam) -> Self {
        MessageContentBlock::ToolUse(param)
    }
}

impl From<ServerToolUseBlockParam> for MessageContentBlock {
    fn from(param: ServerToolUseBlockParam) -> Self {
        MessageContentBlock::ServerToolUse(param)
    }
}

impl From<WebSearchToolResultBlockParam> for MessageContentBlock {
    fn from(param: WebSearchToolResultBlockParam) -> Self {
        MessageContentBlock::WebSearchToolResult(param)
    }
}

impl From<ToolResultBlockParam> for MessageContentBlock {
    fn from(param: ToolResultBlockParam) -> Self {
        MessageContentBlock::ToolResult(param)
    }
}

impl From<DocumentBlockParam> for MessageContentBlock {
    fn from(param: DocumentBlockParam) -> Self {
        MessageContentBlock::Document(param)
    }
}

impl From<ThinkingBlock> for MessageContentBlock {
    fn from(param: ThinkingBlock) -> Self {
        MessageContentBlock::Thinking(param)
    }
}

impl From<RedactedThinkingBlock> for MessageContentBlock {
    fn from(param: RedactedThinkingBlock) -> Self {
        MessageContentBlock::RedactedThinking(param)
    }
}

impl From<ContentBlock> for MessageContentBlock {
    fn from(block: ContentBlock) -> Self {
        MessageContentBlock::ContentBlock(block)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, to_value};

    #[test]
    fn test_message_param_with_string() {
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
    fn test_message_param_with_blocks() {
        let text_block = TextBlock::new("Hello, Claude!".to_string());
        let blocks = vec![MessageContentBlock::Text(text_block)];

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
    fn test_message_param_with_mixed_blocks() {
        let text_block = TextBlock::new("Check out this image:".to_string());

        let image_source =
            crate::types::UrlImageSource::new("https://example.com/image.jpg".to_string());
        let image_block = ImageBlockParam::new_with_url(image_source);

        let blocks = vec![
            MessageContentBlock::Text(text_block),
            MessageContentBlock::Image(image_block),
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
    fn test_message_param_deserialization() {
        let json = json!({
            "content": "Hello, Claude!",
            "role": "user"
        });

        let message: MessageParam = serde_json::from_value(json).unwrap();
        match message.content {
            MessageParamContent::String(s) => assert_eq!(s, "Hello, Claude!"),
            _ => panic!("Expected String variant"),
        }
        assert_eq!(message.role, "user");

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
                    MessageContentBlock::Text(text) => assert_eq!(text.text, "Hello, Claude!"),
                    _ => panic!("Expected Text variant"),
                }
            }
            _ => panic!("Expected Array variant"),
        }
        assert_eq!(message.role, "assistant");
    }
}
