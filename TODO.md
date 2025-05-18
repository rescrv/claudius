# Claudius - Anthropic Rust SDK Development Plan

## Overview
This document outlines a comprehensive plan for developing `claudius`, a Rust SDK for interacting with the Anthropic API. The design is inspired by the official Python and TypeScript SDKs but will leverage Rust's unique features for performance, safety, and developer experience.

## Phase 1: Core Infrastructure

### Client Architecture
- [ ] Define core client traits and structs
  - [ ] Implement `Client` async_trait
  - [ ] Create the main `Anthropic` client struct
  - [ ] Add support for client options similar to TypeScript (e.g., `withOptions` method)
- [ ] Implement authentication mechanisms
  - [ ] API key authentication via header
  - [ ] Auth token authentication via Bearer
  - [ ] Environment variable fallbacks
- [ ] Set up configuration system
  - [ ] Timeouts (with calculation based on token count for non-streaming requests)
  - [ ] Retry mechanisms with exponential backoff
  - [ ] Base URL configuration
  - [ ] Default headers with API versioning
  - [ ] Client-level options customization

### HTTP Layer
- [ ] Evaluate HTTP client options (reqwest, hyper, etc.)
- [ ] Implement request building with proper validation
- [ ] Implement response parsing
- [ ] Design stream handling framework
- [ ] Enable customizable fetch options
- [ ] Implement idempotency keys for non-GET requests
- [ ] Implement error handling and mapping to domain-specific errors
- [ ] Set up connection pooling and keep-alive settings
- [ ] Add header/query parameter sanitization
- [ ] Support for custom headers, custom query parameters

### Error Handling
- [ ] Define comprehensive error types
  - [ ] API error types (400, 401, 403, 404, 429, 500, etc.)
  - [ ] Connection errors
  - [ ] Timeout errors
  - [ ] Validation errors
  - [ ] Abort errors
- [ ] Implement error conversion from HTTP responses
- [ ] Add detailed error messages with context
- [ ] Include request IDs in errors for easier debugging
- [ ] Support retry-after headers for rate limiting
- [ ] Add special handling for request timeouts

## Phase 2: API Resources and Models

### Models & Type System
- [ ] Define core data types
  - [ ] Base types
    - [ ] Model (model.py)
    - [ ] ModelInfo (model_info.py) 
    - [ ] Usage (usage.py)
    - [ ] StopReason (stop_reason.py)
  - [ ] Message types
    - [ ] Message (message.py)
    - [ ] MessageCreate (message_create.py)
    - [ ] MessageCountTokens (message_count_tokens.py)
    - [ ] MessageTokensCount (message_tokens_count.py)
  - [ ] Content block types
    - [ ] ContentBlock (content_block.py)
    - [ ] TextBlock (text_block.py)
    - [ ] ImageBlock (image_block.py)
    - [ ] DocumentBlock (document_block.py)
    - [ ] TextCitation (text_citation.py)
    - [ ] WebSearchResultBlock (web_search_result_block.py)
    - [ ] ThinkingBlock (thinking_block.py)
    - [ ] RedactedThinkingBlock (redacted_thinking_block.py)
    - [ ] ToolUseBlock (tool_use_block.py)
    - [ ] ServerToolUseBlock (server_tool_use_block.py)
    - [ ] ToolResultBlock (tool_result_block.py)
  - [ ] Source types
    - [ ] ContentBlockSource (content_block_source.py)
    - [ ] ContentBlockSourceContent (content_block_source_content.py)
    - [ ] PlainTextSource (plain_text_source.py)
    - [ ] Base64ImageSource (base64_image_source.py)
    - [ ] UrlImageSource (url_image_source.py)
    - [ ] Base64PdfSource (base64_pdf_source.py)
    - [ ] UrlPdfSource (url_pdf_source.py)
  - [ ] Tool types
    - [ ] Tool (tool.py)
    - [ ] ToolUnion (tool_union.py)
    - [ ] ToolChoice (tool_choice.py)
    - [ ] ToolChoiceAny (tool_choice_any.py)
    - [ ] ToolChoiceAuto (tool_choice_auto.py)
    - [ ] ToolChoiceNone (tool_choice_none.py)
    - [ ] ToolChoiceTool (tool_choice_tool.py)
    - [ ] ToolBash20250124 (tool_bash_20250124.py)
    - [ ] ToolTextEditor20250124 (tool_text_editor_20250124.py)
    - [ ] WebSearchTool20250305 (web_search_tool_20250305.py)
    - [ ] ServerToolUsage (server_tool_usage.py)
  - [ ] Thinking types
    - [ ] ThinkingConfig (thinking_config.py)
    - [ ] ThinkingConfigEnabled (thinking_config_enabled.py)
    - [ ] ThinkingConfigDisabled (thinking_config_disabled.py)
    - [ ] ThinkingDelta (thinking_delta.py)
  - [ ] Citation types
    - [ ] CitationsConfig (citations_config.py)
    - [ ] CitationsDelta (citations_delta.py)
    - [ ] CitationCharLocation (citation_char_location.py)
    - [ ] CitationPageLocation (citation_page_location.py)
    - [ ] CitationContentBlockLocation (citation_content_block_location.py)
    - [ ] CitationsWebSearchResultLocation (citations_web_search_result_location.py)
  - [ ] Stream event types
    - [ ] MessageStreamEvent (message_stream_event.py)
    - [ ] MessageStartEvent (message_start_event.py)
    - [ ] MessageStopEvent (message_stop_event.py)
    - [ ] MessageDeltaEvent (message_delta_event.py)
    - [ ] MessageDeltaUsage (message_delta_usage.py)
    - [ ] ContentBlockStartEvent (content_block_start_event.py)
    - [ ] ContentBlockStopEvent (content_block_stop_event.py)
    - [ ] ContentBlockDeltaEvent (content_block_delta_event.py)
    - [ ] TextDelta (text_delta.py)
    - [ ] InputJsonDelta (input_json_delta.py)
    - [ ] SignatureDelta (signature_delta.py)
    - [ ] RawMessageStreamEvent (raw_message_stream_event.py)
    - [ ] RawMessageStartEvent (raw_message_start_event.py)
    - [ ] RawMessageStopEvent (raw_message_stop_event.py)
    - [ ] RawMessageDeltaEvent (raw_message_delta_event.py)
    - [ ] RawContentBlockDelta (raw_content_block_delta.py)
    - [ ] RawContentBlockStartEvent (raw_content_block_start_event.py)
    - [ ] RawContentBlockStopEvent (raw_content_block_stop_event.py)
    - [ ] RawContentBlockDeltaEvent (raw_content_block_delta_event.py)
  - [ ] Error types
    - [ ] ErrorObject (shared/error_object.py)
    - [ ] ApiErrorObject (shared/api_error_object.py)
    - [ ] ErrorResponse (shared/error_response.py)
    - [ ] AuthenticationError (shared/authentication_error.py)
    - [ ] InvalidRequestError (shared/invalid_request_error.py)
    - [ ] NotFoundError (shared/not_found_error.py)
    - [ ] PermissionError (shared/permission_error.py)
    - [ ] RateLimitError (shared/rate_limit_error.py)
    - [ ] OverloadedError (shared/overloaded_error.py)
    - [ ] BillingError (shared/billing_error.py)
    - [ ] GatewayTimeoutError (shared/gateway_timeout_error.py)
  - [ ] Metadata and cache types
    - [ ] Metadata (metadata.py)
    - [ ] CacheControlEphemeral (cache_control_ephemeral.py)
- [ ] Implement serialization/deserialization with serde
- [ ] Create builder patterns for request construction
- [ ] Add type-safe enums for options like ToolChoice, StopReason, etc.

### API Resources
- [ ] Implement Messages API
  - [ ] Create messages (with streaming and non-streaming variants)
  - [ ] Stream messages with proper event handling
  - [ ] Count tokens
- [ ] Implement Models API
  - [ ] List models
  - [ ] Retrieve model information
- [ ] Use type-safe request/response parameters


## Phase 3: Advanced Features

### Streaming Support
- [ ] Implement synchronous streaming
- [ ] Implement asynchronous streaming
- [ ] Create MessageStream struct similar to TypeScript SDK
- [ ] Support event subscription model (listeners)
- [ ] Add helpers for consuming streamed responses
- [ ] Support all stream event types
  - [ ] Message events
  - [ ] Content block events
  - [ ] Thinking events
  - [ ] Tool use events
- [ ] Create helper methods for accessing streamed text, final messages, etc.
- [ ] Add proper cleanup for stream resources
- [ ] Support stream cancellation

### Tool Use Support
- [ ] Implement tool definition types
- [ ] Add tool use response handling
- [ ] Add tool result submission
- [ ] Support JSON schema validation
- [ ] Implement all tool choice types
- [ ] Add convenience methods for common tool use patterns

### Images & Rich Media
- [ ] Support for image inputs
  - [ ] Base64 encoded images
  - [ ] URL images
- [ ] Support for PDF inputs
  - [ ] Base64 encoded PDFs
  - [ ] URL PDFs
- [ ] Support for multi-part content
- [ ] Add Document source handling
- [ ] Implement proper content type handling
- [ ] Provide convenient methods for file loading/conversion

### Rate Limiting & Pagination
- [ ] Implement automatic rate limit handling and retries
- [ ] Add pagination support for list endpoints
- [ ] Support cursor-based pagination
- [ ] Add iterator-based access to paginated resources

## Phase 4: Extended Features

### Thinking Support
- [ ] Implement thinking configuration
- [ ] Add thinking block handling
- [ ] Support redacted thinking blocks
- [ ] Add convenience methods for thinking content extraction

### Web Search Support
- [ ] Implement web search tools
- [ ] Support web search result blocks
- [ ] Add citation handling for web search results
- [ ] Support encrypted indexes

### Citations Support
- [ ] Support citation configuration
- [ ] Implement citation blocks
- [ ] Handle different citation location types
- [ ] Add convenience methods for citation extraction
- [ ] Support PDF page citations
- [ ] Support char location citations
- [ ] Support content block location citations

### Metadata & Caching
- [ ] Support metadata parameters
- [ ] Add cache control functionality
- [ ] Implement usage tracking
- [ ] Add request ID tracking and access
- [ ] Support ephemeral content blocks

## Phase 5: Integrations

### AWS Bedrock Integration
- [ ] Implement Bedrock client
- [ ] Configure AWS authentication
- [ ] Support model name translation
- [ ] Support Bedrock-specific parameters

### Google Vertex Integration
- [ ] Implement Vertex client
- [ ] Configure Google Cloud authentication
- [ ] Support Vertex-specific parameters
- [ ] Add model version translation

## Phase 6: Testing & Documentation

### Testing
- [ ] Unit tests for all components
- [ ] Integration tests against API
- [ ] Mock server for testing without API credentials
- [ ] Snapshot tests for responses
- [ ] Add comprehensive test fixtures
- [ ] Example applications
- [ ] Test both sync and async variants
- [ ] Add CI test pipeline for all platforms

### Documentation
- [ ] API documentation with rustdoc
- [ ] Comprehensive README
- [ ] Code examples for common use cases
- [ ] Detailed API reference
- [ ] Sample applications
- [ ] Method-level documentation
- [ ] Add tutorials for common workflows

## Phase 7: Performance & Optimization

### Performance
- [ ] Benchmark against other SDKs
- [ ] Optimize memory usage
- [ ] Reduce allocations in hot paths
- [ ] Improve streaming performance
- [ ] Optimize serialization/deserialization
- [ ] Implement proper caching
- [ ] Add configurable TCP settings

### Developer Experience
- [ ] Add builder patterns for complex request types
- [ ] Implement convenient helper methods
- [ ] Create idiomatic Rust interfaces while maintaining compatibility
- [ ] Add feature flags for optional dependencies
- [ ] Support custom HTTP client implementations
- [ ] Add debugging tools
- [ ] Implement logging infrastructure
- [ ] Add tracing support

## Phase 8: Release & Maintenance

### Crate Publishing
- [ ] Set up crate metadata
- [ ] Configure CI/CD for automated testing and release
- [ ] Establish versioning strategy
- [ ] Add changelog automation
- [ ] Set up documentation publishing

### Community Support
- [ ] Set up issue templates
- [ ] Add contributing guidelines
- [ ] Create community examples repository
- [ ] Add support for security disclosures
- [ ] Set up community communication channels

## Technical Design Considerations

### Async/Sync Support
- Both synchronous and asynchronous APIs will be supported similar to the Python and TypeScript SDKs
- All async code will be runtime-agnostic, allowing use with any async runtime (tokio, async-std)
- Provide clear examples for both sync and async usage

### Error Handling
- Use rich error types with context
- Provide both error codes and descriptive messages
- Make errors easily convertible to application-specific errors
- Include request IDs in errors for debugging
- Support error retry logic based on status codes

### API Design
- Prioritize type safety and compiler validation
- Use builder patterns for complex request parameters
- Leverage Rust's type system to prevent invalid requests at compile time
- Support for both high-level and low-level API access
- Allow customization at both client and request level

### Resource Management
- Implement proper cleanup for all resources
- Ensure streams are properly closed
- Add support for cancellation through signals/futures
- Implement timeouts with proper cleanup

### Versioning
- Follow semantic versioning
- Document breaking changes clearly
- Provide migration guides for major version changes
- Support multiple API versions
- Deprecate methods with warnings, not breaks
