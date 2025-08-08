# Claudius

![License](https://img.shields.io/badge/license-Apache--2.0-blue.svg)
![Version](https://img.shields.io/badge/version-0.10.0-green.svg)

Claudius is a comprehensive Rust SDK for the Anthropic API, providing both low-level API access and a 
powerful agent framework for building AI-powered applications. This library enables seamless integration 
with Claude through direct API calls, streaming responses, and high-level agent abstractions with built-in 
tool support for filesystem operations, shell commands, and custom integrations.

## Quick Example

```rust
use claudius::{
    Anthropic, ContentBlock, KnownModel, MessageCreateParams, 
    MessageParam, MessageRole, Model, TextBlock,
};
use tokio;

#[tokio::main]
async fn main() -> claudius::Result<()> {
    // Initialize the client (uses CLAUDIUS_API_KEY or ANTHROPIC_API_KEY environment variable)
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
- **Agent Framework**: High-level abstractions for building AI-powered applications with state management
- **Built-in Tools**: Filesystem operations, shell commands, text editing, and web search capabilities
- **Budget System**: Token allocation and tracking for cost control and resource management
- **Streaming Support**: Real-time streaming for conversational applications
- **Strongly Typed**: Take advantage of Rust's type system for predictable API interactions
- **Async First**: Built with async/await for efficient I/O operations
- **Error Handling**: Comprehensive error types for robust application development
- **Extensible**: Modular design with builder patterns and trait-based tool system

## Installation

Add Claudius to your `Cargo.toml`:

```toml
[dependencies]
claudius = "0.10.0"
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
let client = Anthropic::new(None)?; // Uses CLAUDIUS_API_KEY env var and falls back to ANTHROPIC_API_KEY

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

## Agent Framework

Claudius provides a powerful agent framework that abstracts away message management, tool integration, and resource budgeting. Agents can be customized with filesystem access, shell commands, and custom tools.

### Basic Agent Usage

```rust
use claudius::{Agent, Anthropic, Budget, MessageParam, MessageParamContent, MessageRole};
use std::sync::Arc;

#[tokio::main]
async fn main() -> claudius::Result<()> {
    let client = Anthropic::new(None)?;
    let budget = Arc::new(Budget::new(2048)); // 2048 token budget
    
    // Use the unit type as a basic agent
    let agent = ();
    
    // Initialize conversation
    let mut messages = vec![MessageParam {
        role: MessageRole::User,
        content: MessageParamContent::String("Hello! How can you help me today?".to_string()),
    }];
    
    // Let the agent take a turn
    agent.take_turn(&client, &mut messages, &budget).await?;
    
    // Messages now contain the full conversation history
    println!("Conversation has {} messages", messages.len());
    
    Ok(())
}
```

### Custom Agent with Filesystem

```rust
use claudius::{Agent, FileSystem, Anthropic, Budget};
use utf8path::Path;

struct MyAgent {
    root: Path<'static>,
}

#[async_trait::async_trait]
impl Agent for MyAgent {
    async fn filesystem(&self) -> Option<&dyn FileSystem> {
        Some(&self.root) // Path implements FileSystem
    }
    
    async fn system(&self) -> Option<SystemPrompt> {
        Some(SystemPrompt::from_string(
            "You are a helpful assistant with access to the filesystem.".to_string()
        ))
    }
}

#[tokio::main]
async fn main() -> claudius::Result<()> {
    let agent = MyAgent {
        root: Path::from("./workspace"),
    };
    
    let client = Anthropic::new(None)?;
    let budget = Arc::new(Budget::new(4096));
    
    let mut messages = vec![MessageParam {
        role: MessageRole::User,
        content: MessageParamContent::String(
            "Can you search for any .rs files and show me their contents?".to_string()
        ),
    }];
    
    agent.take_turn(&client, &mut messages, &budget).await?;
    
    Ok(())
}
```

### Budget Management

The budget system provides token allocation and tracking:

```rust
use claudius::Budget;
use std::sync::Arc;

// Create a budget with 1000 tokens
let budget = Arc::new(Budget::new(1000));

// Allocate tokens for a request
if let Some(mut allocation) = budget.allocate(500) {
    // Use tokens as needed
    let consumed = allocation.consume(200); // Returns true if successful
    println!("Consumed 200 tokens: {}", consumed);
    
    // Remaining tokens are automatically returned when allocation is dropped
}

// Check remaining budget (approximately, due to concurrent access)
```

## Built-in Tools

Agents can use built-in tools for common operations:

### Filesystem Operations

```rust
// Through the FileSystem trait, agents can:
agent.search("function").await?;           // Search for text in files
agent.view("src/main.rs", None).await?;    // View file contents
agent.str_replace("config.toml", "old", "new").await?; // Replace text
agent.insert("notes.txt", 5, "New line").await?;       // Insert at line
```

### Text Editor Tool

```rust
use claudius::{ToolTextEditor20250429, Tool};

// The text editor tool provides structured file editing
let editor = ToolTextEditor20250429::new();
// Can be used within agent tool integrations
```

### Bash Tool

```rust
use claudius::{ToolBash20250124, Tool};

// Execute shell commands
let bash_tool = ToolBash20250124::new();
// Integrated into agent workflows
```

## Custom Tools

Create custom tools by implementing the `Tool` trait:

```rust
use claudius::{Tool, ToolUnionParam, ToolResultCallback, Agent};

struct MyCustomTool;

impl<A: Agent> Tool<A> for MyCustomTool {
    fn name(&self) -> String {
        "my_custom_tool".to_string()
    }
    
    fn callback(&self) -> ToolResultCallback<A> {
        Box::new(|tool_use| {
            Box::pin(async move {
                // Your tool implementation here
                // Return a ToolResultApplier
            })
        })
    }
    
    fn to_param(&self) -> ToolUnionParam {
        // Define the tool's parameter schema
    }
}
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
- `agent.rs`: Agent framework with filesystem operations
- `models_example.rs`: Model listing and information retrieval
- `retry_example.rs`: Error handling and retry logic

Run examples with:

```bash
# Set your API key
export CLAUDIUS_API_KEY="your-api-key"

# Run the basic chat example
cargo run --example basic_chat

# Run the streaming example
cargo run --example streaming

# Run the agent framework example
cargo run --example agent

# Run the models example
cargo run --example models_example

# Run the retry example
cargo run --example retry_example
```

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## License

This project is licensed under the Apache License 2.0 - see the LICENSE file for details.

## Acknowledgments

- Thanks to Anthropic for creating Claude and providing the API that powers this SDK
- This project is not officially affiliated with Anthropic
