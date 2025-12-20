//! Slash command parsing for the chat application.
//!
//! This module handles parsing of special commands that start with `/`,
//! allowing users to control the chat session without sending messages
//! to the API.

/// A parsed chat command.
///
/// These commands control the chat session and are not sent to the API.
#[derive(Debug, Clone, PartialEq)]
pub enum ChatCommand {
    /// Clear the conversation history.
    Clear,

    /// Change the model.
    Model(String),

    /// Set or clear the system prompt.
    /// `None` clears the current system prompt.
    System(Option<String>),

    /// Set the maximum tokens per response.
    MaxTokens(u32),

    /// Set the sampling temperature.
    Temperature(f32),

    /// Clear the sampling temperature (use model default).
    ClearTemperature,

    /// Set the top-p value.
    TopP(f32),

    /// Clear the top-p value.
    ClearTopP,

    /// Set the top-k value.
    TopK(u32),

    /// Clear the top-k value.
    ClearTopK,

    /// Add a stop sequence.
    AddStopSequence(String),

    /// Clear all stop sequences.
    ClearStopSequences,

    /// List stop sequences.
    ListStopSequences,

    /// Toggle thinking visibility.
    Thinking(bool),

    /// Set a per-session token budget.
    Budget(u64),

    /// Clear the token budget.
    ClearBudget,

    /// Set the auto-save transcript path.
    TranscriptPath(String),

    /// Clear the auto-save transcript path.
    ClearTranscriptPath,

    /// Save the transcript to a specific file immediately.
    SaveTranscript(String),

    /// Load conversation history from a file.
    LoadTranscript(String),

    /// Display help information.
    Help,

    /// Exit the chat application.
    Quit,

    /// Display session statistics (message count, current model, etc.).
    Stats,

    /// Show the current configuration.
    ShowConfig,

    /// Report a parsing error back to the caller.
    Invalid(String),
}

/// Parses user input for slash commands.
///
/// Returns `Some(ChatCommand)` if the input is a valid command,
/// or `None` if it should be treated as a regular message.
///
/// # Examples
///
/// ```
/// # use claudius::chat::parse_command;
/// assert!(parse_command("/quit").is_some());
/// assert!(parse_command("/model claude-sonnet-4-0").is_some());
/// assert!(parse_command("Hello, Claude!").is_none());
/// ```
pub fn parse_command(input: &str) -> Option<ChatCommand> {
    let input = input.trim();

    if !input.starts_with('/') {
        return None;
    }

    let mut parts = input[1..].splitn(2, ' ');
    let command = parts.next()?.to_lowercase();
    let argument = parts.next().map(|s| s.trim()).filter(|s| !s.is_empty());

    let result = match command.as_str() {
        "clear" => ChatCommand::Clear,
        "model" => match argument {
            Some(model) => ChatCommand::Model(model.to_string()),
            None => ChatCommand::Invalid("/model requires a model name".to_string()),
        },
        "system" => ChatCommand::System(argument.map(|s| s.to_string())),
        "help" | "?" => ChatCommand::Help,
        "quit" | "exit" | "q" => ChatCommand::Quit,
        "stats" | "status" => ChatCommand::Stats,
        "config" => ChatCommand::ShowConfig,
        "max_tokens" => parse_u32_command(argument, ChatCommand::MaxTokens, "/max_tokens"),
        "temperature" => match argument {
            Some(arg) if arg.eq_ignore_ascii_case("clear") => ChatCommand::ClearTemperature,
            Some(arg) => match parse_f32_in_range(arg, 0.0, 1.0) {
                Ok(value) => ChatCommand::Temperature(value),
                Err(err) => ChatCommand::Invalid(format!("/temperature {err}")),
            },
            None => ChatCommand::Invalid("/temperature requires a value".to_string()),
        },
        "top_p" => match argument {
            Some(arg) if arg.eq_ignore_ascii_case("clear") => ChatCommand::ClearTopP,
            Some(arg) => match parse_f32_in_range(arg, 0.0, 1.0) {
                Ok(value) => ChatCommand::TopP(value),
                Err(err) => ChatCommand::Invalid(format!("/top_p {err}")),
            },
            None => ChatCommand::Invalid("/top_p requires a value".to_string()),
        },
        "top_k" => match argument {
            Some(arg) if arg.eq_ignore_ascii_case("clear") => ChatCommand::ClearTopK,
            Some(arg) => match arg.parse::<u32>() {
                Ok(value) => ChatCommand::TopK(value),
                Err(_) => ChatCommand::Invalid("/top_k expects a positive integer".to_string()),
            },
            None => ChatCommand::Invalid("/top_k requires a value".to_string()),
        },
        "stop" => parse_stop_command(argument),
        "thinking" => match argument.and_then(parse_on_off) {
            Some(value) => ChatCommand::Thinking(value),
            None => ChatCommand::Invalid("/thinking expects 'on' or 'off'".to_string()),
        },
        "budget" => match argument {
            Some(arg) if arg.eq_ignore_ascii_case("clear") => ChatCommand::ClearBudget,
            Some(arg) => match arg.parse::<u64>() {
                Ok(value) => ChatCommand::Budget(value),
                Err(_) => {
                    ChatCommand::Invalid("/budget expects an integer token count".to_string())
                }
            },
            None => ChatCommand::Invalid("/budget requires a value".to_string()),
        },
        "transcript" => match argument {
            Some(arg) if arg.eq_ignore_ascii_case("clear") => ChatCommand::ClearTranscriptPath,
            Some(arg) => ChatCommand::TranscriptPath(arg.to_string()),
            None => ChatCommand::Invalid("/transcript requires a file path".to_string()),
        },
        "save" => match argument {
            Some(arg) => ChatCommand::SaveTranscript(arg.to_string()),
            None => ChatCommand::Invalid("/save requires a file path".to_string()),
        },
        "load" => match argument {
            Some(arg) => ChatCommand::LoadTranscript(arg.to_string()),
            None => ChatCommand::Invalid("/load requires a file path".to_string()),
        },
        _ => ChatCommand::Invalid(format!("Unknown command: /{}", command)),
    };

    Some(result)
}

fn parse_stop_command(argument: Option<&str>) -> ChatCommand {
    let Some(arg) = argument else {
        return ChatCommand::Invalid(
            "/stop requires 'add <sequence>', 'clear', or 'list'".to_string(),
        );
    };

    let mut parts = arg.splitn(2, ' ');
    let action = parts.next().unwrap();
    match action.to_lowercase().as_str() {
        "add" => {
            let Some(sequence) = parts.next().map(|s| s.trim()).filter(|s| !s.is_empty()) else {
                return ChatCommand::Invalid("/stop add requires a sequence".to_string());
            };
            ChatCommand::AddStopSequence(sequence.to_string())
        }
        "clear" => ChatCommand::ClearStopSequences,
        "list" => ChatCommand::ListStopSequences,
        _ => {
            ChatCommand::Invalid("Unrecognized /stop action (use add, clear, or list)".to_string())
        }
    }
}

fn parse_u32_command<F>(argument: Option<&str>, constructor: F, name: &str) -> ChatCommand
where
    F: Fn(u32) -> ChatCommand,
{
    match argument {
        Some(arg) => match arg.parse::<u32>() {
            Ok(value) => constructor(value),
            Err(_) => ChatCommand::Invalid(format!("{} expects a positive integer", name)),
        },
        None => ChatCommand::Invalid(format!("{} requires a value", name)),
    }
}

fn parse_f32_in_range(value: &str, min: f32, max: f32) -> Result<f32, String> {
    let parsed: f32 = value
        .parse()
        .map_err(|_| format!("expects a value between {min} and {max}"))?;
    if parsed.is_finite() && parsed >= min && parsed <= max {
        Ok(parsed)
    } else {
        Err(format!("expects a value between {min} and {max}"))
    }
}

fn parse_on_off(value: &str) -> Option<bool> {
    match value.to_lowercase().as_str() {
        "on" | "true" | "yes" => Some(true),
        "off" | "false" | "no" => Some(false),
        _ => None,
    }
}

/// Returns help text describing available commands.
pub fn help_text() -> &'static str {
    r#"Available commands:
  /clear                 Clear conversation history
  /model <name>          Change the model (e.g., /model claude-sonnet-4-0)
  /system [prompt]       Set system prompt (no argument clears it)
  /max_tokens <n>        Set maximum response tokens
  /temperature <v>       Set temperature 0.0-1.0 (use 'clear' to reset)
  /top_p <v>             Set top-p 0.0-1.0 (use 'clear' to reset)
  /top_k <n>             Set top-k (use 'clear' to reset)
  /stop add <seq>        Add a stop sequence
  /stop clear            Clear all stop sequences
  /stop list             List current stop sequences
  /thinking on|off       Show or hide thinking blocks
  /budget <tokens>       Set total session budget (or 'clear')
  /transcript <file>     Enable auto-saving transcripts (or 'clear')
  /save <file>           Save the current transcript immediately
  /load <file>           Load a transcript from disk
  /stats                 Show session statistics
  /config                Show current configuration
  /help                  Show this help message
  /quit                  Exit the chat"#
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_quit_commands() {
        assert_eq!(parse_command("/quit"), Some(ChatCommand::Quit));
        assert_eq!(parse_command("/exit"), Some(ChatCommand::Quit));
        assert_eq!(parse_command("/q"), Some(ChatCommand::Quit));
        assert_eq!(parse_command("  /quit  "), Some(ChatCommand::Quit));
    }

    #[test]
    fn parse_clear() {
        assert_eq!(parse_command("/clear"), Some(ChatCommand::Clear));
        assert_eq!(parse_command("/CLEAR"), Some(ChatCommand::Clear));
    }

    #[test]
    fn parse_model() {
        assert_eq!(
            parse_command("/model claude-sonnet-4-0"),
            Some(ChatCommand::Model("claude-sonnet-4-0".to_string()))
        );
        assert_eq!(
            parse_command("/model   claude-haiku-4-5  "),
            Some(ChatCommand::Model("claude-haiku-4-5".to_string()))
        );
        assert_eq!(
            parse_command("/model"),
            Some(ChatCommand::Invalid(
                "/model requires a model name".to_string()
            ))
        );
    }

    #[test]
    fn parse_system() {
        assert_eq!(
            parse_command("/system You are a helpful assistant"),
            Some(ChatCommand::System(Some(
                "You are a helpful assistant".to_string()
            )))
        );
        assert_eq!(parse_command("/system"), Some(ChatCommand::System(None)));
    }

    #[test]
    fn parse_temperature() {
        assert_eq!(
            parse_command("/temperature 0.5"),
            Some(ChatCommand::Temperature(0.5))
        );
        assert_eq!(
            parse_command("/temperature clear"),
            Some(ChatCommand::ClearTemperature)
        );
        assert!(matches!(
            parse_command("/temperature"),
            Some(ChatCommand::Invalid(msg)) if msg.contains("requires")
        ));
    }

    #[test]
    fn parse_stop_commands() {
        assert_eq!(
            parse_command("/stop add END"),
            Some(ChatCommand::AddStopSequence("END".to_string()))
        );
        assert_eq!(
            parse_command("/stop clear"),
            Some(ChatCommand::ClearStopSequences)
        );
        assert_eq!(
            parse_command("/stop list"),
            Some(ChatCommand::ListStopSequences)
        );
    }

    #[test]
    fn parse_thinking_toggle() {
        assert_eq!(
            parse_command("/thinking on"),
            Some(ChatCommand::Thinking(true))
        );
        assert_eq!(
            parse_command("/thinking off"),
            Some(ChatCommand::Thinking(false))
        );
        assert!(matches!(
            parse_command("/thinking maybe"),
            Some(ChatCommand::Invalid(msg)) if msg.contains("expects")
        ));
    }

    #[test]
    fn parse_budget() {
        assert_eq!(
            parse_command("/budget 1000"),
            Some(ChatCommand::Budget(1000))
        );
        assert_eq!(
            parse_command("/budget clear"),
            Some(ChatCommand::ClearBudget)
        );
    }

    #[test]
    fn parse_transcript_commands() {
        assert_eq!(
            parse_command("/transcript chat.json"),
            Some(ChatCommand::TranscriptPath("chat.json".to_string()))
        );
        assert_eq!(
            parse_command("/transcript clear"),
            Some(ChatCommand::ClearTranscriptPath)
        );
        assert_eq!(
            parse_command("/save session.json"),
            Some(ChatCommand::SaveTranscript("session.json".to_string()))
        );
        assert_eq!(
            parse_command("/load session.json"),
            Some(ChatCommand::LoadTranscript("session.json".to_string()))
        );
    }

    #[test]
    fn parse_stats_and_config() {
        assert_eq!(parse_command("/stats"), Some(ChatCommand::Stats));
        assert_eq!(parse_command("/config"), Some(ChatCommand::ShowConfig));
    }

    #[test]
    fn non_commands() {
        assert_eq!(parse_command("Hello, Claude!"), None);
        assert_eq!(parse_command(""), None);
        assert_eq!(parse_command("  "), None);
    }

    #[test]
    fn help_text_not_empty() {
        let help = help_text();
        assert!(!help.is_empty());
        assert!(help.contains("/quit"));
        assert!(help.contains("/clear"));
        assert!(help.contains("/model"));
        assert!(help.contains("/temperature"));
    }
}
