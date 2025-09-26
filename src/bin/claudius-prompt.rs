use arrrg::CommandLine;
use arrrg_derive::CommandLine;
use claudius::{Anthropic, PromptTestConfig};

#[derive(Debug, Clone, PartialEq, Eq, Default)]
enum OutputFormat {
    #[default]
    Text,
    Json,
    Yaml,
}

impl std::fmt::Display for OutputFormat {
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

#[derive(CommandLine, Debug, Default, PartialEq, Eq)]
struct Args {
    #[arrrg(optional, "Output format: text, json, yaml", "FORMAT")]
    format: Option<String>,

    #[arrrg(flag, "Test mode - run assertions and exit with status code")]
    test: bool,

    #[arrrg(flag, "Include timing and token usage information")]
    verbose: bool,
}

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
