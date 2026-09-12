use serde::{Deserialize, Serialize};

/// Indicates whether a model capability is supported.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilitySupport {
    /// Whether the capability is supported.
    pub supported: bool,
}

/// Context-management features exposed by the Models API.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextManagementCapability {
    /// Support for clearing thinking blocks.
    pub clear_thinking_20251015: Option<CapabilitySupport>,
    /// Support for clearing tool-use blocks.
    pub clear_tool_uses_20250919: Option<CapabilitySupport>,
    /// Support for server-side compaction.
    pub compact_20260112: Option<CapabilitySupport>,
    /// Whether any context-management strategy is supported.
    pub supported: bool,
}

/// Effort levels exposed by the Models API.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffortCapability {
    /// High effort support.
    pub high: CapabilitySupport,
    /// Low effort support.
    pub low: CapabilitySupport,
    /// Maximum effort support.
    pub max: CapabilitySupport,
    /// Medium effort support.
    pub medium: CapabilitySupport,
    /// Whether effort control is supported.
    pub supported: bool,
    /// Extra-high effort support, when reported.
    pub xhigh: Option<CapabilitySupport>,
}

/// Thinking modes exposed by the Models API.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThinkingTypes {
    /// Adaptive thinking support.
    pub adaptive: CapabilitySupport,
    /// Manual extended-thinking support.
    pub enabled: CapabilitySupport,
}

/// Thinking support exposed by the Models API.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThinkingCapability {
    /// Whether any thinking mode is supported.
    pub supported: bool,
    /// Supported thinking configurations.
    pub types: ThinkingTypes,
}

/// Capability metadata returned for a model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelCapabilities {
    /// Message Batches API support.
    pub batch: CapabilitySupport,
    /// Citation generation support.
    pub citations: CapabilitySupport,
    /// Code execution support.
    pub code_execution: CapabilitySupport,
    /// Context-management support and strategies.
    pub context_management: ContextManagementCapability,
    /// Effort-control support and levels.
    pub effort: EffortCapability,
    /// Image input support.
    pub image_input: CapabilitySupport,
    /// PDF input support.
    pub pdf_input: CapabilitySupport,
    /// Structured output and strict tool-schema support.
    pub structured_outputs: CapabilitySupport,
    /// Thinking support and modes.
    pub thinking: ThinkingCapability,
}
