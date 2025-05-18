use serde::{Serialize, Deserialize};

/// Represents a billing error returned by the Anthropic API.
/// 
/// This error occurs when there are issues with the account's billing status,
/// such as exceeding the quota or having payment problems.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BillingError {
    /// A human-readable error message.
    pub message: String,

    /// The type of error, always "billing_error" for this struct.
    #[serde(rename = "type")]
    pub error_type: String,
}

impl BillingError {
    /// Creates a new billing error with the specified message.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            error_type: "billing_error".to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_billing_error_serialization() {
        let error = BillingError::new("You have exceeded your quota");
        let json = serde_json::to_string(&error).unwrap();
        assert_eq!(json, r#"{"message":"You have exceeded your quota","type":"billing_error"}"#);
    }

    #[test]
    fn test_billing_error_deserialization() {
        let json = r#"{"message":"You have exceeded your quota","type":"billing_error"}"#;
        let error: BillingError = serde_json::from_str(json).unwrap();
        assert_eq!(error.message, "You have exceeded your quota");
        assert_eq!(error.error_type, "billing_error");
    }
}