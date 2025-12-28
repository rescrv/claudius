use std::sync::Arc;

use utf8path::Path;

use claudius::{
    Agent, Anthropic, Budget, FileSystem, MessageParam, MessageParamContent, MessageRole,
    PlainTextAgentRenderer,
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
    let budget = Arc::new(Budget::new_with_rates(512_000, 100, 500, 125, 10));
    let mut agent = MyAgent {
        root: Path::from("kb"),
    };

    let mut messages = vec![MessageParam {
        role: MessageRole::User,
        content: MessageParamContent::String(
            "Hello! Can you help me understand what files are in this directory?".to_string(),
        ),
    }];

    let mut renderer = PlainTextAgentRenderer::new();
    let stop = agent
        .take_turn_streaming_root(&client, &mut messages, &budget, &mut renderer)
        .await
        .unwrap();

    println!("\nStop reason: {stop:?}");
}
