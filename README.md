# Claudius

![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)
![Version](https://img.shields.io/badge/version-0.2.0-green.svg)

Claudius is a Rust SDK for the Anthropic API, providing a clean, idiomatic interface to interact
with Claude, Anthropic's powerful AI assistant. This library enables seamless integration with all
of Claude's capabilities, including chat completions, streaming responses, and advanced features.

## Quick Example

```rust
use claudius::{
    Anthropic, ContentBlock, KnownModel, MessageCreateParams, 
    MessageParam, MessageRole, Model, TextBlock,
};
use tokio;

#[tokio::main]
async fn main() -> claudius::Result<()> {
    // Initialize the client (uses CLAUDIUS_API_KEY environment variable)
    let client = Anthropic::new(None)?;

    // Create a message from the user
    let message = MessageParam::new_with_string(
        "Explain the significance of the name 'Claudius' in Roman history.",
        MessageRole::User,
    );

    // Set up request parameters
    let params = MessageCreateParams::new(
        1000, // max tokens
        vec![message],
        Model::Known(KnownModel::Claude37SonnetLatest),
    )
    .with_system_string("You are Claude, an AI assistant made by Anthropic.".to_string());

    // Send the request
    let response = client.send(params).await?;

    // Process the response
    if let Some(content) = response.content.first() {
        match content {
            ContentBlock::Text(TextBlock { text, .. }) => {
                println!("Claude's response: {}", text);
            }
            _ => println!("Received non-text content block"),
        }
    }

    Ok(())
}
```

## Features

- **Complete API Coverage**: Access all of Anthropic's AI capabilities through Claude
- **Streaming Support**: Real-time streaming for conversational applications
- **Strongly Typed**: Take advantage of Rust's type system for predictable API interactions
- **Async First**: Built with async/await for efficient I/O operations
- **Error Handling**: Comprehensive error types for robust application development
- **Extensible**: Modular design with builder patterns for flexible configuration

## Installation

Add Claudius to your `Cargo.toml`:

```toml
[dependencies]
claudius = "0.2.0"
```

## Authentication

Claudius uses the Anthropic API key for authentication. You can provide it in two ways:

1. Set the `ANTHROPIC_API_KEY` environment variable:

```bash
export ANTHROPIC_API_KEY="your-api-key"
```

2. Provide it directly when creating the client:

```rust
let client = Anthropic::new(Some("your-api-key".to_string()))?;
```

## Usage

### Basic Chat

```rust
// Create a client
let client = Anthropic::new(None)?; // Uses CLAUDIUS_API_KEY env var

// Create a user message
let message = MessageParam::new_with_string(
    "What are three interesting facts about rust programming language?",
    MessageRole::User,
);

// Set up request parameters
let params = MessageCreateParams::new(
    1000, // max_tokens
    vec![message],
    Model::Known(KnownModel::Claude37SonnetLatest),
)
.with_system_string("Be concise and informative.".to_string());

// Send the request and get the response
let response = client.send(params).await?;

// Process the response
for content in response.content {
    match content {
        ContentBlock::Text(text_block) => {
            println!("{}", text_block.text);
        }
        _ => println!("Received non-text content block"),
    }
}
```

### Streaming Responses

```rust
// Create streaming request parameters
let params = MessageCreateParams::new_streaming(
    1000, // max_tokens
    vec![message],
    Model::Known(KnownModel::Claude37SonnetLatest),
);

// Get a stream of events
let stream = client.stream(params).await?;

// Pin the stream so it can be polled
use futures::StreamExt;
use tokio::pin;
pin!(stream);

// Process the stream events
while let Some(event) = stream.next().await {
    match event {
        Ok(event) => {
            // Handle different event types
            match event {
                MessageStreamEvent::ContentBlockDelta(delta) => {
                    // Process incremental text updates
                    // ...
                }
                // Handle other event types
                _ => {}
            }
        }
        Err(e) => {
            eprintln!("Error: {}", e);
        }
    }
}
```

### Model Selection

Claudius supports all Anthropic models via a typed enum:

```rust
// Use a known model (latest version)
let model = Model::Known(KnownModel::Claude37SonnetLatest);

// Use a specific model version
let model = Model::Known(KnownModel::Claude37Sonnet20250219);

// Use a custom model identifier
let model = Model::Custom("custom-model-identifier".to_string());
```

### Advanced Configuration

```rust
// Customize the client
let client = Anthropic::new(Some("your-api-key".to_string()))?
    .with_base_url("https://custom-api.example.com/".to_string())
    .with_timeout(std::time::Duration::from_secs(60));

// Configure request parameters
let params = MessageCreateParams::new(
    1000, // max_tokens
    vec![message],
    model,
)
.with_system_string("You are Claude, an AI assistant...".to_string())
.with_temperature(0.7)
.with_top_p(0.9)
.with_top_k(40)
.with_stop_sequences(vec!["END".to_string()]);
```

## Error Handling

Claudius provides a robust error system:

```rust
match client.send(params).await {
    Ok(response) => {
        // Process successful response
    }
    Err(err) => {
        if err.is_authentication() {
            // Handle authentication error
        } else if err.is_rate_limit() {
            // Handle rate limiting
            let retry_after = match &err {
                Error::RateLimit { retry_after, .. } => retry_after,
                _ => None,
            };
            // Implement backoff strategy
        } else if err.is_todo() {
            // Handle unimplemented functionality
        } else {
            // Handle other errors
            eprintln!("Error: {}", err);
        }
    }
}
```

## Examples

The repository includes several examples:

- `basic_chat.rs`: Simple request and response with Claude
- `streaming.rs`: Streaming response handling

Run examples with:

```bash
# Set your API key
export CLAUDIUS_API_KEY="your-api-key"

# Run the basic chat example
cargo run --example basic_chat

# Run the streaming example
cargo run --example streaming
```

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## License

This project is licensed under the Apache License 2.0 - see the LICENSE file for details.

## Acknowledgments

- Thanks to Anthropic for creating Claude and providing the API that powers this SDK
- This project is not officially affiliated with Anthropic
