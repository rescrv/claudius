use serde::{Deserialize, Serialize};

/// Represents a gateway timeout error returned by the Anthropic API.
///
/// This error occurs when the API request times out at the gateway level.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayTimeoutError {
    /// A human-readable error message.
    pub message: String,

    /// The type of error, always "timeout_error" for this struct.
    #[serde(rename = "type")]
    pub error_type: String,
}

impl GatewayTimeoutError {
    /// Creates a new gateway timeout error with the specified message.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            error_type: "timeout_error".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gateway_timeout_error_serialization() {
        let error = GatewayTimeoutError::new("Request timed out");
        let json = serde_json::to_string(&error).unwrap();
        assert_eq!(
            json,
            r#"{"message":"Request timed out","type":"timeout_error"}"#
        );
    }

    #[test]
    fn test_gateway_timeout_error_deserialization() {
        let json = r#"{"message":"Request timed out","type":"timeout_error"}"#;
        let error: GatewayTimeoutError = serde_json::from_str(json).unwrap();
        assert_eq!(error.message, "Request timed out");
        assert_eq!(error.error_type, "timeout_error");
    }
}
