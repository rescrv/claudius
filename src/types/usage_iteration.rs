use serde::{Deserialize, Serialize};

use crate::types::Model;

/// Kind of model attempt recorded for fallback billing.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageIterationType {
    /// The requested model attempted the message.
    Message,
    /// A fallback model attempted the message.
    FallbackMessage,
}

/// Per-model token usage for a request that may have fallen back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageIteration {
    /// Attempt kind.
    pub r#type: UsageIterationType,
    /// Model that ran the attempt.
    pub model: Model,
    /// Input tokens consumed by this attempt.
    pub input_tokens: i32,
    /// Output tokens consumed by this attempt.
    pub output_tokens: i32,
    /// Tokens written to the prompt cache.
    pub cache_creation_input_tokens: i32,
    /// Tokens read from the prompt cache.
    pub cache_read_input_tokens: i32,
}
