//! End-to-end tests for span_ops + run_with_baggage helpers.

use std::sync::Mutex;

use iii_sdk::{
    current_span_is_recording, get_baggage_entry, record_span_event, run_with_baggage,
    set_current_span_attribute, set_current_span_error, BaggageSpanProcessor,
};
use opentelemetry::trace::{TraceContextExt, Tracer};
use opentelemetry::Context;
use opentelemetry_sdk::trace::{InMemorySpanExporter, SdkTracerProvider, SimpleSpanProcessor};

/// `set_tracer_provider` is process-wide; serialize to prevent races.
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

fn first_span_attr(exporter: &InMemorySpanExporter, key: &str) -> Option<String> {
    let spans = exporter.get_finished_spans().expect("exporter ok");
    spans.first().and_then(|s| {
        s.attributes
            .iter()
            .find(|kv| kv.key.as_str() == key)
            .map(|kv| kv.value.as_str().to_string())
    })
}

#[test]
fn set_current_span_attribute_writes_to_active_span() {
    let _guard = SERIAL.lock().unwrap();
    let (exporter, _provider) = install_test_provider();

    let tracer = opentelemetry::global::tracer("test");
    let span = tracer.start("inner");
    let cx = Context::current_with_span(span);

    let _attach = cx.clone().attach();
    set_current_span_attribute("k", "v");
    drop(_attach);

    drop(cx);

    assert_eq!(first_span_attr(&exporter, "k").as_deref(), Some("v"));
}

#[test]
fn set_current_span_attribute_is_noop_with_no_active_span() {
    let _guard = SERIAL.lock().unwrap();
    let (exporter, _provider) = install_test_provider();

    set_current_span_attribute("orphan", "value");

    let spans = exporter.get_finished_spans().expect("exporter ok");
    assert!(spans.is_empty(), "no spans should have been exported");
}

#[test]
fn current_span_is_recording_reflects_active_span_state() {
    let _guard = SERIAL.lock().unwrap();
    let (_exporter, _provider) = install_test_provider();

    assert!(!current_span_is_recording());

    let tracer = opentelemetry::global::tracer("test");
    let span = tracer.start("inner");
    let cx = Context::current_with_span(span);
    let _attach = cx.clone().attach();

    assert!(current_span_is_recording());
}

#[test]
fn set_current_span_error_marks_status_error() {
    let _guard = SERIAL.lock().unwrap();
    let (exporter, _provider) = install_test_provider();

    let tracer = opentelemetry::global::tracer("test");
    let span = tracer.start("inner");
    let cx = Context::current_with_span(span);

    let _attach = cx.clone().attach();
    set_current_span_error("boom");
    drop(_attach);
    drop(cx);

    let spans = exporter.get_finished_spans().expect("exporter ok");
    let exported = spans.first().expect("one span");
    match &exported.status {
        opentelemetry::trace::Status::Error { description } => {
            assert_eq!(description.as_ref(), "boom");
        }
        other => panic!("expected Status::Error, got {other:?}"),
    }
}


#[tokio::test]
async fn run_with_baggage_attaches_entries_for_inner_scope() {
    let inside_value = run_with_baggage(&[("k", "v")], async {
        get_baggage_entry("k")
    })
    .await;

    assert_eq!(inside_value.as_deref(), Some("v"));
}

#[tokio::test]
async fn run_with_baggage_does_not_leak_into_caller_scope() {
    run_with_baggage(&[("scoped", "yes")], async {
        assert_eq!(get_baggage_entry("scoped").as_deref(), Some("yes"));
    })
    .await;

    assert!(get_baggage_entry("scoped").is_none());
}

#[tokio::test]
async fn run_with_baggage_overwrites_existing_keys() {
    run_with_baggage(&[("k", "outer")], async {
        run_with_baggage(&[("k", "inner")], async {
            assert_eq!(get_baggage_entry("k").as_deref(), Some("inner"));
        })
        .await;

        assert_eq!(get_baggage_entry("k").as_deref(), Some("outer"));
    })
    .await;
}

#[test]
fn record_span_event_writes_event_with_attributes() {
    let _guard = SERIAL.lock().unwrap();
    let (exporter, _provider) = install_test_provider();

    let tracer = opentelemetry::global::tracer("test");
    let span = tracer.start("inner");
    let cx = Context::current_with_span(span);

    let _attach = cx.clone().attach();
    record_span_event(
        "iii.invocation.input",
        &[
            ("iii.payload.json".to_string(), r#"{"x":1}"#.to_string()),
            ("iii.payload.truncated".to_string(), "false".to_string()),
        ],
    );
    drop(_attach);
    drop(cx);

    let spans = exporter.get_finished_spans().expect("exporter ok");
    let span = spans.first().expect("one span exported");
    let event = span
        .events
        .iter()
        .find(|e| e.name == "iii.invocation.input")
        .expect("invocation.input event recorded");
    assert!(
        event
            .attributes
            .iter()
            .any(|kv| kv.key.as_str() == "iii.payload.json"
                && kv.value.as_str() == r#"{"x":1}"#),
        "payload.json attribute present on event"
    );
}

#[test]
fn record_span_event_is_noop_without_active_span() {
    let _guard = SERIAL.lock().unwrap();
    let (exporter, _provider) = install_test_provider();

    record_span_event(
        "iii.orphan.event",
        &[("k".to_string(), "v".to_string())],
    );

    let spans = exporter.get_finished_spans().expect("exporter ok");
    assert!(spans.is_empty(), "no spans should have been exported");
}
