use serde::{Deserialize, Serialize};

/// Parameters for listing message batches.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct BatchListParams {
    /// ID of the object to use as a cursor for pagination.
    ///
    /// When provided, returns the page of results immediately after this object.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after_id: Option<String>,

    /// ID of the object to use as a cursor for pagination.
    ///
    /// When provided, returns the page of results immediately before this object.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before_id: Option<String>,

    /// Number of items to return per page.
    ///
    /// Defaults to `20`. Ranges from `1` to `1000`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

impl BatchListParams {
    /// Create a new empty `BatchListParams`.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the after_id.
    pub fn with_after_id(mut self, after_id: String) -> Self {
        self.after_id = Some(after_id);
        self
    }

    /// Set the before_id.
    pub fn with_before_id(mut self, before_id: String) -> Self {
        self.before_id = Some(before_id);
        self
    }

    /// Set the limit.
    pub fn with_limit(mut self, limit: u32) -> Self {
        self.limit = Some(limit);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, to_value};

    #[test]
    fn test_batch_list_params_serialization() {
        let params = BatchListParams::new()
            .with_after_id("batch_123".to_string())
            .with_limit(50);

        let json = to_value(&params).unwrap();

        assert_eq!(
            json,
            json!({
                "after_id": "batch_123",
                "limit": 50
            })
        );
    }

    #[test]
    fn test_batch_list_params_serialization_empty() {
        let params = BatchListParams::new();
        let json = to_value(&params).unwrap();

        assert_eq!(json, json!({}));
    }

    #[test]
    fn test_batch_list_params_deserialization() {
        let json = json!({
            "after_id": "batch_123",
            "limit": 50
        });

        let params: BatchListParams = serde_json::from_value(json).unwrap();

        assert_eq!(params.after_id, Some("batch_123".to_string()));
        assert_eq!(params.before_id, None);
        assert_eq!(params.limit, Some(50));
    }
}
