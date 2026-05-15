//! Integration test for the env-flag-gated invocation payload auto-capture.

use std::sync::Mutex;

use iii_sdk::BaggageSpanProcessor;
use iii_sdk::telemetry::payload::redact_and_truncate;
use opentelemetry::trace::{Status, TraceContextExt, Tracer};
use opentelemetry::{Context, KeyValue};
use opentelemetry_sdk::trace::{InMemorySpanExporter, SdkTracerProvider, SimpleSpanProcessor};
use serde_json::json;

static SERIAL: Mutex<()> = Mutex::new(());

fn install_test_provider() -> (InMemorySpanExporter, SdkTracerProvider) {
    let exporter = InMemorySpanExporter::default();
    let provider = SdkTracerProvider::builder()
        .with_span_processor(BaggageSpanProcessor::default())
        .with_span_processor(SimpleSpanProcessor::new(exporter.clone()))
        .build();
    opentelemetry::global::set_tracer_provider(provider.clone());
    (exporter, provider)
}

fn capture_input_event(cx: &Context, data: &serde_json::Value, enabled: bool) {
    if !enabled {
        return;
    }
    let span = cx.span();
    if !span.span_context().is_valid() {
        return;
    }
    let (json, truncated) = redact_and_truncate(data, Some(4096));
    span.add_event(
        "iii.invocation.input",
        vec![
            KeyValue::new("iii.payload.json", json),
            KeyValue::new("iii.payload.truncated", truncated),
        ],
    );
}

fn capture_output_event(cx: &Context, result: &Result<serde_json::Value, String>, enabled: bool) {
    if !enabled {
        return;
    }
    let span = cx.span();
    if !span.span_context().is_valid() {
        return;
    }
    let (json, truncated, ok) = match result {
        Ok(v) => {
            let (j, t) = redact_and_truncate(v, Some(4096));
            (j, t, true)
        }
        Err(err) => {
            let payload = json!({ "error": err });
            let (j, t) = redact_and_truncate(&payload, Some(4096));
            (j, t, false)
        }
    };
    span.add_event(
        "iii.invocation.output",
        vec![
            KeyValue::new("iii.payload.json", json),
            KeyValue::new("iii.payload.truncated", truncated),
            KeyValue::new("iii.payload.ok", ok),
        ],
    );
}

fn read_env_flag() -> bool {
    !std::env::var("III_DISABLE_TRACE_PAYLOADS")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

fn find_event<'a>(
    span: &'a opentelemetry_sdk::trace::SpanData,
    name: &str,
) -> Option<&'a opentelemetry_sdk::trace::SpanEvents> {
    if span.events.events.iter().any(|e| e.name == name) {
        Some(&span.events)
    } else {
        None
    }
}

fn event_attr(
    span: &opentelemetry_sdk::trace::SpanData,
    event_name: &str,
    key: &str,
) -> Option<String> {
    let evt = span.events.events.iter().find(|e| e.name == event_name)?;
    evt.attributes
        .iter()
        .find(|kv| kv.key.as_str() == key)
        .map(|kv| kv.value.as_str().to_string())
}

#[test]
fn input_and_output_events_recorded_when_enabled() {
    let _guard = SERIAL.lock().unwrap();
    let (exporter, _provider) = install_test_provider();

    unsafe { std::env::remove_var("III_DISABLE_TRACE_PAYLOADS") };
    let enabled = read_env_flag();
    assert!(enabled, "default state must be enabled");

    let tracer = opentelemetry::global::tracer("test");
    let span = tracer.start("call test::fn");
    let cx = Context::current_with_span(span);

    capture_input_event(&cx, &json!({"model":"claude-3-5","prompt":"hi"}), enabled);
    let result: Result<serde_json::Value, String> =
        Ok(json!({"content":[{"type":"text","text":"hello"}]}));
    capture_output_event(&cx, &result, enabled);

    drop(cx);

    let spans = exporter.get_finished_spans().expect("exporter ok");
    let span = spans.first().expect("one span exported");
    assert!(find_event(span, "iii.invocation.input").is_some());
    assert!(find_event(span, "iii.invocation.output").is_some());
    let input_payload = event_attr(span, "iii.invocation.input", "iii.payload.json").unwrap();
    assert!(input_payload.contains("claude-3-5"));
    let output_payload = event_attr(span, "iii.invocation.output", "iii.payload.json").unwrap();
    assert!(output_payload.contains("hello"));
}

#[test]
fn events_suppressed_when_env_disables() {
    let _guard = SERIAL.lock().unwrap();
    let (exporter, _provider) = install_test_provider();

    unsafe { std::env::set_var("III_DISABLE_TRACE_PAYLOADS", "1") };
    let enabled = read_env_flag();
    assert!(!enabled, "kill switch must disable");

    let tracer = opentelemetry::global::tracer("test");
    let span = tracer.start("call test::fn");
    let cx = Context::current_with_span(span);

    capture_input_event(&cx, &json!({"x":1}), enabled);
    capture_output_event(&cx, &Ok(json!({"y":2})), enabled);
    drop(cx);

    unsafe { std::env::remove_var("III_DISABLE_TRACE_PAYLOADS") };

    let spans = exporter.get_finished_spans().expect("exporter ok");
    let span = spans.first().expect("span exported");
    assert!(find_event(span, "iii.invocation.input").is_none());
    assert!(find_event(span, "iii.invocation.output").is_none());
}

#[test]
fn output_event_marks_error_when_handler_fails() {
    let _guard = SERIAL.lock().unwrap();
    let (exporter, _provider) = install_test_provider();

    unsafe { std::env::remove_var("III_DISABLE_TRACE_PAYLOADS") };
    let enabled = read_env_flag();

    let tracer = opentelemetry::global::tracer("test");
    let span = tracer.start("call test::fn");
    let cx = Context::current_with_span(span);

    capture_input_event(&cx, &json!({"x":1}), enabled);
    let err: Result<serde_json::Value, String> = Err("rate limited".into());
    capture_output_event(&cx, &err, enabled);
    cx.span().set_status(Status::error("rate limited"));
    drop(cx);

    let spans = exporter.get_finished_spans().expect("exporter ok");
    let span = spans.first().expect("span exported");
    let ok_attr = span
        .events
        .events
        .iter()
        .find(|e| e.name == "iii.invocation.output")
        .expect("output event present")
        .attributes
        .iter()
        .find(|kv| kv.key.as_str() == "iii.payload.ok")
        .map(|kv| kv.value.as_str().to_string())
        .expect("ok attribute present");
    assert_eq!(ok_attr, "false", "ok=false on error path");

    let body = event_attr(span, "iii.invocation.output", "iii.payload.json").unwrap();
    assert!(body.contains("rate limited"));
}

#[test]
fn sensitive_keys_redacted_before_event_recorded() {
    let _guard = SERIAL.lock().unwrap();
    let (exporter, _provider) = install_test_provider();

    unsafe { std::env::remove_var("III_DISABLE_TRACE_PAYLOADS") };
    let enabled = read_env_flag();

    let tracer = opentelemetry::global::tracer("test");
    let span = tracer.start("call test::fn");
    let cx = Context::current_with_span(span);

    capture_input_event(
        &cx,
        &json!({
            "api_key": "sk-MUST-NOT-LEAK",
            "model": "claude-3-5",
            "messages": [{"role":"user","content":"hi"}]
        }),
        enabled,
    );
    drop(cx);

    let spans = exporter.get_finished_spans().expect("exporter ok");
    let span = spans.first().expect("span exported");
    let payload = event_attr(span, "iii.invocation.input", "iii.payload.json").unwrap();
    assert!(!payload.contains("sk-MUST-NOT-LEAK"));
    assert!(payload.contains("[REDACTED]"));
    assert!(payload.contains("claude-3-5"));
}
