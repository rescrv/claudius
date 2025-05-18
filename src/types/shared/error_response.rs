use serde::{Serialize, Deserialize};

use super::error_object::ErrorObject;

/// Represents an error response returned by the Anthropic API.
///
/// This is the top-level structure that contains an error object with details about what went wrong.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorResponse {
    /// The error object containing specific error details.
    pub error: ErrorObject,

    /// The type of the response, always "error" for this struct.
    #[serde(rename = "type")]
    pub response_type: String,
}

impl ErrorResponse {
    /// Creates a new error response with the specified error object.
    pub fn new(error: ErrorObject) -> Self {
        Self {
            error,
            response_type: "error".to_string(),
        }
    }

    /// Returns the error message from the contained error object.
    pub fn message(&self) -> &str {
        self.error.message()
    }

    /// Returns the error type from the contained error object.
    pub fn error_type(&self) -> &str {
        self.error.error_type()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::authentication_error::AuthenticationError;

    #[test]
    fn test_error_response_serialization() {
        let auth_error = AuthenticationError::new("Invalid API key");
        let error_response = ErrorResponse::new(ErrorObject::Authentication(auth_error));
        
        let json = serde_json::to_string(&error_response).unwrap();
        assert!(json.contains(r#""type":"error""#));
        assert!(json.contains(r#""error":{"message":"Invalid API key","type":"authentication_error"}"#));
    }

    #[test]
    fn test_error_response_deserialization() {
        let json = r#"{"error":{"message":"Invalid API key","type":"authentication_error"},"type":"error"}"#;
        let error_response: ErrorResponse = serde_json::from_str(json).unwrap();
        
        assert_eq!(error_response.response_type, "error");
        match &error_response.error {
            ErrorObject::Authentication(auth_error) => {
                assert_eq!(auth_error.message, "Invalid API key");
                assert_eq!(auth_error.error_type, "authentication_error");
            },
            _ => panic!("Expected an authentication error"),
        }
    }

    #[test]
    fn test_error_response_getters() {
        let auth_error = AuthenticationError::new("Invalid API key");
        let error_response = ErrorResponse::new(ErrorObject::Authentication(auth_error));
        
        assert_eq!(error_response.message(), "Invalid API key");
        assert_eq!(error_response.error_type(), "authentication_error");
    }
}