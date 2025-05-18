use serde::{Serialize, Deserialize};

/// Represents a rate limit error returned by the Anthropic API.
/// 
/// This error occurs when the client has sent too many requests in a given amount of time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RateLimitError {
    /// A human-readable error message.
    pub message: String,

    /// The type of error, always "rate_limit_error" for this struct.
    #[serde(rename = "type")]
    pub error_type: String,
}

impl RateLimitError {
    /// Creates a new rate limit error with the specified message.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            error_type: "rate_limit_error".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rate_limit_error_serialization() {
        let error = RateLimitError::new("Too many requests, please try again later");
        let json = serde_json::to_string(&error).unwrap();
        assert_eq!(json, r#"{"message":"Too many requests, please try again later","type":"rate_limit_error"}"#);
    }

    #[test]
    fn test_rate_limit_error_deserialization() {
        let json = r#"{"message":"Too many requests, please try again later","type":"rate_limit_error"}"#;
        let error: RateLimitError = serde_json::from_str(json).unwrap();
        assert_eq!(error.message, "Too many requests, please try again later");
        assert_eq!(error.error_type, "rate_limit_error");
    }
}