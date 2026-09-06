use std::any::Any;
use std::collections::{BTreeSet, HashSet};
use std::ops::ControlFlow;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use futures::StreamExt;
use utf8path::Path;

use crate::cache_control::apply_cache_controls;
use crate::observability::{
    AGENT_TOOL_CALLS, AGENT_TOOL_DURATION, AGENT_TOOL_ERRORS, AGENT_TURN_DURATION,
    AGENT_TURN_REQUESTS,
};
use crate::{
    AccumulatingStream, AgentStreamContext, Anthropic, CacheControlEphemeral, ContentBlock,
    ContentBlockDelta, Error, KnownModel, Message, MessageCreateParams, MessageParam,
    MessageParamContent, MessageRole, MessageStreamEvent, Metadata, Model, OutputConfig, Renderer,
    StopReason, StreamContext, SystemPrompt, ThinkingConfig, ToolBash20241022, ToolBash20250124,
    ToolChoice, ToolParam, ToolResultBlock, ToolResultBlockContent, ToolTextEditor20250124,
    ToolTextEditor20250429, ToolTextEditor20250728, ToolUnionParam, ToolUseBlock, Usage,
    WebSearchTool20250305, push_or_merge_message,
};

struct StreamingContext<'a> {
    renderer: &'a mut dyn Renderer,
    context: &'a AgentStreamContext,
    show_thinking: bool,
}

//////////////////////////////////////////// ToolResult ////////////////////////////////////////////

/// Result type for tool execution, using ControlFlow for early returns.
///
/// `Break` indicates execution should stop with an error, while `Continue`
/// contains the successful or error tool result blocks.
pub type ToolResult = ControlFlow<Error, Result<ToolResultBlock, ToolResultBlock>>;

////////////////////////////////////// IntermediateToolResult //////////////////////////////////////

/// Trait for intermediate tool results that can be passed between compute and apply phases.
///
/// This allows tools to compute results in one phase and apply them in another,
/// enabling better separation of concerns in tool execution.
pub trait IntermediateToolResult: Send {
    /// Returns the result as a type-erased Any reference for downcasting.
    fn as_any(&self) -> &dyn Any;
}

impl IntermediateToolResult for () {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl<T: Send + 'static> IntermediateToolResult for Option<T> {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl IntermediateToolResult for ToolResult {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

//////////////////////////////////////// ToolResultCallback ////////////////////////////////////////

/// Callback trait for implementing tool execution logic.
///
/// Separates tool execution into compute and apply phases, allowing for
/// read-only computation followed by state modification.
#[async_trait::async_trait]
pub trait ToolCallback<A: Agent>: Send + Sync {
    /// Computes the tool result without modifying agent state.
    async fn compute_tool_result(
        &self,
        client: &Anthropic,
        agent: &A,
        tool_use: &ToolUseBlock,
    ) -> Box<dyn IntermediateToolResult>;

    /// Computes the tool result with access to a renderer for streaming output.
    async fn compute_tool_result_streaming(
        &self,
        client: &Anthropic,
        agent: &A,
        tool_use: &ToolUseBlock,
        renderer: &mut dyn Renderer,
        context: &AgentStreamContext,
    ) -> Box<dyn IntermediateToolResult> {
        _ = renderer;
        _ = context;
        self.compute_tool_result(client, agent, tool_use).await
    }

    /// Applies the computed result, potentially modifying agent state.
    async fn apply_tool_result(
        &self,
        client: &Anthropic,
        agent: &mut A,
        tool_use: &ToolUseBlock,
        intermediate: Box<dyn IntermediateToolResult>,
    ) -> ToolResult;
}

/////////////////////////////////////////////// Tool ///////////////////////////////////////////////

/// Trait for tools that can be used by agents.
///
/// Tools provide functionality that agents can invoke during conversations,
/// such as file operations, web searches, or custom computations.
pub trait Tool<A: Agent>: Send + Sync {
    /// Returns the name of the tool.
    fn name(&self) -> String;
    /// Returns the callback implementation for this tool.
    fn callback(&self) -> Box<dyn ToolCallback<A> + '_>;
    /// Converts the tool to a parameter format for the API.
    fn to_param(&self) -> ToolUnionParam;
}

struct ToolNotFound(String);

impl<A: Agent> Tool<A> for ToolNotFound {
    fn name(&self) -> String {
        self.0.clone()
    }

    fn callback(&self) -> Box<dyn ToolCallback<A> + '_> {
        Box::new(ToolNotFoundCallback(self.0.clone()))
    }

    fn to_param(&self) -> ToolUnionParam {
        unimplemented!();
    }
}

struct ToolNotFoundCallback(String);

#[async_trait::async_trait]
impl<A: Agent> ToolCallback<A> for ToolNotFoundCallback {
    async fn compute_tool_result(
        &self,
        _client: &Anthropic,
        _agent: &A,
        _tool_use: &ToolUseBlock,
    ) -> Box<dyn IntermediateToolResult> {
        Box::new(())
    }

    async fn apply_tool_result(
        &self,
        _client: &Anthropic,
        _agent: &mut A,
        tool_use: &ToolUseBlock,
        _intermediate: Box<dyn IntermediateToolResult>,
    ) -> ToolResult {
        ControlFlow::Continue(Err(ToolResultBlock {
            tool_use_id: tool_use.id.clone(),
            content: Some(ToolResultBlockContent::String(format!(
                "{} not found",
                self.0
            ))),
            is_error: Some(true),
            cache_control: None,
        }))
    }
}

impl<A: Agent> Tool<A> for ToolBash20241022 {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn callback(&self) -> Box<dyn ToolCallback<A> + '_> {
        Box::new(BashCallback)
    }

    fn to_param(&self) -> ToolUnionParam {
        ToolUnionParam::Bash20241022(self.clone())
    }
}

impl<A: Agent> Tool<A> for ToolBash20250124 {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn callback(&self) -> Box<dyn ToolCallback<A> + '_> {
        Box::new(BashCallback)
    }

    fn to_param(&self) -> ToolUnionParam {
        ToolUnionParam::Bash20250124(self.clone())
    }
}

struct BashCallback;

#[async_trait::async_trait]
impl<A: Agent> ToolCallback<A> for BashCallback {
    async fn compute_tool_result(
        &self,
        _client: &Anthropic,
        agent: &A,
        tool_use: &ToolUseBlock,
    ) -> Box<dyn IntermediateToolResult> {
        #[derive(serde::Deserialize)]
        struct BashTool {
            command: String,
            #[serde(default)]
            restart: bool,
        }
        let bash: BashTool = match serde_json::from_value(tool_use.input.clone()) {
            Ok(input) => input,
            Err(err) => {
                return Box::new(ControlFlow::Continue(Err(ToolResultBlock {
                    tool_use_id: tool_use.id.clone(),
                    content: Some(ToolResultBlockContent::String(err.to_string())),
                    is_error: Some(true),
                    cache_control: None,
                })));
            }
        };
        match agent.bash(&bash.command, bash.restart).await {
            Ok(answer) => Box::new(ControlFlow::Continue(Ok(ToolResultBlock {
                tool_use_id: tool_use.id.clone(),
                content: Some(ToolResultBlockContent::String(answer.to_string())),
                is_error: None,
                cache_control: None,
            }))),
            Err(err) => Box::new(ControlFlow::Continue(Err(ToolResultBlock {
                tool_use_id: tool_use.id.clone(),
                content: Some(ToolResultBlockContent::String(err.to_string())),
                is_error: Some(true),
                cache_control: None,
            }))),
        }
    }

    async fn apply_tool_result(
        &self,
        _client: &Anthropic,
        _agent: &mut A,
        _tool_use: &ToolUseBlock,
        intermediate: Box<dyn IntermediateToolResult>,
    ) -> ToolResult {
        let Some(intermediate) = intermediate.as_any().downcast_ref::<ToolResult>() else {
            return ControlFlow::Break(Error::unknown(
                "intermediate tool result fails to deserialize",
            ));
        };
        intermediate.clone()
    }
}

struct TextEditorCallback;

#[async_trait::async_trait]
impl<A: Agent> ToolCallback<A> for TextEditorCallback {
    async fn compute_tool_result(
        &self,
        _client: &Anthropic,
        agent: &A,
        tool_use: &ToolUseBlock,
    ) -> Box<dyn IntermediateToolResult> {
        match agent.text_editor(tool_use.clone()).await {
            Ok(result) => Box::new(ControlFlow::Continue(Ok(ToolResultBlock {
                tool_use_id: tool_use.id.clone(),
                content: Some(ToolResultBlockContent::String(result)),
                is_error: None,
                cache_control: None,
            }))),
            Err(err) => Box::new(ControlFlow::Continue(Err(ToolResultBlock {
                tool_use_id: tool_use.id.clone(),
                content: Some(ToolResultBlockContent::String(err.to_string())),
                is_error: Some(true),
                cache_control: None,
            }))),
        }
    }

    async fn apply_tool_result(
        &self,
        _client: &Anthropic,
        _agent: &mut A,
        _tool_use: &ToolUseBlock,
        intermediate: Box<dyn IntermediateToolResult>,
    ) -> ToolResult {
        let Some(intermediate) = intermediate.as_any().downcast_ref::<ToolResult>() else {
            return ControlFlow::Break(Error::unknown(
                "intermediate tool result fails to deserialize",
            ));
        };
        intermediate.clone()
    }
}

struct WebSearchCallback;

#[async_trait::async_trait]
impl<A: Agent> ToolCallback<A> for WebSearchCallback {
    async fn compute_tool_result(
        &self,
        _client: &Anthropic,
        _agent: &A,
        tool_use: &ToolUseBlock,
    ) -> Box<dyn IntermediateToolResult> {
        Box::new(ControlFlow::Continue(Err(ToolResultBlock {
            tool_use_id: tool_use.id.clone(),
            content: Some(ToolResultBlockContent::String(
                "Web search is not implemented".to_string(),
            )),
            is_error: Some(true),
            cache_control: None,
        })))
    }

    async fn apply_tool_result(
        &self,
        _client: &Anthropic,
        _agent: &mut A,
        _tool_use: &ToolUseBlock,
        intermediate: Box<dyn IntermediateToolResult>,
    ) -> ToolResult {
        let Some(intermediate) = intermediate.as_any().downcast_ref::<ToolResult>() else {
            return ControlFlow::Break(Error::unknown(
                "intermediate tool result fails to deserialize",
            ));
        };
        intermediate.clone()
    }
}

struct SearchFilesystemCallback;

#[async_trait::async_trait]
impl<A: Agent> ToolCallback<A> for SearchFilesystemCallback {
    async fn compute_tool_result(
        &self,
        _client: &Anthropic,
        agent: &A,
        tool_use: &ToolUseBlock,
    ) -> Box<dyn IntermediateToolResult> {
        #[derive(serde::Deserialize)]
        struct SearchTool {
            query: String,
        }
        let search: SearchTool = match serde_json::from_value(tool_use.input.clone()) {
            Ok(input) => input,
            Err(err) => {
                return Box::new(ControlFlow::Continue(Err(ToolResultBlock {
                    tool_use_id: tool_use.id.clone(),
                    content: Some(ToolResultBlockContent::String(err.to_string())),
                    is_error: Some(true),
                    cache_control: None,
                })));
            }
        };
        match agent.search(&search.query).await {
            Ok(answer) => Box::new(ControlFlow::Continue(Ok(ToolResultBlock {
                tool_use_id: tool_use.id.clone(),
                content: Some(ToolResultBlockContent::String(answer.to_string())),
                is_error: None,
                cache_control: None,
            }))),
            Err(err) => Box::new(ControlFlow::Continue(Err(ToolResultBlock {
                tool_use_id: tool_use.id.clone(),
                content: Some(ToolResultBlockContent::String(err.to_string())),
                is_error: Some(true),
                cache_control: None,
            }))),
        }
    }

    async fn apply_tool_result(
        &self,
        _client: &Anthropic,
        _agent: &mut A,
        _tool_use: &ToolUseBlock,
        intermediate: Box<dyn IntermediateToolResult>,
    ) -> ToolResult {
        let Some(intermediate) = intermediate.as_any().downcast_ref::<ToolResult>() else {
            return ControlFlow::Break(Error::unknown(
                "intermediate tool result fails to deserialize",
            ));
        };
        intermediate.clone()
    }
}

impl<A: Agent> Tool<A> for ToolTextEditor20250124 {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn callback(&self) -> Box<dyn ToolCallback<A>> {
        Box::new(TextEditorCallback)
    }

    fn to_param(&self) -> ToolUnionParam {
        ToolUnionParam::TextEditor20250124(self.clone())
    }
}

impl<A: Agent> Tool<A> for ToolTextEditor20250429 {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn callback(&self) -> Box<dyn ToolCallback<A>> {
        Box::new(TextEditorCallback)
    }

    fn to_param(&self) -> ToolUnionParam {
        ToolUnionParam::TextEditor20250429(self.clone())
    }
}

impl<A: Agent> Tool<A> for ToolTextEditor20250728 {
    /// Return the name of the text editor tool.
    fn name(&self) -> String {
        self.name.clone()
    }

    /// Return a callback that handles text editor operations.
    ///
    /// This uses the same [`TextEditorCallback`] as other text editor tool versions,
    /// providing consistent functionality across tool versions.
    fn callback(&self) -> Box<dyn ToolCallback<A>> {
        Box::new(TextEditorCallback)
    }

    /// Convert this tool to the union parameter type for API serialization.
    fn to_param(&self) -> ToolUnionParam {
        ToolUnionParam::TextEditor20250728(self.clone())
    }
}

impl<A: Agent> Tool<A> for WebSearchTool20250305 {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn callback(&self) -> Box<dyn ToolCallback<A>> {
        Box::new(WebSearchCallback)
    }

    fn to_param(&self) -> ToolUnionParam {
        ToolUnionParam::WebSearch20250305(self.clone())
    }
}

/// Tool for searching the local filesystem.
///
/// Provides filesystem search functionality to agents, allowing them
/// to find files and directories based on search queries.
pub struct ToolSearchFileSystem;

impl<A: Agent> Tool<A> for ToolSearchFileSystem {
    fn name(&self) -> String {
        "search_filesystem".to_string()
    }

    fn callback(&self) -> Box<dyn ToolCallback<A>> {
        Box::new(SearchFilesystemCallback)
    }

    fn to_param(&self) -> ToolUnionParam {
        let name = <Self as Tool<A>>::name(self).to_string();
        let input_schema = serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "The search query to find on the filesystem."
                }
            },
            "required": ["query"]
        });
        let description = Some("Search the local filesystem.".to_string());
        let cache_control = None;
        ToolUnionParam::CustomTool(ToolParam {
            input_schema,
            name,
            description,
            cache_control,
            strict: None,
        })
    }
}

////////////////////////////////////////////// Budget //////////////////////////////////////////////

/// # Budget Management System
///
/// This module provides a thread-safe monetary budget management system designed for controlling
/// Anthropic API usage costs. The system operates on precise micro-cent accounting to handle
/// fractional pricing models accurately while avoiding floating-point arithmetic issues.
///
/// ## Core Concepts
///
/// ### Micro-cents Monetary System
///
/// All monetary values are represented in **micro-cents** - one-millionth of a cent (1/1,000,000 of a cent).
/// This allows precise tracking of API costs that may be fractions of a cent:
///
/// - 1 dollar = 100,000,000 micro-cents
/// - 1 cent = 1,000,000 micro-cents
/// - 0.001 cents = 1,000 micro-cents
///
/// This precision is essential for accurately tracking modern API pricing models where individual
/// tokens may cost fractions of a cent.
///
/// ### Allocation vs Consumption Model
///
/// The budget system uses a two-phase approach:
///
/// 1. **Allocation**: Reserve budget for the maximum expected cost before making an API call
/// 2. **Consumption**: Deduct the actual cost after receiving the API response
///
/// This prevents race conditions and ensures budget limits are never exceeded, even in concurrent
/// scenarios where multiple API calls might be in flight simultaneously.
///
/// ### Thread Safety
///
/// The budget system is designed for concurrent use:
///
/// - [`Budget`] uses atomic operations for lock-free budget tracking
/// - Multiple allocations can be created concurrently from the same budget
/// - Unused allocated budget is automatically returned when [`BudgetAllocation`] is dropped
/// - All operations are atomic and consistent across threads
///
/// ## Example Usage
///
/// ```rust
/// use std::sync::Arc;
/// use claudius::{Budget, Usage};
///
/// // Create a $5.00 budget with realistic Anthropic API rates
/// let budget = Arc::new(Budget::from_dollars_with_rates(
///     5.0,   // $5.00 total budget
///     300,   // ~$0.0003 per input token
///     1500,  // ~$0.0015 per output token
///     150,   // ~$0.00015 per cache creation token
///     75,    // ~$0.000075 per cache read token
/// ));
///
/// // Allocate budget for an API call expecting up to 1000 tokens
/// if let Some(mut allocation) = budget.allocate(1000) {
///     println!("Allocated budget for up to {} tokens", allocation.remaining_tokens());
///
///     // After making the API call, consume the actual usage
///     let actual_usage = Usage::new(150, 75); // 150 input, 75 output tokens
///     if allocation.consume_usage(&actual_usage) {
///         println!("Successfully consumed budget for actual usage");
///     }
///     // Unused budget is automatically returned when allocation is dropped
/// } else {
///     println!("Insufficient budget for this operation");
/// }
///
/// println!("Remaining budget: ${:.6}",
///          budget.remaining_micro_cents() as f64 / 100_000_000.0);
/// ```
///
/// ## Concurrent Usage Example
///
/// ```rust
/// use std::sync::Arc;
/// use std::thread;
/// use claudius::Budget;
///
/// let budget = Arc::new(Budget::from_dollars_flat_rate(1.0, 100));
/// let mut handles = vec![];
///
/// // Spawn multiple threads that try to allocate budget concurrently
/// for i in 0..10 {
///     let budget_clone = Arc::clone(&budget);
///     handles.push(thread::spawn(move || {
///         if let Some(_allocation) = budget_clone.allocate(50) {
///             println!("Thread {} successfully allocated budget", i);
///             // allocation automatically returns unused budget when dropped
///         } else {
///             println!("Thread {} failed to allocate budget", i);
///         }
///     }));
/// }
///
/// for handle in handles {
///     handle.join().unwrap();
/// }
/// ```
///
/// ## Error Handling and Recovery Example
///
/// ```rust
/// use claudius::{Budget, Usage};
///
/// let budget = Budget::from_dollars_with_rates(2.0, 300, 1500, 150, 75);
///
/// // Function to handle API operations with proper error handling
/// fn make_api_call(
///     budget: &Budget,
///     expected_tokens: u32,
/// ) -> Result<(), &'static str> {
///     // Try to allocate budget
///     let mut allocation = budget.allocate(expected_tokens)
///         .ok_or("Insufficient budget for operation")?;
///
///     // Simulate API call - in reality, you'd make the actual API request here
///     let prompt_tokens = i32::try_from(expected_tokens / 2)
///         .map_err(|_| "Token count too large for i32")?;
///     let completion_tokens = i32::try_from(expected_tokens / 4)
///         .map_err(|_| "Token count too large for i32")?;
///     let actual_usage = Usage::new(prompt_tokens, completion_tokens);
///
///     // Consume actual usage
///     if allocation.consume_usage(&actual_usage) {
///         println!("API call completed successfully");
///         Ok(())
///     } else {
///         // This shouldn't happen if allocation was calculated correctly,
///         // but defensive programming is good practice
///         Err("Usage exceeded allocation - this indicates a bug")
///     }
/// }
///
/// // Multiple API calls with error handling
/// for i in 1..=5 {
///     match make_api_call(&budget, 100) {
///         Ok(()) => println!("API call {} succeeded", i),
///         Err(e) => {
///             println!("API call {} failed: {}", i, e);
///             break; // Stop making calls if budget is exhausted
///         }
///     }
/// }
///
/// println!("Final budget: ${:.6}",
///          budget.remaining_micro_cents() as f64 / 100_000_000.0);
/// ```
///
/// ## Real-world Agent Example
///
/// ```rust
/// use std::sync::Arc;
/// use claudius::{Budget, Usage};
///
/// // Simulate an AI agent that processes multiple tasks
/// struct AIAgent {
///     budget: Arc<Budget>,
///     name: String,
/// }
///
/// impl AIAgent {
///     fn new(name: String, daily_budget_dollars: f64) -> Self {
///         // Create budget with realistic Anthropic API rates
///         let budget = Arc::new(Budget::from_dollars_with_rates(
///             daily_budget_dollars,
///             300,  // ~$0.0003 per input token
///             1500, // ~$0.0015 per output token
///             375,  // ~$0.000375 per cache creation token
///             30,   // ~$0.00003 per cache read token
///         ));
///
///         Self { budget, name }
///     }
///
///     fn process_task(&self, task_complexity: u32) -> Result<String, String> {
///         // Estimate tokens based on task complexity
///         let estimated_tokens = task_complexity * 10;
///
///         let mut allocation = self.budget.allocate(estimated_tokens)
///             .ok_or_else(|| format!(
///                 "Agent {} insufficient budget for task (need {} tokens)",
///                 self.name, estimated_tokens
///             ))?;
///
///         // Simulate API call with actual usage
///         let input_tokens = (estimated_tokens * 6) / 10;
///         let output_tokens = (estimated_tokens * 3) / 10;
///         let cache_read_tokens = estimated_tokens / 10;
///
///         let input_i32 = i32::try_from(input_tokens)
///             .map_err(|_| "Input token count too large for i32".to_string())?;
///         let output_i32 = i32::try_from(output_tokens)
///             .map_err(|_| "Output token count too large for i32".to_string())?;
///         let cache_i32 = i32::try_from(cache_read_tokens)
///             .map_err(|_| "Cache read token count too large for i32".to_string())?;
///
///         let usage = Usage::new(input_i32, output_i32)
///             .with_cache_read_input_tokens(cache_i32);
///
///         if allocation.consume_usage(&usage) {
///             Ok(format!("Task completed by {} using {} total tokens",
///                        self.name, input_tokens + output_tokens + cache_read_tokens))
///         } else {
///             Err("Usage calculation error".to_string())
///         }
///     }
///
///     fn remaining_budget_dollars(&self) -> f64 {
///         self.budget.remaining_micro_cents() as f64 / 100_000_000.0
///     }
/// }
///
/// // Usage example
/// let agent = AIAgent::new("DataAnalyzer".to_string(), 50.0); // $50 daily budget
///
/// let tasks = vec![5, 10, 15, 8, 12]; // Task complexity scores
/// for (i, &complexity) in tasks.iter().enumerate() {
///     match agent.process_task(complexity) {
///         Ok(result) => {
///             println!("Task {}: {}", i + 1, result);
///             println!("  Remaining budget: ${:.2}", agent.remaining_budget_dollars());
///         }
///         Err(error) => {
///             println!("Task {}: Failed - {}", i + 1, error);
///             break;
///         }
///     }
/// }
/// ```
/// Monetary budget manager for controlling API usage costs.
///
/// The `Budget` struct provides thread-safe, atomic budget allocation and tracking
/// for Anthropic API operations. It uses a micro-cent precision monetary system
/// to accurately track costs without floating-point arithmetic issues.
///
/// # Micro-cents Precision
///
/// All monetary amounts are stored as micro-cents (1/1,000,000 of a cent) to provide
/// precise cost tracking for API operations where individual tokens may cost fractions
/// of a cent. This eliminates floating-point rounding errors that could accumulate
/// over many API calls.
///
/// # Token Rate Model
///
/// The budget supports different rates for different types of tokens:
/// - Input tokens: The base cost for processing input text
/// - Output tokens: The cost for generating response text
/// - Cache creation tokens: The cost for creating prompt caches
/// - Cache read tokens: The reduced cost for reading from prompt caches
///
/// # Thread Safety
///
/// `Budget` is designed for concurrent access across multiple threads or async tasks.
/// All budget operations use atomic operations to ensure consistency without locks.
/// Multiple [`BudgetAllocation`]s can be created concurrently, and unused budget
/// is automatically returned when allocations are dropped.
///
/// # Example
///
/// ```rust
/// use claudius::{Budget, Usage};
///
/// // Create a budget with $10 and realistic token rates
/// let budget = Budget::from_dollars_with_rates(
///     10.0, // $10 budget
///     300,  // 300 micro-cents per input token
///     1500, // 1500 micro-cents per output token
///     150,  // 150 micro-cents per cache creation token
///     75,   // 75 micro-cents per cache read token
/// );
///
/// // Allocate budget for an operation expecting up to 500 tokens
/// if let Some(mut allocation) = budget.allocate(500) {
///     // Simulate API usage
///     let usage = Usage::new(100, 50); // 100 input, 50 output tokens
///
///     if allocation.consume_usage(&usage) {
///         println!("Operation completed within budget");
///     }
/// }
/// ```
#[derive(Debug)]
pub struct Budget {
    remaining_micro_cents: Arc<AtomicU64>,
    total_micro_cents: u64,
    input_token_rate_micro_cents: u64,
    output_token_rate_micro_cents: u64,
    cache_creation_token_rate_micro_cents: u64,
    cache_read_token_rate_micro_cents: u64,
}

/// Token categories used for cost accounting.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum TokenKind {
    /// Tokens counted as input tokens.
    Input,
    /// Tokens counted as output tokens.
    Output,
    /// Tokens counted as cache creation tokens.
    CacheCreation,
    /// Tokens counted as cache read tokens.
    CacheRead,
}

impl Budget {
    /// Conversion factor from dollars to micro-cents.
    const MICRO_CENTS_PER_DOLLAR: f64 = 100_000_000.0;

    /// Default rate for deprecated methods (micro-cents per token).
    const DEFAULT_RATE_MICRO_CENTS_PER_TOKEN: u64 = 1000;

    /// Creates a new budget with the specified monetary amount in micro-cents and token rates.
    ///
    /// # Arguments
    /// * `budget_micro_cents` - Total budget in micro-cents (1/1,000,000 of a cent)
    /// * `input_token_rate_micro_cents` - Cost per input token in micro-cents
    /// * `output_token_rate_micro_cents` - Cost per output token in micro-cents
    /// * `cache_creation_token_rate_micro_cents` - Cost per cache creation token in micro-cents
    /// * `cache_read_token_rate_micro_cents` - Cost per cache read token in micro-cents
    pub fn new_with_rates(
        budget_micro_cents: u64,
        input_token_rate_micro_cents: u64,
        output_token_rate_micro_cents: u64,
        cache_creation_token_rate_micro_cents: u64,
        cache_read_token_rate_micro_cents: u64,
    ) -> Self {
        let remaining_micro_cents = Arc::new(AtomicU64::new(budget_micro_cents));
        Self {
            remaining_micro_cents,
            total_micro_cents: budget_micro_cents,
            input_token_rate_micro_cents,
            output_token_rate_micro_cents,
            cache_creation_token_rate_micro_cents,
            cache_read_token_rate_micro_cents,
        }
    }

    /// Creates a new budget with a simplified flat rate per token.
    ///
    /// # Arguments
    /// * `budget_micro_cents` - Total budget in micro-cents
    /// * `token_rate_micro_cents` - Cost per token (applies to all token types)
    pub fn new_flat_rate(budget_micro_cents: u64, token_rate_micro_cents: u64) -> Self {
        Self::new_with_rates(
            budget_micro_cents,
            token_rate_micro_cents,
            token_rate_micro_cents,
            token_rate_micro_cents,
            token_rate_micro_cents,
        )
    }

    /// Creates a budget from dollars with specified rates per token in micro-cents.
    pub fn from_dollars_with_rates(
        budget_dollars: f64,
        input_token_rate_micro_cents: u64,
        output_token_rate_micro_cents: u64,
        cache_creation_token_rate_micro_cents: u64,
        cache_read_token_rate_micro_cents: u64,
    ) -> Self {
        let result = budget_dollars * Self::MICRO_CENTS_PER_DOLLAR;
        let budget_micro_cents = if result.is_finite() && result >= 0.0 {
            result as u64
        } else {
            u64::MAX
        };
        Self::new_with_rates(
            budget_micro_cents,
            input_token_rate_micro_cents,
            output_token_rate_micro_cents,
            cache_creation_token_rate_micro_cents,
            cache_read_token_rate_micro_cents,
        )
    }

    /// Creates a budget from dollars with a flat rate per token.
    ///
    /// # Example
    /// ```rust
    /// # use claudius::Budget;
    /// // Create a $10 budget where each token costs 500 micro-cents
    /// let budget = Budget::from_dollars_flat_rate(10.0, 500);
    ///
    /// // This budget can handle up to 20,000,000 tokens (10 * 100,000,000 / 500)
    /// assert!(budget.allocate(1000).is_some());
    /// ```
    pub fn from_dollars_flat_rate(budget_dollars: f64, token_rate_micro_cents: u64) -> Self {
        let result = budget_dollars * Self::MICRO_CENTS_PER_DOLLAR;
        let budget_micro_cents = if result.is_finite() && result >= 0.0 {
            result as u64
        } else {
            u64::MAX
        };
        Self::new_flat_rate(budget_micro_cents, token_rate_micro_cents)
    }

    /// Creates a budget from a dollar amount using the published token rates
    /// for a known Anthropic model.
    ///
    /// This is the recommended way to create a budget when you know which model
    /// you will be calling, because it automatically uses the correct per-token
    /// rates for input, output, cache-creation, and cache-read tokens.
    ///
    /// # Arguments
    ///
    /// * `budget_dollars` - Total budget in US dollars
    /// * `model` - The Anthropic model whose published rates should be used
    ///
    /// # Example
    ///
    /// ```rust
    /// use claudius::{Budget, KnownModel};
    ///
    /// // Create a $5.00 budget priced for Claude Sonnet 4.5
    /// let budget = Budget::from_dollars_with_model(5.0, KnownModel::ClaudeSonnet45);
    ///
    /// assert_eq!(budget.total_micro_cents(), 500_000_000); // $5.00
    /// ```
    pub fn from_dollars_with_model(budget_dollars: f64, model: crate::KnownModel) -> Self {
        let rates = model.token_rates();
        Self::from_dollars_with_rates(
            budget_dollars,
            rates.input,
            rates.output,
            rates.cache_creation,
            rates.cache_read,
        )
    }

    /// Creates a budget with the given micro-cent amount using the published token rates
    /// for a known Anthropic model.
    ///
    /// # Arguments
    ///
    /// * `budget_micro_cents` - Total budget in micro-cents (1/1,000,000 of a cent)
    /// * `model` - The Anthropic model whose published rates should be used
    ///
    /// # Example
    ///
    /// ```rust
    /// use claudius::{Budget, KnownModel};
    ///
    /// // Create a 500M micro-cent ($5.00) budget priced for Claude Opus 4.5
    /// let budget = Budget::from_model(500_000_000, KnownModel::ClaudeOpus45);
    ///
    /// assert_eq!(budget.total_micro_cents(), 500_000_000);
    /// ```
    pub fn from_model(budget_micro_cents: u64, model: crate::KnownModel) -> Self {
        let rates = model.token_rates();
        Self::new_with_rates(
            budget_micro_cents,
            rates.input,
            rates.output,
            rates.cache_creation,
            rates.cache_read,
        )
    }

    /// Legacy constructor for backward compatibility - creates a token-based budget.
    /// This converts tokens to micro-cents using a default rate.
    #[deprecated(note = "Use new_with_rates or new_flat_rate instead")]
    pub fn new(tokens: u32) -> Self {
        let budget_micro_cents =
            (tokens as u64).saturating_mul(Self::DEFAULT_RATE_MICRO_CENTS_PER_TOKEN);
        Self::new_flat_rate(budget_micro_cents, Self::DEFAULT_RATE_MICRO_CENTS_PER_TOKEN)
    }

    /// Calculates the total cost in micro-cents for a specific token usage pattern.
    ///
    /// This method computes the precise cost by applying the budget's token rates
    /// to each type of token usage. The calculation includes:
    /// - Input tokens × input token rate
    /// - Output tokens × output token rate
    /// - Cache creation tokens × cache creation rate
    /// - Cache read tokens × cache read rate
    ///
    /// # Arguments
    ///
    /// * `usage` - The token usage to calculate costs for
    ///
    /// # Returns
    ///
    /// Total cost in micro-cents as a `u64`
    ///
    /// # Example
    ///
    /// ```rust
    /// use claudius::{Budget, Usage};
    ///
    /// let budget = Budget::new_with_rates(
    ///     1_000_000, // 1M micro-cents budget
    ///     300,       // 300 micro-cents per input token
    ///     1500,      // 1500 micro-cents per output token
    ///     375,       // 375 micro-cents per cache creation token
    ///     30,        // 30 micro-cents per cache read token
    /// );
    ///
    /// let usage = Usage::new(100, 50) // 100 input, 50 output tokens
    ///     .with_cache_creation_input_tokens(20)
    ///     .with_cache_read_input_tokens(10);
    ///
    /// let cost = budget.calculate_cost(&usage);
    /// // Cost = (100 × 300) + (50 × 1500) + (20 × 375) + (10 × 30)
    /// //      = 30,000 + 75,000 + 7,500 + 300 = 112,800 micro-cents
    /// assert_eq!(cost, 112_800);
    /// ```
    ///
    /// # Overflow Safety
    ///
    /// This method performs arithmetic that could theoretically overflow with extreme
    /// values, but overflow would require unrealistic combinations such as billions
    /// of tokens with extremely high rates. All practical API usage scenarios are
    /// well within safe bounds.
    pub fn calculate_cost(&self, usage: &crate::Usage) -> u64 {
        let input_cost =
            (usage.input_tokens.max(0) as u64).saturating_mul(self.input_token_rate_micro_cents);
        let output_cost =
            (usage.output_tokens.max(0) as u64).saturating_mul(self.output_token_rate_micro_cents);
        let cache_creation_cost = (usage.cache_creation_input_tokens.unwrap_or(0).max(0) as u64)
            .saturating_mul(self.cache_creation_token_rate_micro_cents);
        let cache_read_cost = (usage.cache_read_input_tokens.unwrap_or(0).max(0) as u64)
            .saturating_mul(self.cache_read_token_rate_micro_cents);

        input_cost
            .checked_add(output_cost)
            .and_then(|sum| sum.checked_add(cache_creation_cost))
            .and_then(|sum| sum.checked_add(cache_read_cost))
            .unwrap_or(u64::MAX)
    }

    /// Calculates the cost in micro-cents for only input tokens in a usage record.
    ///
    /// This includes input tokens, cache creation tokens, and cache read tokens.
    /// Output tokens are excluded.
    ///
    /// # Returns
    ///
    /// Input cost in micro-cents as a `u64`
    pub fn calculate_input_cost(&self, usage: &crate::Usage) -> u64 {
        let input_cost =
            (usage.input_tokens.max(0) as u64).saturating_mul(self.input_token_rate_micro_cents);
        let cache_creation_cost = (usage.cache_creation_input_tokens.unwrap_or(0).max(0) as u64)
            .saturating_mul(self.cache_creation_token_rate_micro_cents);
        let cache_read_cost = (usage.cache_read_input_tokens.unwrap_or(0).max(0) as u64)
            .saturating_mul(self.cache_read_token_rate_micro_cents);

        input_cost
            .checked_add(cache_creation_cost)
            .and_then(|sum| sum.checked_add(cache_read_cost))
            .unwrap_or(u64::MAX)
    }

    /// Calculates the cost in micro-cents for only output tokens in a usage record.
    ///
    /// # Returns
    ///
    /// Output cost in micro-cents as a `u64`
    pub fn calculate_output_cost(&self, usage: &crate::Usage) -> u64 {
        (usage.output_tokens.max(0) as u64).saturating_mul(self.output_token_rate_micro_cents)
    }

    /// Attempts to allocate cost for the expected maximum tokens from the budget.
    ///
    /// Returns `Some(BudgetAllocation)` if sufficient budget is available,
    /// or `None` if the budget is insufficient.
    ///
    /// # Example
    /// ```rust
    /// # use claudius::Budget;
    /// let budget = Budget::from_dollars_flat_rate(1.0, 100);  // $1 budget, 100 micro-cents per token
    ///
    /// if let Some(allocation) = budget.allocate(50) {
    ///     println!("Successfully allocated budget for up to 50 tokens");
    ///     // Use allocation for API call...
    /// } else {
    ///     println!("Insufficient budget for 50 tokens");
    /// }
    /// ```
    pub fn allocate(&self, max_tokens: u32) -> Option<BudgetAllocation<'_>> {
        let max_cost = self.calculate_output_cost_for_tokens(max_tokens);
        self.allocate_micro_cents(max_cost)
    }

    /// Attempts to allocate as many output tokens as the budget can currently fund.
    ///
    /// Unlike [`allocate`], this method does not fail just because the full
    /// `max_tokens` amount is unavailable. It reserves the smaller of
    /// `max_tokens` and the remaining output-token capacity. This is useful when
    /// a request should be capped by remaining spend instead of rejected outright.
    ///
    /// Returns `None` when no output token can be funded. A zero-token request
    /// preserves [`allocate`]'s behavior and returns an empty allocation.
    ///
    /// [`allocate`]: Budget::allocate
    pub fn allocate_available(&self, max_tokens: u32) -> Option<BudgetAllocation<'_>> {
        if max_tokens == 0 {
            return self.allocate(0);
        }

        let rate = self.output_token_rate_micro_cents;
        if rate == 0 {
            return self.allocate(max_tokens);
        }

        let max_cost = self.calculate_output_cost_for_tokens(max_tokens);
        loop {
            let witness = self.remaining_micro_cents.load(Ordering::Relaxed);
            let cost = std::cmp::min(witness, max_cost);
            let funded_tokens = cost / rate;
            if funded_tokens == 0 {
                return None;
            }
            let capped_cost = funded_tokens.saturating_mul(rate);
            if self
                .remaining_micro_cents
                .compare_exchange(
                    witness,
                    witness.saturating_sub(capped_cost),
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                )
                .is_ok()
            {
                let remaining_micro_cents = Arc::clone(&self.remaining_micro_cents);
                return Some(BudgetAllocation {
                    remaining_micro_cents,
                    allocated_micro_cents: capped_cost,
                    budget: self,
                });
            }
        }
    }

    fn allocate_micro_cents(&self, max_cost: u64) -> Option<BudgetAllocation<'_>> {
        loop {
            let witness = self.remaining_micro_cents.load(Ordering::Relaxed);
            if witness >= max_cost
                && self
                    .remaining_micro_cents
                    .compare_exchange(
                        witness,
                        witness.saturating_sub(max_cost),
                        Ordering::Relaxed,
                        Ordering::Relaxed,
                    )
                    .is_ok()
            {
                let remaining_micro_cents = Arc::clone(&self.remaining_micro_cents);
                return Some(BudgetAllocation {
                    remaining_micro_cents,
                    allocated_micro_cents: max_cost,
                    budget: self,
                });
            } else if witness < max_cost {
                return None;
            }
        }
    }

    /// Calculates the output token cost for the given number of tokens.
    ///
    /// This is used to size budget allocations, which track output token budgets.
    /// Input token costs are charged directly to the main budget rather than
    /// the per-turn allocation, because input costs grow with conversation
    /// history and are not under the agent's control.
    fn calculate_output_cost_for_tokens(&self, tokens: u32) -> u64 {
        (tokens as u64).saturating_mul(self.output_token_rate_micro_cents)
    }

    /// Returns the current remaining budget in micro-cents.
    ///
    /// This method provides a snapshot of the budget's current state. Note that
    /// in concurrent scenarios, the value may change between the time you read
    /// it and when you use it for decisions.
    ///
    /// # Returns
    ///
    /// Current remaining budget as `u64` micro-cents
    ///
    /// # Example
    ///
    /// ```rust
    /// use claudius::Budget;
    ///
    /// let budget = Budget::from_dollars_flat_rate(5.0, 1000);
    /// assert_eq!(budget.remaining_micro_cents(), 500_000_000); // $5.00
    ///
    /// // After some allocations, the remaining amount decreases
    /// let _allocation = budget.allocate(100); // Reserves 100 * 1000 = 100,000 micro-cents
    /// assert_eq!(budget.remaining_micro_cents(), 499_900_000);
    /// ```
    ///
    /// # Thread Safety
    ///
    /// This method is thread-safe and uses atomic loads. The returned value
    /// represents a consistent point-in-time snapshot of the budget state.
    pub fn remaining_micro_cents(&self) -> u64 {
        self.remaining_micro_cents.load(Ordering::Relaxed)
    }

    /// Returns how many output tokens the current remaining budget can fund.
    pub fn remaining_output_tokens(&self) -> u32 {
        if self.output_token_rate_micro_cents == 0 {
            return u32::MAX;
        }
        std::cmp::min(
            self.remaining_micro_cents()
                .checked_div(self.output_token_rate_micro_cents)
                .unwrap_or(0),
            u32::MAX as u64,
        ) as u32
    }

    /// Returns the total micro-cents allocated to this budget.
    pub fn total_micro_cents(&self) -> u64 {
        self.total_micro_cents
    }

    /// Consumes a token count for a specific token category.
    ///
    /// Returns `true` if the budget was sufficient and the tokens were consumed.
    pub fn consume_token(&self, kind: TokenKind, tokens: u64) -> bool {
        let rate = match kind {
            TokenKind::Input => self.input_token_rate_micro_cents,
            TokenKind::Output => self.output_token_rate_micro_cents,
            TokenKind::CacheCreation => self.cache_creation_token_rate_micro_cents,
            TokenKind::CacheRead => self.cache_read_token_rate_micro_cents,
        };
        self.consume_cost_micro_cents(tokens.saturating_mul(rate))
    }

    /// Consumes budget based on an API usage record.
    pub fn consume_usage(&self, usage: &crate::Usage) -> bool {
        let cost = self.calculate_cost(usage);
        self.consume_cost_micro_cents(cost)
    }

    /// Consumes budget based on an API usage record, saturating at zero.
    ///
    /// Returns the amount consumed in micro-cents.
    pub fn consume_usage_saturating(&self, usage: &crate::Usage) -> u64 {
        let cost = self.calculate_cost(usage);
        self.consume_cost_micro_cents_saturating(cost)
    }

    fn consume_cost_micro_cents(&self, cost_micro_cents: u64) -> bool {
        loop {
            let witness = self.remaining_micro_cents.load(Ordering::Relaxed);
            if witness < cost_micro_cents {
                return false;
            }
            if self
                .remaining_micro_cents
                .compare_exchange(
                    witness,
                    witness.saturating_sub(cost_micro_cents),
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                )
                .is_ok()
            {
                return true;
            }
        }
    }

    pub(crate) fn consume_cost_micro_cents_saturating(&self, cost_micro_cents: u64) -> u64 {
        loop {
            let witness = self.remaining_micro_cents.load(Ordering::Relaxed);
            if witness == 0 {
                return 0;
            }
            let new_value = witness.saturating_sub(cost_micro_cents);
            if self
                .remaining_micro_cents
                .compare_exchange(witness, new_value, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                return witness.saturating_sub(new_value);
            }
        }
    }

    /// Legacy field access for backward compatibility.
    #[deprecated(note = "Use remaining_micro_cents() instead")]
    pub fn remaining(&self) -> Arc<AtomicU64> {
        Arc::clone(&self.remaining_micro_cents)
    }
}

impl Clone for Budget {
    fn clone(&self) -> Self {
        Self {
            remaining_micro_cents: Arc::clone(&self.remaining_micro_cents),
            total_micro_cents: self.total_micro_cents,
            input_token_rate_micro_cents: self.input_token_rate_micro_cents,
            output_token_rate_micro_cents: self.output_token_rate_micro_cents,
            cache_creation_token_rate_micro_cents: self.cache_creation_token_rate_micro_cents,
            cache_read_token_rate_micro_cents: self.cache_read_token_rate_micro_cents,
        }
    }
}

/// Represents an allocated portion of a budget for a specific operation.
///
/// A `BudgetAllocation` is created by calling [`Budget::allocate`] and represents
/// a reserved portion of the budget that can be consumed by API operations. The
/// allocation uses pessimistic budgeting - it reserves the maximum possible cost
/// for the expected number of tokens, then allows actual consumption up to that limit.
///
/// # Lifecycle
///
/// 1. **Creation**: Created via [`Budget::allocate`] with a maximum token count
/// 2. **Consumption**: Actual costs are deducted using [`consume_usage`]
/// 3. **Return**: Unused budget is automatically returned when the allocation is dropped
///
/// # Thread Safety
///
/// `BudgetAllocation` is not `Send` or `Sync` because it holds a reference to the
/// creating `Budget`. However, the underlying budget operations are thread-safe,
/// and multiple allocations can exist concurrently for the same budget.
///
/// # Example
///
/// ```rust
/// use claudius::{Budget, Usage};
///
/// let budget = Budget::from_dollars_flat_rate(5.0, 1000); // $5, 1000 micro-cents per token
///
/// // Allocate budget for up to 100 tokens
/// if let Some(mut allocation) = budget.allocate(100) {
///     println!("Allocated budget for {} tokens", allocation.remaining_tokens());
///
///     // Consume budget based on actual usage
///     let actual_usage = Usage::new(30, 20); // 30 input + 20 output = 50 total tokens
///
///     if allocation.consume_usage(&actual_usage) {
///         println!("Consumed budget for 50 tokens");
///         println!("Remaining in allocation: {} tokens", allocation.remaining_tokens());
///     }
///
///     // When allocation is dropped, unused budget (50 tokens worth) returns to the main budget
/// }
/// ```
///
/// [`consume_usage`]: BudgetAllocation::consume_usage
pub struct BudgetAllocation<'a> {
    remaining_micro_cents: Arc<AtomicU64>,
    allocated_micro_cents: u64,
    budget: &'a Budget,
}

impl<'a> BudgetAllocation<'a> {
    /// Consumes budget from this allocation based on actual API token usage.
    ///
    /// This method calculates the precise cost of the actual token usage and
    /// deducts it from the allocated budget. The cost calculation uses the
    /// original budget's token rates for each type of token consumed.
    ///
    /// This is the primary way to "spend" allocated budget after an API
    /// operation completes and you know the actual token consumption.
    ///
    /// # Arguments
    ///
    /// * `usage` - The actual token usage from an API response
    ///
    /// # Returns
    ///
    /// - `true` if the usage cost was within the allocated budget and was successfully consumed
    /// - `false` if the usage cost exceeds the remaining allocated budget
    ///
    /// # Example
    ///
    /// ```rust
    /// use claudius::{Budget, Usage};
    ///
    /// let budget = Budget::from_dollars_with_rates(1.0, 300, 1500, 150, 75);
    /// let mut allocation = budget.allocate(100).unwrap(); // Reserve for 100 tokens
    ///
    /// // API call completes with actual usage
    /// let actual_usage = Usage::new(40, 20) // 40 input + 20 output tokens
    ///     .with_cache_read_input_tokens(10);
    ///
    /// if allocation.consume_usage(&actual_usage) {
    ///     println!("Successfully consumed budget for actual usage");
    ///     // Cost: (40 * 300) + (20 * 1500) + (10 * 75) = 42,750 micro-cents
    /// } else {
    ///     println!("Usage exceeded allocated budget");
    /// }
    /// ```
    ///
    /// # Error Conditions
    ///
    /// Returns `false` when:
    /// - The calculated cost of `usage` exceeds `remaining_micro_cents()`
    /// - Multiple calls to `consume_usage` would exceed the total allocation
    ///
    /// # Note
    ///
    /// The `#[must_use]` attribute ensures you handle the return value, as
    /// failing to consume budget properly may indicate a logic error in
    /// your application.
    #[must_use]
    pub fn consume_usage(&mut self, usage: &crate::Usage) -> bool {
        let actual_cost = self.budget.calculate_cost(usage);
        if actual_cost <= self.allocated_micro_cents {
            self.allocated_micro_cents -= actual_cost;
            true
        } else {
            false
        }
    }

    /// Consumes budget for a complete API response, splitting costs appropriately.
    ///
    /// Output token costs are charged to this allocation, which represents the
    /// agent's generation budget for the current turn. Input token costs (input,
    /// cache creation, cache read) are charged directly to the main budget first
    /// because they grow with conversation history and are not under the agent's
    /// control. If the main budget cannot cover all input costs, the deficit is
    /// charged to this allocation so total budget accounting still saturates at
    /// zero after the request.
    ///
    /// This prevents input token costs from starving the output budget as
    /// conversations grow longer over multiple tool-use steps.
    ///
    /// # Returns
    ///
    /// - `true` if the output cost plus any input deficit was within the remaining allocation
    /// - `false` if that cost exceeds the remaining allocation
    ///
    /// Costs are deducted regardless of the return value, because the API call
    /// already happened.
    #[must_use]
    pub fn consume_response(&mut self, usage: &crate::Usage) -> bool {
        let input_cost = self.budget.calculate_input_cost(usage);
        let output_cost = self.budget.calculate_output_cost(usage);

        // Charge input costs to main budget (sunk cost from the API call).
        let input_consumed = self.budget.consume_cost_micro_cents_saturating(input_cost);
        let input_deficit = input_cost.saturating_sub(input_consumed);

        // Charge output costs to allocation. If the main budget could not cover
        // all input costs, charge that deficit to the reserved output budget so
        // total spend still saturates at zero after the request.
        let allocation_cost = output_cost.saturating_add(input_deficit);
        if allocation_cost <= self.allocated_micro_cents {
            self.allocated_micro_cents -= allocation_cost;
            true
        } else {
            self.allocated_micro_cents = 0;
            false
        }
    }

    /// Returns the number of output tokens that can still be generated from this allocation.
    ///
    /// This method uses the output token rate because allocations track output
    /// token budgets. Input token costs are charged directly to the main budget
    /// and do not reduce this allocation.
    pub fn remaining_tokens(&self) -> u32 {
        let rate = self.budget.output_token_rate_micro_cents;
        if rate > 0 {
            std::cmp::min(
                self.allocated_micro_cents.checked_div(rate).unwrap_or(0),
                u32::MAX as u64,
            ) as u32
        } else {
            0
        }
    }

    /// Returns the remaining budget within this allocation in micro-cents.
    ///
    /// This shows how much of the originally allocated budget remains available
    /// for consumption within this specific allocation. This is different from
    /// the main budget's remaining amount.
    ///
    /// # Returns
    ///
    /// Remaining micro-cents available for consumption in this allocation
    ///
    /// # Example
    ///
    /// ```rust
    /// use claudius::{Budget, Usage};
    ///
    /// let budget = Budget::from_dollars_flat_rate(1.0, 1000); // $1, 1000 micro-cents/token
    /// let mut allocation = budget.allocate(50).unwrap(); // Allocates 50,000 micro-cents
    ///
    /// assert_eq!(allocation.remaining_micro_cents(), 50_000);
    ///
    /// // Consume some budget
    /// let usage = Usage::new(20, 0); // 20,000 micro-cents
    /// allocation.consume_usage(&usage);
    ///
    /// assert_eq!(allocation.remaining_micro_cents(), 30_000); // 50k - 20k
    /// ```
    ///
    /// # Relationship to Main Budget
    ///
    /// This value represents budget "reserved" from the main budget but not yet
    /// consumed. When the allocation is dropped, this remaining amount is
    /// returned to the main budget automatically.
    pub fn remaining_micro_cents(&self) -> u64 {
        self.allocated_micro_cents
    }

    /// Returns the allocated budget in micro-cents for testing.
    #[cfg(test)]
    pub fn get_allocated_micro_cents(&self) -> u64 {
        self.allocated_micro_cents
    }

    /// Legacy field access for backward compatibility.
    #[deprecated(note = "Use remaining_tokens() instead")]
    pub fn allocated(&self) -> u32 {
        self.remaining_tokens()
    }

    /// Legacy method for backward compatibility.
    #[deprecated(note = "Use consume_usage instead")]
    #[must_use]
    pub fn consume(&mut self, amount: u32) -> bool {
        let cost = (amount as u64).saturating_mul(Budget::DEFAULT_RATE_MICRO_CENTS_PER_TOKEN);
        if cost <= self.allocated_micro_cents {
            self.allocated_micro_cents = self.allocated_micro_cents.saturating_sub(cost);
            true
        } else {
            false
        }
    }
}

/// Automatic budget return when allocation is dropped.
///
/// When a `BudgetAllocation` is dropped (goes out of scope), any unused
/// allocated budget is automatically returned to the main budget. This
/// ensures that budget is never permanently lost due to over-allocation.
///
/// # Thread Safety
///
/// The drop implementation uses atomic operations to safely return budget
/// to the main budget, even in concurrent scenarios.
///
/// # Example
///
/// ```rust
/// use claudius::{Budget, Usage};
///
/// let budget = Budget::from_dollars_flat_rate(1.0, 1000);
/// let initial_remaining = budget.remaining_micro_cents();
///
/// {
///     let mut allocation = budget.allocate(100).unwrap(); // Allocates 100,000 micro-cents
///     assert_eq!(budget.remaining_micro_cents(), initial_remaining - 100_000);
///
///     // Use only part of the allocation
///     let usage = Usage::new(30, 0); // Costs 30,000 micro-cents
///     allocation.consume_usage(&usage);
///
///     // allocation still holds 70,000 unused micro-cents
///     assert_eq!(allocation.remaining_micro_cents(), 70_000);
/// } // <- allocation dropped here
///
/// // The unused 70,000 micro-cents are returned to the main budget
/// assert_eq!(budget.remaining_micro_cents(), initial_remaining - 30_000);
/// ```
impl Drop for BudgetAllocation<'_> {
    fn drop(&mut self) {
        self.remaining_micro_cents
            .fetch_add(self.allocated_micro_cents, Ordering::Relaxed);
    }
}

/////////////////////////////////////////// Permissions ///////////////////////////////////////////

/// Permissions for filesystem mount points.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permissions {
    /// Read-only access to the filesystem.
    ReadOnly,
    /// Full read and write access to the filesystem.
    ReadWrite,
    /// Write-only access to the filesystem.
    WriteOnly,
}

/////////////////////////////////////////// FileSystem ////////////////////////////////////////////

/// Trait for implementing filesystem operations.
///
/// Provides an abstraction over filesystem operations that can be used by agents
/// to interact with files and directories.
#[async_trait::async_trait]
pub trait FileSystem: Send + Sync {
    /// Searches for files matching the given query.
    async fn search(&self, search: &str) -> Result<String, std::io::Error>;

    /// Lists directory entries, if `path` is a directory.
    ///
    /// Implementations return `Ok(None)` when they cannot distinguish directory
    /// listings from other `view` results.
    async fn list_directory(&self, path: &str) -> Result<Option<String>, std::io::Error> {
        let _ = path;
        Ok(None)
    }

    /// Views the contents of a file, optionally within a specific line range.
    ///
    /// # Parameters
    ///
    /// * `path` - The path to the file to view
    /// * `view_range` - Optional tuple of `(start, limit)` using 1-based line numbers.
    ///   Returns lines where `start <= line_number < limit`. For example, `(1, 4)` returns
    ///   lines 1, 2, and 3. Both `start` and `limit` must be >= 1.
    ///
    /// # Errors
    ///
    /// Returns an error if the path does not exist, is not readable, or if `view_range`
    /// contains a zero value.
    async fn view(
        &self,
        path: &str,
        view_range: Option<(u32, u32)>,
    ) -> Result<String, std::io::Error>;

    /// Replaces occurrences of a string in a file.
    async fn str_replace(
        &self,
        path: &str,
        old_str: &str,
        new_str: &str,
    ) -> Result<String, std::io::Error>;

    /// Inserts text at a specific line in a file.
    ///
    /// # Parameters
    ///
    /// * `path` - The path to the file to modify
    /// * `insert_line` - The 1-based line number where text will be inserted. Line 1 inserts
    ///   before the first line. To append to the end of a file with N lines, use `N + 1`.
    /// * `new_str` - The text to insert
    ///
    /// # Errors
    ///
    /// Returns [`std::io::ErrorKind::InvalidInput`] if `insert_line` is 0 or greater than
    /// the number of lines + 1.
    async fn insert(
        &self,
        path: &str,
        insert_line: u32,
        insert_text: &str,
    ) -> Result<String, std::io::Error>;

    /// Create a file or error if it already exists.
    ///
    /// This method creates a new file with the specified content. If the file already exists,
    /// it returns an error with [`std::io::ErrorKind::AlreadyExists`].
    ///
    /// # Parameters
    ///
    /// * `path` - The path where the file should be created
    /// * `file_text` - The content to write to the new file
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The file already exists
    /// - Permission is denied
    /// - Other I/O errors occur during file creation
    async fn create(&self, path: &str, file_text: &str) -> Result<String, std::io::Error>;
}

/////////////////////////////////////////////// Agent //////////////////////////////////////////////

/// Aggregated results from an agent turn.
#[derive(Debug, Clone)]
pub struct TurnOutcome {
    /// The reason the turn finished.
    pub stop_reason: StopReason,
    /// Usage accumulated across all requests in the turn.
    pub usage: Usage,
    /// Number of API requests made in the turn.
    pub request_count: u64,
}

/// Usage and request counts accumulated for a single step in a turn.
#[derive(Debug, Clone)]
pub struct TurnStep {
    /// Usage accumulated for the step.
    pub usage: Usage,
    /// Number of API requests made in the step.
    pub request_count: u64,
}

/// Trait for implementing agents that interact with the Anthropic API.
///
/// Agents encapsulate conversation logic, tool use, and configuration for
/// interacting with Claude models.
#[async_trait::async_trait]
pub trait Agent: Send + Sync + Sized {
    /// Returns the maximum number of tokens for responses.
    async fn max_tokens(&self) -> u32 {
        1024
    }

    /// Returns a display label for streaming output.
    fn stream_label(&self) -> String {
        std::any::type_name::<Self>()
            .rsplit("::")
            .next()
            .unwrap_or("Agent")
            .to_string()
    }

    /// Returns the model to use for this agent.
    async fn model(&self) -> Model {
        Model::Known(KnownModel::ClaudeSonnet40)
    }

    /// Returns optional metadata for requests.
    async fn metadata(&self) -> Option<Metadata> {
        None
    }

    /// Returns optional stop sequences to halt generation.
    async fn stop_sequences(&self) -> Option<Vec<String>> {
        None
    }

    /// Returns the system prompt for the agent.
    async fn system(&self) -> Option<SystemPrompt> {
        None
    }

    /// Returns whether prompt caching is enabled.
    fn caching_enabled(&self) -> bool {
        false
    }

    /// Returns the temperature for response generation.
    async fn temperature(&self) -> Option<f32> {
        None
    }

    /// Returns the thinking configuration for the agent.
    async fn thinking(&self) -> Option<ThinkingConfig> {
        None
    }

    /// Returns the tool choice configuration.
    async fn tool_choice(&self) -> Option<ToolChoice> {
        None
    }

    /// Returns the tools available to this agent.
    async fn tools(&self) -> Vec<Arc<dyn Tool<Self>>> {
        vec![]
    }

    /// Returns the top-k sampling parameter.
    async fn top_k(&self) -> Option<u32> {
        None
    }

    /// Returns the top-p (nucleus) sampling parameter.
    async fn top_p(&self) -> Option<f32> {
        None
    }

    /// Returns default beta feature headers for this agent's requests.
    ///
    /// These are merged with any client-level default betas and auto-detected
    /// betas. Duplicates are removed.
    async fn betas(&self) -> Option<Vec<String>> {
        None
    }

    /// Returns the output configuration for requests.
    ///
    /// This is used to pass effort levels for adaptive thinking via `OutputConfig`.
    async fn output_config(&self) -> Option<OutputConfig> {
        None
    }

    /// Returns the models to fall back to if a request is refused.
    ///
    /// When a model with safety classifiers declines a request with
    /// `stop_reason: "refusal"`, the API can transparently retry against these models
    /// in order. Server-side fallback is in beta and requires the `fallbacks` beta header.
    async fn fallbacks(&self) -> Option<Vec<Model>> {
        None
    }

    /// Returns the filesystem implementation for this agent.
    async fn filesystem(&self) -> Option<&dyn FileSystem> {
        None
    }

    /// Handles the case when max tokens is reached.
    async fn handle_max_tokens(&self) -> Result<StopReason, Error> {
        Ok(StopReason::MaxTokens)
    }

    /// Handles the end of a conversation turn.
    async fn handle_end_turn(&self) -> Result<StopReason, Error> {
        Ok(StopReason::EndTurn)
    }

    /// Handles when a stop sequence is encountered.
    async fn handle_stop_sequence(&self, sequence: Option<String>) -> Result<StopReason, Error> {
        _ = sequence;
        Ok(StopReason::StopSequence)
    }

    /// Handles when the model refuses to respond.
    async fn handle_refusal(&self, resp: Message) -> Result<StopReason, Error> {
        _ = resp;
        Ok(StopReason::Refusal)
    }

    /// Hook called before sending a message create request.
    async fn hook_message_create_params(&self, req: &MessageCreateParams) -> Result<(), Error> {
        _ = req;
        Ok(())
    }

    /// Hook called after receiving a message response.
    async fn hook_message(&self, resp: &Message) -> Result<(), Error> {
        _ = resp;
        Ok(())
    }

    /// Takes a conversation turn, potentially making multiple API calls.
    async fn take_turn(
        &mut self,
        client: &Anthropic,
        messages: &mut Vec<MessageParam>,
        budget: &Arc<Budget>,
    ) -> Result<TurnOutcome, Error> {
        self.take_default_turn(client, messages, budget).await
    }

    /// Takes a conversation turn, streaming output to the renderer.
    async fn take_turn_streaming(
        &mut self,
        client: &Anthropic,
        messages: &mut Vec<MessageParam>,
        budget: &Arc<Budget>,
        renderer: &mut dyn Renderer,
        context: AgentStreamContext,
    ) -> Result<TurnOutcome, Error> {
        self.take_default_turn_streaming(client, messages, budget, renderer, context)
            .await
    }

    /// Takes a conversation turn, streaming output with a root context label.
    async fn take_turn_streaming_root(
        &mut self,
        client: &Anthropic,
        messages: &mut Vec<MessageParam>,
        budget: &Arc<Budget>,
        renderer: &mut dyn Renderer,
    ) -> Result<TurnOutcome, Error> {
        let context = AgentStreamContext::root(self.stream_label());
        self.take_turn_streaming(client, messages, budget, renderer, context)
            .await
    }

    /// Default implementation for taking a conversation turn.
    async fn take_default_turn(
        &mut self,
        client: &Anthropic,
        messages: &mut Vec<MessageParam>,
        budget: &Arc<Budget>,
    ) -> Result<TurnOutcome, Error> {
        let turn_start = Instant::now();
        let Some(mut tokens_rem) = budget.allocate_available(self.max_tokens().await) else {
            AGENT_TURN_DURATION.add(turn_start.elapsed().as_secs_f64());
            let stop_reason = self.handle_max_tokens().await?;
            return Ok(TurnOutcome {
                stop_reason,
                usage: Usage::new(0, 0),
                request_count: 0,
            });
        };

        let mut usage_total = Usage::new(0, 0);
        let mut request_count: u64 = 0;

        while tokens_rem.remaining_tokens()
            > self.thinking().await.map(|t| t.num_tokens()).unwrap_or(0)
        {
            match self.step_turn(client, messages, &mut tokens_rem).await {
                ControlFlow::Continue(step) => {
                    usage_total = usage_total + step.usage;
                    request_count = request_count.saturating_add(step.request_count);
                }
                ControlFlow::Break(res) => {
                    AGENT_TURN_DURATION.add(turn_start.elapsed().as_secs_f64());
                    let mut outcome = res?;
                    outcome.usage = outcome.usage + usage_total;
                    outcome.request_count = outcome.request_count.saturating_add(request_count);
                    return Ok(outcome);
                }
            }
        }
        AGENT_TURN_DURATION.add(turn_start.elapsed().as_secs_f64());
        let stop_reason = self.handle_max_tokens().await?;
        Ok(TurnOutcome {
            stop_reason,
            usage: usage_total,
            request_count,
        })
    }

    /// Default implementation for taking a conversation turn with streaming output.
    async fn take_default_turn_streaming(
        &mut self,
        client: &Anthropic,
        messages: &mut Vec<MessageParam>,
        budget: &Arc<Budget>,
        renderer: &mut dyn Renderer,
        context: AgentStreamContext,
    ) -> Result<TurnOutcome, Error> {
        let turn_start = Instant::now();
        renderer.start_agent(&context);
        let Some(mut tokens_rem) = budget.allocate_available(self.max_tokens().await) else {
            AGENT_TURN_DURATION.add(turn_start.elapsed().as_secs_f64());
            let stop_reason = self.handle_max_tokens().await?;
            renderer.finish_agent(&context, Some(&stop_reason));
            return Ok(TurnOutcome {
                stop_reason,
                usage: Usage::new(0, 0),
                request_count: 0,
            });
        };

        let mut usage_total = Usage::new(0, 0);
        let mut request_count: u64 = 0;

        while tokens_rem.remaining_tokens()
            > self.thinking().await.map(|t| t.num_tokens()).unwrap_or(0)
        {
            match self
                .step_turn_streaming(client, messages, &mut tokens_rem, renderer, &context)
                .await
            {
                ControlFlow::Continue(step) => {
                    usage_total = usage_total + step.usage;
                    request_count = request_count.saturating_add(step.request_count);
                }
                ControlFlow::Break(res) => match res {
                    Ok(mut outcome) => {
                        outcome.usage = outcome.usage + usage_total;
                        outcome.request_count = outcome.request_count.saturating_add(request_count);
                        renderer.finish_agent(&context, Some(&outcome.stop_reason));
                        AGENT_TURN_DURATION.add(turn_start.elapsed().as_secs_f64());
                        return Ok(outcome);
                    }
                    Err(err) => {
                        renderer.finish_agent(&context, None);
                        AGENT_TURN_DURATION.add(turn_start.elapsed().as_secs_f64());
                        return Err(err);
                    }
                },
            }
        }
        AGENT_TURN_DURATION.add(turn_start.elapsed().as_secs_f64());
        let stop_reason = self.handle_max_tokens().await?;
        renderer.finish_agent(&context, Some(&stop_reason));
        Ok(TurnOutcome {
            stop_reason,
            usage: usage_total,
            request_count,
        })
    }

    /// Executes a single step in a conversation turn.
    async fn step_turn(
        &mut self,
        client: &Anthropic,
        messages: &mut Vec<MessageParam>,
        tokens_rem: &mut BudgetAllocation,
    ) -> ControlFlow<Result<TurnOutcome, Error>, TurnStep> {
        self.step_default_turn(client, messages, tokens_rem).await
    }

    /// Executes a single step in a conversation turn with streaming output.
    async fn step_turn_streaming(
        &mut self,
        client: &Anthropic,
        messages: &mut Vec<MessageParam>,
        tokens_rem: &mut BudgetAllocation,
        renderer: &mut dyn Renderer,
        context: &AgentStreamContext,
    ) -> ControlFlow<Result<TurnOutcome, Error>, TurnStep> {
        self.step_default_turn_streaming(client, messages, tokens_rem, renderer, context)
            .await
    }

    /// Default implementation for executing a single step in a conversation turn.
    async fn step_default_turn(
        &mut self,
        client: &Anthropic,
        messages: &mut Vec<MessageParam>,
        tokens_rem: &mut BudgetAllocation,
    ) -> ControlFlow<Result<TurnOutcome, Error>, TurnStep> {
        step_default_turn_impl(self, client, messages, tokens_rem, None).await
    }

    /// Default implementation for executing a single step with streaming output.
    async fn step_default_turn_streaming(
        &mut self,
        client: &Anthropic,
        messages: &mut Vec<MessageParam>,
        tokens_rem: &mut BudgetAllocation,
        renderer: &mut dyn Renderer,
        context: &AgentStreamContext,
    ) -> ControlFlow<Result<TurnOutcome, Error>, TurnStep> {
        let show_thinking = self.thinking().await.is_some();
        let streaming = StreamingContext {
            renderer,
            context,
            show_thinking,
        };
        step_default_turn_impl(self, client, messages, tokens_rem, Some(streaming)).await
    }

    /// Handles tool use requests from the model.
    async fn handle_tool_use(
        &mut self,
        client: &Anthropic,
        resp: &Message,
    ) -> ControlFlow<Result<StopReason, Error>, Vec<ContentBlock>> {
        self.handle_default_tool_use(client, resp).await
    }

    /// Handles tool use requests from the model with streaming output.
    async fn handle_tool_use_streaming(
        &mut self,
        client: &Anthropic,
        resp: &Message,
        renderer: &mut dyn Renderer,
        context: &AgentStreamContext,
    ) -> ControlFlow<Result<StopReason, Error>, Vec<ContentBlock>> {
        self.handle_default_tool_use_streaming(client, resp, renderer, context)
            .await
    }

    /// Default implementation for handling tool use requests.
    async fn handle_default_tool_use(
        &mut self,
        client: &Anthropic,
        resp: &Message,
    ) -> ControlFlow<Result<StopReason, Error>, Vec<ContentBlock>> {
        let tools_and_blocks = self.collect_tool_uses(resp).await;
        let mut tool_results = vec![];
        for (tool_use, tool) in tools_and_blocks.iter() {
            AGENT_TOOL_CALLS.click();
            let callback = tool.callback();
            let tool_use = tool_use.clone();
            let this = &*self;
            let compute_start = Instant::now();
            let intermediate = callback.compute_tool_result(client, this, &tool_use).await;
            let compute_duration = compute_start.elapsed();
            let apply_start = Instant::now();
            match callback
                .apply_tool_result(client, self, &tool_use, intermediate)
                .await
            {
                ControlFlow::Continue(result) => {
                    AGENT_TOOL_DURATION
                        .add((compute_duration + apply_start.elapsed()).as_secs_f64());
                    if result.is_err() {
                        AGENT_TOOL_ERRORS.click();
                    }
                    push_tool_result(&mut tool_results, None, result);
                }
                ControlFlow::Break(err) => {
                    AGENT_TOOL_DURATION
                        .add((compute_duration + apply_start.elapsed()).as_secs_f64());
                    AGENT_TOOL_ERRORS.click();
                    return ControlFlow::Break(Err(err));
                }
            }
        }
        ControlFlow::Continue(tool_results)
    }

    /// Default implementation for handling tool use requests with streaming output.
    async fn handle_default_tool_use_streaming(
        &mut self,
        client: &Anthropic,
        resp: &Message,
        renderer: &mut dyn Renderer,
        context: &AgentStreamContext,
    ) -> ControlFlow<Result<StopReason, Error>, Vec<ContentBlock>> {
        let mut tool_results = vec![];
        let tools_and_blocks = self.collect_tool_uses(resp).await;
        for (tool_use, tool) in tools_and_blocks.iter() {
            AGENT_TOOL_CALLS.click();
            let tool_context = context.child(format!("tool:{}", tool_use.name));
            let callback = tool.callback();
            let this = &*self;
            let start = Instant::now();
            let intermediate = callback
                .compute_tool_result_streaming(client, this, tool_use, renderer, &tool_context)
                .await;
            match callback
                .apply_tool_result(client, self, tool_use, intermediate)
                .await
            {
                ControlFlow::Continue(result) => {
                    AGENT_TOOL_DURATION.add(start.elapsed().as_secs_f64());
                    if result.is_err() {
                        AGENT_TOOL_ERRORS.click();
                    }
                    push_tool_result(&mut tool_results, Some((renderer, &tool_context)), result);
                }
                ControlFlow::Break(err) => {
                    AGENT_TOOL_DURATION.add(start.elapsed().as_secs_f64());
                    AGENT_TOOL_ERRORS.click();
                    return ControlFlow::Break(Err(err));
                }
            }
        }
        ControlFlow::Continue(tool_results)
    }

    /// Collect all ToolUseBlock blocks from the message.
    async fn collect_tool_uses(&self, resp: &Message) -> Vec<(ToolUseBlock, Arc<dyn Tool<Self>>)> {
        let tools = self.tools().await;
        let mut tools_and_blocks = vec![];
        for block in resp.content.iter() {
            let ContentBlock::ToolUse(tool_use) = block else {
                continue;
            };
            let tool = tools
                .iter()
                .find(|tool| tool.name() == tool_use.name)
                .cloned()
                .unwrap_or_else(|| Arc::new(ToolNotFound(tool_use.name.clone())) as _);
            tools_and_blocks.push((tool_use.clone(), tool));
        }
        tools_and_blocks
    }

    /// Creates a message request with the agent's configuration.
    async fn create_request(
        &self,
        max_tokens: u32,
        messages: Vec<MessageParam>,
        stream: bool,
    ) -> MessageCreateParams {
        let mut system = self.system().await;
        if self.caching_enabled() {
            apply_cache_controls(&mut system);
        }
        let tools = self
            .tools()
            .await
            .iter()
            .map(|tool| tool.to_param())
            .collect::<Vec<_>>();
        let tools = if tools.is_empty() { None } else { Some(tools) };
        MessageCreateParams {
            max_tokens,
            model: self.model().await,
            messages,
            cache_control: if self.caching_enabled() {
                Some(CacheControlEphemeral::new())
            } else {
                None
            },
            metadata: self.metadata().await,
            output_format: None,
            output_config: self.output_config().await,
            stop_sequences: self.stop_sequences().await,
            system,
            thinking: self.thinking().await,
            temperature: self.temperature().await,
            top_k: self.top_k().await,
            top_p: self.top_p().await,
            fallbacks: self.fallbacks().await,
            stream,
            tool_choice: self.tool_choice().await,
            tools,
            betas: self.betas().await,
        }
    }

    /// Handles text editor tool use.
    async fn text_editor(&self, tool_use: ToolUseBlock) -> Result<String, std::io::Error> {
        #[derive(serde::Deserialize)]
        struct Command {
            command: String,
        }
        let cmd: Command = serde_json::from_value(tool_use.input.clone())?;
        match cmd.command.as_str() {
            "view" => {
                #[derive(serde::Deserialize)]
                struct ViewTool {
                    path: String,
                    view_range: Option<(u32, u32)>,
                }
                let args: ViewTool = serde_json::from_value(tool_use.input)?;
                self.view(&args.path, args.view_range).await
            }
            "str_replace" => {
                #[derive(serde::Deserialize)]
                struct StrReplaceTool {
                    path: String,
                    old_str: String,
                    new_str: Option<String>,
                }
                let args: StrReplaceTool = serde_json::from_value(tool_use.input)?;
                let new_str = args.new_str.as_deref().unwrap_or("");
                self.str_replace(&args.path, &args.old_str, new_str).await
            }
            "insert" => {
                #[derive(serde::Deserialize)]
                struct InsertTool {
                    path: String,
                    insert_line: u32,
                    insert_text: Option<String>,
                    new_str: Option<String>,
                }
                let args: InsertTool = serde_json::from_value(tool_use.input)?;
                let text = args.insert_text.or(args.new_str).ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "missing insert_text field",
                    )
                })?;
                self.insert(&args.path, args.insert_line, &text).await
            }
            "create" => {
                /// Tool parameters for file creation.
                #[derive(serde::Deserialize)]
                struct CreateTool {
                    /// Path where the new file should be created.
                    path: String,
                    /// Content to write to the new file.
                    file_text: String,
                }
                let args: CreateTool = serde_json::from_value(tool_use.input)?;
                self.create(&args.path, &args.file_text).await
            }
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                format!("{} is not a supported tool", tool_use.name),
            )),
        }
    }

    /// Executes a bash command.
    async fn bash(&self, command: &str, restart: bool) -> Result<String, std::io::Error> {
        let _ = command;
        let _ = restart;
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "bash is not supported",
        ))
    }

    /// Searches the filesystem for files matching the query.
    async fn search(&self, search: &str) -> Result<String, std::io::Error> {
        if let Some(fs) = self.filesystem().await {
            fs.search(search).await
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "search is not supported",
            ))
        }
    }

    /// Views the contents of a file.
    async fn view(
        &self,
        path: &str,
        view_range: Option<(u32, u32)>,
    ) -> Result<String, std::io::Error> {
        if let Some(fs) = self.filesystem().await {
            fs.view(path, view_range).await
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "view is not supported",
            ))
        }
    }

    /// Replaces text in a file.
    async fn str_replace(
        &self,
        path: &str,
        old_str: &str,
        new_str: &str,
    ) -> Result<String, std::io::Error> {
        if let Some(fs) = self.filesystem().await {
            fs.str_replace(path, old_str, new_str).await
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "str_replace is not supported",
            ))
        }
    }

    /// Inserts text at a specific line in a file.
    async fn insert(
        &self,
        path: &str,
        insert_line: u32,
        insert_text: &str,
    ) -> Result<String, std::io::Error> {
        if let Some(fs) = self.filesystem().await {
            fs.insert(path, insert_line, insert_text).await
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "insert is not supported",
            ))
        }
    }

    /// Create a file or error if it exists.
    ///
    /// This is a convenience method that delegates to the underlying filesystem's
    /// `create` method. The file will be created with the specified content only
    /// if it doesn't already exist.
    ///
    /// # Parameters
    ///
    /// * `path` - The path where the file should be created
    /// * `file_text` - The content to write to the new file
    ///
    /// # Returns
    ///
    /// Returns "success" on successful file creation.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - No filesystem is available
    /// - The file already exists
    /// - Permission is denied
    /// - Other I/O errors occur
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// # use claudius::Agent;
    /// # async fn example<A: Agent>(agent: &A) -> Result<(), std::io::Error> {
    /// let result = agent.create("new_file.txt", "Hello, world!").await?;
    /// assert_eq!(result, "success");
    /// # Ok(())
    /// # }
    /// ```
    async fn create(&self, path: &str, file_text: &str) -> Result<String, std::io::Error> {
        if let Some(fs) = self.filesystem().await {
            fs.create(path, file_text).await
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "create is not supported",
            ))
        }
    }
}

#[async_trait::async_trait]
impl Agent for () {}

#[async_trait::async_trait]
impl FileSystem for Path<'_> {
    async fn search(&self, search: &str) -> Result<String, std::io::Error> {
        let output = std::process::Command::new("grep")
            .args(["-nRI", "--", search])
            .current_dir(self)
            .output()?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let count = format!(
            "\nsearch returned {} results\n",
            stdout.chars().filter(|c| *c == '\n').count()
        );
        Ok(stdout.to_string() + "\n" + &stderr + &count)
    }

    async fn list_directory(&self, path: &str) -> Result<Option<String>, std::io::Error> {
        let path = sanitize_path(self.clone(), path)?;
        if path.is_dir().unwrap_or(false) {
            Ok(Some(directory_listing(&path)?))
        } else {
            Ok(None)
        }
    }

    async fn view(
        &self,
        path: &str,
        view_range: Option<(u32, u32)>,
    ) -> Result<String, std::io::Error> {
        validate_view_range(view_range)?;
        let path = sanitize_path(self.clone(), path)?;
        if path.is_file().unwrap_or(false) {
            let content = std::fs::read_to_string(path)?;
            let lines = content
                .split('\n')
                .enumerate()
                .filter(|(idx, _)| {
                    view_range
                        .map(|(start, end)| (start..=end).contains(&(*idx as u32 + 1)))
                        .unwrap_or(true)
                })
                .map(|(_, line)| line)
                .collect::<Vec<_>>();
            let mut ret = lines.join("\n");
            ret.push('\n');
            Ok(ret)
        } else if path.is_dir().unwrap_or(false) {
            directory_listing(&path)
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "viewing non-standard file types is not supported",
            ))
        }
    }

    async fn str_replace(
        &self,
        path: &str,
        old_str: &str,
        new_str: &str,
    ) -> Result<String, std::io::Error> {
        let path = sanitize_path(self.clone(), path)?;
        if path.is_file().unwrap_or(false) {
            let content = std::fs::read_to_string(&path)?;
            let count = content.matches(old_str).count();
            if count == 0 {
                Err(std::io::Error::new(
                    std::io::ErrorKind::Unsupported,
                    "old_str not found in file",
                ))
            } else if count > 1 {
                Err(std::io::Error::new(
                    std::io::ErrorKind::Unsupported,
                    "old_str found in file more than once",
                ))
            } else {
                let content = content.replace(old_str, new_str);
                std::fs::write(path, content)?;
                Ok("success".to_string())
            }
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "editing non-standard file types is not supported",
            ))
        }
    }

    async fn insert(
        &self,
        path: &str,
        insert_line: u32,
        insert_text: &str,
    ) -> Result<String, std::io::Error> {
        let path = sanitize_path(self.clone(), path)?;
        if path.is_file().unwrap_or(false) {
            let content = std::fs::read_to_string(&path)?;
            let mut lines = content
                .split_terminator('\n')
                .map(|line| line.to_string())
                .collect::<Vec<_>>();
            let insert_idx = insert_line as usize;
            if insert_idx > lines.len() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "insert_line out of range",
                ));
            }
            lines.insert(insert_idx, insert_text.to_string());
            let mut out = lines.join("\n");
            out.push('\n');
            std::fs::write(path, out)?;
            Ok("success".to_string())
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "editing non-standard file types is not supported",
            ))
        }
    }

    /// Create a file within the filesystem path, ensuring it doesn't already exist.
    ///
    /// This implementation uses atomic file creation semantics - the file is only
    /// created if it doesn't already exist, preventing accidental overwrites.
    ///
    /// # Errors
    ///
    /// Returns [`std::io::ErrorKind::AlreadyExists`] if the file already exists.
    /// Returns other I/O errors if file creation fails for other reasons.
    async fn create(&self, path: &str, file_text: &str) -> Result<String, std::io::Error> {
        let path = sanitize_path(self.clone(), path)?;
        if !path.exists().unwrap_or(false) {
            std::fs::create_dir_all(path.dirname())?;
            std::fs::write(&path, file_text)?;
            Ok("success".to_string())
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "EEXISTS:  file exists",
            ))
        }
    }
}

/////////////////////////////////////////////// Mount //////////////////////////////////////////////

/// A filesystem mount point with associated permissions.
///
/// Wraps a filesystem implementation with a path prefix and access permissions,
/// enabling controlled access to specific parts of the filesystem.
pub struct Mount {
    path: Path<'static>,
    perm: Permissions,
    fs: Box<dyn FileSystem>,
}

#[async_trait::async_trait]
impl FileSystem for Mount {
    async fn search(&self, search: &str) -> Result<String, std::io::Error> {
        match self.perm {
            Permissions::WriteOnly => Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "search not allowed with WriteOnly permissions",
            )),
            Permissions::ReadOnly | Permissions::ReadWrite => self.fs.search(search).await,
        }
    }

    async fn list_directory(&self, path: &str) -> Result<Option<String>, std::io::Error> {
        match self.perm {
            Permissions::WriteOnly => Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "view not allowed with WriteOnly permissions",
            )),
            Permissions::ReadOnly | Permissions::ReadWrite => self.fs.list_directory(path).await,
        }
    }

    async fn view(
        &self,
        path: &str,
        view_range: Option<(u32, u32)>,
    ) -> Result<String, std::io::Error> {
        match self.perm {
            Permissions::WriteOnly => Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "view not allowed with WriteOnly permissions",
            )),
            Permissions::ReadOnly | Permissions::ReadWrite => self.fs.view(path, view_range).await,
        }
    }

    async fn str_replace(
        &self,
        path: &str,
        old_str: &str,
        new_str: &str,
    ) -> Result<String, std::io::Error> {
        match self.perm {
            Permissions::ReadOnly => Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "str_replace not allowed with ReadOnly permissions",
            )),
            Permissions::WriteOnly | Permissions::ReadWrite => {
                self.fs.str_replace(path, old_str, new_str).await
            }
        }
    }

    async fn insert(
        &self,
        path: &str,
        insert_line: u32,
        insert_text: &str,
    ) -> Result<String, std::io::Error> {
        match self.perm {
            Permissions::ReadOnly => Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "insert not allowed with ReadOnly permissions",
            )),
            Permissions::WriteOnly | Permissions::ReadWrite => {
                self.fs.insert(path, insert_line, insert_text).await
            }
        }
    }

    /// Create a file or error if it already exists.
    ///
    /// This method respects the mount's permission settings and delegates to the
    /// underlying filesystem for actual file creation.
    ///
    /// # Errors
    ///
    /// Returns [`std::io::ErrorKind::PermissionDenied`] if the mount has read-only permissions.
    /// Otherwise, returns any errors from the underlying filesystem.
    async fn create(&self, path: &str, file_text: &str) -> Result<String, std::io::Error> {
        match self.perm {
            Permissions::ReadOnly => Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "create not allowed with ReadOnly permissions",
            )),
            Permissions::WriteOnly | Permissions::ReadWrite => {
                self.fs.create(path, file_text).await
            }
        }
    }
}

////////////////////////////////////////// MountHierarchy //////////////////////////////////////////

/// Manages a hierarchy of filesystem mount points.
///
/// Maintains a collection of mount points with different permissions,
/// routing filesystem operations to the appropriate mount based on path.
#[derive(Default)]
pub struct MountHierarchy {
    mounts: Vec<Mount>,
}

impl MountHierarchy {
    /// Adds a new mount point to the hierarchy.
    ///
    /// Returns an error if the path conflicts with existing mounts or if
    /// the initial mount is not at the root.
    pub fn mount(
        &mut self,
        path: Path,
        perm: Permissions,
        fs: impl FileSystem + 'static,
    ) -> Result<(), String> {
        if !path.is_abs() {
            return Err("path must be absolute".to_string());
        }
        for mount in self.mounts.iter() {
            // If mount.path is a prefix of the current mount, then error.
            if mount.path.strip_prefix(path.clone()).is_some() && mount.path != path {
                return Err(format!(
                    "path must extend existing paths: {} masks {path}",
                    mount.path
                ));
            }
        }
        if self.mounts.is_empty() && path != "/".into() {
            return Err("initial mount point must be /".to_string());
        }
        let path = path.into_owned();
        let fs = Box::new(fs);
        self.mounts.push(Mount { path, perm, fs });
        Ok(())
    }

    fn mount_for_path(&self, path: &str) -> Result<(&Mount, Path<'static>), std::io::Error> {
        for mount in self.mounts.iter().rev() {
            if let Some(path) = Path::from(path).strip_prefix(mount.path.clone()) {
                let path = path.into_owned();
                return Ok((mount, path));
            }
        }
        Err(std::io::Error::other(
            "filesystem not initialized".to_string(),
        ))
    }

    fn fs_for_path(&self, path: &str) -> Result<(&dyn FileSystem, Path<'static>), std::io::Error> {
        let (mount, path) = self.mount_for_path(path)?;
        Ok((mount, path))
    }

    fn virtual_directory_entries(&self, path: &str) -> BTreeSet<String> {
        let path = Path::from(path);
        let mut seen_mount_paths = HashSet::new();
        let mut entries = BTreeSet::new();
        for mount in self.mounts.iter().rev() {
            if !seen_mount_paths.insert(mount.path.as_str().to_string()) {
                continue;
            }
            if !matches!(mount.perm, Permissions::ReadOnly | Permissions::ReadWrite) {
                continue;
            }
            if let Some(entry) = immediate_child_component(&mount.path, &path) {
                entries.insert(entry);
            }
        }
        entries
    }
}

#[async_trait::async_trait]
impl FileSystem for MountHierarchy {
    async fn search(&self, search: &str) -> Result<String, std::io::Error> {
        let mut output = String::new();
        for mount in self.mounts.iter() {
            output += &mount.search(search).await?;
            if !output.ends_with('\n') {
                output.push('\n');
            }
        }
        Ok(output)
    }

    async fn view(
        &self,
        path: &str,
        view_range: Option<(u32, u32)>,
    ) -> Result<String, std::io::Error> {
        let (mount, mounted_path) = self.mount_for_path(path)?;
        let virtual_entries = self.virtual_directory_entries(path);
        if virtual_entries.is_empty() {
            return mount.view(mounted_path.as_str(), view_range).await;
        }
        validate_view_range(view_range)?;
        let mut entries = virtual_entries;
        if let Some(listing) = mount.list_directory(mounted_path.as_str()).await? {
            for entry in listing.lines() {
                if !entry.is_empty() {
                    entries.insert(entry.to_string());
                }
            }
        }
        Ok(format_directory_entries(entries))
    }

    async fn str_replace(
        &self,
        path: &str,
        old_str: &str,
        new_str: &str,
    ) -> Result<String, std::io::Error> {
        let (fs, path) = self.fs_for_path(path)?;
        fs.str_replace(path.as_str(), old_str, new_str).await
    }

    async fn insert(
        &self,
        path: &str,
        insert_line: u32,
        insert_text: &str,
    ) -> Result<String, std::io::Error> {
        let (fs, path) = self.fs_for_path(path)?;
        fs.insert(path.as_str(), insert_line, insert_text).await
    }

    async fn create(&self, path: &str, file_text: &str) -> Result<String, std::io::Error> {
        let (fs, path) = self.fs_for_path(path)?;
        fs.create(path.as_str(), file_text).await
    }
}

/////////////////////////////////////////////// Misc ///////////////////////////////////////////////

fn validate_view_range(view_range: Option<(u32, u32)>) -> Result<(), std::io::Error> {
    if let Some((start, limit)) = view_range
        && (start == 0 || limit == 0)
    {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "view_range values must be >= 1",
        ));
    }
    Ok(())
}

fn directory_listing(path: &Path) -> Result<String, std::io::Error> {
    let mut listing = String::new();
    for dirent in std::fs::read_dir(path)? {
        let dirent = dirent?;
        let p = Path::try_from(dirent.path()).map_err(std::io::Error::other)?;
        if let Some(p) = p.strip_prefix(path.clone()) {
            listing.push_str(p.as_str());
            listing.push('\n');
        }
    }
    Ok(listing)
}

fn immediate_child_component(path: &Path, prefix: &Path) -> Option<String> {
    if path.has_root() != prefix.has_root() || path.has_app_defined() != prefix.has_app_defined() {
        return None;
    }
    let path_components = ordinary_components(path)?;
    let prefix_components = ordinary_components(prefix)?;
    if prefix_components.len() >= path_components.len() {
        return None;
    }
    if path_components
        .iter()
        .zip(prefix_components.iter())
        .all(|(path, prefix)| path == prefix)
    {
        Some(path_components[prefix_components.len()].clone())
    } else {
        None
    }
}

fn ordinary_components(path: &Path) -> Option<Vec<String>> {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            utf8path::Component::RootDir
            | utf8path::Component::AppDefined
            | utf8path::Component::CurDir => {}
            utf8path::Component::ParentDir => return None,
            utf8path::Component::Normal(component) => {
                components.push(component.as_str().to_string())
            }
        }
    }
    Some(components)
}

fn format_directory_entries(entries: BTreeSet<String>) -> String {
    let mut listing = String::new();
    for entry in entries {
        listing.push_str(&entry);
        listing.push('\n');
    }
    listing
}

fn sanitize_path(base: Path, path: &str) -> Result<Path<'static>, std::io::Error> {
    let path = Path::from(path);
    if path
        .components()
        .any(|c| matches!(c, utf8path::Component::AppDefined))
    {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "viewing // paths is not supported",
        ))
    } else if path
        .components()
        .any(|c| matches!(c, utf8path::Component::ParentDir))
    {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            ".. path name prohibited",
        ))
    } else {
        let path = path.as_str().trim_start_matches('/');
        Ok(base.join(path).into_owned())
    }
}

//////////////////////////////////////// Streaming Helpers /////////////////////////////////////////

/// Renders a complete tool result block to the renderer.
///
/// Emits the full lifecycle: start_tool_result -> print content -> finish_tool_result.
fn render_tool_result_block(
    renderer: &mut dyn Renderer,
    context: &dyn StreamContext,
    block: &ToolResultBlock,
) {
    renderer.start_tool_result(context, &block.tool_use_id, block.is_error.unwrap_or(false));
    if let Some(content) = &block.content {
        render_tool_result_content(renderer, context, content);
    }
    renderer.finish_tool_result(context);
}

/// Renders tool result content (string or array of content items).
///
/// For arrays, inserts newlines between items and renders images as "[image]" placeholder.
fn render_tool_result_content(
    renderer: &mut dyn Renderer,
    context: &dyn StreamContext,
    content: &ToolResultBlockContent,
) {
    match content {
        ToolResultBlockContent::String(text) => renderer.print_tool_result_text(context, text),
        ToolResultBlockContent::Array(items) => {
            for (idx, item) in items.iter().enumerate() {
                if idx > 0 {
                    renderer.print_tool_result_text(context, "\n");
                }
                match item {
                    crate::types::Content::Text(text) => {
                        renderer.print_tool_result_text(context, &text.text);
                    }
                    crate::types::Content::Image(_) => {
                        renderer.print_tool_result_text(context, "[image]");
                    }
                }
            }
        }
    }
}

fn push_tool_result(
    tool_results: &mut Vec<ContentBlock>,
    renderer: Option<(&mut dyn Renderer, &dyn StreamContext)>,
    result: Result<ToolResultBlock, ToolResultBlock>,
) {
    match result {
        Ok(block) => {
            if let Some((renderer, context)) = renderer {
                render_tool_result_block(renderer, context, &block);
            }
            tool_results.push(block.into());
        }
        Err(block) => {
            if let Some((renderer, context)) = renderer {
                render_tool_result_block(renderer, context, &block);
            }
            tool_results.push(block.with_error(true).into());
        }
    }
}

async fn step_default_turn_impl<A: Agent>(
    agent: &mut A,
    client: &Anthropic,
    messages: &mut Vec<MessageParam>,
    tokens_rem: &mut BudgetAllocation<'_>,
    mut streaming: Option<StreamingContext<'_>>,
) -> ControlFlow<Result<TurnOutcome, Error>, TurnStep> {
    let stream = streaming.is_some();
    let mut usage_total = Usage::new(0, 0);
    let mut request_count: u64 = 0;
    loop {
        let max_tokens = std::cmp::min(agent.max_tokens().await, tokens_rem.remaining_tokens());
        let req = agent
            .create_request(max_tokens, messages.clone(), stream)
            .await;
        if let Err(err) = agent.hook_message_create_params(&req).await {
            return ControlFlow::Break(Err(err));
        }

        AGENT_TURN_REQUESTS.click();
        let resp = if let Some(streaming) = streaming.as_mut() {
            match stream_message_with_renderer(
                client,
                req,
                streaming.renderer,
                streaming.context,
                streaming.show_thinking,
            )
            .await
            {
                Ok(resp) => resp,
                Err(err) => return ControlFlow::Break(Err(err)),
            }
        } else {
            match client.send(req).await {
                Ok(resp) => resp,
                Err(err) => return ControlFlow::Break(Err(err)),
            }
        };

        let assistant_message = MessageParam {
            role: MessageRole::Assistant,
            content: MessageParamContent::Array(resp.content.clone()),
        };
        usage_total = usage_total + resp.usage;
        request_count = request_count.saturating_add(1);
        let response_within_budget = tokens_rem.consume_response(&resp.usage);

        if let Err(err) = agent.hook_message(&resp).await {
            return ControlFlow::Break(Err(err));
        }

        if !response_within_budget {
            return ControlFlow::Break(Ok(TurnOutcome {
                stop_reason: StopReason::MaxTokens,
                usage: usage_total,
                request_count,
            }));
        }
        push_or_merge_message(messages, assistant_message);

        let tool_results = match resp.stop_reason {
            None | Some(StopReason::EndTurn) => {
                let stop_reason = match agent.handle_end_turn().await {
                    Ok(stop_reason) => stop_reason,
                    Err(err) => return ControlFlow::Break(Err(err)),
                };
                return ControlFlow::Break(Ok(TurnOutcome {
                    stop_reason,
                    usage: usage_total,
                    request_count,
                }));
            }
            Some(StopReason::MaxTokens) => {
                let stop_reason = match agent.handle_max_tokens().await {
                    Ok(stop_reason) => stop_reason,
                    Err(err) => return ControlFlow::Break(Err(err)),
                };
                return ControlFlow::Break(Ok(TurnOutcome {
                    stop_reason,
                    usage: usage_total,
                    request_count,
                }));
            }
            Some(StopReason::StopSequence) => {
                let stop_reason = match agent.handle_stop_sequence(resp.stop_sequence).await {
                    Ok(stop_reason) => stop_reason,
                    Err(err) => return ControlFlow::Break(Err(err)),
                };
                return ControlFlow::Break(Ok(TurnOutcome {
                    stop_reason,
                    usage: usage_total,
                    request_count,
                }));
            }
            Some(StopReason::Refusal) => {
                let stop_reason = match agent.handle_refusal(resp).await {
                    Ok(stop_reason) => stop_reason,
                    Err(err) => return ControlFlow::Break(Err(err)),
                };
                return ControlFlow::Break(Ok(TurnOutcome {
                    stop_reason,
                    usage: usage_total,
                    request_count,
                }));
            }
            Some(StopReason::ModelContextWindowExceeded) => {
                return ControlFlow::Break(Ok(TurnOutcome {
                    stop_reason: StopReason::ModelContextWindowExceeded,
                    usage: usage_total,
                    request_count,
                }));
            }
            Some(StopReason::PauseTurn) => {
                continue;
            }
            Some(StopReason::ToolUse) => {
                if let Some(streaming) = streaming.as_mut() {
                    match agent
                        .handle_tool_use_streaming(
                            client,
                            &resp,
                            streaming.renderer,
                            streaming.context,
                        )
                        .await
                    {
                        ControlFlow::Continue(results) => results,
                        ControlFlow::Break(err) => {
                            let outcome = err.map(|stop_reason| TurnOutcome {
                                stop_reason,
                                usage: usage_total,
                                request_count,
                            });
                            return ControlFlow::Break(outcome);
                        }
                    }
                } else {
                    match agent.handle_tool_use(client, &resp).await {
                        ControlFlow::Continue(results) => results,
                        ControlFlow::Break(err) => {
                            let outcome = err.map(|stop_reason| TurnOutcome {
                                stop_reason,
                                usage: usage_total,
                                request_count,
                            });
                            return ControlFlow::Break(outcome);
                        }
                    }
                }
            }
        };

        // The model reported stop_reason = tool_use, but no usable tool_use blocks
        // were collected (e.g. the model rendered text resembling a tool call instead
        // of emitting a structured tool_use block). Appending a user message with empty
        // content here would produce an invalid transcript entry ({"role":"user","content":[]})
        // that the API rejects on the next request or on replay. End the turn instead.
        if tool_results.is_empty() {
            let stop_reason = match agent.handle_end_turn().await {
                Ok(stop_reason) => stop_reason,
                Err(err) => return ControlFlow::Break(Err(err)),
            };
            return ControlFlow::Break(Ok(TurnOutcome {
                stop_reason,
                usage: usage_total,
                request_count,
            }));
        }

        let user_message =
            MessageParam::new(MessageParamContent::Array(tool_results), MessageRole::User);
        push_or_merge_message(messages, user_message);
        return ControlFlow::Continue(TurnStep {
            usage: usage_total,
            request_count,
        });
    }
}

async fn stream_message_with_renderer(
    client: &Anthropic,
    req: MessageCreateParams,
    renderer: &mut dyn Renderer,
    context: &dyn StreamContext,
    show_thinking: bool,
) -> Result<Message, Error> {
    let stream = client.stream(&req).await?;
    let fallback_message = Message::new(
        "streamed".to_string(),
        Vec::new(),
        req.model.clone(),
        Usage::new(0, 0),
    );
    let (mut acc_stream, rx) = AccumulatingStream::new_with_message(stream, fallback_message);
    let mut active_tool_uses = HashSet::new();
    let mut active_tool_results = HashSet::new();
    let mut active_thinking_blocks = HashSet::new();
    let mut rendered_thinking_blocks = HashSet::new();

    while let Some(event) = acc_stream.next().await {
        if renderer.should_interrupt() {
            renderer.print_interrupted(context);
            let mut partial = acc_stream.finalize_partial()?;
            if partial.stop_reason.is_none() {
                partial.stop_reason = Some(StopReason::EndTurn);
            }
            return Ok(partial);
        }
        match event {
            Ok(event) => match &event {
                MessageStreamEvent::Ping => {}
                MessageStreamEvent::MessageStart(_) => {}
                MessageStreamEvent::MessageDelta(_) => {}
                MessageStreamEvent::ContentBlockStart(start_event) => {
                    match &start_event.content_block {
                        ContentBlock::ToolUse(tool_use) => {
                            active_tool_uses.insert(start_event.index);
                            renderer.start_tool_use(context, &tool_use.name, &tool_use.id);
                        }
                        ContentBlock::ToolResult(tool_result) => {
                            active_tool_results.insert(start_event.index);
                            renderer.start_tool_result(
                                context,
                                &tool_result.tool_use_id,
                                tool_result.is_error.unwrap_or(false),
                            );
                            if let Some(content) = &tool_result.content {
                                render_tool_result_content(renderer, context, content);
                            }
                        }
                        ContentBlock::Text(text_block) if !text_block.text.is_empty() => {
                            renderer.print_text(context, &text_block.text);
                        }
                        block if block.is_thinking() => {
                            let thinking_block = block
                                .as_thinking()
                                .expect("is_thinking blocks must expose thinking");
                            active_thinking_blocks.insert(start_event.index);
                            if show_thinking && !thinking_block.thinking.is_empty() {
                                rendered_thinking_blocks.insert(start_event.index);
                                renderer.print_thinking(context, &thinking_block.thinking);
                            }
                        }
                        _ => {}
                    }
                }
                MessageStreamEvent::ContentBlockDelta(delta_event) => match &delta_event.delta {
                    ContentBlockDelta::InputJsonDelta(json_delta) => {
                        if active_tool_uses.contains(&delta_event.index) {
                            renderer.print_tool_input(context, &json_delta.partial_json);
                        }
                    }
                    ContentBlockDelta::TextDelta(text_delta) => {
                        if active_thinking_blocks.contains(&delta_event.index) {
                            if show_thinking && !text_delta.text.is_empty() {
                                rendered_thinking_blocks.insert(delta_event.index);
                                renderer.print_thinking(context, &text_delta.text);
                            }
                        } else if active_tool_results.contains(&delta_event.index) {
                            renderer.print_tool_result_text(context, &text_delta.text);
                        } else {
                            renderer.print_text(context, &text_delta.text);
                        }
                    }
                    ContentBlockDelta::ThinkingDelta(thinking_delta) => {
                        if show_thinking && !thinking_delta.thinking.is_empty() {
                            rendered_thinking_blocks.insert(delta_event.index);
                            renderer.print_thinking(context, &thinking_delta.thinking);
                        }
                    }
                    ContentBlockDelta::SignatureDelta(_) => {}
                    ContentBlockDelta::CitationsDelta(_) => {}
                },
                MessageStreamEvent::ContentBlockStop(stop_event) => {
                    if active_tool_uses.remove(&stop_event.index) {
                        renderer.finish_tool_use(context);
                    }
                    if active_tool_results.remove(&stop_event.index) {
                        renderer.finish_tool_result(context);
                    }
                    active_thinking_blocks.remove(&stop_event.index);
                }
                MessageStreamEvent::MessageStop(_) => {}
            },
            Err(err) => {
                renderer.print_error(context, &err.to_string());
                return Err(err);
            }
        }
    }

    match rx.await {
        Ok(Ok(resp)) => {
            render_unstreamed_thinking_blocks(
                renderer,
                context,
                show_thinking,
                &resp,
                &rendered_thinking_blocks,
            );
            renderer.finish_response(context);
            Ok(resp)
        }
        Ok(Err(err)) => {
            renderer.print_error(context, &err.to_string());
            Err(err)
        }
        Err(_) => {
            let err = Error::streaming("failed to receive accumulated streaming message", None);
            renderer.print_error(context, &err.to_string());
            Err(err)
        }
    }
}

fn render_unstreamed_thinking_blocks(
    renderer: &mut dyn Renderer,
    context: &dyn StreamContext,
    show_thinking: bool,
    message: &Message,
    rendered_thinking_blocks: &HashSet<usize>,
) {
    if !show_thinking {
        return;
    }

    for (index, block) in message.content.iter().enumerate() {
        let Some(thinking_block) = block.as_thinking() else {
            continue;
        };
        if rendered_thinking_blocks.contains(&index) || thinking_block.thinking.is_empty() {
            continue;
        }
        renderer.print_thinking(context, &thinking_block.thinking);
    }
}

/////////////////////////////////////////////// tests //////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TextBlock, ThinkingBlock, Usage};
    use std::sync::atomic::Ordering;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[derive(Default)]
    struct CapturingRenderer {
        thinking: Vec<String>,
    }

    impl Renderer for CapturingRenderer {
        fn print_text(&mut self, _context: &dyn StreamContext, _text: &str) {}

        fn print_thinking(&mut self, _context: &dyn StreamContext, text: &str) {
            self.thinking.push(text.to_string());
        }

        fn print_error(&mut self, _context: &dyn StreamContext, _error: &str) {}

        fn print_info(&mut self, _context: &dyn StreamContext, _info: &str) {}

        fn start_tool_use(&mut self, _context: &dyn StreamContext, _name: &str, _id: &str) {}

        fn print_tool_input(&mut self, _context: &dyn StreamContext, _partial_json: &str) {}

        fn finish_tool_use(&mut self, _context: &dyn StreamContext) {}

        fn start_tool_result(
            &mut self,
            _context: &dyn StreamContext,
            _tool_use_id: &str,
            _is_error: bool,
        ) {
        }

        fn print_tool_result_text(&mut self, _context: &dyn StreamContext, _text: &str) {}

        fn finish_tool_result(&mut self, _context: &dyn StreamContext) {}

        fn finish_response(&mut self, _context: &dyn StreamContext) {}
    }

    fn make_temp_dir(prefix: &str) -> std::path::PathBuf {
        let mut dir = std::env::temp_dir();
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        dir.push(format!(
            "claudius_test_{prefix}_{}_{}",
            std::process::id(),
            nanos
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn render_unstreamed_thinking_blocks_prints_missing_summaries() {
        let message = Message::new(
            "msg".to_string(),
            vec![
                ContentBlock::Thinking(ThinkingBlock::new("summary", "sig")),
                ContentBlock::Text(TextBlock::new("answer")),
            ],
            KnownModel::ClaudeOpus48.into(),
            Usage::new(10, 20),
        );
        let mut renderer = CapturingRenderer::default();

        render_unstreamed_thinking_blocks(&mut renderer, &(), true, &message, &HashSet::new());

        assert_eq!(renderer.thinking, vec!["summary"]);
    }

    #[test]
    fn render_unstreamed_thinking_blocks_skips_already_rendered_summaries() {
        let message = Message::new(
            "msg".to_string(),
            vec![ContentBlock::Thinking(ThinkingBlock::new("summary", "sig"))],
            KnownModel::ClaudeOpus48.into(),
            Usage::new(10, 20),
        );
        let mut renderer = CapturingRenderer::default();

        render_unstreamed_thinking_blocks(&mut renderer, &(), true, &message, &HashSet::from([0]));

        assert!(renderer.thinking.is_empty());
    }

    #[test]
    fn budget_new_flat_rate_creates_with_correct_amount() {
        let budget = Budget::new_flat_rate(1000, 10);
        assert_eq!(budget.remaining_micro_cents(), 1000);
    }

    #[test]
    fn budget_from_dollars_creates_correct_amount() {
        let budget = Budget::from_dollars_flat_rate(1.0, 100);
        assert_eq!(budget.remaining_micro_cents(), 100_000_000);
    }

    #[test]
    fn budget_calculate_cost_basic_usage() {
        use crate::Usage;
        let budget = Budget::new_with_rates(10000, 10, 20, 5, 15);

        let usage = Usage::new(50, 100);
        let cost = budget.calculate_cost(&usage);
        let expected_cost = (50u64 * 10).saturating_add(100u64 * 20);
        assert_eq!(cost, expected_cost);
    }

    #[test]
    fn budget_calculate_cost_with_cache() {
        use crate::Usage;
        let budget = Budget::new_with_rates(10000, 10, 20, 5, 15);

        let usage = Usage::new(50, 100)
            .with_cache_creation_input_tokens(20)
            .with_cache_read_input_tokens(30);
        let cost = budget.calculate_cost(&usage);
        let expected_cost = (50u64 * 10)
            .checked_add(100u64 * 20)
            .and_then(|sum| sum.checked_add(20u64 * 5))
            .and_then(|sum| sum.checked_add(30u64 * 15))
            .unwrap_or(u64::MAX);
        assert_eq!(cost, expected_cost);
    }

    #[test]
    fn budget_allocate_succeeds_when_sufficient_budget() {
        let budget = Budget::new_flat_rate(1000, 10);
        let allocation = budget.allocate(50);
        assert!(allocation.is_some());

        let allocation = allocation.unwrap();
        assert_eq!(allocation.remaining_tokens(), 50);
    }

    #[test]
    fn budget_allocate_fails_when_insufficient_budget() {
        let budget = Budget::new_flat_rate(500, 10);
        let allocation = budget.allocate(100);
        assert!(allocation.is_none());
        assert_eq!(budget.remaining_micro_cents(), 500);
    }

    #[test]
    fn budget_allocate_available_caps_to_remaining_output_tokens() {
        let budget = Budget::new_flat_rate(255, 10);
        let allocation = budget.allocate_available(100).unwrap();

        assert_eq!(allocation.remaining_tokens(), 25);
        assert_eq!(budget.remaining_micro_cents(), 5);
    }

    #[test]
    fn budget_allocate_available_fails_when_no_output_token_fits() {
        let budget = Budget::new_flat_rate(9, 10);

        assert!(budget.allocate_available(100).is_none());
        assert_eq!(budget.remaining_micro_cents(), 9);
    }

    #[test]
    fn budget_allocation_consume_usage_valid() {
        use crate::Usage;
        let budget = Budget::new_flat_rate(1000, 10);
        let mut allocation = budget.allocate(50).unwrap();

        let usage = Usage::new(20, 15);
        assert!(allocation.consume_usage(&usage));

        let remaining_cost = allocation.remaining_micro_cents();
        let expected_remaining = (20u64 * 10)
            .checked_add(15u64 * 10)
            .and_then(|consumed| 500u64.checked_sub(consumed))
            .unwrap_or(0);
        assert_eq!(remaining_cost, expected_remaining);
    }

    #[test]
    fn budget_allocation_consume_usage_excessive() {
        use crate::Usage;
        let budget = Budget::new_flat_rate(300, 10);
        let mut allocation = budget.allocate(20).unwrap();

        let usage = Usage::new(50, 100);
        assert!(!allocation.consume_usage(&usage));
    }

    #[test]
    fn budget_allocation_drop_returns_remaining_budget() {
        let budget = Budget::new_flat_rate(1000, 10);
        let initial_remaining = budget.remaining_micro_cents();

        {
            let _allocation = budget.allocate(50).unwrap();
            assert_eq!(budget.remaining_micro_cents(), initial_remaining - 500);
        }

        assert_eq!(budget.remaining_micro_cents(), initial_remaining);
    }

    #[test]
    fn budget_multiple_allocations() {
        let budget = Budget::new_flat_rate(1000, 10);

        let alloc1 = budget.allocate(30);
        assert!(alloc1.is_some());
        assert_eq!(budget.remaining_micro_cents(), 700);

        let alloc2 = budget.allocate(40);
        assert!(alloc2.is_some());
        assert_eq!(budget.remaining_micro_cents(), 300);

        let alloc3 = budget.allocate(40);
        assert!(alloc3.is_none());
        assert_eq!(budget.remaining_micro_cents(), 300);
    }

    #[test]
    fn budget_concurrent_allocation_safety() {
        use std::sync::{Barrier, Mutex};
        use std::thread;

        // Create budget with enough for exactly 5 allocations of 20 tokens each
        let budget = Budget::new_flat_rate(1000, 10);

        // First, verify our calculation with a single allocation
        let test_alloc = budget.allocate(20);
        assert!(test_alloc.is_some());
        let alloc = test_alloc.unwrap();
        assert_eq!(alloc.remaining_tokens(), 20);
        drop(alloc); // Return the budget

        // Use scoped threads to keep allocations alive during the test
        let allocations = Mutex::new(Vec::new());
        let barrier = Barrier::new(10);

        thread::scope(|s| {
            for _ in 0..10 {
                s.spawn(|| {
                    // All threads wait here until all 10 have reached this point
                    barrier.wait();
                    // Now all threads try to allocate simultaneously
                    if let Some(allocation) = budget.allocate(20) {
                        allocations.lock().unwrap().push(allocation);
                    }
                });
            }
        });

        let final_allocations = allocations.into_inner().unwrap();
        let successful_allocations = final_allocations.len();

        // Each allocation should cost 20 * 10 = 200 micro-cents
        // With 1000 micro-cents total, only 5 allocations should succeed
        assert!(
            successful_allocations <= 5,
            "Got {} successful allocations, expected at most 5",
            successful_allocations
        );

        // Drop all allocations and verify budget accounting
        drop(final_allocations);
        assert_eq!(budget.remaining_micro_cents(), 1000);
    }

    #[test]
    fn budget_allocation_cost_calculation_verification() {
        let budget = Budget::new_flat_rate(1000, 10);

        // Verify the rates are set correctly
        assert_eq!(budget.input_token_rate_micro_cents, 10);
        assert_eq!(budget.output_token_rate_micro_cents, 10);
        assert_eq!(budget.cache_creation_token_rate_micro_cents, 10);
        assert_eq!(budget.cache_read_token_rate_micro_cents, 10);

        // Test the max rate calculation
        let max_rate = budget
            .output_token_rate_micro_cents
            .max(budget.input_token_rate_micro_cents)
            .max(budget.cache_creation_token_rate_micro_cents)
            .max(budget.cache_read_token_rate_micro_cents);
        assert_eq!(max_rate, 10);

        // Calculate expected cost for 20 tokens
        let expected_cost = (20u64).saturating_mul(max_rate);
        assert_eq!(expected_cost, 200);
    }

    #[test]
    fn test_token_consumption_calculation() {
        use crate::Usage;

        let usage_no_cache = Usage::new(50, 100);
        let total_tokens = usage_no_cache.input_tokens
            + usage_no_cache.cache_creation_input_tokens.unwrap_or(0)
            + usage_no_cache.cache_read_input_tokens.unwrap_or(0)
            + usage_no_cache.output_tokens;
        assert_eq!(total_tokens, 150);

        let usage_with_cache = Usage::new(50, 100)
            .with_cache_creation_input_tokens(20)
            .with_cache_read_input_tokens(30);
        let total_tokens_cached = usage_with_cache.input_tokens
            + usage_with_cache.cache_creation_input_tokens.unwrap_or(0)
            + usage_with_cache.cache_read_input_tokens.unwrap_or(0)
            + usage_with_cache.output_tokens;
        assert_eq!(total_tokens_cached, 200);

        let usage_partial_cache = Usage::new(50, 100).with_cache_read_input_tokens(25);
        let total_tokens_partial = usage_partial_cache.input_tokens
            + usage_partial_cache.cache_creation_input_tokens.unwrap_or(0)
            + usage_partial_cache.cache_read_input_tokens.unwrap_or(0)
            + usage_partial_cache.output_tokens;
        assert_eq!(total_tokens_partial, 175);
    }

    // MountHierarchy tests

    #[test]
    fn mount_hierarchy_initial_mount_must_be_root() {
        let mut hierarchy = MountHierarchy { mounts: vec![] };

        // First mount must be /
        let result = hierarchy.mount("/home".into(), Permissions::ReadWrite, Path::from("/tmp"));
        assert_eq!(result, Err("initial mount point must be /".to_string()));

        // After mounting /, other paths can be mounted
        assert!(
            hierarchy
                .mount("/".into(), Permissions::ReadWrite, Path::from("/tmp"))
                .is_ok()
        );
        assert!(
            hierarchy
                .mount("/home".into(), Permissions::ReadWrite, Path::from("/tmp"))
                .is_ok()
        );
    }

    #[test]
    fn mount_hierarchy_path_must_be_absolute() {
        let mut hierarchy = MountHierarchy { mounts: vec![] };

        let result = hierarchy.mount(
            "relative/path".into(),
            Permissions::ReadWrite,
            Path::from("/tmp"),
        );
        assert_eq!(result, Err("path must be absolute".to_string()));

        let result = hierarchy.mount("./path".into(), Permissions::ReadWrite, Path::from("/tmp"));
        assert_eq!(result, Err("path must be absolute".to_string()));

        let result = hierarchy.mount("../path".into(), Permissions::ReadWrite, Path::from("/tmp"));
        assert_eq!(result, Err("path must be absolute".to_string()));
    }

    #[test]
    fn mount_hierarchy_cannot_mask_existing_mount() {
        let mut hierarchy = MountHierarchy { mounts: vec![] };

        // Mount / and /home
        assert!(
            hierarchy
                .mount("/".into(), Permissions::ReadWrite, Path::from("/tmp"))
                .is_ok()
        );
        assert!(
            hierarchy
                .mount("/home".into(), Permissions::ReadWrite, Path::from("/tmp"))
                .is_ok()
        );

        // Cannot mount / again since it would mask /home
        let result = hierarchy.mount("/".into(), Permissions::ReadWrite, Path::from("/tmp"));
        assert!(result.is_err());
        let err_msg = result.unwrap_err();
        eprintln!("err_msg: {err_msg:?}");
        assert!(err_msg.contains("path must extend existing paths"));
        assert!(err_msg.contains("/home masks"));
    }

    #[test]
    fn mount_hierarchy_can_mount_same_path_multiple_times() {
        let mut hierarchy = MountHierarchy { mounts: vec![] };

        // Mount /
        assert!(
            hierarchy
                .mount("/".into(), Permissions::ReadWrite, Path::from("/tmp1"))
                .is_ok()
        );

        // Can mount / again (overlays previous mount)
        assert!(
            hierarchy
                .mount("/".into(), Permissions::ReadWrite, Path::from("/tmp2"))
                .is_ok()
        );

        assert_eq!(hierarchy.mounts.len(), 2);
    }

    #[test]
    fn mount_hierarchy_fs_for_path_finds_most_specific_mount() {
        let mut hierarchy = MountHierarchy { mounts: vec![] };

        // Mount different paths
        assert!(
            hierarchy
                .mount("/".into(), Permissions::ReadWrite, Path::from("/root"))
                .is_ok()
        );
        assert!(
            hierarchy
                .mount(
                    "/home".into(),
                    Permissions::ReadWrite,
                    Path::from("/home_fs")
                )
                .is_ok()
        );
        assert!(
            hierarchy
                .mount(
                    "/home/user".into(),
                    Permissions::ReadWrite,
                    Path::from("/user_fs")
                )
                .is_ok()
        );

        // Check that fs_for_path returns the most specific mount
        let fs = hierarchy.fs_for_path("/file.txt").unwrap().0;
        // Cast both to raw pointers to compare addresses without vtable metadata
        let fs_ptr = fs as *const dyn FileSystem as *const ();
        let expected_ptr =
            &hierarchy.mounts[0] as &dyn FileSystem as *const dyn FileSystem as *const ();
        assert_eq!(fs_ptr, expected_ptr);

        let fs = hierarchy.fs_for_path("/home/file.txt").unwrap().0;
        let fs_ptr = fs as *const dyn FileSystem as *const ();
        let expected_ptr =
            &hierarchy.mounts[1] as &dyn FileSystem as *const dyn FileSystem as *const ();
        assert_eq!(fs_ptr, expected_ptr);

        let fs = hierarchy.fs_for_path("/home/user/file.txt").unwrap().0;
        let fs_ptr = fs as *const dyn FileSystem as *const ();
        let expected_ptr =
            &hierarchy.mounts[2] as &dyn FileSystem as *const dyn FileSystem as *const ();
        assert_eq!(fs_ptr, expected_ptr);
    }

    #[test]
    fn mount_hierarchy_fs_for_path_error_when_no_mount() {
        let hierarchy = MountHierarchy { mounts: vec![] };

        let result = hierarchy.fs_for_path("/any/path");
        assert!(result.is_err());
        if let Err(err) = result {
            assert_eq!(err.kind(), std::io::ErrorKind::Other);
            assert_eq!(err.to_string(), "filesystem not initialized");
        }
    }

    enum MockResult {
        Ok(String),
        Err(std::io::ErrorKind, String),
    }

    struct MockFileSystem {
        search_result: MockResult,
        view_result: MockResult,
        str_replace_result: MockResult,
        insert_result: MockResult,
        create_result: MockResult,
    }

    impl MockFileSystem {
        fn new_ok(name: &str) -> Self {
            Self {
                search_result: MockResult::Ok(format!("search from {name}")),
                view_result: MockResult::Ok(format!("view from {name}")),
                str_replace_result: MockResult::Ok(format!("str_replace from {name}")),
                insert_result: MockResult::Ok(format!("insert from {name}")),
                create_result: MockResult::Ok(format!("create from {name}")),
            }
        }

        fn new_err(name: &str, kind: std::io::ErrorKind) -> Self {
            Self {
                search_result: MockResult::Err(kind, format!("search error from {name}")),
                view_result: MockResult::Err(kind, format!("view error from {name}")),
                str_replace_result: MockResult::Err(kind, format!("str_replace error from {name}")),
                insert_result: MockResult::Err(kind, format!("insert error from {name}")),
                create_result: MockResult::Err(kind, format!("create error from {name}")),
            }
        }
    }

    impl MockResult {
        fn to_result(&self) -> Result<String, std::io::Error> {
            match self {
                MockResult::Ok(s) => Ok(s.clone()),
                MockResult::Err(kind, msg) => Err(std::io::Error::new(*kind, msg.clone())),
            }
        }
    }

    #[async_trait::async_trait]
    impl FileSystem for MockFileSystem {
        async fn search(&self, _search: &str) -> Result<String, std::io::Error> {
            self.search_result.to_result()
        }

        async fn view(
            &self,
            _path: &str,
            _view_range: Option<(u32, u32)>,
        ) -> Result<String, std::io::Error> {
            self.view_result.to_result()
        }

        async fn str_replace(
            &self,
            _path: &str,
            _old_str: &str,
            _insert_text: &str,
        ) -> Result<String, std::io::Error> {
            self.str_replace_result.to_result()
        }

        async fn insert(
            &self,
            _path: &str,
            _insert_line: u32,
            _insert_text: &str,
        ) -> Result<String, std::io::Error> {
            self.insert_result.to_result()
        }

        async fn create(&self, _path: &str, _file_text: &str) -> Result<String, std::io::Error> {
            self.create_result.to_result()
        }
    }

    #[tokio::test]
    async fn mount_hierarchy_search_aggregates_all_mounts() {
        let mut hierarchy = MountHierarchy { mounts: vec![] };

        hierarchy
            .mount(
                "/".into(),
                Permissions::ReadWrite,
                MockFileSystem::new_ok("root"),
            )
            .unwrap();
        hierarchy
            .mount(
                "/home".into(),
                Permissions::ReadWrite,
                MockFileSystem::new_ok("home"),
            )
            .unwrap();
        hierarchy
            .mount(
                "/usr".into(),
                Permissions::ReadWrite,
                MockFileSystem::new_ok("usr"),
            )
            .unwrap();

        let result = hierarchy.search("test").await.unwrap();
        assert_eq!(
            result,
            "search from root\nsearch from home\nsearch from usr\n"
        );
    }

    #[tokio::test]
    async fn mount_hierarchy_search_error_propagates() {
        let mut hierarchy = MountHierarchy { mounts: vec![] };

        hierarchy
            .mount(
                "/".into(),
                Permissions::ReadWrite,
                MockFileSystem::new_ok("root"),
            )
            .unwrap();
        hierarchy
            .mount(
                "/home".into(),
                Permissions::ReadWrite,
                MockFileSystem::new_err("home", std::io::ErrorKind::PermissionDenied),
            )
            .unwrap();

        let result = hierarchy.search("test").await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
        assert!(err.to_string().contains("search error from home"));
    }

    #[tokio::test]
    async fn mount_hierarchy_search_adds_newlines() {
        let mut hierarchy = MountHierarchy { mounts: vec![] };

        // Mock that doesn't end with newline
        let mut mock = MockFileSystem::new_ok("no_newline");
        mock.search_result = MockResult::Ok("result without newline".to_string());

        hierarchy
            .mount("/".into(), Permissions::ReadWrite, mock)
            .unwrap();
        hierarchy
            .mount(
                "/home".into(),
                Permissions::ReadWrite,
                MockFileSystem::new_ok("home"),
            )
            .unwrap();

        let result = hierarchy.search("test").await.unwrap();
        assert_eq!(result, "result without newline\nsearch from home\n");
    }

    #[tokio::test]
    async fn mount_hierarchy_view_uses_correct_filesystem() {
        let mut hierarchy = MountHierarchy { mounts: vec![] };

        hierarchy
            .mount(
                "/".into(),
                Permissions::ReadWrite,
                MockFileSystem::new_ok("root"),
            )
            .unwrap();
        hierarchy
            .mount(
                "/home".into(),
                Permissions::ReadWrite,
                MockFileSystem::new_ok("home"),
            )
            .unwrap();

        let result = hierarchy.view("/file.txt", None).await.unwrap();
        assert_eq!(result, "view from root");

        let result = hierarchy
            .view("/home/file.txt", Some((1, 10)))
            .await
            .unwrap();
        assert_eq!(result, "view from home");
    }

    #[tokio::test]
    async fn mount_hierarchy_view_error_no_filesystem() {
        let hierarchy = MountHierarchy { mounts: vec![] };

        let result = hierarchy.view("/file.txt", None).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::Other);
        assert_eq!(err.to_string(), "filesystem not initialized");
    }

    #[tokio::test]
    async fn mount_hierarchy_view_error_from_filesystem() {
        let mut hierarchy = MountHierarchy { mounts: vec![] };

        hierarchy
            .mount(
                "/".into(),
                Permissions::ReadWrite,
                MockFileSystem::new_err("root", std::io::ErrorKind::NotFound),
            )
            .unwrap();

        let result = hierarchy.view("/file.txt", None).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
        assert!(err.to_string().contains("view error from root"));
    }

    #[tokio::test]
    async fn mount_hierarchy_views_virtual_mount_parent_directory() {
        let workspace = make_temp_dir("mount_parent_workspace");
        let skill = make_temp_dir("mount_parent_skill");
        std::fs::write(skill.join("SKILL.md"), "# Rust").unwrap();

        let mut hierarchy = MountHierarchy::default();
        hierarchy
            .mount(
                "/".into(),
                Permissions::ReadWrite,
                Path::try_from(workspace.clone()).unwrap(),
            )
            .unwrap();
        hierarchy
            .mount(
                "/skills/rust".into(),
                Permissions::ReadOnly,
                Path::try_from(skill.clone()).unwrap(),
            )
            .unwrap();

        let content = hierarchy.view("/skills/rust/SKILL.md", None).await.unwrap();
        assert_eq!(content, "# Rust\n");

        let listing = hierarchy.view("/skills", None).await.unwrap();
        assert_eq!(listing, "rust\n");

        std::fs::remove_dir_all(workspace).ok();
        std::fs::remove_dir_all(skill).ok();
    }

    #[tokio::test]
    async fn mount_hierarchy_virtual_directory_lists_only_immediate_children() {
        let workspace = make_temp_dir("mount_immediate_workspace");
        let rustfmt = make_temp_dir("mount_immediate_rustfmt");
        let lint = make_temp_dir("mount_immediate_lint");

        let mut hierarchy = MountHierarchy::default();
        hierarchy
            .mount(
                "/".into(),
                Permissions::ReadWrite,
                Path::try_from(workspace.clone()).unwrap(),
            )
            .unwrap();
        hierarchy
            .mount(
                "/skills/rust/rustfmt".into(),
                Permissions::ReadOnly,
                Path::try_from(rustfmt.clone()).unwrap(),
            )
            .unwrap();
        hierarchy
            .mount(
                "/skills/python/lint".into(),
                Permissions::ReadOnly,
                Path::try_from(lint.clone()).unwrap(),
            )
            .unwrap();

        let listing = hierarchy.view("/skills", None).await.unwrap();
        assert_eq!(listing, "python\nrust\n");

        let listing = hierarchy.view("/skills/python", None).await.unwrap();
        assert_eq!(listing, "lint\n");

        let listing = hierarchy.view("/skills/rust", None).await.unwrap();
        assert_eq!(listing, "rustfmt\n");

        std::fs::remove_dir_all(workspace).ok();
        std::fs::remove_dir_all(rustfmt).ok();
        std::fs::remove_dir_all(lint).ok();
    }

    #[tokio::test]
    async fn mount_hierarchy_virtual_directory_merges_physical_directory_entries() {
        let workspace = make_temp_dir("mount_merge_workspace");
        let skill = make_temp_dir("mount_merge_skill");
        std::fs::create_dir_all(workspace.join("skills")).unwrap();
        std::fs::write(workspace.join("skills/local.txt"), "local\n").unwrap();

        let mut hierarchy = MountHierarchy::default();
        hierarchy
            .mount(
                "/".into(),
                Permissions::ReadWrite,
                Path::try_from(workspace.clone()).unwrap(),
            )
            .unwrap();
        hierarchy
            .mount(
                "/skills/rust".into(),
                Permissions::ReadOnly,
                Path::try_from(skill.clone()).unwrap(),
            )
            .unwrap();

        let listing = hierarchy.view("/skills", None).await.unwrap();
        assert_eq!(listing, "local.txt\nrust\n");

        std::fs::remove_dir_all(workspace).ok();
        std::fs::remove_dir_all(skill).ok();
    }

    #[tokio::test]
    async fn mount_hierarchy_virtual_directory_overrides_physical_file() {
        let workspace = make_temp_dir("mount_file_workspace");
        let skill = make_temp_dir("mount_file_skill");
        std::fs::write(workspace.join("skills"), "not a directory\n").unwrap();

        let mut hierarchy = MountHierarchy::default();
        hierarchy
            .mount(
                "/".into(),
                Permissions::ReadWrite,
                Path::try_from(workspace.clone()).unwrap(),
            )
            .unwrap();
        hierarchy
            .mount(
                "/skills/rust".into(),
                Permissions::ReadOnly,
                Path::try_from(skill.clone()).unwrap(),
            )
            .unwrap();

        let listing = hierarchy.view("/skills", None).await.unwrap();
        assert_eq!(listing, "rust\n");

        std::fs::remove_dir_all(workspace).ok();
        std::fs::remove_dir_all(skill).ok();
    }

    #[tokio::test]
    async fn mount_hierarchy_virtual_entries_are_merged_at_exact_mounts() {
        let workspace = make_temp_dir("mount_exact_workspace");
        let skills = make_temp_dir("mount_exact_skills");
        let rust = make_temp_dir("mount_exact_rust");
        std::fs::write(skills.join("local.txt"), "local\n").unwrap();

        let mut hierarchy = MountHierarchy::default();
        hierarchy
            .mount(
                "/".into(),
                Permissions::ReadWrite,
                Path::try_from(workspace.clone()).unwrap(),
            )
            .unwrap();
        hierarchy
            .mount(
                "/skills".into(),
                Permissions::ReadOnly,
                Path::try_from(skills.clone()).unwrap(),
            )
            .unwrap();
        hierarchy
            .mount(
                "/skills/rust".into(),
                Permissions::ReadOnly,
                Path::try_from(rust.clone()).unwrap(),
            )
            .unwrap();

        let listing = hierarchy.view("/skills", None).await.unwrap();
        assert_eq!(listing, "local.txt\nrust\n");

        std::fs::remove_dir_all(workspace).ok();
        std::fs::remove_dir_all(skills).ok();
        std::fs::remove_dir_all(rust).ok();
    }

    #[tokio::test]
    async fn mount_hierarchy_virtual_entries_are_merged_at_root() {
        let workspace = make_temp_dir("mount_root_workspace");
        let skill = make_temp_dir("mount_root_skill");
        std::fs::write(workspace.join("README.md"), "workspace\n").unwrap();

        let mut hierarchy = MountHierarchy::default();
        hierarchy
            .mount(
                "/".into(),
                Permissions::ReadWrite,
                Path::try_from(workspace.clone()).unwrap(),
            )
            .unwrap();
        hierarchy
            .mount(
                "/skills/rust".into(),
                Permissions::ReadOnly,
                Path::try_from(skill.clone()).unwrap(),
            )
            .unwrap();

        let listing = hierarchy.view("/", None).await.unwrap();
        assert_eq!(listing, "README.md\nskills\n");

        std::fs::remove_dir_all(workspace).ok();
        std::fs::remove_dir_all(skill).ok();
    }

    #[tokio::test]
    async fn mount_hierarchy_virtual_directory_omits_write_only_children() {
        let workspace = make_temp_dir("mount_perms_workspace");
        let read = make_temp_dir("mount_perms_read");
        let write = make_temp_dir("mount_perms_write");

        let mut hierarchy = MountHierarchy::default();
        hierarchy
            .mount(
                "/".into(),
                Permissions::ReadWrite,
                Path::try_from(workspace.clone()).unwrap(),
            )
            .unwrap();
        hierarchy
            .mount(
                "/skills/read".into(),
                Permissions::ReadOnly,
                Path::try_from(read.clone()).unwrap(),
            )
            .unwrap();
        hierarchy
            .mount(
                "/skills/write".into(),
                Permissions::WriteOnly,
                Path::try_from(write.clone()).unwrap(),
            )
            .unwrap();

        let listing = hierarchy.view("/skills", None).await.unwrap();
        assert_eq!(listing, "read\n");

        std::fs::remove_dir_all(workspace).ok();
        std::fs::remove_dir_all(read).ok();
        std::fs::remove_dir_all(write).ok();
    }

    #[tokio::test]
    async fn mount_hierarchy_virtual_directory_respects_overlay_permissions() {
        let workspace = make_temp_dir("mount_overlay_workspace");
        let readable = make_temp_dir("mount_overlay_readable");
        let hidden = make_temp_dir("mount_overlay_hidden");
        let python = make_temp_dir("mount_overlay_python");

        let mut hierarchy = MountHierarchy::default();
        hierarchy
            .mount(
                "/".into(),
                Permissions::ReadWrite,
                Path::try_from(workspace.clone()).unwrap(),
            )
            .unwrap();
        hierarchy
            .mount(
                "/skills/rust".into(),
                Permissions::ReadOnly,
                Path::try_from(readable.clone()).unwrap(),
            )
            .unwrap();
        hierarchy
            .mount(
                "/skills/rust".into(),
                Permissions::WriteOnly,
                Path::try_from(hidden.clone()).unwrap(),
            )
            .unwrap();
        hierarchy
            .mount(
                "/skills/python".into(),
                Permissions::ReadOnly,
                Path::try_from(python.clone()).unwrap(),
            )
            .unwrap();

        let listing = hierarchy.view("/skills", None).await.unwrap();
        assert_eq!(listing, "python\n");

        std::fs::remove_dir_all(workspace).ok();
        std::fs::remove_dir_all(readable).ok();
        std::fs::remove_dir_all(hidden).ok();
        std::fs::remove_dir_all(python).ok();
    }

    #[tokio::test]
    async fn mount_hierarchy_virtual_directory_rejects_invalid_view_range() {
        let workspace = make_temp_dir("mount_range_workspace");
        let skill = make_temp_dir("mount_range_skill");

        let mut hierarchy = MountHierarchy::default();
        hierarchy
            .mount(
                "/".into(),
                Permissions::ReadWrite,
                Path::try_from(workspace.clone()).unwrap(),
            )
            .unwrap();
        hierarchy
            .mount(
                "/skills/rust".into(),
                Permissions::ReadOnly,
                Path::try_from(skill.clone()).unwrap(),
            )
            .unwrap();

        let result = hierarchy.view("/skills", Some((0, 1))).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        assert_eq!(err.to_string(), "view_range values must be >= 1");

        std::fs::remove_dir_all(workspace).ok();
        std::fs::remove_dir_all(skill).ok();
    }

    #[tokio::test]
    async fn mount_hierarchy_str_replace_uses_correct_filesystem() {
        let mut hierarchy = MountHierarchy { mounts: vec![] };

        hierarchy
            .mount(
                "/".into(),
                Permissions::ReadWrite,
                MockFileSystem::new_ok("root"),
            )
            .unwrap();
        hierarchy
            .mount(
                "/home".into(),
                Permissions::ReadWrite,
                MockFileSystem::new_ok("home"),
            )
            .unwrap();

        let result = hierarchy
            .str_replace("/file.txt", "old", "new")
            .await
            .unwrap();
        assert_eq!(result, "str_replace from root");

        let result = hierarchy
            .str_replace("/home/file.txt", "old", "new")
            .await
            .unwrap();
        assert_eq!(result, "str_replace from home");
    }

    #[tokio::test]
    async fn mount_hierarchy_str_replace_error_no_filesystem() {
        let hierarchy = MountHierarchy { mounts: vec![] };

        let result = hierarchy.str_replace("/file.txt", "old", "new").await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::Other);
        assert_eq!(err.to_string(), "filesystem not initialized");
    }

    #[tokio::test]
    async fn mount_hierarchy_str_replace_error_from_filesystem() {
        let mut hierarchy = MountHierarchy { mounts: vec![] };

        hierarchy
            .mount(
                "/".into(),
                Permissions::ReadWrite,
                MockFileSystem::new_err("root", std::io::ErrorKind::PermissionDenied),
            )
            .unwrap();

        let result = hierarchy.str_replace("/file.txt", "old", "new").await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
        assert!(err.to_string().contains("str_replace error from root"));
    }

    #[tokio::test]
    async fn mount_hierarchy_insert_uses_correct_filesystem() {
        let mut hierarchy = MountHierarchy { mounts: vec![] };

        hierarchy
            .mount(
                "/".into(),
                Permissions::ReadWrite,
                MockFileSystem::new_ok("root"),
            )
            .unwrap();
        hierarchy
            .mount(
                "/home".into(),
                Permissions::ReadWrite,
                MockFileSystem::new_ok("home"),
            )
            .unwrap();

        let result = hierarchy.insert("/file.txt", 5, "new line").await.unwrap();
        assert_eq!(result, "insert from root");

        let result = hierarchy
            .insert("/home/file.txt", 10, "new line")
            .await
            .unwrap();
        assert_eq!(result, "insert from home");
    }

    #[tokio::test]
    async fn mount_hierarchy_insert_error_no_filesystem() {
        let hierarchy = MountHierarchy { mounts: vec![] };

        let result = hierarchy.insert("/file.txt", 5, "new line").await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::Other);
        assert_eq!(err.to_string(), "filesystem not initialized");
    }

    #[tokio::test]
    async fn mount_hierarchy_insert_error_from_filesystem() {
        let mut hierarchy = MountHierarchy { mounts: vec![] };

        hierarchy
            .mount(
                "/".into(),
                Permissions::ReadWrite,
                MockFileSystem::new_err("root", std::io::ErrorKind::AddrInUse),
            )
            .unwrap();

        let result = hierarchy.insert("/file.txt", 5, "new line").await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::AddrInUse);
        assert!(err.to_string().contains("insert error from root"));
    }

    #[tokio::test]
    async fn mount_hierarchy_overlay_mounts() {
        let mut hierarchy = MountHierarchy { mounts: vec![] };

        // First mount at /
        hierarchy
            .mount(
                "/".into(),
                Permissions::ReadWrite,
                MockFileSystem::new_ok("first"),
            )
            .unwrap();

        // Overlay mount at same path
        hierarchy
            .mount(
                "/".into(),
                Permissions::ReadWrite,
                MockFileSystem::new_ok("second"),
            )
            .unwrap();

        // Should use the most recent mount
        let result = hierarchy.view("/file.txt", None).await.unwrap();
        assert_eq!(result, "view from second");
    }

    #[test]
    fn mount_hierarchy_complex_path_scenarios() {
        let mut hierarchy = MountHierarchy { mounts: vec![] };

        // Mount various paths
        assert!(
            hierarchy
                .mount("/".into(), Permissions::ReadWrite, Path::from("/root"))
                .is_ok()
        );
        assert!(
            hierarchy
                .mount("/home".into(), Permissions::ReadWrite, Path::from("/home"))
                .is_ok()
        );
        assert!(
            hierarchy
                .mount(
                    "/home/user".into(),
                    Permissions::ReadWrite,
                    Path::from("/user")
                )
                .is_ok()
        );
        assert!(
            hierarchy
                .mount("/var".into(), Permissions::ReadWrite, Path::from("/var"))
                .is_ok()
        );
        assert!(
            hierarchy
                .mount(
                    "/var/log".into(),
                    Permissions::ReadWrite,
                    Path::from("/log")
                )
                .is_ok()
        );

        // Cannot mount path that would mask existing deeper paths
        let result = hierarchy.mount(
            "/home".into(),
            Permissions::ReadWrite,
            Path::from("/new_home"),
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("/home/user masks"));

        let result = hierarchy.mount(
            "/var".into(),
            Permissions::ReadWrite,
            Path::from("/new_var"),
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("/var/log masks"));

        // Can mount paths that don't conflict
        assert!(
            hierarchy
                .mount("/usr".into(), Permissions::ReadWrite, Path::from("/usr"))
                .is_ok()
        );
        assert!(
            hierarchy
                .mount(
                    "/home/other".into(),
                    Permissions::ReadWrite,
                    Path::from("/other")
                )
                .is_ok()
        );
    }

    #[tokio::test]
    async fn filesystem_search_dash_query_is_pattern() {
        let dir = make_temp_dir("search_dash");
        let file_path = dir.join("file.txt");
        std::fs::write(&file_path, "-n pattern\n").unwrap();
        let base = Path::try_from(dir.as_path()).unwrap();

        let output = base.search("-n").await.unwrap();
        assert!(output.contains("file.txt:1:-n pattern"));

        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn filesystem_insert_zero_prepends() {
        let dir = make_temp_dir("insert_zero");
        let file_path = dir.join("file.txt");
        std::fs::write(&file_path, "a\nb\n").unwrap();
        let base = Path::try_from(dir.as_path()).unwrap();

        // insert_line=0 places text at the beginning of the file
        base.insert("file.txt", 0, "x").await.unwrap();
        let contents = std::fs::read_to_string(&file_path).unwrap();
        assert_eq!(contents, "x\na\nb\n");

        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn filesystem_insert_after_line() {
        let dir = make_temp_dir("insert_after");
        let file_path = dir.join("file.txt");
        std::fs::write(&file_path, "a\nb\n").unwrap();
        let base = Path::try_from(dir.as_path()).unwrap();

        // insert_line=1 inserts after line 1
        base.insert("file.txt", 1, "x").await.unwrap();
        let contents = std::fs::read_to_string(&file_path).unwrap();
        assert_eq!(contents, "a\nx\nb\n");

        // insert_line=3 appends at the end (after line 3)
        base.insert("file.txt", 3, "y").await.unwrap();
        let contents = std::fs::read_to_string(&file_path).unwrap();
        assert_eq!(contents, "a\nx\nb\ny\n");

        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn filesystem_insert_rejects_out_of_range() {
        let dir = make_temp_dir("insert_invalid");
        let file_path = dir.join("file.txt");
        std::fs::write(&file_path, "a\nb\n").unwrap();
        let base = Path::try_from(dir.as_path()).unwrap();

        // insert_line=5 is out of range for a 2-line file
        let err = base.insert("file.txt", 5, "x").await.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);

        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn filesystem_view_range_is_one_based() {
        let dir = make_temp_dir("view_one_based");
        let file_path = dir.join("file.txt");
        std::fs::write(&file_path, "line1\nline2\nline3\nline4\n").unwrap();
        let base = Path::try_from(dir.as_path()).unwrap();

        // view_range=(1, 2) should return lines 1 and 2 (1-based, inclusive)
        let result = base.view("file.txt", Some((1, 2))).await.unwrap();
        assert_eq!(result, "line1\nline2\n");

        // view_range=(2, 3) should return lines 2 and 3
        let result = base.view("file.txt", Some((2, 3))).await.unwrap();
        assert_eq!(result, "line2\nline3\n");

        // view_range=(1, 4) should return all 4 lines
        let result = base.view("file.txt", Some((1, 4))).await.unwrap();
        assert_eq!(result, "line1\nline2\nline3\nline4\n");

        // view_range=(3, 3) should return just line 3
        let result = base.view("file.txt", Some((3, 3))).await.unwrap();
        assert_eq!(result, "line3\n");

        std::fs::remove_dir_all(dir).ok();
    }

    #[tokio::test]
    async fn filesystem_view_range_rejects_zero() {
        let dir = make_temp_dir("view_zero");
        let file_path = dir.join("file.txt");
        std::fs::write(&file_path, "line1\nline2\n").unwrap();
        let base = Path::try_from(dir.as_path()).unwrap();

        // start=0 should error
        let err = base.view("file.txt", Some((0, 2))).await.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);

        // limit=0 should error
        let err = base.view("file.txt", Some((1, 0))).await.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);

        std::fs::remove_dir_all(dir).ok();
    }

    // Permission tests
    #[tokio::test]
    async fn mount_permissions_readonly_allows_search_and_view() {
        let mut hierarchy = MountHierarchy { mounts: vec![] };

        hierarchy
            .mount(
                "/".into(),
                Permissions::ReadOnly,
                MockFileSystem::new_ok("readonly"),
            )
            .unwrap();

        // Search should work
        let result = hierarchy.search("test").await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "search from readonly\n");

        // View should work
        let result = hierarchy.view("/file.txt", None).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "view from readonly");
    }

    #[tokio::test]
    async fn mount_permissions_readonly_denies_write_operations() {
        let mut hierarchy = MountHierarchy { mounts: vec![] };

        hierarchy
            .mount(
                "/".into(),
                Permissions::ReadOnly,
                MockFileSystem::new_ok("readonly"),
            )
            .unwrap();

        // str_replace should fail
        let result = hierarchy.str_replace("/file.txt", "old", "new").await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
        assert!(
            err.to_string()
                .contains("str_replace not allowed with ReadOnly permissions")
        );

        // insert should fail
        let result = hierarchy.insert("/file.txt", 1, "new line").await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
        assert!(
            err.to_string()
                .contains("insert not allowed with ReadOnly permissions")
        );
    }

    #[tokio::test]
    async fn mount_permissions_writeonly_allows_write_operations() {
        let mut hierarchy = MountHierarchy { mounts: vec![] };

        hierarchy
            .mount(
                "/".into(),
                Permissions::WriteOnly,
                MockFileSystem::new_ok("writeonly"),
            )
            .unwrap();

        // str_replace should work
        let result = hierarchy.str_replace("/file.txt", "old", "new").await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "str_replace from writeonly");

        // insert should work
        let result = hierarchy.insert("/file.txt", 1, "new line").await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "insert from writeonly");
    }

    #[tokio::test]
    async fn mount_permissions_writeonly_denies_read_operations() {
        let mut hierarchy = MountHierarchy { mounts: vec![] };

        hierarchy
            .mount(
                "/".into(),
                Permissions::WriteOnly,
                MockFileSystem::new_ok("writeonly"),
            )
            .unwrap();

        // search should fail
        let result = hierarchy.search("test").await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
        assert!(
            err.to_string()
                .contains("search not allowed with WriteOnly permissions")
        );

        // view should fail
        let result = hierarchy.view("/file.txt", None).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
        assert!(
            err.to_string()
                .contains("view not allowed with WriteOnly permissions")
        );
    }

    #[tokio::test]
    async fn mount_permissions_readwrite_allows_all_operations() {
        let mut hierarchy = MountHierarchy { mounts: vec![] };

        hierarchy
            .mount(
                "/".into(),
                Permissions::ReadWrite,
                MockFileSystem::new_ok("readwrite"),
            )
            .unwrap();

        // All operations should work
        let result = hierarchy.search("test").await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "search from readwrite\n");

        let result = hierarchy.view("/file.txt", None).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "view from readwrite");

        let result = hierarchy.str_replace("/file.txt", "old", "new").await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "str_replace from readwrite");

        let result = hierarchy.insert("/file.txt", 1, "new line").await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "insert from readwrite");
    }

    #[tokio::test]
    async fn mount_permissions_different_mounts_different_permissions() {
        let mut hierarchy = MountHierarchy { mounts: vec![] };

        // Mount with different permissions
        hierarchy
            .mount(
                "/".into(),
                Permissions::ReadWrite,
                MockFileSystem::new_ok("root"),
            )
            .unwrap();
        hierarchy
            .mount(
                "/readonly".into(),
                Permissions::ReadOnly,
                MockFileSystem::new_ok("readonly"),
            )
            .unwrap();
        hierarchy
            .mount(
                "/writeonly".into(),
                Permissions::WriteOnly,
                MockFileSystem::new_ok("writeonly"),
            )
            .unwrap();

        // Root mount allows all operations
        let result = hierarchy.str_replace("/file.txt", "old", "new").await;
        assert!(result.is_ok());

        // ReadOnly mount denies write
        let result = hierarchy
            .str_replace("/readonly/file.txt", "old", "new")
            .await;
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied
        );

        // WriteOnly mount denies read
        let result = hierarchy.view("/writeonly/file.txt", None).await;
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied
        );
    }

    // ==== Comprehensive Budget Tests ====

    // Budget Creation Method Tests
    #[test]
    fn budget_new_with_rates_creates_correct_budget() {
        let budget = Budget::new_with_rates(50000, 10, 25, 5, 15);

        assert_eq!(budget.remaining_micro_cents(), 50000);
        assert_eq!(budget.input_token_rate_micro_cents, 10);
        assert_eq!(budget.output_token_rate_micro_cents, 25);
        assert_eq!(budget.cache_creation_token_rate_micro_cents, 5);
        assert_eq!(budget.cache_read_token_rate_micro_cents, 15);
    }

    #[test]
    fn budget_new_flat_rate_sets_all_rates_equal() {
        let budget = Budget::new_flat_rate(10000, 50);

        assert_eq!(budget.remaining_micro_cents(), 10000);
        assert_eq!(budget.input_token_rate_micro_cents, 50);
        assert_eq!(budget.output_token_rate_micro_cents, 50);
        assert_eq!(budget.cache_creation_token_rate_micro_cents, 50);
        assert_eq!(budget.cache_read_token_rate_micro_cents, 50);
    }

    #[test]
    fn budget_from_dollars_with_rates_converts_correctly() {
        let budget = Budget::from_dollars_with_rates(0.5, 100, 200, 75, 150);

        // 0.5 dollars = 50,000,000 micro-cents
        assert_eq!(budget.remaining_micro_cents(), 50_000_000);
        assert_eq!(budget.input_token_rate_micro_cents, 100);
        assert_eq!(budget.output_token_rate_micro_cents, 200);
        assert_eq!(budget.cache_creation_token_rate_micro_cents, 75);
        assert_eq!(budget.cache_read_token_rate_micro_cents, 150);
    }

    #[test]
    fn budget_from_dollars_flat_rate_converts_correctly() {
        let budget = Budget::from_dollars_flat_rate(2.0, 125);

        // 2.0 dollars = 200,000,000 micro-cents
        assert_eq!(budget.remaining_micro_cents(), 200_000_000);
        assert_eq!(budget.input_token_rate_micro_cents, 125);
        assert_eq!(budget.output_token_rate_micro_cents, 125);
        assert_eq!(budget.cache_creation_token_rate_micro_cents, 125);
        assert_eq!(budget.cache_read_token_rate_micro_cents, 125);
    }

    #[test]
    fn budget_creation_edge_cases() {
        // Zero budget
        let zero_budget = Budget::new_flat_rate(0, 10);
        assert_eq!(zero_budget.remaining_micro_cents(), 0);

        // Zero rates
        let zero_rate_budget = Budget::new_with_rates(1000, 0, 0, 0, 0);
        assert_eq!(zero_rate_budget.remaining_micro_cents(), 1000);

        // Very large budget
        let large_budget = Budget::new_flat_rate(u64::MAX, 1);
        assert_eq!(large_budget.remaining_micro_cents(), u64::MAX);

        // Very large rates
        let large_rate_budget = Budget::new_flat_rate(1000, u64::MAX);
        assert_eq!(large_rate_budget.input_token_rate_micro_cents, u64::MAX);
    }

    // Cost Calculation Tests
    #[test]
    fn budget_calculate_cost_all_token_types() {
        let budget = Budget::new_with_rates(100000, 10, 20, 5, 15);

        let usage = Usage::new(100, 50)
            .with_cache_creation_input_tokens(20)
            .with_cache_read_input_tokens(30);

        let expected_cost = (100u64 * 10)
            .checked_add(50u64 * 20)
            .and_then(|sum| sum.checked_add(20u64 * 5))
            .and_then(|sum| sum.checked_add(30u64 * 15))
            .unwrap_or(u64::MAX);
        assert_eq!(budget.calculate_cost(&usage), expected_cost);
    }

    #[test]
    fn budget_calculate_cost_partial_cache_usage() {
        let budget = Budget::new_with_rates(100000, 10, 20, 5, 15);

        // Only cache creation, no cache read
        let usage1 = Usage::new(100, 50).with_cache_creation_input_tokens(20);
        let expected_cost1 = (100u64 * 10)
            .checked_add(50u64 * 20)
            .and_then(|sum| sum.checked_add(20u64 * 5))
            .unwrap_or(u64::MAX);
        assert_eq!(budget.calculate_cost(&usage1), expected_cost1);

        // Only cache read, no cache creation
        let usage2 = Usage::new(100, 50).with_cache_read_input_tokens(30);
        let expected_cost2 = (100u64 * 10)
            .checked_add(50u64 * 20)
            .and_then(|sum| sum.checked_add(30u64 * 15))
            .unwrap_or(u64::MAX);
        assert_eq!(budget.calculate_cost(&usage2), expected_cost2);
    }

    #[test]
    fn budget_calculate_cost_zero_tokens() {
        let budget = Budget::new_with_rates(100000, 10, 20, 5, 15);

        let zero_usage = Usage::new(0, 0);
        assert_eq!(budget.calculate_cost(&zero_usage), 0);

        let partial_zero = Usage::new(100, 0)
            .with_cache_creation_input_tokens(0)
            .with_cache_read_input_tokens(0);
        assert_eq!(budget.calculate_cost(&partial_zero), 100 * 10);
    }

    #[test]
    fn budget_calculate_cost_large_numbers() {
        let budget = Budget::new_with_rates(u64::MAX, 1000, 2000, 500, 1500);

        let large_usage = Usage::new(10000, 5000)
            .with_cache_creation_input_tokens(2000)
            .with_cache_read_input_tokens(3000);

        let expected_cost = (10000_u64 * 1000)
            .checked_add(5000_u64 * 2000)
            .and_then(|sum| sum.checked_add(2000_u64 * 500))
            .and_then(|sum| sum.checked_add(3000_u64 * 1500))
            .unwrap_or(u64::MAX);
        assert_eq!(budget.calculate_cost(&large_usage), expected_cost);
    }

    #[test]
    fn budget_calculate_cost_with_zero_rates() {
        let budget = Budget::new_with_rates(100000, 0, 0, 0, 0);

        let usage = Usage::new(1000, 500)
            .with_cache_creation_input_tokens(200)
            .with_cache_read_input_tokens(300);

        // All costs should be zero due to zero rates
        assert_eq!(budget.calculate_cost(&usage), 0);
    }

    // Budget Allocation Tests
    #[test]
    fn budget_allocate_exact_match() {
        let budget = Budget::new_flat_rate(1000, 10);

        // Allocate exactly what the budget can handle
        let allocation = budget.allocate(100);
        assert!(allocation.is_some());

        let allocation = allocation.unwrap();
        assert_eq!(allocation.remaining_tokens(), 100);
        assert_eq!(budget.remaining_micro_cents(), 0);
    }

    #[test]
    fn budget_allocate_with_different_rates() {
        // Test with different token rates to ensure max rate is used for allocation
        let budget = Budget::new_with_rates(5000, 10, 50, 5, 25); // max rate is 50

        let allocation = budget.allocate(100);
        assert!(allocation.is_some());

        // Should allocate based on highest rate (50)
        assert_eq!(budget.remaining_micro_cents(), 0); // 5000 - (100 * 50) = 0
    }

    #[test]
    fn budget_allocate_zero_tokens() {
        let budget = Budget::new_flat_rate(1000, 10);

        let allocation = budget.allocate(0);
        assert!(allocation.is_some());

        let allocation = allocation.unwrap();
        assert_eq!(allocation.remaining_tokens(), 0);
        assert_eq!(budget.remaining_micro_cents(), 1000); // Nothing allocated
    }

    #[test]
    fn budget_allocate_insufficient_budget_edge_case() {
        let budget = Budget::new_flat_rate(999, 10); // Just 1 micro-cent short

        let allocation = budget.allocate(100); // Needs 1000 micro-cents
        assert!(allocation.is_none());
        assert_eq!(budget.remaining_micro_cents(), 999); // Budget unchanged
    }

    #[test]
    fn budget_allocate_maximum_tokens_calculation() {
        // Allocation uses the output token rate because allocations track output budgets.
        let budget = Budget::new_with_rates(10000, 5, 15, 25, 10); // output rate is 15

        let allocation = budget.allocate(200);
        assert!(allocation.is_some());

        // Should reserve 200 * 15 = 3000 micro-cents (output rate)
        assert_eq!(budget.remaining_micro_cents(), 7000);

        drop(allocation);

        // A flat-rate budget should allocate at the flat rate.
        let budget2 = Budget::new_flat_rate(10000, 25);
        let allocation2 = budget2.allocate(200);
        assert!(allocation2.is_some());
        assert_eq!(budget2.remaining_micro_cents(), 5000);
    }

    // Budget Consumption Tests
    #[test]
    fn budget_consume_usage_within_allocation() {
        let budget = Budget::new_with_rates(10000, 10, 20, 5, 15);
        let mut allocation = budget.allocate(100).unwrap();

        let usage = Usage::new(50, 30)
            .with_cache_creation_input_tokens(10)
            .with_cache_read_input_tokens(20);

        assert!(allocation.consume_usage(&usage));

        // Calculate remaining allocation
        let used_cost = (50u64 * 10)
            .checked_add(30u64 * 20)
            .and_then(|sum| sum.checked_add(10u64 * 5))
            .and_then(|sum| sum.checked_add(20u64 * 15))
            .unwrap_or(u64::MAX);
        let max_rate = 20; // highest rate
        let allocated_cost = 100u64 * max_rate;
        let remaining = allocated_cost.saturating_sub(used_cost);

        assert_eq!(allocation.remaining_micro_cents(), remaining);
    }

    #[test]
    fn budget_consume_usage_exceeding_allocation() {
        let budget = Budget::new_flat_rate(1000, 10);
        let mut allocation = budget.allocate(50).unwrap(); // Allocates 500 micro-cents

        let usage = Usage::new(60, 0); // Would cost 600 micro-cents
        assert!(!allocation.consume_usage(&usage));

        // Allocation should remain unchanged
        assert_eq!(allocation.remaining_micro_cents(), 500);
    }

    #[test]
    fn budget_consume_usage_exact_allocation() {
        let budget = Budget::new_flat_rate(1000, 10);
        let mut allocation = budget.allocate(50).unwrap(); // Allocates 500 micro-cents

        let usage = Usage::new(50, 0); // Costs exactly 500 micro-cents
        assert!(allocation.consume_usage(&usage));

        assert_eq!(allocation.remaining_micro_cents(), 0);
    }

    #[test]
    fn budget_consume_usage_multiple_times() {
        let budget = Budget::new_flat_rate(2000, 10);
        let mut allocation = budget.allocate(100).unwrap(); // Allocates 1000 micro-cents

        // First consumption
        let usage1 = Usage::new(20, 0); // 200 micro-cents
        assert!(allocation.consume_usage(&usage1));
        assert_eq!(allocation.remaining_micro_cents(), 800);

        // Second consumption
        let usage2 = Usage::new(30, 0); // 300 micro-cents
        assert!(allocation.consume_usage(&usage2));
        assert_eq!(allocation.remaining_micro_cents(), 500);

        // Third consumption that would exceed remaining
        let usage3 = Usage::new(60, 0); // 600 micro-cents
        assert!(!allocation.consume_usage(&usage3));
        assert_eq!(allocation.remaining_micro_cents(), 500); // Unchanged
    }

    #[test]
    fn budget_consume_usage_zero_cost() {
        let budget = Budget::new_flat_rate(1000, 10);
        let mut allocation = budget.allocate(50).unwrap();

        let zero_usage = Usage::new(0, 0);
        assert!(allocation.consume_usage(&zero_usage));

        // Allocation should remain unchanged
        assert_eq!(allocation.remaining_micro_cents(), 500);
    }

    #[test]
    fn calculate_input_cost_sums_non_output_tokens() {
        let budget = Budget::new_with_rates(1_000_000, 300, 1500, 375, 30);
        let usage = Usage::new(100, 50)
            .with_cache_creation_input_tokens(20)
            .with_cache_read_input_tokens(10);

        let input_cost = budget.calculate_input_cost(&usage);
        // 100*300 + 20*375 + 10*30 = 30000 + 7500 + 300 = 37800
        assert_eq!(input_cost, 37800);
    }

    #[test]
    fn calculate_output_cost_only_counts_output_tokens() {
        let budget = Budget::new_with_rates(1_000_000, 300, 1500, 375, 30);
        let usage = Usage::new(100, 50)
            .with_cache_creation_input_tokens(20)
            .with_cache_read_input_tokens(10);

        let output_cost = budget.calculate_output_cost(&usage);
        // 50*1500 = 75000
        assert_eq!(output_cost, 75000);
    }

    #[test]
    fn input_plus_output_cost_equals_total_cost() {
        let budget = Budget::new_with_rates(1_000_000, 300, 1500, 375, 30);
        let usage = Usage::new(100, 50)
            .with_cache_creation_input_tokens(20)
            .with_cache_read_input_tokens(10);

        let total = budget.calculate_cost(&usage);
        let split = budget.calculate_input_cost(&usage) + budget.calculate_output_cost(&usage);
        assert_eq!(total, split);
    }

    #[test]
    fn consume_response_charges_output_to_allocation_input_to_budget() {
        // Rates: input=300, output=1500, cache_creation=375, cache_read=30
        let budget = Budget::new_with_rates(500_000_000, 300, 1500, 375, 30);
        let initial_budget = budget.remaining_micro_cents();

        // Allocate for 100 output tokens: 100 * 1500 = 150_000
        let mut allocation = budget.allocate(100).unwrap();
        assert_eq!(budget.remaining_micro_cents(), initial_budget - 150_000);
        assert_eq!(allocation.remaining_tokens(), 100);

        // Simulate an API response with 2000 input + 30 output tokens
        let usage = Usage::new(2000, 30).with_cache_read_input_tokens(500);

        let input_cost = budget.calculate_input_cost(&usage);
        // 2000*300 + 500*30 = 600_000 + 15_000 = 615_000

        assert!(allocation.consume_response(&usage));

        // Allocation should only lose output cost: 30 * 1500 = 45_000
        // Remaining in allocation: 150_000 - 45_000 = 105_000 = 70 tokens
        assert_eq!(allocation.remaining_tokens(), 70);

        // Main budget should have been charged the input cost
        assert_eq!(
            budget.remaining_micro_cents(),
            initial_budget - 150_000 - input_cost
        );
    }

    #[test]
    fn consume_response_charges_input_deficit_to_allocation() {
        let budget = Budget::new_flat_rate(1000, 10);
        let initial_budget = budget.remaining_micro_cents();

        let mut allocation = budget.allocate_available(100).unwrap();
        assert_eq!(budget.remaining_micro_cents(), 0);

        let usage = Usage::new(20, 30);
        assert!(allocation.consume_response(&usage));

        drop(allocation);
        assert_eq!(budget.remaining_micro_cents(), initial_budget - 500);
    }

    #[test]
    fn consume_response_input_does_not_starve_output_budget() {
        // This test demonstrates the fix: large input costs don't drain the output budget.
        let budget = Budget::new_with_rates(500_000_000, 300, 1500, 375, 30);

        let mut allocation = budget.allocate(4096).unwrap();
        // Output allocation: 4096 * 1500 = 6_144_000

        // Step 1: 2000 input, 500 output
        let usage1 = Usage::new(2000, 500);
        assert!(allocation.consume_response(&usage1));
        // Allocation loses 500 * 1500 = 750_000. Remaining output: 3596 tokens.
        assert_eq!(allocation.remaining_tokens(), 3596);

        // Step 2: 5000 input (conversation grew), 400 output
        let usage2 = Usage::new(5000, 400);
        assert!(allocation.consume_response(&usage2));
        // Allocation loses 400 * 1500 = 600_000. Remaining output: 3196 tokens.
        assert_eq!(allocation.remaining_tokens(), 3196);

        // Step 3: 10000 input (big tool results), 300 output
        let usage3 = Usage::new(10000, 300);
        assert!(allocation.consume_response(&usage3));
        // Allocation loses 300 * 1500 = 450_000. Remaining output: 2896 tokens.
        assert_eq!(allocation.remaining_tokens(), 2896);

        // With consume_usage (old behavior), the allocation would have been exhausted
        // by step 2 or 3 due to input costs draining it.
        // With consume_response, the agent still has 2896 output tokens available.
    }

    #[test]
    fn consume_response_returns_false_when_output_exceeds_allocation() {
        let budget = Budget::new_with_rates(500_000_000, 300, 1500, 375, 30);
        let mut allocation = budget.allocate(50).unwrap();
        // Output allocation: 50 * 1500 = 75_000

        // Response with 100 output tokens (cost 150_000 > allocation 75_000)
        let usage = Usage::new(10, 100);
        assert!(!allocation.consume_response(&usage));
    }

    #[test]
    fn remaining_tokens_uses_output_rate() {
        // When output rate differs from highest rate, remaining_tokens should
        // use the output rate since allocation tracks output budgets.
        let budget = Budget::new_with_rates(100_000, 300, 1500, 3750, 30);
        // cache_creation (3750) is the highest rate, output is 1500

        let allocation = budget.allocate(50).unwrap();
        // Allocated: 50 * 1500 = 75_000 (output rate)
        // remaining_tokens = 75_000 / 1500 = 50
        assert_eq!(allocation.remaining_tokens(), 50);
    }

    #[test]
    fn allocate_uses_output_rate_not_highest_rate() {
        let budget = Budget::new_with_rates(100_000, 300, 1500, 3750, 30);
        // Highest rate is 3750 (cache_creation), output rate is 1500

        let allocation = budget.allocate(50).unwrap();
        // Allocated at output rate: 50 * 1500 = 75_000
        assert_eq!(budget.remaining_micro_cents(), 25_000);
        assert_eq!(allocation.remaining_tokens(), 50);
    }

    struct RequestCapAgent {
        seen_max_tokens: Arc<std::sync::Mutex<Option<u32>>>,
    }

    #[async_trait::async_trait]
    impl Agent for RequestCapAgent {
        async fn max_tokens(&self) -> u32 {
            100
        }

        async fn hook_message_create_params(&self, req: &MessageCreateParams) -> Result<(), Error> {
            *self.seen_max_tokens.lock().unwrap() = Some(req.max_tokens);
            Err(Error::bad_request(
                "stop before network",
                Some("max_tokens".to_string()),
            ))
        }
    }

    #[tokio::test]
    async fn default_turn_caps_request_max_tokens_to_available_budget() {
        let seen_max_tokens = Arc::new(std::sync::Mutex::new(None));
        let mut agent = RequestCapAgent {
            seen_max_tokens: Arc::clone(&seen_max_tokens),
        };
        let client = Anthropic::new(None).unwrap();
        let budget = Arc::new(Budget::new_flat_rate(255, 10));
        let mut messages = vec![MessageParam::user("hello")];

        let err = agent
            .take_turn(&client, &mut messages, &budget)
            .await
            .unwrap_err();

        assert!(matches!(err, Error::BadRequest { .. }));
        assert_eq!(*seen_max_tokens.lock().unwrap(), Some(25));
    }

    struct EmptyToolUseAgent;

    #[async_trait::async_trait]
    impl Agent for EmptyToolUseAgent {
        async fn max_tokens(&self) -> u32 {
            100
        }
    }

    /// Spawn an HTTP/1.1 server that replies to up to `max_requests` requests
    /// with `body`, then returns the base URL the client should target.
    ///
    /// Bounding the number of replies means that if the turn loop fails to stop
    /// (the bug), it appends empty user messages on the served iterations rather
    /// than looping forever; the regression assertions then catch the empty
    /// message rather than hanging.
    async fn spawn_canned_response_server(body: String, max_requests: usize) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            for _ in 0..max_requests {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                // Drain the request without fully parsing it; we only need to
                // read enough that the client's write completes.
                let mut buf = [0u8; 4096];
                let _ = socket.read(&mut buf).await;
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = socket.write_all(response.as_bytes()).await;
                let _ = socket.flush().await;
            }
        });
        format!("http://{addr}")
    }

    /// Regression test for the empty tool-result turn bug: when the model
    /// reports `stop_reason = tool_use` but emits no usable `tool_use` blocks
    /// (e.g. it rendered text resembling a tool call), the turn loop must not
    /// append a `{"role":"user","content":[]}` message, which the API rejects.
    #[tokio::test]
    async fn tool_use_stop_with_no_tool_blocks_does_not_append_empty_user_message() {
        // Build a response that has stop_reason = tool_use but only a text block
        // (no structured tool_use block), mirroring the bug's transcript.
        let content = vec![ContentBlock::Text(TextBlock::new(
            "bash\n<invoke name=\"bash\">...".to_string(),
        ))];
        let model = Model::Known(KnownModel::ClaudeSonnet40);
        let response = Message::new("msg_regression".to_string(), content, model, Usage::new(5, 5))
            .with_stop_reason(StopReason::ToolUse);
        let body = serde_json::to_string(&response).unwrap();

        // Serve a few responses so that, absent the fix, the loop appends empty
        // user messages on those iterations (which the assertions below catch)
        // rather than failing on a refused connection.
        let base_url = spawn_canned_response_server(body, 4).await;

        let client = Anthropic::new(Some("test-key".to_string()))
            .unwrap()
            .with_base_url(base_url);
        let mut agent = EmptyToolUseAgent;
        let budget = Arc::new(Budget::new_flat_rate(100_000, 10));
        let mut messages = vec![MessageParam::user("hello")];

        let result = agent.take_turn(&client, &mut messages, &budget).await;

        // No message may have empty array content; that is the invalid entry
        // the API rejects ("user messages must have non-empty content"). Check
        // this regardless of whether the turn returned Ok or Err, since the
        // buggy loop mutates `messages` before any later failure.
        for (i, message) in messages.iter().enumerate() {
            if let MessageParamContent::Array(blocks) = &message.content {
                assert!(
                    !blocks.is_empty(),
                    "message {i} (role {:?}) has empty array content",
                    message.role
                );
            }
        }

        // Specifically, no empty user message was appended after the assistant turn.
        let appended_empty_user = messages.iter().any(|m| {
            m.role == MessageRole::User
                && matches!(&m.content, MessageParamContent::Array(b) if b.is_empty())
        });
        assert!(!appended_empty_user, "an empty user message was appended");

        // With the fix the turn terminates cleanly rather than looping.
        let outcome = result.expect("turn should end cleanly, not error");
        assert_eq!(outcome.stop_reason, StopReason::EndTurn);
        assert_eq!(outcome.request_count, 1);
    }

    // Budget State Management Tests
    #[test]
    fn budget_allocation_drop_behavior() {
        let budget = Budget::new_flat_rate(2000, 10);
        let initial_remaining = budget.remaining_micro_cents();

        {
            let mut allocation = budget.allocate(50).unwrap(); // Allocates 500 micro-cents
            assert_eq!(budget.remaining_micro_cents(), initial_remaining - 500);

            // Consume some of the allocation
            let usage = Usage::new(20, 0); // 200 micro-cents
            assert!(allocation.consume_usage(&usage));
            assert_eq!(allocation.remaining_micro_cents(), 300);

            // When allocation drops, remaining 300 micro-cents should be returned
        }

        // Budget should have the unused portion returned
        assert_eq!(budget.remaining_micro_cents(), initial_remaining - 200);
    }

    #[test]
    fn budget_multiple_allocations_sequential() {
        let budget = Budget::new_flat_rate(3000, 10);

        // First allocation
        {
            let _allocation1 = budget.allocate(100).unwrap(); // 1000 micro-cents
            assert_eq!(budget.remaining_micro_cents(), 2000);
            // _allocation1 drops here, returning 1000 micro-cents
        }

        assert_eq!(budget.remaining_micro_cents(), 3000);

        // Second allocation after first is dropped
        let allocation2 = budget.allocate(150).unwrap(); // 1500 micro-cents
        assert_eq!(budget.remaining_micro_cents(), 1500);

        drop(allocation2);
        assert_eq!(budget.remaining_micro_cents(), 3000);
    }

    #[test]
    fn budget_multiple_allocations_concurrent() {
        let budget = Budget::new_flat_rate(5000, 10);

        let allocation1 = budget.allocate(200).unwrap(); // 2000 micro-cents
        assert_eq!(budget.remaining_micro_cents(), 3000);

        let allocation2 = budget.allocate(150).unwrap(); // 1500 micro-cents
        assert_eq!(budget.remaining_micro_cents(), 1500);

        // Third allocation should fail
        let allocation3 = budget.allocate(200); // Would need 2000 micro-cents
        assert!(allocation3.is_none());
        assert_eq!(budget.remaining_micro_cents(), 1500);

        drop(allocation1);
        assert_eq!(budget.remaining_micro_cents(), 3500); // 1500 + 2000

        drop(allocation2);
        assert_eq!(budget.remaining_micro_cents(), 5000); // Back to original
    }

    #[test]
    fn budget_exhaustion_scenarios() {
        let budget = Budget::new_flat_rate(1000, 10);

        // Exhaust budget completely
        let mut allocation = budget.allocate(100).unwrap();
        assert_eq!(budget.remaining_micro_cents(), 0);

        let usage = Usage::new(100, 0); // Use all 1000 micro-cents
        assert!(allocation.consume_usage(&usage));
        assert_eq!(allocation.remaining_micro_cents(), 0);

        // When dropped, nothing should be returned
        drop(allocation);
        assert_eq!(budget.remaining_micro_cents(), 0);

        // Further allocations should fail
        let failed_allocation = budget.allocate(1);
        assert!(failed_allocation.is_none());
    }

    // Integration and Realistic Usage Tests
    #[test]
    fn budget_realistic_api_usage_pattern() {
        // Simulate realistic Anthropic API costs (approximate rates in micro-cents)
        let budget = Budget::from_dollars_with_rates(
            1.0,  // $1.00 budget
            300,  // ~$0.0003 per input token
            1500, // ~$0.0015 per output token
            150,  // ~$0.00015 per cache creation token
            60,   // ~$0.00006 per cache read token
        );

        // Should have 100,000,000 micro-cents
        assert_eq!(budget.remaining_micro_cents(), 100_000_000);

        let mut allocation = budget.allocate(4000).unwrap(); // Allocate for 4k tokens
        let allocated_cost = 4000 * 1500; // Max rate for allocation
        assert_eq!(budget.remaining_micro_cents(), 100_000_000 - allocated_cost);

        // Simulate a typical API response
        let usage = Usage::new(1000, 500)
            .with_cache_creation_input_tokens(200)
            .with_cache_read_input_tokens(800);

        assert!(allocation.consume_usage(&usage));

        let actual_cost = (1000u64 * 300)
            .saturating_add(500u64 * 1500)
            .saturating_add(200u64 * 150)
            .saturating_add(800u64 * 60);
        let remaining_in_allocation = allocated_cost.saturating_sub(actual_cost);
        assert_eq!(allocation.remaining_micro_cents(), remaining_in_allocation);
    }

    #[test]
    fn budget_multiple_api_calls_simulation() {
        let budget = Budget::from_dollars_flat_rate(1.0, 500); // $1.00 with flat rate

        let mut total_consumed = 0u64;

        // Simulate 5 API calls with smaller allocations
        for call_num in 1..=5 {
            // Allocate enough for the worst-case usage in this call
            let needed_tokens = 20 * call_num + 15 * call_num;
            let mut allocation = budget.allocate(needed_tokens as u32).unwrap();

            let usage = Usage::new(20 * call_num, 15 * call_num);
            assert!(allocation.consume_usage(&usage));

            let call_cost = (20 * call_num as u64)
                .checked_add(15 * call_num as u64)
                .and_then(|sum| sum.checked_mul(500))
                .unwrap_or(u64::MAX); // Already converted
            total_consumed = total_consumed.saturating_add(call_cost);

            // Allocation drops here, returning unused budget
        }

        assert_eq!(budget.remaining_micro_cents(), 100_000_000 - total_consumed);
    }

    #[test]
    fn budget_mixed_token_types_real_scenario() {
        // Test with varied token usage patterns
        let budget = Budget::new_with_rates(50000, 10, 30, 8, 12);

        let scenarios = vec![
            // (input, output, cache_creation, cache_read)
            (100, 50, Some(20), None),     // Cache creation only
            (80, 40, None, Some(30)),      // Cache read only
            (120, 60, Some(15), Some(25)), // Both cache types
            (200, 100, None, None),        // No cache usage
        ];

        let mut remaining_budget = 50000u64;

        for (input, output, cache_creation, cache_read) in scenarios {
            let mut allocation = budget.allocate((input + output) as u32).unwrap();

            let mut usage = Usage::new(input, output);
            if let Some(cc) = cache_creation {
                usage = usage.with_cache_creation_input_tokens(cc);
            }
            if let Some(cr) = cache_read {
                usage = usage.with_cache_read_input_tokens(cr);
            }

            assert!(allocation.consume_usage(&usage));

            let actual_cost = (input as u64)
                .checked_mul(10)
                .and_then(|sum| sum.checked_add((output as u64).checked_mul(30).unwrap_or(0)))
                .and_then(|sum| {
                    sum.checked_add(
                        (cache_creation.unwrap_or(0) as u64)
                            .checked_mul(8)
                            .unwrap_or(0),
                    )
                })
                .and_then(|sum| {
                    sum.checked_add(
                        (cache_read.unwrap_or(0) as u64)
                            .checked_mul(12)
                            .unwrap_or(0),
                    )
                })
                .unwrap_or(u64::MAX);

            remaining_budget = remaining_budget.saturating_sub(actual_cost);
        }

        assert_eq!(budget.remaining_micro_cents(), remaining_budget);
    }

    // Thread Safety Tests
    #[test]
    fn budget_concurrent_allocation_stress_test() {
        use std::sync::{Barrier, Mutex};
        use std::thread;

        let budget = Budget::new_flat_rate(10000, 10);
        let barrier = Barrier::new(20);
        let allocations = Mutex::new(Vec::new());

        thread::scope(|s| {
            // Spawn 20 threads trying to allocate 100 tokens each (1000 micro-cents each)
            // Only 10 should succeed (10000 / 1000 = 10)
            for _ in 0..20 {
                s.spawn(|| {
                    barrier.wait();
                    if let Some(allocation) = budget.allocate(100) {
                        allocations.lock().unwrap().push(allocation);
                    }
                });
            }
        });

        let final_allocations = allocations.into_inner().unwrap();
        let successful_count = final_allocations.len();

        // At most 10 allocations should succeed (10000 / 1000 = 10)
        // Due to concurrent nature, we might get fewer but not more
        assert!(
            successful_count <= 10,
            "Got {} successful allocations, expected at most 10",
            successful_count
        );

        // Drop allocations and verify budget is returned
        drop(final_allocations);
        assert_eq!(budget.remaining_micro_cents(), 10000);
    }

    #[test]
    fn budget_concurrent_mixed_operations() {
        use std::sync::{Barrier, Mutex};
        use std::thread;

        let budget = Budget::new_flat_rate(5000, 25);
        let barrier = Barrier::new(5);
        let allocations = Mutex::new(Vec::new());

        thread::scope(|s| {
            // Spawn threads for different allocation sizes
            s.spawn(|| {
                barrier.wait();
                if let Some(allocation) = budget.allocate(50) {
                    allocations.lock().unwrap().push(allocation);
                }
            });
            s.spawn(|| {
                barrier.wait();
                if let Some(allocation) = budget.allocate(75) {
                    allocations.lock().unwrap().push(allocation);
                }
            });
            s.spawn(|| {
                barrier.wait();
                if let Some(allocation) = budget.allocate(100) {
                    allocations.lock().unwrap().push(allocation);
                }
            });
            s.spawn(|| {
                barrier.wait();
                if let Some(allocation) = budget.allocate(25) {
                    allocations.lock().unwrap().push(allocation);
                }
            });
            s.spawn(|| {
                barrier.wait();
                if let Some(allocation) = budget.allocate(150) {
                    allocations.lock().unwrap().push(allocation);
                }
            });
        });

        let final_allocations = allocations.into_inner().unwrap();
        let successful_allocations = final_allocations.len();

        // Not all allocations should succeed since total requested exceeds budget
        // Budget capacity: 5000 micro-cents / 25 = 200 tokens max
        // Some subset of the allocations should succeed, but not all
        assert!(
            successful_allocations < 5,
            "Expected some allocation failures, but {} out of 5 succeeded",
            successful_allocations
        );

        // Drop allocations and verify budget is returned
        drop(final_allocations);
        assert_eq!(budget.remaining_micro_cents(), 5000);
    }

    // Edge Case and Error Condition Tests
    #[test]
    fn budget_remaining_tokens_calculation_edge_cases() {
        // All rates are the same
        let budget1 = Budget::new_flat_rate(1000, 20);
        let allocation1 = budget1.allocate(50).unwrap();
        assert_eq!(allocation1.remaining_tokens(), 50);

        // Different rates - should use the highest
        let budget2 = Budget::new_with_rates(2000, 10, 50, 20, 30);
        let allocation2 = budget2.allocate(40).unwrap(); // Allocated at highest rate (50)
        assert_eq!(allocation2.remaining_tokens(), 40);

        // Zero highest rate
        let budget3 = Budget::new_with_rates(1000, 0, 0, 0, 0);
        let allocation3 = budget3.allocate(100).unwrap();
        assert_eq!(allocation3.remaining_tokens(), 0); // Division by zero protection
    }

    #[test]
    fn budget_allocation_with_partial_consumption_patterns() {
        let budget = Budget::new_flat_rate(10000, 50);
        let mut allocation = budget.allocate(100).unwrap(); // 5000 micro-cents allocated

        // Consume in small increments
        for i in 1..=10 {
            let usage = Usage::new(i * 2, 0); // Increasing usage - safe for small values
            let expected_usage_cost = ((i * 2) as u64).saturating_mul(50);
            let expected_success = allocation.remaining_micro_cents() >= expected_usage_cost;
            assert_eq!(allocation.consume_usage(&usage), expected_success);
        }

        // Should have consumed: 2+4+6+8+10+12+14+16+18+20 = 110 tokens = 5500 micro-cents
        // But we only allocated 5000, so some consumptions should have failed
        assert!(allocation.remaining_micro_cents() < 5000);
    }

    #[test]
    fn budget_extreme_values_handling() {
        // Test with extreme values to ensure no overflow/underflow
        let large_budget = Budget::new_flat_rate(u64::MAX - 1000, u32::MAX as u64);

        // Should be able to allocate small amount
        let allocation = large_budget.allocate(1);
        assert!(allocation.is_some());

        // Test with very small budget and large rates
        let small_budget = Budget::new_flat_rate(1, u64::MAX);
        let no_allocation = small_budget.allocate(1); // Would overflow
        assert!(no_allocation.is_none());
    }

    #[test]
    fn budget_legacy_compatibility_behavior() {
        #![allow(deprecated)]

        // Test that legacy methods still work
        let budget = Budget::new(100); // Legacy constructor
        assert_eq!(budget.remaining_micro_cents(), 100000); // 100 * 1000

        // Test legacy remaining() method
        let remaining_arc = budget.remaining();
        assert_eq!(remaining_arc.load(Ordering::Relaxed), 100000);

        // Test legacy allocation methods
        let mut allocation = budget.allocate(50).unwrap();
        assert_eq!(allocation.allocated(), 50); // Legacy method

        // Test legacy consume method
        assert!(allocation.consume(25)); // Legacy method
        assert_eq!(allocation.remaining_tokens(), 25);
    }

    #[test]
    fn budget_from_dollars_with_model_uses_correct_rates() {
        use crate::KnownModel;

        let budget = Budget::from_dollars_with_model(1.0, KnownModel::ClaudeSonnet45);
        assert_eq!(budget.total_micro_cents(), 100_000_000); // $1.00

        // Verify the rates are applied correctly by checking cost calculations
        let usage = crate::Usage::new(1_000_000, 0); // 1M input tokens
        let cost = budget.calculate_cost(&usage);
        // 1M tokens × 300 micro-cents/token = 300M micro-cents = $3.00
        assert_eq!(cost, 300_000_000);

        let usage = crate::Usage::new(0, 1_000_000); // 1M output tokens
        let cost = budget.calculate_cost(&usage);
        // 1M tokens × 1500 micro-cents/token = 1.5B micro-cents = $15.00
        assert_eq!(cost, 1_500_000_000);
    }

    #[test]
    fn budget_from_model_uses_correct_rates() {
        use crate::KnownModel;

        let budget = Budget::from_model(500_000_000, KnownModel::ClaudeOpus45);
        assert_eq!(budget.total_micro_cents(), 500_000_000);

        // Opus 4.5 rates: input=500, output=2500
        let usage = crate::Usage::new(1_000_000, 0);
        let cost = budget.calculate_cost(&usage);
        assert_eq!(cost, 500_000_000); // $5/MTok

        let usage = crate::Usage::new(0, 1_000_000);
        let cost = budget.calculate_cost(&usage);
        assert_eq!(cost, 2_500_000_000); // $25/MTok
    }

    #[test]
    fn budget_from_dollars_with_model_haiku() {
        use crate::KnownModel;

        let budget = Budget::from_dollars_with_model(10.0, KnownModel::ClaudeHaiku45);
        assert_eq!(budget.total_micro_cents(), 1_000_000_000); // $10.00

        // Haiku 4.5 rates: input=100, output=500, cache_creation=125, cache_read=10
        let usage = crate::Usage::new(1000, 500)
            .with_cache_creation_input_tokens(200)
            .with_cache_read_input_tokens(100);
        let cost = budget.calculate_cost(&usage);
        // (1000 × 100) + (500 × 500) + (200 × 125) + (100 × 10) = 100000 + 250000 + 25000 + 1000
        assert_eq!(cost, 376_000);
    }

    #[tokio::test]
    async fn insert_at_line_zero_prepends_to_file() {
        let temp_dir = make_temp_dir("insert_zero");
        let file_path = temp_dir.join("test.txt");
        std::fs::write(&file_path, "line1\nline2\nline3\n").unwrap();

        let path = Path::try_from(temp_dir.clone()).unwrap();
        let result = path.insert("test.txt", 0, "prepended").await;
        assert!(
            result.is_ok(),
            "insert at line 0 should succeed: {result:?}"
        );

        let content = std::fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "prepended\nline1\nline2\nline3\n");
        std::fs::remove_dir_all(temp_dir).unwrap();
    }

    #[tokio::test]
    async fn insert_at_line_one_inserts_after_first_line() {
        let temp_dir = make_temp_dir("insert_one");
        let file_path = temp_dir.join("test.txt");
        std::fs::write(&file_path, "line1\nline2\nline3\n").unwrap();

        let path = Path::try_from(temp_dir.clone()).unwrap();
        let result = path.insert("test.txt", 1, "inserted").await;
        assert!(
            result.is_ok(),
            "insert at line 1 should succeed: {result:?}"
        );

        let content = std::fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "line1\ninserted\nline2\nline3\n");
        std::fs::remove_dir_all(temp_dir).unwrap();
    }

    #[tokio::test]
    async fn str_replace_without_new_str_deletes_old_str() {
        let temp_dir = make_temp_dir("str_replace_delete");
        let file_path = temp_dir.join("test.txt");
        std::fs::write(&file_path, "hello world\n").unwrap();

        let path = Path::try_from(temp_dir.clone()).unwrap();
        let result = path.str_replace("test.txt", " world", "").await;
        assert!(
            result.is_ok(),
            "str_replace with empty new_str should succeed: {result:?}"
        );

        let content = std::fs::read_to_string(&file_path).unwrap();
        assert_eq!(content, "hello\n");
        std::fs::remove_dir_all(temp_dir).unwrap();
    }
}
