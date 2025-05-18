use serde::{Serialize, Deserialize};

use super::api_error_object::ApiErrorObject;
use super::authentication_error::AuthenticationError;
use super::billing_error::BillingError;
use super::gateway_timeout_error::GatewayTimeoutError;
use super::invalid_request_error::InvalidRequestError;
use super::not_found_error::NotFoundError;
use super::overloaded_error::OverloadedError;
use super::permission_error::PermissionError;
use super::rate_limit_error::RateLimitError;

/// A union type representing all possible error types that can be returned by the Anthropic API.
///
/// Each variant corresponds to a specific error type with its own structure and semantics.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ErrorObject {
    /// Error indicating a general API error
    #[serde(rename = "api_error")]
    ApiError(ApiErrorObject),

    /// Error indicating an authentication issue
    #[serde(rename = "authentication_error")]
    Authentication(AuthenticationError),

    /// Error indicating a billing related issue
    #[serde(rename = "billing_error")]
    Billing(BillingError),

    /// Error indicating a gateway timeout
    #[serde(rename = "timeout_error")]
    GatewayTimeout(GatewayTimeoutError),

    /// Error indicating an invalid request
    #[serde(rename = "invalid_request_error")]
    InvalidRequest(InvalidRequestError),

    /// Error indicating a resource was not found
    #[serde(rename = "not_found_error")]
    NotFound(NotFoundError),

    /// Error indicating the server is overloaded
    #[serde(rename = "overloaded_error")]
    Overloaded(OverloadedError),

    /// Error indicating a permission issue
    #[serde(rename = "permission_error")]
    Permission(PermissionError),

    /// Error indicating too many requests were made
    #[serde(rename = "rate_limit_error")]
    RateLimit(RateLimitError),
}

impl ErrorObject {
    /// Returns the error message for this error object.
    pub fn message(&self) -> &str {
        match self {
            ErrorObject::ApiError(err) => &err.message,
            ErrorObject::Authentication(err) => &err.message,
            ErrorObject::Billing(err) => &err.message,
            ErrorObject::GatewayTimeout(err) => &err.message,
            ErrorObject::InvalidRequest(err) => &err.message,
            ErrorObject::NotFound(err) => &err.message,
            ErrorObject::Overloaded(err) => &err.message,
            ErrorObject::Permission(err) => &err.message,
            ErrorObject::RateLimit(err) => &err.message,
        }
    }

    /// Returns the error type as a string.
    pub fn error_type(&self) -> &str {
        match self {
            ErrorObject::ApiError(_) => "api_error",
            ErrorObject::Authentication(_) => "authentication_error",
            ErrorObject::Billing(_) => "billing_error",
            ErrorObject::GatewayTimeout(_) => "timeout_error",
            ErrorObject::InvalidRequest(_) => "invalid_request_error",
            ErrorObject::NotFound(_) => "not_found_error",
            ErrorObject::Overloaded(_) => "overloaded_error",
            ErrorObject::Permission(_) => "permission_error",
            ErrorObject::RateLimit(_) => "rate_limit_error",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_object_api_error_serialization() {
        let error = ErrorObject::ApiError(ApiErrorObject::new("An API error occurred"));
        let json = serde_json::to_string(&error).unwrap();
        assert!(json.contains(r#""type":"api_error""#));
        assert!(json.contains(r#""message":"An API error occurred""#));
    }

    #[test]
    fn test_error_object_authentication_error_serialization() {
        let error = ErrorObject::Authentication(AuthenticationError::new("Invalid API key"));
        let json = serde_json::to_string(&error).unwrap();
        assert!(json.contains(r#""type":"authentication_error""#));
        assert!(json.contains(r#""message":"Invalid API key""#));
    }

    #[test]
    fn test_error_object_deserialization() {
        let json = r#"{"message":"Invalid API key","type":"authentication_error"}"#;
        let error: ErrorObject = serde_json::from_str(json).unwrap();
        
        match error {
            ErrorObject::Authentication(auth_error) => {
                assert_eq!(auth_error.message, "Invalid API key");
                assert_eq!(auth_error.error_type, "authentication_error");
            },
            _ => panic!("Expected an authentication error"),
        }
    }

    #[test]
    fn test_error_object_getters() {
        let error = ErrorObject::RateLimit(RateLimitError::new("Too many requests"));
        assert_eq!(error.message(), "Too many requests");
        assert_eq!(error.error_type(), "rate_limit_error");
    }
}