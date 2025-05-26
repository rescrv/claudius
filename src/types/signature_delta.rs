use serde::{Deserialize, Serialize};

/// A signature delta, representing a piece of a signature in a streaming response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SignatureDelta {
    /// The signature content.
    pub signature: String,
}

impl SignatureDelta {
    /// Create a new `SignatureDelta` with the given signature.
    pub fn new(signature: String) -> Self {
        Self { signature }
    }
}

impl std::str::FromStr for SignatureDelta {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::new(s.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, to_value};

    #[test]
    fn signature_delta_serialization() {
        let delta = SignatureDelta::new("Robert Paulson".to_string());
        let json = to_value(&delta).unwrap();

        assert_eq!(
            json,
            json!({
                "signature": "Robert Paulson"
            })
        );
    }

    #[test]
    fn signature_delta_deserialization() {
        let json = json!({
            "signature": "Robert Paulson"
        });

        let delta: SignatureDelta = serde_json::from_value(json).unwrap();
        assert_eq!(delta.signature, "Robert Paulson");
    }

    #[test]
    fn from_str() {
        let delta = "Robert Paulson".parse::<SignatureDelta>().unwrap();
        assert_eq!(delta.signature, "Robert Paulson");
    }
}
