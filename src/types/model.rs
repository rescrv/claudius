use serde::{Deserialize, Serialize, Serializer};
use std::fmt;
use std::str::FromStr;

/// Represents an Anthropic model identifier.
///
/// This can be a predefined model version or a custom string value
/// for models that may be added in the future.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Model {
    /// Known model versions
    Known(KnownModel),

    /// Custom model identifier (for future models or private models)
    Custom(String),
}

/// Known Anthropic model versions
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum KnownModel {
    /// Claude 3.7 Sonnet (latest version)
    Claude37SonnetLatest,

    /// Claude 3.7 Sonnet (2025-02-19 version)
    Claude37Sonnet20250219,

    /// Claude 3.5 Haiku (latest version)
    Claude35HaikuLatest,

    /// Claude 3.5 Haiku (2024-10-22 version)
    Claude35Haiku20241022,

    /// Claude 4 Sonnet (2025-05-14 version)
    ClaudeSonnet420250514,

    /// Claude 4 Sonnet (version 0)
    ClaudeSonnet40,

    /// Claude 4 Sonnet (2025-05-14 version)
    Claude4Sonnet20250514,

    /// Claude 3.5 Sonnet (latest version)
    Claude35SonnetLatest,

    /// Claude 3.5 Sonnet (2024-10-22 version)
    Claude35Sonnet20241022,

    /// Claude 3.5 Sonnet (2024-06-20 version)
    Claude35Sonnet20240620,

    /// Claude 4 Opus (version 0)
    ClaudeOpus40,

    /// Claude 4 Opus (2025-05-14 version)
    ClaudeOpus420250514,

    /// Claude 4 Opus (2025-05-14 version)
    Claude4Opus20250514,

    /// Claude 3 Opus (latest version)
    Claude3OpusLatest,

    /// Claude 3 Opus (2024-02-29 version)
    Claude3Opus20240229,

    /// Claude 3 Sonnet (2024-02-29 version)
    Claude3Sonnet20240229,

    /// Claude 3 Haiku (2024-03-07 version)
    Claude3Haiku20240307,

    /// Claude 2.1
    Claude21,

    /// Claude 2.0
    Claude20,
}

impl fmt::Display for Model {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Model::Known(known_model) => write!(f, "{}", known_model),
            Model::Custom(custom) => write!(f, "{}", custom),
        }
    }
}

impl fmt::Display for KnownModel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            KnownModel::Claude37SonnetLatest => write!(f, "claude-3-7-sonnet-latest"),
            KnownModel::Claude37Sonnet20250219 => write!(f, "claude-3-7-sonnet-20250219"),
            KnownModel::Claude35HaikuLatest => write!(f, "claude-3-5-haiku-latest"),
            KnownModel::Claude35Haiku20241022 => write!(f, "claude-3-5-haiku-20241022"),
            KnownModel::ClaudeSonnet420250514 => write!(f, "claude-sonnet-4-20250514"),
            KnownModel::ClaudeSonnet40 => write!(f, "claude-sonnet-4-0"),
            KnownModel::Claude4Sonnet20250514 => write!(f, "claude-4-sonnet-20250514"),
            KnownModel::Claude35SonnetLatest => write!(f, "claude-3-5-sonnet-latest"),
            KnownModel::Claude35Sonnet20241022 => write!(f, "claude-3-5-sonnet-20241022"),
            KnownModel::Claude35Sonnet20240620 => write!(f, "claude-3-5-sonnet-20240620"),
            KnownModel::ClaudeOpus40 => write!(f, "claude-opus-4-0"),
            KnownModel::ClaudeOpus420250514 => write!(f, "claude-opus-4-20250514"),
            KnownModel::Claude4Opus20250514 => write!(f, "claude-4-opus-20250514"),
            KnownModel::Claude3OpusLatest => write!(f, "claude-3-opus-latest"),
            KnownModel::Claude3Opus20240229 => write!(f, "claude-3-opus-20240229"),
            KnownModel::Claude3Sonnet20240229 => write!(f, "claude-3-sonnet-20240229"),
            KnownModel::Claude3Haiku20240307 => write!(f, "claude-3-haiku-20240307"),
            KnownModel::Claude21 => write!(f, "claude-2.1"),
            KnownModel::Claude20 => write!(f, "claude-2.0"),
        }
    }
}

impl Serialize for Model {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // Serialize the model as a string using Display implementation
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Model {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;

        // Check if it matches any known model
        match s.as_str() {
            "claude-3-7-sonnet-latest" => Ok(Model::Known(KnownModel::Claude37SonnetLatest)),
            "claude-3-7-sonnet-20250219" => Ok(Model::Known(KnownModel::Claude37Sonnet20250219)),
            "claude-3-5-haiku-latest" => Ok(Model::Known(KnownModel::Claude35HaikuLatest)),
            "claude-3-5-haiku-20241022" => Ok(Model::Known(KnownModel::Claude35Haiku20241022)),
            "claude-sonnet-4-20250514" => Ok(Model::Known(KnownModel::ClaudeSonnet420250514)),
            "claude-sonnet-4-0" => Ok(Model::Known(KnownModel::ClaudeSonnet40)),
            "claude-4-sonnet-20250514" => Ok(Model::Known(KnownModel::Claude4Sonnet20250514)),
            "claude-3-5-sonnet-latest" => Ok(Model::Known(KnownModel::Claude35SonnetLatest)),
            "claude-3-5-sonnet-20241022" => Ok(Model::Known(KnownModel::Claude35Sonnet20241022)),
            "claude-3-5-sonnet-20240620" => Ok(Model::Known(KnownModel::Claude35Sonnet20240620)),
            "claude-opus-4-0" => Ok(Model::Known(KnownModel::ClaudeOpus40)),
            "claude-opus-4-20250514" => Ok(Model::Known(KnownModel::ClaudeOpus420250514)),
            "claude-4-opus-20250514" => Ok(Model::Known(KnownModel::Claude4Opus20250514)),
            "claude-3-opus-latest" => Ok(Model::Known(KnownModel::Claude3OpusLatest)),
            "claude-3-opus-20240229" => Ok(Model::Known(KnownModel::Claude3Opus20240229)),
            "claude-3-sonnet-20240229" => Ok(Model::Known(KnownModel::Claude3Sonnet20240229)),
            "claude-3-haiku-20240307" => Ok(Model::Known(KnownModel::Claude3Haiku20240307)),
            "claude-2.1" => Ok(Model::Known(KnownModel::Claude21)),
            "claude-2.0" => Ok(Model::Known(KnownModel::Claude20)),
            // If it doesn't match any known model, treat it as a custom model
            _ => Ok(Model::Custom(s)),
        }
    }
}

impl From<KnownModel> for Model {
    fn from(model: KnownModel) -> Self {
        Model::Known(model)
    }
}


impl FromStr for KnownModel {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "claude-3-7-sonnet-latest" => Ok(KnownModel::Claude37SonnetLatest),
            "claude-3-7-sonnet-20250219" => Ok(KnownModel::Claude37Sonnet20250219),
            "claude-3-5-haiku-latest" => Ok(KnownModel::Claude35HaikuLatest),
            "claude-3-5-haiku-20241022" => Ok(KnownModel::Claude35Haiku20241022),
            "claude-sonnet-4-20250514" => Ok(KnownModel::ClaudeSonnet420250514),
            "claude-sonnet-4-0" => Ok(KnownModel::ClaudeSonnet40),
            "claude-4-sonnet-20250514" => Ok(KnownModel::Claude4Sonnet20250514),
            "claude-3-5-sonnet-latest" => Ok(KnownModel::Claude35SonnetLatest),
            "claude-3-5-sonnet-20241022" => Ok(KnownModel::Claude35Sonnet20241022),
            "claude-3-5-sonnet-20240620" => Ok(KnownModel::Claude35Sonnet20240620),
            "claude-opus-4-0" => Ok(KnownModel::ClaudeOpus40),
            "claude-opus-4-20250514" => Ok(KnownModel::ClaudeOpus420250514),
            "claude-4-opus-20250514" => Ok(KnownModel::Claude4Opus20250514),
            "claude-3-opus-latest" => Ok(KnownModel::Claude3OpusLatest),
            "claude-3-opus-20240229" => Ok(KnownModel::Claude3Opus20240229),
            "claude-3-sonnet-20240229" => Ok(KnownModel::Claude3Sonnet20240229),
            "claude-3-haiku-20240307" => Ok(KnownModel::Claude3Haiku20240307),
            "claude-2.1" => Ok(KnownModel::Claude21),
            "claude-2.0" => Ok(KnownModel::Claude20),
            _ => Err(()),
        }
    }
}

impl FromStr for Model {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match KnownModel::from_str(s) {
            Ok(known_model) => Ok(Model::Known(known_model)),
            Err(_) => Ok(Model::Custom(s.to_string())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_model_serialization() {
        let model = Model::Known(KnownModel::Claude37SonnetLatest);
        let json = serde_json::to_string(&model).unwrap();
        assert_eq!(json, r#""claude-3-7-sonnet-latest""#);

        let model = Model::Known(KnownModel::Claude35Sonnet20240620);
        let json = serde_json::to_string(&model).unwrap();
        assert_eq!(json, r#""claude-3-5-sonnet-20240620""#);
    }

    #[test]
    fn custom_model_serialization() {
        let model = Model::Custom("claude-4-custom".to_string());
        let json = serde_json::to_string(&model).unwrap();
        assert_eq!(json, r#""claude-4-custom""#);
    }

    #[test]
    fn model_deserialization() {
        let json = r#""claude-3-7-sonnet-latest""#;
        let model: Model = serde_json::from_str(json).unwrap();
        assert_eq!(model, Model::Known(KnownModel::Claude37SonnetLatest));

        let json = r#""claude-4-custom""#;
        let model: Model = serde_json::from_str(json).unwrap();
        assert_eq!(model, Model::Custom("claude-4-custom".to_string()));
    }

    #[test]
    fn claude_4_models() {
        // Test Claude 4 Sonnet models
        let model = Model::Known(KnownModel::ClaudeSonnet420250514);
        assert_eq!(model.to_string(), "claude-sonnet-4-20250514");
        let json = serde_json::to_string(&model).unwrap();
        assert_eq!(json, r#""claude-sonnet-4-20250514""#);

        let model = Model::Known(KnownModel::Claude4Sonnet20250514);
        assert_eq!(model.to_string(), "claude-4-sonnet-20250514");

        // Test Claude 4 Opus models
        let model = Model::Known(KnownModel::ClaudeOpus40);
        assert_eq!(model.to_string(), "claude-opus-4-0");

        let model = Model::Known(KnownModel::Claude4Opus20250514);
        assert_eq!(model.to_string(), "claude-4-opus-20250514");

        // Test deserialization of Claude 4 models
        let json = r#""claude-sonnet-4-20250514""#;
        let model: Model = serde_json::from_str(json).unwrap();
        assert_eq!(model, Model::Known(KnownModel::ClaudeSonnet420250514));

        let json = r#""claude-4-opus-20250514""#;
        let model: Model = serde_json::from_str(json).unwrap();
        assert_eq!(model, Model::Known(KnownModel::Claude4Opus20250514));
    }

    #[test]
    fn display() {
        let model = Model::Known(KnownModel::Claude37SonnetLatest);
        assert_eq!(model.to_string(), "claude-3-7-sonnet-latest");

        let model = Model::Custom("claude-4-custom".to_string());
        assert_eq!(model.to_string(), "claude-4-custom");
    }
}
