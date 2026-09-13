//! Public-API tests for the 4096-char chunker.

use claudius_telegram::{DEFAULT_CHUNK_LIMIT, chunk_message};

// Compile-time check that the default limit respects Telegram's hard cap.
const _: () = assert!(DEFAULT_CHUNK_LIMIT <= 4096);

#[test]
fn boundaries_3999_4000_4001() {
    assert_eq!(chunk_message(&"x".repeat(3999), 4000).len(), 1);
    assert_eq!(chunk_message(&"x".repeat(4000), 4000).len(), 1);
    assert_eq!(chunk_message(&"x".repeat(4001), 4000).len(), 2);
}

#[test]
fn eight_thousand_one_splits_into_three() {
    let chunks = chunk_message(&"x".repeat(8001), 4000);
    assert_eq!(chunks.len(), 3);
    assert_eq!(chunks.iter().map(|c| c.chars().count()).sum::<usize>(), 8001);
}

#[test]
fn emoji_input_never_splits_mid_codepoint_and_reassembles() {
    let s = "🎉🚀✨".repeat(2000); // 6000 chars, multi-byte
    let chunks = chunk_message(&s, DEFAULT_CHUNK_LIMIT);
    assert!(chunks.len() >= 2);
    assert_eq!(chunks.concat(), s);
    assert!(chunks.iter().all(|c| !c.is_empty()));
}

#[test]
fn never_emits_empty_chunk() {
    assert!(chunk_message("", DEFAULT_CHUNK_LIMIT).is_empty());
    let chunks = chunk_message(&"a".repeat(12000), 4000);
    assert!(chunks.iter().all(|c| !c.is_empty()));
}
