//! Accumulates streaming events into a complete message while passing events through.

use std::pin::Pin;

use futures::Stream;
use serde_json::Value;

use crate::{
    CacheControlEphemeral, Citation, ContentBlock, ContentBlockDelta, Error, Message,
    MessageStreamEvent, ServerToolUseBlock, StopReason, TextBlock, TextCitation, ThinkingBlock,
    ToolUseBlock,
};

/// A stream wrapper that accumulates `MessageStreamEvent`s into a complete `Message`.
///
/// This allows streaming tokens to the user while simultaneously building the final message
/// without buffering. When the stream is fully drained, the accumulated message is sent via
/// the oneshot channel returned by `new()`.
pub struct AccumulatingStream {
    inner: Pin<Box<dyn Stream<Item = Result<MessageStreamEvent, Error>> + Send>>,
    message_tx: Option<tokio::sync::oneshot::Sender<Result<Message, Error>>>,
    message: Option<Message>,
    content_blocks: Vec<ContentBlockBuilder>,
}

impl AccumulatingStream {
    /// Wraps a `MessageStreamEvent` stream to accumulate events into a `Message`.
    ///
    /// Returns the stream and a receiver that will contain the accumulated `Message` once the
    /// stream is fully drained.
    pub fn new<S>(stream: S) -> (Self, tokio::sync::oneshot::Receiver<Result<Message, Error>>)
    where
        S: Stream<Item = Result<MessageStreamEvent, Error>> + Send + 'static,
    {
        Self::new_with_message(stream, None)
    }

    /// Wraps a `MessageStreamEvent` stream and seeds accumulation with a fallback message.
    pub fn new_with_message<S>(
        stream: S,
        message: impl Into<Option<Message>>,
    ) -> (Self, tokio::sync::oneshot::Receiver<Result<Message, Error>>)
    where
        S: Stream<Item = Result<MessageStreamEvent, Error>> + Send + 'static,
    {
        let (tx, rx) = tokio::sync::oneshot::channel();
        let this = Self {
            inner: Box::pin(stream),
            message_tx: Some(tx),
            message: message.into(),
            content_blocks: Vec::new(),
        };
        (this, rx)
    }

    fn accumulate_event(&mut self, event: &MessageStreamEvent) {
        match event {
            MessageStreamEvent::MessageStart(start) => {
                self.message = Some(start.message.clone());
            }
            MessageStreamEvent::ContentBlockStart(start) => {
                let idx = start.index;
                while self.content_blocks.len() <= idx {
                    self.content_blocks.push(ContentBlockBuilder::Empty);
                }
                self.content_blocks[idx] =
                    ContentBlockBuilder::from_content_block(start.content_block.clone());
            }
            MessageStreamEvent::ContentBlockDelta(delta_event) => {
                let idx = delta_event.index;
                if idx < self.content_blocks.len() {
                    self.content_blocks[idx].apply_delta(delta_event.delta.clone());
                }
            }
            MessageStreamEvent::ContentBlockStop(_) => {}
            MessageStreamEvent::MessageDelta(delta_event) => {
                if let Some(ref mut msg) = self.message {
                    if delta_event.delta.stop_reason.is_some() {
                        msg.stop_reason = delta_event.delta.stop_reason;
                    }
                    if delta_event.delta.stop_sequence.is_some() {
                        msg.stop_sequence = delta_event.delta.stop_sequence.clone();
                    }
                    if let Some(input_tokens) = delta_event.usage.input_tokens {
                        msg.usage.input_tokens = input_tokens;
                    }
                    msg.usage.output_tokens = delta_event.usage.output_tokens;
                    if let Some(cache) = delta_event.usage.cache_creation_input_tokens {
                        msg.usage.cache_creation_input_tokens = Some(cache);
                    }
                    if let Some(cache_read) = delta_event.usage.cache_read_input_tokens {
                        msg.usage.cache_read_input_tokens = Some(cache_read);
                    }
                    if let Some(server_tool) = delta_event.usage.server_tool_use {
                        msg.usage.server_tool_use = Some(server_tool);
                    }
                }
            }
            MessageStreamEvent::MessageStop(_) => {}
            MessageStreamEvent::Ping => {}
        }
    }

    fn finalize(&mut self) -> Result<Message, Error> {
        let mut msg = self
            .message
            .take()
            .ok_or_else(|| Error::streaming("stream ended without a message start event", None))?;
        let mut blocks = Vec::new();
        for builder in std::mem::take(&mut self.content_blocks) {
            if let Some(block) = builder.build(msg.stop_reason)? {
                blocks.push(block);
            }
        }
        msg.content = blocks;
        Ok(msg)
    }

    /// Finalizes the currently accumulated message without draining the stream.
    pub fn finalize_partial(&mut self) -> Result<Message, Error> {
        self.message_tx.take();
        self.finalize()
    }
}

impl Stream for AccumulatingStream {
    type Item = Result<MessageStreamEvent, Error>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        match self.inner.as_mut().poll_next(cx) {
            std::task::Poll::Ready(Some(Ok(event))) => {
                self.accumulate_event(&event);
                std::task::Poll::Ready(Some(Ok(event)))
            }
            std::task::Poll::Ready(Some(Err(e))) => std::task::Poll::Ready(Some(Err(e))),
            std::task::Poll::Ready(None) => {
                if let Some(tx) = self.message_tx.take() {
                    let _ = tx.send(self.finalize());
                }
                std::task::Poll::Ready(None)
            }
            std::task::Poll::Pending => std::task::Poll::Pending,
        }
    }
}

enum ContentBlockBuilder {
    Empty,
    Text {
        text: String,
        citations: Option<Vec<TextCitation>>,
        cache_control: Option<CacheControlEphemeral>,
    },
    ToolUse {
        id: String,
        name: String,
        input_json: String,
        input_value: Option<Value>,
        saw_delta: bool,
        cache_control: Option<CacheControlEphemeral>,
    },
    ServerToolUse {
        id: String,
        name: String,
        input: Value,
        cache_control: Option<CacheControlEphemeral>,
    },
    Thinking {
        thinking: String,
        signature: String,
    },
    Complete(ContentBlock),
}

impl ContentBlockBuilder {
    fn from_content_block(block: ContentBlock) -> Self {
        match block {
            ContentBlock::Text(text_block) => ContentBlockBuilder::Text {
                text: text_block.text,
                citations: text_block.citations,
                cache_control: text_block.cache_control,
            },
            ContentBlock::ToolUse(tool_use) => ContentBlockBuilder::ToolUse {
                id: tool_use.id,
                name: tool_use.name,
                input_json: String::new(),
                input_value: Some(tool_use.input),
                saw_delta: false,
                cache_control: tool_use.cache_control,
            },
            ContentBlock::ServerToolUse(server_tool_use) => ContentBlockBuilder::ServerToolUse {
                id: server_tool_use.id,
                name: server_tool_use.name,
                input: server_tool_use.input,
                cache_control: server_tool_use.cache_control,
            },
            ContentBlock::Thinking(thinking) => ContentBlockBuilder::Thinking {
                thinking: thinking.thinking,
                signature: thinking.signature,
            },
            other => ContentBlockBuilder::Complete(other),
        }
    }

    fn apply_delta(&mut self, delta: ContentBlockDelta) {
        match (self, delta) {
            (ContentBlockBuilder::Text { text, .. }, ContentBlockDelta::TextDelta(text_delta)) => {
                text.push_str(&text_delta.text);
            }
            (
                ContentBlockBuilder::Text { citations, .. },
                ContentBlockDelta::CitationsDelta(citations_delta),
            ) => {
                let citation = match citations_delta.citation {
                    Citation::CharLocation(loc) => TextCitation::CharLocation(loc),
                    Citation::PageLocation(loc) => TextCitation::PageLocation(loc),
                    Citation::ContentBlockLocation(loc) => TextCitation::ContentBlockLocation(loc),
                    Citation::WebSearchResultLocation(loc) => {
                        TextCitation::WebSearchResultLocation(loc)
                    }
                };
                citations.get_or_insert_with(Vec::new).push(citation);
            }
            (
                ContentBlockBuilder::ToolUse {
                    input_json,
                    saw_delta,
                    ..
                },
                ContentBlockDelta::InputJsonDelta(json_delta),
            ) => {
                *saw_delta = true;
                input_json.push_str(&json_delta.partial_json);
            }
            (
                ContentBlockBuilder::Thinking { thinking, .. },
                ContentBlockDelta::ThinkingDelta(thinking_delta),
            ) => {
                thinking.push_str(&thinking_delta.thinking);
            }
            (
                ContentBlockBuilder::Thinking { signature, .. },
                ContentBlockDelta::SignatureDelta(sig_delta),
            ) => {
                signature.push_str(&sig_delta.signature);
            }
            _ => {}
        }
    }

    fn build(self, stop_reason: Option<StopReason>) -> Result<Option<ContentBlock>, Error> {
        match self {
            ContentBlockBuilder::Empty => Ok(None),
            ContentBlockBuilder::Text {
                text,
                citations,
                cache_control,
            } => Ok(Some(ContentBlock::Text(TextBlock {
                text,
                citations,
                cache_control,
            }))),
            ContentBlockBuilder::ToolUse {
                id,
                name,
                input_json,
                input_value,
                saw_delta,
                cache_control,
            } => {
                let input = if saw_delta {
                    match serde_json::from_str::<Value>(&input_json) {
                        Ok(value) => value,
                        Err(err) => {
                            if stop_reason == Some(StopReason::MaxTokens) {
                                return Ok(None);
                            }
                            return Err(Error::serialization(
                                "failed to parse tool input JSON",
                                Some(Box::new(err)),
                            ));
                        }
                    }
                } else if let Some(input) = input_value {
                    input
                } else if input_json.is_empty() {
                    Value::Null
                } else {
                    match serde_json::from_str::<Value>(&input_json) {
                        Ok(value) => value,
                        Err(err) => {
                            if stop_reason == Some(StopReason::MaxTokens) {
                                return Ok(None);
                            }
                            return Err(Error::serialization(
                                "failed to parse tool input JSON",
                                Some(Box::new(err)),
                            ));
                        }
                    }
                };
                Ok(Some(ContentBlock::ToolUse(ToolUseBlock {
                    id,
                    name,
                    input,
                    cache_control,
                })))
            }
            ContentBlockBuilder::ServerToolUse {
                id,
                name,
                input,
                cache_control,
            } => Ok(Some(ContentBlock::ServerToolUse(ServerToolUseBlock {
                id,
                name,
                input,
                cache_control,
            }))),
            ContentBlockBuilder::Thinking {
                thinking,
                signature,
            } => Ok(Some(ContentBlock::Thinking(ThinkingBlock {
                thinking,
                signature,
            }))),
            ContentBlockBuilder::Complete(block) => Ok(Some(block)),
        }
    }
}
