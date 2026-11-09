//! Decision 62: a call's metadata is shared (`Arc<Value>`), not deep-copied,
//! by the observed call that keeps it and by the activation envelopes built
//! from it, and what the observers receive is unchanged.

use std::sync::{Arc, atomic::AtomicUsize};

use serde_json::{Value, json};
use uuid::Uuid;

use super::super::{InvocationCtx, events::test_support};
use crate::{
    engine::{Engine, EngineTrait, Handler, RegisterFunctionRequest},
    function::FunctionResult,
    protocol::{Capture, DEFAULT_NAMESPACE},
};

fn source(engine: &Engine) {
    engine.register_function_handler(
        RegisterFunctionRequest {
            function_id: "test::src".to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
        },
        Handler::new(|input: Value| async move { FunctionResult::Success(Some(input)) }),
    );
}

/// `id` observes `test::src` into `target`, capturing `capture`.
async fn observe(engine: &Arc<Engine>, id: &str, target: &str, capture: Vec<Capture>) {
    let (worker, socket) = test_support::worker(engine);
    // The observing worker's socket stays open for the test's length.
    std::mem::forget(socket);
    let mut request = test_support::request(id, "test::src", target, None);
    request.source.capture = Some(capture);
    request.source.raw_input = false;
    test_support::observe_request(engine, &worker, request).await;
}

/// Metadata a caller or a middleware may attach: routing keys, a tenant, a
/// large opaque blob.
fn call_metadata() -> Value {
    json!({
        "for_observer": "obs-a",
        "tenant": { "id": "acme", "roles": ["admin", "billing"], "n": 3, "ok": true },
        "blob": "m".repeat(4096),
        "nothing": null,
    })
}

fn completed(metadata: Option<Arc<Value>>) -> super::Completed {
    super::Completed {
        delivery: super::Delivery::Engine,
        function: Arc::new((DEFAULT_NAMESPACE.to_string(), "test::src".to_string())),
        invocation_id: super::InvocationId::Uuid(Uuid::new_v4()),
        success: true,
        output_is_input: false,
        input: Some(value(json!({ "n": 1 }))),
        output: Some(value(json!({ "ok": true }))),
        error: None,
        caller: None,
        metadata: metadata.map(super::Payload::Value),
        started_at_ms: 1,
        duration_ms: 2.0,
        depth: 0,
        traceparent: None,
        baggage: None,
        queues: AtomicUsize::new(0),
        charged: AtomicUsize::new(0),
        encoded: once_cell::race::OnceBox::new(),
    }
}

fn value(value: Value) -> super::Payload {
    super::Payload::Value(Arc::new(value))
}

/// The metadata an event holds as a value.
fn metadata_of(event: &super::Completed) -> Option<&Arc<Value>> {
    match &event.metadata {
        Some(super::Payload::Value(value)) => Some(value),
        _ => None,
    }
}

fn blob_ptr(value: &Value) -> *const u8 {
    value["blob"].as_str().expect("blob").as_ptr()
}

/// The envelope's `metadata` is the call's metadata, byte for byte, and the
/// rest of the envelope is untouched by it.
#[tokio::test]
async fn the_envelope_carries_the_call_metadata_unchanged() {
    let engine = Arc::new(Engine::new());
    source(&engine);
    let mut sink = test_support::capture(&engine, "test::dst");
    observe(
        &engine,
        "obs",
        "test::dst",
        vec![Capture::Input, Capture::Output, Capture::Metadata],
    )
    .await;

    let out = engine
        .call_with_metadata("test::src", json!({ "n": 1 }), Some(call_metadata()))
        .await;
    assert_eq!(out.unwrap(), Some(json!({ "n": 1 })));
    let payload = test_support::next(&mut sink).await;
    assert_eq!(payload["metadata"], call_metadata());
    assert_eq!(payload["input"], json!({ "n": 1 }));
    assert_eq!(payload["output"], json!({ "n": 1 }));
    assert_eq!(payload["observer_id"], "obs");
    assert_eq!(payload["status"], "success");

    // A call without metadata: the field is left out, not `null`.
    engine
        .call_with_metadata("test::src", json!({ "n": 2 }), None)
        .await
        .unwrap();
    let payload = test_support::next(&mut sink).await;
    assert!(payload.get("metadata").is_none(), "{payload}");
}

/// Every observer that captures metadata receives the same metadata, the
/// one that shares the copy last included; one that does not capture it
/// receives none.
#[tokio::test]
async fn every_observer_capturing_metadata_receives_identical_metadata() {
    let engine = Arc::new(Engine::new());
    source(&engine);
    let mut sink = test_support::capture(&engine, "test::dst");
    for id in ["obs-1", "obs-2", "obs-3"] {
        observe(
            &engine,
            id,
            "test::dst",
            vec![Capture::Input, Capture::Metadata],
        )
        .await;
    }
    observe(&engine, "obs-plain", "test::dst", vec![Capture::Input]).await;

    engine
        .call_with_metadata("test::src", json!({ "n": 1 }), Some(call_metadata()))
        .await
        .unwrap();
    let mut got = std::collections::BTreeMap::new();
    for _ in 0..4 {
        let payload = test_support::next(&mut sink).await;
        let id = payload["observer_id"].as_str().unwrap().to_string();
        got.insert(id, payload);
    }
    for id in ["obs-1", "obs-2", "obs-3"] {
        assert_eq!(got[id]["metadata"], call_metadata(), "{id}");
        assert_eq!(got[id]["input"], json!({ "n": 1 }), "{id}");
    }
    assert!(got["obs-plain"].get("metadata").is_none());
    assert_eq!(got["obs-plain"]["input"], json!({ "n": 1 }));
}

/// The observed call keeps the caller's metadata by sharing it: `begin`
/// takes one more reference, copies nothing, and the activation lets it
/// go once delivered.
#[tokio::test]
async fn the_observed_call_shares_the_metadata_it_keeps() {
    let engine = Arc::new(Engine::new());
    source(&engine);
    let mut sink = test_support::capture(&engine, "test::dst");
    observe(
        &engine,
        "obs",
        "test::dst",
        vec![Capture::Input, Capture::Metadata],
    )
    .await;
    let registry = engine.observers.clone();
    let set = registry.watch(DEFAULT_NAMESPACE, "test::src").unwrap();

    let metadata = Arc::new(call_metadata());
    let call = registry.begin(
        set,
        &json!({ "n": 1 }),
        &Some(metadata.clone()),
        &None,
        &None,
    );
    assert_eq!(Arc::strong_count(&metadata), 2, "shared, not copied");
    registry.complete(
        call,
        DEFAULT_NAMESPACE,
        "test::src",
        Uuid::new_v4(),
        &Ok(Ok(Some(json!(true)))),
        None,
        &Arc::new(InvocationCtx::engine_internal_at(0)),
    );
    let payload = test_support::next(&mut sink).await;
    assert_eq!(payload["metadata"], *metadata);
    assert_eq!(Arc::strong_count(&metadata), 1, "released once delivered");
}

/// The last holder of an event moves its metadata into the envelope: the
/// target gets the very allocation the call carried, not a copy.
#[tokio::test]
async fn the_last_activation_moves_the_metadata_into_its_envelope() {
    let engine = Arc::new(Engine::new());
    source(&engine);
    let _sink = test_support::capture(&engine, "test::dst");
    observe(
        &engine,
        "obs",
        "test::dst",
        vec![Capture::Input, Capture::Metadata],
    )
    .await;
    let observer = engine.observers.get("obs").expect("observer");

    let metadata = call_metadata();
    let original = blob_ptr(&metadata);
    let mut event = Arc::new(completed(Some(Arc::new(metadata))));
    let envelope = super::project_owned(&observer, &mut event, "a");
    assert_eq!(envelope["metadata"], call_metadata());
    assert_eq!(
        blob_ptr(&envelope["metadata"]),
        original,
        "moved, not copied"
    );
    assert_eq!(envelope["input"], json!({ "n": 1 }));
    assert_eq!(event.function.1, "test::src", "the rest of the event stays");
}

/// While another observer still holds the event, its envelope gets a copy
/// and the event keeps its metadata for the others.
#[tokio::test]
async fn a_shared_event_keeps_its_metadata_for_the_other_observers() {
    let engine = Arc::new(Engine::new());
    source(&engine);
    let _sink = test_support::capture(&engine, "test::dst");
    observe(
        &engine,
        "obs",
        "test::dst",
        vec![Capture::Input, Capture::Metadata],
    )
    .await;
    let observer = engine.observers.get("obs").expect("observer");

    let metadata = Arc::new(call_metadata());
    let mut event = Arc::new(completed(Some(metadata.clone())));
    let other = event.clone();
    let first = super::project_owned(&observer, &mut event, "a");
    assert_eq!(first["metadata"], *metadata);
    assert!(
        Arc::ptr_eq(metadata_of(&other).unwrap(), &metadata),
        "the other observers' event still shares the call's metadata"
    );
    drop(other);
    let second = super::project_owned(&observer, &mut event, "b");
    assert_eq!(second, {
        let mut first = first;
        first["activation_id"] = "b".into();
        first
    });
}

/// What an event holds is counted once per value, however many of its
/// parts share it: a report whose output is its input (`output_is_input`)
/// holds one value, not two.
#[test]
fn the_queue_counts_a_shared_value_once() {
    let shared = Arc::new(json!({ "big": "x".repeat(1000) }));
    let mut event = completed(Some(Arc::new(json!({ "for_observer": "o" }))));
    event.input = Some(super::Payload::Value(shared.clone()));
    event.output = Some(super::Payload::Value(shared.clone()));
    let alone = {
        let mut event = completed(metadata_of(&event).cloned());
        event.input = Some(super::Payload::Value(shared.clone()));
        event.output = None;
        event.invocation_id = super::InvocationId::Reported(String::new());
        event.cost()
    };
    event.invocation_id = super::InvocationId::Reported(String::new());
    assert_eq!(event.cost(), alone, "the shared input/output counted once");
}
