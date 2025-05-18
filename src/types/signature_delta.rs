use serde::{Deserialize, Serialize};

/// A signature delta, representing a piece of a signature in a streaming response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SignatureDelta {
    /// The signature content.
    pub signature: String,
    
    /// The type, which is always "signature_delta".
    pub r#type: String,
}

impl SignatureDelta {
    /// Create a new `SignatureDelta` with the given signature.
    pub fn new(signature: String) -> Self {
        Self {
            signature,
            r#type: "signature_delta".to_string(),
        }
    }
    
    /// Create a new `SignatureDelta` from a string reference.
    pub fn from_str(signature: &str) -> Self {
        Self::new(signature.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, to_value};

    #[test]
    fn test_signature_delta_serialization() {
        let delta = SignatureDelta::new("Robert Paulson".to_string());
        let json = to_value(&delta).unwrap();
        
        assert_eq!(
            json,
            json!({
                "signature": "Robert Paulson",
                "type": "signature_delta"
            })
        );
    }

    #[test]
    fn test_signature_delta_deserialization() {
        let json = json!({
            "signature": "Robert Paulson",
            "type": "signature_delta"
        });
        
        let delta: SignatureDelta = serde_json::from_value(json).unwrap();
        assert_eq!(delta.signature, "Robert Paulson");
        assert_eq!(delta.r#type, "signature_delta");
    }
}