use claudius::{Anthropic, KnownModel, MessageCreateParams, MessageParam, MessageRole, Model};
use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Create a client with custom retry settings
    let client = Anthropic::new(None)?
        .with_max_retries(5) // Try up to 5 times
        .with_backoff_params(1.0 / 60.0, 1.0 / 60.0) // Conservative backoff for 1 req/min capacity
        .with_timeout(Duration::from_secs(30))?;

    println!("Client configured with retry settings:");
    println!("- Max retries: 5");
    println!("- Backoff parameters: 1/60 ops/sec throughput, 1/60 reserve capacity");
    println!("- Timeout: 30 seconds");
    println!();

    let message =
        MessageParam::new_with_string("Hello, how are you?".to_string(), MessageRole::User);

    let params = MessageCreateParams::new(
        1000, // max tokens
        vec![message],
        Model::Known(KnownModel::Claude37SonnetLatest),
    );

    println!("Sending message with automatic retry on failures...");
    match client.send(params).await {
        Ok(message) => {
            println!("Success! Message received:");
            for content in &message.content {
                if let Some(text_block) = content.as_text() {
                    println!("{}", text_block.text);
                }
            }
        }
        Err(e) => {
            println!("Failed after all retries: {e}");
            if e.is_rate_limit() {
                println!(
                    "This was a rate limit error - the client would have automatically retried with backoff"
                );
            } else if e.is_retryable() {
                println!(
                    "This was a retryable error - the client would have automatically retried"
                );
            } else {
                println!("This was a non-retryable error - no retry was attempted");
            }
        }
    }

    Ok(())
}
