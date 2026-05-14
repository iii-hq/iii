//! High-level span operations so consumers don't need `opentelemetry` directly.

use opentelemetry::trace::{Status, TraceContextExt};
use opentelemetry::{Context, KeyValue};

/// Returns `false` when there is no active span or the sampler dropped it.
#[must_use]
pub fn current_span_is_recording() -> bool {
    Context::current().span().span_context().is_valid()
}

/// No-op when the current span is not recording.
pub fn set_current_span_attribute(key: &'static str, value: impl Into<String>) {
    let cx = Context::current();
    let span = cx.span();
    if span.span_context().is_valid() {
        span.set_attribute(KeyValue::new(key, value.into()));
    }
}

/// No-op when there is no active span.
pub fn set_current_span_error(message: impl Into<String>) {
    let cx = Context::current();
    cx.span().set_status(Status::error(message.into()));
}

/// No-op when the current span is not recording.
pub fn record_span_event(name: impl Into<String>, attrs: &[(String, String)]) {
    let cx = Context::current();
    let span = cx.span();
    if !span.span_context().is_valid() {
        return;
    }
    let kvs: Vec<KeyValue> = attrs
        .iter()
        .map(|(k, v)| KeyValue::new(k.clone(), v.clone()))
        .collect();
    span.add_event(name.into(), kvs);
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn current_span_is_recording_returns_false_without_tracer() {
        assert!(!current_span_is_recording());
    }

    #[test]
    fn set_current_span_attribute_is_safe_without_tracer() {
        set_current_span_attribute("test.key", "value");
    }

    #[test]
    fn set_current_span_error_is_safe_without_tracer() {
        set_current_span_error("test error");
    }

    #[test]
    fn record_span_event_is_safe_without_tracer() {
        record_span_event(
            "test.event",
            &[
                ("k1".to_string(), "v1".to_string()),
                ("k2".to_string(), "v2".to_string()),
            ],
        );
    }
}
