use claudius::{
    Anthropic, KnownModel, MessageCreateParams, MessageParam, MessageRole, Model, Result,
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

    // Set up common message parameters
    let system_prompt = "You are Claude, an AI assistant made by Anthropic.".to_string();

    let params = MessageCreateParams::new_streaming(
        1000, // max tokens
        vec![message],
        Model::Known(KnownModel::Claude37SonnetLatest),
    )
    .with_system_string(system_prompt);

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
