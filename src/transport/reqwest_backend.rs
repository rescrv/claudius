//! The reqwest HTTP backend.
//!
//! This backend issues requests with [`reqwest`]. It is the default backend
//! (see the `reqwest-backend` cargo feature) and preserves the transport
//! behavior claudius has historically shipped: connection pooling, TCP
//! keepalive, and a shared connect/read-inactivity timeout.

use std::time::Duration;

use bytes::Bytes;
use futures::StreamExt;
use http::{HeaderMap, StatusCode};
use reqwest::Client as ReqwestClient;

use super::{ByteStream, HttpRequest, format_error_chain};
use crate::{Error, Result};

/// Maximum number of idle pooled connections per host.
const MAX_IDLE_PER_HOST: usize = 10;
/// How long idle pooled connections are kept alive.
const POOL_IDLE_TIMEOUT: Duration = Duration::from_secs(90);
/// TCP keepalive idle time.
const TCP_KEEPALIVE: Duration = Duration::from_secs(60);

/// The reqwest-based HTTP transport.
#[derive(Debug, Clone)]
pub(crate) struct Transport {
    client: ReqwestClient,
    timeout: Duration,
}

impl Transport {
    /// Build a transport with a shared connect and read-inactivity timeout.
    pub(crate) fn new(timeout: Duration) -> Result<Self> {
        let client = ReqwestClient::builder()
            .connect_timeout(timeout)
            .read_timeout(timeout)
            .pool_max_idle_per_host(MAX_IDLE_PER_HOST)
            .pool_idle_timeout(POOL_IDLE_TIMEOUT)
            .tcp_keepalive(TCP_KEEPALIVE)
            .build()
            .map_err(|e| {
                Error::http_client(
                    format!("Failed to build HTTP client: {e}"),
                    Some(Box::new(e)),
                )
            })?;
        Ok(Self { client, timeout })
    }

    /// Execute a request and return the response.
    ///
    /// This includes waiting for the response headers; the shared timeout
    /// bounds both connection establishment and the header wait (reqwest's
    /// `read_timeout`), as well as each subsequent body read.
    pub(crate) async fn execute(&self, request: HttpRequest) -> Result<HttpResponse> {
        let HttpRequest {
            method,
            url,
            headers,
            body,
        } = request;

        let mut builder = self.client.request(method, url).headers(headers);
        if let Some(body) = body {
            builder = builder.body(body);
        }

        let response = builder
            .send()
            .await
            .map_err(|e| map_request_error(e, self.timeout))?;

        Ok(HttpResponse {
            response,
            timeout: self.timeout,
        })
    }
}

/// A response from the reqwest backend.
#[derive(Debug)]
pub(crate) struct HttpResponse {
    response: reqwest::Response,
    timeout: Duration,
}

impl HttpResponse {
    /// The response status code.
    pub(crate) fn status(&self) -> StatusCode {
        self.response.status()
    }

    /// Whether the response status is in the 2xx range.
    pub(crate) fn is_success(&self) -> bool {
        self.response.status().is_success()
    }

    /// The response headers.
    pub(crate) fn headers(&self) -> &HeaderMap {
        self.response.headers()
    }

    /// Read the whole response body as text.
    pub(crate) async fn text(self) -> Result<String> {
        let Self { response, timeout } = self;
        response
            .text()
            .await
            .map_err(|e| map_body_error(e, timeout))
    }

    /// Read the whole response body as bytes.
    pub(crate) async fn bytes(self) -> Result<Bytes> {
        let Self { response, timeout } = self;
        response
            .bytes()
            .await
            .map_err(|e| map_body_error(e, timeout))
    }

    /// Convert the response body into a stream of chunks with a
    /// read-inactivity timeout.
    ///
    /// reqwest applies the client's `read_timeout` to the body stream
    /// internally; errors are mapped to [`crate::Error`] here.
    pub(crate) fn into_byte_stream(self) -> ByteStream {
        Box::pin(
            self.response
                .bytes_stream()
                .map(|result| result.map_err(map_stream_error)),
        )
    }
}

/// Map a request-phase reqwest error to a [`crate::Error`].
fn map_request_error(err: reqwest::Error, timeout: Duration) -> Error {
    let details = format_error_chain(&err);
    if err.is_timeout() {
        Error::timeout(
            format!("Request timed out: {details}"),
            Some(timeout.as_secs_f64()),
        )
    } else if err.is_connect() {
        Error::connection(format!("Connection error: {details}"), Some(Box::new(err)))
    } else {
        Error::http_client(format!("Request failed: {details}"), Some(Box::new(err)))
    }
}

/// Map a response-body reqwest error to a [`crate::Error`].
fn map_body_error(err: reqwest::Error, timeout: Duration) -> Error {
    let details = format_error_chain(&err);
    if err.is_timeout() {
        Error::timeout(
            format!("Response body timed out: {details}"),
            Some(timeout.as_secs_f64()),
        )
    } else if err.is_connect() {
        Error::connection(
            format!("Response body connection error: {details}"),
            Some(Box::new(err)),
        )
    } else {
        Error::http_client(
            format!("Failed to read response body: {details}"),
            Some(Box::new(err)),
        )
    }
}

/// Map a byte-stream reqwest error to a [`crate::Error`].
fn map_stream_error(err: reqwest::Error) -> Error {
    let details = format_error_chain(&err);
    if err.is_timeout() {
        Error::timeout(format!("HTTP stream timed out: {details}"), None)
    } else if err.is_connect() {
        Error::connection(
            format!("HTTP stream connection error: {details}"),
            Some(Box::new(err)),
        )
    } else {
        Error::streaming(
            format!("Error in HTTP stream: {details}"),
            Some(Box::new(err)),
        )
    }
}
