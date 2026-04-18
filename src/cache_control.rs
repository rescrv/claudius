//! Cache control for Anthropic prompt caching.
//!
//! The prefix up to and including each cache_control breakpoint must hash identically across
//! requests.  Breakpoints that land on varying content (timestamps, the latest user message, new
//! tool results) produce a different prefix hash on every call, miss the cache, and pay for a fresh
//! write each time.
//!
//! This module provides a single entry point — [`apply_cache_controls`] — that clears any existing
//! markers, sets one breakpoint on the system prompt, and then places additional breakpoints every
//! [`CACHE_BREAKPOINT_INTERVAL`] content blocks throughout the messages.  When the total exceeds
//! [`MAX_MESSAGE_BREAKPOINTS`], the earliest message breakpoints are dropped to make room.  Because
//! the placement is deterministic and based only on block position, every request with the same
//! prefix produces identical breakpoint placement and therefore identical prefix hashes.

use crate::types::{
    CacheControlEphemeral, ContentBlock, MessageParam, MessageParamContent, SystemPrompt, TextBlock,
};

/// Maximum number of cache control breakpoints allowed by the API.
const MAX_CACHE_BREAKPOINTS: usize = 4;

/// One breakpoint is always reserved for the system prompt.
const MAX_MESSAGE_BREAKPOINTS: usize = MAX_CACHE_BREAKPOINTS - 1;

/// Interval in content blocks between cache breakpoints within messages.
const CACHE_BREAKPOINT_INTERVAL: usize = 20;

/// Apply cache_control markers to a system prompt and messages for a single API request.
///
/// All existing cache_control markers are cleared first so that placement is entirely determined by
/// this function.  One breakpoint is always reserved for the system prompt.  The remaining budget
/// ([`MAX_MESSAGE_BREAKPOINTS`]) is filled by placing a breakpoint every
/// [`CACHE_BREAKPOINT_INTERVAL`] content blocks throughout `messages`.  When there are more
/// candidates than the budget allows, the earliest message breakpoints are removed first, keeping
/// the later (longer-prefix) breakpoints that maximize cache value.
pub fn apply_cache_controls(system: &mut Option<SystemPrompt>, messages: &mut [MessageParam]) {
    // Clear every existing marker so placement is fully deterministic.
    clear_system_cache_controls(system);
    for msg in messages.iter_mut() {
        clear_cache_control_from_message(msg);
    }

    // The system prompt always gets a breakpoint.
    apply_cache_control_to_system(system);

    // Walk every content block and note the position of every CACHE_BREAKPOINT_INTERVAL-th block.
    let mut block_counter: usize = 0;
    let mut candidates: Vec<(usize, usize)> = Vec::new();
    for (msg_idx, message) in messages.iter().enumerate() {
        let num_blocks = match &message.content {
            MessageParamContent::Array(blocks) => blocks.len(),
            MessageParamContent::String(_) => 1,
        };
        for block_idx in 0..num_blocks {
            block_counter += 1;
            if block_counter.is_multiple_of(CACHE_BREAKPOINT_INTERVAL) {
                candidates.push((msg_idx, block_idx));
            }
        }
    }

    // Drop the earliest candidates to stay within budget.
    while candidates.len() > MAX_MESSAGE_BREAKPOINTS {
        candidates.remove(0);
    }

    for (msg_idx, block_idx) in candidates {
        apply_cache_control_at(messages, msg_idx, block_idx);
    }
}

/// Set cache_control on the last block of the system prompt, converting from String to Blocks when
/// necessary.
fn apply_cache_control_to_system(system: &mut Option<SystemPrompt>) {
    match system {
        Some(SystemPrompt::String(text)) => {
            let block =
                TextBlock::new(text.clone()).with_cache_control(CacheControlEphemeral::new());
            *system = Some(SystemPrompt::from_blocks(vec![block]));
        }
        Some(SystemPrompt::Blocks(blocks)) => {
            if let Some(last) = blocks.last_mut() {
                last.block.cache_control = Some(CacheControlEphemeral::new());
            }
        }
        None => {}
    }
}

/// Clear all cache_control markers from the system prompt.
fn clear_system_cache_controls(system: &mut Option<SystemPrompt>) {
    if let Some(SystemPrompt::Blocks(blocks)) = system {
        for block in blocks.iter_mut() {
            block.block.cache_control = None;
        }
    }
}

/// Set cache_control on a specific block within a message.
fn apply_cache_control_at(messages: &mut [MessageParam], msg_idx: usize, block_idx: usize) {
    let message = &mut messages[msg_idx];
    match &mut message.content {
        MessageParamContent::String(text) => {
            let block = ContentBlock::Text(
                TextBlock::new(text.clone()).with_cache_control(CacheControlEphemeral::new()),
            );
            message.content = MessageParamContent::Array(vec![block]);
        }
        MessageParamContent::Array(blocks) => {
            if let Some(block) = blocks.get_mut(block_idx) {
                set_cache_control_on_block(block);
            }
        }
    }
}

/// Clear cache_control from all content blocks in a message.
fn clear_cache_control_from_message(message: &mut MessageParam) {
    if let MessageParamContent::Array(blocks) = &mut message.content {
        for block in blocks.iter_mut() {
            clear_cache_control_on_block(block);
        }
    }
}

/// Clear cache_control on a single content block.
fn clear_cache_control_on_block(block: &mut ContentBlock) {
    match block {
        ContentBlock::Text(text_block) => {
            text_block.cache_control = None;
        }
        ContentBlock::ToolResult(tool_result) => {
            tool_result.cache_control = None;
        }
        ContentBlock::ToolUse(tool_use) => {
            tool_use.cache_control = None;
        }
        ContentBlock::Image(image_block) => {
            image_block.cache_control = None;
        }
        ContentBlock::Document(document_block) => {
            document_block.cache_control = None;
        }
        ContentBlock::ServerToolUse(server_tool_use) => {
            server_tool_use.cache_control = None;
        }
        ContentBlock::WebSearchToolResult(web_search_result) => {
            web_search_result.cache_control = None;
        }
        ContentBlock::Thinking(_) | ContentBlock::RedactedThinking(_) => {}
    }
}

/// Set cache_control on a content block that supports caching.
fn set_cache_control_on_block(block: &mut ContentBlock) {
    match block {
        ContentBlock::Text(text_block) => {
            text_block.cache_control = Some(CacheControlEphemeral::new());
        }
        ContentBlock::ToolResult(tool_result) => {
            tool_result.cache_control = Some(CacheControlEphemeral::new());
        }
        ContentBlock::ToolUse(tool_use) => {
            tool_use.cache_control = Some(CacheControlEphemeral::new());
        }
        ContentBlock::Image(_)
        | ContentBlock::Document(_)
        | ContentBlock::ServerToolUse(_)
        | ContentBlock::WebSearchToolResult(_)
        | ContentBlock::Thinking(_)
        | ContentBlock::RedactedThinking(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::MessageRole;

    fn text_block(text: &str) -> ContentBlock {
        ContentBlock::Text(TextBlock::new(text))
    }

    fn user_msg_with_blocks(n: usize, prefix: &str) -> MessageParam {
        let blocks: Vec<ContentBlock> = (0..n)
            .map(|i| text_block(&format!("{prefix}{i}")))
            .collect();
        MessageParam {
            role: MessageRole::User,
            content: MessageParamContent::Array(blocks),
        }
    }

    fn assistant_msg(text: &str) -> MessageParam {
        MessageParam {
            role: MessageRole::Assistant,
            content: MessageParamContent::String(text.to_string()),
        }
    }

    fn count_message_cache_controls(messages: &[MessageParam]) -> usize {
        let mut count = 0;
        for msg in messages {
            if let MessageParamContent::Array(blocks) = &msg.content {
                for block in blocks {
                    if let ContentBlock::Text(t) = block
                        && t.cache_control.is_some()
                    {
                        count += 1;
                    }
                }
            }
        }
        count
    }

    fn system_has_cache_control(system: &Option<SystemPrompt>) -> bool {
        match system {
            Some(SystemPrompt::Blocks(blocks)) => blocks
                .last()
                .is_some_and(|b| b.block.cache_control.is_some()),
            _ => false,
        }
    }

    #[test]
    fn no_system_no_messages() {
        let mut system = None;
        let mut messages: Vec<MessageParam> = vec![];
        apply_cache_controls(&mut system, &mut messages);
        assert!(!system_has_cache_control(&system));
        assert_eq!(count_message_cache_controls(&messages), 0);
    }

    #[test]
    fn system_prompt_gets_breakpoint() {
        let mut system = Some(SystemPrompt::from_string("You are helpful.".to_string()));
        let mut messages: Vec<MessageParam> = vec![];
        apply_cache_controls(&mut system, &mut messages);
        assert!(system_has_cache_control(&system));
    }

    #[test]
    fn system_blocks_get_breakpoint_on_last() {
        let blocks = vec![TextBlock::new("first"), TextBlock::new("second")];
        let mut system = Some(SystemPrompt::from_blocks(blocks));
        let mut messages: Vec<MessageParam> = vec![];
        apply_cache_controls(&mut system, &mut messages);
        if let Some(SystemPrompt::Blocks(blocks)) = &system {
            assert!(blocks[0].block.cache_control.is_none());
            assert!(blocks[1].block.cache_control.is_some());
        } else {
            panic!("Expected Blocks variant");
        }
    }

    #[test]
    fn fewer_than_20_blocks_gets_no_message_breakpoints() {
        let mut system = Some(SystemPrompt::from_string("sys".to_string()));
        let mut messages = vec![
            user_msg_with_blocks(5, "u"),
            assistant_msg("a"),
            user_msg_with_blocks(5, "v"),
        ];
        apply_cache_controls(&mut system, &mut messages);
        assert!(system_has_cache_control(&system));
        assert_eq!(count_message_cache_controls(&messages), 0);
    }

    #[test]
    fn exactly_20_blocks_gets_one_message_breakpoint() {
        let mut system = Some(SystemPrompt::from_string("sys".to_string()));
        let mut messages = vec![user_msg_with_blocks(20, "u")];
        apply_cache_controls(&mut system, &mut messages);
        assert!(system_has_cache_control(&system));
        assert_eq!(count_message_cache_controls(&messages), 1);
        if let MessageParamContent::Array(blocks) = &messages[0].content {
            for (i, block) in blocks.iter().enumerate() {
                if let ContentBlock::Text(t) = block {
                    if i == 19 {
                        assert!(
                            t.cache_control.is_some(),
                            "block 19 should have cache_control"
                        );
                    } else {
                        assert!(
                            t.cache_control.is_none(),
                            "block {i} should not have cache_control"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn breakpoints_every_20_blocks() {
        let mut system = Some(SystemPrompt::from_string("sys".to_string()));
        let mut messages = vec![user_msg_with_blocks(45, "u")];
        apply_cache_controls(&mut system, &mut messages);
        assert_eq!(count_message_cache_controls(&messages), 2);
        if let MessageParamContent::Array(blocks) = &messages[0].content {
            for (i, block) in blocks.iter().enumerate() {
                if let ContentBlock::Text(t) = block {
                    if i == 19 || i == 39 {
                        assert!(
                            t.cache_control.is_some(),
                            "block {i} should have cache_control"
                        );
                    } else {
                        assert!(
                            t.cache_control.is_none(),
                            "block {i} should not have cache_control"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn exceeding_max_breakpoints_drops_earliest_message_breakpoints() {
        let mut system = Some(SystemPrompt::from_string("sys".to_string()));
        // 80 blocks → candidates at 20, 40, 60, 80. Budget is 3.
        // Drops the candidate at 20; keeps 40, 60, 80.
        let mut messages = vec![user_msg_with_blocks(80, "u")];
        apply_cache_controls(&mut system, &mut messages);
        assert!(system_has_cache_control(&system));
        assert_eq!(count_message_cache_controls(&messages), 3);
        if let MessageParamContent::Array(blocks) = &messages[0].content {
            for (i, block) in blocks.iter().enumerate() {
                if let ContentBlock::Text(t) = block {
                    if i == 39 || i == 59 || i == 79 {
                        assert!(
                            t.cache_control.is_some(),
                            "block {i} should have cache_control"
                        );
                    } else {
                        assert!(
                            t.cache_control.is_none(),
                            "block {i} should not have cache_control"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn clears_preexisting_markers() {
        let mut system = Some(SystemPrompt::from_blocks(vec![
            TextBlock::new("sys").with_cache_control(CacheControlEphemeral::new()),
        ]));
        let mut messages = vec![MessageParam {
            role: MessageRole::User,
            content: MessageParamContent::Array(vec![ContentBlock::Text(
                TextBlock::new("stale marker").with_cache_control(CacheControlEphemeral::new()),
            )]),
        }];
        apply_cache_controls(&mut system, &mut messages);
        assert_eq!(count_message_cache_controls(&messages), 0);
        assert!(system_has_cache_control(&system));
    }

    #[test]
    fn breakpoints_span_multiple_messages() {
        let mut system = Some(SystemPrompt::from_string("sys".to_string()));
        // 10 blocks in msg0, 1 assistant string, 15 blocks in msg2 → total 26 blocks.
        // Breakpoint at counter 20: msg0 uses 10, msg1 uses 1 (counter=11), msg2 block 8 = counter 20.
        let mut messages = vec![
            user_msg_with_blocks(10, "a"),
            assistant_msg("middle"),
            user_msg_with_blocks(15, "b"),
        ];
        apply_cache_controls(&mut system, &mut messages);
        assert_eq!(count_message_cache_controls(&messages), 1);
        if let MessageParamContent::Array(blocks) = &messages[2].content {
            for (i, block) in blocks.iter().enumerate() {
                if let ContentBlock::Text(t) = block {
                    if i == 8 {
                        assert!(
                            t.cache_control.is_some(),
                            "block {i} of msg2 should have cache_control"
                        );
                    } else {
                        assert!(t.cache_control.is_none(), "block {i} of msg2 should not");
                    }
                }
            }
        }
    }

    #[test]
    fn string_content_at_breakpoint_converts_to_array() {
        let mut system = None;
        let mut messages = vec![
            user_msg_with_blocks(19, "u"),
            MessageParam {
                role: MessageRole::User,
                content: MessageParamContent::String("twentieth".to_string()),
            },
        ];
        apply_cache_controls(&mut system, &mut messages);
        match &messages[1].content {
            MessageParamContent::Array(blocks) => {
                assert_eq!(blocks.len(), 1);
                if let ContentBlock::Text(t) = &blocks[0] {
                    assert_eq!(t.text, "twentieth");
                    assert!(t.cache_control.is_some());
                } else {
                    panic!("Expected Text block");
                }
            }
            MessageParamContent::String(_) => {
                panic!("Expected conversion to Array");
            }
        }
    }

    #[test]
    fn no_system_gives_full_budget_to_messages() {
        let mut system = None;
        // No system prompt → all MAX_MESSAGE_BREAKPOINTS available.
        // 60 blocks → candidates at 20, 40, 60.  Budget = 3.  All fit.
        let mut messages = vec![user_msg_with_blocks(60, "u")];
        apply_cache_controls(&mut system, &mut messages);
        assert_eq!(count_message_cache_controls(&messages), 3);
    }

    #[test]
    fn stability_across_identical_calls() {
        let mut system1 = Some(SystemPrompt::from_string("sys".to_string()));
        let mut messages1 = vec![
            user_msg_with_blocks(15, "u"),
            assistant_msg("a"),
            user_msg_with_blocks(15, "v"),
        ];
        apply_cache_controls(&mut system1, &mut messages1);

        let mut system2 = Some(SystemPrompt::from_string("sys".to_string()));
        let mut messages2 = vec![
            user_msg_with_blocks(15, "u"),
            assistant_msg("a"),
            user_msg_with_blocks(15, "v"),
        ];
        apply_cache_controls(&mut system2, &mut messages2);

        assert_eq!(system1, system2);
        assert_eq!(messages1, messages2);
    }

    #[test]
    fn stability_when_messages_grow() {
        let mut system = Some(SystemPrompt::from_string("sys".to_string()));
        let mut messages = vec![user_msg_with_blocks(25, "u")];
        apply_cache_controls(&mut system, &mut messages);
        println!(
            "first_placement: {}",
            count_message_cache_controls(&messages)
        );
        assert_eq!(count_message_cache_controls(&messages), 1);

        // Append more blocks (total 31); breakpoint at 20 stays put.
        messages.push(assistant_msg("a"));
        messages.push(user_msg_with_blocks(5, "v"));
        apply_cache_controls(&mut system, &mut messages);
        println!(
            "second_placement: {}",
            count_message_cache_controls(&messages)
        );
        assert_eq!(count_message_cache_controls(&messages), 1);

        if let MessageParamContent::Array(blocks) = &messages[0].content
            && let ContentBlock::Text(t) = &blocks[19]
        {
            assert!(
                t.cache_control.is_some(),
                "block 19 should still have cache_control after growth"
            );
        }
    }

    #[test]
    fn preexisting_system_blocks_cleared_before_reapply() {
        let blocks = vec![
            TextBlock::new("a").with_cache_control(CacheControlEphemeral::new()),
            TextBlock::new("b").with_cache_control(CacheControlEphemeral::new()),
        ];
        let mut system = Some(SystemPrompt::from_blocks(blocks));
        let mut messages: Vec<MessageParam> = vec![];
        apply_cache_controls(&mut system, &mut messages);
        if let Some(SystemPrompt::Blocks(blocks)) = &system {
            assert!(
                blocks[0].block.cache_control.is_none(),
                "first block should be cleared"
            );
            assert!(
                blocks[1].block.cache_control.is_some(),
                "last block gets the breakpoint"
            );
        }
    }
}
