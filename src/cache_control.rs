//! Cache control for Anthropic prompt caching.
//!
//! This module provides [`apply_cache_controls`], which sets a cache_control breakpoint on the
//! system prompt.  Any existing markers on the system prompt are cleared first so that placement is
//! entirely determined by this function.

use crate::types::{CacheControlEphemeral, SystemPrompt, TextBlock};

/// Apply a cache_control breakpoint to the system prompt.
///
/// Clears any existing cache_control markers on the system prompt, then sets a breakpoint on its
/// last block.  When the system prompt is a plain string it is converted to a single-element blocks
/// variant so the marker can be attached.
pub fn apply_cache_controls(system: &mut Option<SystemPrompt>) {
    clear_system_cache_controls(system);
    apply_cache_control_to_system(system);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn system_has_cache_control(system: &Option<SystemPrompt>) -> bool {
        match system {
            Some(SystemPrompt::Blocks(blocks)) => blocks
                .last()
                .is_some_and(|b| b.block.cache_control.is_some()),
            _ => false,
        }
    }

    #[test]
    fn no_system() {
        let mut system = None;
        apply_cache_controls(&mut system);
        assert!(!system_has_cache_control(&system));
    }

    #[test]
    fn system_prompt_gets_breakpoint() {
        let mut system = Some(SystemPrompt::from_string("You are helpful.".to_string()));
        apply_cache_controls(&mut system);
        assert!(system_has_cache_control(&system));
    }

    #[test]
    fn system_blocks_get_breakpoint_on_last() {
        let blocks = vec![TextBlock::new("first"), TextBlock::new("second")];
        let mut system = Some(SystemPrompt::from_blocks(blocks));
        apply_cache_controls(&mut system);
        if let Some(SystemPrompt::Blocks(blocks)) = &system {
            assert!(blocks[0].block.cache_control.is_none());
            assert!(blocks[1].block.cache_control.is_some());
        } else {
            panic!("Expected Blocks variant");
        }
    }

    #[test]
    fn preexisting_system_blocks_cleared_before_reapply() {
        let blocks = vec![
            TextBlock::new("a").with_cache_control(CacheControlEphemeral::new()),
            TextBlock::new("b").with_cache_control(CacheControlEphemeral::new()),
        ];
        let mut system = Some(SystemPrompt::from_blocks(blocks));
        apply_cache_controls(&mut system);
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
