//! W3C Trace Context and Baggage Propagation
//!
//! This module provides functions for working with W3C trace context and baggage headers,
//! enabling distributed tracing across service boundaries.

use opentelemetry::baggage::BaggageExt;
use opentelemetry::propagation::{Extractor, Injector, TextMapPropagator};
use opentelemetry::trace::TraceContextExt;
use opentelemetry::{Context as OtelContext, KeyValue};
use opentelemetry_sdk::propagation::{BaggagePropagator, TraceContextPropagator};
use std::collections::HashMap;
use std::sync::OnceLock;

static TRACE_PROPAGATOR: OnceLock<TraceContextPropagator> = OnceLock::new();
static BAGGAGE_PROPAGATOR: OnceLock<BaggagePropagator> = OnceLock::new();

fn trace_propagator() -> &'static TraceContextPropagator {
    TRACE_PROPAGATOR.get_or_init(TraceContextPropagator::new)
}

fn baggage_propagator() -> &'static BaggagePropagator {
    BAGGAGE_PROPAGATOR.get_or_init(BaggagePropagator::new)
}

/// A newtype wrapper around HashMap to implement OpenTelemetry traits
struct HeaderMap(HashMap<String, String>);

impl Injector for HeaderMap {
    fn set(&mut self, key: &str, value: String) {
        self.0.insert(key.to_string(), value);
    }
}

impl Extractor for HeaderMap {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).map(|v| v.as_str())
    }

    fn keys(&self) -> Vec<&str> {
        self.0.keys().map(|k| k.as_str()).collect()
    }
}

/// Get the current trace ID from the active span context
pub fn current_trace_id() -> Option<String> {
    let cx = OtelContext::current();
    let span = cx.span();
    let span_context = span.span_context();

    if span_context.is_valid() {
        Some(span_context.trace_id().to_string())
    } else {
        None
    }
}

/// Get the current span ID from the active span context
pub fn current_span_id() -> Option<String> {
    let cx = OtelContext::current();
    let span = cx.span();
    let span_context = span.span_context();

    if span_context.is_valid() {
        Some(span_context.span_id().to_string())
    } else {
        None
    }
}

/// Inject current trace context into a W3C traceparent header string
pub fn inject_traceparent() -> Option<String> {
    let cx = OtelContext::current();
    let mut carrier = HeaderMap(HashMap::new());

    trace_propagator().inject_context(&cx, &mut carrier);

    carrier.0.get("traceparent").cloned()
}

/// Extract trace context from a W3C traceparent header string
pub fn extract_traceparent(traceparent: &str) -> OtelContext {
    let mut carrier = HeaderMap(HashMap::new());
    carrier
        .0
        .insert("traceparent".to_string(), traceparent.to_string());

    trace_propagator().extract(&carrier)
}

/// Inject current baggage into a W3C baggage header string
pub fn inject_baggage() -> Option<String> {
    let cx = OtelContext::current();
    let mut carrier = HeaderMap(HashMap::new());

    baggage_propagator().inject_context(&cx, &mut carrier);

    carrier.0.get("baggage").cloned()
}

/// Extract baggage from a W3C baggage header string
pub fn extract_baggage(baggage: &str) -> OtelContext {
    let mut carrier = HeaderMap(HashMap::new());
    carrier.0.insert("baggage".to_string(), baggage.to_string());

    baggage_propagator().extract(&carrier)
}

/// Extract both trace context and baggage from their respective headers
pub fn extract_context(traceparent: Option<&str>, baggage: Option<&str>) -> OtelContext {
    let mut carrier = HeaderMap(HashMap::new());

    if let Some(tp) = traceparent {
        carrier.0.insert("traceparent".to_string(), tp.to_string());
    }

    if let Some(bg) = baggage {
        carrier.0.insert("baggage".to_string(), bg.to_string());
    }

    // Extract trace context first
    let cx = trace_propagator().extract(&carrier);

    // Then extract baggage into that context
    baggage_propagator().extract_with_context(&cx, &carrier)
}

/// Get a baggage entry from the current context
pub fn get_baggage_entry(key: &str) -> Option<String> {
    let cx = OtelContext::current();
    cx.baggage().get(key).map(|value| value.to_string())
}

/// Set a baggage entry in the current context (returns new context)
pub fn set_baggage_entry(key: &str, value: &str) -> OtelContext {
    let cx = OtelContext::current();
    let baggage = cx.baggage();

    // Build vec of all entries except the one we're replacing
    let mut entries: Vec<KeyValue> = baggage
        .iter()
        .filter(|(k, _)| k.as_str() != key)
        .map(|(k, (v, _meta))| KeyValue::new(k.clone(), v.clone()))
        .collect();

    // Add the new entry
    entries.push(KeyValue::new(key.to_string(), value.to_string()));

    cx.with_baggage(entries)
}

/// Remove a baggage entry from the current context
pub fn remove_baggage_entry(key: &str) -> OtelContext {
    let cx = OtelContext::current();
    let baggage = cx.baggage();

    // Build vec of all entries except the one we're removing
    let entries: Vec<KeyValue> = baggage
        .iter()
        .filter(|(k, _)| k.as_str() != key)
        .map(|(k, (v, _meta))| KeyValue::new(k.clone(), v.clone()))
        .collect();

    cx.with_baggage(entries)
}

/// Get all baggage entries from the current context
pub fn get_all_baggage() -> HashMap<String, String> {
    let cx = OtelContext::current();
    cx.baggage()
        .iter()
        .map(|(k, (v, _meta))| (k.to_string(), v.to_string()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use opentelemetry::trace::{SpanContext, SpanId, TraceFlags, TraceId};

    #[test]
    fn test_inject_extract_traceparent() {
        // Create a test span context
        let trace_id = TraceId::from_hex("4bf92f3577b34da6a3ce929d0e0e4736").unwrap();
        let span_id = SpanId::from_hex("00f067aa0ba902b7").unwrap();
        let span_context = SpanContext::new(
            trace_id,
            span_id,
            TraceFlags::SAMPLED,
            false,
            Default::default(),
        );

        // Create context with this span
        let cx = OtelContext::current().with_remote_span_context(span_context);

        // Make it the current context
        let _guard = cx.attach();

        // Inject
        let traceparent = inject_traceparent();
        assert!(traceparent.is_some());

        let tp = traceparent.unwrap();
        assert!(tp.starts_with("00-"));
        assert!(tp.contains("4bf92f3577b34da6a3ce929d0e0e4736"));
    }

    #[test]
    fn test_extract_traceparent() {
        let traceparent = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
        let cx = extract_traceparent(traceparent);

        let span = cx.span();
        let span_context = span.span_context();
        assert!(span_context.is_valid());
        assert_eq!(
            span_context.trace_id().to_string(),
            "4bf92f3577b34da6a3ce929d0e0e4736"
        );
        assert_eq!(span_context.span_id().to_string(), "00f067aa0ba902b7");
    }

    #[test]
    fn test_baggage_operations() {
        let cx = set_baggage_entry("user_id", "12345");
        let _guard = cx.attach();

        assert_eq!(get_baggage_entry("user_id"), Some("12345".to_string()));

        let all = get_all_baggage();
        assert_eq!(all.get("user_id"), Some(&"12345".to_string()));

        drop(_guard);
        let cx = remove_baggage_entry("user_id");
        let _guard = cx.attach();

        assert_eq!(get_baggage_entry("user_id"), None);
    }

    #[test]
    fn test_inject_extract_baggage() {
        let cx = set_baggage_entry("key1", "value1");
        let entries: Vec<KeyValue> = vec![
            KeyValue::new("key1".to_string(), "value1".to_string()),
            KeyValue::new("key2".to_string(), "value2".to_string()),
        ];
        let cx = cx.with_baggage(entries);
        let _guard = cx.attach();

        let baggage_header = inject_baggage();
        assert!(baggage_header.is_some());

        let header = baggage_header.unwrap();
        assert!(header.contains("key1=value1"));
        assert!(header.contains("key2=value2"));

        drop(_guard);
        // Extract it back
        let extracted_cx = extract_baggage(&header);
        let _guard = extracted_cx.attach();

        assert_eq!(get_baggage_entry("key1"), Some("value1".to_string()));
        assert_eq!(get_baggage_entry("key2"), Some("value2".to_string()));
    }
}
