use std::ops::ControlFlow;
use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use claudius::{
    Agent, Anthropic, Budget, Error, IntermediateToolResult, MessageParam, PlainTextRenderer, Tool,
    ToolCallback, ToolParam, ToolResult, ToolResultBlock, ToolUnionParam, ToolUseBlock,
};

/// A custom tool that provides basic mathematical operations.
struct CalculatorTool;

impl<A: Agent> Tool<A> for CalculatorTool {
    fn name(&self) -> String {
        "calculator".to_string()
    }

    fn callback(&self) -> Box<dyn ToolCallback<A>> {
        Box::new(CalculatorCallback)
    }

    fn to_param(&self) -> ToolUnionParam {
        ToolUnionParam::CustomTool(
            ToolParam::new(
                <Self as Tool<A>>::name(self),
                json!({
                    "type": "object",
                    "properties": {
                        "a": { "type": "number", "description": "The first number" },
                        "b": { "type": "number", "description": "The second number" },
                        "operation": {
                            "type": "string",
                            "enum": ["add", "subtract", "multiply", "divide"],
                            "description": "The operation to perform"
                        }
                    },
                    "required": ["a", "b", "operation"]
                }),
            )
            .with_description("Perform basic mathematical operations.".to_string()),
        )
    }
}

struct CalculatorCallback;

#[async_trait]
impl<A: Agent> ToolCallback<A> for CalculatorCallback {
    async fn compute_tool_result(
        &self,
        _client: &Anthropic,
        _agent: &A,
        tool_use: &ToolUseBlock,
    ) -> Box<dyn IntermediateToolResult> {
        #[derive(Deserialize)]
        struct Input {
            a: f64,
            b: f64,
            operation: String,
        }

        let result = match serde_json::from_value::<Input>(tool_use.input.clone()) {
            Ok(input) => {
                let res = match input.operation.as_str() {
                    "add" => input.a + input.b,
                    "subtract" => input.a - input.b,
                    "multiply" => input.a * input.b,
                    "divide" => {
                        if input.b == 0.0 {
                            return Box::new(ControlFlow::Continue(Err(ToolResultBlock::new(
                                tool_use.id.clone(),
                            )
                            .with_string_content("Error: Division by zero".to_string())
                            .with_error(true))));
                        }
                        input.a / input.b
                    }
                    _ => unreachable!(),
                };
                Ok(ToolResultBlock::new(tool_use.id.clone())
                    .with_string_content(format!("The result is {}", res)))
            }
            Err(err) => Err(ToolResultBlock::new(tool_use.id.clone())
                .with_string_content(format!("Invalid input: {}", err))
                .with_error(true)),
        };

        Box::new(ControlFlow::Continue(result))
    }

    async fn apply_tool_result(
        &self,
        _client: &Anthropic,
        _agent: &mut A,
        _tool_use: &ToolUseBlock,
        intermediate: Box<dyn IntermediateToolResult>,
    ) -> ToolResult {
        // We know the intermediate result is a ToolResult because we returned it in compute_tool_result
        if let Some(res) = intermediate.as_any().downcast_ref::<ToolResult>() {
            res.clone()
        } else {
            ControlFlow::Break(Error::unknown("Failed to downcast intermediate result"))
        }
    }
}

/// A simple agent that uses the CalculatorTool.
struct MathAgent;

#[async_trait]
impl Agent for MathAgent {
    async fn tools(&self) -> Vec<Arc<dyn Tool<Self>>> {
        vec![Arc::new(CalculatorTool)]
    }

    fn stream_label(&self) -> String {
        "MathBot".to_string()
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize the Anthropic client from environment variable ANTHROPIC_API_KEY
    let client = Anthropic::new(None)?;

    // Create a budget of $1.00 with realistic token rates
    let budget = Arc::new(Budget::from_dollars_with_rates(
        1.0,  // $1.00
        300,  // 300 micro-cents per input token
        1500, // 1500 micro-cents per output token
        375,  // 375 micro-cents per cache creation token
        30,   // 30 micro-cents per cache read token
    ));

    let mut agent = MathAgent;
    let mut messages = vec![MessageParam::user(
        "Can you multiply 1234.56 by 789.01 and then divide the result by 2?",
    )];

    // Use PlainTextRenderer for streaming output to console
    let mut renderer = PlainTextRenderer::new();

    println!(
        "--- Starting Agent Streaming Turn ---
"
    );

    let outcome = agent
        .take_turn_streaming_root(&client, &mut messages, &budget, &mut renderer)
        .await?;

    println!("\n\n--- Turn Complete ---");
    println!("Stop Reason: {:?}", outcome.stop_reason);
    println!("Total Usage: {:?}", outcome.usage);
    println!("Total Requests: {}", outcome.request_count);

    Ok(())
}
