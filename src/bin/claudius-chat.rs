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

use claudius::Renderer;
use claudius::chat::{
    ChatAgent, ChatArgs, ChatCommand, ChatConfig, ChatSession, PlainTextRenderer, help_text,
    parse_command,
};
use claudius::{Anthropic, Model, SystemPrompt, ThinkingConfig};

/// Main entry point for the claudius-chat application.
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (args, _) = ChatArgs::from_command_line_relaxed("claudius-chat [OPTIONS]");
    let config = ChatConfig::from(args);
    let use_color = config.use_color;

    let client = Anthropic::new(None)?;
    let mut session = ChatSession::new(client, config);
    let mut rl = DefaultEditor::new()?;

    // Flag for interrupt handling during streaming
    let interrupted = Arc::new(AtomicBool::new(false));
    let mut renderer = PlainTextRenderer::with_color_and_interrupt(use_color, interrupted.clone());
    let context = ();

    // Set up Ctrl+C handler
    let interrupted_clone = interrupted.clone();
    ctrlc::set_handler(move || {
        interrupted_clone.store(true, Ordering::Relaxed);
    })?;

    println!("Claude Chat (model: {})", session.config().model());
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
                            renderer.print_info(&context, "Conversation cleared.");
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
                            session.template_mut().model = Some(model);
                            renderer
                                .print_info(&context, &format!("Model changed to: {}", model_name));
                        }
                        ChatCommand::System(prompt) => {
                            session.template_mut().system = prompt.clone().map(SystemPrompt::from);
                            match prompt {
                                Some(p) => renderer
                                    .print_info(&context, &format!("System prompt set to: {}", p)),
                                None => renderer.print_info(&context, "System prompt cleared."),
                            }
                        }
                        ChatCommand::MaxTokens(value) => {
                            session.template_mut().max_tokens = Some(value);
                            renderer.print_info(&context, &format!("max_tokens set to {value}"));
                        }
                        ChatCommand::Temperature(value) => {
                            session.template_mut().temperature = Some(value);
                            renderer
                                .print_info(&context, &format!("temperature set to {:.2}", value));
                        }
                        ChatCommand::ClearTemperature => {
                            session.template_mut().temperature = None;
                            renderer.print_info(&context, "temperature reset to model default");
                        }
                        ChatCommand::TopP(value) => {
                            session.template_mut().top_p = Some(value);
                            renderer.print_info(&context, &format!("top_p set to {:.2}", value));
                        }
                        ChatCommand::ClearTopP => {
                            session.template_mut().top_p = None;
                            renderer.print_info(&context, "top_p reset to model default");
                        }
                        ChatCommand::TopK(value) => {
                            session.template_mut().top_k = Some(value);
                            renderer.print_info(&context, &format!("top_k set to {value}"));
                        }
                        ChatCommand::ClearTopK => {
                            session.template_mut().top_k = None;
                            renderer.print_info(&context, "top_k reset to model default");
                        }
                        ChatCommand::AddStopSequence(sequence) => {
                            let stop_sequences = session
                                .template_mut()
                                .stop_sequences
                                .get_or_insert_with(Vec::new);
                            if !stop_sequences.iter().any(|s| s == &sequence) {
                                stop_sequences.push(sequence.clone());
                            }
                            renderer
                                .print_info(&context, &format!("Added stop sequence: {sequence}"));
                        }
                        ChatCommand::ClearStopSequences => {
                            session.template_mut().stop_sequences = None;
                            renderer.print_info(&context, "Stop sequences cleared.");
                        }
                        ChatCommand::ListStopSequences => {
                            let sequences =
                                session.template().stop_sequences.as_deref().unwrap_or(&[]);
                            print_stop_sequences(sequences);
                        }
                        ChatCommand::Thinking(budget) => {
                            session.template_mut().thinking = budget.map(ThinkingConfig::enabled);
                            match budget {
                                Some(tokens) => {
                                    renderer.print_info(
                                        &context,
                                        &format!(
                                            "Extended thinking enabled with {} token budget.",
                                            tokens
                                        ),
                                    );
                                }
                                None => {
                                    renderer.print_info(&context, "Extended thinking disabled.");
                                }
                            }
                        }
                        ChatCommand::Budget(_tokens) => {
                            renderer.print_error(&context, "budget not supported");
                        }
                        ChatCommand::ClearBudget => {
                            session.config_mut().session_budget = None;
                            renderer.print_info(&context, "Session budget cleared.");
                        }
                        ChatCommand::Caching(enabled) => {
                            session.config_mut().caching_enabled = enabled;
                            if enabled {
                                renderer.print_info(&context, "Prompt caching enabled.");
                            } else {
                                renderer.print_info(&context, "Prompt caching disabled.");
                            }
                        }
                        ChatCommand::TranscriptPath(path) => {
                            session.config_mut().transcript_path = Some(PathBuf::from(&path));
                            renderer.print_info(
                                &context,
                                &format!("Transcript auto-save set to {}", path),
                            );
                        }
                        ChatCommand::ClearTranscriptPath => {
                            session.config_mut().transcript_path = None;
                            renderer.print_info(&context, "Transcript auto-save disabled.");
                        }
                        ChatCommand::SaveTranscript(path) => {
                            match session.save_transcript_to(&path) {
                                Ok(_) => renderer
                                    .print_info(&context, &format!("Transcript saved to {}", path)),
                                Err(err) => renderer.print_error(
                                    &context,
                                    &format!("Failed to save transcript: {}", err),
                                ),
                            }
                        }
                        ChatCommand::LoadTranscript(path) => {
                            match session.load_transcript_from(&path) {
                                Ok(_) => renderer.print_info(
                                    &context,
                                    &format!("Transcript loaded from {}", path),
                                ),
                                Err(err) => renderer.print_error(
                                    &context,
                                    &format!("Failed to load transcript: {}", err),
                                ),
                            }
                        }
                        ChatCommand::Stats => {
                            print_stats(&session);
                        }
                        ChatCommand::ShowConfig => {
                            print_config(&session);
                        }
                        ChatCommand::Invalid(message) => {
                            renderer.print_error(&context, &message);
                        }
                    }
                    continue;
                }

                // Regular message - send to API
                println!("Claude:");
                let message = claudius::MessageParam::user(line);
                if let Err(e) = session.send_message(message, &mut renderer).await {
                    renderer.print_error(&context, &e.to_string());
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
                renderer.print_error(&context, &format!("Input error: {}", err));
                break;
            }
        }
    }

    Ok(())
}

fn print_stats<A: ChatAgent>(session: &ChatSession<A>) {
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
        "      Thinking: {}",
        match stats.thinking_budget {
            Some(budget) => format!("enabled ({} tokens)", budget),
            None => "disabled".to_string(),
        }
    );
    print_stop_sequences(&stats.stop_sequences);
    println!(
        "      Total tokens: {} in / {} out ({} requests)",
        stats.total_input_tokens, stats.total_output_tokens, stats.total_requests
    );
    if stats.caching_enabled {
        println!(
            "      Cache tokens: {} created / {} read",
            stats.total_cache_creation_tokens, stats.total_cache_read_tokens
        );
    }
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

fn print_config<A: ChatAgent>(session: &ChatSession<A>) {
    let stats = session.stats();
    println!("    Current Configuration:");
    println!("      Model: {}", stats.model);
    println!("      Max tokens: {}", stats.max_tokens);
    println!("      Temperature: {}", describe_float(stats.temperature));
    println!("      Top-p: {}", describe_float(stats.top_p));
    println!("      Top-k: {}", describe_top_k(stats.top_k));
    println!(
        "      Thinking: {}",
        match stats.thinking_budget {
            Some(budget) => format!("enabled ({} tokens)", budget),
            None => "disabled".to_string(),
        }
    );
    println!(
        "      Caching: {}",
        if stats.caching_enabled {
            "enabled"
        } else {
            "disabled"
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
