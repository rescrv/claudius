# Claudius Architecture

## Overview

Claudius is a comprehensive Rust client for the Anthropic API, designed with modularity, safety, and extensibility in mind. The library follows Rust best practices and provides both low-level API access and high-level agent abstractions.

## Core Components

### 1. Client Layer (`client.rs`)

The `Anthropic` client is the main entry point for API interactions:

```rust
┌─────────────────┐
│   Anthropic     │
│     Client      │
├─────────────────┤
│ - API calls     │
│ - Retry logic   │
│ - Backoff       │
│ - Streaming     │
└─────────────────┘
```

**Key Features:**
- Configurable retry mechanism with exponential backoff
- Automatic rate limit handling
- Support for both streaming and non-streaming responses
- Request/response validation

### 2. Error Handling (`error.rs`)

Comprehensive error type system with semantic error categories:

```rust
Error
├── Api (HTTP errors)
├── Authentication
├── RateLimit
├── Validation
├── Streaming
└── ... (15+ error types)
```

**Design Principles:**
- All errors are `Clone` and `Send + Sync`
- Semantic helper methods (`is_retryable()`, `is_rate_limit()`)
- Proper error chaining with source tracking

### 3. SSE Processing (`sse.rs`)

Dedicated module for Server-Sent Events parsing:

```
Byte Stream → SSE Parser → MessageStreamEvent
                  ↓
            Buffer Management
                  ↓
            Event Extraction
```

**Features:**
- Handles partial events across chunks
- UTF-8 validation
- Error recovery for malformed events

### 4. Agent Framework (`agent.rs`)

High-level abstraction for building conversational agents:

```rust
┌──────────────────┐
│      Agent       │ (trait)
├──────────────────┤
│ - max_tokens()   │
│ - model()        │
│ - tools()        │
│ - system_prompt()│
│ - thinking_config()│
└──────────────────┘
         ↑
    implements
         │
┌──────────────────┐
│  Custom Agent    │
└──────────────────┘
```

**Components:**
- `Agent` trait: Core agent interface
- `Tool` trait: Tool abstraction with compute/apply phases
- `ToolCallback` trait: Two-phase tool execution
- `FileSystem`: Virtual filesystem for agents
- `Mount` and `MountHierarchy`: File system mounting abstractions

### 5. Type System (`types/`)

Comprehensive type definitions for Anthropic API interactions:

```
types/
├── Core Message Types (message.rs, message_param.rs, content_block.rs)
├── Tool System Types (tool_*.rs variants by date)
├── Streaming Types (message_stream_event.rs, *_delta.rs)
├── Citation System (citation_*.rs, text_citation.rs)  
├── Model Management (model.rs, model_info.rs, model_list_*.rs)
├── Thinking System (thinking_*.rs, redacted_thinking_block.rs)
├── Web Search (web_search_*.rs)
└── Document Handling (document_block.rs, *_source.rs)
```

**Key Features:**
- 65+ type definitions covering all API aspects
- Versioned tool types with date suffixes (e.g., `ToolBash20250124`)
- Support for citations, thinking, and web search capabilities
- Comprehensive streaming event types
- Document and image block handling

### 6. Tool System

#### Core Tool Architecture

```rust
Tool<A: Agent>
├── name() -> String
├── callback() -> ToolCallback<A>
└── to_param() -> ToolUnionParam
```

#### Two-Phase Tool Execution

```rust
ToolCallback<A>
├── compute_tool_result() -> IntermediateToolResult
└── apply_tool_result() -> ToolResult
```

**Built-in Tool Types:**
- `ToolBash20241022` / `ToolBash20250124`: Shell command execution
- `ToolTextEditor20250124` / `20250429` / `20250728`: File editing tools
- `WebSearchTool20250305`: Web search capabilities

#### Tool Result System

```rust
ToolResult = ControlFlow<Error, Result<ToolResultBlock, ToolResultBlock>>
├── Break(Error): Stop execution with error
└── Continue(Result): Continue with success/error result
```

### 7. Backoff and Retry (`backoff.rs`)

Exponential backoff implementation for API resilience:

```rust
ExponentialBackoff
├── initial_delay
├── max_delay  
├── multiplier
├── max_retries
└── jitter
```

## Data Flow

### Non-Streaming Request

```
User Code
    ↓
MessageCreateParams
    ↓
Validation
    ↓
Anthropic Client
    ↓
Retry Logic
    ↓
HTTP Request
    ↓
Response Parsing
    ↓
Message
```

### Streaming Request

```
User Code
    ↓
MessageCreateParams
    ↓
Anthropic Client
    ↓
HTTP Stream
    ↓
SSE Parser
    ↓
Stream<MessageStreamEvent>
    ↓
User Handler
```

### Agent Interaction

```
User Input
    ↓
Agent.chat_with() or Agent.step()
    ↓
Message Building & Validation
    ↓
Anthropic Client API Call
    ↓
Tool Execution (Two-Phase)
  ├── compute_tool_result()
  └── apply_tool_result()
    ↓
Message Assembly & Return
```

## Type System

### Message Types

```
MessageParam (Input)
├── role: MessageRole
└── content: MessageParamContent
    ├── String
    └── Array<ContentBlock>

Message (Output)
├── id: String
├── content: Vec<ContentBlock>
├── stop_reason: Option<StopReason>
└── usage: Option<Usage>
```

### Content Blocks

```
ContentBlock
├── Text(TextBlock)
├── Image(ImageBlock) 
├── Document(DocumentBlock)
├── ToolUse(ToolUseBlock)
├── ToolResult(ToolResultBlock)
├── Thinking(ThinkingBlock)
├── RedactedThinking(RedactedThinkingBlock)
└── WebSearchResult(WebSearchResultBlock)
```

## Extension Points

### 1. Custom Agents

Implement the `Agent` trait to create custom agents:

```rust
impl Agent for MyAgent {
    async fn max_tokens(&self) -> u32 { 2048 }
    async fn model(&self) -> Model { ... }
    async fn tools(&self) -> Vec<Box<dyn Tool<Self>>> { ... }
}
```

### 2. Custom Tools

Implement the `Tool` trait for custom functionality:

```rust
impl<A: Agent> Tool<A> for MyTool {
    fn name(&self) -> String { ... }
    fn callback(&self) -> Box<dyn ToolCallback<A>> { ... }
    fn to_param(&self) -> ToolUnionParam { ... }
}
```

### 3. Middleware

Create custom middleware for cross-cutting concerns:

```rust
impl<A: Agent> ToolMiddleware<A> for MyMiddleware {
    async fn before_execute(...) { ... }
    async fn after_execute(...) { ... }
    async fn on_error(...) { ... }
}
```

### 4. Custom File Systems

Implement virtual file systems for agents:

```rust
impl FileSystem for MyFileSystem {
    fn read(&self, path: &Path) -> Result<String, Error> { ... }
    fn write(&self, path: &Path, contents: &str) -> Result<(), Error> { ... }
    fn exists(&self, path: &Path) -> bool { ... }
}
```

## Performance Considerations

### Two-Phase Tool Execution

The tool system separates computation from state modification for better performance and safety:

```rust
Phase 1: compute_tool_result() (read-only, parallel-safe)
Phase 2: apply_tool_result() (state modification, sequential)
```

### Streaming Efficiency

- Lazy evaluation of SSE events
- Minimal buffer copying
- Backpressure support

### Connection Pooling

The underlying `reqwest` client maintains connection pools for efficiency.

## Security Model

### Input Validation

All API parameters are validated before sending:
- Message content validation
- Tool parameter validation  
- Model parameter validation
- Token limit enforcement

## Testing Strategy

### Unit Tests
- Individual component testing
- Error condition coverage
- State transition validation

### Integration Tests
- End-to-end API flows
- Mock server testing
- Retry mechanism validation

### Property-Based Tests
- Builder pattern validation
- State machine properties
- Security policy enforcement

## Best Practices

### 1. Error Handling

Always handle errors explicitly:

```rust
match client.send(params).await {
    Ok(message) => process(message),
    Err(e) if e.is_rate_limit() => wait_and_retry(),
    Err(e) => handle_error(e),
}
```

### 2. Tool Implementation

Implement tools with two-phase execution:

```rust
impl<A: Agent> ToolCallback<A> for MyTool {
    async fn compute_tool_result(&self, ...) -> Box<dyn IntermediateToolResult> {
        // Read-only computation phase
        ...
    }
    
    async fn apply_tool_result(&self, ...) -> ToolResult {
        // State modification phase  
        ...
    }
}
```

### 3. Message Handling

Use helper functions for message management:

```rust
// Merge consecutive messages from same role
push_or_merge_message(&mut messages, new_message);

// Combine message content 
merge_message_content(&mut existing_content, new_content);
```

### 4. Streaming

Handle streaming responses properly:

```rust
let mut stream = client.stream_message(params).await?;
while let Some(event) = stream.next().await {
    match event? {
        MessageStreamEvent::ContentBlockStart { content_block, .. } => { ... }
        MessageStreamEvent::ContentBlockDelta { delta, .. } => { ... }
        // ... handle other events
    }
}
```

## Future Enhancements

### Planned Features

1. **Enhanced Tool System**: More built-in tool types and capabilities
2. **Improved Error Recovery**: Better handling of partial failures
3. **Performance Optimization**: Connection pooling and request batching 
4. **Extended Type Coverage**: Support for new API features as they're released
5. **Developer Experience**: Better debugging and introspection tools

### API Evolution

The library tracks Anthropic API evolution with:
- Versioned tool types (dated suffixes like `20250124`)
- Extensible content block system
- Comprehensive streaming event coverage
- Support for new features like citations and thinking

## Contributing

When contributing to Claudius:

1. Follow Rust idioms and naming conventions
2. Add comprehensive tests for new features
3. Update documentation for API changes
4. Ensure backward compatibility when possible
5. Run `cargo clippy` and `cargo fmt` before submitting

## License

Apache 2.0 - See LICENSE file for details.