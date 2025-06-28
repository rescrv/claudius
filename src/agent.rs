use std::future::Future;
use std::ops::ControlFlow;
use std::pin::Pin;
use std::sync::Arc;

use utf8path::Path;

use crate::{
    merge_message_content, push_or_merge_message, Anthropic, ContentBlock, Error, JsonSchema,
    MessageCreateParams, MessageParam, MessageParamContent, MessageRole, Metadata, Model,
    StopReason, SystemPrompt, ThinkingConfig, ToolBash20250124, ToolChoice, ToolParam,
    ToolResultBlock, ToolResultBlockContent, ToolTextEditor20250124, ToolUnionParam, ToolUseBlock,
    WebSearchTool20250305,
};

/////////////////////////////////////////////// Agent //////////////////////////////////////////////

#[async_trait::async_trait]
pub trait Agent: Send {
    async fn apply_text_editor20250124(
        &mut self,
        tool_use: ToolUseBlock,
    ) -> Result<String, std::io::Error> {
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

    async fn bash(&mut self, command: &str, restart: bool) -> Result<String, std::io::Error> {
        let _ = command;
        let _ = restart;
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "bash is not supported",
        ))
    }

    async fn search(&mut self, search: &str) -> Result<String, std::io::Error> {
        let _ = search;
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "search is not supported",
        ))
    }

    async fn view(
        &mut self,
        path: &str,
        view_range: Option<(u32, u32)>,
    ) -> Result<String, std::io::Error> {
        let _ = path;
        let _ = view_range;
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "view is not supported",
        ))
    }

    async fn str_replace(
        &mut self,
        path: &str,
        old_str: &str,
        new_str: &str,
    ) -> Result<String, std::io::Error> {
        let _ = path;
        let _ = old_str;
        let _ = new_str;
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "str_replace is not supported",
        ))
    }

    async fn insert(
        &mut self,
        path: &str,
        insert_line: u32,
        new_str: &str,
    ) -> Result<String, std::io::Error> {
        let _ = path;
        let _ = insert_line;
        let _ = new_str;
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "insert is not supported",
        ))
    }
}

#[async_trait::async_trait]
impl Agent for () {}

#[async_trait::async_trait]
impl Agent for Path<'_> {
    async fn search(&mut self, search: &str) -> Result<String, std::io::Error> {
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
        &mut self,
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
        &mut self,
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
        &mut self,
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

//////////////////////////////////////////// ToolResult ////////////////////////////////////////////

type ToolResult = ControlFlow<Error, Result<ToolResultBlock, ToolResultBlock>>;
type ToolResultApplier<A> =
    Box<dyn for<'a> FnOnce(&'a mut A) -> Pin<Box<dyn Future<Output = ToolResult> + 'a>>>;

/////////////////////////////////////////////// Tool ///////////////////////////////////////////////

#[derive(Clone)]
pub enum Tool<A: Agent> {
    Custom {
        name: String,
        description: Option<String>,
        input_schema: serde_json::Value,
        #[allow(clippy::type_complexity)]
        callback: Arc<dyn Fn(ToolUseBlock) -> Pin<Box<dyn Future<Output = ToolResultApplier<A>>>>>,
    },
    SearchFileSystem,
    Bash20250124(ToolBash20250124),
    TextEditor20250124(ToolTextEditor20250124),
    WebSearch20250305(WebSearchTool20250305),
}

impl<A: Agent> Tool<A> {
    fn name(&self) -> &str {
        match self {
            Tool::Custom { name, .. } => name,
            Tool::SearchFileSystem => "search",
            Tool::Bash20250124(_) => "bash",
            Tool::TextEditor20250124(_) => "str_replace_editor",
            Tool::WebSearch20250305(_) => "web_search",
        }
    }
}

#[derive(serde::Deserialize)]
pub struct SearchTool {
    search: String,
}

impl JsonSchema for SearchTool {
    fn json_schema() -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "search": {
                    "type": "string",
                    "description": "The search term to look for"
                }
            },
            "required": ["search"]
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
            match agent.search(&search.search).await {
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

#[derive(serde::Deserialize)]
pub struct BashTool {
    command: String,
    restart: bool,
}

impl JsonSchema for BashTool {
    fn json_schema() -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The command to run."
                },
                "restart": {
                    "type": "boolean",
                    "description": "Set to true to restart the bash session."
                }
            },
            "required": ["search"]
        })
    }
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

pub fn text_editor_callback<A: Agent>(tool_use: ToolUseBlock) -> ToolResultApplier<A> {
    Box::new(|agent| {
        let id = tool_use.id.clone();
        Box::pin(async move {
            match agent.apply_text_editor20250124(tool_use).await {
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

pub struct AgentLoop<A: Agent> {
    pub client: Anthropic,
    pub agent: A,

    pub max_tokens: u32,
    pub model: Model,
    pub messages: Vec<MessageParam>,
    pub metadata: Option<Metadata>,
    pub stop_sequences: Option<Vec<String>>,
    pub system: Option<SystemPrompt>,
    pub temperature: Option<f32>,
    pub thinking: Option<ThinkingConfig>,
    pub tool_choice: Option<ToolChoice>,
    pub tools: Vec<Tool<A>>,
    pub top_k: Option<u32>,
    pub top_p: Option<f32>,
}

impl<A: Agent> AgentLoop<A> {
    pub async fn take_turn(&mut self) -> Result<(StopReason, MessageParamContent), Error> {
        let mut tokens_rem = self.max_tokens;
        let mut final_content = MessageParamContent::Array(vec![]);

        while tokens_rem > self.thinking.map(|t| t.num_tokens()).unwrap_or(0) {
            let req = self.create_request(tokens_rem);
            let resp = self.client.send(req).await?;
            let mut tool_results = vec![];
            eprintln!("{:#?}", resp.content);

            let assistant_message = MessageParam {
                role: MessageRole::Assistant,
                content: MessageParamContent::Array(resp.content.clone()),
            };
            push_or_merge_message(&mut self.messages, assistant_message);

            // Accumulate content for return value
            merge_message_content(
                &mut final_content,
                MessageParamContent::Array(resp.content.clone()),
            );

            tokens_rem = tokens_rem.saturating_sub(resp.usage.output_tokens as u32);
            match resp.stop_reason {
                None | Some(StopReason::EndTurn) => {
                    return Ok((StopReason::EndTurn, final_content));
                }
                Some(StopReason::MaxTokens) => {
                    return Ok((StopReason::MaxTokens, final_content));
                }
                Some(StopReason::StopSequence) => {
                    return Ok((StopReason::StopSequence, final_content));
                }
                Some(StopReason::Refusal) => {
                    return Ok((StopReason::Refusal, final_content));
                }
                Some(StopReason::PauseTurn) => {
                    continue;
                }
                Some(StopReason::ToolUse) => {
                    let mut futures = Vec::with_capacity(resp.content.len());
                    for block in resp.content.iter() {
                        if let ContentBlock::ToolUse(tool) = block {
                            futures.push(self.process_tool_use(tool));
                        }
                    }
                    let tool_result_appliers = futures::future::join_all(futures).await;
                    for tool_result_applier in tool_result_appliers {
                        let Some(tool_result_applier) = tool_result_applier else {
                            continue;
                        };
                        match tool_result_applier(&mut self.agent).await {
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
            eprintln!("{:#?}", tool_results);
            let user_message =
                MessageParam::new(MessageParamContent::Array(tool_results), MessageRole::User);
            push_or_merge_message(&mut self.messages, user_message);
        }
        Ok((StopReason::MaxTokens, final_content))
    }

    fn create_request(&self, max_tokens: u32) -> MessageCreateParams {
        let tools = self
            .tools
            .iter()
            .map(|tool| match tool {
                Tool::Custom {
                    name,
                    input_schema,
                    description,
                    callback: _,
                } => ToolUnionParam::CustomTool(ToolParam {
                    name: name.clone(),
                    input_schema: input_schema.clone(),
                    cache_control: None,
                    description: description.clone(),
                }),
                Tool::SearchFileSystem => ToolUnionParam::CustomTool(ToolParam {
                    name: "search".to_string(),
                    input_schema: SearchTool::json_schema(),
                    cache_control: None,
                    description: Some("Search the files available to the text editor tool for a given search string.".to_string()),
                }),
                Tool::Bash20250124(tool) => ToolUnionParam::Bash20250124(tool.clone()),
                Tool::TextEditor20250124(tool) => ToolUnionParam::TextEditor20250124(tool.clone()),
                Tool::WebSearch20250305(tool) => ToolUnionParam::WebSearch20250305(tool.clone()),
            })
            .collect::<Vec<_>>();
        MessageCreateParams {
            max_tokens,
            model: self.model.clone(),
            messages: self.messages.clone(),
            metadata: self.metadata.clone(),
            stop_sequences: self.stop_sequences.clone(),
            system: self.system.clone(),
            thinking: self.thinking,
            temperature: self.temperature,
            top_k: self.top_k,
            top_p: self.top_p,
            stream: false,
            tool_choice: self.tool_choice.clone(),
            tools: Some(tools),
        }
    }

    async fn process_tool_use(&self, tool_use: &ToolUseBlock) -> Option<ToolResultApplier<A>> {
        let Some(tool) = self.tools.iter().find(|t| t.name() == tool_use.name) else {
            let id = tool_use.id.clone();
            return Some(Box::new(|_| {
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
            }));
        };
        match tool {
            Tool::Custom {
                name: _,
                description: _,
                input_schema: _,
                callback,
            } => Some(callback(tool_use.clone()).await),
            Tool::SearchFileSystem => {
                let tool_use = tool_use.clone();
                Some(search_callback(tool_use))
            }
            Tool::Bash20250124(_) => {
                let tool_use = tool_use.clone();
                Some(bash_callback(tool_use))
            }
            Tool::TextEditor20250124(_) => {
                let tool_use = tool_use.clone();
                Some(text_editor_callback(tool_use))
            }
            Tool::WebSearch20250305(_) => None,
        }
    }
}

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
