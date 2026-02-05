use claudius::{
    Anthropic, ContentBlock, KnownModel, MessageCreateParams, MessageParam, MessageRole, Model,
    TextBlock, ThinkingConfig, push_or_merge_message,
};

#[tokio::main]
async fn main() {
    let files = std::env::args().skip(1).collect::<Vec<_>>();
    let mut contents = Vec::with_capacity(files.len());
    for file in files {
        let content = std::fs::read_to_string(&file)
            .inspect_err(|_| eprintln!("could not read {file}"))
            .expect("should be able to read file");
        contents.push(content);
    }
    let mut messages = vec![];
    for (id, content) in contents.into_iter().enumerate() {
        push_or_merge_message(
            &mut messages,
            MessageParam {
                role: MessageRole::User,
                content: format!(r#"<document id="{id}">{content}</document>"#).into(),
            },
        );
    }
    let create = MessageCreateParams {
        max_tokens: 12_500,
        messages,
        model: Model::Known(KnownModel::ClaudeOpus40),
        stream: false,
        system: Some(r#"You are tasked with providing the best transcription for a document from multiple transcriptions.

From the set of documents provided, select the document that makes the most sense given the content.

Output the corrected/unified document and only the corrected/unified document.
"#.into()),
        metadata: None,
        output_format: None,
        output_config: None,
        stop_sequences: None,
        thinking: Some(ThinkingConfig::enabled(1024)),
        tools: None,
        temperature: None,
        tool_choice: None,
        top_k: None,
        top_p: None,
    };
    let client = Anthropic::new(None).expect("could not create anthropic client");
    let resp = client.send(create).await.expect("claude failed");
    let content = resp
        .content
        .into_iter()
        .filter_map(|c| {
            if let ContentBlock::Text(TextBlock { text, .. }) = c {
                Some(text)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    println!("{}", content.join("\n"));
}
