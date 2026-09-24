//! Backend-neutral HTTP transport.
//!
//! claudius supports two HTTP backends, selected by cargo features:
//!
//! - `reqwest-backend` (default): requests are issued with [`reqwest`].
//! - `hyper-boring`: requests are issued with [`hyper`] over BoringSSL via
//!   [`hyper_boring`].
//!
//! Both backends expose the same [`Transport`] and [`HttpResponse`] interface,
//! which is the only HTTP API the rest of the crate touches. Response bodies
//! are surfaced as [`ByteStream`]s whose errors are already mapped to
//! [`crate::Error`], so consumers such as [`crate::sse`] are backend agnostic.

use std::error::Error as StdError;
use std::pin::Pin;

use bytes::Bytes;
use futures::Stream;
use http::{HeaderMap, Method};

use crate::Result;

#[cfg(all(feature = "reqwest-backend", not(feature = "hyper-boring")))]
mod reqwest_backend;
#[cfg(all(feature = "reqwest-backend", not(feature = "hyper-boring")))]
pub(crate) use reqwest_backend::{HttpResponse, Transport};

#[cfg(feature = "hyper-boring")]
mod hyper_boring_backend;
#[cfg(feature = "hyper-boring")]
pub(crate) use hyper_boring_backend::{HttpResponse, Transport};

#[cfg(not(any(feature = "reqwest-backend", feature = "hyper-boring")))]
compile_error!(
    "claudius requires an HTTP backend: enable the default `reqwest-backend` \
     feature or the `hyper-boring` feature"
);

/// A stream of response body chunks with backend errors already mapped to
/// [`crate::Error`].
pub(crate) type ByteStream = Pin<Box<dyn Stream<Item = Result<Bytes>> + Send>>;

/// A backend-neutral HTTP request.
///
/// The body, when present, is already serialized; the transport only moves
/// bytes.
#[derive(Debug)]
pub(crate) struct HttpRequest {
    /// The HTTP method.
    pub(crate) method: Method,
    /// The absolute request URL, including any query string.
    pub(crate) url: String,
    /// The request headers.
    pub(crate) headers: HeaderMap,
    /// The serialized request body, or `None` for an empty body.
    pub(crate) body: Option<Vec<u8>>,
}

impl HttpRequest {
    /// Build a `GET` request.
    pub(crate) fn get(url: String, headers: HeaderMap) -> Self {
        Self {
            method: Method::GET,
            url,
            headers,
            body: None,
        }
    }

    /// Build a `POST` request carrying a pre-serialized body.
    pub(crate) fn post_body(url: String, headers: HeaderMap, body: Vec<u8>) -> Self {
        Self {
            method: Method::POST,
            url,
            headers,
            body: Some(body),
        }
    }

    /// Build a `POST` request with an empty body.
    pub(crate) fn post_empty(url: String, headers: HeaderMap) -> Self {
        Self {
            method: Method::POST,
            url,
            headers,
            body: None,
        }
    }

    /// Build a `DELETE` request.
    pub(crate) fn delete(url: String, headers: HeaderMap) -> Self {
        Self {
            method: Method::DELETE,
            url,
            headers,
            body: None,
        }
    }
}

/// Append `params` to `url` as a percent-encoded query string.
///
/// The encoding matches what the reqwest backend's `query` support produces.
/// A URL without parameters is returned unchanged.
pub(crate) fn append_query_params(url: &str, params: &[(String, String)]) -> String {
    if params.is_empty() {
        return url.to_string();
    }
    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
    for (key, value) in params {
        serializer.append_pair(key, value);
    }
    format!("{url}?{}", serializer.finish())
}

/// Format an error and its whole source chain into one string, eliding
/// duplicated messages.
///
/// Both backends surface transport errors this way so that error messages are
/// comparable regardless of the backend in use.
pub(crate) fn format_error_chain(error: &(dyn StdError + 'static)) -> String {
    let mut parts = vec![error.to_string()];
    let mut source = error.source();
    while let Some(inner) = source {
        let detail = inner.to_string();
        if !parts.iter().any(|part| part == &detail) {
            parts.push(detail);
        }
        source = inner.source();
    }
    parts.join(": ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_params_are_percent_encoded() {
        let params = vec![
            ("after_id".to_string(), "msgbatch_01ABC def".to_string()),
            ("limit".to_string(), "10".to_string()),
        ];
        let url = append_query_params("http://localhost/v1/messages/batches", &params);
        assert_eq!(
            url,
            "http://localhost/v1/messages/batches?after_id=msgbatch_01ABC+def&limit=10"
        );
    }

    #[test]
    fn query_params_without_entries_leave_url_untouched() {
        let url = append_query_params("http://localhost/v1/models", &[]);
        assert_eq!(url, "http://localhost/v1/models");
    }
}
