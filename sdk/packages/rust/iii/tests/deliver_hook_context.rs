//! A deliver hook runs in the function's invocation context and span: the
//! calls it makes carry the function's invocation as their caller, and the
//! trace context of its `execute` span. A test binary of its own, since it
//! installs a recording tracer provider for the whole process.

mod common;

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use opentelemetry_sdk::trace::{InMemorySpanExporter, SdkTracerProvider, SimpleSpanProcessor};
use serde_json::{Value, json};

use common::mock_engine::{MockEngine, message_type};
use iii_helpers::observability::OtelConfig;
use iii_sdk::runtime::WorkerMetadata;
use iii_sdk::{
    Call, CallAction, CallRequest, Error, InitOptions, Observer, RegisterFunction,
    WorkerIdentityMode, current_invocation, register_worker,
};

const WAIT: Duration = Duration::from_secs(5);
const TRACEPARENT: &str = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";

/// An invocation the handler ran as: its id and its trace context.
type Invocation = (String, Option<String>);

/// The span id of a W3C `traceparent`.
fn span_id(traceparent: &str) -> &str {
    traceparent.split('-').nth(2).expect("a traceparent")
}

#[tokio::test]
async fn a_deliver_hook_calls_from_the_functions_invocation_and_span() {
    let exporter = InMemorySpanExporter::default();
    let provider = SdkTracerProvider::builder()
        .with_span_processor(SimpleSpanProcessor::new(exporter.clone()))
        .build();
    opentelemetry::global::set_tracer_provider(provider.clone());

    let mock = MockEngine::start().await;
    let iii = register_worker(
        mock.url(),
        InitOptions {
            metadata: Some(WorkerMetadata {
                name: "http".into(),
                ..WorkerMetadata::default()
            }),
            otel: Some(OtelConfig {
                enabled: Some(false),
                ..OtelConfig::default()
            }),
            identity: WorkerIdentityMode::Explicit,
            ..InitOptions::default()
        },
    );
    mock.wait_for(
        |m| {
            m.iter()
                .any(|f| f["function_id"] == "engine::workers::register")
        },
        WAIT,
    )
    .await;
    mock.send_to_client(json!({
        "type": "workerregistered", "worker_id": "w-1",
        "features": ["observe.v1", "observe.v2", "local-dispatch.v1"]
    }));

    // the handler records the invocation it runs as
    let invocations: Arc<Mutex<Vec<Invocation>>> = Arc::default();
    iii.on_observer_deliver_for("dc::on-request", {
        let iii = iii.clone();
        move |call: Call, observers: Vec<Observer>| {
            let iii = iii.clone();
            async move {
                let route = observers.first().expect("the route");
                iii.call(CallRequest::observer(route, call.input.clone()).action(CallAction::Void))
                    .await
            }
        }
    });
    let function = iii.register_function(
        "dc::on-request",
        RegisterFunction::new({
            let invocations = invocations.clone();
            move |request: Value| {
                let ctx = current_invocation().expect("the function's invocation");
                invocations
                    .lock()
                    .unwrap()
                    .push((ctx.invocation_id, ctx.traceparent));
                Ok::<_, Error>(request)
            }
        })
        .private(),
    );
    mock.wait_for(
        |m| {
            m.iter()
                .any(|f| f["type"] == "registerfunction" && f["id"] == "dc::on-request")
        },
        WAIT,
    )
    .await;
    mock.send_to_client(json!({
        "type": "functionregistered", "id": "dc::on-request", "namespace": "default",
        "revision": 1, "local_dispatch": true,
        "observers": [{ "id": "route", "args": {}, "filters": {}, "target": "t::route",
                        "capture": ["input"], "on": "success" }]
    }));
    let deadline = tokio::time::Instant::now() + WAIT;
    while !(function.is_observed() && function.local_dispatch_active()) {
        assert!(tokio::time::Instant::now() < deadline, "never hosted");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    // the hook's call through the route, for the request to `path`
    let through = |msgs: &[Value], path: &str| {
        msgs.iter()
            .find(|m| {
                message_type(m) == Some("invokefunction")
                    && m["function_id"] == "t::route"
                    && m["data"]["path"] == path
            })
            .cloned()
    };
    let execute_span_ids = || -> Vec<String> {
        exporter
            .get_finished_spans()
            .expect("the exporter")
            .iter()
            .filter(|span| span.name == "execute dc::on-request")
            .map(|span| span.span_context.span_id().to_string())
            .collect()
    };

    // routed, with the caller's trace context
    mock.send_to_client(json!({
        "type": "invokefunction", "invocation_id": "41111111-2222-4333-8444-555555555555",
        "function_id": "dc::on-request", "data": { "path": "/routed" },
        "traceparent": TRACEPARENT
    }));
    let msgs = mock
        .wait_for(|m| through(m, "/routed").is_some(), WAIT)
        .await;
    let call = through(&msgs, "/routed").expect("the hook's call");
    let (invocation_id, traceparent) = invocations.lock().unwrap()[0].clone();
    assert_eq!(invocation_id, "41111111-2222-4333-8444-555555555555");
    assert_eq!(call["caller"]["function_id"], "dc::on-request");
    assert_eq!(call["caller"]["invocation_id"], json!(invocation_id));
    let traceparent = traceparent.expect("the execute span's trace context");
    assert!(
        traceparent.starts_with("00-4bf92f3577b34da6a3ce929d0e0e4736-"),
        "{traceparent}"
    );
    assert_ne!(span_id(&traceparent), span_id(TRACEPARENT), "its own span");
    assert_eq!(call["traceparent"], json!(traceparent), "{call:#}");

    // local
    iii.call(CallRequest::new(
        "dc::on-request",
        json!({ "path": "/local" }),
    ))
    .await
    .unwrap();
    let msgs = mock
        .wait_for(|m| through(m, "/local").is_some(), WAIT)
        .await;
    let call = through(&msgs, "/local").expect("the hook's call");
    let (local_id, local_traceparent) = invocations.lock().unwrap()[1].clone();
    assert_eq!(call["caller"]["invocation_id"], json!(local_id));
    let local_traceparent = local_traceparent.expect("the execute span's trace context");
    assert_eq!(call["traceparent"], json!(local_traceparent), "{call:#}");

    // both are the function's `execute` spans
    let spans = execute_span_ids();
    for traceparent in [&traceparent, &local_traceparent] {
        assert!(
            spans.iter().any(|id| id == span_id(traceparent)),
            "{traceparent} is not an execute span: {spans:?}"
        );
    }
    iii.shutdown_async().await;
    let _ = provider.shutdown();
}
