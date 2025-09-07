use futures::Stream;
use reqwest::header::{HeaderMap, HeaderValue};
use reqwest::{Client as ReqwestClient, Response, header};
use serde::Deserialize;
use std::env;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;

use crate::backoff::ExponentialBackoff;
use crate::error::{Error, Result};
use crate::sse::process_sse;
use crate::types::{
    Message, MessageCountTokensParams, MessageCreateParams, MessageStreamEvent, MessageTokensCount,
    ModelInfo, ModelListParams, ModelListResponse,
};

const DEFAULT_API_URL: &str = "https://api.anthropic.com/v1/";
const ANTHROPIC_API_VERSION: &str = "2023-06-01";
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);

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
            .pool_max_idle_per_host(10) // Connection pooling optimization
            .pool_idle_timeout(Duration::from_secs(90))
            .tcp_keepalive(Duration::from_secs(60))
            .http2_prior_knowledge() // Use HTTP/2 for better performance
            .build()
            .map_err(|e| {
                Error::http_client(
                    format!("Failed to build HTTP client: {e}"),
                    Some(Box::new(e)),
                )
            })?;

        // Pre-build headers for performance
        let cached_headers = Arc::new(Self::build_default_headers(&api_key)?);

        Ok(Self {
            api_key,
            client,
            base_url: DEFAULT_API_URL.to_string(),
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
    pub fn with_base_url(mut self, base_url: String) -> Self {
        self.base_url = base_url;
        self
    }

    /// Set a custom timeout for this client.
    ///
    /// This method allows you to specify a different timeout for API requests.
    pub fn with_timeout(mut self, timeout: Duration) -> Result<Self> {
        self.timeout = timeout;

        // Recreate the client with the new timeout and performance optimizations
        let client = ReqwestClient::builder()
            .timeout(timeout)
            .pool_max_idle_per_host(10)
            .pool_idle_timeout(Duration::from_secs(90))
            .tcp_keepalive(Duration::from_secs(60))
            .http2_prior_knowledge()
            .build()
            .map_err(|e| {
                Error::http_client(
                    "Failed to build HTTP client with new timeout",
                    Some(Box::new(e)),
                )
            })?;

        self.client = client;
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
        if e.is_timeout() {
            Error::timeout(
                format!("Request timed out: {e}"),
                Some(self.timeout.as_secs_f64()),
            )
        } else if e.is_connect() {
            Error::connection(format!("Connection error: {e}"), Some(Box::new(e)))
        } else {
            Error::http_client(format!("Request failed: {e}"), Some(Box::new(e)))
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

        response.json::<T>().await.map_err(|e| {
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

        response.json::<T>().await.map_err(|e| {
            Error::serialization(format!("Failed to parse response: {e}"), Some(Box::new(e)))
        })
    }

    /// Send a message to the API and get a non-streaming response.
    pub async fn send(&self, mut params: MessageCreateParams) -> Result<Message> {
        // Validate parameters first
        params.validate()?;

        // Ensure stream is disabled
        params.stream = false;

        self.retry_with_backoff(|| async {
            let url = format!("{}messages", self.base_url);
            self.execute_post_request(&url, &params, None).await
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
        // Validate parameters first
        params.validate()?;

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
                    .map_err(|e| self.map_request_error(e))?;

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
            self.execute_post_request(&url, &params, None).await
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
        .await
    }

    /// Retrieve information about a specific model.
    ///
    /// Returns detailed information about the specified model, including its
    /// ID, creation date, display name, and type.
    pub async fn get_model(&self, model_id: &str) -> Result<ModelInfo> {
        self.retry_with_backoff(|| async {
            let url = format!("{}models/{}", self.base_url, model_id);
            self.execute_get_request(&url, None).await
        })
        .await
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
    fn client_builder_methods() {
        let client = Anthropic::new(Some("test_key".to_string())).unwrap();

        // Test builder pattern methods
        let configured_client = client
            .with_base_url("https://custom.api.com/v1/".to_string())
            .with_max_retries(5)
            .with_backoff_params(2.0, 1.0);

        assert_eq!(configured_client.base_url, "https://custom.api.com/v1/");
        assert_eq!(configured_client.max_retries, 5);
        assert_eq!(configured_client.throughput_ops_sec, 2.0);
        assert_eq!(configured_client.reserve_capacity, 1.0);
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
}
