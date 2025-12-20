//! Output rendering for the chat application.
//!
//! This module provides a trait-based rendering abstraction that allows
//! for different output styles. The default implementation uses ANSI
//! escape codes for styling thinking blocks differently from regular text.

use std::io::{self, Stdout, Write};

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

/// Trait for rendering chat output.
///
/// This abstraction allows for different rendering strategies:
/// - Plain text with ANSI styling
/// - Plain text without styling (for piping/redirecting)
/// - TUI rendering, tool call display for agents
pub trait Renderer: Send {
    /// Print a chunk of regular response text.
    ///
    /// This is called incrementally as tokens are streamed from the API.
    fn print_text(&mut self, text: &str);

    /// Print a chunk of thinking text.
    ///
    /// Thinking blocks are displayed differently (dim/italic) to
    /// distinguish them from the main response.
    fn print_thinking(&mut self, text: &str);

    /// Print an error message.
    fn print_error(&mut self, error: &str);

    /// Print an informational message.
    fn print_info(&mut self, info: &str);

    /// Called when a tool use block starts.
    ///
    /// This is called when the model begins a tool call, before any
    /// input JSON is streamed.
    fn start_tool_use(&mut self, name: &str, id: &str);

    /// Print a chunk of tool input JSON.
    ///
    /// This is called incrementally as the tool input JSON is streamed.
    fn print_tool_input(&mut self, partial_json: &str);

    /// Called when a tool use block is complete.
    ///
    /// This is called after all tool input JSON has been streamed.
    fn finish_tool_use(&mut self);

    /// Called when the model streams a tool result block.
    fn start_tool_result(&mut self, tool_use_id: &str, is_error: bool);

    /// Print tool result text content.
    fn print_tool_result_text(&mut self, text: &str);

    /// Called when a tool result block is complete.
    fn finish_tool_result(&mut self);

    /// Called when a response is complete.
    ///
    /// Used to ensure proper newlines and cleanup after streaming.
    fn finish_response(&mut self);

    /// Called when the stream is interrupted by the user.
    fn print_interrupted(&mut self);
}

/// Plain text renderer with optional ANSI styling.
///
/// This renderer outputs text directly to stdout with optional
/// ANSI escape codes for styling thinking blocks and tool use.
pub struct PlainTextRenderer {
    stdout: Stdout,
    use_color: bool,
    in_thinking: bool,
    in_tool_use: bool,
    in_tool_result: bool,
}

impl PlainTextRenderer {
    /// Creates a new PlainTextRenderer with ANSI colors enabled.
    pub fn new() -> Self {
        Self {
            stdout: io::stdout(),
            use_color: true,
            in_thinking: false,
            in_tool_use: false,
            in_tool_result: false,
        }
    }

    /// Creates a new PlainTextRenderer with specified color setting.
    pub fn with_color(use_color: bool) -> Self {
        Self {
            stdout: io::stdout(),
            use_color,
            in_thinking: false,
            in_tool_use: false,
            in_tool_result: false,
        }
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
}

impl Default for PlainTextRenderer {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderer for PlainTextRenderer {
    fn print_text(&mut self, text: &str) {
        self.reset_styles();
        print!("{text}");
        self.flush();
    }

    fn print_thinking(&mut self, text: &str) {
        if self.use_color {
            if !self.in_thinking {
                print!("{ANSI_DIM}{ANSI_ITALIC}");
                self.in_thinking = true;
            }
            print!("{text}");
        } else {
            if !self.in_thinking {
                print!("\n[thinking] ");
                self.in_thinking = true;
            }
            print!("{text}");
        }
        self.flush();
    }

    fn print_error(&mut self, error: &str) {
        self.reset_styles();
        eprintln!("\nError: {error}");
    }

    fn print_info(&mut self, info: &str) {
        self.reset_styles();
        println!("{info}");
    }

    fn start_tool_use(&mut self, name: &str, id: &str) {
        self.reset_styles();
        self.in_tool_use = true;

        if self.use_color {
            print!("\n{ANSI_CYAN}[tool: {name}]{ANSI_RESET} {ANSI_DIM}({id}){ANSI_RESET}\n");
            print!("{ANSI_YELLOW}");
        } else {
            print!("\n[tool: {name}] ({id})\n");
        }
        self.flush();
    }

    fn print_tool_input(&mut self, partial_json: &str) {
        print!("{partial_json}");
        self.flush();
    }

    fn finish_tool_use(&mut self) {
        if self.use_color {
            print!("{ANSI_RESET}");
        }
        println!();
        self.in_tool_use = false;
        self.flush();
    }

    fn start_tool_result(&mut self, tool_use_id: &str, is_error: bool) {
        self.reset_styles();
        self.in_tool_result = true;
        if self.use_color {
            let label_color = if is_error { ANSI_RED } else { ANSI_GREEN };
            let status = if is_error { "error" } else { "ok" };
            print!(
                "\n{label_color}[tool result: {tool_use_id} ({status})]{ANSI_RESET}\n{ANSI_MAGENTA}"
            );
        } else if is_error {
            print!("\n[tool result: {tool_use_id} error]\n");
        } else {
            print!("\n[tool result: {tool_use_id}]\n");
        }
        self.flush();
    }

    fn print_tool_result_text(&mut self, text: &str) {
        print!("{text}");
        self.flush();
    }

    fn finish_tool_result(&mut self) {
        self.reset_tool_result();
        println!();
        self.flush();
    }

    fn finish_response(&mut self) {
        self.reset_styles();
        println!();
        self.flush();
    }

    fn print_interrupted(&mut self) {
        self.reset_styles();
        println!("\n[interrupted]");
        self.flush();
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
