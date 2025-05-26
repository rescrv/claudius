# Claudius - Anthropic Rust SDK Development Plan

## Overview
This document outlines a comprehensive plan for developing `claudius`, a Rust SDK for interacting with the Anthropic API. The design is inspired by the official Python and TypeScript SDKs but will leverage Rust's unique features for performance, safety, and developer experience.

## Recent Completed Work

### Models API Implementation (Latest)
- [X] Implemented complete Models API with list and retrieve endpoints
- [X] Added `list_models()` method with pagination support via `ModelListParams`
- [X] Added `get_model()` method for retrieving specific model information
- [X] Created `ModelListResponse` type for paginated model listings
- [X] Integrated proper error handling and retry logic for Models API
- [X] Added comprehensive test coverage for all new model types
- [X] Created example demonstrating Models API usage patterns
- [X] All 238 tests now passing with Models API functionality

### Type System Consolidation
- [X] Fixed all failing tests for serialization format compatibility
- [X] Consolidated duplicate content types (`ContentBlockSourceContentParam` and `ToolResultContent`) into unified `Content` enum
- [X] Renamed `TextOrImage` to `Content` for better semantic meaning and future extensibility
- [X] Removed Param suffix from `ToolResultBlockParam` (renamed to `ToolResultBlock`)
- [X] Removed Param suffix from tool types (`WebSearchTool20250305Param`, `ToolTextEditor20250124Param`, `ToolBash20250124Param`)
- [X] Consolidated WebSearchToolResultBlock types by moving cache_control to main type
- [X] Replaced duplicate `WebSearchToolRequestErrorCode` with `WebSearchErrorCode`
- [X] Replaced duplicate `WebSearchToolRequestErrorParam` with `WebSearchToolResultError`
- [X] Added proper serde tagging for `ToolResultContent` and related types
- [X] Consolidated duplicate `SystemPrompt` types with unified implementation
- [X] Replaced `MessageCountTokensToolParam` with existing `ToolUnionParam`
- [X] Added Claude 4 model support from Python SDK reference
- [X] All tests passing with proper serialization format expectations

## Phase 1: Core Infrastructure

### Client Architecture
- [X] Define core client traits and structs
  - [-] Implement `Client` async_trait
  - [X] Create the main `Anthropic` client struct
  - [-] Add support for client options similar to TypeScript (e.g., `withOptions` method)
- [X] Implement authentication mechanisms
  - [X] API key authentication via header
  - [-] Auth token authentication via Bearer
  - [X] Environment variable fallbacks
- [X] Set up configuration system
  - [X] Timeouts (with calculation based on token count for non-streaming requests)
  - [X] Retry mechanisms with exponential backoff
  - [X] Base URL configuration
  - [X] Default headers with API versioning
  - [-] Client-level options customization

### HTTP Layer
- [X] Evaluate HTTP client options (reqwest, hyper, etc.)
- [X] Implement request building with proper validation
- [X] Implement response parsing
- [X] Design stream handling framework
- [-] Enable customizable fetch options
- [ ] Implement idempotency keys for non-GET requests
- [X] Implement error handling and mapping to domain-specific errors
- [X] Set up connection pooling and keep-alive settings
- [-] Add header/query parameter sanitization
- [-] Support for custom headers, custom query parameters

### Error Handling
- [X] Define comprehensive error types
  - [X] API error types (400, 401, 403, 404, 429, 500, etc.)
  - [X] Connection errors
  - [X] Timeout errors
  - [X] Validation errors
  - [-] Abort errors
- [X] Implement error conversion from HTTP responses
- [X] Add detailed error messages with context
- [X] Include request IDs in errors for easier debugging
- [X] Support retry-after headers for rate limiting
- [ ] Add special handling for request timeouts

## Phase 2: API Resources and Models

### Models & Type System
- [X] Define core data types
  - [X] Base types
    - [X] Model (model.py)
    - [X] ModelInfo (model_info.py) 
    - [X] Usage (usage.py)
    - [X] StopReason (stop_reason.py)
  - [X] Message types
    - [X] Message (message.py)
    - [X] MessageCreate (message_create.py)
    - [X] MessageCountTokens (message_count_tokens.py)
    - [X] MessageTokensCount (message_tokens_count.py)
  - [X] Content block types
    - [X] ContentBlock (content_block.py)
    - [X] TextBlock (text_block.py)
    - [X] ImageBlock (image_block.py)
    - [X] DocumentBlock (document_block.py)
    - [X] TextCitation (text_citation.py)
    - [X] WebSearchResultBlock (web_search_result_block.py)
    - [X] ThinkingBlock (thinking_block.py)
    - [X] RedactedThinkingBlock (redacted_thinking_block.py)
    - [X] ToolUseBlock (tool_use_block.py)
    - [X] ServerToolUseBlock (server_tool_use_block.py)
    - [X] ToolResultBlock (tool_result_block.py)
  - [X] Source types
    - [X] ContentBlockSource (content_block_source.py)
    - [X] ContentBlockSourceContent (content_block_source_content.py)
    - [X] PlainTextSource (plain_text_source.py)
    - [X] Base64ImageSource (base64_image_source.py)
    - [X] UrlImageSource (url_image_source.py)
    - [X] Base64PdfSource (base64_pdf_source.py)
    - [X] UrlPdfSource (url_pdf_source.py)
  - [X] Tool types
    - [X] Tool (tool.py)
    - [X] ToolUnion (tool_union.py)
    - [X] ToolChoice (tool_choice.py)
    - [X] ToolChoiceAny (tool_choice_any.py)
    - [X] ToolChoiceAuto (tool_choice_auto.py)
    - [X] ToolChoiceNone (tool_choice_none.py)
    - [X] ToolChoiceTool (tool_choice_tool.py)
    - [X] ToolBash20250124 (tool_bash_20250124.py)
    - [X] ToolTextEditor20250124 (tool_text_editor_20250124.py)
    - [X] WebSearchTool20250305 (web_search_tool_20250305.py)
    - [X] ServerToolUsage (server_tool_usage.py)
  - [X] Thinking types
    - [X] ThinkingConfig (thinking_config.py)
    - [X] ThinkingConfigEnabled (thinking_config_enabled.py)
    - [X] ThinkingConfigDisabled (thinking_config_disabled.py)
    - [X] ThinkingDelta (thinking_delta.py)
  - [X] Citation types
    - [X] CitationsConfig (citations_config.py)
    - [X] CitationsDelta (citations_delta.py)
    - [X] CitationCharLocation (citation_char_location.py)
    - [X] CitationPageLocation (citation_page_location.py)
    - [X] CitationContentBlockLocation (citation_content_block_location.py)
    - [X] CitationsWebSearchResultLocation (citations_web_search_result_location.py)
  - [X] Stream event types
    - [X] MessageStreamEvent (message_stream_event.py)
    - [X] MessageStartEvent (message_start_event.py)
    - [X] MessageStopEvent (message_stop_event.py)
    - [X] MessageDeltaEvent (message_delta_event.py)
    - [X] MessageDeltaUsage (message_delta_usage.py)
    - [X] ContentBlockStartEvent (content_block_start_event.py)
    - [X] ContentBlockStopEvent (content_block_stop_event.py)
    - [X] ContentBlockDeltaEvent (content_block_delta_event.py)
    - [X] TextDelta (text_delta.py)
    - [X] InputJsonDelta (input_json_delta.py)
    - [X] SignatureDelta (signature_delta.py)
    - [-] RawMessageStreamEvent (raw_message_stream_event.py)
    - [-] RawMessageStartEvent (raw_message_start_event.py)
    - [-] RawMessageStopEvent (raw_message_stop_event.py)
    - [-] RawMessageDeltaEvent (raw_message_delta_event.py)
    - [-] RawContentBlockDelta (raw_content_block_delta.py)
    - [-] RawContentBlockStartEvent (raw_content_block_start_event.py)
    - [-] RawContentBlockStopEvent (raw_content_block_stop_event.py)
    - [-] RawContentBlockDeltaEvent (raw_content_block_delta_event.py)
  - [-] Error types
    - [-] ErrorObject (shared/error_object.py)
    - [-] ApiErrorObject (shared/api_error_object.py)
    - [-] ErrorResponse (shared/error_response.py)
    - [-] AuthenticationError (shared/authentication_error.py)
    - [-] InvalidRequestError (shared/invalid_request_error.py)
    - [-] NotFoundError (shared/not_found_error.py)
    - [-] PermissionError (shared/permission_error.py)
    - [-] RateLimitError (shared/rate_limit_error.py)
    - [-] OverloadedError (shared/overloaded_error.py)
    - [-] BillingError (shared/billing_error.py)
    - [-] GatewayTimeoutError (shared/gateway_timeout_error.py)
  - [X] Metadata and cache types
    - [X] Metadata (metadata.py)
    - [X] CacheControlEphemeral (cache_control_ephemeral.py)
- [X] Implement serialization/deserialization with serde
- [X] Create builder patterns for request construction
- [X] Add type-safe enums for options like ToolChoice, StopReason, etc.

### API Resources
- [X] Implement Messages API
  - [X] Create messages (with streaming and non-streaming variants)
  - [X] Stream messages with proper event handling
  - [X] Count tokens
- [X] Implement Models API
  - [X] List models
  - [X] Retrieve model information
- [X] Use type-safe request/response parameters


## Phase 3: Advanced Features

### Streaming Support
- [-] Implement synchronous streaming
- [X] Implement asynchronous streaming
- [X] Create MessageStream struct similar to TypeScript SDK
- [-] Support event subscription model (listeners)
- [X] Add helpers for consuming streamed responses
- [X] Support all stream event types
  - [X] Message events
  - [X] Content block events
  - [X] Thinking events
  - [X] Tool use events
- [ ] Create helper methods for accessing streamed text, final messages, etc.
- [ ] Add proper cleanup for stream resources
- [ ] Support stream cancellation

### Tool Use Support
- [X] Implement tool definition types
- [X] Add tool use response handling
- [X] Add tool result submission
- [ ] Support JSON schema validation
- [X] Implement all tool choice types
- [ ] Add convenience methods for common tool use patterns

### Images & Rich Media
- [X] Support for image inputs
  - [X] Base64 encoded images
  - [X] URL images
- [X] Support for PDF inputs
  - [X] Base64 encoded PDFs
  - [X] URL PDFs
- [X] Support for multi-part content
- [X] Add Document source handling
- [X] Implement proper content type handling
- [ ] Provide convenient methods for file loading/conversion

### Rate Limiting & Pagination
- [-] Implement automatic rate limit handling and retries
- [-] Add pagination support for list endpoints
- [-] Support cursor-based pagination
- [-] Add iterator-based access to paginated resources

## Phase 4: Extended Features

### Thinking Support
- [X] Implement thinking configuration
- [X] Add thinking block handling
- [X] Support redacted thinking blocks
- [ ] Add convenience methods for thinking content extraction

### Web Search Support
- [X] Implement web search tools
- [X] Support web search result blocks
- [X] Add citation handling for web search results
- [ ] Support encrypted indexes

### Citations Support
- [X] Support citation configuration
- [X] Implement citation blocks
- [X] Handle different citation location types
- [ ] Add convenience methods for citation extraction
- [X] Support PDF page citations
- [X] Support char location citations
- [X] Support content block location citations

### Metadata & Caching
- [X] Support metadata parameters
- [X] Add cache control functionality
- [X] Implement usage tracking
- [ ] Add request ID tracking and access
- [X] Support ephemeral content blocks

## Phase 5: Integrations

### AWS Bedrock Integration
- [-] Implement Bedrock client
- [-] Configure AWS authentication
- [-] Support model name translation
- [-] Support Bedrock-specific parameters

### Google Vertex Integration
- [-] Implement Vertex client
- [-] Configure Google Cloud authentication
- [-] Support Vertex-specific parameters
- [-] Add model version translation

## Phase 6: Testing & Documentation

### Testing
- [X] Unit tests for all components
- [ ] Integration tests against API
- [-] Mock server for testing without API credentials
- [-] Snapshot tests for responses
- [-] Add comprehensive test fixtures
- [X] Example applications
- [-] Test both sync and async variants
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
- [-] Benchmark against other SDKs
- [-] Optimize memory usage
- [-] Reduce allocations in hot paths
- [-] Improve streaming performance
- [-] Optimize serialization/deserialization
- [-] Implement proper caching
- [-] Add configurable TCP settings

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
- [X] Set up crate metadata
- [ ] Configure CI/CD for automated testing and release
- [X] Establish versioning strategy
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
