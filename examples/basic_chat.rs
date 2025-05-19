use claudius::{
    Anthropic, ContentBlock, KnownModel, MessageCreateParams, MessageCreateParamsBase,
    MessageParam, MessageRole, Model, Result, TextBlock,
};
use futures::StreamExt;
use tokio::pin;

#[tokio::main]
async fn main() -> Result<()> {
    // Create a client using the API key from the environment variable CLAUDIUS_API_KEY
    let client = Anthropic::new(None)?;

    // Create a message with a simple prompt
    let message = MessageParam::new_with_string(
        "Hello, I'm a human. Can you tell me about yourself?".to_string(),
        MessageRole::User,
    );

    // Set up the message parameters
    let base_params = MessageCreateParamsBase::new(
        1000, // max tokens
        vec![message],
        Model::Known(KnownModel::Claude37SonnetLatest),
    )
    .with_system_string("You are Claude, an AI assistant made by Anthropic.".to_string());

    // Example 1: Non-streaming request
    println!("EXAMPLE 1: Non-streaming request");
    println!("---------------------------------");

    let params = MessageCreateParams::new_non_streaming(base_params.clone());
    let response = client.send(params).await?;

    println!("Response ID: {}", response.id);
    if let Some(content) = response.content.first() {
        match content {
            ContentBlock::Text(TextBlock { text, .. }) => {
                println!("Response: {}", text);
            }
            _ => println!("Received non-text content block"),
        }
    }
    println!();

    // Example 2: Streaming request
    println!("EXAMPLE 2: Streaming request");
    println!("----------------------------");

    let params = MessageCreateParams::new_streaming(base_params);
    let stream = client.stream(params).await?;

    // Pin the stream so it can be polled
    pin!(stream);

    println!("Streaming response:");
    while let Some(event) = stream.next().await {
        match event {
            Ok(event) => {
                // In a real application, you would handle different event types appropriately
                println!("Received event: {:?}", event);
            }
            Err(e) => {
                eprintln!("Error: {}", e);
            }
        }
    }

    Ok(())
}
