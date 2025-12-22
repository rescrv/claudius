//! Chat with your documents using LLM streaming combinators.
//!
//! This example demonstrates how to build a document Q&A agent that:
//! - Loads documents from the filesystem based on command-line arguments
//! - Includes document contents in the system prompt
//! - Uses the `unfold_until` combinator for an interactive chat loop
//! - Streams responses token by token
//!
//! # Usage
//!
//! ```bash
//! cargo run --bin chat-with-docs -- doc1.txt doc2.md notes.txt
//! ```

use std::io::Write;
use std::pin::Pin;

use claudius::{
    ContentBlock, DocumentBlock, Error, Message, MessageCreateTemplate, MessageParam, MessageRole,
    MessageStreamEvent, PlainTextSource, push_or_merge_message,
};
use futures::stream::{Stream, StreamExt};
use utf8path::Path;

use claudius::combinators::{VecContext, client, read_user_input, unfold_until};
use claudius::{impl_from_vec_context, impl_simple_context};

fn load_document(path: Path) -> Result<DocumentBlock, std::io::Error> {
    let content = std::fs::read_to_string(&path)?;
    Ok(DocumentBlock {
        source: PlainTextSource::new(content).into(),
        cache_control: None,
        citations: None,
        context: None,
        title: Some(path.to_string()),
    })
}

fn load_documents(paths: Vec<Path>) -> Vec<DocumentBlock> {
    let mut documents = Vec::new();
    for path in paths {
        match load_document(path.clone()) {
            Ok(doc) => {
                println!("  Loaded: {} ", path);
                documents.push(doc);
            }
            Err(e) => {
                eprintln!("  Failed to load {}: {}", path, e);
            }
        }
    }
    documents
}

/// The state maintained across agent turns.
#[derive(Clone)]
struct ChatState {
    /// The thread of conversation.
    thread: VecContext,
    /// Whether the user has requested to quit.
    should_quit: bool,
}

impl_simple_context!(ChatState, thread);
impl_from_vec_context!(ChatState { should_quit: false });

#[tokio::main]
async fn main() -> Result<(), Error> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.is_empty() {
        eprintln!("Usage: chat-with-docs <file1> [file2] [file3] ...");
        eprintln!();
        eprintln!("Chat with your documents using Claude. Provide one or more file paths.");
        std::process::exit(1);
    }

    println!("Chat With Documents");
    println!("====================");
    println!();
    println!("Loading documents...");

    let paths: Vec<Path> = args.into_iter().map(Path::from).collect();
    let documents = load_documents(paths);

    if documents.is_empty() {
        eprintln!("No documents were loaded. Exiting.");
        std::process::exit(1);
    }

    println!();
    println!(
        "Loaded {} document(s). Type 'quit' to exit.",
        documents.len()
    );
    println!("Ask questions about your documents:");

    let blocks: Vec<ContentBlock> = documents.into_iter().map(|doc| doc.into()).collect();
    let messages = vec![MessageParam::new_with_blocks(blocks, MessageRole::User)];
    let template = MessageCreateTemplate {
        stream: Some(true),
        ..Default::default()
    };
    let initial_state = ChatState {
        thread: VecContext(messages),
        should_quit: false,
    };

    type UpdateFn = Box<dyn FnOnce(Message) -> ChatState + Send>;
    let agent = unfold_until(
        initial_state,
        move |mut state: ChatState| {
            let template = template.clone();
            async move {
                let user_input = match read_user_input() {
                    Some(input) => input,
                    None => {
                        state.should_quit = true;
                        // Return an empty stream for the final iteration
                        let empty: futures::stream::Empty<Result<MessageStreamEvent, Error>> =
                            futures::stream::empty();
                        let update_fn: UpdateFn = Box::new(move |_msg: Message| state);
                        return Ok((Box::pin(empty) as _, update_fn));
                    }
                };
                push_or_merge_message(&mut state.thread.0, MessageParam::user(&user_input));
                let streamer = client::<VecContext>(None);
                let stream: Pin<Box<dyn Stream<Item = Result<MessageStreamEvent, Error>> + Send>> =
                    streamer(template, state.thread.clone()).await?;
                let update_fn: UpdateFn = Box::new(move |msg: Message| {
                    let mut state = state;
                    push_or_merge_message(&mut state.thread.0, msg.into());
                    state
                });
                Ok((stream, update_fn))
            }
        },
        |state| state.should_quit,
    );

    // Drive the agent stream
    futures::pin_mut!(agent);
    while let Some(result) = agent.next().await {
        let mut stream = result?;
        let mut first = true;
        while let Some(event) = stream.next().await {
            if first {
                print!("\nClaude: ");
            }
            first = false;
            match event {
                Err(e) => {
                    eprintln!("\nError: {}", e);
                    break;
                }
                Ok(MessageStreamEvent::ContentBlockDelta(delta)) => {
                    if let claudius::ContentBlockDelta::TextDelta(text) = delta.delta {
                        print!("{}", text.text);
                        std::io::stdout().flush().ok();
                    }
                }
                Ok(_) => {}
            }
        }
        println!();
    }

    println!("\nGoodbye!");
    Ok(())
}
