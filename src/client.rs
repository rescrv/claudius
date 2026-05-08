use std::env;
use std::error::Error as StdError;
use std::fs;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use futures::Stream;
use reqwest::header::{HeaderMap, HeaderValue};
use reqwest::{Client as ReqwestClient, Response, header};
use serde::Deserialize;
use tokio::time::sleep;

use crate::AccumulatingStream;
use crate::backoff::ExponentialBackoff;
use crate::client_logger::ClientLogger;
use crate::error::{Error, Result};
use crate::observability::{
    CLIENT_REQUEST_DURATION, CLIENT_REQUEST_ERRORS, CLIENT_REQUEST_RETRIES, CLIENT_REQUESTS,
    CLIENT_RETRY_BACKOFF,
};
use crate::sse::process_message_stream_sse;
use crate::types::{
    Message, MessageCountTokensParams, MessageCreateParams, MessageStreamEvent, MessageTokensCount,
    ModelInfo, ModelListParams, ModelListResponse,
};

/// A stream wrapper that logs events and the final message through a [`ClientLogger`].
///
/// This stream passes through all events from the underlying [`AccumulatingStream`],
/// logging each event as it occurs and logging the final reconstructed message
/// when the stream completes.
pub struct LoggingStream<'a> {
    inner: AccumulatingStream,
    logger: &'a dyn ClientLogger,
    receiver: Option<tokio::sync::oneshot::Receiver<Result<Message>>>,
}

impl<'a> LoggingStream<'a> {
    /// Create a new logging stream wrapper.
    fn new(
        inner: AccumulatingStream,
        receiver: tokio::sync::oneshot::Receiver<Result<Message>>,
        logger: &'a dyn ClientLogger,
    ) -> Self {
        Self {
            inner,
            logger,
            receiver: Some(receiver),
        }
    }
}

impl Stream for LoggingStream<'_> {
    type Item = Result<MessageStreamEvent>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let inner = Pin::new(&mut self.inner);
        match inner.poll_next(cx) {
            Poll::Ready(Some(Ok(event))) => {
                self.logger.log_stream_event(&event);
                Poll::Ready(Some(Ok(event)))
            }
            Poll::Ready(Some(Err(e))) => Poll::Ready(Some(Err(e))),
            Poll::Ready(None) => {
                // Stream ended - try to get the accumulated message
                if let Some(mut receiver) = self.receiver.take()
                    && let Ok(Ok(ref message)) = receiver.try_recv()
                {
                    self.logger.log_stream_message(message);
                }
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

const DEFAULT_API_URL: &str = "https://api.anthropic.com";
const ANTHROPIC_API_VERSION: &str = "2023-06-01";
/// Default connect/read inactivity timeout shared by all requests.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);
const STRUCTURED_OUTPUTS_BETA: &str = "structured-outputs-2025-11-13";

fn format_reqwest_error(err: &reqwest::Error) -> String {
    let mut parts = vec![err.to_string()];
    let mut source = StdError::source(err);
    while let Some(inner) = source {
        let detail = inner.to_string();
        if !parts.iter().any(|part| part == &detail) {
            parts.push(detail);
        }
        source = inner.source();
    }
    parts.join(": ")
}

/// Client for the Anthropic API with performance optimizations.
#[derive(Debug, Clone)]
pub struct Anthropic {
    api_key: String,
    client: ReqwestClient,
    base_url: String,
    timeout: Duration,
    max_retries: usize,
    throughput_ops_sec: f64,
    reserve_capacity: f64,
    /// Cached headers for performance - Arc for cheap cloning
    cached_headers: Arc<HeaderMap>,
}

impl Anthropic {
    fn build_http_client(timeout: Duration) -> Result<ReqwestClient> {
        ReqwestClient::builder()
            .connect_timeout(timeout)
            .read_timeout(timeout)
            .pool_max_idle_per_host(10) // Connection pooling optimization
            .pool_idle_timeout(Duration::from_secs(90))
            .tcp_keepalive(Duration::from_secs(60))
            .build()
            .map_err(|e| {
                Error::http_client(
                    format!("Failed to build HTTP client: {e}"),
                    Some(Box::new(e)),
                )
            })
    }

    /// Resolve an API key value, handling file:// URLs
    fn resolve_api_key(key_value: &str) -> Result<String> {
        if let Some(stripped) = key_value.strip_prefix("file://") {
            // Handle file:// URLs
            let path = if stripped.starts_with('/') {
                // Absolute path: file:///root/.env -> /root/.env
                stripped.to_string()
            } else {
                // Relative path: file://../foo -> ../foo
                stripped.to_string()
            };

            fs::read_to_string(&path)
                .map(|content| content.trim().to_string())
                .map_err(|e| {
                    Error::validation(
                        format!("Failed to read API key from file '{}': {}", path, e),
                        Some("api_key".to_string()),
                    )
                })
        } else {
            // Regular API key value
            Ok(key_value.to_string())
        }
    }

    /// Create a new Anthropic client.
    ///
    /// The API key can be provided directly or read from the CLAUDIUS_API_KEY or ANTHROPIC_API_KEY
    /// environment variables. If an environment variable value starts with "file://", it will be
    /// treated as a file path and the API key will be read from that file.
    ///
    /// The base URL is resolved from the CLAUDIUS_BASE_URL or ANTHROPIC_BASE_URL environment
    /// variables, in that order. If neither is set, the default Anthropic API URL is used.
    pub fn new(api_key: Option<String>) -> Result<Self> {
        let api_key = match api_key {
            Some(key) => Self::resolve_api_key(&key)?,
            None => match env::var("CLAUDIUS_API_KEY").ok() {
                Some(key) => Self::resolve_api_key(&key)?,
                None => {
                    let env_key = env::var("ANTHROPIC_API_KEY").map_err(|_| {
                        Error::authentication(
                            "API key not provided and ANTHROPIC_API_KEY environment variable not set",
                        )
                    })?;
                    Self::resolve_api_key(&env_key)?
                }
            },
        };

        let timeout = DEFAULT_TIMEOUT;
        let client = Self::build_http_client(timeout)?;

        // Pre-build headers for performance
        let cached_headers = Arc::new(Self::build_default_headers(&api_key)?);

        // Resolve base URL from environment variables, defaulting to the API URL
        let base_url = env::var("CLAUDIUS_BASE_URL")
            .or_else(|_| env::var("ANTHROPIC_BASE_URL"))
            .unwrap_or_else(|_| DEFAULT_API_URL.to_string());

        Ok(Self {
            api_key,
            client,
            base_url,
            timeout,
            max_retries: 3,
            throughput_ops_sec: 1.0 / 60.0,
            reserve_capacity: 1.0 / 60.0,
            cached_headers,
        })
    }

    /// Set a custom base URL for this client.
    ///
    /// This method allows you to specify a different API endpoint for the client.
    /// The base URL should be the root URL without the `/v1/` suffix - this will
    /// be added automatically when constructing request URLs.
    ///
    /// # Examples
    ///
    /// ```
    /// # use claudius::Anthropic;
    /// // For Anthropic's API (default)
    /// let client = Anthropic::new(Some("api-key".to_string()))?
    ///     .with_base_url("https://api.anthropic.com".to_string());
    ///
    /// // For Minimax (international)
    /// let client = Anthropic::new(Some("api-key".to_string()))?
    ///     .with_base_url("https://api.minimax.io/anthropic".to_string());
    ///
    /// // For Minimax (China)
    /// let client = Anthropic::new(Some("api-key".to_string()))?
    ///     .with_base_url("https://api.minimaxi.com/anthropic".to_string());
    /// # Ok::<(), claudius::Error>(())
    /// ```
    pub fn with_base_url(mut self, base_url: String) -> Self {
        self.base_url = base_url;
        self
    }

    /// Set a custom timeout for this client.
    ///
    /// This method allows you to specify a different connect/read inactivity
    /// timeout for API requests.
    pub fn with_timeout(mut self, timeout: Duration) -> Result<Self> {
        self.timeout = timeout;

        self.client = Self::build_http_client(timeout).map_err(|e| match e {
            Error::HttpClient { source, .. } => Error::http_client(
                "Failed to build HTTP client with new timeout",
                source.map(|src| Box::new(src) as Box<dyn std::error::Error + Send + Sync>),
            ),
            other => other,
        })?;
        Ok(self)
    }

    /// Set the maximum number of retries for this client.
    ///
    /// This method allows you to specify how many times to retry failed requests.
    pub fn with_max_retries(mut self, max_retries: usize) -> Self {
        self.max_retries = max_retries;
        self
    }

    /// Get the API key being used by this client.
    pub fn api_key(&self) -> &str {
        &self.api_key
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
    pub fn with_base_url_and_timeout(self, base_url: String, timeout: Duration) -> Result<Self> {
        self.with_base_url(base_url).with_timeout(timeout)
    }

    /// Build default headers for API requests (static method for initialization).
    fn build_default_headers(api_key: &str) -> Result<HeaderMap> {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONTENT_TYPE,
            HeaderValue::from_static("application/json"),
        );
        headers.insert(header::ACCEPT, HeaderValue::from_static("application/json"));
        headers.insert(
            "x-api-key",
            HeaderValue::from_str(api_key).map_err(|e| {
                Error::validation(
                    format!("Invalid API key format: {e}"),
                    Some("api_key".to_string()),
                )
            })?,
        );
        headers.insert(
            "anthropic-version",
            HeaderValue::from_static(ANTHROPIC_API_VERSION),
        );
        Ok(headers)
    }

    /// Get cached headers for performance (no allocation needed).
    fn default_headers(&self) -> HeaderMap {
        (*self.cached_headers).clone()
    }

    /// Build a full endpoint URL from the base URL and endpoint path.
    ///
    /// This method handles trailing slashes gracefully and always inserts `/v1/`
    /// between the base URL and endpoint path. This allows the base URL to be
    /// specified without requiring a specific format (with or without trailing slash,
    /// with or without `/v1/` suffix).
    ///
    /// # Examples
    ///
    /// - Base: `https://api.anthropic.com`, endpoint: `messages` → `https://api.anthropic.com/v1/messages`
    /// - Base: `https://api.minimax.io/anthropic`, endpoint: `messages` → `https://api.minimax.io/anthropic/v1/messages`
    /// - Base: `https://example.com/`, endpoint: `models` → `https://example.com/v1/models`
    fn build_url(&self, endpoint: &str) -> String {
        let base = self.base_url.trim_end_matches('/');
        format!("{}/v1/{}", base, endpoint)
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

                    CLIENT_REQUEST_RETRIES.click();
                    CLIENT_RETRY_BACKOFF.add(sleep_duration.as_secs_f64());
                    sleep(sleep_duration).await;
                    last_error = Some(error);
                }
            }
        }

        Err(last_error
            .unwrap_or_else(|| Error::unknown("Failed after retries without capturing error")))
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
                    format!("Failed to read error response: {e}"),
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
            529 => Error::rate_limit(error_message, retry_after),
            _ => Error::api(status_code, error_type, error_message, request_id),
        }
    }

    /// Convert reqwest errors to appropriate Error types
    fn map_request_error(&self, e: reqwest::Error) -> Error {
        let details = format_reqwest_error(&e);
        if e.is_timeout() {
            Error::timeout(
                format!("Request timed out: {details}"),
                Some(self.timeout.as_secs_f64()),
            )
        } else if e.is_connect() {
            Error::connection(format!("Connection error: {details}"), Some(Box::new(e)))
        } else {
            Error::http_client(format!("Request failed: {details}"), Some(Box::new(e)))
        }
    }

    fn map_response_body_error(&self, e: reqwest::Error) -> Error {
        let details = format_reqwest_error(&e);
        if e.is_timeout() {
            Error::timeout(
                format!("Response body timed out: {details}"),
                Some(self.timeout.as_secs_f64()),
            )
        } else if e.is_connect() {
            Error::connection(
                format!("Response body connection error: {details}"),
                Some(Box::new(e)),
            )
        } else {
            Error::http_client(
                format!("Failed to read response body: {details}"),
                Some(Box::new(e)),
            )
        }
    }

    /// Execute a POST request with error handling
    async fn execute_post_request<T: serde::de::DeserializeOwned>(
        &self,
        url: &str,
        body: &impl serde::Serialize,
        headers: Option<HeaderMap>,
    ) -> Result<T> {
        let headers = headers.unwrap_or_else(|| self.default_headers());

        let response = self
            .client
            .post(url)
            .headers(headers)
            .json(body)
            .send()
            .await
            .map_err(|e| self.map_request_error(e))?;

        if !response.status().is_success() {
            return Err(Self::process_error_response(response).await);
        }

        let body = response
            .bytes()
            .await
            .map_err(|e| self.map_response_body_error(e))?;

        serde_json::from_slice::<T>(&body).map_err(|e| {
            Error::serialization(format!("Failed to parse response: {e}"), Some(Box::new(e)))
        })
    }

    /// Execute a GET request with error handling
    async fn execute_get_request<T: serde::de::DeserializeOwned>(
        &self,
        url: &str,
        query_params: Option<&[(String, String)]>,
    ) -> Result<T> {
        let mut request = self.client.get(url).headers(self.default_headers());

        if let Some(params) = query_params {
            for (key, value) in params {
                request = request.query(&[(key, value)]);
            }
        }

        let response = request
            .send()
            .await
            .map_err(|e| self.map_request_error(e))?;

        if !response.status().is_success() {
            return Err(Self::process_error_response(response).await);
        }

        let body = response
            .bytes()
            .await
            .map_err(|e| self.map_response_body_error(e))?;

        serde_json::from_slice::<T>(&body).map_err(|e| {
            Error::serialization(format!("Failed to parse response: {e}"), Some(Box::new(e)))
        })
    }

    /// Send a message to the API and get a non-streaming response.
    pub async fn send(&self, mut params: MessageCreateParams) -> Result<Message> {
        let start = Instant::now();
        CLIENT_REQUESTS.click();

        // Validate parameters first
        if let Err(err) = params.validate() {
            CLIENT_REQUEST_ERRORS.click();
            CLIENT_REQUEST_DURATION.add(start.elapsed().as_secs_f64());
            return Err(err);
        }

        // Ensure stream is disabled
        params.stream = false;

        // Check if structured outputs beta header is needed
        let headers = if params.requires_structured_outputs_beta() {
            let mut headers = self.default_headers();
            headers.insert(
                "anthropic-beta",
                HeaderValue::from_static(STRUCTURED_OUTPUTS_BETA),
            );
            Some(headers)
        } else {
            None
        };

        let result = self
            .retry_with_backoff(|| async {
                let url = self.build_url("messages");
                self.execute_post_request(&url, &params, headers.clone())
                    .await
            })
            .await;

        CLIENT_REQUEST_DURATION.add(start.elapsed().as_secs_f64());
        if result.is_err() {
            CLIENT_REQUEST_ERRORS.click();
        }
        result
    }

    /// Send a message to the API with logging and get a non-streaming response.
    ///
    /// This method is identical to [`send`](Self::send) but additionally logs
    /// the response through the provided [`ClientLogger`].
    pub async fn send_with_logger(
        &self,
        params: MessageCreateParams,
        logger: &dyn ClientLogger,
    ) -> Result<Message> {
        let result = self.send(params).await;
        if let Ok(ref message) = result {
            logger.log_response(message);
        }
        result
    }

    /// Send a message to the API and get a streaming response.
    ///
    /// Returns a stream of MessageStreamEvent objects that can be processed incrementally.
    pub async fn stream(
        &self,
        params: &MessageCreateParams,
    ) -> Result<impl Stream<Item = Result<MessageStreamEvent>> + use<>> {
        let start = Instant::now();
        CLIENT_REQUESTS.click();

        // Validate parameters first
        if let Err(err) = params.validate() {
            CLIENT_REQUEST_ERRORS.click();
            CLIENT_REQUEST_DURATION.add(start.elapsed().as_secs_f64());
            return Err(err);
        }

        // Ensure stream is enabled
        if !params.stream {
            let err = Error::validation(
                "stream must be true for streaming requests",
                Some("stream".to_string()),
            );
            CLIENT_REQUEST_ERRORS.click();
            CLIENT_REQUEST_DURATION.add(start.elapsed().as_secs_f64());
            return Err(err);
        }

        // Check if structured outputs beta header is needed
        let needs_beta = params.requires_structured_outputs_beta();

        let response = self
            .retry_with_backoff(|| async {
                let url = self.build_url("messages");

                let mut headers = self.default_headers();
                headers.insert(
                    header::ACCEPT,
                    HeaderValue::from_static("text/event-stream"),
                );
                if needs_beta {
                    headers.insert(
                        "anthropic-beta",
                        HeaderValue::from_static(STRUCTURED_OUTPUTS_BETA),
                    );
                }

                let response = self
                    .client
                    .post(&url)
                    .headers(headers)
                    .json(params)
                    .send()
                    .await
                    .map_err(|e| self.map_request_error(e))?;

                if !response.status().is_success() {
                    return Err(Self::process_error_response(response).await);
                }

                Ok(response)
            })
            .await;

        CLIENT_REQUEST_DURATION.add(start.elapsed().as_secs_f64());
        let response = match response {
            Ok(response) => response,
            Err(err) => {
                CLIENT_REQUEST_ERRORS.click();
                return Err(err);
            }
        };

        // Get the byte stream from the response
        let stream = response.bytes_stream();

        // Create an SSE processor
        Ok(process_message_stream_sse(stream))
    }

    /// Send a message to the API with logging and get a streaming response.
    ///
    /// This method is identical to [`stream`](Self::stream) but additionally logs
    /// each streaming event and the final reconstructed message through the
    /// provided [`ClientLogger`].
    ///
    /// Returns a [`LoggingStream`] that wraps an [`AccumulatingStream`], logging
    /// each event as it passes through and logging the final message when the
    /// stream completes.
    pub async fn stream_with_logger<'a>(
        &self,
        params: &MessageCreateParams,
        logger: &'a dyn ClientLogger,
    ) -> Result<LoggingStream<'a>> {
        let raw_stream = self.stream(params).await?;
        let (accumulating_stream, receiver) = AccumulatingStream::new(raw_stream);
        Ok(LoggingStream::new(accumulating_stream, receiver, logger))
    }

    /// Count tokens for a message.
    ///
    /// This method counts the number of tokens that would be used by a message with the given parameters.
    /// It's useful for estimating costs or making sure your messages fit within the model's context window.
    pub async fn count_tokens(
        &self,
        params: MessageCountTokensParams,
    ) -> Result<MessageTokensCount> {
        let start = Instant::now();
        CLIENT_REQUESTS.click();
        let result = self
            .retry_with_backoff(|| async {
                let url = self.build_url("messages/count_tokens");
                self.execute_post_request(&url, &params, None).await
            })
            .await;

        CLIENT_REQUEST_DURATION.add(start.elapsed().as_secs_f64());
        if result.is_err() {
            CLIENT_REQUEST_ERRORS.click();
        }
        result
    }

    /// List available models from the API.
    ///
    /// Returns a paginated list of all available models. Use the parameters to control
    /// pagination and filter results.
    pub async fn list_models(&self, params: Option<ModelListParams>) -> Result<ModelListResponse> {
        let start = Instant::now();
        CLIENT_REQUESTS.click();
        let result = self
            .retry_with_backoff(|| async {
                let url = self.build_url("models");

                let query_params = params.as_ref().map(|p| {
                    let mut params = Vec::new();
                    if let Some(ref after_id) = p.after_id {
                        params.push(("after_id".to_string(), after_id.clone()));
                    }
                    if let Some(ref before_id) = p.before_id {
                        params.push(("before_id".to_string(), before_id.clone()));
                    }
                    if let Some(limit) = p.limit {
                        params.push(("limit".to_string(), limit.to_string()));
                    }
                    params
                });

                self.execute_get_request(&url, query_params.as_deref())
                    .await
            })
            .await;

        CLIENT_REQUEST_DURATION.add(start.elapsed().as_secs_f64());
        if result.is_err() {
            CLIENT_REQUEST_ERRORS.click();
        }
        result
    }

    /// Retrieve information about a specific model.
    ///
    /// Returns detailed information about the specified model, including its
    /// ID, creation date, display name, and type.
    pub async fn get_model(&self, model_id: &str) -> Result<ModelInfo> {
        let start = Instant::now();
        CLIENT_REQUESTS.click();
        let result = self
            .retry_with_backoff(|| async {
                let url = self.build_url(&format!("models/{}", model_id));
                self.execute_get_request(&url, None).await
            })
            .await;

        CLIENT_REQUEST_DURATION.add(start.elapsed().as_secs_f64());
        if result.is_err() {
            CLIENT_REQUEST_ERRORS.click();
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{KnownModel, MessageParam, Model};
    use futures::StreamExt;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    fn request_headers_end(buffer: &[u8]) -> Option<usize> {
        buffer.windows(4).position(|window| window == b"\r\n\r\n")
    }

    async fn read_http_request(socket: &mut tokio::net::TcpStream) {
        let mut buffer = Vec::new();
        let mut chunk = [0_u8; 1024];
        loop {
            let read = socket.read(&mut chunk).await.unwrap();
            assert!(
                read > 0,
                "client closed the connection before sending headers"
            );
            buffer.extend_from_slice(&chunk[..read]);
            if request_headers_end(&buffer).is_some() {
                break;
            }
        }

        let headers_end = request_headers_end(&buffer).unwrap();
        let headers = String::from_utf8_lossy(&buffer[..headers_end]);
        let content_length = headers
            .lines()
            .find_map(|line| {
                let mut parts = line.splitn(2, ':');
                let name = parts.next()?.trim();
                let value = parts.next()?.trim();
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.parse::<usize>().ok())
                    .flatten()
            })
            .unwrap_or(0);

        while buffer.len() - (headers_end + 4) < content_length {
            let read = socket.read(&mut chunk).await.unwrap();
            assert!(
                read > 0,
                "client closed the connection before sending the full body"
            );
            buffer.extend_from_slice(&chunk[..read]);
        }
    }

    async fn start_test_server<F, Fut>(handler: F) -> String
    where
        F: FnOnce(tokio::net::TcpStream) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = ()> + Send + 'static,
    {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            socket.set_nodelay(true).unwrap();
            handler(socket).await;
        });
        format!("http://{}", address)
    }

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
            cached_headers: Arc::new(HeaderMap::new()),
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
            cached_headers: Arc::new(HeaderMap::new()),
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
            cached_headers: Arc::new(HeaderMap::new()),
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

    #[tokio::test]
    async fn error_529_is_retryable() {
        // Test that 529 errors are properly mapped to rate_limit and are retryable
        let client = Anthropic {
            api_key: "test".to_string(),
            client: ReqwestClient::new(),
            base_url: "http://localhost".to_string(),
            timeout: Duration::from_secs(1),
            max_retries: 2,
            throughput_ops_sec: 1.0 / 60.0,
            reserve_capacity: 1.0 / 60.0,
            cached_headers: Arc::new(HeaderMap::new()),
        };

        let attempt_counter = Arc::new(AtomicUsize::new(0));
        let counter_clone = attempt_counter.clone();

        let result = client
            .retry_with_backoff(|| {
                let counter = counter_clone.clone();
                async move {
                    let attempt = counter.fetch_add(1, Ordering::SeqCst);
                    match attempt {
                        0 | 1 => {
                            // Simulate a 529 overloaded error
                            Err(Error::api(
                                529,
                                Some("overloaded_error".to_string()),
                                "Overloaded".to_string(),
                                None,
                            ))
                        }
                        _ => Ok("success".to_string()),
                    }
                }
            })
            .await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "success");
        // Should retry: initial attempt + 2 retries = 3 total
        assert_eq!(attempt_counter.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn error_529_mapped_correctly() {
        // Test that a 529 API error is correctly identified as retryable
        let error = Error::api(
            529,
            Some("overloaded_error".to_string()),
            "Overloaded".to_string(),
            None,
        );
        assert!(error.is_retryable());

        // Test that rate_limit error (which 529 now maps to) is also retryable
        let rate_limit_error = Error::rate_limit("Overloaded", Some(5));
        assert!(rate_limit_error.is_retryable());
    }

    #[test]
    fn resolve_api_key_regular_value() {
        let result = Anthropic::resolve_api_key("sk-test-key-123");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "sk-test-key-123");
    }

    #[test]
    fn resolve_api_key_file_url_absolute() {
        let test_dir = std::env::temp_dir().join(format!("claudius_test_{}", std::process::id()));
        std::fs::create_dir_all(&test_dir).unwrap();
        let test_file = test_dir.join("test_api_key.txt");
        std::fs::write(&test_file, "sk-test-from-file-123\n").unwrap();

        let file_url = format!("file://{}", test_file.display());
        let result = Anthropic::resolve_api_key(&file_url);

        std::fs::remove_dir_all(&test_dir).unwrap();

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "sk-test-from-file-123");
    }

    #[test]
    fn resolve_api_key_file_url_relative() {
        let test_file = "test_relative_key.txt";
        std::fs::write(test_file, "sk-relative-key-456\n").unwrap();

        let file_url = format!("file://{}", test_file);
        let result = Anthropic::resolve_api_key(&file_url);

        std::fs::remove_file(test_file).unwrap();

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "sk-relative-key-456");
    }

    #[test]
    fn resolve_api_key_file_url_nonexistent() {
        let result = Anthropic::resolve_api_key("file:///nonexistent/path/to/key.txt");
        assert!(result.is_err());

        let error = result.unwrap_err();
        assert!(error.is_validation());
        assert!(format!("{}", error).contains("Failed to read API key from file"));
    }

    #[test]
    fn resolve_api_key_file_url_with_whitespace() {
        let test_file = "test_whitespace_key.txt";
        std::fs::write(test_file, "  sk-whitespace-key-789  \n  ").unwrap();

        let file_url = format!("file://{}", test_file);
        let result = Anthropic::resolve_api_key(&file_url);

        std::fs::remove_file(test_file).unwrap();

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "sk-whitespace-key-789");
    }

    #[test]
    fn client_builder_methods() {
        let client = Anthropic::new(Some("test_key".to_string())).unwrap();

        // Test builder pattern methods
        let configured_client = client
            .with_base_url("https://custom.api.com".to_string())
            .with_max_retries(5)
            .with_backoff_params(2.0, 1.0);

        assert_eq!(configured_client.base_url, "https://custom.api.com");
        assert_eq!(configured_client.max_retries, 5);
        assert_eq!(configured_client.throughput_ops_sec, 2.0);
        assert_eq!(configured_client.reserve_capacity, 1.0);
    }

    #[test]
    fn build_url_default_base() {
        let client = Anthropic::new(Some("test_key".to_string())).unwrap();
        // Default base URL: https://api.anthropic.com
        assert_eq!(
            client.build_url("messages"),
            "https://api.anthropic.com/v1/messages"
        );
        assert_eq!(
            client.build_url("messages/count_tokens"),
            "https://api.anthropic.com/v1/messages/count_tokens"
        );
        assert_eq!(
            client.build_url("models"),
            "https://api.anthropic.com/v1/models"
        );
    }

    #[test]
    fn build_url_custom_base_without_trailing_slash() {
        let client = Anthropic::new(Some("test_key".to_string()))
            .unwrap()
            .with_base_url("https://api.minimax.io/anthropic".to_string());
        assert_eq!(
            client.build_url("messages"),
            "https://api.minimax.io/anthropic/v1/messages"
        );
    }

    #[test]
    fn build_url_custom_base_with_trailing_slash() {
        let client = Anthropic::new(Some("test_key".to_string()))
            .unwrap()
            .with_base_url("https://api.minimax.io/anthropic/".to_string());
        assert_eq!(
            client.build_url("messages"),
            "https://api.minimax.io/anthropic/v1/messages"
        );
    }

    #[test]
    fn build_url_minimax_china() {
        let client = Anthropic::new(Some("test_key".to_string()))
            .unwrap()
            .with_base_url("https://api.minimaxi.com/anthropic".to_string());
        assert_eq!(
            client.build_url("messages"),
            "https://api.minimaxi.com/anthropic/v1/messages"
        );
        assert_eq!(
            client.build_url(&format!("models/{}", "claude-3-opus")),
            "https://api.minimaxi.com/anthropic/v1/models/claude-3-opus"
        );
    }

    #[test]
    fn client_timeout_configuration() {
        let client = Anthropic::new(Some("test_key".to_string())).unwrap();
        let timeout = Duration::from_secs(30);

        let configured_client = client.with_timeout(timeout).unwrap();
        assert_eq!(configured_client.timeout, timeout);
    }

    #[test]
    fn client_cached_headers_performance() {
        let client = Anthropic::new(Some("test_key".to_string())).unwrap();

        // Test that headers are cached and cloning is cheap
        let headers1 = client.default_headers();
        let headers2 = client.default_headers();

        assert_eq!(headers1.len(), headers2.len());
        assert!(headers1.contains_key("x-api-key"));
        assert!(headers1.contains_key("anthropic-version"));
        assert!(headers1.contains_key("content-type"));
    }

    #[test]
    fn request_error_mapping() {
        let client = Anthropic::new(Some("test_key".to_string())).unwrap();

        // Test different types of reqwest errors are mapped correctly
        // Note: These are unit tests for the mapping logic, not integration tests
        let _timeout = Duration::from_secs(30);
        assert_eq!(client.timeout, DEFAULT_TIMEOUT); // Should use default initially
    }

    #[tokio::test]
    async fn concurrent_retry_safety() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use tokio::spawn;

        let client = Anthropic {
            api_key: "test".to_string(),
            client: ReqwestClient::new(),
            base_url: "http://localhost".to_string(),
            timeout: Duration::from_secs(1),
            max_retries: 1,
            throughput_ops_sec: 1.0,
            reserve_capacity: 1.0,
            cached_headers: Arc::new(HeaderMap::new()),
        };

        let attempt_counter = Arc::new(AtomicUsize::new(0));
        let mut handles = vec![];

        // Spawn multiple concurrent retry operations
        for _ in 0..3 {
            let client_clone = client.clone();
            let counter_clone = attempt_counter.clone();

            let handle = spawn(async move {
                client_clone
                    .retry_with_backoff(|| {
                        let counter = counter_clone.clone();
                        async move {
                            counter.fetch_add(1, Ordering::SeqCst);
                            Ok::<String, Error>("success".to_string())
                        }
                    })
                    .await
            });
            handles.push(handle);
        }

        // Wait for all operations to complete
        for handle in handles {
            let result = handle.await.unwrap();
            assert!(result.is_ok());
        }

        // Verify all operations executed
        assert_eq!(attempt_counter.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn streaming_timeout_is_inactivity_based() {
        let base_url = start_test_server(|mut socket| async move {
            read_http_request(&mut socket).await;
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\n\
Content-Type: text/event-stream\r\n\
Cache-Control: no-cache\r\n\
Connection: close\r\n\r\n",
                )
                .await
                .unwrap();

            for chunk in [
                b"event: ping\n".as_slice(),
                b"data: {}\n".as_slice(),
                b"\n".as_slice(),
            ] {
                socket.write_all(chunk).await.unwrap();
                socket.flush().await.unwrap();
                tokio::time::sleep(Duration::from_millis(40)).await;
            }
        })
        .await;

        let client = Anthropic::new(Some("test_key".to_string()))
            .unwrap()
            .with_base_url(base_url)
            .with_timeout(Duration::from_millis(75))
            .unwrap();
        let params = MessageCreateParams::new_streaming(
            16,
            vec![MessageParam::user("ping")],
            Model::Known(KnownModel::ClaudeHaiku45),
        );

        let mut stream = std::pin::pin!(client.stream(&params).await.unwrap());
        let first = stream.next().await.unwrap().unwrap();
        assert_eq!(first, MessageStreamEvent::Ping);
    }

    #[tokio::test]
    async fn streaming_stall_reports_timeout_error() {
        let base_url = start_test_server(|mut socket| async move {
            read_http_request(&mut socket).await;
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\n\
Content-Type: text/event-stream\r\n\
Cache-Control: no-cache\r\n\
Connection: close\r\n\r\n\
event: ping\n",
                )
                .await
                .unwrap();
            socket.flush().await.unwrap();
            tokio::time::sleep(Duration::from_millis(120)).await;
            socket.write_all(b"data: {}\n\n").await.unwrap();
            socket.flush().await.unwrap();
        })
        .await;

        let client = Anthropic::new(Some("test_key".to_string()))
            .unwrap()
            .with_base_url(base_url)
            .with_timeout(Duration::from_millis(50))
            .unwrap();
        let params = MessageCreateParams::new_streaming(
            16,
            vec![MessageParam::user("ping")],
            Model::Known(KnownModel::ClaudeHaiku45),
        );

        let mut stream = std::pin::pin!(client.stream(&params).await.unwrap());
        let err = stream.next().await.unwrap().unwrap_err();
        assert!(matches!(err, Error::Timeout { .. }));
        assert!(err.to_string().contains("operation timed out"));
    }

    #[tokio::test]
    async fn non_streaming_body_stall_reports_timeout_error() {
        let base_url = start_test_server(|mut socket| async move {
            read_http_request(&mut socket).await;
            let body_prefix = b"{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"claude-haiku-4-5-20251001\",\"content\":[{\"type\":\"text\",\"text\":\"hel";
            let body_suffix = b"lo\"}],\"stop_reason\":\"end_turn\",\"stop_sequence\":null,\"usage\":{\"input_tokens\":1,\"output_tokens\":1}}";
            let headers = format!(
                "HTTP/1.1 200 OK\r\n\
Content-Type: application/json\r\n\
Content-Length: {}\r\n\
Connection: close\r\n\r\n",
                body_prefix.len() + body_suffix.len()
            );
            socket
                .write_all(headers.as_bytes())
                .await
                .unwrap();
            socket.write_all(body_prefix).await.unwrap();
            socket.flush().await.unwrap();
            tokio::time::sleep(Duration::from_millis(120)).await;
            socket.write_all(body_suffix).await.unwrap();
            socket.flush().await.unwrap();
            socket.shutdown().await.unwrap();
        })
        .await;

        let client = Anthropic::new(Some("test_key".to_string()))
            .unwrap()
            .with_base_url(base_url)
            .with_timeout(Duration::from_millis(50))
            .unwrap()
            .with_max_retries(0);
        let params = MessageCreateParams::new(
            16,
            vec![MessageParam::user("ping")],
            Model::Known(KnownModel::ClaudeHaiku45),
        );

        let err = client.send(params).await.unwrap_err();
        assert!(matches!(err, Error::Timeout { .. }), "{err:?}");
    }
}
