use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use utf8path::Path;

use claudius::{
    Agent, Anthropic, Budget, FileSystem, KnownModel, Model, PlainTextRenderer, StopReason,
    SystemPrompt, Tool, ToolTextEditor20250728, TurnOutcome,
};

pub struct AgentKB {
    tools: Vec<Arc<dyn Tool<Self>>>,
    filesystem: Path<'static>,
}

impl AgentKB {
    pub fn new(filesystem: Path) -> Self {
        let tools = vec![Arc::new(ToolTextEditor20250728::new()) as _];
        let filesystem = filesystem.into_owned();
        Self { tools, filesystem }
    }
}

#[async_trait::async_trait]
impl Agent for AgentKB {
    async fn model(&self) -> Model {
        Model::Known(KnownModel::ClaudeHaiku45)
    }

    async fn max_tokens(&self) -> u32 {
        8192
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
    let budget = Arc::new(Budget::new_with_rates(25_000_000, 100, 500, 125, 10));
    let mut messages = vec![claudius::MessageParam {
        role: claudius::MessageRole::User,
        content: claudius::MessageParamContent::String(
            "Make an extensive, multi-page wiki about Abraham Lincoln with many small files."
                .to_string(),
        ),
    }];
    let interrupted = Arc::new(AtomicBool::new(false));
    let interrupted_clone = interrupted.clone();
    ctrlc::set_handler(move || {
        interrupted_clone.store(true, Ordering::Relaxed);
    })
    .unwrap();
    let mut renderer = PlainTextRenderer::with_color_and_interrupt(true, interrupted.clone());
    loop {
        interrupted.store(false, Ordering::Relaxed);
        match agent
            .take_turn_streaming_root(&client, &mut messages, &budget, &mut renderer)
            .await
            .unwrap()
        {
            TurnOutcome {
                stop_reason: StopReason::MaxTokens,
                ..
            }
            | TurnOutcome {
                stop_reason: StopReason::EndTurn,
                ..
            }
            | TurnOutcome {
                stop_reason: StopReason::ModelContextWindowExceeded,
                ..
            } => {
                break;
            }
            TurnOutcome {
                stop_reason: StopReason::Refusal,
                ..
            } => {
                todo!();
            }
            TurnOutcome {
                stop_reason: StopReason::StopSequence,
                ..
            } => {
                todo!();
            }
            TurnOutcome {
                stop_reason: StopReason::ToolUse,
                ..
            } => {
                todo!();
            }
            TurnOutcome {
                stop_reason: StopReason::PauseTurn,
                ..
            } => {
                todo!();
            }
        }
    }
}
