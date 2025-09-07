//! Server-Sent Events (SSE) processing for streaming responses.
//!
//! This module handles parsing and processing of SSE streams from the Anthropic API,
//! converting raw byte streams into structured MessageStreamEvent objects.

use bytes::Bytes;
use futures::stream::{self, Stream, StreamExt};

use crate::{
    ContentBlockDeltaEvent, ContentBlockStartEvent, ContentBlockStopEvent, Error,
    MessageDeltaEvent, MessageStartEvent, MessageStopEvent, MessageStreamEvent, Result,
};

/// Process a stream of bytes into a stream of server-sent events.
///
/// This function takes a byte stream from an HTTP response and converts it into
/// a stream of parsed MessageStreamEvent objects, handling SSE parsing,
/// buffering, and error conditions.
pub fn process_sse<S>(byte_stream: S) -> impl Stream<Item = Result<MessageStreamEvent>>
where
    S: Stream<Item = std::result::Result<Bytes, reqwest::Error>> + Unpin + 'static,
{
    // Convert reqwest errors to our error type
    let stream = byte_stream.map(|result| {
        result
            .map_err(|e| Error::streaming(format!("Error in HTTP stream: {e}"), Some(Box::new(e))))
    });

    // Use a state machine to process the SSE stream
    let buffer = String::new();

    stream::unfold(
        (stream, buffer),
        move |(mut stream, mut buffer)| async move {
            loop {
                // First check if we have a complete event in the buffer
                if let Some((event, remaining)) = extract_event(&buffer) {
                    buffer = remaining;
                    return Some((event, (stream, buffer)));
                }

                // Read more data
                match stream.next().await {
                    Some(Ok(bytes)) => match String::from_utf8(bytes.to_vec()) {
                        Ok(text) => buffer.push_str(&text),
                        Err(e) => {
                            return Some((
                                Err(Error::encoding(
                                    format!("Invalid UTF-8 in stream: {e}"),
                                    Some(Box::new(e)),
                                )),
                                (stream, buffer),
                            ));
                        }
                    },
                    Some(Err(e)) => {
                        return Some((Err(e), (stream, buffer)));
                    }
                    None => {
                        // End of stream
                        if !buffer.is_empty() {
                            if let Some((event, _)) = extract_event(&buffer) {
                                return Some((event, (stream, buffer)));
                            }
                        }
                        return None;
                    }
                }
            }
        },
    )
}

/// Extract a complete SSE event from a buffer string.
///
/// Parses SSE format where events are delimited by double newlines and
/// each event has an event type line followed by a data line.
fn extract_event(buffer: &str) -> Option<(Result<MessageStreamEvent>, String)> {
    // Simple SSE parsing - each event is delimited by double newlines
    let parts: Vec<&str> = buffer.splitn(2, "\n\n").collect();
    if parts.len() != 2 {
        return None;
    }
    let event_text = parts[0];
    let rest = parts[1].to_string();

    // Parse event type and data
    let Some((event_type, event_data)) = event_text.split_once('\n') else {
        return Some((
            Err(Error::serialization(
                format!("Malformed SSE event: missing newline separator in '{event_text}'"),
                None,
            )),
            rest,
        ));
    };

    let Some(event_data) = event_data.strip_prefix("data:").map(str::trim) else {
        return Some((
            Err(Error::serialization(
                format!("Malformed SSE event: missing 'data:' prefix in '{event_data}'"),
                None,
            )),
            rest,
        ));
    };

    // Parse specific event types
    parse_event_type(event_type, event_data, rest)
}

/// Parse a specific SSE event type and its data.
fn parse_event_type(
    event_type: &str,
    event_data: &str,
    rest: String,
) -> Option<(Result<MessageStreamEvent>, String)> {
    match event_type {
        "event: ping" => Some((Ok(MessageStreamEvent::Ping), rest)),

        "event: message_start" => match serde_json::from_str::<MessageStartEvent>(event_data) {
            Ok(event) => Some((Ok(MessageStreamEvent::MessageStart(event)), rest)),
            Err(e) => Some((Err(e.into()), rest)),
        },

        "event: message_delta" => match serde_json::from_str::<MessageDeltaEvent>(event_data) {
            Ok(event) => Some((Ok(MessageStreamEvent::MessageDelta(event)), rest)),
            Err(e) => Some((Err(e.into()), rest)),
        },

        "event: message_stop" => match serde_json::from_str::<MessageStopEvent>(event_data) {
            Ok(event) => Some((Ok(MessageStreamEvent::MessageStop(event)), rest)),
            Err(e) => Some((Err(e.into()), rest)),
        },

        "event: content_block_start" => {
            match serde_json::from_str::<ContentBlockStartEvent>(event_data) {
                Ok(event) => Some((Ok(MessageStreamEvent::ContentBlockStart(event)), rest)),
                Err(e) => Some((Err(e.into()), rest)),
            }
        }

        "event: content_block_delta" => {
            match serde_json::from_str::<ContentBlockDeltaEvent>(event_data) {
                Ok(event) => Some((Ok(MessageStreamEvent::ContentBlockDelta(event)), rest)),
                Err(e) => Some((Err(e.into()), rest)),
            }
        }

        "event: content_block_stop" => {
            match serde_json::from_str::<ContentBlockStopEvent>(event_data) {
                Ok(event) => Some((Ok(MessageStreamEvent::ContentBlockStop(event)), rest)),
                Err(e) => Some((Err(e.into()), rest)),
            }
        }

        "event: error" => {
            // Parse error event - the data should contain error details
            Some((
                Err(Error::api(
                    500,
                    Some("stream_error".to_string()),
                    event_data.to_string(),
                    None,
                )),
                rest,
            ))
        }

        _ => Some((
            Err(Error::serialization(
                format!("Unknown SSE event type: {event_type}"),
                None,
            )),
            rest,
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;

    #[tokio::test]
    async fn parse_ping_event() {
        let data = b"event: ping\ndata: {}\n\n";
        let stream = Box::pin(stream::once(async { Ok(Bytes::from(&data[..])) }));

        let mut sse_stream = Box::pin(process_sse(stream));
        let event = sse_stream.next().await.unwrap();

        assert!(matches!(event, Ok(MessageStreamEvent::Ping)));
    }

    #[tokio::test]
    async fn parse_multiple_events() {
        let data = b"event: ping\ndata: {}\n\nevent: ping\ndata: {}\n\n";
        let stream = Box::pin(stream::once(async { Ok(Bytes::from(&data[..])) }));

        let mut sse_stream = Box::pin(process_sse(stream));

        let event1 = sse_stream.next().await.unwrap();
        assert!(matches!(event1, Ok(MessageStreamEvent::Ping)));

        let event2 = sse_stream.next().await.unwrap();
        assert!(matches!(event2, Ok(MessageStreamEvent::Ping)));
    }

    #[tokio::test]
    async fn handle_malformed_event() {
        let data = b"malformed data without proper format\n\n";
        let stream = Box::pin(stream::once(async { Ok(Bytes::from(&data[..])) }));

        let mut sse_stream = Box::pin(process_sse(stream));
        let event = sse_stream.next().await.unwrap();

        assert!(event.is_err());
    }

    #[tokio::test]
    async fn handle_split_event() {
        // Simulate an event split across multiple chunks
        let chunk1 = b"event: ping\n";
        let chunk2 = b"data: {}\n\n";

        let stream = Box::pin(stream::iter(vec![
            Ok(Bytes::from(&chunk1[..])),
            Ok(Bytes::from(&chunk2[..])),
        ]));

        let mut sse_stream = Box::pin(process_sse(stream));
        let event = sse_stream.next().await.unwrap();

        assert!(matches!(event, Ok(MessageStreamEvent::Ping)));
    }

    #[tokio::test]
    async fn handle_unknown_event_type() {
        let data = b"event: unknown_event\ndata: {}\n\n";
        let stream = Box::pin(stream::once(async { Ok(Bytes::from(&data[..])) }));

        let mut sse_stream = Box::pin(process_sse(stream));
        let event = sse_stream.next().await.unwrap();

        assert!(event.is_err());
        if let Err(e) = event {
            assert!(e.to_string().contains("Unknown SSE event type"));
        }
    }
}
