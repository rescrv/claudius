use std::sync::Arc;

use utf8path::Path;

use claudius::{
    Agent, Anthropic, Budget, FileSystem, KnownModel, Message, MessageCreateParams, Model,
    StopReason, SystemPrompt, Tool, ToolTextEditor20250429,
};

pub struct AgentKB {
    tools: Vec<Arc<dyn Tool<Self>>>,
    filesystem: Path<'static>,
}

impl AgentKB {
    pub fn new(filesystem: Path) -> Self {
        let tools = vec![Arc::new(ToolTextEditor20250429::new()) as _];
        let filesystem = filesystem.into_owned();
        Self { tools, filesystem }
    }
}

#[async_trait::async_trait]
impl Agent for AgentKB {
    async fn model(&self) -> Model {
        Model::Known(KnownModel::ClaudeOpus40)
    }

    async fn max_tokens(&self) -> u32 {
        1024
    }

    async fn hook_message_create_params(
        &self,
        params: &MessageCreateParams,
    ) -> Result<(), claudius::Error> {
        eprintln!("{params:#?}");
        Ok(())
    }

    async fn hook_message(&self, msg: &Message) -> Result<(), claudius::Error> {
        eprintln!("{msg:#?}");
        Ok(())
    }

    async fn tools(&self) -> Vec<Arc<dyn Tool<Self>>> {
        self.tools.clone()
    }

    async fn system(&self) -> Option<SystemPrompt> {
        Some("You are chrooted in an extensive, cross-linked, markdown-based Wiki in /.  You are a proof-reading agent.  Accomplish the user's task".into())
    }

    async fn filesystem(&self) -> Option<&dyn FileSystem> {
        Some(&self.filesystem)
    }
}

#[tokio::main]
async fn main() {
    let client = Anthropic::new(None).unwrap();
    let mut agent = AgentKB::new("kb".into());
    let budget = Arc::new(Budget::new(10_000));
    let mut messages = vec![claudius::MessageParam {
        role: claudius::MessageRole::User,
        content: claudius::MessageParamContent::String(
            "Change all references to \"Alice\" to \"Alyssa\"".to_string(),
        ),
    }];
    loop {
        match agent
            .take_turn(&client, &mut messages, &budget)
            .await
            .unwrap()
        {
            StopReason::MaxTokens | StopReason::EndTurn => {
                break;
            }
            StopReason::Refusal => {
                todo!();
            }
            StopReason::StopSequence => {
                todo!();
            }
            StopReason::ToolUse => {
                todo!();
            }
            StopReason::PauseTurn => {
                todo!();
            }
        }
    }
}
