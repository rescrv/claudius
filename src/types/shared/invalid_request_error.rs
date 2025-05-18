use serde::{Serialize, Deserialize};

/// Represents an invalid request error returned by the Anthropic API.
/// 
/// This error occurs when the request is malformed or contains invalid parameters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvalidRequestError {
    /// A human-readable error message.
    pub message: String,

    /// The type of error, always "invalid_request_error" for this struct.
    #[serde(rename = "type")]
    pub error_type: String,
}

impl InvalidRequestError {
    /// Creates a new invalid request error with the specified message.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            error_type: "invalid_request_error".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_invalid_request_error_serialization() {
        let error = InvalidRequestError::new("Invalid parameter: max_tokens");
        let json = serde_json::to_string(&error).unwrap();
        assert_eq!(json, r#"{"message":"Invalid parameter: max_tokens","type":"invalid_request_error"}"#);
    }

    #[test]
    fn test_invalid_request_error_deserialization() {
        let json = r#"{"message":"Invalid parameter: max_tokens","type":"invalid_request_error"}"#;
        let error: InvalidRequestError = serde_json::from_str(json).unwrap();
        assert_eq!(error.message, "Invalid parameter: max_tokens");
        assert_eq!(error.error_type, "invalid_request_error");
    }
}