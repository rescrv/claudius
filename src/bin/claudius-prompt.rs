//! Command-line tool for running Anthropic API prompt tests.
//!
//! This binary provides a convenient way to test prompts against the Anthropic API
//! using either plain text files or YAML configuration files with advanced testing features.
//!
//! # Usage
//!
//! ```bash
//! # Run a simple text file as a prompt
//! claudius-prompt my_prompt.txt
//!
//! # Run a YAML configuration file with test assertions
//! claudius-prompt test_config.yaml
//!
//! # Run multiple files and get JSON output
//! claudius-prompt --format json file1.txt file2.yaml
//!
//! # Run in test mode (exit with status code based on assertion results)
//! claudius-prompt --test my_test.yaml
//! ```
//!
//! # File Types
//!
//! - **Text files** (`.txt`): Treated as simple prompts sent to the API
//! - **YAML files** (`.yaml`, `.yml`): Advanced test configurations with assertions and parameters

use arrrg::CommandLine;
use arrrg_derive::CommandLine;
use claudius::{Anthropic, PromptTestConfig};

/// Output format for displaying test results.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
enum OutputFormat {
    /// Plain text format (default) - human-readable output.
    #[default]
    Text,
    /// JSON format - structured output suitable for parsing.
    Json,
    /// YAML format - structured output in YAML format.
    Yaml,
}

impl std::fmt::Display for OutputFormat {
    /// Format the output format as its string representation.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OutputFormat::Text => write!(f, "text"),
            OutputFormat::Json => write!(f, "json"),
            OutputFormat::Yaml => write!(f, "yaml"),
        }
    }
}

impl std::str::FromStr for OutputFormat {
    type Err = String;

    /// Parse an output format from its string representation.
    ///
    /// Accepts "text", "json", "yaml", or "yml" (case-insensitive).
    ///
    /// # Errors
    ///
    /// Returns an error string if the format is not recognized.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "text" => Ok(OutputFormat::Text),
            "json" => Ok(OutputFormat::Json),
            "yaml" | "yml" => Ok(OutputFormat::Yaml),
            _ => Err(format!(
                "Invalid output format: {}. Valid options: text, json, yaml",
                s
            )),
        }
    }
}

/// Command-line arguments for the claudius-prompt tool.
#[derive(CommandLine, Debug, Default, PartialEq, Eq)]
struct Args {
    /// Output format for results (text, json, yaml).
    #[arrrg(optional, "Output format: text, json, yaml", "FORMAT")]
    format: Option<String>,

    /// Test mode - run assertions and exit with appropriate status code.
    ///
    /// When enabled, the program will exit with code 0 if all tests pass,
    /// or code 1 if any test fails. Useful for CI/CD integration.
    #[arrrg(flag, "Test mode - run assertions and exit with status code")]
    test: bool,

    /// Include verbose output with timing and token usage information.
    #[arrrg(flag, "Include timing and token usage information")]
    verbose: bool,
}

/// Main entry point for the claudius-prompt command-line tool.
///
/// Processes command-line arguments, loads prompt files or test configurations,
/// executes them against the Anthropic API, and outputs results in the requested format.
///
/// # File Processing
///
/// - Files ending in `.yaml` or `.yml` are treated as test configurations
/// - All other files are treated as plain text prompts
///
/// # Output Formats
///
/// - **Text**: Human-readable output with optional verbose information
/// - **JSON**: Structured JSON output suitable for programmatic processing
/// - **YAML**: Structured YAML output
///
/// # Test Mode
///
/// When `--test` is specified, the program runs assertions defined in the test
/// configurations and exits with:
/// - Exit code 0: All tests passed
/// - Exit code 1: One or more tests failed
///
/// # Examples
///
/// ```bash
/// # Basic prompt execution
/// claudius-prompt hello.txt
///
/// # Run test with verbose output
/// claudius-prompt --verbose test.yaml
///
/// # Test mode for CI/CD
/// claudius-prompt --test integration_tests.yaml
///
/// # JSON output for processing
/// claudius-prompt --format json results.yaml
/// ```
///
/// # Errors
///
/// Returns an error if:
/// - No files are specified
/// - File loading fails
/// - API authentication fails
/// - Invalid output format is specified
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (args, files) = Args::from_command_line_relaxed("claudius-prompt [OPTIONS] <FILES>...");

    if files.is_empty() {
        eprintln!("Error: Must specify at least one prompt file or config file");
        std::process::exit(1);
    }

    let client = Anthropic::new(None)?;
    let output_format = if let Some(format_str) = args.format {
        format_str
            .parse()
            .map_err(|e| format!("Invalid format: {}", e))?
    } else {
        OutputFormat::Text
    };
    let mut all_passed = true;
    let mut failed_files = Vec::new();

    for (i, file_path) in files.iter().enumerate() {
        let test_config = if file_path.ends_with(".yaml") || file_path.ends_with(".yml") {
            // Load from YAML config file
            PromptTestConfig::from_file(file_path)?
        } else {
            // Treat as prompt text file - read directly
            let prompt_text = std::fs::read_to_string(file_path)?;
            PromptTestConfig::new(prompt_text).with_name(file_path.clone())
        };

        // Run the test
        let result = test_config.run(&client).await?;

        if !result.assertions_passed {
            all_passed = false;
            failed_files.push((file_path.clone(), result.assertion_failures.len()));
        }

        // Output result immediately based on format
        match output_format {
            OutputFormat::Text => {
                if files.len() > 1 {
                    println!("=== {} ===", file_path);
                }

                if args.verbose {
                    if let Some(ref name) = result.config.name {
                        println!("Test: {}", name);
                    }
                    println!(
                        "Model: {}",
                        result.config.model.as_deref().unwrap_or("default")
                    );
                    println!("Duration: {:?}", result.duration);
                    println!("Input tokens: {}", result.input_tokens);
                    println!("Output tokens: {}", result.output_tokens);
                    if !result.assertion_failures.is_empty() {
                        println!("Assertion failures:");
                        for failure in &result.assertion_failures {
                            println!("  - {}", failure);
                        }
                    }
                    println!("---");
                }
                println!("{}", result.response);

                if files.len() > 1 && i < files.len() - 1 {
                    println!();
                }
            }
            OutputFormat::Json => {
                let json = serde_json::to_string_pretty(&result)?;
                println!("{}", json);
                if i < files.len() - 1 {
                    println!();
                }
            }
            OutputFormat::Yaml => {
                let yaml = serde_yaml::to_string(&result)?;
                print!("{}", yaml);
                if i < files.len() - 1 {
                    println!("---");
                }
            }
        }
    }

    // Exit with appropriate status code in test mode
    if args.test {
        if all_passed {
            std::process::exit(0);
        } else {
            eprintln!(
                "Tests failed: {}/{} files had assertion failures",
                failed_files.len(),
                files.len()
            );
            for (file_path, failure_count) in &failed_files {
                eprintln!("  {}: {} failures", file_path, failure_count);
            }
            std::process::exit(1);
        }
    }

    Ok(())
}
