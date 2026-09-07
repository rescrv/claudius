use serde::{Deserialize, Serialize};

use crate::types::Model;

/// Additional details returned when generation stops with a refusal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StopDetails {
    /// Detail discriminator. Currently `refusal`.
    pub r#type: String,
    /// Stable refusal category, when one applies.
    pub category: Option<String>,
    /// Human-readable explanation; callers should display rather than parse it.
    pub explanation: Option<String>,
    /// Model suggested for a direct retry when server-side fallback could not run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommended_model: Option<Model>,
    /// Opaque token used by the fallback-credit beta.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_credit_token: Option<String>,
    /// Whether the fallback credit permits an appended assistant prefill.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fallback_has_prefill_claim: Option<bool>,
}
