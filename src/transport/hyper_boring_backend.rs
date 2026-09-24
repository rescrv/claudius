//! The hyper + BoringSSL HTTP backend.
//!
//! This backend issues requests with [`hyper`] over BoringSSL via
//! [`hyper_boring`] (see the `hyper-boring` cargo feature). It mirrors the
//! transport behavior of the reqwest backend: HTTP/1.1 only, connection
//! pooling, TCP keepalive, and a shared connect and read-inactivity timeout
//! that bounds both the wait for response headers and the gaps between body
//! chunks.

use std::error::Error as StdError;
use std::time::Duration;

use boring::ssl::{SslConnector, SslMethod};
use bytes::{Bytes, BytesMut};
use futures::{StreamExt, stream};
use http::{HeaderMap, StatusCode, Uri};
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper_boring::HttpsConnector;
use hyper_util::client::legacy::connect::HttpConnector;
use hyper_util::client::legacy::{Client, Error as HyperClientError};
use hyper_util::rt::{TokioExecutor, TokioTimer};

use super::{ByteStream, HttpRequest, format_error_chain};
use crate::{Error, Result};

/// Maximum number of idle pooled connections per host.
const MAX_IDLE_PER_HOST: usize = 10;
/// How long idle pooled connections are kept alive.
const POOL_IDLE_TIMEOUT: Duration = Duration::from_secs(90);
/// TCP keepalive idle time.
const TCP_KEEPALIVE: Duration = Duration::from_secs(60);

/// The hyper client: HTTP/1.1 requests with full bodies over BoringSSL.
type HyperClient = Client<HttpsConnector<HttpConnector>, Full<Bytes>>;

/// The hyper + BoringSSL HTTP transport.
#[derive(Debug, Clone)]
pub(crate) struct Transport {
    client: HyperClient,
    timeout: Duration,
}

impl Transport {
    /// Build a transport with a shared connect and read-inactivity timeout.
    pub(crate) fn new(timeout: Duration) -> Result<Self> {
        let mut http = HttpConnector::new();
        // The TLS connector handles `https` URIs itself, so the inner
        // connector must accept both schemes.
        http.enforce_http(false);
        http.set_nodelay(true);
        http.set_connect_timeout(Some(timeout));
        http.set_keepalive(Some(TCP_KEEPALIVE));

        // Advertise only HTTP/1.1 over ALPN so that TLS connections always
        // negotiate HTTP/1.1, matching the reqwest backend (which is built
        // without reqwest's `http2` feature).
        let mut ssl = SslConnector::builder(SslMethod::tls()).map_err(|e| {
            Error::http_client(
                format!("Failed to build BoringSSL connector: {e}"),
                Some(Box::new(e)),
            )
        })?;
        ssl.set_alpn_protos(b"\x08http/1.1").map_err(|e| {
            Error::http_client(
                format!("Failed to configure BoringSSL ALPN protocols: {e}"),
                Some(Box::new(e)),
            )
        })?;

        let connector = HttpsConnector::with_connector(http, ssl).map_err(|e| {
            Error::http_client(
                format!("Failed to build HTTPS connector: {e}"),
                Some(Box::new(e)),
            )
        })?;

        let client = Client::builder(TokioExecutor::new())
            .pool_max_idle_per_host(MAX_IDLE_PER_HOST)
            .pool_idle_timeout(POOL_IDLE_TIMEOUT)
            .pool_timer(TokioTimer::new())
            .build(connector);

        Ok(Self { client, timeout })
    }

    /// Execute a request and return the response.
    ///
    /// The shared timeout bounds connection establishment and the wait for
    /// response headers, mirroring reqwest's `read_timeout`. Body reads are
    /// bounded separately by [`HttpResponse`].
    pub(crate) async fn execute(&self, request: HttpRequest) -> Result<HttpResponse> {
        let HttpRequest {
            method,
            url,
            headers,
            body,
        } = request;

        let uri = url.parse::<Uri>().map_err(|e| {
            Error::http_client(
                format!("Invalid request URL '{url}': {e}"),
                Some(Box::new(e)),
            )
        })?;

        let body = match body {
            Some(bytes) => Full::new(Bytes::from(bytes)),
            None => Full::new(Bytes::new()),
        };
        let mut hyper_request = http::Request::builder()
            .method(method)
            .uri(uri)
            .body(body)
            .map_err(|e| {
                Error::http_client(
                    format!("Failed to build HTTP request: {e}"),
                    Some(Box::new(e)),
                )
            })?;
        *hyper_request.headers_mut() = headers;

        let response = tokio::time::timeout(self.timeout, self.client.request(hyper_request))
            .await
            .map_err(|_| {
                Error::timeout(
                    format!(
                        "Request timed out: operation timed out \
                         (no response within {:?})",
                        self.timeout
                    ),
                    Some(self.timeout.as_secs_f64()),
                )
            })?
            .map_err(|e| self.map_request_error(e))?;

        Ok(HttpResponse {
            response,
            timeout: self.timeout,
        })
    }

    /// Map a request-phase hyper client error to a [`crate::Error`].
    fn map_request_error(&self, err: HyperClientError) -> Error {
        let details = format_error_chain(&err);
        if chain_is_timeout(&err) {
            Error::timeout(
                format!("Request timed out: {details}"),
                Some(self.timeout.as_secs_f64()),
            )
        } else if err.is_connect() {
            Error::connection(format!("Connection error: {details}"), Some(Box::new(err)))
        } else {
            Error::http_client(format!("Request failed: {details}"), Some(Box::new(err)))
        }
    }
}

/// A response from the hyper backend.
#[derive(Debug)]
pub(crate) struct HttpResponse {
    response: http::Response<Incoming>,
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
        let bytes = self.bytes().await?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }

    /// Read the whole response body as bytes.
    ///
    /// The gap between body chunks is bounded by the read-inactivity timeout,
    /// matching reqwest's `read_timeout`.
    pub(crate) async fn bytes(self) -> Result<Bytes> {
        let Self { response, timeout } = self;
        let mut stream = response.into_body().into_data_stream();
        let mut buffer = BytesMut::new();
        loop {
            match tokio::time::timeout(timeout, stream.next()).await {
                Ok(Some(Ok(chunk))) => buffer.extend_from_slice(&chunk),
                Ok(Some(Err(err))) => return Err(map_body_error(err, timeout)),
                Ok(None) => break,
                Err(_) => {
                    return Err(Error::timeout(
                        format!(
                            "Response body timed out: operation timed out \
                             (no data for {timeout:?})"
                        ),
                        Some(timeout.as_secs_f64()),
                    ));
                }
            }
        }
        Ok(buffer.freeze())
    }

    /// Convert the response body into a stream of chunks with a
    /// read-inactivity timeout.
    ///
    /// The timeout clock resets on every chunk, so long-lived streams (SSE)
    /// survive as long as data keeps arriving; only stalled streams time out.
    pub(crate) fn into_byte_stream(self) -> ByteStream {
        let Self { response, timeout } = self;
        let stream = response.into_body().into_data_stream();
        let stream = stream::unfold((stream, timeout), |(mut stream, timeout)| async move {
            let item = match tokio::time::timeout(timeout, stream.next()).await {
                // The underlying body ended; end the produced stream as well.
                Ok(None) => return None,
                Ok(Some(result)) => result.map_err(map_stream_error),
                Err(_) => Err(Error::timeout(
                    format!(
                        "HTTP stream timed out: operation timed out \
                         (no data for {timeout:?})"
                    ),
                    None,
                )),
            };
            Some((item, (stream, timeout)))
        });
        Box::pin(stream)
    }
}

/// Map a response-body hyper error to a [`crate::Error`].
fn map_body_error(err: hyper::Error, timeout: Duration) -> Error {
    let details = format_error_chain(&err);
    if err.is_timeout() || chain_is_timeout(&err) {
        Error::timeout(
            format!("Response body timed out: {details}"),
            Some(timeout.as_secs_f64()),
        )
    } else {
        Error::http_client(
            format!("Failed to read response body: {details}"),
            Some(Box::new(err)),
        )
    }
}

/// Map a byte-stream hyper error to a [`crate::Error`].
fn map_stream_error(err: hyper::Error) -> Error {
    let details = format_error_chain(&err);
    if err.is_timeout() || chain_is_timeout(&err) {
        Error::timeout(format!("HTTP stream timed out: {details}"), None)
    } else {
        Error::streaming(
            format!("Error in HTTP stream: {details}"),
            Some(Box::new(err)),
        )
    }
}

/// Returns `true` when the error chain contains an `io::Error` of the
/// `TimedOut` kind, which is how hyper surfaces connect timeouts.
fn chain_is_timeout(error: &(dyn StdError + 'static)) -> bool {
    let mut current = Some(error);
    while let Some(err) = current {
        if let Some(io) = err.downcast_ref::<std::io::Error>()
            && io.kind() == std::io::ErrorKind::TimedOut
        {
            return true;
        }
        current = err.source();
    }
    false
}
