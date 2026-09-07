use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::types::{Model, OutputConfig, ThinkingConfig};

/// Inference speed for a message or fallback attempt.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InferenceSpeed {
    /// Standard inference speed and pricing.
    Standard,
    /// Fast inference mode, where supported.
    Fast,
}

/// Configuration for one named server-side fallback attempt.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FallbackConfig {
    /// Model to try for this attempt.
    pub model: Model,
    /// Output-token limit override.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    /// Thinking configuration override.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking: Option<ThinkingConfig>,
    /// Output configuration override.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_config: Option<OutputConfig>,
    /// Inference speed override.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed: Option<InferenceSpeed>,
}

impl FallbackConfig {
    /// Create a fallback attempt using the model's inherited request settings.
    pub fn new(model: impl Into<Model>) -> Self {
        Self {
            model: model.into(),
            max_tokens: None,
            thinking: None,
            output_config: None,
            speed: None,
        }
    }

    /// Override the output-token limit for this fallback.
    pub fn with_max_tokens(mut self, max_tokens: u32) -> Self {
        self.max_tokens = Some(max_tokens);
        self
    }

    /// Override thinking configuration for this fallback.
    pub fn with_thinking(mut self, thinking: ThinkingConfig) -> Self {
        self.thinking = Some(thinking);
        self
    }

    /// Override output configuration for this fallback.
    pub fn with_output_config(mut self, output_config: OutputConfig) -> Self {
        self.output_config = Some(output_config);
        self
    }

    /// Override inference speed for this fallback.
    pub fn with_speed(mut self, speed: InferenceSpeed) -> Self {
        self.speed = Some(speed);
        self
    }
}

impl<T: Into<Model>> From<T> for FallbackConfig {
    fn from(model: T) -> Self {
        Self::new(model)
    }
}

/// Server-side fallback routing configuration.
#[derive(Debug, Clone, PartialEq)]
pub enum Fallbacks {
    /// Let Anthropic select the recommended fallback for the refusal category.
    Default,
    /// Try the listed fallback configurations in order.
    Models(Vec<FallbackConfig>),
}

impl Serialize for Fallbacks {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Default => serializer.serialize_str("default"),
            Self::Models(models) => models.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for Fallbacks {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Wire {
            Default(String),
            Models(Vec<FallbackConfig>),
        }

        match Wire::deserialize(deserializer)? {
            Wire::Default(value) if value == "default" => Ok(Self::Default),
            Wire::Default(value) => Err(serde::de::Error::custom(format!(
                "unknown fallback mode: {value}"
            ))),
            Wire::Models(models) => Ok(Self::Models(models)),
        }
    }
}
