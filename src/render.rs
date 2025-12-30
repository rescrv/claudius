//! Output rendering for chat and agent streaming.
//!
//! This module provides renderer traits and plain-text implementations for
//! both chat output and agent streaming output.

use std::io::{self, Stdout, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::StopReason;

/// ANSI escape code for dim text (used for thinking blocks).
const ANSI_DIM: &str = "\x1b[2m";

/// ANSI escape code for italic text (used for thinking blocks).
const ANSI_ITALIC: &str = "\x1b[3m";

/// ANSI escape code to reset all styling.
const ANSI_RESET: &str = "\x1b[0m";

/// ANSI escape code for cyan text (used for tool names).
const ANSI_CYAN: &str = "\x1b[36m";

/// ANSI escape code for yellow text (used for tool input).
const ANSI_YELLOW: &str = "\x1b[33m";

/// ANSI escape code for green text (used for tool result success).
const ANSI_GREEN: &str = "\x1b[32m";

/// ANSI escape code for red text (used for tool result errors).
const ANSI_RED: &str = "\x1b[31m";

/// ANSI escape code for magenta text (used for tool result bodies).
const ANSI_MAGENTA: &str = "\x1b[35m";

///////////////////////////////////////// Streaming /////////////////////////////////////////

/// Stream context information for renderer output.
pub trait StreamContext: Send + Sync {
    /// Display label for the stream, if any.
    fn label(&self) -> Option<&str> {
        None
    }

    /// Nesting depth for sub-streams (0 = root).
    fn depth(&self) -> usize {
        0
    }
}

/// Context for streaming agent output, including display label and nesting depth.
#[derive(Debug, Clone)]
pub struct AgentStreamContext {
    /// Display label for the agent.
    pub label: String,
    /// Nesting depth for sub-agents (0 = root).
    pub depth: usize,
}

impl AgentStreamContext {
    /// Creates a root stream context.
    pub fn root(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            depth: 0,
        }
    }

    /// Creates a child stream context with incremented depth.
    ///
    /// Use this when invoking sub-agents or nested tool executions to maintain
    /// proper indentation hierarchy in the rendered output.
    ///
    /// # Example
    ///
    /// ```rust
    /// use claudius::AgentStreamContext;
    ///
    /// let root = AgentStreamContext::root("MainAgent");
    /// assert_eq!(root.depth, 0);
    ///
    /// let child = root.child("SubAgent");
    /// assert_eq!(child.depth, 1);
    /// assert_eq!(child.label, "SubAgent");
    /// ```
    pub fn child(&self, label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            depth: self.depth + 1,
        }
    }
}

impl StreamContext for AgentStreamContext {
    fn label(&self) -> Option<&str> {
        Some(&self.label)
    }

    fn depth(&self) -> usize {
        self.depth
    }
}

impl StreamContext for () {}

/// Trait for rendering streaming output.
///
/// This abstraction allows for different rendering strategies:
/// - Plain text with ANSI styling
/// - Plain text without styling (for piping/redirecting)
/// - TUI rendering, tool call display for agents
pub trait Renderer: Send {
    /// Called when a stream begins.
    fn start_agent(&mut self, context: &dyn StreamContext) {
        _ = context;
    }

    /// Called when a stream finishes.
    fn finish_agent(&mut self, context: &dyn StreamContext, stop_reason: Option<&StopReason>) {
        _ = context;
        _ = stop_reason;
    }

    /// Print a chunk of regular response text.
    ///
    /// This is called incrementally as tokens are streamed from the API.
    fn print_text(&mut self, context: &dyn StreamContext, text: &str);

    /// Print a chunk of thinking text.
    ///
    /// Thinking blocks are displayed differently (dim/italic) to
    /// distinguish them from the main response.
    fn print_thinking(&mut self, context: &dyn StreamContext, text: &str);

    /// Print an error message.
    fn print_error(&mut self, context: &dyn StreamContext, error: &str);

    /// Print an informational message.
    fn print_info(&mut self, context: &dyn StreamContext, info: &str);

    /// Called when a tool use block starts.
    ///
    /// This is called when the model begins a tool call, before any
    /// input JSON is streamed.
    fn start_tool_use(&mut self, context: &dyn StreamContext, name: &str, id: &str);

    /// Print a chunk of tool input JSON.
    ///
    /// This is called incrementally as the tool input JSON is streamed.
    fn print_tool_input(&mut self, context: &dyn StreamContext, partial_json: &str);

    /// Called when a tool use block is complete.
    ///
    /// This is called after all tool input JSON has been streamed.
    fn finish_tool_use(&mut self, context: &dyn StreamContext);

    /// Called when the model streams a tool result block.
    fn start_tool_result(&mut self, context: &dyn StreamContext, tool_use_id: &str, is_error: bool);

    /// Print tool result text content.
    fn print_tool_result_text(&mut self, context: &dyn StreamContext, text: &str);

    /// Called when a tool result block is complete.
    fn finish_tool_result(&mut self, context: &dyn StreamContext);

    /// Called when a response is complete.
    ///
    /// Used to ensure proper newlines and cleanup after streaming.
    fn finish_response(&mut self, context: &dyn StreamContext);

    /// Called when the stream is interrupted by the user.
    fn print_interrupted(&mut self, context: &dyn StreamContext) {
        _ = context;
    }

    /// Returns true if streaming should be interrupted.
    fn should_interrupt(&self) -> bool {
        false
    }
}

/// Plain text renderer with optional ANSI styling.
///
/// This renderer outputs text directly to stdout with optional
/// ANSI escape codes for styling thinking blocks and tool use.
pub struct PlainTextRenderer {
    stdout: Stdout,
    use_color: bool,
    in_thinking: bool,
    in_tool_result: bool,
    line_start: bool,
    interrupted: Option<Arc<AtomicBool>>,
}

impl PlainTextRenderer {
    /// Creates a new PlainTextRenderer with ANSI colors enabled.
    pub fn new() -> Self {
        Self {
            stdout: io::stdout(),
            use_color: true,
            in_thinking: false,
            in_tool_result: false,
            line_start: true,
            interrupted: None,
        }
    }

    /// Creates a new PlainTextRenderer with specified color setting.
    pub fn with_color(use_color: bool) -> Self {
        Self {
            stdout: io::stdout(),
            use_color,
            in_thinking: false,
            in_tool_result: false,
            line_start: true,
            interrupted: None,
        }
    }

    /// Attaches an interrupt flag to the renderer.
    pub fn with_interrupt(mut self, interrupted: Arc<AtomicBool>) -> Self {
        self.interrupted = Some(interrupted);
        self
    }

    /// Creates a new PlainTextRenderer with specified color and interrupt flag.
    pub fn with_color_and_interrupt(use_color: bool, interrupted: Arc<AtomicBool>) -> Self {
        Self::with_color(use_color).with_interrupt(interrupted)
    }

    /// Flushes stdout to ensure immediate display of streamed content.
    fn flush(&mut self) {
        let _ = self.stdout.flush();
    }

    fn reset_thinking(&mut self) {
        if self.in_thinking {
            if self.use_color {
                print!("{ANSI_RESET}");
            }
            self.in_thinking = false;
        }
    }

    fn reset_tool_result(&mut self) {
        if self.in_tool_result {
            if self.use_color {
                print!("{ANSI_RESET}");
            }
            self.in_tool_result = false;
        }
    }

    fn reset_styles(&mut self) {
        self.reset_thinking();
        self.reset_tool_result();
    }

    /// Writes text with proper indentation based on context depth.
    ///
    /// Each line is prefixed with indentation corresponding to the nesting depth.
    fn write_with_indent(&mut self, context: &dyn StreamContext, text: &str) {
        let prefix = "  ".repeat(context.depth());
        for line in text.split_inclusive('\n') {
            if self.line_start {
                print!("{prefix}");
            }
            print!("{line}");
            self.line_start = line.ends_with('\n');
        }
        self.flush();
    }
}

impl Default for PlainTextRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderer for PlainTextRenderer {
    fn start_agent(&mut self, context: &dyn StreamContext) {
        let Some(label) = context.label() else {
            return;
        };
        self.reset_styles();
        self.write_with_indent(context, &format!("[agent: {label}]\n"));
    }

    fn finish_agent(&mut self, context: &dyn StreamContext, stop_reason: Option<&StopReason>) {
        let Some(label) = context.label() else {
            return;
        };
        self.reset_styles();
        if let Some(stop_reason) = stop_reason {
            self.write_with_indent(
                context,
                &format!("[agent: {label} done: {stop_reason:?}]\n"),
            );
        } else {
            self.write_with_indent(context, &format!("[agent: {label} done]\n"));
        }
    }

    fn print_text(&mut self, context: &dyn StreamContext, text: &str) {
        self.reset_styles();
        self.write_with_indent(context, text);
    }

    fn print_thinking(&mut self, context: &dyn StreamContext, text: &str) {
        if self.use_color {
            if !self.in_thinking {
                self.write_with_indent(context, ANSI_DIM);
                self.write_with_indent(context, ANSI_ITALIC);
                self.in_thinking = true;
            }
            self.write_with_indent(context, text);
        } else {
            if !self.in_thinking {
                let prefix = if context.depth() == 0 && context.label().is_none() {
                    "\n[thinking] "
                } else {
                    "[thinking] "
                };
                self.write_with_indent(context, prefix);
                self.in_thinking = true;
            }
            self.write_with_indent(context, text);
        }
    }

    fn print_error(&mut self, context: &dyn StreamContext, error: &str) {
        self.reset_styles();
        if context.depth() == 0 && context.label().is_none() {
            eprintln!("\nError: {error}");
        } else {
            self.write_with_indent(context, &format!("\nError: {error}\n"));
        }
    }

    fn print_info(&mut self, context: &dyn StreamContext, info: &str) {
        self.reset_styles();
        if context.depth() == 0 && context.label().is_none() {
            println!("{info}");
            self.line_start = true;
            self.flush();
        } else {
            self.write_with_indent(context, &format!("{info}\n"));
        }
    }

    fn start_tool_use(&mut self, context: &dyn StreamContext, name: &str, id: &str) {
        self.reset_styles();

        if self.use_color {
            self.write_with_indent(
                context,
                &format!("\n{ANSI_CYAN}[tool: {name}]{ANSI_RESET} {ANSI_DIM}({id}){ANSI_RESET}\n"),
            );
            self.write_with_indent(context, ANSI_YELLOW);
        } else {
            self.write_with_indent(context, &format!("\n[tool: {name}] ({id})\n"));
        }
    }

    fn print_tool_input(&mut self, context: &dyn StreamContext, partial_json: &str) {
        self.write_with_indent(context, partial_json);
    }

    fn finish_tool_use(&mut self, context: &dyn StreamContext) {
        if self.use_color {
            self.write_with_indent(context, ANSI_RESET);
        }
        self.write_with_indent(context, "\n");
    }

    fn start_tool_result(
        &mut self,
        context: &dyn StreamContext,
        tool_use_id: &str,
        is_error: bool,
    ) {
        self.reset_styles();
        self.in_tool_result = true;
        if self.use_color {
            let label_color = if is_error { ANSI_RED } else { ANSI_GREEN };
            let status = if is_error { "error" } else { "ok" };
            self.write_with_indent(
                context,
                &format!(
                    "\n{label_color}[tool result: {tool_use_id} ({status})]{ANSI_RESET}\n{ANSI_MAGENTA}"
                ),
            );
        } else if is_error {
            self.write_with_indent(context, &format!("\n[tool result: {tool_use_id} error]\n"));
        } else {
            self.write_with_indent(context, &format!("\n[tool result: {tool_use_id}]\n"));
        }
    }

    fn print_tool_result_text(&mut self, context: &dyn StreamContext, text: &str) {
        self.write_with_indent(context, text);
    }

    fn finish_tool_result(&mut self, context: &dyn StreamContext) {
        self.reset_tool_result();
        self.write_with_indent(context, "\n");
    }

    fn finish_response(&mut self, context: &dyn StreamContext) {
        self.reset_styles();
        self.write_with_indent(context, "\n");
    }

    fn print_interrupted(&mut self, context: &dyn StreamContext) {
        self.reset_styles();
        let message = if context.depth() == 0 && context.label().is_none() {
            "\n[interrupted]\n"
        } else {
            "[interrupted]\n"
        };
        self.write_with_indent(context, message);
    }

    fn should_interrupt(&self) -> bool {
        self.interrupted
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::Relaxed))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renderer_default_has_color() {
        let renderer = PlainTextRenderer::new();
        assert!(renderer.use_color);
    }

    #[test]
    fn renderer_without_color() {
        let renderer = PlainTextRenderer::with_color(false);
        assert!(!renderer.use_color);
    }
}
