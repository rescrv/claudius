use serde::{Deserialize, Serialize};

/// Represents a permission error returned by the Anthropic API.
///
/// This error occurs when the API key does not have permission to access the requested resource.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionError {
    /// A human-readable error message.
    pub message: String,

    /// The type of error, always "permission_error" for this struct.
    #[serde(rename = "type")]
    pub error_type: String,
}

impl PermissionError {
    /// Creates a new permission error with the specified message.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            error_type: "permission_error".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_permission_error_serialization() {
        let error = PermissionError::new("You don't have permission to access this resource");
        let json = serde_json::to_string(&error).unwrap();
        assert_eq!(
            json,
            r#"{"message":"You don't have permission to access this resource","type":"permission_error"}"#
        );
    }

    #[test]
    fn test_permission_error_deserialization() {
        let json = r#"{"message":"You don't have permission to access this resource","type":"permission_error"}"#;
        let error: PermissionError = serde_json::from_str(json).unwrap();
        assert_eq!(
            error.message,
            "You don't have permission to access this resource"
        );
        assert_eq!(error.error_type, "permission_error");
    }
}
