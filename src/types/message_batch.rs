use std::collections::HashSet;

use serde::de::Error as DeError;
use serde::ser::{SerializeStruct, Serializer};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::types::{Message, MessageCreateParams};

const MAX_MESSAGE_BATCH_REQUESTS: usize = 100_000;
const MAX_MESSAGE_BATCH_BODY_BYTES: usize = 256 * 1024 * 1024;

/// Parameters for creating a Message Batch.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct MessageBatchCreateParams {
    /// Individual Messages API requests to process asynchronously.
    pub requests: Vec<MessageBatchCreateRequest>,

    /// Beta feature headers to include with this batch request.
    ///
    /// These are sent as the `anthropic-beta` HTTP header and are not serialized
    /// into the JSON request body.
    #[serde(skip)]
    pub betas: Option<Vec<String>>,
}

impl MessageBatchCreateParams {
    /// Create batch creation parameters from individual batch requests.
    pub fn new(requests: Vec<MessageBatchCreateRequest>) -> Self {
        Self {
            requests,
            betas: None,
        }
    }

    /// Set beta feature headers for this batch creation request.
    pub fn with_betas(mut self, betas: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.betas = Some(betas.into_iter().map(Into::into).collect());
        self
    }

    /// Add a single beta feature header for this batch creation request.
    pub fn with_beta(mut self, beta: impl Into<String>) -> Self {
        self.betas.get_or_insert_with(Vec::new).push(beta.into());
        self
    }

    /// Validate batch creation parameters before sending them to the API.
    pub fn validate(&self) -> crate::Result<()> {
        if self.requests.is_empty() {
            return Err(crate::Error::validation(
                "At least one batch request is required",
                Some("requests".to_string()),
            ));
        }

        if self.requests.len() > MAX_MESSAGE_BATCH_REQUESTS {
            return Err(crate::Error::validation(
                format!(
                    "Batch request count {} exceeds limit of {}",
                    self.requests.len(),
                    MAX_MESSAGE_BATCH_REQUESTS
                ),
                Some("requests".to_string()),
            ));
        }

        let mut custom_ids = HashSet::with_capacity(self.requests.len());
        for (i, request) in self.requests.iter().enumerate() {
            if !is_valid_custom_id(&request.custom_id) {
                return Err(crate::Error::validation(
                    "custom_id must be 1 to 64 characters and contain only alphanumeric characters, hyphens, and underscores",
                    Some(format!("requests[{i}].custom_id")),
                ));
            }

            if !custom_ids.insert(request.custom_id.as_str()) {
                return Err(crate::Error::validation(
                    format!("Duplicate custom_id: {}", request.custom_id),
                    Some(format!("requests[{i}].custom_id")),
                ));
            }

            if request.params.stream {
                return Err(crate::Error::validation(
                    "stream is not supported in message batch requests",
                    Some(format!("requests[{i}].params.stream")),
                ));
            }

            request.params.validate().map_err(|err| match err {
                crate::Error::Validation { message, param } => crate::Error::validation(
                    message,
                    param.map(|param| format!("requests[{i}].params.{param}")),
                ),
                other => other,
            })?;
        }

        let body = serde_json::to_vec(self).map_err(|e| {
            crate::Error::serialization(
                format!("Failed to serialize message batch create params: {e}"),
                Some(Box::new(e)),
            )
        })?;
        if body.len() > MAX_MESSAGE_BATCH_BODY_BYTES {
            return Err(crate::Error::validation(
                format!(
                    "Serialized batch request size {} exceeds limit of {} bytes",
                    body.len(),
                    MAX_MESSAGE_BATCH_BODY_BYTES
                ),
                Some("requests".to_string()),
            ));
        }

        Ok(())
    }
}

impl Serialize for MessageBatchCreateParams {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("MessageBatchCreateParams", 1)?;
        state.serialize_field("requests", &self.requests)?;
        state.end()
    }
}

/// A single Messages API request within a Message Batch.
#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct MessageBatchCreateRequest {
    /// Caller-provided identifier used to match results back to requests.
    pub custom_id: String,

    /// Non-streaming Messages API parameters for this individual request.
    pub params: MessageCreateParams,
}

impl MessageBatchCreateRequest {
    /// Create a batch request from a custom ID and message creation parameters.
    pub fn new(custom_id: impl Into<String>, params: MessageCreateParams) -> Self {
        Self {
            custom_id: custom_id.into(),
            params,
        }
    }
}

impl Serialize for MessageBatchCreateRequest {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut state = serializer.serialize_struct("MessageBatchCreateRequest", 2)?;
        state.serialize_field("custom_id", &self.custom_id)?;
        state.serialize_field("params", &MessageBatchRequestParams(&self.params))?;
        state.end()
    }
}

struct MessageBatchRequestParams<'a>(&'a MessageCreateParams);

impl Serialize for MessageBatchRequestParams<'_> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let params = self.0;
        let mut len = 3;
        len += usize::from(params.cache_control.is_some());
        len += usize::from(params.metadata.is_some());
        len += usize::from(params.output_format.is_some());
        len += usize::from(params.output_config.is_some());
        len += usize::from(params.stop_sequences.is_some());
        len += usize::from(params.system.is_some());
        len += usize::from(params.temperature.is_some());
        len += usize::from(params.thinking.is_some());
        len += usize::from(params.tool_choice.is_some());
        len += usize::from(params.tools.is_some());
        len += usize::from(params.top_k.is_some());
        len += usize::from(params.top_p.is_some());

        let mut state = serializer.serialize_struct("MessageBatchRequestParams", len)?;
        state.serialize_field("max_tokens", &params.max_tokens)?;
        state.serialize_field("messages", &params.messages)?;
        state.serialize_field("model", &params.model)?;
        if let Some(cache_control) = &params.cache_control {
            state.serialize_field("cache_control", cache_control)?;
        }
        if let Some(metadata) = &params.metadata {
            state.serialize_field("metadata", metadata)?;
        }
        if let Some(output_format) = &params.output_format {
            state.serialize_field("output_format", output_format)?;
        }
        if let Some(output_config) = &params.output_config {
            state.serialize_field("output_config", output_config)?;
        }
        if let Some(stop_sequences) = &params.stop_sequences {
            state.serialize_field("stop_sequences", stop_sequences)?;
        }
        if let Some(system) = &params.system {
            state.serialize_field("system", system)?;
        }
        if let Some(temperature) = &params.temperature {
            state.serialize_field("temperature", temperature)?;
        }
        if let Some(thinking) = &params.thinking {
            state.serialize_field("thinking", thinking)?;
        }
        if let Some(tool_choice) = &params.tool_choice {
            state.serialize_field("tool_choice", tool_choice)?;
        }
        if let Some(tools) = &params.tools {
            state.serialize_field("tools", tools)?;
        }
        if let Some(top_k) = &params.top_k {
            state.serialize_field("top_k", top_k)?;
        }
        if let Some(top_p) = &params.top_p {
            state.serialize_field("top_p", top_p)?;
        }
        state.end()
    }
}

/// A Message Batch returned by the API.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MessageBatch {
    /// Unique object identifier.
    pub id: String,

    /// Object type, always `"message_batch"`.
    #[serde(rename = "type")]
    pub r#type: String,

    /// Current processing status for the batch.
    pub processing_status: MessageBatchProcessingStatus,

    /// Counts of individual requests by processing state.
    pub request_counts: MessageBatchRequestCounts,

    /// Time at which processing ended, if the batch has ended.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "time::serde::rfc3339::option"
    )]
    pub ended_at: Option<OffsetDateTime>,

    /// Time at which the batch was created.
    #[serde(with = "time::serde::rfc3339")]
    pub created_at: OffsetDateTime,

    /// Time at which the batch expires if it has not completed.
    #[serde(with = "time::serde::rfc3339")]
    pub expires_at: OffsetDateTime,

    /// Time at which cancellation was initiated, if any.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "time::serde::rfc3339::option"
    )]
    pub cancel_initiated_at: Option<OffsetDateTime>,

    /// URL where results may be downloaded once available.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub results_url: Option<String>,

    /// Time at which results were archived and became unavailable, if any.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        with = "time::serde::rfc3339::option"
    )]
    pub archived_at: Option<OffsetDateTime>,
}

/// Processing status for a Message Batch.
#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MessageBatchProcessingStatus {
    /// The batch is actively processing.
    InProgress,

    /// Cancellation has been requested and is being finalized.
    Canceling,

    /// The batch has ended and no more requests will be processed.
    Ended,
}

/// Counts of individual batch requests by processing state.
#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MessageBatchRequestCounts {
    /// Requests still being processed.
    pub processing: u32,

    /// Requests that completed successfully.
    pub succeeded: u32,

    /// Requests that returned an error without creating a message.
    pub errored: u32,

    /// Requests canceled before being sent to the model.
    pub canceled: u32,

    /// Requests that expired before being sent to the model.
    pub expired: u32,
}

/// Parameters for listing Message Batches.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct MessageBatchListParams {
    /// ID of the object to use as a cursor for results after this object.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after_id: Option<String>,

    /// ID of the object to use as a cursor for results before this object.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before_id: Option<String>,

    /// Number of items to return per page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,

    /// Beta feature headers to include with this list request.
    #[serde(skip)]
    pub betas: Option<Vec<String>>,
}

impl MessageBatchListParams {
    /// Create empty list parameters.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the `after_id` cursor.
    pub fn with_after_id(mut self, after_id: impl Into<String>) -> Self {
        self.after_id = Some(after_id.into());
        self
    }

    /// Set the `before_id` cursor.
    pub fn with_before_id(mut self, before_id: impl Into<String>) -> Self {
        self.before_id = Some(before_id.into());
        self
    }

    /// Set the number of items to return.
    pub fn with_limit(mut self, limit: u32) -> Self {
        self.limit = Some(limit);
        self
    }

    /// Set beta feature headers for this list request.
    pub fn with_betas(mut self, betas: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.betas = Some(betas.into_iter().map(Into::into).collect());
        self
    }

    /// Add a single beta feature header for this list request.
    pub fn with_beta(mut self, beta: impl Into<String>) -> Self {
        self.betas.get_or_insert_with(Vec::new).push(beta.into());
        self
    }
}

/// A page of Message Batch objects.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MessageBatchListResponse {
    /// Message Batch objects returned by the API.
    pub data: Vec<MessageBatch>,

    /// Whether another page is available.
    pub has_more: bool,

    /// The first object ID in this page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_id: Option<String>,

    /// The last object ID in this page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_id: Option<String>,
}

/// A single result line from a Message Batch results JSONL stream.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MessageBatchResult {
    /// Caller-provided ID from the corresponding batch request.
    pub custom_id: String,

    /// Result for the individual request.
    pub result: MessageBatchResultVariant,
}

/// Result for a single request within a Message Batch.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum MessageBatchResultVariant {
    /// The request completed successfully and produced a message.
    #[serde(rename = "succeeded")]
    Succeeded {
        /// Message generated by the model.
        message: Message,
    },

    /// The request failed before a message was created.
    #[serde(rename = "errored")]
    Errored {
        /// Standard Anthropic error response for the failed request.
        error: MessageBatchErrorResponse,
    },

    /// The request was canceled before being sent to the model.
    #[serde(rename = "canceled")]
    Canceled,

    /// The request expired before being sent to the model.
    #[serde(rename = "expired")]
    Expired,
}

/// Standard Anthropic error response embedded in a batch result.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct MessageBatchErrorResponse {
    /// Object type, normally `"error"`.
    #[serde(rename = "type")]
    pub r#type: String,

    /// Error details.
    pub error: MessageBatchError,
}

impl<'de> Deserialize<'de> for MessageBatchErrorResponse {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Helper {
            #[serde(rename = "type")]
            r#type: Option<String>,
            error: Option<MessageBatchError>,
            message: Option<String>,
            param: Option<String>,
        }

        let helper = Helper::deserialize(deserializer)?;
        if let Some(error) = helper.error {
            return Ok(Self {
                r#type: helper.r#type.unwrap_or_else(|| "error".to_string()),
                error,
            });
        }

        let error_type = helper
            .r#type
            .ok_or_else(|| D::Error::missing_field("type"))?;
        let message = helper
            .message
            .ok_or_else(|| D::Error::missing_field("message"))?;
        Ok(Self {
            r#type: "error".to_string(),
            error: MessageBatchError {
                r#type: error_type,
                message,
                param: helper.param,
            },
        })
    }
}

/// Error details for a failed request within a Message Batch.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MessageBatchError {
    /// Anthropic error type.
    #[serde(rename = "type")]
    pub r#type: String,

    /// Human-readable error message.
    pub message: String,

    /// Request parameter associated with the error, if supplied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub param: Option<String>,
}

/// Response returned after deleting a Message Batch.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DeletedMessageBatch {
    /// Deleted batch ID.
    pub id: String,

    /// Object type, always `"message_batch_deleted"`.
    #[serde(rename = "type")]
    pub r#type: String,
}

fn is_valid_custom_id(custom_id: &str) -> bool {
    !custom_id.is_empty()
        && custom_id.len() <= 64
        && custom_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{KnownModel, MessageParam, Model, TextBlock, Usage};
    use serde_json::{json, to_value};
    use time::macros::datetime;

    fn valid_message_params() -> MessageCreateParams {
        MessageCreateParams::new(
            1024,
            vec![MessageParam::user("Hello, world")],
            Model::Known(KnownModel::ClaudeOpus48),
        )
    }

    fn valid_batch_request(custom_id: &str) -> MessageBatchCreateRequest {
        MessageBatchCreateRequest::new(custom_id, valid_message_params())
    }

    #[test]
    fn batch_create_params_serialize_without_stream_or_betas() {
        let params = MessageBatchCreateParams::new(vec![valid_batch_request("my-first-request")])
            .with_beta("output-300k-2026-03-24");

        let json = to_value(&params).unwrap();
        assert_eq!(
            json,
            json!({
                "requests": [{
                    "custom_id": "my-first-request",
                    "params": {
                        "max_tokens": 1024,
                        "messages": [{
                            "role": "user",
                            "content": "Hello, world"
                        }],
                        "model": "claude-opus-4-8"
                    }
                }]
            })
        );
        assert!(json["requests"][0]["params"].get("stream").is_none());
        assert!(json.get("betas").is_none());
    }

    #[test]
    fn batch_create_params_validate_success() {
        let params = MessageBatchCreateParams::new(vec![valid_batch_request("request_1")]);
        assert!(params.validate().is_ok());
    }

    #[test]
    fn batch_create_params_reject_empty_requests() {
        let params = MessageBatchCreateParams::new(Vec::new());
        assert!(params.validate().unwrap_err().is_validation());
    }

    #[test]
    fn batch_create_params_reject_too_many_requests() {
        let params = MessageBatchCreateParams::new(vec![
            valid_batch_request("request_1");
            MAX_MESSAGE_BATCH_REQUESTS + 1
        ]);
        assert!(params.validate().unwrap_err().is_validation());
    }

    #[test]
    fn batch_create_params_reject_invalid_custom_id() {
        let params = MessageBatchCreateParams::new(vec![valid_batch_request("bad id")]);
        let err = params.validate().unwrap_err();
        assert!(err.is_validation());
        assert!(err.to_string().contains("custom_id"));
    }

    #[test]
    fn batch_create_params_reject_duplicate_custom_id() {
        let params = MessageBatchCreateParams::new(vec![
            valid_batch_request("same-id"),
            valid_batch_request("same-id"),
        ]);
        let err = params.validate().unwrap_err();
        assert!(err.is_validation());
        assert!(err.to_string().contains("Duplicate custom_id"));
    }

    #[test]
    fn batch_create_params_reject_streaming_request() {
        let mut request = valid_batch_request("streaming");
        request.params.stream = true;
        let params = MessageBatchCreateParams::new(vec![request]);
        let err = params.validate().unwrap_err();
        assert!(err.is_validation());
        assert!(err.to_string().contains("stream"));
    }

    #[test]
    fn batch_create_params_reject_zero_max_tokens() {
        let mut request = valid_batch_request("zero-tokens");
        request.params.max_tokens = 0;
        let params = MessageBatchCreateParams::new(vec![request]);
        let err = params.validate().unwrap_err();
        assert!(err.is_validation());
        assert!(err.to_string().contains("max_tokens"));
    }

    #[test]
    fn message_batch_deserialization() {
        let json = json!({
            "id": "msgbatch_01HkcTjaV5uDC8jWR4ZsDV8d",
            "type": "message_batch",
            "processing_status": "in_progress",
            "request_counts": {
                "processing": 2,
                "succeeded": 0,
                "errored": 0,
                "canceled": 0,
                "expired": 0
            },
            "ended_at": null,
            "created_at": "2024-09-24T18:37:24.100435Z",
            "expires_at": "2024-09-25T18:37:24.100435Z",
            "cancel_initiated_at": null,
            "results_url": null,
            "archived_at": null
        });

        let batch: MessageBatch = serde_json::from_value(json).unwrap();
        assert_eq!(batch.id, "msgbatch_01HkcTjaV5uDC8jWR4ZsDV8d");
        assert_eq!(
            batch.processing_status,
            MessageBatchProcessingStatus::InProgress
        );
        assert_eq!(batch.request_counts.processing, 2);
        assert!(batch.ended_at.is_none());
    }

    #[test]
    fn message_batch_list_response_deserialization() {
        let batch = MessageBatch {
            id: "msgbatch_123".to_string(),
            r#type: "message_batch".to_string(),
            processing_status: MessageBatchProcessingStatus::Ended,
            request_counts: MessageBatchRequestCounts {
                processing: 0,
                succeeded: 1,
                errored: 0,
                canceled: 0,
                expired: 0,
            },
            ended_at: Some(datetime!(2024-09-24 19:37:24 UTC)),
            created_at: datetime!(2024-09-24 18:37:24 UTC),
            expires_at: datetime!(2024-09-25 18:37:24 UTC),
            cancel_initiated_at: None,
            results_url: Some("https://api.anthropic.com/result".to_string()),
            archived_at: None,
        };
        let response = MessageBatchListResponse {
            data: vec![batch.clone()],
            has_more: false,
            first_id: Some(batch.id.clone()),
            last_id: Some(batch.id.clone()),
        };

        let json = to_value(&response).unwrap();
        let decoded: MessageBatchListResponse = serde_json::from_value(json).unwrap();
        assert_eq!(decoded.data[0], batch);
        assert!(!decoded.has_more);
    }

    #[test]
    fn batch_result_succeeded_deserialization() {
        let json = json!({
            "custom_id": "my-first-request",
            "result": {
                "type": "succeeded",
                "message": {
                    "id": "msg_123",
                    "type": "message",
                    "role": "assistant",
                    "model": "claude-opus-4-8",
                    "content": [{"type": "text", "text": "Hello"}],
                    "stop_reason": "end_turn",
                    "stop_sequence": null,
                    "usage": {"input_tokens": 10, "output_tokens": 2}
                }
            }
        });

        let result: MessageBatchResult = serde_json::from_value(json).unwrap();
        match result.result {
            MessageBatchResultVariant::Succeeded { message } => {
                assert_eq!(message.id, "msg_123");
            }
            _ => panic!("expected succeeded result"),
        }
    }

    #[test]
    fn batch_result_errored_deserializes_standard_error_shape() {
        let json = json!({
            "custom_id": "bad-request",
            "result": {
                "type": "errored",
                "error": {
                    "type": "error",
                    "error": {
                        "type": "invalid_request_error",
                        "message": "max_tokens must be at least 1"
                    }
                }
            }
        });

        let result: MessageBatchResult = serde_json::from_value(json).unwrap();
        match result.result {
            MessageBatchResultVariant::Errored { error } => {
                assert_eq!(error.r#type, "error");
                assert_eq!(error.error.r#type, "invalid_request_error");
            }
            _ => panic!("expected errored result"),
        }
    }

    #[test]
    fn batch_result_errored_deserializes_direct_error_shape() {
        let json = json!({
            "custom_id": "bad-request",
            "result": {
                "type": "errored",
                "error": {
                    "type": "invalid_request_error",
                    "message": "max_tokens must be at least 1"
                }
            }
        });

        let result: MessageBatchResult = serde_json::from_value(json).unwrap();
        match result.result {
            MessageBatchResultVariant::Errored { error } => {
                assert_eq!(error.r#type, "error");
                assert_eq!(error.error.r#type, "invalid_request_error");
            }
            _ => panic!("expected errored result"),
        }
    }

    #[test]
    fn batch_result_canceled_and_expired_deserialization() {
        let canceled: MessageBatchResult = serde_json::from_value(json!({
            "custom_id": "canceled-request",
            "result": {"type": "canceled"}
        }))
        .unwrap();
        assert!(matches!(
            canceled.result,
            MessageBatchResultVariant::Canceled
        ));

        let expired: MessageBatchResult = serde_json::from_value(json!({
            "custom_id": "expired-request",
            "result": {"type": "expired"}
        }))
        .unwrap();
        assert!(matches!(expired.result, MessageBatchResultVariant::Expired));
    }

    #[test]
    fn deleted_message_batch_deserialization() {
        let deleted: DeletedMessageBatch = serde_json::from_value(json!({
            "id": "msgbatch_123",
            "type": "message_batch_deleted"
        }))
        .unwrap();
        assert_eq!(deleted.id, "msgbatch_123");
        assert_eq!(deleted.r#type, "message_batch_deleted");
    }

    #[test]
    fn message_batch_result_round_trip_succeeded() {
        let message = Message::new(
            "msg_123".to_string(),
            vec![TextBlock::new("Hello").into()],
            Model::Known(KnownModel::ClaudeOpus48),
            Usage::new(1, 1),
        );
        let result = MessageBatchResult {
            custom_id: "request-1".to_string(),
            result: MessageBatchResultVariant::Succeeded { message },
        };

        let json = to_value(&result).unwrap();
        assert_eq!(json["result"]["type"], "succeeded");
        let decoded: MessageBatchResult = serde_json::from_value(json).unwrap();
        assert_eq!(decoded.custom_id, "request-1");
    }
}
