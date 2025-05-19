use serde::{Deserialize, Serialize};

use crate::types::{
    ToolChoiceAnyParam, ToolChoiceAutoParam, ToolChoiceNoneParam, ToolChoiceToolParam,
};

/// Parameter for configuring Claude's tool choice behavior.
///
/// This can be one of the following:
/// - "auto": Let the model decide if and when to use tools
/// - "any": Allow the model to use any available tool
/// - "tool": Force the model to use a specific named tool
/// - "none": Do not use any tools
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum ToolChoiceParam {
    /// Automatic tool choice
    Auto(ToolChoiceAutoParam),

    /// Any tool choice
    Any(ToolChoiceAnyParam),

    /// Specific tool choice
    Tool(ToolChoiceToolParam),

    /// No tools
    None(ToolChoiceNoneParam),
}

impl ToolChoiceParam {
    /// Create a new `ToolChoiceParam` with auto mode.
    pub fn auto() -> Self {
        Self::Auto(ToolChoiceAutoParam::new())
    }

    /// Create a new `ToolChoiceParam` with auto mode, specifying whether to disable parallel tool use.
    pub fn auto_with_disable_parallel(disable: bool) -> Self {
        Self::Auto(ToolChoiceAutoParam::new().with_disable_parallel_tool_use(disable))
    }

    /// Create a new `ToolChoiceParam` allowing any tool.
    pub fn any() -> Self {
        Self::Any(ToolChoiceAnyParam::new())
    }

    /// Create a new `ToolChoiceParam` allowing any tool, specifying whether to disable parallel tool use.
    pub fn any_with_disable_parallel(disable: bool) -> Self {
        Self::Any(ToolChoiceAnyParam::new().with_disable_parallel_tool_use(disable))
    }

    /// Create a new `ToolChoiceParam` with a specific named tool.
    pub fn tool(name: impl Into<String>) -> Self {
        Self::Tool(ToolChoiceToolParam::new(name))
    }

    /// Create a new `ToolChoiceParam` with a specific named tool, specifying whether to disable parallel tool use.
    pub fn tool_with_disable_parallel(name: impl Into<String>, disable: bool) -> Self {
        Self::Tool(ToolChoiceToolParam::new(name).with_disable_parallel_tool_use(disable))
    }

    /// Create a new `ToolChoiceParam` with no tools.
    pub fn none() -> Self {
        Self::None(ToolChoiceNoneParam::new())
    }
}

impl Default for ToolChoiceParam {
    fn default() -> Self {
        Self::auto()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, to_value};

    #[test]
    fn test_tool_choice_param_auto() {
        let param = ToolChoiceParam::auto();
        let json = to_value(&param).unwrap();

        assert_eq!(
            json,
            json!({
                "type": "auto"
            })
        );
    }

    #[test]
    fn test_tool_choice_param_any() {
        let param = ToolChoiceParam::any();
        let json = to_value(&param).unwrap();

        assert_eq!(
            json,
            json!({
                "type": "any"
            })
        );
    }

    #[test]
    fn test_tool_choice_param_tool() {
        let param = ToolChoiceParam::tool("my_tool");
        let json = to_value(&param).unwrap();

        assert_eq!(
            json,
            json!({
                "name": "my_tool",
                "type": "tool"
            })
        );
    }

    #[test]
    fn test_tool_choice_param_none() {
        let param = ToolChoiceParam::none();
        let json = to_value(&param).unwrap();

        assert_eq!(
            json,
            json!({
                "type": "none"
            })
        );
    }

    #[test]
    fn test_tool_choice_param_auto_with_disable_parallel() {
        let param = ToolChoiceParam::auto_with_disable_parallel(true);
        let json = to_value(&param).unwrap();

        assert_eq!(
            json,
            json!({
                "type": "auto",
                "disable_parallel_tool_use": true
            })
        );
    }

    #[test]
    fn test_tool_choice_param_deserialization_auto() {
        let json = json!({
            "type": "auto",
            "disable_parallel_tool_use": true
        });

        let param: ToolChoiceParam = serde_json::from_value(json).unwrap();
        match param {
            ToolChoiceParam::Auto(auto) => {
                assert_eq!(auto.r#type, "auto");
                assert_eq!(auto.disable_parallel_tool_use, Some(true));
            }
            _ => panic!("Expected Auto variant"),
        }
    }

    #[test]
    fn test_tool_choice_param_deserialization_tool() {
        let json = json!({
            "name": "my_tool",
            "type": "tool",
            "disable_parallel_tool_use": true
        });

        let param: ToolChoiceParam = serde_json::from_value(json).unwrap();
        match param {
            ToolChoiceParam::Tool(tool) => {
                assert_eq!(tool.r#type, "tool");
                assert_eq!(tool.name, "my_tool");
                assert_eq!(tool.disable_parallel_tool_use, Some(true));
            }
            _ => panic!("Expected Tool variant"),
        }
    }
}
