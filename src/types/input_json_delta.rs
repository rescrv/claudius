use serde::{Deserialize, Serialize};

/// A JSON delta, representing a piece of JSON in a streaming response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InputJsonDelta {
    /// The partial JSON content.
    #[serde(rename = "partial_json")]
    pub partial_json: String,
}

impl InputJsonDelta {
    /// Create a new `InputJsonDelta` with the given partial JSON.
    pub fn new(partial_json: String) -> Self {
        Self { partial_json }
    }

    /// Create a new `InputJsonDelta` from a string reference.
    // TODO(claude): no from_string_ref
    pub fn from_string_ref(partial_json: &str) -> Self {
        Self::new(partial_json.to_string())
    }
}

impl std::str::FromStr for InputJsonDelta {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::from_string_ref(s))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, to_value};

    #[test]
    fn test_input_json_delta_serialization() {
        let delta = InputJsonDelta::new(r#"{"key":"#.to_string());
        let json = to_value(&delta).unwrap();

        assert_eq!(
            json,
            json!({
                "partial_json": r#"{"key":"#
            })
        );
    }

    #[test]
    fn test_input_json_delta_deserialization() {
        let json = json!({
            "partial_json": r#"{"key":"#
        });

        let delta: InputJsonDelta = serde_json::from_value(json).unwrap();
        assert_eq!(delta.partial_json, r#"{"key":"#);
    }

    #[test]
    fn test_from_str() {
        let delta = "partial json".parse::<InputJsonDelta>().unwrap();
        assert_eq!(delta.partial_json, "partial json");
    }
}
