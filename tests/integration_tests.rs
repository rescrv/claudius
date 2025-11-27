//! Comprehensive integration and unit tests for the Claudius library.
//!
//! API tests require an API key in the environment to run.
//! Unit tests run without external dependencies.

#[cfg(test)]
mod tests {
    use claudius::{
        Anthropic, KnownModel, MessageCreateParams, MessageParam, MessageRole, Model,
        ThinkingConfig,
    };
    use std::time::Duration;

    #[tokio::test]
    async fn simple_message_request() {
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
    async fn streaming_response() {
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

    #[tokio::test]
    async fn parameter_validation() {
        // Test validation without making API calls
        let mut params = MessageCreateParams {
            max_tokens: 0,
            ..Default::default()
        };
        assert!(params.validate().is_err(), "Should reject max_tokens = 0");

        // Test empty messages validation
        params.max_tokens = 100;
        params.messages.clear();
        assert!(params.validate().is_err(), "Should reject empty messages");

        // Test valid parameters
        params.messages.push(MessageParam::user("test"));
        assert!(params.validate().is_ok(), "Should accept valid parameters");

        // Test temperature validation
        let temp_result = params.clone().with_temperature(2.0);
        assert!(temp_result.is_err(), "Should reject temperature > 1.0");
        params = params.with_temperature(0.5).unwrap(); // Should succeed
        assert!(params.validate().is_ok());
    }

    #[tokio::test]
    async fn thinking_config_validation() {
        let mut params = MessageCreateParams::simple("test", KnownModel::ClaudeHaiku45);

        // Test thinking config with insufficient budget
        params = params.with_thinking(ThinkingConfig::Enabled { budget_tokens: 500 });
        assert!(params.validate().is_err(), "Should reject budget < 1024");

        // Test thinking config exceeding max_tokens
        params.max_tokens = 1000;
        params = params.with_thinking(ThinkingConfig::Enabled {
            budget_tokens: 1500,
        });
        assert!(
            params.validate().is_err(),
            "Should reject budget > max_tokens"
        );

        // Test valid thinking config
        params.max_tokens = 2000;
        params = params.with_thinking(ThinkingConfig::Enabled {
            budget_tokens: 1024,
        });
        assert!(
            params.validate().is_ok(),
            "Should accept valid thinking config"
        );
    }

    #[tokio::test]
    async fn client_configuration() {
        // Test client creation with various configurations
        let _client = Anthropic::new(Some("test_key".to_string()))
            .expect("Should create client")
            .with_max_retries(5)
            .with_backoff_params(1.0, 0.5)
            .with_base_url("https://test.example.com/v1/".to_string());

        // Note: Fields are private, so we can only test that construction succeeds
        // In a real application, we'd add getter methods if needed
    }

    #[tokio::test]
    async fn timeout_configuration() {
        let client = Anthropic::new(Some("test_key".to_string())).expect("Should create client");

        let _timeout_client = client
            .with_timeout(Duration::from_secs(30))
            .expect("Should set timeout");

        // Note: timeout field is private, but we can verify construction succeeded
    }

    #[test]
    fn message_param_builders() {
        // Test ergonomic constructors
        let user_msg = MessageParam::user("Hello");
        assert_eq!(user_msg.role, MessageRole::User);

        let assistant_msg = MessageParam::assistant("Hi there");
        assert_eq!(assistant_msg.role, MessageRole::Assistant);

        // Test string conversion
        let from_str: MessageParam = "Test message".into();
        assert_eq!(from_str.role, MessageRole::User);
    }

    #[test]
    fn model_display() {
        let known_model = Model::Known(KnownModel::ClaudeHaiku45);
        let custom_model = Model::Custom("custom-model-name".to_string());

        assert!(!known_model.to_string().is_empty());
        assert_eq!(custom_model.to_string(), "custom-model-name");
    }

    #[test]
    fn builder_pattern_completeness() {
        // Test that all builder methods work together
        let params = MessageCreateParams::simple("test", KnownModel::ClaudeHaiku45)
            .with_temperature(0.7)
            .unwrap()
            .with_top_p(0.9)
            .unwrap()
            .with_top_k(50)
            .with_stop_sequences(vec!["STOP".to_string()])
            .with_system_string("You are helpful".to_string())
            .with_stream(true);

        assert_eq!(params.temperature, Some(0.7));
        assert_eq!(params.top_p, Some(0.9));
        assert_eq!(params.top_k, Some(50));
        assert!(params.stop_sequences.is_some());
        assert!(params.system.is_some());
        assert!(params.stream);
    }

    #[tokio::test]
    async fn streaming_endpoint_with_env_api_key() {
        // Test streaming endpoint using Anthropic::new(None) to read from environment
        assert!(
            std::env::var("ANTHROPIC_API_KEY").is_ok() || std::env::var("CLAUDIUS_API_KEY").is_ok(),
            "Either ANTHROPIC_API_KEY or CLAUDIUS_API_KEY must be set for this test"
        );

        let client = Anthropic::new(None).expect("Failed to create client with env API key");

        let params = MessageCreateParams::new(
            20,
            vec![MessageParam::new_with_string(
                "Say 'streaming test passed' briefly".to_string(),
                MessageRole::User,
            )],
            Model::Known(KnownModel::Claude35HaikuLatest),
        );

        let result = client.stream(params).await;
        assert!(
            result.is_ok(),
            "Streaming request should succeed with env API key"
        );

        // Verify we get a proper stream by consuming at least one event
        if let Ok(stream) = result {
            use futures::StreamExt;
            let mut pinned_stream = std::pin::pin!(stream);
            let first_event = pinned_stream.next().await;
            assert!(
                first_event.is_some(),
                "Stream should yield at least one event"
            );
            if let Some(event_result) = first_event {
                assert!(event_result.is_ok(), "First stream event should be valid");
            }
        }
    }

    #[tokio::test]
    async fn non_streaming_endpoint_with_env_api_key() {
        // Test non-streaming endpoint using Anthropic::new(None) to read from environment
        assert!(
            std::env::var("ANTHROPIC_API_KEY").is_ok() || std::env::var("CLAUDIUS_API_KEY").is_ok(),
            "Either ANTHROPIC_API_KEY or CLAUDIUS_API_KEY must be set for this test"
        );

        let client = Anthropic::new(None).expect("Failed to create client with env API key");

        let params = MessageCreateParams::new(
            20,
            vec![MessageParam::new_with_string(
                "Say 'non-streaming test passed' briefly".to_string(),
                MessageRole::User,
            )],
            Model::Known(KnownModel::Claude35HaikuLatest),
        );

        let response = client.send(params).await;
        assert!(
            response.is_ok(),
            "Non-streaming request should succeed with env API key"
        );

        if let Ok(message) = response {
            assert!(
                !message.content.is_empty(),
                "Response should contain content"
            );
            assert!(
                message.usage.input_tokens > 0,
                "Should report input token usage"
            );
            assert!(
                message.usage.output_tokens > 0,
                "Should report output token usage"
            );
        }
    }
}
