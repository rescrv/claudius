use std::ops::Add;

use serde::{Deserialize, Serialize};

/// Breakdown of output tokens by category.
#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OutputTokensDetails {
    /// The number of output tokens generated as internal reasoning.
    pub thinking_tokens: i32,
}

impl OutputTokensDetails {
    /// Create a new output-token breakdown.
    pub fn new(thinking_tokens: i32) -> Self {
        Self { thinking_tokens }
    }
}

impl Add for OutputTokensDetails {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self {
            thinking_tokens: self.thinking_tokens + rhs.thinking_tokens,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, to_value};

    #[test]
    fn output_tokens_details_serialization() {
        let details = OutputTokensDetails::new(312);
        let json = to_value(details).unwrap();

        assert_eq!(
            json,
            json!({
                "thinking_tokens": 312
            })
        );
    }

    #[test]
    fn output_tokens_details_deserialization() {
        let json = json!({
            "thinking_tokens": 312
        });

        let details: OutputTokensDetails = serde_json::from_value(json).unwrap();
        assert_eq!(details.thinking_tokens, 312);
    }

    #[test]
    fn output_tokens_details_adds() {
        let left = OutputTokensDetails::new(10);
        let right = OutputTokensDetails::new(20);

        assert_eq!(left + right, OutputTokensDetails::new(30));
    }
}
