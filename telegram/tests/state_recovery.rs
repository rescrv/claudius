//! Durability tests for `TransportState` + `FileStateStore`.

use claudius::{ContentBlock, MessageParam, MessageParamContent, MessageRole, TextBlock};
use claudius_telegram::{
    ChatId, FileStateStore, OutboxRecord, OutboxStatus, StateStore, TransportState, text_content,
};

fn temp_path(name: &str) -> std::path::PathBuf {
    let mut p = std::env::temp_dir();
    let unique = format!(
        "claudius-tg-{}-{}-{}.json",
        name,
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    p.push(unique);
    p
}

#[tokio::test]
async fn commit_then_reload_preserves_everything() {
    let path = temp_path("roundtrip");
    let store = FileStateStore::new(&path);

    let mut state = TransportState {
        next_offset: 12345,
        ..Default::default()
    };
    state.note_chat(ChatId(99));
    state.push_message(ChatId(99), MessageParam::user("hello"));
    state.push_message(
        ChatId(99),
        MessageParam::new(
            MessageParamContent::Array(vec![ContentBlock::Text(TextBlock::new("hi there"))]),
            MessageRole::Assistant,
        ),
    );
    state
        .outbox
        .push(OutboxRecord::pending(ChatId(99), text_content("queued")));
    state.mark_dead(ChatId(-42));

    store.commit(&state).await.unwrap();

    // Fresh store at the same path simulates a process restart.
    let reloaded = FileStateStore::new(&path).load().await.unwrap();
    assert_eq!(reloaded.next_offset, 12345);
    assert_eq!(reloaded.conversation(ChatId(99)).unwrap().len(), 2);
    assert!(reloaded.is_dead(ChatId(-42)));
    assert_eq!(reloaded.outbox.len(), 1);
    assert_eq!(reloaded.outbox[0].status, OutboxStatus::Pending);

    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn load_missing_file_yields_default() {
    let path = temp_path("missing");
    let state = FileStateStore::new(&path).load().await.unwrap();
    assert_eq!(state, TransportState::default());
}

#[tokio::test]
async fn partial_tmp_write_does_not_corrupt_committed_state() {
    let path = temp_path("atomic");
    let store = FileStateStore::new(&path);

    // Commit a known-good state.
    let good = TransportState {
        next_offset: 7,
        ..Default::default()
    };
    store.commit(&good).await.unwrap();

    // Simulate a crash mid-write: a stray, garbage .tmp sibling with no rename.
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, b"{ this is not valid json").unwrap();

    // Reload must observe the prior committed state, not the partial tmp.
    let reloaded = FileStateStore::new(&path).load().await.unwrap();
    assert_eq!(reloaded.next_offset, 7);

    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(&tmp);
}

#[tokio::test]
async fn sent_records_survive_with_message_id() {
    let path = temp_path("outbox");
    let store = FileStateStore::new(&path);

    let mut state = TransportState::default();
    let mut rec = OutboxRecord::pending(ChatId(5), text_content("payload"));
    rec.status = OutboxStatus::Sent { message_id: 808 };
    state.outbox.push(rec);
    store.commit(&state).await.unwrap();

    let reloaded = FileStateStore::new(&path).load().await.unwrap();
    assert_eq!(reloaded.outbox.len(), 1);
    assert_eq!(
        reloaded.outbox[0].status,
        OutboxStatus::Sent { message_id: 808 }
    );

    let _ = std::fs::remove_file(&path);
}
