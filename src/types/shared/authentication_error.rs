use serde::{Serialize, Deserialize};

/// Represents an authentication error returned by the Anthropic API.
/// 
/// This error occurs when the API key is invalid or missing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AuthenticationError {
    /// A human-readable error message.
    pub message: String,

    /// The type of error, always "authentication_error" for this struct.
    #[serde(rename = "type")]
    pub error_type: String,
}

impl AuthenticationError {
    /// Creates a new authentication error with the specified message.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            error_type: "authentication_error".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_authentication_error_serialization() {
        let error = AuthenticationError::new("Invalid API key");
        let json = serde_json::to_string(&error).unwrap();
        assert_eq!(json, r#"{"message":"Invalid API key","type":"authentication_error"}"#);
    }

    #[test]
    fn test_authentication_error_deserialization() {
        let json = r#"{"message":"Invalid API key","type":"authentication_error"}"#;
        let error: AuthenticationError = serde_json::from_str(json).unwrap();
        assert_eq!(error.message, "Invalid API key");
        assert_eq!(error.error_type, "authentication_error");
    }
}