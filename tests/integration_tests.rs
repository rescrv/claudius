//! Integration tests for the Claudius library.
//! These tests require an API key in the environment to run.

#[cfg(test)]
mod tests {
    use claudius::{Anthropic, KnownModel, MessageCreateParams, MessageParam, MessageRole, Model};

    #[tokio::test]
    async fn test_simple_message_request() {
        // This test requires ANTHROPIC_API_KEY to be set
        let api_key = std::env::var("ANTHROPIC_API_KEY").ok();
        if api_key.is_none() {
            eprintln!("Skipping test: ANTHROPIC_API_KEY not set");
            return;
        }

        let client = Anthropic::new(api_key).expect("Failed to create client");

        let params = MessageCreateParams::new(
            10,
            vec![MessageParam::new_with_string(
                "Say 'test passed'".to_string(),
                MessageRole::User,
            )],
            Model::Known(KnownModel::Claude35HaikuLatest),
        );

        let response = client.send(params).await;
        assert!(
            response.is_ok(),
            "Request should succeed with valid API key"
        );
    }

    #[tokio::test]
    async fn test_streaming_response() {
        let api_key = std::env::var("ANTHROPIC_API_KEY").ok();
        if api_key.is_none() {
            eprintln!("Skipping test: ANTHROPIC_API_KEY not set");
            return;
        }

        let client = Anthropic::new(api_key).expect("Failed to create client");

        let params = MessageCreateParams::new(
            10,
            vec![MessageParam::new_with_string(
                "Count to 3".to_string(),
                MessageRole::User,
            )],
            Model::Known(KnownModel::Claude35HaikuLatest),
        );

        let stream = client.stream(params).await;
        assert!(stream.is_ok(), "Stream request should succeed");
    }
}
