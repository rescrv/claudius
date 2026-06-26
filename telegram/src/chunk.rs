//! Splitting outbound text into Telegram-sized message chunks.
//!
//! Telegram rejects `sendMessage` text longer than 4096 characters. The exact
//! limit is counted in UTF-16 code units in some edge cases; v1 uses Rust
//! `char` count as a conservative proxy and chunks at [`DEFAULT_CHUNK_LIMIT`]
//! (4000) to leave headroom. Chunking never splits a multi-byte codepoint and
//! never emits an empty chunk.

/// The default chunk limit, in `char`s.
///
/// Set below Telegram's 4096 hard limit to leave headroom for the UTF-16
/// counting discrepancy described in the module docs.
pub const DEFAULT_CHUNK_LIMIT: usize = 4000;

/// Splits `text` into pieces of at most `max` `char`s each.
///
/// - Never splits in the middle of a Unicode scalar value (operates on `char`s).
/// - Never emits an empty chunk: empty input yields an empty `Vec`.
/// - A `max` of `0` is treated as `1` to guarantee progress.
///
/// The split is purely by character count; it does not attempt to break on word
/// or line boundaries.
pub fn chunk_message(text: &str, max: usize) -> Vec<String> {
    let max = max.max(1);
    if text.is_empty() {
        return Vec::new();
    }

    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut count = 0usize;
    for ch in text.chars() {
        current.push(ch);
        count += 1;
        if count == max {
            chunks.push(std::mem::take(&mut current));
            count = 0;
        }
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_yields_no_chunks() {
        assert!(chunk_message("", DEFAULT_CHUNK_LIMIT).is_empty());
    }

    #[test]
    fn short_input_is_one_chunk() {
        assert_eq!(chunk_message("hello", DEFAULT_CHUNK_LIMIT), vec!["hello"]);
    }

    #[test]
    fn exact_boundary_is_one_chunk() {
        let s = "a".repeat(4000);
        let chunks = chunk_message(&s, 4000);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].chars().count(), 4000);
    }

    #[test]
    fn just_over_boundary_splits_into_two() {
        let s = "a".repeat(4001);
        let chunks = chunk_message(&s, 4000);
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].chars().count(), 4000);
        assert_eq!(chunks[1].chars().count(), 1);
    }

    #[test]
    fn just_under_boundary_is_one_chunk() {
        let s = "a".repeat(3999);
        let chunks = chunk_message(&s, 4000);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].chars().count(), 3999);
    }

    #[test]
    fn large_input_chunks_evenly() {
        let s = "a".repeat(8001);
        let chunks = chunk_message(&s, 4000);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].chars().count(), 4000);
        assert_eq!(chunks[1].chars().count(), 4000);
        assert_eq!(chunks[2].chars().count(), 1);
    }

    #[test]
    fn multibyte_never_splits_mid_codepoint() {
        // Each emoji is multiple bytes but one char.
        let s = "😀".repeat(10);
        let chunks = chunk_message(&s, 4);
        assert_eq!(chunks.len(), 3);
        // Reassembling must reproduce the original exactly.
        assert_eq!(chunks.concat(), s);
        for c in &chunks {
            assert!(!c.is_empty());
        }
    }

    #[test]
    fn zero_max_treated_as_one() {
        let chunks = chunk_message("abc", 0);
        assert_eq!(chunks, vec!["a", "b", "c"]);
    }
}
