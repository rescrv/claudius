use serde::{Deserialize, Serialize};

/// Union type for different tool parameter types.
///
/// This type represents a union of different tool types that can be used with Claude, including:
/// - Custom tools
/// - Bash tools
/// - Text editor tools
/// - Web search tools
///
/// The API accepts any of these tool types when tools are provided to Claude.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum ToolUnionParam {
    // Note: These variants will be enabled as each tool type is implemented
    // TODO: Add CustomTool(ToolParam) when implemented
    // TODO: Add Bash20250124(ToolBash20250124Param) when implemented
    // TODO: Add TextEditor20250124(ToolTextEditor20250124Param) when implemented 
    // TODO: Add WebSearch20250305(WebSearchTool20250305Param) when implemented
}

// Note: Factory methods will be added as tool types are implemented

#[cfg(test)]
mod tests {
    use super::*;
    
    // Tests will be added as tool types are implemented
}