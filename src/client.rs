use bytes::Bytes;
use futures::Stream;
use futures::stream::{self, StreamExt};
use reqwest::header::{HeaderMap, HeaderValue};
use reqwest::{Client as ReqwestClient, Response, header};
use serde::Deserialize;
use std::env;
use std::pin::Pin;
use std::time::Duration;

use crate::error::{Error, Result};
use crate::types::{Message, MessageCreateParams, MessageStreamEvent, RawMessageStreamEvent};

const DEFAULT_API_URL: &str = "https://api.anthropic.com/v1/";
const ANTHROPIC_API_VERSION: &str = "2023-06-01";
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);

/// Client for the Anthropic API.
#[derive(Debug, Clone)]
pub struct Anthropic {
    api_key: String,
    client: ReqwestClient,
    base_url: String,
    timeout: Duration,
}

impl Anthropic {
    /// Create a new Anthropic client.
    ///
    /// The API key can be provided directly or read from the CLAUDIUS_API_KEY
    /// environment variable.
    pub fn new(api_key: Option<String>) -> Result<Self> {
        let api_key = match api_key {
            Some(key) => key,
            None => env::var("CLAUDIUS_API_KEY").map_err(|_| {
                Error::authentication(
                    "API key not provided and CLAUDIUS_API_KEY environment variable not set",
                )
            })?,
        };

        let timeout = DEFAULT_TIMEOUT;
        let client = ReqwestClient::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| {
                Error::http_client(
                    format!("Failed to build HTTP client: {}", e),
                    Some(Box::new(e)),
                )
            })?;

        Ok(Self {
            api_key,
            client,
            base_url: DEFAULT_API_URL.to_string(),
            timeout,
        })
    }

    /// Create a new client with custom settings.
    pub fn with_options(
        api_key: Option<String>,
        base_url: Option<String>,
        timeout: Option<Duration>,
    ) -> Result<Self> {
        let api_key = match api_key {
            Some(key) => key,
            None => env::var("CLAUDIUS_API_KEY").map_err(|_| {
                Error::authentication(
                    "API key not provided and CLAUDIUS_API_KEY environment variable not set",
                )
            })?,
        };

        let timeout = timeout.unwrap_or(DEFAULT_TIMEOUT);
        let client = ReqwestClient::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| {
                Error::http_client(
                    format!("Failed to build HTTP client: {}", e),
                    Some(Box::new(e)),
                )
            })?;

        Ok(Self {
            api_key,
            client,
            base_url: base_url.unwrap_or_else(|| DEFAULT_API_URL.to_string()),
            timeout,
        })
    }

    /// Create and return default headers for API requests.
    fn default_headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        headers.insert(header::ACCEPT, HeaderValue::from_static("application/json"));
        headers.insert(
            "x-api-key",
            HeaderValue::from_str(&self.api_key).expect("API key should be valid"),
        );
        headers.insert(
            "anthropic-version",
            HeaderValue::from_static(ANTHROPIC_API_VERSION),
        );
        headers
    }

    /// Process API response errors and convert to our Error type
    async fn process_error_response(response: Response) -> Error {
        let status = response.status();
        let status_code = status.as_u16();

        // Get headers we might need for error processing
        let request_id = response
            .headers()
            .get("x-request-id")
            .and_then(|val| val.to_str().ok())
            .map(String::from);

        let retry_after = response
            .headers()
            .get("retry-after")
            .and_then(|val| val.to_str().ok())
            .and_then(|val| val.parse::<u64>().ok());

        // Try to parse error response body
        #[derive(Deserialize)]
        struct ErrorResponse {
            error: Option<ErrorDetail>,
        }

        #[derive(Deserialize)]
        struct ErrorDetail {
            #[serde(rename = "type")]
            error_type: Option<String>,
            message: Option<String>,
            param: Option<String>,
        }

        let error_body = match response.text().await {
            Ok(body) => body,
            Err(e) => {
                return Error::http_client(
                    format!("Failed to read error response: {}", e),
                    Some(Box::new(e)),
                );
            }
        };

        // Try to parse as JSON first
        let parsed_error = serde_json::from_str::<ErrorResponse>(&error_body).ok();
        let error_type = parsed_error
            .as_ref()
            .and_then(|e| e.error.as_ref())
            .and_then(|e| e.error_type.clone());
        let error_message = parsed_error
            .as_ref()
            .and_then(|e| e.error.as_ref())
            .and_then(|e| e.message.clone())
            .unwrap_or_else(|| error_body.clone());
        let error_param = parsed_error
            .as_ref()
            .and_then(|e| e.error.as_ref())
            .and_then(|e| e.param.clone());

        // Map HTTP status code to appropriate error type
        match status_code {
            400 => Error::bad_request(error_message, error_param),
            401 => Error::authentication(error_message),
            403 => Error::permission(error_message),
            404 => Error::not_found(error_message, None, None),
            408 => Error::timeout(error_message, None),
            429 => Error::rate_limit(error_message, retry_after),
            500 => Error::internal_server(error_message, request_id),
            502..=504 => Error::service_unavailable(error_message, retry_after),
            _ => Error::api(status_code, error_type, error_message, request_id),
        }
    }

    /// Send a message to the API and get a non-streaming response.
    pub async fn send(&self, params: MessageCreateParams) -> Result<Message> {
        let url = format!("{}messages", self.base_url);

        let response = self
            .client
            .post(&url)
            .headers(self.default_headers())
            .json(&params)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    Error::timeout(
                        format!("Request timed out: {}", e),
                        Some(self.timeout.as_secs_f64()),
                    )
                } else if e.is_connect() {
                    Error::connection(format!("Connection error: {}", e), Some(Box::new(e)))
                } else {
                    Error::http_client(format!("Request failed: {}", e), Some(Box::new(e)))
                }
            })?;

        if !response.status().is_success() {
            return Err(Self::process_error_response(response).await);
        }

        response.json::<Message>().await.map_err(|e| {
            Error::serialization(
                format!("Failed to parse response: {}", e),
                Some(Box::new(e)),
            )
        })
    }

    /// Send a message to the API and get a streaming response.
    ///
    /// Returns a stream of MessageStreamEvent objects that can be processed incrementally.
    pub async fn stream(
        &self,
        mut params: MessageCreateParams,
    ) -> Result<impl Stream<Item = Result<MessageStreamEvent>>> {
        // Ensure stream is enabled based on the variant
        match &mut params {
            MessageCreateParams::NonStreaming(p) => p.stream = true,
            MessageCreateParams::Streaming(p) => p.stream = true,
        }

        let url = format!("{}messages", self.base_url);

        let mut headers = self.default_headers();
        headers.insert(
            header::ACCEPT,
            HeaderValue::from_static("text/event-stream"),
        );

        let response = self
            .client
            .post(&url)
            .headers(headers)
            .json(&params)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    Error::timeout(
                        format!("Request timed out: {}", e),
                        Some(self.timeout.as_secs_f64()),
                    )
                } else if e.is_connect() {
                    Error::connection(format!("Connection error: {}", e), Some(Box::new(e)))
                } else {
                    Error::http_client(format!("Request failed: {}", e), Some(Box::new(e)))
                }
            })?;

        if !response.status().is_success() {
            return Err(Self::process_error_response(response).await);
        }

        // Get the byte stream from the response
        let stream = response.bytes_stream();

        // Create an SSE processor
        let event_stream = process_sse(stream);

        Ok(event_stream)
    }

    /// Send a message to the API and get a raw streaming response.
    ///
    /// This method provides access to the lower-level RawMessageStreamEvent objects
    /// directly from the server-sent events stream. This is useful for advanced use
    /// cases where you need more control over the streaming response processing.
    ///
    /// Returns a stream of RawMessageStreamEvent objects that can be processed incrementally.
    pub async fn stream_raw(
        &self,
        mut params: MessageCreateParams,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<RawMessageStreamEvent>> + Send>>> {
        // Ensure stream is enabled based on the variant
        match &mut params {
            MessageCreateParams::NonStreaming(p) => p.stream = true,
            MessageCreateParams::Streaming(p) => p.stream = true,
        }

        let url = format!("{}messages", self.base_url);

        let mut headers = self.default_headers();
        headers.insert(
            header::ACCEPT,
            HeaderValue::from_static("text/event-stream"),
        );

        let response = self
            .client
            .post(&url)
            .headers(headers)
            .json(&params)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    Error::timeout(
                        format!("Request timed out: {}", e),
                        Some(self.timeout.as_secs_f64()),
                    )
                } else if e.is_connect() {
                    Error::connection(format!("Connection error: {}", e), Some(Box::new(e)))
                } else {
                    Error::http_client(format!("Request failed: {}", e), Some(Box::new(e)))
                }
            })?;

        if !response.status().is_success() {
            return Err(Self::process_error_response(response).await);
        }

        // Get the byte stream from the response
        let stream = response.bytes_stream();

        // Create an SSE processor for raw events
        let event_stream = process_raw_sse(stream);

        Ok(Box::pin(event_stream))
    }
}

/// Process a stream of bytes into a stream of server-sent events
fn process_sse<S>(byte_stream: S) -> impl Stream<Item = Result<MessageStreamEvent>>
where
    S: Stream<Item = std::result::Result<Bytes, reqwest::Error>> + Unpin + 'static,
{
    // Convert reqwest errors to our error type
    let stream = byte_stream.map(|result| {
        result.map_err(|e| {
            Error::streaming(format!("Error in HTTP stream: {}", e), Some(Box::new(e)))
        })
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
                                    format!("Invalid UTF-8 in stream: {}", e),
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

/// Process a stream of bytes into a stream of raw server-sent events
fn process_raw_sse<S>(byte_stream: S) -> impl Stream<Item = Result<RawMessageStreamEvent>>
where
    S: Stream<Item = std::result::Result<Bytes, reqwest::Error>> + Unpin + 'static,
{
    // Convert reqwest errors to our error type
    let stream = byte_stream.map(|result| {
        result.map_err(|e| {
            Error::streaming(format!("Error in HTTP stream: {}", e), Some(Box::new(e)))
        })
    });

    // Use a state machine to process the SSE stream
    let buffer = String::new();

    stream::unfold(
        (stream, buffer),
        move |(mut stream, mut buffer)| async move {
            loop {
                // First check if we have a complete event in the buffer
                if let Some((event, remaining)) = extract_raw_event(&buffer) {
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
                                    format!("Invalid UTF-8 in stream: {}", e),
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
                            if let Some((event, _)) = extract_raw_event(&buffer) {
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

/// Extract a complete SSE event from a buffer string
fn extract_event(buffer: &str) -> Option<(Result<MessageStreamEvent>, String)> {
    // Simple SSE parsing - each event is delimited by double newlines
    let parts: Vec<&str> = buffer.splitn(2, "\n\n").collect();
    if parts.len() != 2 {
        return None;
    }

    let event_text = parts[0];
    let rest = parts[1].to_string();

    // Process the event data
    let mut data = None;
    for line in event_text.lines() {
        if line.starts_with("data: ") {
            data = Some(line.trim_start_matches("data: "));
        }
    }

    // Process the data field
    match data {
        Some("[DONE]") => {
            // End of stream marker
            Some((
                Ok(MessageStreamEvent::MessageStop(Default::default())),
                rest,
            ))
        }
        Some(json_str) => {
            // Parse the JSON
            match serde_json::from_str::<MessageStreamEvent>(json_str) {
                Ok(event) => Some((Ok(event), rest)),
                Err(e) => Some((
                    Err(Error::serialization(
                        format!("Failed to parse event JSON: {}", e),
                        Some(Box::new(e)),
                    )),
                    rest,
                )),
            }
        }
        None => {
            // Skip empty events
            Some((
                Ok(MessageStreamEvent::MessageStop(Default::default())),
                rest,
            ))
        }
    }
}

/// Extract a complete raw SSE event from a buffer string
fn extract_raw_event(buffer: &str) -> Option<(Result<RawMessageStreamEvent>, String)> {
    // Simple SSE parsing - each event is delimited by double newlines
    let parts: Vec<&str> = buffer.splitn(2, "\n\n").collect();
    if parts.len() != 2 {
        return None;
    }

    let event_text = parts[0];
    let rest = parts[1].to_string();

    // Process the event data
    let mut data = None;
    for line in event_text.lines() {
        if line.starts_with("data: ") {
            data = Some(line.trim_start_matches("data: "));
        }
    }

    // Process the data field
    match data {
        Some("[DONE]") => {
            // End of stream marker
            Some((
                Ok(RawMessageStreamEvent::MessageStop(Default::default())),
                rest,
            ))
        }
        Some(json_str) => {
            // Parse the JSON
            match serde_json::from_str::<RawMessageStreamEvent>(json_str) {
                Ok(event) => Some((Ok(event), rest)),
                Err(e) => Some((
                    Err(Error::serialization(
                        format!("Failed to parse event JSON: {}", e),
                        Some(Box::new(e)),
                    )),
                    rest,
                )),
            }
        }
        None => {
            // Skip empty events
            Some((
                Ok(RawMessageStreamEvent::MessageStop(Default::default())),
                rest,
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{KnownModel, MessageCreateParamsBase, MessageParam, MessageRole, Model};
    use std::env;

    #[test]
    fn test_client_creation() {
        // Test with explicit API key
        let client = Anthropic::new(Some("test-key".to_string())).unwrap();
        assert_eq!(client.api_key, "test-key");
        assert_eq!(client.base_url, DEFAULT_API_URL);
        assert_eq!(client.timeout, DEFAULT_TIMEOUT);

        // Test with custom options
        let client = Anthropic::with_options(
            Some("test-key".to_string()),
            Some("https://custom-api.example.com/".to_string()),
            Some(Duration::from_secs(30)),
        )
        .unwrap();
        assert_eq!(client.api_key, "test-key");
        assert_eq!(client.base_url, "https://custom-api.example.com/");
        assert_eq!(client.timeout, Duration::from_secs(30));
    }

    #[tokio::test]
    #[ignore] // Ignore by default as this requires a real API key
    async fn test_stream_raw() {
        // This test requires a valid API key in the CLAUDIUS_API_KEY environment variable
        let api_key = env::var("CLAUDIUS_API_KEY").ok();
        if api_key.is_none() {
            println!("Skipping test_stream_raw: CLAUDIUS_API_KEY not set");
            return;
        }

        let client = Anthropic::new(api_key).unwrap();

        // Create a message with a simple prompt
        let message = MessageParam::new_with_string(
            "Hello, Claude. Please respond with a short greeting.".to_string(),
            MessageRole::User,
        );

        // Set up the message parameters
        let base_params = MessageCreateParamsBase::new(
            100, // max tokens
            vec![message],
            Model::Known(KnownModel::Claude37SonnetLatest),
        );

        let params = MessageCreateParams::new_streaming(base_params);
        let stream = client.stream_raw(params).await.unwrap();

        // Pin the stream and iterate through events
        futures::pin_mut!(stream);

        let mut received_events = false;

        while let Some(event_result) = stream.next().await {
            match event_result {
                Ok(event) => {
                    // Just check that we're receiving some events
                    println!("Received raw event: {:?}", event);
                    received_events = true;

                    // For detailed testing, we could match on specific event types:
                    match event {
                        RawMessageStreamEvent::MessageStart(_) => {
                            println!("Message start event received");
                        }
                        RawMessageStreamEvent::ContentBlockStart(_) => {
                            println!("Content block start event received");
                        }
                        RawMessageStreamEvent::ContentBlockDelta(_) => {
                            println!("Content block delta event received");
                        }
                        _ => {}
                    }
                }
                Err(e) => {
                    panic!("Error in stream: {:?}", e);
                }
            }
        }

        assert!(received_events, "Expected to receive some streaming events");
    }
}
