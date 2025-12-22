//! Chat with tools using LLM streaming combinators.
//!
//! This example demonstrates how to build a tool-using agent that:
//! - Defines custom tools for the model to use
//! - Uses `unfold_with_tools` to handle the tool-use loop automatically
//! - Streams responses token by token while handling tool calls
//!
//! This follows the same pattern as `chat-with-docs`, but with tool support added.
//!
//! # Usage
//!
//! ```bash
//! cargo run --bin chat-with-tools
//! ```
//!
//! # Available Tools
//!
//! - `get_weather`: Get the current weather for a location
//! - `calculator`: Perform basic arithmetic operations

use std::io::Write;

use claudius::{
    push_or_merge_message, ContentBlock, Error, Message, MessageCreateTemplate, MessageParam,
    MessageStreamEvent, ToolParam, ToolUseBlock,
};
use futures::stream::StreamExt;
use serde_json::json;

use claudius::combinators::{client, debug_stream, read_user_input, unfold_with_tools, VecContext};
use claudius::{impl_from_vec_context, impl_simple_context};

/// The state maintained across agent turns.
#[derive(Clone)]
struct ChatState {
    /// The conversation thread.
    thread: VecContext,
    /// Whether the user has requested to quit.
    should_quit: bool,
}

impl_simple_context!(ChatState, thread);
impl_from_vec_context!(ChatState { should_quit: false });

/// Define the tools available to the model.
fn get_tools() -> Vec<claudius::ToolUnionParam> {
    vec![
        claudius::ToolUnionParam::CustomTool(ToolParam {
            name: "get_weather".to_string(),
            description: Some(
                "Get the current weather for a location. Returns temperature and conditions."
                    .to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "location": {
                        "type": "string",
                        "description": "The city and state, e.g. San Francisco, CA"
                    }
                },
                "required": ["location"]
            }),
            cache_control: None,
        }),
        claudius::ToolUnionParam::CustomTool(ToolParam {
            name: "calculator".to_string(),
            description: Some(
                "Perform basic arithmetic operations. Supports add, subtract, multiply, divide."
                    .to_string(),
            ),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "operation": {
                        "type": "string",
                        "enum": ["add", "subtract", "multiply", "divide"],
                        "description": "The arithmetic operation to perform"
                    },
                    "a": {
                        "type": "number",
                        "description": "The first operand"
                    },
                    "b": {
                        "type": "number",
                        "description": "The second operand"
                    }
                },
                "required": ["operation", "a", "b"]
            }),
            cache_control: None,
        }),
    ]
}

/// Execute a tool and return the result.
fn execute_tool(tool_use: &ToolUseBlock) -> Result<String, String> {
    match tool_use.name.as_str() {
        "get_weather" => {
            let location = tool_use.input["location"]
                .as_str()
                .unwrap_or("Unknown location");
            Ok(format!(
                "Weather in {}: Sunny, 72F (22C), Humidity: 45%, Wind: 8 mph NW",
                location
            ))
        }
        "calculator" => {
            let op = tool_use.input["operation"]
                .as_str()
                .ok_or("Missing operation")?;
            let a = tool_use.input["a"]
                .as_f64()
                .ok_or("Missing or invalid operand 'a'")?;
            let b = tool_use.input["b"]
                .as_f64()
                .ok_or("Missing or invalid operand 'b'")?;

            let result = match op {
                "add" => a + b,
                "subtract" => a - b,
                "multiply" => a * b,
                "divide" => {
                    if b == 0.0 {
                        return Err("Division by zero".to_string());
                    }
                    a / b
                }
                _ => return Err(format!("Unknown operation: {}", op)),
            };

            Ok(format!("{}", result))
        }
        _ => Err(format!("Unknown tool: {}", tool_use.name)),
    }
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    println!("Chat With Tools");
    println!("================");
    println!();
    println!("Available tools:");
    println!("  - get_weather: Get weather for a location");
    println!("  - calculator: Perform arithmetic (add, subtract, multiply, divide)");
    println!();
    println!("Type 'quit' to exit.");

    let template = MessageCreateTemplate {
        stream: Some(true),
        tools: Some(get_tools()),
        ..Default::default()
    };

    let initial_state = ChatState {
        thread: VecContext(vec![]),
        should_quit: false,
    };

    type UpdateFn = Box<dyn FnOnce(Message) -> ChatState + Send>;
    let agent = unfold_with_tools(
        initial_state,
        // step_fn: called for user turns only - reads input, updates context
        move |mut state: ChatState| async move {
            let user_input = match read_user_input() {
                Some(input) => input,
                None => {
                    state.should_quit = true;
                    let state_for_update = state.clone();
                    let update_fn: UpdateFn = Box::new(move |_msg: Message| state_for_update);
                    return Ok((state, update_fn));
                }
            };
            push_or_merge_message(&mut state.thread.0, MessageParam::user(&user_input));
            let state_for_update = state.clone();
            let update_fn: UpdateFn = Box::new(move |msg: Message| {
                let mut state = state_for_update;
                push_or_merge_message(&mut state.thread.0, msg.into());
                state
            });
            Ok((state, update_fn))
        },
        // make_stream: creates API call from context (wrapped with debug_stream)
        debug_stream("API Request", {
            let template = template.clone();
            move |ctx: &ChatState| {
                let template = template.clone();
                let ctx = ctx.clone();
                async move { client::<VecContext>(None)(template, ctx.thread).await }
            }
        }),
        execute_tool,
        |state| state.should_quit,
    );

    // Drive the agent stream - same pattern as chat-with-docs
    futures::pin_mut!(agent);
    while let Some(result) = agent.next().await {
        let mut stream = result?;
        let mut first = true;
        let mut tool_count = 0;
        while let Some(event) = stream.next().await {
            match event {
                Err(e) => {
                    eprintln!("\nError: {}", e);
                    break;
                }
                Ok(MessageStreamEvent::ContentBlockDelta(delta)) => {
                    if let claudius::ContentBlockDelta::TextDelta(text) = delta.delta {
                        if first {
                            print!("\nClaude: ");
                        }
                        first = false;
                        print!("{}", text.text);
                        std::io::stdout().flush().ok();
                    }
                }
                Ok(MessageStreamEvent::ContentBlockStart(start)) => {
                    if let ContentBlock::ToolUse(tool_use) = &start.content_block {
                        tool_count += 1;
                        if tool_count == 1 {
                            if first {
                                print!("\nClaude: ");
                            }
                            print!("[Using tools: ");
                        } else {
                            print!(", ");
                        }
                        print!("{}", tool_use.name);
                        std::io::stdout().flush().ok();
                        first = false;
                    }
                }
                Ok(MessageStreamEvent::MessageDelta(delta)) => {
                    if let Some(claudius::StopReason::ToolUse) = delta.delta.stop_reason {
                        println!("]");
                    }
                }
                Ok(_) => {}
            }
        }
        if !first && tool_count == 0 {
            println!();
        }
    }

    println!("\nGoodbye!");
    Ok(())
}
