use claudius::{Anthropic, KnownModel, MessageCreateParams, Result};
use futures::StreamExt;
use tokio::pin;

#[tokio::main]
async fn main() -> Result<()> {
    // Create a client using the API key from the environment variable CLAUDIUS_API_KEY
    let client = Anthropic::new(None)?;

    // Create a streaming request with the new ergonomic API
    let params = MessageCreateParams::simple_streaming(
        "Hello, I'm a human. Can you tell me about yourself?",
        KnownModel::Claude37SonnetLatest,
    )
    .with_system("You are Claude, an AI assistant made by Anthropic.");

    let stream = client.stream(&params).await?;

    // Pin the stream so it can be polled
    pin!(stream);

    println!("Streaming response:");
    while let Some(event) = stream.next().await {
        match event {
            Ok(event) => {
                // In a real application, you would handle different event types appropriately
                println!("Received event: {event:?}");
            }
            Err(e) => {
                eprintln!("Error: {e}");
            }
        }
    }

    Ok(())
}
