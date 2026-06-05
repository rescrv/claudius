use std::env;
use std::time::Duration;

use claudius::{
    Anthropic, KnownModel, MessageBatch, MessageBatchCreateParams, MessageBatchCreateRequest,
    MessageBatchProcessingStatus, MessageBatchResult, MessageBatchResultVariant,
    MessageCreateParams, Result,
};
use futures::StreamExt;
use tokio::pin;
use tokio::time::sleep;

// Anthropic's batch-processing guide polls once per minute.
const POLL_INTERVAL: Duration = Duration::from_secs(60);

#[tokio::main]
async fn main() -> Result<()> {
    let client = Anthropic::new(None)?;

    let batch_id = match env::args().nth(1) {
        Some(batch_id) => {
            println!("Polling existing Message Batch: {batch_id}");
            batch_id
        }
        None => {
            let batch = create_demo_batch(&client).await?;
            println!("Created Message Batch: {}", batch.id);
            batch.id
        }
    };

    let batch = poll_until_ended(&client, &batch_id).await?;

    println!();
    println!("Batch ended:");
    print_batch_status(&batch);

    println!();
    println!("Streaming results:");
    let results = client.stream_message_batch_results(&batch.id).await?;
    pin!(results);

    while let Some(result) = results.next().await {
        print_result(&result?);
    }

    Ok(())
}

async fn create_demo_batch(client: &Anthropic) -> Result<MessageBatch> {
    let requests = vec![
        MessageBatchCreateRequest::new(
            "batch-example-summary",
            MessageCreateParams::simple(
                "Explain message batch processing in one sentence.",
                KnownModel::ClaudeHaiku45,
            ),
        ),
        MessageBatchCreateRequest::new(
            "batch-example-list",
            MessageCreateParams::simple(
                "List three practical use cases for asynchronous batch processing.",
                KnownModel::ClaudeHaiku45,
            ),
        ),
    ];

    client
        .create_message_batch(MessageBatchCreateParams::new(requests))
        .await
}

async fn poll_until_ended(client: &Anthropic, batch_id: &str) -> Result<MessageBatch> {
    loop {
        let batch = client.retrieve_message_batch(batch_id).await?;
        print_batch_status(&batch);

        if batch.processing_status == MessageBatchProcessingStatus::Ended {
            return Ok(batch);
        }

        sleep(POLL_INTERVAL).await;
    }
}

fn print_batch_status(batch: &MessageBatch) {
    println!(
        "{}: {:?} | processing={} succeeded={} errored={} canceled={} expired={}",
        batch.id,
        batch.processing_status,
        batch.request_counts.processing,
        batch.request_counts.succeeded,
        batch.request_counts.errored,
        batch.request_counts.canceled,
        batch.request_counts.expired
    );
}

fn print_result(result: &MessageBatchResult) {
    match &result.result {
        MessageBatchResultVariant::Succeeded { message } => {
            println!("{} succeeded with message {}", result.custom_id, message.id);
            for block in &message.content {
                if let Some(text) = block.as_text() {
                    println!("{}", text.text);
                } else {
                    println!("{block:?}");
                }
            }
        }
        MessageBatchResultVariant::Errored { error } => {
            println!(
                "{} errored: {}: {}",
                result.custom_id, error.error.r#type, error.error.message
            );
        }
        MessageBatchResultVariant::Canceled => {
            println!("{} was canceled", result.custom_id);
        }
        MessageBatchResultVariant::Expired => {
            println!("{} expired", result.custom_id);
        }
    }
}
