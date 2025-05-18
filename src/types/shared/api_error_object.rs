use serde::{Serialize, Deserialize};

/// Represents an API error object returned by the Anthropic API.
///
/// This is a generic error type used for API-level errors.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApiErrorObject {
    /// A human-readable error message.
    pub message: String,

    /// The type of error, always "api_error" for this struct.
    #[serde(rename = "type")]
    pub error_type: String,
}

impl ApiErrorObject {
    /// Creates a new API error object with the specified message.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            error_type: "api_error".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_api_error_object_serialization() {
        let error = ApiErrorObject::new("An error occurred");
        let json = serde_json::to_string(&error).unwrap();
        assert_eq!(json, r#"{"message":"An error occurred","type":"api_error"}"#);
    }

    #[test]
    fn test_api_error_object_deserialization() {
        let json = r#"{"message":"An error occurred","type":"api_error"}"#;
        let error: ApiErrorObject = serde_json::from_str(json).unwrap();
        assert_eq!(error.message, "An error occurred");
        assert_eq!(error.error_type, "api_error");
    }
}