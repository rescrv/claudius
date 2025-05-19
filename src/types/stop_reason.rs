use serde::{Deserialize, Serialize};
use std::fmt;

/// Reasons why the model stopped generating a response.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    /// The model reached the end of a generated turn
    EndTurn,

    /// The response reached the maximum token limit for the response
    MaxTokens,

    /// The model reached a specified stop sequence
    StopSequence,

    /// The model indicated it wants to use a tool
    ToolUse,

    /// The model paused in the middle of a turn
    PauseTurn,

    /// The model refused to respond due to safety or other considerations
    Refusal,
}

impl fmt::Display for StopReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StopReason::EndTurn => write!(f, "end_turn"),
            StopReason::MaxTokens => write!(f, "max_tokens"),
            StopReason::StopSequence => write!(f, "stop_sequence"),
            StopReason::ToolUse => write!(f, "tool_use"),
            StopReason::PauseTurn => write!(f, "pause_turn"),
            StopReason::Refusal => write!(f, "refusal"),
        }
    }
}

impl From<&str> for StopReason {
    fn from(reason: &str) -> Self {
        match reason {
            "end_turn" => StopReason::EndTurn,
            "max_tokens" => StopReason::MaxTokens,
            "stop_sequence" => StopReason::StopSequence,
            "tool_use" => StopReason::ToolUse,
            "pause_turn" => StopReason::PauseTurn,
            "refusal" => StopReason::Refusal,
            _ => panic!("Unknown stop reason: {}", reason),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serialization() {
        let reason = StopReason::EndTurn;
        let json = serde_json::to_string(&reason).unwrap();
        assert_eq!(json, r#""end_turn""#);

        let reason = StopReason::MaxTokens;
        let json = serde_json::to_string(&reason).unwrap();
        assert_eq!(json, r#""max_tokens""#);
    }

    #[test]
    fn test_deserialization() {
        let json = r#""end_turn""#;
        let reason: StopReason = serde_json::from_str(json).unwrap();
        assert_eq!(reason, StopReason::EndTurn);

        let json = r#""stop_sequence""#;
        let reason: StopReason = serde_json::from_str(json).unwrap();
        assert_eq!(reason, StopReason::StopSequence);
    }

    #[test]
    fn test_display() {
        let reason = StopReason::EndTurn;
        assert_eq!(reason.to_string(), "end_turn");

        let reason = StopReason::MaxTokens;
        assert_eq!(reason.to_string(), "max_tokens");
    }
}
