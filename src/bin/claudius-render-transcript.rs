//! Render claudius-chat saved transcripts as markdown.
//!
//! `claudius-chat` persists transcripts as JSON containing a version and a
//! sequence of Anthropic message parameters. This tool turns that saved JSON
//! into markdown suitable for reading, archiving, or further processing.

use std::fmt::Write as _;
use std::io::Read as _;
use std::path::PathBuf;

use claudius::{
    Content, ContentBlock, ContentBlockSourceContent, DocumentSource, ImageMediaType, ImageSource,
    MessageParam, MessageParamContent, MessageRole, ToolResultBlockContent,
    WebSearchToolResultBlockContent,
};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct TranscriptFile {
    version: u8,
    messages: Vec<MessageParam>,
}

#[derive(Debug, Default, PartialEq, Eq)]
struct Args {
    input: Option<PathBuf>,
    output: Option<PathBuf>,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(message) => {
            eprintln!("{message}");
            eprintln!();
            eprintln!("{}", usage());
            std::process::exit(2);
        }
    };

    let input = read_input(args.input.as_ref())?;
    let transcript: TranscriptFile = serde_json::from_slice(&input)?;
    if transcript.version != 1 {
        return Err(format!("unsupported transcript version: {}", transcript.version).into());
    }

    let markdown = render_transcript(&transcript);
    match args.output {
        Some(path) => std::fs::write(path, markdown)?,
        None => print!("{markdown}"),
    }

    Ok(())
}

fn parse_args<I, S>(args: I) -> Result<Args, String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut parsed = Args::default();
    let mut args = args.into_iter().map(Into::into).peekable();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                println!("{}", usage());
                std::process::exit(0);
            }
            "-o" | "--output" => {
                let Some(path) = args.next() else {
                    return Err(format!("{arg} requires a path"));
                };
                parsed.output = Some(PathBuf::from(path));
            }
            _ if arg.starts_with("--output=") => {
                parsed.output = Some(PathBuf::from(&arg["--output=".len()..]));
            }
            _ if arg.starts_with('-') && arg != "-" => {
                return Err(format!("unknown option: {arg}"));
            }
            _ => {
                if parsed.input.is_some() {
                    return Err(format!("unexpected extra argument: {arg}"));
                }
                if arg != "-" {
                    parsed.input = Some(PathBuf::from(arg));
                }
            }
        }
    }
    Ok(parsed)
}

fn usage() -> &'static str {
    "Usage: claudius-render-transcript [OPTIONS] [TRANSCRIPT]\n\
\n\
Options:\n\
  -o, --output PATH    Write markdown to PATH instead of stdout\n\
  -h, --help           Show this help text\n\
\n\
Reads TRANSCRIPT as a claudius-chat saved JSON transcript. If TRANSCRIPT is\n\
omitted or '-', reads from stdin."
}

fn read_input(input: Option<&PathBuf>) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    match input {
        Some(path) => Ok(std::fs::read(path)?),
        None => {
            let mut buffer = Vec::new();
            std::io::stdin().read_to_end(&mut buffer)?;
            Ok(buffer)
        }
    }
}

fn render_transcript(transcript: &TranscriptFile) -> String {
    let mut out = String::new();
    out.push_str("# Transcript\n\n");
    for message in &transcript.messages {
        render_message(&mut out, message);
    }
    out
}

fn render_message(out: &mut String, message: &MessageParam) {
    match message.role {
        MessageRole::User => out.push_str("## User\n\n"),
        MessageRole::Assistant => out.push_str("## Assistant\n\n"),
        MessageRole::System => out.push_str("## System\n\n"),
    }

    match &message.content {
        MessageParamContent::String(text) => render_markdown_text(out, text),
        MessageParamContent::Array(blocks) => {
            for block in blocks {
                render_content_block(out, block);
            }
        }
    }
}

fn render_content_block(out: &mut String, block: &ContentBlock) {
    match block {
        ContentBlock::Text(block) => render_markdown_text(out, &block.text),
        ContentBlock::Image(block) => render_image(out, &block.source),
        ContentBlock::ToolUse(block) => {
            let _ = writeln!(out, "### Tool Use: {}", block.name);
            let _ = writeln!(out);
            let _ = writeln!(out, "ID: `{}`", block.id);
            let _ = writeln!(out);
            let input = pretty_json(&block.input);
            write_fenced(out, "json", &input);
        }
        ContentBlock::ServerToolUse(block) => {
            let _ = writeln!(out, "### Server Tool Use: {}", block.name);
            let _ = writeln!(out);
            let _ = writeln!(out, "ID: `{}`", block.id);
            let _ = writeln!(out);
            let input = pretty_json(&block.input);
            write_fenced(out, "json", &input);
        }
        ContentBlock::WebSearchToolResult(block) => {
            let _ = writeln!(out, "### Web Search Result");
            let _ = writeln!(out);
            let _ = writeln!(out, "Tool use ID: `{}`", block.tool_use_id);
            let _ = writeln!(out);
            match &block.content {
                WebSearchToolResultBlockContent::Results(results) => {
                    if results.is_empty() {
                        out.push_str("_No results._\n\n");
                    } else {
                        for result in results {
                            let _ = write!(out, "- [{}]({})", result.title, result.url);
                            if let Some(page_age) = &result.page_age {
                                let _ = write!(out, " ({page_age})");
                            }
                            let _ = writeln!(out);
                        }
                        let _ = writeln!(out);
                    }
                }
                WebSearchToolResultBlockContent::Error(error) => {
                    let _ = writeln!(out, "Error: `{}`", error.error_code);
                    let _ = writeln!(out);
                }
            }
        }
        ContentBlock::ToolResult(block) => {
            let _ = writeln!(out, "### Tool Result");
            let _ = writeln!(out);
            let _ = writeln!(out, "Tool use ID: `{}`", block.tool_use_id);
            if matches!(block.is_error, Some(true)) {
                out.push_str("Error: `true`\n");
            }
            let _ = writeln!(out);
            if let Some(content) = &block.content {
                render_tool_result_content(out, content);
            } else {
                out.push_str("_No content._\n\n");
            }
        }
        ContentBlock::Document(block) => render_document(out, block),
        ContentBlock::Thinking(block) => {
            out.push_str("<details>\n<summary>Thinking</summary>\n\n");
            write_fenced(out, "text", &block.thinking);
            out.push_str("</details>\n\n");
        }
        ContentBlock::RedactedThinking(block) => {
            out.push_str("<details>\n<summary>Redacted Thinking</summary>\n\n");
            write_fenced(out, "text", &block.data);
            out.push_str("</details>\n\n");
        }
        ContentBlock::Fallback(block) => {
            let _ = writeln!(
                out,
                "_Model fallback: `{}` → `{}`_",
                block.from.model, block.to.model
            );
            let _ = writeln!(out);
        }
    }
}

fn render_tool_result_content(out: &mut String, content: &ToolResultBlockContent) {
    match content {
        ToolResultBlockContent::String(text) => write_fenced(out, "text", text),
        ToolResultBlockContent::Array(items) => {
            for item in items {
                render_content_item(out, item);
            }
        }
    }
}

fn render_document(out: &mut String, block: &claudius::DocumentBlock) {
    match &block.title {
        Some(title) => {
            let _ = writeln!(out, "### Document: {title}");
        }
        None => out.push_str("### Document\n"),
    }
    let _ = writeln!(out);

    if let Some(context) = &block.context {
        out.push_str("Context:\n\n");
        write_fenced(out, "text", context);
    }

    match &block.source {
        DocumentSource::Base64Pdf(source) => {
            let uri = data_uri(&source.media_type, &source.data);
            let _ = writeln!(out, "[PDF document]({uri})\n");
        }
        DocumentSource::PlainText(source) => {
            write_fenced(out, "text", &source.data);
        }
        DocumentSource::ContentBlock(source) => match &source.content {
            ContentBlockSourceContent::String(text) => write_fenced(out, "text", text),
            ContentBlockSourceContent::Array(items) => {
                for item in items {
                    render_content_item(out, item);
                }
            }
        },
        DocumentSource::UrlPdf(source) => {
            let _ = writeln!(out, "[PDF document]({})\n", source.url);
        }
    }
}

fn render_content_item(out: &mut String, item: &Content) {
    match item {
        Content::Text(block) => render_markdown_text(out, &block.text),
        Content::Image(block) => render_image(out, &block.source),
    }
}

fn render_image(out: &mut String, source: &ImageSource) {
    match source {
        ImageSource::Base64(source) => {
            let media_type = image_media_type(&source.media_type);
            let uri = data_uri(media_type, &source.data);
            let _ = writeln!(out, "![Image]({uri})\n");
        }
        ImageSource::Url(source) => {
            let _ = writeln!(out, "![Image]({})\n", source.url);
        }
    }
}

fn render_markdown_text(out: &mut String, text: &str) {
    out.push_str(text);
    ensure_blank_line(out);
}

fn ensure_blank_line(out: &mut String) {
    if out.ends_with("\n\n") {
        return;
    }
    if out.ends_with('\n') {
        out.push('\n');
    } else {
        out.push_str("\n\n");
    }
}

fn write_fenced(out: &mut String, language: &str, text: &str) {
    let fence_len = max_backtick_run(text).saturating_add(1).max(3);
    let fence = "`".repeat(fence_len);
    let _ = writeln!(out, "{fence}{language}");
    out.push_str(text);
    if !text.ends_with('\n') {
        out.push('\n');
    }
    let _ = writeln!(out, "{fence}");
    let _ = writeln!(out);
}

fn max_backtick_run(text: &str) -> usize {
    let mut max_run = 0;
    let mut current_run = 0;
    for ch in text.chars() {
        if ch == '`' {
            current_run += 1;
            max_run = max_run.max(current_run);
        } else {
            current_run = 0;
        }
    }
    max_run
}

fn data_uri(media_type: &str, data: &str) -> String {
    if data.starts_with("data:") {
        data.to_string()
    } else {
        format!("data:{media_type};base64,{data}")
    }
}

fn image_media_type(media_type: &ImageMediaType) -> &'static str {
    match media_type {
        ImageMediaType::Jpeg => "image/jpeg",
        ImageMediaType::Png => "image/png",
        ImageMediaType::Gif => "image/gif",
        ImageMediaType::Webp => "image/webp",
    }
}

fn pretty_json(value: &serde_json::Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use claudius::{TextBlock, ToolResultBlock, ToolUseBlock};
    use serde_json::json;

    #[test]
    fn parses_args() {
        assert_eq!(
            parse_args(["input.json"]).unwrap(),
            Args {
                input: Some(PathBuf::from("input.json")),
                output: None,
            }
        );
        assert_eq!(
            parse_args(["--output", "out.md", "-"]).unwrap(),
            Args {
                input: None,
                output: Some(PathBuf::from("out.md")),
            }
        );
        assert_eq!(
            parse_args(["--output=out.md", "input.json"]).unwrap(),
            Args {
                input: Some(PathBuf::from("input.json")),
                output: Some(PathBuf::from("out.md")),
            }
        );
    }

    #[test]
    fn renders_string_messages() {
        let transcript = TranscriptFile {
            version: 1,
            messages: vec![
                MessageParam::user("Hello"),
                MessageParam::assistant("Hi there\n"),
            ],
        };

        assert_eq!(
            render_transcript(&transcript),
            "# Transcript\n\n## User\n\nHello\n\n## Assistant\n\nHi there\n\n"
        );
    }

    #[test]
    fn renders_structured_blocks() {
        let transcript = TranscriptFile {
            version: 1,
            messages: vec![MessageParam::new_with_blocks(
                vec![
                    ContentBlock::Text(TextBlock::new("Use this tool:")),
                    ContentBlock::ToolUse(ToolUseBlock::new(
                        "tool_1",
                        "search",
                        json!({"query": "rust markdown"}),
                    )),
                    ContentBlock::ToolResult(
                        ToolResultBlock::new("tool_1".to_string())
                            .with_string_content("found it".to_string())
                            .with_error(false),
                    ),
                ],
                MessageRole::Assistant,
            )],
        };

        let rendered = render_transcript(&transcript);
        assert!(rendered.contains("## Assistant\n\nUse this tool:\n\n"));
        assert!(rendered.contains("### Tool Use: search\n\nID: `tool_1`\n\n```json\n"));
        assert!(rendered.contains("\"query\": \"rust markdown\""));
        assert!(
            rendered.contains("### Tool Result\n\nTool use ID: `tool_1`\n\n```text\nfound it\n```")
        );
    }

    #[test]
    fn expands_fence_for_backticks() {
        let mut out = String::new();
        write_fenced(&mut out, "text", "contains ``` fence");
        assert_eq!(out, "````text\ncontains ``` fence\n````\n\n");
    }
}
