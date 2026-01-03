use std::collections::HashSet;
use std::io::{self, Write};

use claudius::{
    AccumulatingStream, Anthropic, ContentBlock, ContentBlockDelta, Error, KnownModel, Message,
    MessageCreateParams, MessageParam, MessageRole, MessageStreamEvent, Model, Result, StopReason,
    ToolChoice, ToolParam, ToolResultBlock, ToolUnionParam, ToolUseBlock,
};
use futures::StreamExt;
use serde::Deserialize;
use serde_json::json;

#[derive(Debug, Deserialize)]
struct MathAddInput {
    a: i64,
    b: i64,
}

#[derive(Debug, Deserialize)]
struct ShoutInput {
    text: String,
}

fn run_tool(tool_use: &ToolUseBlock) -> ToolResultBlock {
    match tool_use.name.as_str() {
        "math_add" => match serde_json::from_value::<MathAddInput>(tool_use.input.clone()) {
            Ok(input) => ToolResultBlock::new(tool_use.id.clone())
                .with_string_content((input.a + input.b).to_string()),
            Err(err) => ToolResultBlock::new(tool_use.id.clone())
                .with_string_content(format!("invalid input: {err}"))
                .with_error(true),
        },
        "shout" => match serde_json::from_value::<ShoutInput>(tool_use.input.clone()) {
            Ok(input) => ToolResultBlock::new(tool_use.id.clone())
                .with_string_content(input.text.to_uppercase()),
            Err(err) => ToolResultBlock::new(tool_use.id.clone())
                .with_string_content(format!("invalid input: {err}"))
                .with_error(true),
        },
        _ => ToolResultBlock::new(tool_use.id.clone())
            .with_string_content(format!("unknown tool: {}", tool_use.name))
            .with_error(true),
    }
}

async fn stream_message(client: &Anthropic, params: &MessageCreateParams) -> Result<Message> {
    let stream = client.stream(params).await?;
    let (mut acc_stream, rx) = AccumulatingStream::new(stream);
    let mut active_tool_uses = HashSet::new();
    let mut stdout = io::stdout();

    while let Some(event) = acc_stream.next().await {
        match event? {
            MessageStreamEvent::ContentBlockStart(start) => match start.content_block {
                ContentBlock::Text(text) => {
                    if !text.text.is_empty() {
                        print!("{}", text.text);
                        stdout.flush()?;
                    }
                }
                ContentBlock::ToolUse(tool_use) => {
                    active_tool_uses.insert(start.index);
                    println!("\nTool use: {} ({})", tool_use.name, tool_use.id);
                    print!("Input: ");
                    stdout.flush()?;
                }
                _ => {}
            },
            MessageStreamEvent::ContentBlockDelta(delta) => match delta.delta {
                ContentBlockDelta::TextDelta(text_delta) => {
                    print!("{}", text_delta.text);
                    stdout.flush()?;
                }
                ContentBlockDelta::InputJsonDelta(json_delta) => {
                    if active_tool_uses.contains(&delta.index) {
                        print!("{}", json_delta.partial_json);
                        stdout.flush()?;
                    }
                }
                _ => {}
            },
            MessageStreamEvent::ContentBlockStop(stop) => {
                if active_tool_uses.remove(&stop.index) {
                    println!();
                }
            }
            _ => {}
        }
    }

    match rx.await {
        Ok(result) => result,
        Err(_) => Err(Error::streaming(
            "failed to receive accumulated streaming message",
            None,
        )),
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let client = Anthropic::new(None)?;
    let prompt = "Use the math_add tool to add 24 and 18. Then respond with the sum.";

    let tools = vec![
        ToolUnionParam::CustomTool(
            ToolParam::new(
                "math_add".to_string(),
                json!({
                    "type": "object",
                    "properties": {
                        "a": { "type": "integer", "description": "First number" },
                        "b": { "type": "integer", "description": "Second number" }
                    },
                    "required": ["a", "b"]
                }),
            )
            .with_description("Add two integers and return the sum as a string.".to_string()),
        ),
        ToolUnionParam::CustomTool(
            ToolParam::new(
                "shout".to_string(),
                json!({
                    "type": "object",
                    "properties": {
                        "text": { "type": "string", "description": "Text to uppercase" }
                    },
                    "required": ["text"]
                }),
            )
            .with_description("Uppercase the provided text.".to_string()),
        ),
    ];

    let mut messages = vec![MessageParam::user(prompt)];
    let mut turn = 0;

    loop {
        turn += 1;
        println!("\n--- Turn {turn} ---");
        let mut params = MessageCreateParams::new_streaming(
            1024,
            messages.clone(),
            Model::Known(KnownModel::ClaudeHaiku45),
        )
        .with_tools(tools.clone());

        if turn == 1 {
            params = params.with_tool_choice(ToolChoice::tool("math_add"));
        }

        let response = stream_message(&client, &params).await?;
        messages.push(MessageParam::from(response.clone()));

        if response.stop_reason != Some(StopReason::ToolUse) {
            break;
        }

        let tool_uses: Vec<ToolUseBlock> = response
            .content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::ToolUse(tool_use) => Some(tool_use.clone()),
                _ => None,
            })
            .collect();

        if tool_uses.is_empty() {
            break;
        }

        let tool_results = tool_uses
            .iter()
            .map(run_tool)
            .map(ContentBlock::ToolResult)
            .collect();

        messages.push(MessageParam::new_with_blocks(
            tool_results,
            MessageRole::User,
        ));
    }

    Ok(())
}
