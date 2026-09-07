use serde::{Deserialize, Serialize};

use crate::types::Model;

/// A model endpoint on either side of a fallback boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FallbackModel {
    /// Model at this side of the boundary.
    pub model: Model,
}

/// Marks where generation moved from a refusing model to a fallback model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FallbackBlock {
    /// Model that refused the request.
    pub from: FallbackModel,
    /// Model that continued generation.
    pub to: FallbackModel,
}

impl FallbackBlock {
    /// Create a fallback boundary between two models.
    pub fn new(from: impl Into<Model>, to: impl Into<Model>) -> Self {
        Self {
            from: FallbackModel { model: from.into() },
            to: FallbackModel { model: to.into() },
        }
    }
}
