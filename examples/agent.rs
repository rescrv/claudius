use utf8path::Path;

use claudius::{
    AgentLoop, Anthropic, KnownModel, MessageParam, MessageParamContent, MessageRole, Model,
    SystemPrompt, ThinkingConfig, ToolBash20250124, ToolSearchFileSystem, ToolTextEditor20250124,
    WebSearchTool20250305,
};

#[tokio::main]
async fn main() {
    let mut agent = AgentLoop {
        client: Anthropic::new(None).expect("could not create anthropic client"),
        agent: Path::from("kb"),
        max_tokens: 2048,
        model: Model::Known(KnownModel::Claude37SonnetLatest),
        messages: vec![MessageParam {
            role: MessageRole::User,
            content: MessageParamContent::String(
                "Search the web for the closest Spanish name to Alice and change all references to Alice to that name.".to_string(),
            ),
        }],
        metadata: None,
        stop_sequences: None,
        system: Some(SystemPrompt::from_string(
            "You are chrooted in an extensive, cross-linked, markdown-based Wiki in /.  Accomplish the user's task".to_string(),
        )),
        thinking: Some(ThinkingConfig::enabled(1024)),
        temperature: None,
        top_k: None,
        top_p: None,
        tool_choice: None,
        tools: vec![
            Box::new(ToolSearchFileSystem) as _,
            Box::new(ToolBash20250124::new()) as _,
            Box::new(ToolTextEditor20250124::new()) as _,
            Box::new(WebSearchTool20250305::new()) as _,
        ],
    };
    println!("{:#?}", agent.take_turn().await.unwrap());
}
