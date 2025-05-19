use serde::{Deserialize, Serialize};

/// Represents an overloaded error returned by the Anthropic API.
///
/// This error occurs when the API is experiencing high traffic and cannot process the request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverloadedError {
    /// A human-readable error message.
    pub message: String,

    /// The type of error, always "overloaded_error" for this struct.
    #[serde(rename = "type")]
    pub error_type: String,
}

impl OverloadedError {
    /// Creates a new overloaded error with the specified message.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            error_type: "overloaded_error".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_overloaded_error_serialization() {
        let error = OverloadedError::new("Server is overloaded");
        let json = serde_json::to_string(&error).unwrap();
        assert_eq!(
            json,
            r#"{"message":"Server is overloaded","type":"overloaded_error"}"#
        );
    }

    #[test]
    fn test_overloaded_error_deserialization() {
        let json = r#"{"message":"Server is overloaded","type":"overloaded_error"}"#;
        let error: OverloadedError = serde_json::from_str(json).unwrap();
        assert_eq!(error.message, "Server is overloaded");
        assert_eq!(error.error_type, "overloaded_error");
    }
}
