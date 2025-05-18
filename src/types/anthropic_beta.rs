use std::fmt;
use serde::{Serialize, Deserialize};

/// Represents an Anthropic beta feature identifier.
///
/// This can be a predefined beta feature identifier or a custom string value
/// for beta features that may be added in the future.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AnthropicBeta {
    /// Known beta feature identifiers
    #[serde(rename_all = "kebab-case")]
    Known(KnownBeta),
    
    /// Custom beta feature identifier (for future beta features)
    Custom(String),
}

/// Known Anthropic beta feature identifiers
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum KnownBeta {
    /// Message batches feature (2024-09-24)
    MessageBatches20240924,
    
    /// Prompt caching feature (2024-07-31)
    PromptCaching20240731,
    
    /// Computer use feature (2024-10-22)
    ComputerUse20241022,
    
    /// Computer use feature (2025-01-24)
    ComputerUse20250124,
    
    /// PDFs feature (2024-09-25)
    Pdfs20240925,
    
    /// Token counting feature (2024-11-01)
    TokenCounting20241101,
    
    /// Token efficient tools feature (2025-02-19)
    TokenEfficientTools20250219,
    
    /// 128K output feature (2025-02-19)
    Output128k20250219,
}

impl fmt::Display for AnthropicBeta {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AnthropicBeta::Known(known_beta) => write!(f, "{}", known_beta),
            AnthropicBeta::Custom(custom) => write!(f, "{}", custom),
        }
    }
}

impl fmt::Display for KnownBeta {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KnownBeta::MessageBatches20240924 => write!(f, "message-batches-2024-09-24"),
            KnownBeta::PromptCaching20240731 => write!(f, "prompt-caching-2024-07-31"),
            KnownBeta::ComputerUse20241022 => write!(f, "computer-use-2024-10-22"),
            KnownBeta::ComputerUse20250124 => write!(f, "computer-use-2025-01-24"),
            KnownBeta::Pdfs20240925 => write!(f, "pdfs-2024-09-25"),
            KnownBeta::TokenCounting20241101 => write!(f, "token-counting-2024-11-01"),
            KnownBeta::TokenEfficientTools20250219 => write!(f, "token-efficient-tools-2025-02-19"),
            KnownBeta::Output128k20250219 => write!(f, "output-128k-2025-02-19"),
        }
    }
}

impl From<KnownBeta> for AnthropicBeta {
    fn from(beta: KnownBeta) -> Self {
        AnthropicBeta::Known(beta)
    }
}

impl From<String> for AnthropicBeta {
    fn from(beta: String) -> Self {
        AnthropicBeta::Custom(beta)
    }
}

impl From<&str> for AnthropicBeta {
    fn from(beta: &str) -> Self {
        AnthropicBeta::Custom(beta.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_known_beta_serialization() {
        let beta = AnthropicBeta::Known(KnownBeta::MessageBatches20240924);
        let json = serde_json::to_string(&beta).unwrap();
        assert_eq!(json, r#""message-batches-2024-09-24""#);
        
        let beta = AnthropicBeta::Known(KnownBeta::ComputerUse20250124);
        let json = serde_json::to_string(&beta).unwrap();
        assert_eq!(json, r#""computer-use-2025-01-24""#);
    }
    
    #[test]
    fn test_custom_beta_serialization() {
        let beta = AnthropicBeta::Custom("new-feature-2025-06-15".to_string());
        let json = serde_json::to_string(&beta).unwrap();
        assert_eq!(json, r#""new-feature-2025-06-15""#);
    }
    
    #[test]
    fn test_beta_deserialization() {
        let json = r#""message-batches-2024-09-24""#;
        let beta: AnthropicBeta = serde_json::from_str(json).unwrap();
        assert_eq!(beta, AnthropicBeta::Known(KnownBeta::MessageBatches20240924));
        
        let json = r#""new-feature-2025-06-15""#;
        let beta: AnthropicBeta = serde_json::from_str(json).unwrap();
        assert_eq!(beta, AnthropicBeta::Custom("new-feature-2025-06-15".to_string()));
    }
    
    #[test]
    fn test_display() {
        let beta = AnthropicBeta::Known(KnownBeta::MessageBatches20240924);
        assert_eq!(beta.to_string(), "message-batches-2024-09-24");
        
        let beta = AnthropicBeta::Custom("new-feature-2025-06-15".to_string());
        assert_eq!(beta.to_string(), "new-feature-2025-06-15");
    }
}