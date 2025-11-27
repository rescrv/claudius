# Claudius Prompt Test Vectors

This directory contains test vectors for the `claudius-prompt` binary. These tests cover various scenarios and edge cases to validate the functionality of the prompt testing system.

## Test Categories

### Basic Tests
- `basic_hello.txt` - Simple text prompt file
- `simple_math.yaml` - Basic YAML config with assertion testing
- `edge_case_empty.yaml` - Minimal response testing

### Advanced Features
- `multi_turn_conversation.yaml` - Multi-message conversation testing
- `creative_writing.yaml` - Creative output with system prompt
- `code_generation.yaml` - Code generation with multiple assertions
- `long_response.yaml` - Testing longer responses with detailed assertions
- `temperature_test.yaml` - High temperature creative testing
- `stop_sequence_test.yaml` - Stop sequence functionality
- `model_comparison.yaml` - Different model testing
- `json_parsing.yaml` - Structured data parsing

### Edge Cases & Error Conditions
- `refusal_test.yaml` - Testing AI safety refusal behaviors
- `error_case_invalid_model.yaml` - Invalid model error handling

## Usage Examples

### Run a single test
```bash
cargo run --bin claudius-prompt -- prompts/simple_math.yaml
```

### Run multiple tests
```bash
cargo run --bin claudius-prompt -- prompts/simple_math.yaml prompts/creative_writing.yaml
```

### Run in test mode (exit codes)
```bash
cargo run --bin claudius-prompt -- --test prompts/simple_math.yaml
```

### Get verbose output
```bash
cargo run --bin claudius-prompt -- --verbose prompts/simple_math.yaml
```

### Different output formats
```bash
cargo run --bin claudius-prompt -- --format json prompts/simple_math.yaml
cargo run --bin claudius-prompt -- --format yaml prompts/simple_math.yaml
```

## Test Configuration Format

YAML configuration files support the following fields:

- `name`: Test name (optional)
- `prompt`: Single prompt string (for simple tests)
- `messages`: Array of conversation messages (for multi-turn tests)
- `system`: System prompt (optional)
- `model`: Model to use (default: claude-haiku-4-5)
- `max_tokens`: Maximum tokens to generate (default: 1000)
- `temperature`: Temperature setting (0.0-1.0, optional)
- `top_p`: Top-p setting (0.0-1.0, optional)
- `top_k`: Top-k setting (optional)
- `stop_sequences`: Array of stop sequences (optional)
- `expected_contains`: Array of strings that must appear in response
- `expected_not_contains`: Array of strings that must NOT appear in response
- `min_response_length`: Minimum response length in characters
- `max_response_length`: Maximum response length in characters
- `expected_tool_calls`: Array of tool names that should be called (optional)

## Assertion Testing

The test framework supports several types of assertions:

1. **Content assertions**: Check if response contains or doesn't contain specific text
2. **Length assertions**: Verify response length is within expected bounds
3. **Tool call assertions**: Verify that specific tools were called (when tools are configured)

All assertions are checked automatically, and test results include detailed failure information when assertions don't pass.