use claudius::{
    Anthropic, KnownModel, MessageCreateParams, MessageParam, MessageRole, Model,
    RawContentBlockDelta, RawMessageStreamEvent, Result,
};
use futures::StreamExt;
use tokio::pin;

#[tokio::main]
async fn main() -> Result<()> {
    // Create a client using the API key from the environment variable CLAUDIUS_API_KEY
    let client = Anthropic::new(None)?;

    // Create a message with a simple prompt
    let message = MessageParam::new_with_string(
        "Hello, I'm a human. Please provide a brief introduction about yourself.".to_string(),
        MessageRole::User,
    );

    // Create a streaming request
    let params = MessageCreateParams::new_streaming(
        1000, // max tokens
        vec![message],
        Model::Known(KnownModel::Claude37SonnetLatest),
    )
    .with_system_string("You are Claude, an AI assistant made by Anthropic.".to_string());

    // Use raw streaming to get direct access to the server-sent events
    let stream = client.stream_raw(params).await?;

    // Pin the stream so it can be polled
    pin!(stream);

    println!("Raw streaming response:");

    // Track state to demonstrate how you might process raw events
    let mut current_message_id = None;
    let mut content_blocks = vec![];
    let mut current_text = String::new();

    while let Some(event) = stream.next().await {
        match event {
            Ok(event) => {
                // Process different event types
                match event {
                    RawMessageStreamEvent::MessageStart(start_event) => {
                        println!("=== Message started ===");
                        println!("Message ID: {}", start_event.message.id);
                        println!("Model: {}", start_event.message.model);
                        current_message_id = Some(start_event.message.id);
                    }

                    RawMessageStreamEvent::ContentBlockStart(block_start) => {
                        println!("\n=== Content block {} started ===", block_start.index);
                        // You can examine the content block type here
                        if let Some(text_block) = block_start.content_block.as_text() {
                            println!("Text block started: {}", text_block.text);
                            current_text = text_block.text.clone();
                            content_blocks.push((block_start.index, current_text.clone()));
                        } else {
                            println!("Non-text block started");
                        }
                    }

                    RawMessageStreamEvent::ContentBlockDelta(block_delta) => {
                        // Handle incremental content updates
                        match &block_delta.delta {
                            RawContentBlockDelta::TextDelta(text_delta) => {
                                print!("{}", text_delta.text); // Print without newline for continuous output

                                // Update our tracked text
                                if let Some((_, text)) = content_blocks
                                    .iter_mut()
                                    .find(|(idx, _)| *idx == block_delta.index)
                                {
                                    current_text.push_str(&text_delta.text);
                                    *text = current_text.clone();
                                } else {
                                    current_text = text_delta.text.clone();
                                    content_blocks.push((block_delta.index, current_text.clone()));
                                }
                            }
                            _ => println!("Received non-text delta: {:?}", block_delta.delta),
                        }
                    }

                    RawMessageStreamEvent::ContentBlockStop(block_stop) => {
                        println!("\n=== Content block {} stopped ===", block_stop.index);
                    }

                    RawMessageStreamEvent::MessageDelta(delta) => {
                        if let Some(stop_reason) = &delta.delta.stop_reason {
                            println!("\n=== Message delta: stop_reason={} ===", stop_reason);
                        }

                        // Usage information is not optional in the event
                        println!(
                            "Usage - Input tokens: {:?}, Output tokens: {}",
                            delta.usage.input_tokens, delta.usage.output_tokens
                        );
                    }

                    RawMessageStreamEvent::MessageStop(_) => {
                        println!("\n=== Message stopped ===");

                        // Final summary
                        println!("\nMessage summary:");
                        println!("Message ID: {:?}", current_message_id);
                        println!("Content blocks: {}", content_blocks.len());

                        for (idx, text) in &content_blocks {
                            println!("Block {}: {} chars", idx, text.len());
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                break;
            }
        }
    }

    Ok(())
}
