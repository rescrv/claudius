# Claude Configuration for claudius Project

This file contains persistent directives for Claude when working on the claudius project.

## Ownership and Accountability

- The buck stops with you. Own each task completely - you can either complete it, explicitly drop it with justification, or delegate it with clear accountability
- Every task must have a clear owner and trace back to the project's core objectives
- When asking for clarification or suggesting alternatives, maintain accountability for the final outcome
- Provide clear accounts of what was done, why, and how it contributes to the broader goal

## Goal

- Your goal is to create an Anthropic API client in Rust.

## Code Quality and Testing

- Test systems at transient extremes: design for 2 orders of magnitude more than promised to customers
- Embed positive signals that demonstrate the system works as intended
- Build systems in two parts: one that does work, one that verifies the work
- Prefer fewer conditional paths: systems with single defined paths test themselves on every execution

## Rust Idioms

- Follow Rust naming conventions
- Use explicit typing where it improves readability
- Implement specialized constructor methods for enum variants when appropriate
- Provide builder-style methods with meaningful names (like `with_timeout`) for configurable types
- Always place test modules at the bottom of the file
- Always organize module declarations with the following pattern:
  - Group public modules first (`pub mod bar;`), sorted alphabetically
  - Add a blank line
  - Group private modules next (`mod foo;`)
  - Add descriptive comments for each group
- Return `Option<&T>` rather than `&Option<T>` for accessor methods, using `.as_ref()`
- Re-export necessary types for doctest examples instead of marking them as `ignore`
- Prefix imports in doctests with `# ` to hide them in documentation while still testing them
- Maintain semantic accuracy in error conversions (e.g., map HttpError to HttpError when possible)
- Don't wrap types that already use Arc internally (like ReqwestClient) in another Arc
- Remove unnecessary wrapper methods that don't add value beyond the methods they wrap
- Don't add comments explaining what was removed or not implemented - just remove the code
- Remove commented-out code and imports rather than leaving them in the codebase
- Don't add explanatory comments about why a function or operation isn't needed - simply implement the code correctly without calling attention to alternatives
- Prefer a single pattern for methods that do the same thing - avoid aliases like both `register` and `add_handler`
- When parameterizing a type with generics, implement methods for the generic type, not just the concrete type
- Prefer using built-in API methods like `error_for_status()` instead of writing custom error handling logic
- Use appropriate error variants (e.g., `HttpError` for HTTP errors) rather than generic error types
- Create specific error types for common error cases instead of using a generic error with a message
- Follow Rust naming conventions for acronyms: use JsonRpc, not JSONRPC or JSONRpc
- Don't use `unwrap()` or `expect()` in public APIs; instead, return `Result` to properly propagate errors
- Re-export utility types/methods at the crate root with `pub use` to make them visible to users and silence unused method warnings
- tests/*.rs do not require a `mod test` block
- Do not add #[serde(tag ...)] annotations to a struct

## Development Workflow

- Always make sure tests are passing before embarking on a new task.  Always make sure tests are passing before returning to the user.
- Work flows down, accounts flow up: break down objectives into concrete sub-tasks, then provide clear accounts of completion upward
- Maintain consistent decision-making patterns to enable predictable collaboration
- When priorities conflict, escalate explicitly rather than making assumptions
- Practice non-interference: trust the process to unfold while providing support as needed
- Ask questions that reveal blind spots: "What do you see? What don't you see? What do you expect but don't see?"

## Sensitive Instructions

- NEVER update HITL.md.  It's Human In The Loop.

## Project Philosophy

- This is a green-field project.  Don't do things for backwards compatibility.
- Align locally to achieve global alignment: ensure each decision aligns with the broader project objectives
- Prefer explicit delegation over implicit assumptions about scope or responsibility
- Quality over scope: compromise on what the system does to achieve quality, never compromise quality to do more
- Build atomic units of value completely, then leave them or acknowledge they will be deleted
- Design systems that can verify themselves: embed positive signals that demonstrate correct operation
- Make choices that enable behavior rather than restrict it; consistent philosophy over ad-hoc rules

## Cargo Management

- Always add dependencies with `cargo add depname`.