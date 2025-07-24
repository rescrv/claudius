use std::any::Any;
use std::ops::ControlFlow;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use utf8path::Path;

use crate::{
    merge_message_content, push_or_merge_message, Anthropic, ContentBlock, Error, KnownModel,
    Message, MessageCreateParams, MessageParam, MessageParamContent, MessageRole, Metadata, Model,
    StopReason, SystemPrompt, ThinkingConfig, ToolBash20241022, ToolBash20250124, ToolChoice,
    ToolParam, ToolResultBlock, ToolResultBlockContent, ToolTextEditor20250124,
    ToolTextEditor20250429, ToolUnionParam, ToolUseBlock, WebSearchTool20250305,
};

//////////////////////////////////////////// ToolResult ////////////////////////////////////////////

pub type ToolResult = ControlFlow<Error, Result<ToolResultBlock, ToolResultBlock>>;

////////////////////////////////////// IntermediateToolResult //////////////////////////////////////

pub trait IntermediateToolResult: Send {
    fn as_any(&self) -> &dyn Any;
}

impl IntermediateToolResult for () {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl<T: Send + 'static> IntermediateToolResult for Option<T> {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl IntermediateToolResult for ToolResult {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

//////////////////////////////////////// ToolResultCallback ////////////////////////////////////////

#[async_trait::async_trait]
pub trait ToolCallback<A>: Send {
    async fn compute_tool_result(
        &self,
        client: &Anthropic,
        agent: &A,
        tool_use: &ToolUseBlock,
    ) -> Box<dyn IntermediateToolResult>;
    async fn apply_tool_result(
        &self,
        client: &Anthropic,
        agent: &mut A,
        tool_use: &ToolUseBlock,
        intermediate: Box<dyn IntermediateToolResult>,
    ) -> ToolResult;
}

/////////////////////////////////////////////// Tool ///////////////////////////////////////////////

pub trait Tool<A: Agent>: Send + Sync {
    fn name(&self) -> String;
    fn callback(&self) -> Box<dyn ToolCallback<A> + '_>;
    fn to_param(&self) -> ToolUnionParam;
}

struct ToolNotFound(String);

impl<A: Agent> Tool<A> for ToolNotFound {
    fn name(&self) -> String {
        self.0.clone()
    }

    fn callback(&self) -> Box<dyn ToolCallback<A> + '_> {
        Box::new(ToolNotFoundCallback(self.0.clone()))
    }

    fn to_param(&self) -> ToolUnionParam {
        unimplemented!();
    }
}

struct ToolNotFoundCallback(String);

#[async_trait::async_trait]
impl<A: Agent> ToolCallback<A> for ToolNotFoundCallback {
    async fn compute_tool_result(
        &self,
        _client: &Anthropic,
        _agent: &A,
        _tool_use: &ToolUseBlock,
    ) -> Box<dyn IntermediateToolResult> {
        Box::new(())
    }

    async fn apply_tool_result(
        &self,
        _client: &Anthropic,
        _agent: &mut A,
        tool_use: &ToolUseBlock,
        _intermediate: Box<dyn IntermediateToolResult>,
    ) -> ToolResult {
        ControlFlow::Continue(Err(ToolResultBlock {
            tool_use_id: tool_use.id.clone(),
            content: Some(ToolResultBlockContent::String(format!(
                "{} not found",
                self.0
            ))),
            is_error: Some(true),
            cache_control: None,
        }))
    }
}

impl<A: Agent> Tool<A> for ToolBash20241022 {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn callback(&self) -> Box<dyn ToolCallback<A> + '_> {
        Box::new(BashCallback)
    }

    fn to_param(&self) -> ToolUnionParam {
        ToolUnionParam::Bash20241022(self.clone())
    }
}

impl<A: Agent> Tool<A> for ToolBash20250124 {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn callback(&self) -> Box<dyn ToolCallback<A> + '_> {
        Box::new(BashCallback)
    }

    fn to_param(&self) -> ToolUnionParam {
        ToolUnionParam::Bash20250124(self.clone())
    }
}

struct BashCallback;

#[async_trait::async_trait]
impl<A: Agent> ToolCallback<A> for BashCallback {
    async fn compute_tool_result(
        &self,
        _client: &Anthropic,
        agent: &A,
        tool_use: &ToolUseBlock,
    ) -> Box<dyn IntermediateToolResult> {
        #[derive(serde::Deserialize)]
        struct BashTool {
            command: String,
            restart: bool,
        }
        let bash: BashTool = match serde_json::from_value(tool_use.input.clone()) {
            Ok(input) => input,
            Err(err) => {
                return Box::new(ControlFlow::Continue(Err(ToolResultBlock {
                    tool_use_id: tool_use.id.clone(),
                    content: Some(ToolResultBlockContent::String(err.to_string())),
                    is_error: Some(true),
                    cache_control: None,
                })));
            }
        };
        match agent.bash(&bash.command, bash.restart).await {
            Ok(answer) => Box::new(ControlFlow::Continue(Ok(ToolResultBlock {
                tool_use_id: tool_use.id.clone(),
                content: Some(ToolResultBlockContent::String(answer.to_string())),
                is_error: None,
                cache_control: None,
            }))),
            Err(err) => Box::new(ControlFlow::Continue(Err(ToolResultBlock {
                tool_use_id: tool_use.id.clone(),
                content: Some(ToolResultBlockContent::String(err.to_string())),
                is_error: Some(true),
                cache_control: None,
            }))),
        }
    }

    async fn apply_tool_result(
        &self,
        _client: &Anthropic,
        _agent: &mut A,
        _tool_use: &ToolUseBlock,
        intermediate: Box<dyn IntermediateToolResult>,
    ) -> ToolResult {
        let Some(intermediate) = intermediate.as_any().downcast_ref::<ToolResult>() else {
            return ControlFlow::Break(Error::unknown(
                "intermediate tool result fails to deserialize",
            ));
        };
        intermediate.clone()
    }
}

struct TextEditorCallback;

#[async_trait::async_trait]
impl<A: Agent> ToolCallback<A> for TextEditorCallback {
    async fn compute_tool_result(
        &self,
        _client: &Anthropic,
        agent: &A,
        tool_use: &ToolUseBlock,
    ) -> Box<dyn IntermediateToolResult> {
        match agent.text_editor(tool_use.clone()).await {
            Ok(result) => Box::new(ControlFlow::Continue(Ok(ToolResultBlock {
                tool_use_id: tool_use.id.clone(),
                content: Some(ToolResultBlockContent::String(result)),
                is_error: None,
                cache_control: None,
            }))),
            Err(err) => Box::new(ControlFlow::Continue(Err(ToolResultBlock {
                tool_use_id: tool_use.id.clone(),
                content: Some(ToolResultBlockContent::String(err.to_string())),
                is_error: Some(true),
                cache_control: None,
            }))),
        }
    }

    async fn apply_tool_result(
        &self,
        _client: &Anthropic,
        _agent: &mut A,
        _tool_use: &ToolUseBlock,
        intermediate: Box<dyn IntermediateToolResult>,
    ) -> ToolResult {
        let Some(intermediate) = intermediate.as_any().downcast_ref::<ToolResult>() else {
            return ControlFlow::Break(Error::unknown(
                "intermediate tool result fails to deserialize",
            ));
        };
        intermediate.clone()
    }
}

struct WebSearchCallback;

#[async_trait::async_trait]
impl<A: Agent> ToolCallback<A> for WebSearchCallback {
    async fn compute_tool_result(
        &self,
        _client: &Anthropic,
        _agent: &A,
        tool_use: &ToolUseBlock,
    ) -> Box<dyn IntermediateToolResult> {
        Box::new(ControlFlow::Continue(Err(ToolResultBlock {
            tool_use_id: tool_use.id.clone(),
            content: Some(ToolResultBlockContent::String(
                "Web search is not implemented".to_string(),
            )),
            is_error: Some(true),
            cache_control: None,
        })))
    }

    async fn apply_tool_result(
        &self,
        _client: &Anthropic,
        _agent: &mut A,
        _tool_use: &ToolUseBlock,
        intermediate: Box<dyn IntermediateToolResult>,
    ) -> ToolResult {
        let Some(intermediate) = intermediate.as_any().downcast_ref::<ToolResult>() else {
            return ControlFlow::Break(Error::unknown(
                "intermediate tool result fails to deserialize",
            ));
        };
        intermediate.clone()
    }
}

struct SearchFilesystemCallback;

#[async_trait::async_trait]
impl<A: Agent> ToolCallback<A> for SearchFilesystemCallback {
    async fn compute_tool_result(
        &self,
        _client: &Anthropic,
        agent: &A,
        tool_use: &ToolUseBlock,
    ) -> Box<dyn IntermediateToolResult> {
        #[derive(serde::Deserialize)]
        struct SearchTool {
            query: String,
        }
        let search: SearchTool = match serde_json::from_value(tool_use.input.clone()) {
            Ok(input) => input,
            Err(err) => {
                return Box::new(ControlFlow::Continue(Err(ToolResultBlock {
                    tool_use_id: tool_use.id.clone(),
                    content: Some(ToolResultBlockContent::String(err.to_string())),
                    is_error: Some(true),
                    cache_control: None,
                })));
            }
        };
        match agent.search(&search.query).await {
            Ok(answer) => Box::new(ControlFlow::Continue(Ok(ToolResultBlock {
                tool_use_id: tool_use.id.clone(),
                content: Some(ToolResultBlockContent::String(answer.to_string())),
                is_error: None,
                cache_control: None,
            }))),
            Err(err) => Box::new(ControlFlow::Continue(Err(ToolResultBlock {
                tool_use_id: tool_use.id.clone(),
                content: Some(ToolResultBlockContent::String(err.to_string())),
                is_error: Some(true),
                cache_control: None,
            }))),
        }
    }

    async fn apply_tool_result(
        &self,
        _client: &Anthropic,
        _agent: &mut A,
        _tool_use: &ToolUseBlock,
        intermediate: Box<dyn IntermediateToolResult>,
    ) -> ToolResult {
        let Some(intermediate) = intermediate.as_any().downcast_ref::<ToolResult>() else {
            return ControlFlow::Break(Error::unknown(
                "intermediate tool result fails to deserialize",
            ));
        };
        intermediate.clone()
    }
}

impl<A: Agent> Tool<A> for ToolTextEditor20250124 {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn callback(&self) -> Box<dyn ToolCallback<A>> {
        Box::new(TextEditorCallback)
    }

    fn to_param(&self) -> ToolUnionParam {
        ToolUnionParam::TextEditor20250124(self.clone())
    }
}

impl<A: Agent> Tool<A> for ToolTextEditor20250429 {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn callback(&self) -> Box<dyn ToolCallback<A>> {
        Box::new(TextEditorCallback)
    }

    fn to_param(&self) -> ToolUnionParam {
        ToolUnionParam::TextEditor20250429(self.clone())
    }
}

impl<A: Agent> Tool<A> for WebSearchTool20250305 {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn callback(&self) -> Box<dyn ToolCallback<A>> {
        Box::new(WebSearchCallback)
    }

    fn to_param(&self) -> ToolUnionParam {
        ToolUnionParam::WebSearch20250305(self.clone())
    }
}

pub struct ToolSearchFileSystem;

impl<A: Agent> Tool<A> for ToolSearchFileSystem {
    fn name(&self) -> String {
        "search_filesystem".to_string()
    }

    fn callback(&self) -> Box<dyn ToolCallback<A>> {
        Box::new(SearchFilesystemCallback)
    }

    fn to_param(&self) -> ToolUnionParam {
        let name = <Self as Tool<A>>::name(self).to_string();
        let input_schema = serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "The search query to find on the filesystem."
                }
            },
            "required": ["query"]
        });
        let description = Some("Search the local filesystem.".to_string());
        let cache_control = None;
        ToolUnionParam::CustomTool(ToolParam {
            input_schema,
            name,
            description,
            cache_control,
        })
    }
}

////////////////////////////////////////////// Budget //////////////////////////////////////////////

pub struct Budget {
    remaining: Arc<AtomicU64>,
}

impl Budget {
    pub fn new(tokens: u32) -> Self {
        let remaining = Arc::new(AtomicU64::new(tokens as u64));
        Self { remaining }
    }

    pub fn allocate(&self, amount: u32) -> Option<BudgetAllocation> {
        let allocated = amount;
        let amount = amount as u64;
        loop {
            let witness = self.remaining.load(Ordering::Relaxed);
            if witness >= amount
                && self
                    .remaining
                    .compare_exchange(
                        witness,
                        witness - amount,
                        Ordering::Relaxed,
                        Ordering::Relaxed,
                    )
                    .is_ok()
            {
                let remaining = Arc::clone(&self.remaining);
                return Some(BudgetAllocation {
                    remaining,
                    allocated,
                });
            } else if witness < amount {
                return None;
            }
        }
    }
}

pub struct BudgetAllocation {
    remaining: Arc<AtomicU64>,
    allocated: u32,
}

impl BudgetAllocation {
    #[must_use]
    pub fn consume(&mut self, amount: u32) -> bool {
        if amount <= self.allocated {
            self.allocated -= amount;
            true
        } else {
            false
        }
    }
}

impl Drop for BudgetAllocation {
    fn drop(&mut self) {
        self.remaining
            .fetch_add(self.allocated as u64, Ordering::Relaxed);
    }
}

/////////////////////////////////////////// Permissions ///////////////////////////////////////////

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permissions {
    ReadOnly,
    ReadWrite,
    WriteOnly,
}

/////////////////////////////////////////// FileSystem ////////////////////////////////////////////

#[async_trait::async_trait]
pub trait FileSystem: Send + Sync {
    async fn search(&self, search: &str) -> Result<String, std::io::Error>;

    async fn view(
        &self,
        path: &str,
        view_range: Option<(u32, u32)>,
    ) -> Result<String, std::io::Error>;

    async fn str_replace(
        &self,
        path: &str,
        old_str: &str,
        new_str: &str,
    ) -> Result<String, std::io::Error>;

    async fn insert(
        &self,
        path: &str,
        insert_line: u32,
        new_str: &str,
    ) -> Result<String, std::io::Error>;
}

/////////////////////////////////////////////// Agent //////////////////////////////////////////////

#[async_trait::async_trait]
pub trait Agent: Send + Sync + Sized {
    async fn max_tokens(&self) -> u32 {
        1024
    }

    async fn model(&self) -> Model {
        Model::Known(KnownModel::ClaudeSonnet40)
    }

    async fn metadata(&self) -> Option<Metadata> {
        None
    }

    async fn stop_sequences(&self) -> Option<Vec<String>> {
        None
    }

    async fn system(&self) -> Option<SystemPrompt> {
        None
    }

    async fn temperature(&self) -> Option<f32> {
        None
    }

    async fn thinking(&self) -> Option<ThinkingConfig> {
        None
    }

    async fn tool_choice(&self) -> Option<ToolChoice> {
        None
    }

    async fn tools(&self) -> Vec<Arc<dyn Tool<Self>>> {
        vec![]
    }

    async fn top_k(&self) -> Option<u32> {
        None
    }

    async fn top_p(&self) -> Option<f32> {
        None
    }

    async fn filesystem(&self) -> Option<&dyn FileSystem> {
        None
    }

    async fn handle_max_tokens(&self) -> Result<(), Error> {
        Ok(())
    }

    async fn handle_end_turn(&self) -> Result<(), Error> {
        Ok(())
    }

    async fn handle_stop_sequence(&self, sequence: Option<String>) -> Result<(), Error> {
        _ = sequence;
        Ok(())
    }

    async fn handle_refusal(&self, resp: Message) -> Result<(), Error> {
        _ = resp;
        Ok(())
    }

    async fn hook_message_create_params(&self, req: &MessageCreateParams) -> Result<(), Error> {
        _ = req;
        Ok(())
    }

    async fn hook_message(&self, resp: &Message) -> Result<(), Error> {
        _ = resp;
        Ok(())
    }

    async fn take_turn(
        &mut self,
        client: &Anthropic,
        messages: &mut Vec<MessageParam>,
        budget: &Arc<Budget>,
    ) -> Result<(), Error> {
        self.take_default_turn(client, messages, budget).await
    }

    async fn take_default_turn(
        &mut self,
        client: &Anthropic,
        messages: &mut Vec<MessageParam>,
        budget: &Arc<Budget>,
    ) -> Result<(), Error> {
        let mut final_content = MessageParamContent::Array(vec![]);
        let Some(mut tokens_rem) = budget.allocate(self.max_tokens().await) else {
            return self.handle_max_tokens().await;
        };

        while tokens_rem.allocated > self.thinking().await.map(|t| t.num_tokens()).unwrap_or(0) {
            let tools = self
                .tools()
                .await
                .into_iter()
                .map(|tool| tool.to_param())
                .collect::<Vec<_>>();
            let req = MessageCreateParams {
                max_tokens: tokens_rem.allocated,
                model: self.model().await,
                messages: messages.clone(),
                metadata: self.metadata().await,
                stop_sequences: self.stop_sequences().await,
                system: self.system().await,
                thinking: self.thinking().await,
                temperature: self.temperature().await,
                top_k: self.top_k().await,
                top_p: self.top_p().await,
                stream: false,
                tool_choice: self.tool_choice().await,
                tools: Some(tools),
            };
            self.hook_message_create_params(&req).await?;
            let resp = client.send(req).await?;
            self.hook_message(&resp).await?;
            let mut tool_results = vec![];

            let assistant_message = MessageParam {
                role: MessageRole::Assistant,
                content: MessageParamContent::Array(resp.content.clone()),
            };
            push_or_merge_message(messages, assistant_message);
            merge_message_content(
                &mut final_content,
                MessageParamContent::Array(resp.content.clone()),
            );

            let _ = tokens_rem.consume(resp.usage.output_tokens as u32);
            match resp.stop_reason {
                None | Some(StopReason::EndTurn) => return self.handle_end_turn().await,
                Some(StopReason::MaxTokens) => return self.handle_max_tokens().await,
                Some(StopReason::StopSequence) => {
                    return self.handle_stop_sequence(resp.stop_sequence).await;
                }
                Some(StopReason::Refusal) => return self.handle_refusal(resp).await,
                Some(StopReason::PauseTurn) => {
                    continue;
                }
                Some(StopReason::ToolUse) => {
                    let mut tools_and_blocks = vec![];
                    for block in resp.content.iter() {
                        if let ContentBlock::ToolUse(tool_use) = block {
                            let Some(tool) = self
                                .tools()
                                .await
                                .iter()
                                .find(|t| t.name() == tool_use.name)
                                .cloned()
                            else {
                                tools_and_blocks.push((
                                    tool_use.clone(),
                                    Arc::new(ToolNotFound(tool_use.name.clone())) as _,
                                ));
                                continue;
                            };
                            tools_and_blocks.push((tool_use.clone(), tool));
                        }
                    }
                    let mut futures = Vec::with_capacity(tools_and_blocks.len());
                    for (tool_use, tool) in tools_and_blocks.iter() {
                        let callback = tool.callback();
                        futures.push(async {
                            let this = &*self;
                            let tool_use = tool_use.clone();
                            async move { callback.compute_tool_result(client, this, &tool_use).await }.await
                        });
                    }
                    let intermediate_tool_results = futures::future::join_all(futures).await;
                    for ((tool_use, tool), result) in
                        std::iter::zip(tools_and_blocks, intermediate_tool_results)
                    {
                        let callback = tool.callback();
                        match callback
                            .apply_tool_result(client, self, &tool_use, result)
                            .await
                        {
                            ControlFlow::Continue(result) => match result {
                                Ok(block) => tool_results.push(block.into()),
                                Err(block) => {
                                    tool_results.push(block.with_error(true).into());
                                }
                            },
                            ControlFlow::Break(err) => return Err(err),
                        }
                    }
                }
            }
            let user_message =
                MessageParam::new(MessageParamContent::Array(tool_results), MessageRole::User);
            push_or_merge_message(messages, user_message);
        }
        self.handle_max_tokens().await
    }

    async fn create_request(
        &self,
        max_tokens: u32,
        messages: Vec<MessageParam>,
    ) -> MessageCreateParams {
        let tools = self
            .tools()
            .await
            .iter()
            .map(|tool| tool.to_param())
            .collect::<Vec<_>>();
        MessageCreateParams {
            max_tokens,
            model: self.model().await,
            messages,
            metadata: self.metadata().await,
            stop_sequences: self.stop_sequences().await,
            system: self.system().await.clone(),
            thinking: self.thinking().await,
            temperature: self.temperature().await,
            top_k: self.top_k().await,
            top_p: self.top_p().await,
            stream: false,
            tool_choice: self.tool_choice().await,
            tools: Some(tools),
        }
    }

    async fn text_editor(&self, tool_use: ToolUseBlock) -> Result<String, std::io::Error> {
        #[derive(serde::Deserialize)]
        struct Command {
            command: String,
        }
        let cmd: Command = serde_json::from_value(tool_use.input.clone())?;
        match cmd.command.as_str() {
            "view" => {
                #[derive(serde::Deserialize)]
                struct ViewTool {
                    path: String,
                    view_range: Option<(u32, u32)>,
                }
                let args: ViewTool = serde_json::from_value(tool_use.input)?;
                self.view(&args.path, args.view_range).await
            }
            "str_replace" => {
                #[derive(serde::Deserialize)]
                struct StrReplaceTool {
                    path: String,
                    old_str: String,
                    new_str: String,
                }
                let args: StrReplaceTool = serde_json::from_value(tool_use.input)?;
                self.str_replace(&args.path, &args.old_str, &args.new_str)
                    .await
            }
            "insert" => {
                #[derive(serde::Deserialize)]
                struct InsertTool {
                    path: String,
                    insert_line: u32,
                    new_str: String,
                }
                let args: InsertTool = serde_json::from_value(tool_use.input)?;
                self.insert(&args.path, args.insert_line, &args.new_str)
                    .await
            }
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                format!("{} is not a supported tool", tool_use.name),
            )),
        }
    }

    async fn bash(&self, command: &str, restart: bool) -> Result<String, std::io::Error> {
        let _ = command;
        let _ = restart;
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "bash is not supported",
        ))
    }

    async fn search(&self, search: &str) -> Result<String, std::io::Error> {
        if let Some(fs) = self.filesystem().await {
            fs.search(search).await
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "search is not supported",
            ))
        }
    }

    async fn view(
        &self,
        path: &str,
        view_range: Option<(u32, u32)>,
    ) -> Result<String, std::io::Error> {
        if let Some(fs) = self.filesystem().await {
            fs.view(path, view_range).await
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "view is not supported",
            ))
        }
    }

    async fn str_replace(
        &self,
        path: &str,
        old_str: &str,
        new_str: &str,
    ) -> Result<String, std::io::Error> {
        if let Some(fs) = self.filesystem().await {
            fs.str_replace(path, old_str, new_str).await
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "str_replace is not supported",
            ))
        }
    }

    async fn insert(
        &self,
        path: &str,
        insert_line: u32,
        new_str: &str,
    ) -> Result<String, std::io::Error> {
        if let Some(fs) = self.filesystem().await {
            fs.insert(path, insert_line, new_str).await
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "insert is not supported",
            ))
        }
    }
}

#[async_trait::async_trait]
impl Agent for () {}

#[async_trait::async_trait]
impl FileSystem for Path<'_> {
    async fn search(&self, search: &str) -> Result<String, std::io::Error> {
        let output = std::process::Command::new("grep")
            .args(["-nRI", search])
            .current_dir(self)
            .output()?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        let count = format!(
            "\nsearch returned {} results\n",
            stdout.chars().filter(|c| *c == '\n').count()
        );
        Ok(stdout.to_string() + "\n" + &stderr + &count)
    }

    async fn view(
        &self,
        path: &str,
        view_range: Option<(u32, u32)>,
    ) -> Result<String, std::io::Error> {
        let path = sanitize_path(self.clone(), path)?;
        if path.is_file() {
            let content = std::fs::read_to_string(path)?;
            let lines = content
                .split('\n')
                .enumerate()
                .filter(|(idx, _)| {
                    view_range
                        .map(|(start, limit)| (start..limit).contains(&(*idx as u32 + 1)))
                        .unwrap_or(true)
                })
                .map(|(_, line)| line)
                .collect::<Vec<_>>();
            let mut ret = lines.join("\n");
            ret.push('\n');
            Ok(ret)
        } else if path.is_dir() {
            let mut listing = String::new();
            for dirent in std::fs::read_dir(&path)? {
                let dirent = dirent?;
                let p = Path::try_from(dirent.path()).map_err(std::io::Error::other)?;
                if let Some(p) = p.strip_prefix(path.clone()) {
                    listing.push_str(p.as_str());
                    listing.push('\n');
                }
            }
            Ok(listing)
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "viewing non-standard file types is not supported",
            ))
        }
    }

    async fn str_replace(
        &self,
        path: &str,
        old_str: &str,
        new_str: &str,
    ) -> Result<String, std::io::Error> {
        let path = sanitize_path(self.clone(), path)?;
        if path.is_file() {
            let content = std::fs::read_to_string(&path)?;
            let count = content.matches(old_str).count();
            if count == 0 {
                Err(std::io::Error::new(
                    std::io::ErrorKind::Unsupported,
                    "old_str not found in file",
                ))
            } else if count > 1 {
                Err(std::io::Error::new(
                    std::io::ErrorKind::Unsupported,
                    "old_str found in file more than once",
                ))
            } else {
                let content = content.replace(old_str, new_str);
                std::fs::write(path, content)?;
                Ok("success".to_string())
            }
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "editing non-standard file types is not supported",
            ))
        }
    }

    async fn insert(
        &self,
        path: &str,
        insert_line: u32,
        new_str: &str,
    ) -> Result<String, std::io::Error> {
        let path = sanitize_path(self.clone(), path)?;
        if path.is_file() {
            let content = std::fs::read_to_string(&path)?;
            let lines = content
                .split('\n')
                .enumerate()
                .map(|(idx, line)| {
                    if idx == insert_line as usize {
                        new_str.to_string() + "\n" + line
                    } else {
                        line.to_string()
                    }
                })
                .collect::<Vec<_>>();
            let mut out = lines.join("\n");
            out.push('\n');
            std::fs::write(path, out)?;
            Ok("success".to_string())
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "editing non-standard file types is not supported",
            ))
        }
    }
}

/////////////////////////////////////////////// Mount //////////////////////////////////////////////

pub struct Mount {
    path: Path<'static>,
    perm: Permissions,
    fs: Box<dyn FileSystem>,
}

#[async_trait::async_trait]
impl FileSystem for Mount {
    async fn search(&self, search: &str) -> Result<String, std::io::Error> {
        match self.perm {
            Permissions::WriteOnly => Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "search not allowed with WriteOnly permissions",
            )),
            Permissions::ReadOnly | Permissions::ReadWrite => self.fs.search(search).await,
        }
    }

    async fn view(
        &self,
        path: &str,
        view_range: Option<(u32, u32)>,
    ) -> Result<String, std::io::Error> {
        match self.perm {
            Permissions::WriteOnly => Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "view not allowed with WriteOnly permissions",
            )),
            Permissions::ReadOnly | Permissions::ReadWrite => self.fs.view(path, view_range).await,
        }
    }

    async fn str_replace(
        &self,
        path: &str,
        old_str: &str,
        new_str: &str,
    ) -> Result<String, std::io::Error> {
        match self.perm {
            Permissions::ReadOnly => Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "str_replace not allowed with ReadOnly permissions",
            )),
            Permissions::WriteOnly | Permissions::ReadWrite => {
                self.fs.str_replace(path, old_str, new_str).await
            }
        }
    }

    async fn insert(
        &self,
        path: &str,
        insert_line: u32,
        new_str: &str,
    ) -> Result<String, std::io::Error> {
        match self.perm {
            Permissions::ReadOnly => Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "insert not allowed with ReadOnly permissions",
            )),
            Permissions::WriteOnly | Permissions::ReadWrite => {
                self.fs.insert(path, insert_line, new_str).await
            }
        }
    }
}

////////////////////////////////////////// MountHierarchy //////////////////////////////////////////

pub struct MountHierarchy {
    mounts: Vec<Mount>,
}

impl MountHierarchy {
    pub fn mount(
        &mut self,
        path: Path,
        perm: Permissions,
        fs: impl FileSystem + 'static,
    ) -> Result<(), String> {
        if !path.is_abs() {
            return Err("path must be absolute".to_string());
        }
        for mount in self.mounts.iter() {
            // If mount.path is a prefix of the current mount, then error.
            if mount.path.strip_prefix(path.clone()).is_some() && mount.path != path {
                return Err(format!(
                    "path must extend existing paths: {} masks {path}",
                    mount.path
                ));
            }
        }
        if self.mounts.is_empty() && path != "/".into() {
            return Err("initial mount point must be /".to_string());
        }
        let path = path.into_owned();
        let fs = Box::new(fs);
        self.mounts.push(Mount { path, perm, fs });
        Ok(())
    }

    fn fs_for_path(&self, path: &str) -> Result<(&dyn FileSystem, Path<'static>), std::io::Error> {
        for mount in self.mounts.iter().rev() {
            if let Some(path) = Path::from(path).strip_prefix(mount.path.clone()) {
                let path = path.into_owned();
                return Ok((mount, path));
            }
        }
        Err(std::io::Error::other(
            "filesystem not initialized".to_string(),
        ))
    }
}

#[async_trait::async_trait]
impl FileSystem for MountHierarchy {
    async fn search(&self, search: &str) -> Result<String, std::io::Error> {
        let mut output = String::new();
        for mount in self.mounts.iter() {
            output += &mount.search(search).await?;
            if !output.ends_with('\n') {
                output.push('\n');
            }
        }
        Ok(output)
    }

    async fn view(
        &self,
        path: &str,
        view_range: Option<(u32, u32)>,
    ) -> Result<String, std::io::Error> {
        let (fs, path) = self.fs_for_path(path)?;
        fs.view(path.as_str(), view_range).await
    }

    async fn str_replace(
        &self,
        path: &str,
        old_str: &str,
        new_str: &str,
    ) -> Result<String, std::io::Error> {
        let (fs, path) = self.fs_for_path(path)?;
        fs.str_replace(path.as_str(), old_str, new_str).await
    }

    async fn insert(
        &self,
        path: &str,
        insert_line: u32,
        new_str: &str,
    ) -> Result<String, std::io::Error> {
        let (fs, path) = self.fs_for_path(path)?;
        fs.insert(path.as_str(), insert_line, new_str).await
    }
}

/////////////////////////////////////////////// Misc ///////////////////////////////////////////////

fn sanitize_path(base: Path, path: &str) -> Result<Path<'static>, std::io::Error> {
    let path = Path::from(path);
    if path
        .components()
        .any(|c| matches!(c, utf8path::Component::AppDefined))
    {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "viewing // paths is not supported",
        ))
    } else if path
        .components()
        .any(|c| matches!(c, utf8path::Component::ParentDir))
    {
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            ".. path name prohibited",
        ))
    } else {
        let path = path.as_str().trim_start_matches('/');
        Ok(base.join(path).into_owned())
    }
}

/////////////////////////////////////////////// tests //////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_new_creates_with_correct_amount() {
        let budget = Budget::new(1000);
        assert_eq!(budget.remaining.load(Ordering::Relaxed), 1000);
    }

    #[test]
    fn budget_allocate_succeeds_when_sufficient_tokens() {
        let budget = Budget::new(1000);
        let allocation = budget.allocate(500);
        assert!(allocation.is_some());
        assert_eq!(budget.remaining.load(Ordering::Relaxed), 500);

        let allocation = allocation.unwrap();
        assert_eq!(allocation.allocated, 500);
    }

    #[test]
    fn budget_allocate_fails_when_insufficient_tokens() {
        let budget = Budget::new(500);
        let allocation = budget.allocate(1000);
        assert!(allocation.is_none());
        assert_eq!(budget.remaining.load(Ordering::Relaxed), 500);
    }

    #[test]
    fn budget_allocate_exact_amount() {
        let budget = Budget::new(500);
        let allocation = budget.allocate(500);
        assert!(allocation.is_some());
        assert_eq!(budget.remaining.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn budget_allocate_zero_tokens() {
        let budget = Budget::new(1000);
        let allocation = budget.allocate(0);
        assert!(allocation.is_some());
        assert_eq!(budget.remaining.load(Ordering::Relaxed), 1000);

        let allocation = allocation.unwrap();
        assert_eq!(allocation.allocated, 0);
    }

    #[test]
    fn budget_allocation_consume_valid_amount() {
        let budget = Budget::new(1000);
        let mut allocation = budget.allocate(500).unwrap();

        assert!(allocation.consume(200));
        assert_eq!(allocation.allocated, 300);

        assert!(allocation.consume(300));
        assert_eq!(allocation.allocated, 0);
    }

    #[test]
    fn budget_allocation_consume_excessive_amount() {
        let budget = Budget::new(1000);
        let mut allocation = budget.allocate(500).unwrap();

        assert!(!allocation.consume(600));
        assert_eq!(allocation.allocated, 500);
    }

    #[test]
    fn budget_allocation_consume_exact_amount() {
        let budget = Budget::new(1000);
        let mut allocation = budget.allocate(500).unwrap();

        assert!(allocation.consume(500));
        assert_eq!(allocation.allocated, 0);
    }

    #[test]
    fn budget_allocation_drop_returns_remaining_tokens() {
        let budget = Budget::new(1000);
        {
            let mut allocation = budget.allocate(500).unwrap();
            let _ = allocation.consume(200);
            // allocation goes out of scope here with 300 tokens remaining
        }
        // Should have returned 300 tokens to budget
        assert_eq!(budget.remaining.load(Ordering::Relaxed), 800);
    }

    #[test]
    fn budget_allocation_drop_returns_all_tokens_when_none_consumed() {
        let budget = Budget::new(1000);
        {
            let _allocation = budget.allocate(500).unwrap();
            // allocation goes out of scope here with all 500 tokens remaining
        }
        // Should have returned all 500 tokens to budget
        assert_eq!(budget.remaining.load(Ordering::Relaxed), 1000);
    }

    #[test]
    fn budget_allocation_drop_returns_zero_when_all_consumed() {
        let budget = Budget::new(1000);
        {
            let mut allocation = budget.allocate(500).unwrap();
            let _ = allocation.consume(500);
            // allocation goes out of scope here with 0 tokens remaining
        }
        // Should return 0 tokens to budget
        assert_eq!(budget.remaining.load(Ordering::Relaxed), 500);
    }

    #[test]
    fn budget_multiple_allocations() {
        let budget = Budget::new(1000);

        let alloc1 = budget.allocate(300);
        assert!(alloc1.is_some());
        assert_eq!(budget.remaining.load(Ordering::Relaxed), 700);

        let alloc2 = budget.allocate(400);
        assert!(alloc2.is_some());
        assert_eq!(budget.remaining.load(Ordering::Relaxed), 300);

        let alloc3 = budget.allocate(400);
        assert!(alloc3.is_none());
        assert_eq!(budget.remaining.load(Ordering::Relaxed), 300);
    }

    #[test]
    fn budget_concurrent_allocation_safety() {
        use std::thread;

        let budget = Arc::new(Budget::new(1000));
        let mut handles = vec![];

        // Spawn 10 threads each trying to allocate 200 tokens
        for _ in 0..10 {
            let budget_clone = Arc::clone(&budget);
            handles.push(thread::spawn(move || budget_clone.allocate(200)));
        }

        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        let successful_allocations = results.iter().filter(|r| r.is_some()).count();

        // Should only allow 5 successful allocations (5 * 200 = 1000)
        assert_eq!(successful_allocations, 5);
        assert_eq!(budget.remaining.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn budget_allocation_with_mixed_operations() {
        let budget = Budget::new(1000);

        let mut alloc1 = budget.allocate(400).unwrap();
        assert_eq!(budget.remaining.load(Ordering::Relaxed), 600);

        let _ = alloc1.consume(150);
        assert_eq!(alloc1.allocated, 250);

        let mut alloc2 = budget.allocate(300).unwrap();
        assert_eq!(budget.remaining.load(Ordering::Relaxed), 300);

        let _ = alloc2.consume(100);
        assert_eq!(alloc2.allocated, 200);

        // Drop alloc1, should return 250 tokens
        drop(alloc1);
        assert_eq!(budget.remaining.load(Ordering::Relaxed), 550);

        // Should now be able to allocate more
        let alloc3 = budget.allocate(500);
        assert!(alloc3.is_some());
        assert_eq!(budget.remaining.load(Ordering::Relaxed), 50);
    }

    // MountHierarchy tests

    #[test]
    fn mount_hierarchy_initial_mount_must_be_root() {
        let mut hierarchy = MountHierarchy { mounts: vec![] };

        // First mount must be /
        let result = hierarchy.mount("/home".into(), Permissions::ReadWrite, Path::from("/tmp"));
        assert_eq!(result, Err("initial mount point must be /".to_string()));

        // After mounting /, other paths can be mounted
        assert!(hierarchy
            .mount("/".into(), Permissions::ReadWrite, Path::from("/tmp"))
            .is_ok());
        assert!(hierarchy
            .mount("/home".into(), Permissions::ReadWrite, Path::from("/tmp"))
            .is_ok());
    }

    #[test]
    fn mount_hierarchy_path_must_be_absolute() {
        let mut hierarchy = MountHierarchy { mounts: vec![] };

        let result = hierarchy.mount(
            "relative/path".into(),
            Permissions::ReadWrite,
            Path::from("/tmp"),
        );
        assert_eq!(result, Err("path must be absolute".to_string()));

        let result = hierarchy.mount("./path".into(), Permissions::ReadWrite, Path::from("/tmp"));
        assert_eq!(result, Err("path must be absolute".to_string()));

        let result = hierarchy.mount("../path".into(), Permissions::ReadWrite, Path::from("/tmp"));
        assert_eq!(result, Err("path must be absolute".to_string()));
    }

    #[test]
    fn mount_hierarchy_cannot_mask_existing_mount() {
        let mut hierarchy = MountHierarchy { mounts: vec![] };

        // Mount / and /home
        assert!(hierarchy
            .mount("/".into(), Permissions::ReadWrite, Path::from("/tmp"))
            .is_ok());
        assert!(hierarchy
            .mount("/home".into(), Permissions::ReadWrite, Path::from("/tmp"))
            .is_ok());

        // Cannot mount / again since it would mask /home
        let result = hierarchy.mount("/".into(), Permissions::ReadWrite, Path::from("/tmp"));
        assert!(result.is_err());
        let err_msg = result.unwrap_err();
        eprintln!("err_msg: {err_msg:?}");
        assert!(err_msg.contains("path must extend existing paths"));
        assert!(err_msg.contains("/home masks"));
    }

    #[test]
    fn mount_hierarchy_can_mount_same_path_multiple_times() {
        let mut hierarchy = MountHierarchy { mounts: vec![] };

        // Mount /
        assert!(hierarchy
            .mount("/".into(), Permissions::ReadWrite, Path::from("/tmp1"))
            .is_ok());

        // Can mount / again (overlays previous mount)
        assert!(hierarchy
            .mount("/".into(), Permissions::ReadWrite, Path::from("/tmp2"))
            .is_ok());

        assert_eq!(hierarchy.mounts.len(), 2);
    }

    #[test]
    fn mount_hierarchy_fs_for_path_finds_most_specific_mount() {
        let mut hierarchy = MountHierarchy { mounts: vec![] };

        // Mount different paths
        assert!(hierarchy
            .mount("/".into(), Permissions::ReadWrite, Path::from("/root"))
            .is_ok());
        assert!(hierarchy
            .mount(
                "/home".into(),
                Permissions::ReadWrite,
                Path::from("/home_fs")
            )
            .is_ok());
        assert!(hierarchy
            .mount(
                "/home/user".into(),
                Permissions::ReadWrite,
                Path::from("/user_fs")
            )
            .is_ok());

        // Check that fs_for_path returns the most specific mount
        let fs = hierarchy.fs_for_path("/file.txt").unwrap().0;
        // Cast both to raw pointers to compare addresses without vtable metadata
        let fs_ptr = fs as *const dyn FileSystem as *const ();
        let expected_ptr = &hierarchy.mounts[0] as &dyn FileSystem as *const dyn FileSystem as *const ();
        assert_eq!(fs_ptr, expected_ptr);

        let fs = hierarchy.fs_for_path("/home/file.txt").unwrap().0;
        let fs_ptr = fs as *const dyn FileSystem as *const ();
        let expected_ptr = &hierarchy.mounts[1] as &dyn FileSystem as *const dyn FileSystem as *const ();
        assert_eq!(fs_ptr, expected_ptr);

        let fs = hierarchy.fs_for_path("/home/user/file.txt").unwrap().0;
        let fs_ptr = fs as *const dyn FileSystem as *const ();
        let expected_ptr = &hierarchy.mounts[2] as &dyn FileSystem as *const dyn FileSystem as *const ();
        assert_eq!(fs_ptr, expected_ptr);
    }

    #[test]
    fn mount_hierarchy_fs_for_path_error_when_no_mount() {
        let hierarchy = MountHierarchy { mounts: vec![] };

        let result = hierarchy.fs_for_path("/any/path");
        assert!(result.is_err());
        if let Err(err) = result {
            assert_eq!(err.kind(), std::io::ErrorKind::Other);
            assert_eq!(err.to_string(), "filesystem not initialized");
        }
    }

    enum MockResult {
        Ok(String),
        Err(std::io::ErrorKind, String),
    }

    struct MockFileSystem {
        search_result: MockResult,
        view_result: MockResult,
        str_replace_result: MockResult,
        insert_result: MockResult,
    }

    impl MockFileSystem {
        fn new_ok(name: &str) -> Self {
            Self {
                search_result: MockResult::Ok(format!("search from {name}")),
                view_result: MockResult::Ok(format!("view from {name}")),
                str_replace_result: MockResult::Ok(format!("str_replace from {name}")),
                insert_result: MockResult::Ok(format!("insert from {name}")),
            }
        }

        fn new_err(name: &str, kind: std::io::ErrorKind) -> Self {
            Self {
                search_result: MockResult::Err(kind, format!("search error from {name}")),
                view_result: MockResult::Err(kind, format!("view error from {name}")),
                str_replace_result: MockResult::Err(kind, format!("str_replace error from {name}")),
                insert_result: MockResult::Err(kind, format!("insert error from {name}")),
            }
        }
    }

    impl MockResult {
        fn to_result(&self) -> Result<String, std::io::Error> {
            match self {
                MockResult::Ok(s) => Ok(s.clone()),
                MockResult::Err(kind, msg) => Err(std::io::Error::new(*kind, msg.clone())),
            }
        }
    }

    #[async_trait::async_trait]
    impl FileSystem for MockFileSystem {
        async fn search(&self, _search: &str) -> Result<String, std::io::Error> {
            self.search_result.to_result()
        }

        async fn view(
            &self,
            _path: &str,
            _view_range: Option<(u32, u32)>,
        ) -> Result<String, std::io::Error> {
            self.view_result.to_result()
        }

        async fn str_replace(
            &self,
            _path: &str,
            _old_str: &str,
            _new_str: &str,
        ) -> Result<String, std::io::Error> {
            self.str_replace_result.to_result()
        }

        async fn insert(
            &self,
            _path: &str,
            _insert_line: u32,
            _new_str: &str,
        ) -> Result<String, std::io::Error> {
            self.insert_result.to_result()
        }
    }

    #[tokio::test]
    async fn mount_hierarchy_search_aggregates_all_mounts() {
        let mut hierarchy = MountHierarchy { mounts: vec![] };

        hierarchy
            .mount(
                "/".into(),
                Permissions::ReadWrite,
                MockFileSystem::new_ok("root"),
            )
            .unwrap();
        hierarchy
            .mount(
                "/home".into(),
                Permissions::ReadWrite,
                MockFileSystem::new_ok("home"),
            )
            .unwrap();
        hierarchy
            .mount(
                "/usr".into(),
                Permissions::ReadWrite,
                MockFileSystem::new_ok("usr"),
            )
            .unwrap();

        let result = hierarchy.search("test").await.unwrap();
        assert_eq!(
            result,
            "search from root\nsearch from home\nsearch from usr\n"
        );
    }

    #[tokio::test]
    async fn mount_hierarchy_search_error_propagates() {
        let mut hierarchy = MountHierarchy { mounts: vec![] };

        hierarchy
            .mount(
                "/".into(),
                Permissions::ReadWrite,
                MockFileSystem::new_ok("root"),
            )
            .unwrap();
        hierarchy
            .mount(
                "/home".into(),
                Permissions::ReadWrite,
                MockFileSystem::new_err("home", std::io::ErrorKind::PermissionDenied),
            )
            .unwrap();

        let result = hierarchy.search("test").await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
        assert!(err.to_string().contains("search error from home"));
    }

    #[tokio::test]
    async fn mount_hierarchy_search_adds_newlines() {
        let mut hierarchy = MountHierarchy { mounts: vec![] };

        // Mock that doesn't end with newline
        let mut mock = MockFileSystem::new_ok("no_newline");
        mock.search_result = MockResult::Ok("result without newline".to_string());

        hierarchy
            .mount("/".into(), Permissions::ReadWrite, mock)
            .unwrap();
        hierarchy
            .mount(
                "/home".into(),
                Permissions::ReadWrite,
                MockFileSystem::new_ok("home"),
            )
            .unwrap();

        let result = hierarchy.search("test").await.unwrap();
        assert_eq!(result, "result without newline\nsearch from home\n");
    }

    #[tokio::test]
    async fn mount_hierarchy_view_uses_correct_filesystem() {
        let mut hierarchy = MountHierarchy { mounts: vec![] };

        hierarchy
            .mount(
                "/".into(),
                Permissions::ReadWrite,
                MockFileSystem::new_ok("root"),
            )
            .unwrap();
        hierarchy
            .mount(
                "/home".into(),
                Permissions::ReadWrite,
                MockFileSystem::new_ok("home"),
            )
            .unwrap();

        let result = hierarchy.view("/file.txt", None).await.unwrap();
        assert_eq!(result, "view from root");

        let result = hierarchy
            .view("/home/file.txt", Some((1, 10)))
            .await
            .unwrap();
        assert_eq!(result, "view from home");
    }

    #[tokio::test]
    async fn mount_hierarchy_view_error_no_filesystem() {
        let hierarchy = MountHierarchy { mounts: vec![] };

        let result = hierarchy.view("/file.txt", None).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::Other);
        assert_eq!(err.to_string(), "filesystem not initialized");
    }

    #[tokio::test]
    async fn mount_hierarchy_view_error_from_filesystem() {
        let mut hierarchy = MountHierarchy { mounts: vec![] };

        hierarchy
            .mount(
                "/".into(),
                Permissions::ReadWrite,
                MockFileSystem::new_err("root", std::io::ErrorKind::NotFound),
            )
            .unwrap();

        let result = hierarchy.view("/file.txt", None).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
        assert!(err.to_string().contains("view error from root"));
    }

    #[tokio::test]
    async fn mount_hierarchy_str_replace_uses_correct_filesystem() {
        let mut hierarchy = MountHierarchy { mounts: vec![] };

        hierarchy
            .mount(
                "/".into(),
                Permissions::ReadWrite,
                MockFileSystem::new_ok("root"),
            )
            .unwrap();
        hierarchy
            .mount(
                "/home".into(),
                Permissions::ReadWrite,
                MockFileSystem::new_ok("home"),
            )
            .unwrap();

        let result = hierarchy
            .str_replace("/file.txt", "old", "new")
            .await
            .unwrap();
        assert_eq!(result, "str_replace from root");

        let result = hierarchy
            .str_replace("/home/file.txt", "old", "new")
            .await
            .unwrap();
        assert_eq!(result, "str_replace from home");
    }

    #[tokio::test]
    async fn mount_hierarchy_str_replace_error_no_filesystem() {
        let hierarchy = MountHierarchy { mounts: vec![] };

        let result = hierarchy.str_replace("/file.txt", "old", "new").await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::Other);
        assert_eq!(err.to_string(), "filesystem not initialized");
    }

    #[tokio::test]
    async fn mount_hierarchy_str_replace_error_from_filesystem() {
        let mut hierarchy = MountHierarchy { mounts: vec![] };

        hierarchy
            .mount(
                "/".into(),
                Permissions::ReadWrite,
                MockFileSystem::new_err("root", std::io::ErrorKind::PermissionDenied),
            )
            .unwrap();

        let result = hierarchy.str_replace("/file.txt", "old", "new").await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
        assert!(err.to_string().contains("str_replace error from root"));
    }

    #[tokio::test]
    async fn mount_hierarchy_insert_uses_correct_filesystem() {
        let mut hierarchy = MountHierarchy { mounts: vec![] };

        hierarchy
            .mount(
                "/".into(),
                Permissions::ReadWrite,
                MockFileSystem::new_ok("root"),
            )
            .unwrap();
        hierarchy
            .mount(
                "/home".into(),
                Permissions::ReadWrite,
                MockFileSystem::new_ok("home"),
            )
            .unwrap();

        let result = hierarchy.insert("/file.txt", 5, "new line").await.unwrap();
        assert_eq!(result, "insert from root");

        let result = hierarchy
            .insert("/home/file.txt", 10, "new line")
            .await
            .unwrap();
        assert_eq!(result, "insert from home");
    }

    #[tokio::test]
    async fn mount_hierarchy_insert_error_no_filesystem() {
        let hierarchy = MountHierarchy { mounts: vec![] };

        let result = hierarchy.insert("/file.txt", 5, "new line").await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::Other);
        assert_eq!(err.to_string(), "filesystem not initialized");
    }

    #[tokio::test]
    async fn mount_hierarchy_insert_error_from_filesystem() {
        let mut hierarchy = MountHierarchy { mounts: vec![] };

        hierarchy
            .mount(
                "/".into(),
                Permissions::ReadWrite,
                MockFileSystem::new_err("root", std::io::ErrorKind::AddrInUse),
            )
            .unwrap();

        let result = hierarchy.insert("/file.txt", 5, "new line").await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::AddrInUse);
        assert!(err.to_string().contains("insert error from root"));
    }

    #[tokio::test]
    async fn mount_hierarchy_overlay_mounts() {
        let mut hierarchy = MountHierarchy { mounts: vec![] };

        // First mount at /
        hierarchy
            .mount(
                "/".into(),
                Permissions::ReadWrite,
                MockFileSystem::new_ok("first"),
            )
            .unwrap();

        // Overlay mount at same path
        hierarchy
            .mount(
                "/".into(),
                Permissions::ReadWrite,
                MockFileSystem::new_ok("second"),
            )
            .unwrap();

        // Should use the most recent mount
        let result = hierarchy.view("/file.txt", None).await.unwrap();
        assert_eq!(result, "view from second");
    }

    #[test]
    fn mount_hierarchy_complex_path_scenarios() {
        let mut hierarchy = MountHierarchy { mounts: vec![] };

        // Mount various paths
        assert!(hierarchy
            .mount("/".into(), Permissions::ReadWrite, Path::from("/root"))
            .is_ok());
        assert!(hierarchy
            .mount("/home".into(), Permissions::ReadWrite, Path::from("/home"))
            .is_ok());
        assert!(hierarchy
            .mount(
                "/home/user".into(),
                Permissions::ReadWrite,
                Path::from("/user")
            )
            .is_ok());
        assert!(hierarchy
            .mount("/var".into(), Permissions::ReadWrite, Path::from("/var"))
            .is_ok());
        assert!(hierarchy
            .mount(
                "/var/log".into(),
                Permissions::ReadWrite,
                Path::from("/log")
            )
            .is_ok());

        // Cannot mount path that would mask existing deeper paths
        let result = hierarchy.mount(
            "/home".into(),
            Permissions::ReadWrite,
            Path::from("/new_home"),
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("/home/user masks"));

        let result = hierarchy.mount(
            "/var".into(),
            Permissions::ReadWrite,
            Path::from("/new_var"),
        );
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("/var/log masks"));

        // Can mount paths that don't conflict
        assert!(hierarchy
            .mount("/usr".into(), Permissions::ReadWrite, Path::from("/usr"))
            .is_ok());
        assert!(hierarchy
            .mount(
                "/home/other".into(),
                Permissions::ReadWrite,
                Path::from("/other")
            )
            .is_ok());
    }

    // Permission tests
    #[tokio::test]
    async fn mount_permissions_readonly_allows_search_and_view() {
        let mut hierarchy = MountHierarchy { mounts: vec![] };

        hierarchy
            .mount(
                "/".into(),
                Permissions::ReadOnly,
                MockFileSystem::new_ok("readonly"),
            )
            .unwrap();

        // Search should work
        let result = hierarchy.search("test").await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "search from readonly\n");

        // View should work
        let result = hierarchy.view("/file.txt", None).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "view from readonly");
    }

    #[tokio::test]
    async fn mount_permissions_readonly_denies_write_operations() {
        let mut hierarchy = MountHierarchy { mounts: vec![] };

        hierarchy
            .mount(
                "/".into(),
                Permissions::ReadOnly,
                MockFileSystem::new_ok("readonly"),
            )
            .unwrap();

        // str_replace should fail
        let result = hierarchy.str_replace("/file.txt", "old", "new").await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
        assert!(err
            .to_string()
            .contains("str_replace not allowed with ReadOnly permissions"));

        // insert should fail
        let result = hierarchy.insert("/file.txt", 1, "new line").await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
        assert!(err
            .to_string()
            .contains("insert not allowed with ReadOnly permissions"));
    }

    #[tokio::test]
    async fn mount_permissions_writeonly_allows_write_operations() {
        let mut hierarchy = MountHierarchy { mounts: vec![] };

        hierarchy
            .mount(
                "/".into(),
                Permissions::WriteOnly,
                MockFileSystem::new_ok("writeonly"),
            )
            .unwrap();

        // str_replace should work
        let result = hierarchy.str_replace("/file.txt", "old", "new").await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "str_replace from writeonly");

        // insert should work
        let result = hierarchy.insert("/file.txt", 1, "new line").await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "insert from writeonly");
    }

    #[tokio::test]
    async fn mount_permissions_writeonly_denies_read_operations() {
        let mut hierarchy = MountHierarchy { mounts: vec![] };

        hierarchy
            .mount(
                "/".into(),
                Permissions::WriteOnly,
                MockFileSystem::new_ok("writeonly"),
            )
            .unwrap();

        // search should fail
        let result = hierarchy.search("test").await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
        assert!(err
            .to_string()
            .contains("search not allowed with WriteOnly permissions"));

        // view should fail
        let result = hierarchy.view("/file.txt", None).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
        assert!(err
            .to_string()
            .contains("view not allowed with WriteOnly permissions"));
    }

    #[tokio::test]
    async fn mount_permissions_readwrite_allows_all_operations() {
        let mut hierarchy = MountHierarchy { mounts: vec![] };

        hierarchy
            .mount(
                "/".into(),
                Permissions::ReadWrite,
                MockFileSystem::new_ok("readwrite"),
            )
            .unwrap();

        // All operations should work
        let result = hierarchy.search("test").await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "search from readwrite\n");

        let result = hierarchy.view("/file.txt", None).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "view from readwrite");

        let result = hierarchy.str_replace("/file.txt", "old", "new").await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "str_replace from readwrite");

        let result = hierarchy.insert("/file.txt", 1, "new line").await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "insert from readwrite");
    }

    #[tokio::test]
    async fn mount_permissions_different_mounts_different_permissions() {
        let mut hierarchy = MountHierarchy { mounts: vec![] };

        // Mount with different permissions
        hierarchy
            .mount(
                "/".into(),
                Permissions::ReadWrite,
                MockFileSystem::new_ok("root"),
            )
            .unwrap();
        hierarchy
            .mount(
                "/readonly".into(),
                Permissions::ReadOnly,
                MockFileSystem::new_ok("readonly"),
            )
            .unwrap();
        hierarchy
            .mount(
                "/writeonly".into(),
                Permissions::WriteOnly,
                MockFileSystem::new_ok("writeonly"),
            )
            .unwrap();

        // Root mount allows all operations
        let result = hierarchy.str_replace("/file.txt", "old", "new").await;
        assert!(result.is_ok());

        // ReadOnly mount denies write
        let result = hierarchy
            .str_replace("/readonly/file.txt", "old", "new")
            .await;
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied
        );

        // WriteOnly mount denies read
        let result = hierarchy.view("/writeonly/file.txt", None).await;
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().kind(),
            std::io::ErrorKind::PermissionDenied
        );
    }
}
