use serde::{Deserialize, Serialize};

/// Represents a not found error returned by the Anthropic API.
///
/// This error occurs when a requested resource cannot be found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NotFoundError {
    /// A human-readable error message.
    pub message: String,

    /// The type of error, always "not_found_error" for this struct.
    #[serde(rename = "type")]
    pub error_type: String,
}

impl NotFoundError {
    /// Creates a new not found error with the specified message.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            error_type: "not_found_error".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_not_found_error_serialization() {
        let error = NotFoundError::new("Resource not found");
        let json = serde_json::to_string(&error).unwrap();
        assert_eq!(
            json,
            r#"{"message":"Resource not found","type":"not_found_error"}"#
        );
    }

    #[test]
    fn test_not_found_error_deserialization() {
        let json = r#"{"message":"Resource not found","type":"not_found_error"}"#;
        let error: NotFoundError = serde_json::from_str(json).unwrap();
        assert_eq!(error.message, "Resource not found");
        assert_eq!(error.error_type, "not_found_error");
    }
}
