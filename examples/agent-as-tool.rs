//! Agent-as-Tool Demo: One agent exposes a tool-like interface for another agent.
//!
//! This demonstrates:
//! - An "inner agent" (Database Agent) that maintains stateful key-value storage
//! - An "outer agent" (Client Agent) that uses the inner agent as a tool
//! - State persists across consecutive tool calls within a session
//! - Both agents stream their responses (fully unrolled)
//!
//! The inner agent interprets natural language requests and maintains state, so the
//! outer agent can describe what it wants without knowing the exact schema.
//!
//! # Usage
//!
//! ```bash
//! cargo run --bin agent-as-tool
//! ```
//!
//! # Example Interaction
//!
//! ```text
//! You: Store my favorite color as blue
//! Claude: [Using tools: query_database_agent]
//!   [DB Agent: db_set] Done! I've stored "favorite_color" = "blue".
//! Claude: I've stored your favorite color as blue.
//!
//! You: What's my favorite color?
//! Claude: [Using tools: query_database_agent]
//!   [DB Agent: db_get] Your favorite color is blue.
//! Claude: Your favorite color is blue!
//! ```

use std::collections::HashMap;
use std::io::Write;
use std::pin::Pin;
use std::sync::Arc;

use claudius::{
    ContentBlock, Error, Message, MessageCreateTemplate, MessageParam, MessageStreamEvent,
    ToolParam, ToolUseBlock,
};
use futures::Stream;
use futures::stream::StreamExt;
use serde_json::json;
use tokio::sync::Mutex;

use claudius::combinators::{
    AccumulatingStream, VecContext, client, debug_stream, read_user_input, unfold_with_tools_async,
};
use claudius::{impl_from_vec_context, impl_simple_context};

/// The stateful database maintained by the inner agent.
#[derive(Clone, Debug, Default)]
struct Database {
    /// Key-value storage.
    data: HashMap<String, String>,
}

impl Database {
    fn set(&mut self, key: &str, value: &str) {
        self.data.insert(key.to_string(), value.to_string());
    }

    fn get(&self, key: &str) -> Option<&String> {
        self.data.get(key)
    }

    fn delete(&mut self, key: &str) -> bool {
        self.data.remove(key).is_some()
    }

    fn list(&self) -> Vec<(&String, &String)> {
        self.data.iter().collect()
    }

    fn format_state(&self) -> String {
        if self.data.is_empty() {
            "Database is empty.".to_string()
        } else {
            let items: Vec<String> = self
                .data
                .iter()
                .map(|(k, v)| format!("  {} = {}", k, v))
                .collect();
            format!("Current database contents:\n{}", items.join("\n"))
        }
    }
}

/// State for the inner Database Agent.
#[derive(Clone)]
struct DbAgentState {
    thread: VecContext,
    done: bool,
}

impl_simple_context!(DbAgentState, thread);
impl_from_vec_context!(DbAgentState { done: false });

/// The inner "Database Agent" that interprets commands and manages state.
///
/// This agent receives natural language requests and translates them into
/// database operations, maintaining state across calls.
struct DatabaseAgent {
    db: Arc<Mutex<Database>>,
}

impl DatabaseAgent {
    fn new() -> Self {
        Self {
            db: Arc::new(Mutex::new(Database::default())),
        }
    }

    fn get_tools() -> Vec<claudius::ToolUnionParam> {
        vec![
            claudius::ToolUnionParam::CustomTool(ToolParam {
                name: "db_set".to_string(),
                description: Some("Store a value in the database".to_string()),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "key": { "type": "string", "description": "The key to store" },
                        "value": { "type": "string", "description": "The value to store" }
                    },
                    "required": ["key", "value"]
                }),
                cache_control: None,
            }),
            claudius::ToolUnionParam::CustomTool(ToolParam {
                name: "db_get".to_string(),
                description: Some("Retrieve a value from the database".to_string()),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "key": { "type": "string", "description": "The key to retrieve" }
                    },
                    "required": ["key"]
                }),
                cache_control: None,
            }),
            claudius::ToolUnionParam::CustomTool(ToolParam {
                name: "db_delete".to_string(),
                description: Some("Delete a key-value pair from the database".to_string()),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "key": { "type": "string", "description": "The key to delete" }
                    },
                    "required": ["key"]
                }),
                cache_control: None,
            }),
            claudius::ToolUnionParam::CustomTool(ToolParam {
                name: "db_list".to_string(),
                description: Some("List all entries in the database".to_string()),
                input_schema: json!({
                    "type": "object",
                    "properties": {}
                }),
                cache_control: None,
            }),
        ]
    }

    async fn execute_tool(&self, tool_use: &ToolUseBlock) -> Result<String, String> {
        let mut db = self.db.lock().await;

        match tool_use.name.as_str() {
            "db_set" => {
                let key = tool_use.input["key"].as_str().unwrap_or("");
                let value = tool_use.input["value"].as_str().unwrap_or("");
                db.set(key, value);
                Ok(format!("Stored: {} = {}", key, value))
            }
            "db_get" => {
                let key = tool_use.input["key"].as_str().unwrap_or("");
                match db.get(key) {
                    Some(value) => Ok(format!("Value for '{}': {}", key, value)),
                    None => Ok(format!("Key '{}' not found", key)),
                }
            }
            "db_delete" => {
                let key = tool_use.input["key"].as_str().unwrap_or("");
                if db.delete(key) {
                    Ok(format!("Deleted key '{}'", key))
                } else {
                    Ok(format!("Key '{}' was not found", key))
                }
            }
            "db_list" => {
                let entries = db.list();
                if entries.is_empty() {
                    Ok("Database is empty".to_string())
                } else {
                    let items: Vec<String> = entries
                        .iter()
                        .map(|(k, v)| format!("{}: {}", k, v))
                        .collect();
                    Ok(format!("Database entries:\n{}", items.join("\n")))
                }
            }
            _ => Err(format!("Unknown tool: {}", tool_use.name)),
        }
    }

    /// Process a request and return a stream of streams (one per turn).
    ///
    /// This fully unrolls the inner agent, yielding each turn's stream.
    fn process_request(
        self: Arc<Self>,
        request: String,
    ) -> impl Stream<Item = Result<AccumulatingStream, Error>> {
        let db_clone = Arc::clone(&self);
        let db_state_future = async move { db_clone.db.lock().await.format_state() };

        let db_agent = Arc::clone(&self);

        futures::stream::once(db_state_future).flat_map(move |db_state| {
            let system_prompt = format!(
                r#"You are a database assistant. You manage a key-value store.

Current database state:
{}

You have access to the following tools to manipulate the database:
- db_set: Store a value with a key
- db_get: Retrieve a value by key  
- db_delete: Remove a key-value pair
- db_list: List all entries

Interpret the user's natural language request and use the appropriate tool(s).
After performing operations, respond with a brief confirmation of what was done.
If the user asks about data, return the relevant information.
Be concise but helpful."#,
                db_state
            );

            let template = MessageCreateTemplate {
                system: Some(claudius::SystemPrompt::String(system_prompt)),
                tools: Some(Self::get_tools()),
                stream: Some(true),
                ..Default::default()
            };

            let initial_state = DbAgentState {
                thread: VecContext(vec![MessageParam::user(&request)]),
                done: false,
            };

            let db_agent = Arc::clone(&db_agent);
            type UpdateFn = Box<dyn FnOnce(Message) -> DbAgentState + Send>;

            unfold_with_tools_async(
                initial_state,
                move |state: DbAgentState| async move {
                    // Don't mark done here - let the update_fn check stop_reason
                    let state_for_update = state.clone();
                    let update_fn: UpdateFn = Box::new(move |msg: Message| {
                        let mut state = state_for_update;
                        // Mark done when we get a non-tool-use response
                        if msg.stop_reason != Some(claudius::StopReason::ToolUse) {
                            state.done = true;
                        }
                        claudius::push_or_merge_message(&mut state.thread.0, msg.into());
                        state
                    });
                    Ok((state, update_fn))
                },
                {
                    let template = template.clone();
                    move |ctx: &DbAgentState| {
                        let template = template.clone();
                        let ctx = ctx.clone();
                        async move { client::<VecContext>(None)(template, ctx.thread).await }
                    }
                },
                {
                    let db_agent = Arc::clone(&db_agent);
                    move |tool_use: &ToolUseBlock| {
                        let db_agent = Arc::clone(&db_agent);
                        let tool_use_id = tool_use.id.clone();
                        let tool_use_name = tool_use.name.clone();
                        let tool_use_input = tool_use.input.clone();
                        async move {
                            let fake_tool_use = ToolUseBlock {
                                id: tool_use_id,
                                name: tool_use_name,
                                input: tool_use_input,
                                cache_control: None,
                            };
                            db_agent.execute_tool(&fake_tool_use).await
                        }
                    }
                },
                |state| state.done,
            )
        })
    }
}

/// State for the outer "Client Agent" that talks to users.
#[derive(Clone)]
struct ClientState {
    thread: VecContext,
    should_quit: bool,
}

impl_simple_context!(ClientState, thread);
impl_from_vec_context!(ClientState { should_quit: false });

/// Define the tool that exposes the database agent to the client agent.
fn get_client_tools() -> Vec<claudius::ToolUnionParam> {
    vec![claudius::ToolUnionParam::CustomTool(ToolParam {
        name: "query_database_agent".to_string(),
        description: Some(
            "Send a natural language request to the database agent. \
             The database agent maintains a persistent key-value store \
             and can store, retrieve, delete, or list data. \
             You can ask it things like 'store my name as Alice', \
             'what is my name?', 'delete my name', or 'show me everything stored'."
                .to_string(),
        ),
        input_schema: json!({
            "type": "object",
            "properties": {
                "request": {
                    "type": "string",
                    "description": "The natural language request to send to the database agent"
                }
            },
            "required": ["request"]
        }),
        cache_control: None,
    })]
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    println!("Agent-as-Tool Demo (Fully Unrolled)");
    println!("===================================");
    println!();
    println!("This demo shows one agent using another agent as a tool.");
    println!("Both agents stream their responses - you'll see the inner");
    println!("Database Agent's output as it processes requests.");
    println!();
    println!("Try things like:");
    println!("  - 'Remember that my favorite color is blue'");
    println!("  - 'What's my favorite color?'");
    println!("  - 'Store my age as 30 and my city as Seattle'");
    println!("  - 'What do you know about me?'");
    println!("  - 'Delete my age'");
    println!();
    println!("Type 'quit' to exit.");

    let db_agent = Arc::new(DatabaseAgent::new());

    let template = MessageCreateTemplate {
        system: Some(claudius::SystemPrompt::String(
            "You are a helpful assistant. You have access to a database agent that can \
             store and retrieve information persistently. Use it to remember things the \
             user tells you and to answer questions about stored information. \
             Be conversational and helpful."
                .to_string(),
        )),
        tools: Some(get_client_tools()),
        stream: Some(true),
        ..Default::default()
    };

    let initial_state = ClientState {
        thread: VecContext(vec![]),
        should_quit: false,
    };

    // We need to handle tool calls specially to stream the inner agent.
    // Instead of using unfold_with_tools_async directly, we'll manually
    // interleave the streams.

    type UpdateFn = Box<dyn FnOnce(Message) -> ClientState + Send>;
    type InnerStream = Pin<Box<dyn Stream<Item = Result<AccumulatingStream, Error>> + Send>>;

    // Channel to send inner agent streams for display
    let (inner_tx, mut inner_rx) = tokio::sync::mpsc::unbounded_channel::<(
        String,
        InnerStream,
        tokio::sync::oneshot::Sender<String>,
    )>();

    let db_agent_for_handler = Arc::clone(&db_agent);

    let agent = unfold_with_tools_async(
        initial_state,
        move |mut state: ClientState| async move {
            let user_input = match read_user_input() {
                Some(input) => input,
                None => {
                    state.should_quit = true;
                    let state_for_update = state.clone();
                    let update_fn: UpdateFn = Box::new(move |_msg: Message| state_for_update);
                    return Ok((state, update_fn));
                }
            };
            claudius::push_or_merge_message(&mut state.thread.0, MessageParam::user(&user_input));
            let state_for_update = state.clone();
            let update_fn: UpdateFn = Box::new(move |msg: Message| {
                let mut state = state_for_update;
                claudius::push_or_merge_message(&mut state.thread.0, msg.into());
                state
            });
            Ok((state, update_fn))
        },
        debug_stream("API Request", {
            let template = template.clone();
            move |ctx: &ClientState| {
                let template = template.clone();
                let ctx = ctx.clone();
                async move { client::<VecContext>(None)(template, ctx.thread).await }
            }
        }),
        move |tool_use: &ToolUseBlock| {
            let db_agent = Arc::clone(&db_agent_for_handler);
            let request = tool_use.input["request"].as_str().unwrap_or("").to_string();
            let inner_tx = inner_tx.clone();
            async move {
                // Create a channel to receive the final response
                let (response_tx, response_rx) = tokio::sync::oneshot::channel();

                // Send the inner agent stream to the display loop
                let inner_stream =
                    Box::pin(db_agent.process_request(request.clone())) as InnerStream;
                if inner_tx
                    .send((request.clone(), inner_stream, response_tx))
                    .is_err()
                {
                    return Err("Failed to send inner stream".to_string());
                }

                // Wait for the display loop to finish processing and send back the response
                match response_rx.await {
                    Ok(response) => Ok(response),
                    Err(_) => Err("Failed to receive response from inner agent".to_string()),
                }
            }
        },
        |state| state.should_quit,
    );

    futures::pin_mut!(agent);

    // Helper to process inner agent streams
    async fn process_inner_stream(request: String, mut inner_stream: InnerStream) -> String {
        print!("\n  [DB Agent processing: \"{}\"]", request);
        std::io::stdout().flush().ok();

        let mut response_text = String::new();

        while let Some(result) = inner_stream.next().await {
            match result {
                Ok(mut stream) => {
                    let mut first = true;
                    let mut in_tool = false;
                    while let Some(event) = stream.next().await {
                        match event {
                            Ok(MessageStreamEvent::ContentBlockDelta(delta)) => {
                                if let claudius::ContentBlockDelta::TextDelta(text) = delta.delta {
                                    if first {
                                        print!("\n  [DB Agent]: ");
                                        first = false;
                                    }
                                    print!("{}", text.text);
                                    response_text.push_str(&text.text);
                                    std::io::stdout().flush().ok();
                                }
                            }
                            Ok(MessageStreamEvent::ContentBlockStart(start)) => {
                                if let ContentBlock::ToolUse(tool_use) = &start.content_block {
                                    if !in_tool {
                                        print!("\n  [DB Agent tools: ");
                                        in_tool = true;
                                    } else {
                                        print!(", ");
                                    }
                                    print!("{}", tool_use.name);
                                    std::io::stdout().flush().ok();
                                }
                            }
                            Ok(MessageStreamEvent::MessageDelta(delta)) => {
                                if let Some(claudius::StopReason::ToolUse) = delta.delta.stop_reason
                                    && in_tool
                                {
                                    print!("]");
                                    in_tool = false;
                                }
                            }
                            Ok(_) => {}
                            Err(e) => {
                                eprintln!("\n  [DB Agent Error]: {}", e);
                                break;
                            }
                        }
                    }
                    if !first {
                        println!();
                    }
                }
                Err(e) => {
                    eprintln!("\n  [DB Agent Error]: {}", e);
                }
            }
        }

        response_text
    }

    // Spawn a task to handle inner agent stream processing
    let inner_handler = tokio::spawn(async move {
        while let Some((request, inner_stream, response_tx)) = inner_rx.recv().await {
            let response = process_inner_stream(request, inner_stream).await;
            let _ = response_tx.send(response);
        }
    });

    // Main display loop - just handle outer agent streams
    while let Some(result) = agent.next().await {
        match result {
            Ok(mut stream) => {
                let mut first = true;
                let mut tool_count = 0;
                while let Some(event) = stream.next().await {
                    match event {
                        Err(e) => {
                            eprintln!("\nError: {}", e);
                            break;
                        }
                        Ok(MessageStreamEvent::ContentBlockDelta(delta)) => {
                            if let claudius::ContentBlockDelta::TextDelta(text) = delta.delta {
                                if first {
                                    print!("\nClaude: ");
                                }
                                first = false;
                                print!("{}", text.text);
                                std::io::stdout().flush().ok();
                            }
                        }
                        Ok(MessageStreamEvent::ContentBlockStart(start)) => {
                            if let ContentBlock::ToolUse(tool_use) = &start.content_block {
                                tool_count += 1;
                                if tool_count == 1 {
                                    if first {
                                        print!("\nClaude: ");
                                    }
                                    print!("[Using: ");
                                } else {
                                    print!(", ");
                                }
                                print!("{}", tool_use.name);
                                std::io::stdout().flush().ok();
                                first = false;
                            }
                        }
                        Ok(MessageStreamEvent::MessageDelta(delta)) => {
                            if let Some(claudius::StopReason::ToolUse) = delta.delta.stop_reason {
                                println!("]");
                            }
                        }
                        Ok(_) => {}
                    }
                }
                if !first && tool_count == 0 {
                    println!();
                }
            }
            Err(e) => {
                eprintln!("\nError: {}", e);
                break;
            }
        }
    }

    // Clean up
    drop(inner_handler);

    println!("\nGoodbye!");
    Ok(())
}
