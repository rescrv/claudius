//! Interactive chat application for conversing with Claude.
//!
//! This binary provides a streaming REPL interface for chatting with Claude
//! models via the Anthropic API.
//!
//! # Usage
//!
//! ```bash
//! # Basic usage with default settings
//! claudius-chat
//!
//! # Specify a model
//! claudius-chat --model claude-sonnet-4-0
//!
//! # Set a system prompt
//! claudius-chat --system "You are a helpful coding assistant"
//!
//! # Disable colors (useful for piping output)
//! claudius-chat --no-color
//! ```
//!
//! # Commands
//!
//! While chatting, you can use slash commands:
//! - `/help` - Show available commands
//! - `/clear` - Clear conversation history
//! - `/model <name>` - Change the model
//! - `/system [prompt]` - Set or clear system prompt
//! - `/stats` - Show session statistics
//! - `/quit` - Exit the application

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use arrrg::CommandLine;
use rustyline::DefaultEditor;
use rustyline::error::ReadlineError;

use claudius::chat::{
    ChatArgs, ChatCommand, ChatConfig, ChatSession, PlainTextRenderer, Renderer, help_text,
    parse_command,
};
use claudius::{Anthropic, Model};

/// Main entry point for the claudius-chat application.
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (args, _) = ChatArgs::from_command_line_relaxed("claudius-chat [OPTIONS]");
    let config = ChatConfig::from(args);
    let use_color = config.use_color;

    let client = Anthropic::new(None)?;
    let mut session = ChatSession::new(client, config);
    let mut renderer = PlainTextRenderer::with_color(use_color);
    let mut rl = DefaultEditor::new()?;

    // Flag for interrupt handling during streaming
    let interrupted = Arc::new(AtomicBool::new(false));

    // Set up Ctrl+C handler
    let interrupted_clone = interrupted.clone();
    ctrlc::set_handler(move || {
        interrupted_clone.store(true, Ordering::Relaxed);
    })?;

    println!("Claude Chat (model: {})", session.model());
    println!("Type /help for commands, /quit to exit\n");

    loop {
        // Reset interrupt flag before each input
        interrupted.store(false, Ordering::Relaxed);

        let readline = rl.readline("You: ");

        match readline {
            Ok(line) => {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }

                let _ = rl.add_history_entry(line);

                // Check for slash commands
                if let Some(cmd) = parse_command(line) {
                    match cmd {
                        ChatCommand::Quit => {
                            println!("Goodbye!");
                            break;
                        }
                        ChatCommand::Clear => {
                            session.clear();
                            renderer.print_info("Conversation cleared.");
                        }
                        ChatCommand::Help => {
                            for line in help_text().lines() {
                                println!("    {}", line);
                            }
                        }
                        ChatCommand::Model(model_name) => {
                            let model = model_name
                                .parse()
                                .unwrap_or_else(|_| Model::Custom(model_name.clone()));
                            session.set_model(model);
                            renderer.print_info(&format!("Model changed to: {}", model_name));
                        }
                        ChatCommand::System(prompt) => {
                            session.set_system_prompt(prompt.clone());
                            match prompt {
                                Some(p) => {
                                    renderer.print_info(&format!("System prompt set to: {}", p))
                                }
                                None => renderer.print_info("System prompt cleared."),
                            }
                        }
                        ChatCommand::MaxTokens(value) => {
                            session.set_max_tokens(value);
                            renderer.print_info(&format!("max_tokens set to {value}"));
                        }
                        ChatCommand::Temperature(value) => {
                            session.set_temperature(Some(value));
                            renderer.print_info(&format!("temperature set to {:.2}", value));
                        }
                        ChatCommand::ClearTemperature => {
                            session.set_temperature(None);
                            renderer.print_info("temperature reset to model default");
                        }
                        ChatCommand::TopP(value) => {
                            session.set_top_p(Some(value));
                            renderer.print_info(&format!("top_p set to {:.2}", value));
                        }
                        ChatCommand::ClearTopP => {
                            session.set_top_p(None);
                            renderer.print_info("top_p reset to model default");
                        }
                        ChatCommand::TopK(value) => {
                            session.set_top_k(Some(value));
                            renderer.print_info(&format!("top_k set to {value}"));
                        }
                        ChatCommand::ClearTopK => {
                            session.set_top_k(None);
                            renderer.print_info("top_k reset to model default");
                        }
                        ChatCommand::AddStopSequence(sequence) => {
                            session.add_stop_sequence(sequence.clone());
                            renderer.print_info(&format!("Added stop sequence: {sequence}"));
                        }
                        ChatCommand::ClearStopSequences => {
                            session.clear_stop_sequences();
                            renderer.print_info("Stop sequences cleared.");
                        }
                        ChatCommand::ListStopSequences => {
                            print_stop_sequences(session.stop_sequences());
                        }
                        ChatCommand::Thinking(show) => {
                            session.set_show_thinking(show);
                            if show {
                                renderer.print_info("Thinking output enabled.");
                            } else {
                                renderer.print_info("Thinking output hidden.");
                            }
                        }
                        ChatCommand::Budget(tokens) => {
                            session.set_session_budget(Some(tokens));
                            renderer.print_info(&format!("Session budget set to {tokens} tokens."));
                        }
                        ChatCommand::ClearBudget => {
                            session.set_session_budget(None);
                            renderer.print_info("Session budget cleared.");
                        }
                        ChatCommand::TranscriptPath(path) => {
                            session.set_transcript_path(Some(PathBuf::from(&path)));
                            renderer.print_info(&format!("Transcript auto-save set to {}", path));
                        }
                        ChatCommand::ClearTranscriptPath => {
                            session.set_transcript_path(None);
                            renderer.print_info("Transcript auto-save disabled.");
                        }
                        ChatCommand::SaveTranscript(path) => {
                            match session.save_transcript_to(&path) {
                                Ok(_) => {
                                    renderer.print_info(&format!("Transcript saved to {}", path))
                                }
                                Err(err) => renderer
                                    .print_error(&format!("Failed to save transcript: {}", err)),
                            }
                        }
                        ChatCommand::LoadTranscript(path) => {
                            match session.load_transcript_from(&path) {
                                Ok(_) => {
                                    renderer.print_info(&format!("Transcript loaded from {}", path))
                                }
                                Err(err) => renderer
                                    .print_error(&format!("Failed to load transcript: {}", err)),
                            }
                        }
                        ChatCommand::Stats => {
                            print_stats(&session);
                        }
                        ChatCommand::ShowConfig => {
                            print_config(&session);
                        }
                        ChatCommand::Invalid(message) => {
                            renderer.print_error(&message);
                        }
                    }
                    continue;
                }

                // Regular message - send to API
                println!("Claude:");
                if let Err(e) = session
                    .send_streaming(line, &mut renderer, interrupted.clone())
                    .await
                {
                    renderer.print_error(&e.to_string());
                }
            }
            Err(ReadlineError::Interrupted) => {
                // Ctrl+C at prompt - soft interrupt
                println!();
                continue;
            }
            Err(ReadlineError::Eof) => {
                // Ctrl+D - exit
                println!("\nGoodbye!");
                break;
            }
            Err(err) => {
                renderer.print_error(&format!("Input error: {}", err));
                break;
            }
        }
    }

    Ok(())
}

fn print_stats(session: &ChatSession) {
    let stats = session.stats();
    println!("    Session Statistics:");
    println!("      Model: {}", stats.model);
    println!("      Messages: {}", stats.message_count);
    println!("      Max tokens: {}", stats.max_tokens);
    println!("      Temperature: {}", describe_float(stats.temperature));
    println!("      Top-p: {}", describe_float(stats.top_p));
    println!("      Top-k: {}", describe_top_k(stats.top_k));
    if let Some(prompt) = stats.system_prompt.as_deref() {
        println!("      System prompt: {}", prompt);
    } else {
        println!("      System prompt: (none)");
    }
    println!(
        "      Thinking output: {}",
        if stats.show_thinking {
            "shown"
        } else {
            "hidden"
        }
    );
    print_stop_sequences(&stats.stop_sequences);
    println!(
        "      Total tokens: {} in / {} out ({} requests)",
        stats.total_input_tokens, stats.total_output_tokens, stats.total_requests
    );
    if let Some(input) = stats.last_turn_input_tokens {
        let output = stats.last_turn_output_tokens.unwrap_or(0);
        println!("      Last turn tokens: {input} in / {output} out");
    }
    if let Some(limit) = stats.session_budget_tokens {
        let remaining = limit.saturating_sub(stats.budget_spent_tokens);
        println!(
            "      Budget: {}/{} tokens ({} remaining)",
            stats.budget_spent_tokens, limit, remaining
        );
    } else {
        println!("      Budget: (not set)");
    }
    match stats.transcript_path {
        Some(ref path) => println!("      Transcript file: {}", path.display()),
        None => println!("      Transcript file: (disabled)"),
    }
}

fn print_config(session: &ChatSession) {
    let stats = session.stats();
    println!("    Current Configuration:");
    println!("      Model: {}", stats.model);
    println!("      Max tokens: {}", stats.max_tokens);
    println!("      Temperature: {}", describe_float(stats.temperature));
    println!("      Top-p: {}", describe_float(stats.top_p));
    println!("      Top-k: {}", describe_top_k(stats.top_k));
    println!(
        "      Thinking output: {}",
        if stats.show_thinking {
            "shown"
        } else {
            "hidden"
        }
    );
    if let Some(prompt) = stats.system_prompt.as_deref() {
        println!("      System prompt: {}", prompt);
    } else {
        println!("      System prompt: (none)");
    }
    print_stop_sequences(&stats.stop_sequences);
    match stats.transcript_path {
        Some(ref path) => println!("      Transcript file: {}", path.display()),
        None => println!("      Transcript file: (disabled)"),
    }
}

fn print_stop_sequences(stop_sequences: &[String]) {
    if stop_sequences.is_empty() {
        println!("      Stop sequences: (none)");
    } else {
        println!("      Stop sequences:");
        for seq in stop_sequences {
            println!("        - {}", seq);
        }
    }
}

fn describe_float(value: Option<f32>) -> String {
    value
        .map(|v| format!("{v:.2}"))
        .unwrap_or_else(|| "default".to_string())
}

fn describe_top_k(value: Option<u32>) -> String {
    value
        .map(|v| v.to_string())
        .unwrap_or_else(|| "default".to_string())
}
