use std::future::Future;
use std::ops::ControlFlow;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use utf8path::Path;

use crate::{
    Anthropic, ContentBlock, Error, JsonSchema, KnownModel, Message, MessageCreateParams,
    MessageParam, MessageParamContent, MessageRole, Metadata, Model, StopReason, SystemPrompt,
    ThinkingConfig, ToolBash20241022, ToolBash20250124, ToolChoice, ToolParam, ToolResultBlock,
    ToolResultBlockContent, ToolTextEditor20250124, ToolTextEditor20250429, ToolUnionParam,
    ToolUseBlock, WebSearchTool20250305, merge_message_content, push_or_merge_message,
};

/////////////////////////////////////////////// Tool ///////////////////////////////////////////////

pub trait Tool<A: Agent> {
    fn name(&self) -> String;
    fn callback(&self) -> ToolResultCallback<A>;
    fn to_param(&self) -> ToolUnionParam;
}

impl<A: Agent> Tool<A> for ToolBash20241022 {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn callback(&self) -> ToolResultCallback<A> {
        Box::new(|tool_use| Box::pin(async move { bash_callback(tool_use) }))
    }

    fn to_param(&self) -> ToolUnionParam {
        ToolUnionParam::Bash20241022(self.clone())
    }
}

impl<A: Agent> Tool<A> for ToolBash20250124 {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn callback(&self) -> ToolResultCallback<A> {
        Box::new(|tool_use| Box::pin(async move { bash_callback(tool_use) }))
    }

    fn to_param(&self) -> ToolUnionParam {
        ToolUnionParam::Bash20250124(self.clone())
    }
}

#[derive(serde::Deserialize)]
pub struct BashTool {
    command: String,
    restart: bool,
}

pub fn bash_callback<A: Agent>(tool_use: ToolUseBlock) -> ToolResultApplier<A> {
    Box::new(|agent| {
        Box::pin(async move {
            let bash: BashTool = match serde_json::from_value(tool_use.input) {
                Ok(input) => input,
                Err(err) => {
                    return ControlFlow::Continue(Err(ToolResultBlock {
                        tool_use_id: tool_use.id,
                        content: Some(ToolResultBlockContent::String(err.to_string())),
                        is_error: Some(true),
                        cache_control: None,
                    }));
                }
            };
            match agent.bash(&bash.command, bash.restart).await {
                Ok(answer) => ControlFlow::Continue(Ok(ToolResultBlock {
                    tool_use_id: tool_use.id,
                    content: Some(ToolResultBlockContent::String(answer.to_string())),
                    is_error: None,
                    cache_control: None,
                })),
                Err(err) => ControlFlow::Continue(Err(ToolResultBlock {
                    tool_use_id: tool_use.id,
                    content: Some(ToolResultBlockContent::String(err.to_string())),
                    is_error: Some(true),
                    cache_control: None,
                })),
            }
        })
    })
}

impl<A: Agent> Tool<A> for ToolTextEditor20250124 {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn callback(&self) -> ToolResultCallback<A> {
        Box::new(|tool_use| Box::pin(async move { text_editor_callback(tool_use) }))
    }

    fn to_param(&self) -> ToolUnionParam {
        ToolUnionParam::TextEditor20250124(self.clone())
    }
}

impl<A: Agent> Tool<A> for ToolTextEditor20250429 {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn callback(&self) -> ToolResultCallback<A> {
        Box::new(|tool_use| Box::pin(async move { text_editor_callback(tool_use) }))
    }

    fn to_param(&self) -> ToolUnionParam {
        ToolUnionParam::TextEditor20250429(self.clone())
    }
}

fn text_editor_callback<A: Agent>(tool_use: ToolUseBlock) -> ToolResultApplier<A> {
    Box::new(|agent| {
        let id = tool_use.id.clone();
        Box::pin(async move {
            match agent.text_editor(tool_use).await {
                Ok(result) => ControlFlow::Continue(Ok(ToolResultBlock {
                    tool_use_id: id,
                    content: Some(ToolResultBlockContent::String(result)),
                    is_error: None,
                    cache_control: None,
                })),
                Err(err) => ControlFlow::Continue(Err(ToolResultBlock {
                    tool_use_id: id,
                    content: Some(ToolResultBlockContent::String(err.to_string())),
                    is_error: Some(true),
                    cache_control: None,
                })),
            }
        })
    })
}

impl<A: Agent> Tool<A> for WebSearchTool20250305 {
    fn name(&self) -> String {
        self.name.clone()
    }

    fn callback(&self) -> ToolResultCallback<A> {
        Box::new(|tool_use| Box::pin(async move { web_search_callback(tool_use) }))
    }

    fn to_param(&self) -> ToolUnionParam {
        ToolUnionParam::WebSearch20250305(self.clone())
    }
}

fn web_search_callback<A: Agent>(tool_use: ToolUseBlock) -> ToolResultApplier<A> {
    Box::new(move |_agent| {
        let id = tool_use.id.clone();
        Box::pin(async move {
            ControlFlow::Continue(Err(ToolResultBlock {
                tool_use_id: id,
                content: Some(ToolResultBlockContent::String(
                    "Web search is not implemented".to_string(),
                )),
                is_error: Some(true),
                cache_control: None,
            }))
        })
    })
}

pub struct ToolSearchFileSystem;

impl<A: Agent> Tool<A> for ToolSearchFileSystem {
    fn name(&self) -> String {
        "search_filesystem".to_string()
    }

    fn callback(&self) -> ToolResultCallback<A> {
        Box::new(|tool_use| Box::pin(async move { search_callback(tool_use) }))
    }

    fn to_param(&self) -> ToolUnionParam {
        let name = <Self as Tool<A>>::name(self).to_string();
        let input_schema = SearchTool::json_schema();
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

#[derive(serde::Deserialize)]
pub struct SearchTool {
    query: String,
}

impl JsonSchema for SearchTool {
    fn json_schema() -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "The search query to find on the filesystem."
                }
            },
            "required": ["query"]
        })
    }
}

pub fn search_callback<A: Agent>(tool_use: ToolUseBlock) -> ToolResultApplier<A> {
    Box::new(|agent| {
        Box::pin(async move {
            let search: SearchTool = match serde_json::from_value(tool_use.input) {
                Ok(input) => input,
                Err(err) => {
                    return ControlFlow::Continue(Err(ToolResultBlock {
                        tool_use_id: tool_use.id,
                        content: Some(ToolResultBlockContent::String(err.to_string())),
                        is_error: Some(true),
                        cache_control: None,
                    }));
                }
            };
            match agent.search(&search.query).await {
                Ok(answer) => ControlFlow::Continue(Ok(ToolResultBlock {
                    tool_use_id: tool_use.id,
                    content: Some(ToolResultBlockContent::String(answer.to_string())),
                    is_error: None,
                    cache_control: None,
                })),
                Err(err) => ControlFlow::Continue(Err(ToolResultBlock {
                    tool_use_id: tool_use.id,
                    content: Some(ToolResultBlockContent::String(err.to_string())),
                    is_error: Some(true),
                    cache_control: None,
                })),
            }
        })
    })
}

//////////////////////////////////////////// ToolResult ////////////////////////////////////////////

pub type ToolResult = ControlFlow<Error, Result<ToolResultBlock, ToolResultBlock>>;

///////////////////////////////////////// ToolResultApplier ////////////////////////////////////////

pub type ToolResultApplier<A> =
    Box<dyn for<'a> FnOnce(&'a A) -> Pin<Box<dyn Future<Output = ToolResult> + Send + 'a>> + Send>;

//////////////////////////////////////// ToolResultCallback ////////////////////////////////////////

pub type ToolResultCallback<A> =
    Box<dyn Fn(ToolUseBlock) -> Pin<Box<dyn Future<Output = ToolResultApplier<A>> + Send>> + Send>;

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

    async fn tools(&self) -> &[Box<dyn Tool<Self>>] {
        &[]
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
        &self,
        client: &Anthropic,
        messages: &mut Vec<MessageParam>,
        budget: &Arc<Budget>,
    ) -> Result<(), Error> {
        let mut final_content = MessageParamContent::Array(vec![]);
        let Some(mut tokens_rem) = budget.allocate(self.max_tokens().await) else {
            return self.handle_max_tokens().await;
        };

        while tokens_rem.allocated > self.thinking().await.map(|t| t.num_tokens()).unwrap_or(0) {
            let agent_tools = self.tools().await;
            let api_tools = if !agent_tools.is_empty() {
                Some(agent_tools.iter().map(|tool| tool.to_param()).collect())
            } else {
                None
            };
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
                tools: api_tools,
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
                    let mut futures = Vec::with_capacity(resp.content.len());
                    for block in resp.content.iter() {
                        if let ContentBlock::ToolUse(tool_use) = block {
                            futures.push(self.process_tool_use(tool_use));
                        }
                    }
                    let tool_result_appliers = futures::future::join_all(futures).await;
                    for tool_result_applier in tool_result_appliers {
                        match tool_result_applier(self).await {
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

    async fn process_tool_use(&self, tool_use: &ToolUseBlock) -> ToolResultApplier<Self> {
        let Some(tool) = self
            .tools()
            .await
            .iter()
            .find(|t| t.name() == tool_use.name)
        else {
            let id = tool_use.id.clone();
            return Box::new(|_| {
                Box::pin(async move {
                    ControlFlow::Continue(Err(ToolResultBlock {
                        tool_use_id: id.clone(),
                        content: Some(ToolResultBlockContent::String(
                            "error: no such tool".to_string(),
                        )),
                        is_error: Some(true),
                        cache_control: None,
                    }))
                })
            });
        };
        (tool.callback())(tool_use.clone()).await
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
}
