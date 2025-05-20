use serde::{Deserialize, Serialize};

use crate::types::MessageCreateParams;

/// Parameters for creating a batch of messages.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BatchCreateParams {
    /// List of requests for prompt completion.
    ///
    /// Each is an individual request to create a Message.
    pub requests: Vec<Request>,
}

impl BatchCreateParams {
    /// Create a new `BatchCreateParams` with the given requests.
    pub fn new(requests: Vec<Request>) -> Self {
        Self { requests }
    }
}

/// A single request within a message batch.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Request {
    /// Developer-provided ID created for each request in a Message Batch.
    ///
    /// Useful for matching results to requests, as results may be given out of request
    /// order.
    ///
    /// Must be unique for each request within the Message Batch.
    pub custom_id: String,

    /// Messages API creation parameters for the individual request.
    ///
    /// See the [Messages API reference](https://docs.anthropic.com/claude/reference/messages_post) for full documentation on
    /// available parameters.
    pub params: MessageCreateParams,
}

impl Request {
    /// Create a new `Request` with the given parameters.
    pub fn new(custom_id: String, params: MessageCreateParams) -> Self {
        // Ensure streaming is disabled for batch requests
        let mut params = params;
        params.stream = false;
        Self { custom_id, params }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{KnownModel, MessageParam, MessageRole, Model};
    use serde_json::{json, to_value};

    #[test]
    fn test_batch_create_params_serialization() {
        // For this test, we'll just create a request directly
        let message = MessageParam::new_with_string("Hello, world".to_string(), MessageRole::User);
        let params = MessageCreateParams::new(
            1000,
            vec![message],
            Model::Known(KnownModel::Claude37Sonnet20250219),
        );

        let request = Request::new("request-123".to_string(), params);
        let batch_params = BatchCreateParams::new(vec![request]);

        let json = to_value(&batch_params).unwrap();

        // Assert the structure matches what we expect
        assert!(json["requests"].is_array());
        assert_eq!(json["requests"].as_array().unwrap().len(), 1);
        assert_eq!(json["requests"][0]["custom_id"], "request-123");
        assert_eq!(json["requests"][0]["params"]["stream"], false);
    }

    #[test]
    fn test_batch_create_params_deserialization() {
        let json = json!({
            "requests": [
                {
                    "custom_id": "request-123",
                    "params": {
                        "model": "claude-3-sonnet-20240229",
                        "messages": [
                            {
                                "role": "user",
                                "content": [
                                    {
                                        "type": "text",
                                        "text": "Hello, Claude!"
                                    }
                                ]
                            }
                        ],
                        "max_tokens": 1000,
                        "stream": false
                    }
                }
            ]
        });

        let batch_params: BatchCreateParams = serde_json::from_value(json).unwrap();

        assert_eq!(batch_params.requests.len(), 1);
        assert_eq!(batch_params.requests[0].custom_id, "request-123");
        assert!(!batch_params.requests[0].params.stream);
    }
}
