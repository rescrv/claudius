use std::sync::Arc;

use utf8path::Path;

use claudius::{
    Agent, Anthropic, Budget, FileSystem, MessageParam, MessageParamContent, MessageRole,
};

struct MyAgent {
    root: Path<'static>,
}

#[async_trait::async_trait]
impl Agent for MyAgent {
    async fn filesystem(&self) -> Option<&dyn FileSystem> {
        Some(&self.root)
    }
}

#[tokio::main]
async fn main() {
    let client = Anthropic::new(None).unwrap();
    let budget = Arc::new(Budget::from_dollars_flat_rate(0.25, 1000));
    let mut agent = MyAgent {
        root: Path::from("kb"),
    };

    // Initialize message history
    let mut messages = vec![MessageParam {
        role: MessageRole::User,
        content: MessageParamContent::String(
            "Hello! Can you help me understand what files are in this directory?".to_string(),
        ),
    }];

    println!(
        "{:#?}",
        agent
            .take_turn(&client, &mut messages, &budget)
            .await
            .unwrap()
    );

    // Show the message history after the conversation
    println!("\nMessage history:");
    for (i, msg) in messages.iter().enumerate() {
        println!("Message {}: {:?} - {:?}", i + 1, msg.role, msg.content);
    }
}
