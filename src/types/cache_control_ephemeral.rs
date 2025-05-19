use serde::{Deserialize, Serialize};

/// CacheControlEphemeral specifies that content should be ephemeral, meaning it should
/// not be cached or persisted beyond the immediate request.
///
/// This is useful for sensitive information that shouldn't be stored long-term.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CacheControlEphemeral {
    /// The type is always "ephemeral" for this struct
    #[serde(default = "default_type")]
    pub r#type: String,
}

fn default_type() -> String {
    "ephemeral".to_string()
}

impl CacheControlEphemeral {
    /// Creates a new CacheControlEphemeral instance
    pub fn new() -> Self {
        Self {
            r#type: default_type(),
        }
    }
}

impl Default for CacheControlEphemeral {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialization() {
        let cache_control = CacheControlEphemeral::new();

        let json = serde_json::to_string(&cache_control).unwrap();
        let expected = r#"{"type":"ephemeral"}"#;

        assert_eq!(json, expected);
    }

    #[test]
    fn test_deserialization() {
        let json = r#"{"type":"ephemeral"}"#;
        let cache_control: CacheControlEphemeral = serde_json::from_str(json).unwrap();

        assert_eq!(cache_control.r#type, "ephemeral");
    }
}
