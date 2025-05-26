use bytes::Bytes;
use futures::Stream;
use futures::stream::{self, StreamExt};
use reqwest::header::{HeaderMap, HeaderValue};
use reqwest::{Client as ReqwestClient, Response, header};
use serde::Deserialize;
use std::env;
use std::time::Duration;
use tokio::time::sleep;

use crate::backoff::ExponentialBackoff;
use crate::error::{Error, Result};
use crate::types::{
    ContentBlockDeltaEvent, ContentBlockStartEvent, ContentBlockStopEvent, Message,
    MessageCountTokensParams, MessageCreateParams, MessageDeltaEvent, MessageStartEvent,
    MessageStopEvent, MessageStreamEvent, MessageTokensCount, ModelInfo, ModelListParams,
    ModelListResponse,
};

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
    max_retries: usize,
    throughput_ops_sec: f64,
    reserve_capacity: f64,
}

impl Anthropic {
    /// Create a new Anthropic client.
    ///
    /// The API key can be provided directly or read from the CLAUDIUS_API_KEY or ANTHROPIC_API_KEY
    /// environment variables.
    pub fn new(api_key: Option<String>) -> Result<Self> {
        let api_key = match api_key {
            Some(key) => key,
            None => match env::var("CLAUDIUS_API_KEY").ok() {
                Some(key) => key,
                None => env::var("ANTHROPIC_API_KEY").map_err(|_| {
                    Error::authentication(
                        "API key not provided and ANTHROPIC_API_KEY environment variable not set",
                    )
                })?,
            },
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
            max_retries: 3,
            throughput_ops_sec: 1.0 / 60.0,
            reserve_capacity: 1.0 / 60.0,
        })
    }

    /// Set a custom base URL for this client.
    ///
    /// This method allows you to specify a different API endpoint for the client.
    pub fn with_base_url(mut self, base_url: String) -> Self {
        self.base_url = base_url;
        self
    }

    /// Set a custom timeout for this client.
    ///
    /// This method allows you to specify a different timeout for API requests.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;

        // Recreate the client with the new timeout
        let client = ReqwestClient::builder()
            .timeout(timeout)
            .build()
            .expect("Failed to build HTTP client with new timeout");

        self.client = client;
        self
    }

    /// Set the maximum number of retries for this client.
    ///
    /// This method allows you to specify how many times to retry failed requests.
    pub fn with_max_retries(mut self, max_retries: usize) -> Self {
        self.max_retries = max_retries;
        self
    }

    /// Set the backoff parameters for this client.
    ///
    /// This method allows you to configure the exponential backoff algorithm.
    pub fn with_backoff_params(mut self, throughput_ops_sec: f64, reserve_capacity: f64) -> Self {
        self.throughput_ops_sec = throughput_ops_sec;
        self.reserve_capacity = reserve_capacity;
        self
    }

    /// Set both a custom base URL and timeout for this client.
    ///
    /// This is a convenience method that chains with_base_url and with_timeout.
    pub fn with_base_url_and_timeout(self, base_url: String, timeout: Duration) -> Self {
        self.with_base_url(base_url).with_timeout(timeout)
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

    /// Retry wrapper that implements exponential backoff with header-based retry-after
    async fn retry_with_backoff<F, Fut, T>(&self, operation: F) -> Result<T>
    where
        F: Fn() -> Fut,
        Fut: std::future::Future<Output = Result<T>>,
    {
        let backoff = ExponentialBackoff::new(self.throughput_ops_sec, self.reserve_capacity);
        let mut last_error = None;

        for attempt in 0..=self.max_retries {
            match operation().await {
                Ok(result) => return Ok(result),
                Err(error) => {
                    // Check if error is retryable
                    if !error.is_retryable() {
                        return Err(error);
                    }

                    // Don't sleep on the last attempt
                    if attempt == self.max_retries {
                        last_error = Some(error);
                        break;
                    }

                    // Calculate backoff duration
                    let exp_backoff_duration = backoff.next();

                    // Get retry-after from error if available
                    let header_backoff_duration = match &error {
                        Error::RateLimit {
                            retry_after: Some(seconds),
                            ..
                        } => Some(Duration::from_secs(*seconds)),
                        Error::ServiceUnavailable {
                            retry_after: Some(seconds),
                            ..
                        } => Some(Duration::from_secs(*seconds)),
                        _ => None,
                    };

                    // Take the maximum of exponential backoff and header-based backoff
                    let sleep_duration = match header_backoff_duration {
                        Some(header_duration) => exp_backoff_duration.max(header_duration),
                        None => exp_backoff_duration,
                    };

                    sleep(sleep_duration).await;
                    last_error = Some(error);
                }
            }
        }

        Err(last_error.unwrap())
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
    pub async fn send(&self, mut params: MessageCreateParams) -> Result<Message> {
        // Ensure stream is disabled
        params.stream = false;

        self.retry_with_backoff(|| async {
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
        })
        .await
    }

    /// Send a message to the API and get a streaming response.
    ///
    /// Returns a stream of MessageStreamEvent objects that can be processed incrementally.
    pub async fn stream(
        &self,
        mut params: MessageCreateParams,
    ) -> Result<impl Stream<Item = Result<MessageStreamEvent>>> {
        // Ensure stream is enabled
        params.stream = true;

        let response = self
            .retry_with_backoff(|| async {
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

                Ok(response)
            })
            .await?;

        // Get the byte stream from the response
        let stream = response.bytes_stream();

        // Create an SSE processor
        Ok(process_sse(stream))
    }

    /// Count tokens for a message.
    ///
    /// This method counts the number of tokens that would be used by a message with the given parameters.
    /// It's useful for estimating costs or making sure your messages fit within the model's context window.
    pub async fn count_tokens(
        &self,
        params: MessageCountTokensParams,
    ) -> Result<MessageTokensCount> {
        self.retry_with_backoff(|| async {
            let url = format!("{}messages/count_tokens", self.base_url);

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

            response.json::<MessageTokensCount>().await.map_err(|e| {
                Error::serialization(
                    format!("Failed to parse response: {}", e),
                    Some(Box::new(e)),
                )
            })
        })
        .await
    }

    /// List available models from the API.
    ///
    /// Returns a paginated list of all available models. Use the parameters to control
    /// pagination and filter results.
    pub async fn list_models(&self, params: Option<ModelListParams>) -> Result<ModelListResponse> {
        self.retry_with_backoff(|| async {
            let url = format!("{}models", self.base_url);
            let mut request = self.client.get(&url).headers(self.default_headers());

            // Add query parameters if provided
            if let Some(ref params) = params {
                if let Some(ref after_id) = params.after_id {
                    request = request.query(&[("after_id", after_id)]);
                }
                if let Some(ref before_id) = params.before_id {
                    request = request.query(&[("before_id", before_id)]);
                }
                if let Some(limit) = params.limit {
                    request = request.query(&[("limit", limit.to_string())]);
                }
                // Note: betas parameter is typically sent as a header, not query param
                // but we'll follow the API specification here
            }

            let response = request.send().await.map_err(|e| {
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

            response.json::<ModelListResponse>().await.map_err(|e| {
                Error::serialization(
                    format!("Failed to parse response: {}", e),
                    Some(Box::new(e)),
                )
            })
        })
        .await
    }

    /// Retrieve information about a specific model.
    ///
    /// Returns detailed information about the specified model, including its
    /// ID, creation date, display name, and type.
    pub async fn get_model(&self, model_id: &str) -> Result<ModelInfo> {
        self.retry_with_backoff(|| async {
            let url = format!("{}models/{}", self.base_url, model_id);

            let response = self
                .client
                .get(&url)
                .headers(self.default_headers())
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

            response.json::<ModelInfo>().await.map_err(|e| {
                Error::serialization(
                    format!("Failed to parse response: {}", e),
                    Some(Box::new(e)),
                )
            })
        })
        .await
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

/// Extract a complete SSE event from a buffer string
fn extract_event(buffer: &str) -> Option<(Result<MessageStreamEvent>, String)> {
    // Simple SSE parsing - each event is delimited by double newlines
    let parts: Vec<&str> = buffer.splitn(2, "\n\n").collect();
    if parts.len() != 2 {
        return None;
    }
    let event_text = parts[0];
    let rest = parts[1].to_string();
    let Some((event_type, event_data)) = event_text.split_once('\n') else {
        todo!();
    };
    let Some(event_data) = event_data.strip_prefix("data:").map(str::trim) else {
        todo!();
    };
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
        event_type => Some((Err(Error::todo(format!("handle {}", event_type))), rest)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn retry_logic_with_backoff() {
        let client = Anthropic {
            api_key: "test".to_string(),
            client: ReqwestClient::new(),
            base_url: "http://localhost".to_string(),
            timeout: Duration::from_secs(1),
            max_retries: 2,
            throughput_ops_sec: 1.0 / 60.0,
            reserve_capacity: 1.0 / 60.0,
        };

        let attempt_counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = attempt_counter.clone();

        let result = client
            .retry_with_backoff(|| {
                let counter = counter_clone.clone();
                async move {
                    let attempt = counter.fetch_add(1, Ordering::SeqCst);
                    match attempt {
                        0 | 1 => Err(Error::rate_limit("Rate limited", Some(1))),
                        _ => Ok("success".to_string()),
                    }
                }
            })
            .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "success");
        assert_eq!(attempt_counter.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn retry_logic_with_non_retryable_error() {
        let client = Anthropic {
            api_key: "test".to_string(),
            client: ReqwestClient::new(),
            base_url: "http://localhost".to_string(),
            timeout: Duration::from_secs(1),
            max_retries: 2,
            throughput_ops_sec: 1.0 / 60.0,
            reserve_capacity: 1.0 / 60.0,
        };

        let attempt_counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = attempt_counter.clone();

        let result: Result<String> = client
            .retry_with_backoff(|| {
                let counter = counter_clone.clone();
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    Err(Error::authentication("Invalid API key"))
                }
            })
            .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().is_authentication());
        // Should only attempt once since authentication errors are not retryable
        assert_eq!(attempt_counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn retry_logic_max_retries_exceeded() {
        let client = Anthropic {
            api_key: "test".to_string(),
            client: ReqwestClient::new(),
            base_url: "http://localhost".to_string(),
            timeout: Duration::from_secs(1),
            max_retries: 2,
            throughput_ops_sec: 1.0 / 60.0,
            reserve_capacity: 1.0 / 60.0,
        };

        let attempt_counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = attempt_counter.clone();

        let result: Result<String> = client
            .retry_with_backoff(|| {
                let counter = counter_clone.clone();
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    Err(Error::rate_limit("Always rate limited", Some(1)))
                }
            })
            .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().is_rate_limit());
        // Should attempt max_retries + 1 times (3 total: initial + 2 retries)
        assert_eq!(attempt_counter.load(Ordering::SeqCst), 3);
    }
}
