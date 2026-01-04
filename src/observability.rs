use biometrics::{Collector, Counter, Moments};

pub(crate) static CLIENT_REQUESTS: Counter = Counter::new("claudius.client.requests");
pub(crate) static CLIENT_REQUEST_ERRORS: Counter = Counter::new("claudius.client.request_errors");
pub(crate) static CLIENT_REQUEST_RETRIES: Counter = Counter::new("claudius.client.retries");
pub(crate) static CLIENT_REQUEST_DURATION: Moments =
    Moments::new("claudius.client.request_duration_seconds");
pub(crate) static CLIENT_RETRY_BACKOFF: Moments =
    Moments::new("claudius.client.retry_backoff_seconds");

pub(crate) static STREAM_EVENTS: Counter = Counter::new("claudius.stream.events");
pub(crate) static STREAM_ERRORS: Counter = Counter::new("claudius.stream.errors");
pub(crate) static STREAM_BYTES: Counter = Counter::new("claudius.stream.bytes");
pub(crate) static STREAM_TTFB: Moments = Moments::new("claudius.stream.ttfb_seconds");
pub(crate) static STREAM_DURATION: Moments = Moments::new("claudius.stream.duration_seconds");

pub(crate) static AGENT_TURN_DURATION: Moments =
    Moments::new("claudius.agent.turn_duration_seconds");
pub(crate) static AGENT_TURN_REQUESTS: Counter = Counter::new("claudius.agent.turn_requests");
pub(crate) static AGENT_TOOL_CALLS: Counter = Counter::new("claudius.agent.tool_calls");
pub(crate) static AGENT_TOOL_ERRORS: Counter = Counter::new("claudius.agent.tool_errors");
pub(crate) static AGENT_TOOL_DURATION: Moments =
    Moments::new("claudius.agent.tool_duration_seconds");

/// Register this crate's biometrics with the provided collector.
pub fn register_biometrics(collector: Collector) {
    collector.register_counter(&CLIENT_REQUESTS);
    collector.register_counter(&CLIENT_REQUEST_ERRORS);
    collector.register_counter(&CLIENT_REQUEST_RETRIES);
    collector.register_moments(&CLIENT_REQUEST_DURATION);
    collector.register_moments(&CLIENT_RETRY_BACKOFF);

    collector.register_counter(&STREAM_EVENTS);
    collector.register_counter(&STREAM_ERRORS);
    collector.register_counter(&STREAM_BYTES);
    collector.register_moments(&STREAM_TTFB);
    collector.register_moments(&STREAM_DURATION);

    collector.register_moments(&AGENT_TURN_DURATION);
    collector.register_counter(&AGENT_TURN_REQUESTS);
    collector.register_counter(&AGENT_TOOL_CALLS);
    collector.register_counter(&AGENT_TOOL_ERRORS);
    collector.register_moments(&AGENT_TOOL_DURATION);
}
