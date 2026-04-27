/// Demonstrates Anthropic prompt caching with cache write and cache read.
///
/// The first request populates the cache (cache_creation_input_tokens > 0).
/// The second request reads from the cache (cache_read_input_tokens > 0).
///
/// Requires ANTHROPIC_API_KEY or CLAUDIUS_API_KEY in the environment.
use claudius::{
    Anthropic, CacheControlEphemeral, ContentBlock, KnownModel, MessageCreateParams, Model, Result,
    SystemPrompt, TextBlock,
};

/// Generate a system prompt long enough to be eligible for caching.
///
/// Anthropic requires a minimum number of tokens before a cache breakpoint is effective.  This
/// function produces a prompt well above that threshold.
fn long_system_prompt() -> SystemPrompt {
    let mut paragraphs = Vec::new();
    for i in 0..120 {
        paragraphs.push(format!(
            "Section {i}: You are an expert assistant that provides detailed, accurate, and \
             well-structured answers to questions about mathematics, science, history, and \
             general knowledge. You always cite your reasoning step by step and ensure your \
             responses are comprehensive, well-organized, and easy to follow. You consider \
             multiple perspectives and present information in a balanced, nuanced way. You \
             use concrete examples and analogies to illustrate complex concepts, and you \
             provide actionable recommendations when appropriate. You are thorough but \
             concise, and you always verify your reasoning before presenting conclusions."
        ));
    }
    let text = paragraphs.join("\n\n");
    let block = TextBlock::new(text).with_cache_control(CacheControlEphemeral::new());
    SystemPrompt::from_blocks(vec![block])
}

#[tokio::main]
async fn main() -> Result<()> {
    let client = Anthropic::new(None)?;

    let system = long_system_prompt();

    // First request: cache write.
    let params = MessageCreateParams::new(
        64,
        vec!["What is 2+2?".into()],
        Model::Known(KnownModel::ClaudeHaiku45),
    )
    .with_system(system.clone());

    let first = client.send(params).await?;

    println!("First response (cache write):");
    for block in &first.content {
        if let ContentBlock::Text(t) = block {
            println!("  {}", t.text);
        }
    }
    println!(
        "  cache_creation_input_tokens: {:?}",
        first.usage.cache_creation_input_tokens
    );
    println!(
        "  cache_read_input_tokens:     {:?}",
        first.usage.cache_read_input_tokens
    );

    assert!(
        first
            .usage
            .cache_creation_input_tokens
            .is_some_and(|t| t > 0),
        "Expected cache_creation_input_tokens > 0 on first request, got {:?}",
        first.usage.cache_creation_input_tokens,
    );

    // Second request: cache read. Same system prompt, different user message.
    let params = MessageCreateParams::new(
        64,
        vec!["What is 3+3?".into()],
        Model::Known(KnownModel::ClaudeHaiku45),
    )
    .with_system(system);

    let second = client.send(params).await?;

    println!("\nSecond response (cache read):");
    for block in &second.content {
        if let ContentBlock::Text(t) = block {
            println!("  {}", t.text);
        }
    }
    println!(
        "  cache_creation_input_tokens: {:?}",
        second.usage.cache_creation_input_tokens
    );
    println!(
        "  cache_read_input_tokens:     {:?}",
        second.usage.cache_read_input_tokens
    );

    assert!(
        second.usage.cache_read_input_tokens.is_some_and(|t| t > 0),
        "Expected cache_read_input_tokens > 0 on second request, got {:?}",
        second.usage.cache_read_input_tokens,
    );

    println!("\nCache write and read verified successfully.");
    Ok(())
}
