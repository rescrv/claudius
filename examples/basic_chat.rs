use claudius::{
    Anthropic, ContentBlock, KnownModel, MessageCreateParams, MessageParam, MessageRole, Model,
    Result, TextBlock,
};
use futures::StreamExt;
use tokio::pin;

#[tokio::main]
async fn main() -> Result<()> {
    // Create a client using the API key from the environment variable CLAUDIUS_API_KEY
    let client = Anthropic::new(None)?;

    // Example 1: Non-streaming request (old verbose way)
    println!("EXAMPLE 1: Non-streaming request (old verbose way)");
    println!("--------------------------------------------------");

    let message = MessageParam::new_with_string(
        "Hello, I'm a human. Can you tell me about yourself?".to_string(),
        MessageRole::User,
    );
    let system_prompt = "You are Claude, an AI assistant made by Anthropic.".to_string();
    let params = MessageCreateParams::new(
        1000, // max tokens
        vec![message.clone()],
        Model::Known(KnownModel::Claude37SonnetLatest),
    )
    .with_system_string(system_prompt.clone());

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

    // Example 2: New ergonomic API - much simpler!
    println!("EXAMPLE 2: New ergonomic API - much simpler!");
    println!("---------------------------------------------");

    let params = MessageCreateParams::simple(
        "Hello, I'm a human. Can you tell me about yourself?",
        KnownModel::Claude37SonnetLatest,
    )
    .with_system("You are Claude, an AI assistant made by Anthropic.");

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

    // Example 3: Streaming request with new API
    println!("EXAMPLE 3: Streaming request with new API");
    println!("-----------------------------------------");

    let params = MessageCreateParams::simple_streaming(
        "Tell me a short joke.",
        KnownModel::Claude37SonnetLatest,
    );

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
