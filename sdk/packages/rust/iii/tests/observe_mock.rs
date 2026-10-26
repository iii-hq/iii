//! observe against a fake engine: features, `observe()` replay, local
//! dispatch, the current invocation, reports, hooks, owner delivery and
//! reconnect, asserted on the frames the SDK puts on the socket.
//!
//! No engine needed: the in-process mock plays the engine's part frame by
//! frame.

mod common;

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use async_trait::async_trait;
use serde_json::{Value, json};

use common::mock_engine::{MockEngine, message_type};
use iii_helpers::observability::OtelConfig;
use iii_sdk::protocol::TriggerRequest;
use iii_sdk::runtime::{FunctionRef, WorkerMetadata};
use iii_sdk::{
    Capture, Dispatch, Error, IIIClient, InitOptions, Observe, ObserveHandler, ObserveRejection,
    Observer, ObserverOptions, ObserverState, On, RegisterFunction, Source, TriggerAction,
    WorkerIdentityMode, current_invocation, register_worker, scope,
};

const WAIT: Duration = Duration::from_secs(5);

fn worker(mock: &MockEngine, name: &str) -> IIIClient {
    register_worker(
        mock.url(),
        InitOptions {
            metadata: Some(WorkerMetadata {
                name: name.to_string(),
                ..WorkerMetadata::default()
            }),
            otel: Some(OtelConfig {
                enabled: Some(false),
                ..OtelConfig::default()
            }),
            identity: WorkerIdentityMode::Explicit,
            ..InitOptions::default()
        },
    )
}

fn fixture(name: &str) -> Value {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../fixtures/protocol/observe")
        .join(name);
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// Waits for the first frame `pred` accepts and returns it.
async fn frame(mock: &MockEngine, pred: impl Fn(&Value) -> bool) -> Value {
    let msgs = mock.wait_for(|m| m.iter().any(&pred), WAIT).await;
    msgs.into_iter()
        .find(|m| pred(m))
        .unwrap_or_else(|| panic!("no matching frame within {WAIT:?}"))
}

fn frames(mock: &MockEngine, ty: &str) -> Vec<Value> {
    mock.received_messages()
        .into_iter()
        .filter(|m| message_type(m) == Some(ty))
        .collect()
}

fn is(ty: &'static str) -> impl Fn(&Value) -> bool {
    move |m| message_type(m) == Some(ty)
}

fn registered(id: &'static str) -> impl Fn(&Value) -> bool {
    move |m| message_type(m) == Some("registerfunction") && m["id"] == id
}

async fn until(what: &str, cond: impl Fn() -> bool) {
    let deadline = Instant::now() + WAIT;
    while !cond() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

/// The engine's hello on this connection.
fn hello(mock: &MockEngine, observe: bool) {
    let mut msg = json!({ "type": "workerregistered", "worker_id": "w-1", "reattach_token": "t" });
    if observe {
        msg["features"] = json!(["observe.v1", "local-dispatch.v1"]);
    }
    mock.send_to_client(msg);
}

fn confirm(mock: &MockEngine, id: &str, revision: u64, local_dispatch: bool, observers: Value) {
    mock.send_to_client(json!({
        "type": "functionregistered", "id": id, "namespace": "default",
        "revision": revision, "local_dispatch": local_dispatch, "observers": observers,
    }));
}

fn view(id: &str, filters: Value, capture: Value, on: &str) -> Value {
    json!({ "id": id, "args": {}, "filters": filters, "target": "t::target", "capture": capture, "on": on })
}

/// Registers `id` on a connected worker and lets the engine confirm it.
async fn hosted(
    mock: &MockEngine,
    iii: &IIIClient,
    id: &'static str,
    registration: RegisterFunction,
    observers: Value,
) -> FunctionRef {
    let function = iii.register_function(id, registration);
    frame(mock, registered(id)).await;
    confirm(mock, id, 1, true, observers);
    until("local dispatch", || function.local_dispatch_active()).await;
    function
}

async fn connected(mock: &MockEngine, iii: &IIIClient, observe: bool) {
    frame(mock, |m| {
        m["type"] == "invokefunction" && m["function_id"] == "engine::workers::register"
    })
    .await;
    hello(mock, observe);
    let _ = iii;
}

fn echo() -> RegisterFunction {
    RegisterFunction::new(|input: Value| Ok::<_, Error>(input))
}

fn invocations(mock: &MockEngine) -> usize {
    frames(mock, "invokefunction")
        .iter()
        .filter(|m| m["function_id"] != "engine::workers::register")
        .count()
}

// ───────────────────────────── features ──────────────────────────────────

#[tokio::test]
async fn features_are_declared_on_workers_register() {
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "owner");
    let register = frame(&mock, |m| m["function_id"] == "engine::workers::register").await;
    assert_eq!(
        register["data"]["features"],
        json!(["observe.v1", "local-dispatch.v1", "large-messages.v1"])
    );
    assert!(
        register.get("invocation_id").is_none(),
        "void calls carry no id"
    );
    assert_eq!(register["action"], json!({ "type": "void" }));
    let shape = fixture("invokefunction-workers-register.json");
    let keys = |v: &Value| {
        let mut k: Vec<String> = v.as_object().unwrap().keys().cloned().collect();
        k.sort();
        k
    };
    assert_eq!(keys(&register), keys(&shape));
    iii.shutdown_async().await;
}

/// A worker can tell whether the engine supports observe, and hears when a
/// reconnect lands on one that differs.
#[tokio::test]
async fn the_engines_observe_support_follows_reconnects() {
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "fallback");
    let heard = Arc::new(Mutex::new(Vec::new()));
    let listener = {
        let heard = heard.clone();
        iii.on_engine_supports_observe(move |observe| heard.lock().unwrap().push(observe))
    };
    assert_eq!(iii.engine_supports_observe(), None);
    connected(&mock, &iii, false).await;
    until("an old engine", || {
        iii.engine_supports_observe() == Some(false)
    })
    .await;
    until("told", || heard.lock().unwrap().len() == 1).await;

    mock.clear();
    mock.close_active_connection();
    connected(&mock, &iii, true).await;
    until("an upgraded engine", || {
        iii.engine_supports_observe() == Some(true)
    })
    .await;
    until("told again", || heard.lock().unwrap().len() == 2).await;
    assert_eq!(*heard.lock().unwrap(), vec![false, true]);
    listener.unsubscribe();
    iii.shutdown_async().await;
}

#[tokio::test]
async fn observe_against_an_engine_without_observe_is_unsupported() {
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "observer");
    let early = iii.observe(Source::function("a::b"), "c::d");
    assert_eq!(
        early.state(),
        ObserverState::Pending,
        "pending until the engine says"
    );
    connected(&mock, &iii, false).await;
    until("unsupported", || early.state() == ObserverState::Error).await;
    assert_eq!(early.reason().unwrap().code, "UNSUPPORTED");
    let err = early.ready().await.unwrap_err();
    assert_eq!(err.invocation_error().unwrap().code, "UNSUPPORTED");

    let late = iii.observe(Source::function("a::b"), "c::d");
    assert_eq!(late.state(), ObserverState::Error);
    assert_eq!(late.reason().unwrap().code, "UNSUPPORTED");

    // An owner function registers as a plain function; nothing runs locally.
    let function = iii.register_function(
        "o::plain",
        echo().observe(Observe::owner().handler(Recorder::default())),
    );
    frame(&mock, registered("o::plain")).await;
    assert!(!function.local_dispatch_active());
    assert!(
        frames(&mock, "observe").is_empty(),
        "nothing sent to an old engine"
    );
    iii.shutdown_async().await;
}

// ───────────────────────────── observe() ─────────────────────────────────

#[tokio::test]
async fn observe_frames_match_the_fixtures() {
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "observer");
    connected(&mock, &iii, true).await;

    let minimal = iii.observe_with(
        Source::function("orders::archive"),
        "search::reindex-order",
        ObserverOptions::default().id("d94f3a5b-6c7d-4e8f-a01b-2c3d4e5f6a7b"),
    );
    let auto = iii.observe_with(
        Source::function("payments::charge")
            .filters(json!({ "input.amount": { "$gte": 500 } }))
            .capture([Capture::Input, Capture::Error, Capture::Caller])
            .on(On::Error),
        "alerts::payment-failed",
        ObserverOptions::default()
            .id("c81e2f4a-5b6c-4d7e-9f0a-1b2c3d4e5f60")
            .max_in_flight(4),
    );
    let sent = frame(&mock, |m| m["type"] == "observe" && m["id"] == minimal.id()).await;
    assert_eq!(sent, fixture("observe-minimal.json"));
    let sent = frame(&mock, |m| m["type"] == "observe" && m["id"] == auto.id()).await;
    assert_eq!(sent, fixture("observe-auto.json"));

    // The engine's answers drive the handle.
    mock.send_to_client(json!({
        "type": "observeresult", "id": minimal.id(), "state": "pending",
        "warnings": ["output.total: response_format is open; path not verified"]
    }));
    mock.send_to_client(json!({
        "type": "observerstatechanged", "id": minimal.id(), "state": "active", "fires": 0
    }));
    tokio::time::timeout(WAIT, minimal.ready())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(minimal.warnings().len(), 1);

    // dispose: an `unobserve`, resolved by the engine's `disposed`.
    let disposing = tokio::spawn({
        let minimal = minimal.clone();
        async move { minimal.dispose().await }
    });
    let unobserve = frame(&mock, is("unobserve")).await;
    assert_eq!(
        unobserve,
        json!({ "type": "unobserve", "id": minimal.id() })
    );
    mock.send_to_client(json!({
        "type": "observerstatechanged", "id": minimal.id(), "state": "disposed",
        "reason": { "code": "UNOBSERVED", "message": "unobserved" }, "fires": 0
    }));
    tokio::time::timeout(WAIT, disposing)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(minimal.state(), ObserverState::Disposed);
    let err = minimal.ready().await.unwrap_err();
    assert_eq!(err.invocation_error().unwrap().code, "UNOBSERVED");
    iii.shutdown_async().await;
}

#[tokio::test]
async fn observers_are_replayed_with_the_same_id_after_functions() {
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "observer");
    iii.register_function("t::target", echo());
    let handle = iii.observe(Source::function("a::b"), "t::target");
    frame(&mock, registered("t::target")).await;
    assert!(
        frames(&mock, "observe").is_empty(),
        "held until the engine's features"
    );
    hello(&mock, true);
    frame(&mock, |m| m["type"] == "observe" && m["id"] == handle.id()).await;

    mock.clear();
    mock.close_active_connection();
    frame(&mock, registered("t::target")).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        frames(&mock, "observe").is_empty(),
        "replayed only after the new hello"
    );
    hello(&mock, true);
    frame(&mock, |m| m["type"] == "observe" && m["id"] == handle.id()).await;
    let msgs = mock.received_messages();
    let function_at = msgs.iter().position(registered("t::target")).unwrap();
    let observe_at = msgs.iter().position(is("observe")).unwrap();
    assert!(function_at < observe_at, "functions first: {msgs:#?}");
    assert_eq!(
        frames(&mock, "observe").len(),
        1,
        "one replay per connection"
    );
    iii.shutdown_async().await;
}

// ───────────────────────────── local dispatch ────────────────────────────

#[tokio::test]
async fn local_dispatch_needs_every_condition() {
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "owner");
    connected(&mock, &iii, true).await;
    let calls = Arc::new(AtomicUsize::new(0));
    let counting = |calls: &Arc<AtomicUsize>| {
        let calls = calls.clone();
        RegisterFunction::new(move |input: Value| {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok::<_, Error>(input)
        })
    };

    // Not confirmed yet: remote.
    let function = iii.register_function("l::f", counting(&calls));
    frame(&mock, registered("l::f")).await;
    let void = |dispatch: Dispatch| {
        TriggerRequest {
            function_id: "l::f".into(),
            payload: json!({ "n": 1 }),
            action: Some(TriggerAction::Void),
            timeout_ms: None,
        }
        .dispatch(dispatch)
    };
    iii.trigger(void(Dispatch::Auto)).await.unwrap();
    frame(&mock, |m| m["function_id"] == "l::f").await;
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    // Confirmed with local_dispatch: false (an RBAC middleware session): remote.
    confirm(&mock, "l::f", 1, false, json!([]));
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!function.local_dispatch_active());

    // Confirmed with local_dispatch: true: local, nothing on the wire.
    confirm(&mock, "l::f", 1, true, json!([]));
    until("local dispatch", || function.local_dispatch_active()).await;
    mock.clear();
    let out = iii
        .trigger(TriggerRequest::new("l::f", json!({ "n": 2 })))
        .await
        .unwrap();
    assert_eq!(out, json!({ "n": 2 }));
    iii.trigger(void(Dispatch::Auto)).await.unwrap();
    until("void local call", || calls.load(Ordering::SeqCst) == 2).await;
    assert_eq!(invocations(&mock), 0);

    // Dispatch::Remote, an enqueue and another namespace go through the engine.
    iii.trigger(void(Dispatch::Remote)).await.unwrap();
    let enqueue = TriggerRequest {
        function_id: "l::f".into(),
        payload: json!({}),
        action: Some(TriggerAction::Enqueue { queue: "q".into() }),
        timeout_ms: Some(50),
    };
    let _ = iii.trigger(enqueue).await;
    iii.trigger(void(Dispatch::Auto).namespace("other"))
        .await
        .unwrap();
    mock.wait_for(|m| m.len() >= 3, WAIT).await;
    assert_eq!(invocations(&mock), 3);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let sent = frames(&mock, "invokefunction");
    assert_eq!(sent[1]["action"]["type"], "enqueue");
    assert_eq!(sent[2]["namespace"], "other");

    // Opted out, and an HTTP function: remote even when confirmed.
    let opted_out = iii.register_function("l::out", echo().local_dispatch(false));
    frame(&mock, registered("l::out")).await;
    confirm(&mock, "l::out", 1, true, json!([]));
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!opted_out.local_dispatch_active());

    // A FUNCTION_NAMESPACE_CONFLICT turns it off again.
    mock.send_to_client(json!({
        "type": "registrationrejected", "code": "FUNCTION_NAMESPACE_CONFLICT",
        "namespace": "default", "function_id": "l::f", "owner_worker_id": "w-other"
    }));
    until("conflict", || !function.local_dispatch_active()).await;
    mock.clear();
    iii.trigger(void(Dispatch::Auto)).await.unwrap();
    frame(&mock, |m| m["function_id"] == "l::f").await;
    iii.shutdown_async().await;
}

/// Local calls the same way as the zero-wire loop: the mock reads on other
/// threads, and the loop yields so nothing it sends can go unseen.
async fn local_calls(iii: &IIIClient, function_id: &str, n: u64) {
    for i in 0..n {
        let out = iii
            .trigger(TriggerRequest::new(function_id, json!(i)))
            .await
            .unwrap();
        assert_eq!(out, json!(i));
        if i % 1_000 == 0 {
            tokio::task::yield_now().await;
        }
    }
    // let the socket and the mock catch up before counting
    tokio::time::sleep(Duration::from_millis(200)).await;
}

fn sent_except_stats(mock: &MockEngine) -> Vec<Value> {
    // The periodic `localinvocationstats` (one per 10 s window, whatever the
    // call count) is the only frame allowed: no invocation, no report.
    mock.received_messages()
        .into_iter()
        .filter(|m| message_type(m) != Some("localinvocationstats"))
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_million_unobserved_local_calls_send_no_frame() {
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "owner");
    connected(&mock, &iii, true).await;
    let quiet = hosted(&mock, &iii, "z::f", echo(), json!([])).await;
    let observed = hosted(
        &mock,
        &iii,
        "z::observed",
        echo(),
        json!([view("all", json!({}), json!(["input"]), "success")]),
    )
    .await;
    until("the observer", || observed.is_observed()).await;

    // Positive control: the same harness sees what an observed function sends.
    mock.clear();
    local_calls(&iii, "z::observed", 10_000).await;
    let reported: usize = sent_except_stats(&mock)
        .iter()
        .filter(|m| message_type(m) == Some("invocationsobserved"))
        .map(|m| m["items"].as_array().unwrap().len())
        .sum();
    assert_eq!(reported, 10_000, "the harness must see report frames");

    mock.clear();
    let started = Instant::now();
    local_calls(&iii, "z::f", 1_000_000).await;
    let elapsed = started.elapsed();
    assert!(!quiet.is_observed());
    let sent = sent_except_stats(&mock);
    assert!(sent.is_empty(), "{} frames: {:?}", sent.len(), sent.first());
    eprintln!("1M local calls in {elapsed:?}");
    iii.shutdown_async().await;
}

#[tokio::test]
async fn the_handler_gets_its_own_value() {
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "owner");
    connected(&mock, &iii, true).await;
    let kept: Arc<Mutex<Option<Value>>> = Arc::default();
    let registration = RegisterFunction::new({
        let kept = kept.clone();
        move |mut input: Value| {
            input["touched"] = json!(true);
            *kept.lock().unwrap() = Some(input.clone());
            Ok::<_, Error>(input)
        }
    });
    hosted(&mock, &iii, "v::f", registration, json!([])).await;

    let original = json!({ "n": 1 });
    let mut out = iii
        .trigger(TriggerRequest::new("v::f", original.clone()))
        .await
        .unwrap();
    assert_eq!(
        original,
        json!({ "n": 1 }),
        "the caller's value is untouched"
    );
    out["mutated_by_caller"] = json!(true);
    assert_eq!(
        kept.lock().unwrap().clone().unwrap(),
        json!({ "n": 1, "touched": true }),
        "the handler's value is untouched by the caller"
    );
    iii.shutdown_async().await;
}

#[tokio::test]
async fn local_calls_keep_the_remote_error_shape_and_timeout() {
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "owner");
    connected(&mock, &iii, true).await;
    hosted(
        &mock,
        &iii,
        "e::fail",
        RegisterFunction::new(|_: Value| Err::<Value, _>(Error::Handler("boom".into()))),
        json!([]),
    )
    .await;
    hosted(
        &mock,
        &iii,
        "e::coded",
        RegisterFunction::new(|_: Value| {
            Err::<Value, _>(Error::Remote {
                code: "OUT_OF_STOCK".into(),
                message: "none left".into(),
                stacktrace: None,
            })
        }),
        json!([]),
    )
    .await;
    let finished = Arc::new(AtomicUsize::new(0));
    hosted(
        &mock,
        &iii,
        "e::slow",
        RegisterFunction::new_async({
            let finished = finished.clone();
            move |_: Value| {
                let finished = finished.clone();
                async move {
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    finished.fetch_add(1, Ordering::SeqCst);
                    Ok::<_, Error>(json!("late"))
                }
            }
        }),
        json!([]),
    )
    .await;
    hosted(
        &mock,
        &iii,
        "e::panics",
        RegisterFunction::new(|_: Value| -> Result<Value, Error> { panic!("kaboom") }),
        json!([]),
    )
    .await;

    let err = iii
        .trigger(TriggerRequest::new("e::fail", json!({})))
        .await
        .unwrap_err();
    let inv = err.invocation_error().expect("a remote-shaped error");
    assert_eq!(inv.code, "invocation_failed");
    assert_eq!(inv.message, "handler error: boom");
    assert!(inv.stacktrace.is_some());

    let err = iii
        .trigger(TriggerRequest::new("e::coded", json!({})))
        .await
        .unwrap_err();
    let inv = err.invocation_error().unwrap();
    assert_eq!((inv.code.as_str(), inv.stacktrace), ("OUT_OF_STOCK", None));

    let slow = TriggerRequest {
        timeout_ms: Some(50),
        ..TriggerRequest::new("e::slow", json!({}))
    };
    assert!(matches!(iii.trigger(slow).await, Err(Error::Timeout)));
    until("the slow handler to finish", || {
        finished.load(Ordering::SeqCst) == 1
    })
    .await;

    let err = iii
        .trigger(TriggerRequest::new("e::panics", json!({})))
        .await
        .unwrap_err();
    assert!(err.invocation_error().unwrap().message.contains("kaboom"));
    assert_eq!(invocations(&mock), 0);
    iii.shutdown_async().await;
}

// ───────────────────────────── current invocation ────────────────────────

#[tokio::test]
async fn the_current_invocation_and_caller_propagation() {
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "owner");
    connected(&mock, &iii, true).await;
    let seen: Arc<Mutex<Vec<iii_sdk::InvocationCtx>>> = Arc::default();
    let inner = RegisterFunction::new({
        let seen = seen.clone();
        move |input: Value| {
            seen.lock()
                .unwrap()
                .push(current_invocation().expect("inside a handler"));
            Ok::<_, Error>(input)
        }
    });
    hosted(&mock, &iii, "c::inner", inner, json!([])).await;
    let outer = RegisterFunction::new_async({
        let iii = iii.clone();
        move |input: Value| {
            let iii = iii.clone();
            async move {
                // local nested call: its caller is this handler
                iii.trigger(TriggerRequest::new("c::inner", input.clone()))
                    .await?;
                // remote call from a spawned task, with the context carried
                let ctx = current_invocation();
                tokio::spawn(scope(ctx, {
                    let iii = iii.clone();
                    async move {
                        let _ = iii
                            .trigger(TriggerRequest {
                                function_id: "other::fn".into(),
                                payload: json!({}),
                                action: Some(TriggerAction::Void),
                                timeout_ms: None,
                            })
                            .await;
                    }
                }))
                .await
                .unwrap();
                // a spawned task without scope() has no current invocation
                let lost = tokio::spawn(async { current_invocation() }).await.unwrap();
                assert!(lost.is_none());
                Ok::<_, Error>(input)
            }
        }
    });
    hosted(&mock, &iii, "c::outer", outer, json!([])).await;

    // A remote invocation from the engine, as an observer activation.
    mock.send_to_client(json!({
        "type": "invokefunction", "invocation_id": "11111111-2222-4333-8444-555555555555",
        "function_id": "c::outer", "data": { "x": 1 },
        "caller": { "worker_id": "w-billing", "worker_name": "billing",
                    "function_id": "billing::close", "invocation_id": "inv-b" },
        "observe_depth": 2,
        "activation": { "observer_id": "obs-1", "activation_id": "act-1" }
    }));
    let result = frame(&mock, is("invocationresult")).await;
    assert_eq!(
        result["result"],
        json!({ "x": 1 }),
        "an activation is answered like any call"
    );

    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 1);
    let inner_ctx = &seen[0];
    assert_eq!(inner_ctx.function_id, "c::inner");
    assert_eq!(inner_ctx.observe_depth, 2, "depth flows into local calls");
    let caller = inner_ctx.caller.clone().unwrap();
    assert_eq!(caller.worker_id.as_deref(), Some("w-1"));
    assert_eq!(caller.worker_name.as_deref(), Some("owner"));
    assert_eq!(caller.function_id.as_deref(), Some("c::outer"));
    assert_eq!(
        caller.invocation_id.as_deref(),
        Some("11111111-2222-4333-8444-555555555555")
    );
    let parent = caller.parent.unwrap();
    assert_eq!(parent.worker_name.as_deref(), Some("billing"));
    assert_eq!(parent.function_id.as_deref(), Some("billing::close"));

    let remote = frame(&mock, |m| m["function_id"] == "other::fn").await;
    assert_eq!(
        remote["caller"],
        json!({
            "function_id": "c::outer",
            "invocation_id": "11111111-2222-4333-8444-555555555555",
            "parent": { "worker_id": "w-billing", "worker_name": "billing",
                        "function_id": "billing::close", "invocation_id": "inv-b" }
        })
    );
    assert_eq!(remote["observe_depth"], 2);
    iii.shutdown_async().await;
}

#[tokio::test]
async fn a_top_level_call_has_no_caller_and_no_depth() {
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "owner");
    connected(&mock, &iii, true).await;
    iii.trigger(TriggerRequest {
        function_id: "x::y".into(),
        payload: json!({}),
        action: Some(TriggerAction::Void),
        timeout_ms: None,
    })
    .await
    .unwrap();
    let sent = frame(&mock, |m| m["function_id"] == "x::y").await;
    assert!(sent.get("caller").is_none());
    assert!(sent.get("observe_depth").is_none());
    assert!(sent.get("activation").is_none());
    iii.shutdown_async().await;
}

// ───────────────────────────── auto reports ──────────────────────────────

#[tokio::test]
async fn auto_functions_report_only_matching_local_calls() {
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "owner");
    connected(&mock, &iii, true).await;
    let charge = RegisterFunction::new(|input: Value| {
        if input["fail"] == json!(true) {
            Err(Error::Remote {
                code: "CARD_DECLINED".into(),
                message: "declined".into(),
                stacktrace: None,
            })
        } else {
            Ok(json!({ "ok": true }))
        }
    });
    let observers = json!([view(
        "obs-err",
        json!({ "$and": [{ "input.amount": { "$gte": 500 } }] }),
        json!(["input", "error", "caller"]),
        "error"
    )]);
    let function = hosted(&mock, &iii, "p::charge", charge, observers).await;
    let ack = frame(&mock, is("observerack")).await;
    assert_eq!(
        ack,
        json!({ "type": "observerack", "function_id": "p::charge", "namespace": "default",
                "id": "obs-err", "revision": 1 })
    );
    assert!(function.is_observed());
    assert_eq!(function.observers()[0].id(), "obs-err");

    mock.clear();
    let call = |input: Value| iii.trigger(TriggerRequest::new("p::charge", input));
    call(json!({ "amount": 900 })).await.unwrap(); // success: on error only
    let _ = call(json!({ "amount": 100, "fail": true })).await; // filter miss
    let err = call(json!({ "amount": 1250, "fail": true }))
        .await
        .unwrap_err();
    assert_eq!(err.invocation_error().unwrap().code, "CARD_DECLINED");

    let report = frame(&mock, is("invocationsobserved")).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(frames(&mock, "invocationsobserved").len(), 1);
    let items = report["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);
    let item = &items[0];
    assert_eq!(item["function_id"], "p::charge");
    assert_eq!(item["namespace"], "default");
    assert_eq!(item["matched"], json!(["obs-err"]));
    assert_eq!(item["status"], "error");
    assert_eq!(item["input"], json!({ "amount": 1250, "fail": true }));
    assert_eq!(
        item["error"],
        json!({ "code": "CARD_DECLINED", "message": "declined" })
    );
    assert_eq!(item["caller"]["worker_name"], "owner");
    assert!(item.get("output").is_none(), "output is not captured");
    assert_eq!(item["observe_depth"], 0);
    assert!(item["duration_ms"].as_f64().unwrap() >= 0.0);
    assert!(item["started_at_ms"].as_u64().unwrap() > 1_700_000_000_000);
    assert_eq!(invocations(&mock), 0, "the calls themselves stay local");
    iii.shutdown_async().await;
}

#[tokio::test]
async fn reports_are_batched_by_256_and_by_linger() {
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "owner");
    connected(&mock, &iii, true).await;
    hosted(
        &mock,
        &iii,
        "b::f",
        echo(),
        json!([view("all", json!({}), json!(["input"]), "success")]),
    )
    .await;
    frame(&mock, is("observerack")).await;
    mock.clear();
    for i in 0..300 {
        iii.trigger(TriggerRequest::new("b::f", json!(i)))
            .await
            .unwrap();
    }
    let msgs = mock
        .wait_for(
            |m| {
                m.iter()
                    .filter(|f| message_type(f) == Some("invocationsobserved"))
                    .map(|f| f["items"].as_array().unwrap().len())
                    .sum::<usize>()
                    == 300
            },
            WAIT,
        )
        .await;
    let sizes: Vec<usize> = msgs
        .iter()
        .filter(|f| message_type(f) == Some("invocationsobserved"))
        .map(|f| f["items"].as_array().unwrap().len())
        .collect();
    assert_eq!(sizes.iter().sum::<usize>(), 300);
    assert!(sizes.iter().all(|n| *n <= 256), "{sizes:?}");
    assert!(
        sizes.len() <= 4,
        "batched, not one frame per call: {sizes:?}"
    );
    let inputs: Vec<Value> = msgs
        .iter()
        .filter(|f| message_type(f) == Some("invocationsobserved"))
        .flat_map(|f| f["items"].as_array().unwrap().clone())
        .map(|i| i["input"].clone())
        .collect();
    assert_eq!(
        inputs,
        (0..300).map(|i| json!(i)).collect::<Vec<_>>(),
        "in order"
    );
    iii.shutdown_async().await;
}

/// After the ack (the engine marks the observer active), every local call is
/// reported: the observer was compiled in before the ack was sent.
#[tokio::test]
async fn every_local_call_after_the_ack_is_reported() {
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "owner");
    connected(&mock, &iii, true).await;
    let function = hosted(&mock, &iii, "r::f", echo(), json!([])).await;
    mock.send_to_client(json!({
        "type": "observeradded", "function_id": "r::f", "namespace": "default", "revision": 2,
        "observer": view("late", json!({}), json!(["input"]), "success")
    }));
    // The engine turns the observer active when this ack arrives.
    frame(&mock, is("observerack")).await;
    assert!(function.is_observed());
    for i in 0..1_000 {
        iii.trigger(TriggerRequest::new("r::f", json!(i)))
            .await
            .unwrap();
    }
    let total = |m: &[Value]| -> usize {
        m.iter()
            .filter(|f| message_type(f) == Some("invocationsobserved"))
            .map(|f| f["items"].as_array().unwrap().len())
            .sum()
    };
    let msgs = mock.wait_for(|m| total(m) >= 1_000, WAIT).await;
    assert_eq!(total(&msgs), 1_000);
    iii.shutdown_async().await;
}

// ───────────────────────────── hooks ─────────────────────────────────────

#[derive(Clone, Default)]
struct Recorder {
    events: Arc<Mutex<Vec<String>>>,
    observers: Arc<Mutex<Vec<Observer>>>,
}

#[async_trait]
impl ObserveHandler for Recorder {
    async fn on_observer_added(&self, obs: &Observer) -> Result<(), ObserveRejection> {
        if let Some(delay) = obs.args_value().get("delay_ms").and_then(Value::as_u64) {
            tokio::time::sleep(Duration::from_millis(delay)).await;
        }
        if let Some(code) = obs.args_value().get("reject").and_then(Value::as_str) {
            self.events
                .lock()
                .unwrap()
                .push(format!("reject {}", obs.id()));
            return Err(ObserveRejection::new(code, "invalid day-of-week field"));
        }
        if obs.args_value().get("plain_error").is_some() {
            return Err(Error::Handler("no".into()).into());
        }
        self.events
            .lock()
            .unwrap()
            .push(format!("add {}", obs.id()));
        self.observers.lock().unwrap().push(obs.clone());
        Ok(())
    }

    async fn on_observer_removed(&self, obs: &Observer) {
        self.events
            .lock()
            .unwrap()
            .push(format!("remove {}", obs.id()));
        self.observers.lock().unwrap().retain(|o| o != obs);
    }
}

fn added(function_id: &str, revision: u64, id: &str, args: Value) -> Value {
    json!({
        "type": "observeradded", "function_id": function_id, "namespace": "default",
        "revision": revision,
        "observer": { "id": id, "args": args, "filters": {}, "target": "t::target",
                      "capture": ["input"], "on": "success" }
    })
}

fn acks(mock: &MockEngine) -> Vec<Value> {
    frames(mock, "observerack")
}

/// Waits until the SDK acked each `(observer id, revision)`.
async fn acked(mock: &MockEngine, expected: &[(&str, u64)]) -> Vec<Value> {
    acked_where(mock, None, expected).await
}

/// [`acked`], for the observers of one function.
async fn acked_for(mock: &MockEngine, function_id: &str, expected: &[(&str, u64)]) -> Vec<Value> {
    acked_where(mock, Some(function_id), expected).await
}

async fn acked_where(
    mock: &MockEngine,
    function_id: Option<&str>,
    expected: &[(&str, u64)],
) -> Vec<Value> {
    let has = |msgs: &[Value], id: &str, revision: u64| {
        msgs.iter().any(|m| {
            message_type(m) == Some("observerack")
                && m["id"] == id
                && m["revision"] == revision
                && function_id.is_none_or(|f| m["function_id"] == f)
        })
    };
    let msgs = mock
        .wait_for(|m| expected.iter().all(|(id, rev)| has(m, id, *rev)), WAIT)
        .await;
    for (id, rev) in expected {
        assert!(
            has(&msgs, id, *rev),
            "no ack of {id} at revision {rev}: {msgs:#?}"
        );
    }
    msgs.into_iter()
        .filter(|m| message_type(m) == Some("observerack"))
        .collect()
}

#[tokio::test]
async fn hooks_ack_with_the_fixture_shapes() {
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "cron");
    connected(&mock, &iii, true).await;
    let recorder = Recorder::default();
    hosted(
        &mock,
        &iii,
        "cron::on-tick",
        echo().observe(Observe::owner().handler(recorder.clone())),
        json!([]),
    )
    .await;

    mock.send_to_client(fixture("observeradded.json"));
    let ack = frame(&mock, is("observerack")).await;
    assert_eq!(ack, fixture("observerack.json"));
    let obs = recorder.observers.lock().unwrap()[0].clone();
    assert_eq!(
        obs.args::<Value>().unwrap()["expression"],
        "0 0 9 * * Mon-Fri"
    );
    assert_eq!(obs.target(), "billing::send-reminders");
    assert_eq!(obs.capture(), &[Capture::Input]);
    assert!(obs.legacy().is_none());

    // removed, then re-added with a rejection: the error ack fixture
    mock.send_to_client(json!({
        "type": "observerremoved", "function_id": "cron::on-tick", "namespace": "default",
        "revision": 14, "id": obs.id()
    }));
    until("removed", || recorder.observers.lock().unwrap().is_empty()).await;
    mock.clear();
    let mut rejected = fixture("observeradded.json");
    rejected["revision"] = json!(13 + 2);
    rejected["observer"]["args"] = json!({ "reject": "INVALID_CRON_EXPRESSION" });
    mock.send_to_client(rejected);
    let ack = frame(&mock, is("observerack")).await;
    let mut expected = fixture("observerack-error.json");
    expected["revision"] = json!(15);
    assert_eq!(ack, expected);
    iii.shutdown_async().await;
}

#[tokio::test]
async fn hooks_run_one_at_a_time_in_revision_order() {
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "owner");
    connected(&mock, &iii, true).await;
    let recorder = Recorder::default();
    let function = hosted(
        &mock,
        &iii,
        "h::f",
        echo().observe(Observe::owner().handler(recorder.clone())),
        json!([]),
    )
    .await;

    mock.send_to_client(added("h::f", 2, "slow", json!({ "delay_ms": 150 })));
    mock.send_to_client(added("h::f", 3, "fast", json!({})));
    mock.send_to_client(json!({
        "type": "observerremoved", "function_id": "h::f", "revision": 4, "id": "slow"
    }));
    // stale: not greater than the revision held
    mock.send_to_client(added("h::f", 3, "stale", json!({})));
    mock.send_to_client(json!({
        "type": "observerremoved", "function_id": "h::f", "revision": 2, "id": "fast"
    }));
    // a plain error is OBSERVE_REJECTED
    mock.send_to_client(added("h::f", 5, "plain", json!({ "plain_error": true })));
    let acks = acked(&mock, &[("slow", 2), ("fast", 3), ("plain", 5)]).await;
    assert_eq!(acks[0]["id"], "slow");
    assert_eq!(acks[0]["revision"], 2);
    assert_eq!(acks[1]["id"], "fast");
    assert_eq!(acks[2]["id"], "plain");
    assert_eq!(acks[2]["error"]["code"], "OBSERVE_REJECTED");
    assert_eq!(
        *recorder.events.lock().unwrap(),
        vec!["add slow", "add fast", "remove slow"]
    );
    let ids: Vec<String> = function
        .observers()
        .iter()
        .map(|o| o.id().to_string())
        .collect();
    assert_eq!(ids, vec!["fast"]);
    iii.shutdown_async().await;
}

#[tokio::test]
async fn args_format_on_an_auto_function_is_refused_at_registration() {
    #[derive(serde::Deserialize, schemars::JsonSchema)]
    #[allow(dead_code)]
    struct Args {
        expression: String,
    }
    let result = std::panic::catch_unwind(|| {
        let iii = IIIClient::new("ws://127.0.0.1:1");
        iii.register_function(
            "bad::auto",
            echo().observe(Observe::auto().args_format::<Args>()),
        );
    });
    let panic = result.unwrap_err();
    let message = panic.downcast_ref::<String>().cloned().unwrap_or_default();
    assert!(
        message.starts_with("INVALID_OBSERVE_DESCRIPTOR"),
        "{message}"
    );

    // owner delivery takes it, as a JSON Schema from the type
    let iii = IIIClient::new("ws://127.0.0.1:1");
    iii.register_function(
        "good::owner",
        echo().observe(Observe::owner().args_format::<Args>()),
    );
}

// ───────────────────────────── reconnect ─────────────────────────────────

#[tokio::test]
async fn reconnect_diffs_the_set_and_keeps_serving_locally() {
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "owner");
    connected(&mock, &iii, true).await;
    let recorder = Recorder::default();
    let function = hosted(
        &mock,
        &iii,
        "d::f",
        echo().observe(Observe::auto().handler(recorder.clone())),
        json!([
            view("a", json!({}), json!(["input"]), "success"),
            view("b", json!({}), json!(["input"]), "success")
        ]),
    )
    .await;
    acked(&mock, &[("a", 1), ("b", 1)]).await;
    assert_eq!(*recorder.events.lock().unwrap(), vec!["add a", "add b"]);

    // Disconnected: local dispatch keeps serving and reports wait.
    mock.close_active_connection();
    until("disconnect", || {
        iii.get_connection_state() != iii_sdk::runtime::IIIConnectionState::Connected
    })
    .await;
    assert!(function.local_dispatch_active());
    let out = iii
        .trigger(TriggerRequest::new("d::f", json!("offline")))
        .await
        .unwrap();
    assert_eq!(out, json!("offline"));

    mock.clear();
    frame(&mock, registered("d::f")).await;
    hello(&mock, true);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        frames(&mock, "invocationsobserved").is_empty(),
        "reports wait for the function's confirmation on the new connection"
    );
    // The engine restarted: revisions start over and the set changed.
    confirm(
        &mock,
        "d::f",
        1,
        true,
        json!([
            view("b", json!({}), json!(["input"]), "success"),
            view("c", json!({}), json!(["input"]), "success")
        ]),
    );
    let report = frame(&mock, is("invocationsobserved")).await;
    let item = &report["items"][0];
    assert_eq!(item["input"], "offline");
    assert_eq!(item["matched"], json!(["a", "b"]));
    acked(&mock, &[("b", 1), ("c", 1)]).await;
    assert_eq!(
        *recorder.events.lock().unwrap(),
        vec!["add a", "add b", "remove a", "add c"]
    );
    let acked: Vec<Value> = acks(&mock).iter().map(|a| a["id"].clone()).collect();
    assert_eq!(
        acked,
        vec![json!("b"), json!("c")],
        "kept ones are acked again"
    );
    iii.shutdown_async().await;
}

// ───────────────────────────── owner delivery ────────────────────────────

/// Owner function `src::events` with three observers; the test's own index
/// picks who gets what.
async fn owner_setup(mock: &MockEngine, iii: &IIIClient) -> (FunctionRef, Recorder) {
    let recorder = Recorder::default();
    let function = hosted(
        mock,
        iii,
        "src::events",
        RegisterFunction::new_async({
            let recorder = recorder.clone();
            move |event: Value| {
                let observers = recorder.observers.lock().unwrap().clone();
                async move {
                    let mut sent = 0;
                    for obs in observers {
                        if obs.emit(&event)? {
                            sent += 1;
                        }
                    }
                    Ok::<_, Error>(json!(sent))
                }
            }
        })
        .observe(Observe::owner().handler(recorder.clone())),
        json!([
            {
                "id": "only-x1", "args": {}, "target": "t::one",
                "filters": { "$and": [{ "input.x": { "$eq": 1 } }] },
                "capture": ["input", "caller"], "on": "success"
            },
            { "id": "all-a", "args": {}, "filters": {}, "target": "t::two",
              "capture": ["input"], "on": "success" },
            { "id": "all-b", "args": {}, "filters": {}, "target": "t::three",
              "capture": ["input", "caller", "metadata"], "on": "success" }
        ]),
    )
    .await;
    acked(mock, &[("only-x1", 1), ("all-a", 1), ("all-b", 1)]).await;
    (function, recorder)
}

fn observer(recorder: &Recorder, id: &str) -> Observer {
    recorder
        .observers
        .lock()
        .unwrap()
        .iter()
        .find(|o| o.id() == id)
        .cloned()
        .unwrap()
}

#[tokio::test]
async fn emit_applies_filters_and_coalesces_one_event() {
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "database");
    connected(&mock, &iii, true).await;
    let (_function, recorder) = owner_setup(&mock, &iii).await;
    mock.clear();

    // Outside any handler: a filter miss sends nothing.
    assert!(
        !observer(&recorder, "only-x1")
            .emit(&json!({ "x": 2 }))
            .unwrap()
    );
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(frames(&mock, "invocationsobserved").is_empty());

    // Inside the function's own handler (a local call): the event is that
    // invocation, and one event to three observers is one item.
    let sent = iii
        .trigger(TriggerRequest::new("src::events", json!({ "x": 1 })).metadata(json!({ "m": 1 })))
        .await
        .unwrap();
    assert_eq!(sent, json!(3));
    let report = frame(&mock, is("invocationsobserved")).await;
    let items = report["items"].as_array().unwrap();
    assert_eq!(items.len(), 1, "{items:#?}");
    let item = &items[0];
    assert_eq!(item["matched"], json!(["only-x1", "all-a", "all-b"]));
    assert_eq!(item["status"], "success");
    assert_eq!(item["input"], json!({ "x": 1 }));
    assert_eq!(item["metadata"], json!({ "m": 1 }), "captures are unioned");
    let caller = &item["caller"];
    assert_eq!(caller["worker_name"], "database");
    assert!(
        caller.get("function_id").is_none(),
        "a top-level local call"
    );

    // Outside any handler: the caller is this worker only.
    mock.clear();
    assert!(
        observer(&recorder, "all-b")
            .emit(&json!({ "x": 9 }))
            .unwrap()
    );
    let report = frame(&mock, is("invocationsobserved")).await;
    assert_eq!(
        report["items"][0]["caller"],
        json!({ "worker_id": "w-1", "worker_name": "database" })
    );
    assert!(report["items"][0].get("metadata").is_none());

    // Inside another handler: that handler is the caller.
    mock.clear();
    let other = RegisterFunction::new({
        let obs = observer(&recorder, "all-b");
        move |event: Value| Ok::<_, Error>(json!(obs.emit(&event)?))
    });
    hosted(&mock, &iii, "src::other", other, json!([])).await;
    iii.trigger(TriggerRequest::new("src::other", json!({ "x": 3 })))
        .await
        .unwrap();
    let report = frame(&mock, is("invocationsobserved")).await;
    let caller = &report["items"][0]["caller"];
    assert_eq!(caller["function_id"], "src::other");
    assert_eq!(caller["worker_name"], "database");
    assert!(caller["invocation_id"].is_string());
    iii.shutdown_async().await;
}

#[tokio::test]
async fn call_returns_the_targets_result_and_checks_filters() {
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "http");
    connected(&mock, &iii, true).await;
    let (_function, recorder) = owner_setup(&mock, &iii).await;
    mock.clear();

    let miss = observer(&recorder, "only-x1")
        .call::<Value>(json!({ "x": 2 }))
        .await
        .unwrap_err();
    assert_eq!(miss.invocation_error().unwrap().code, "FILTERS_NOT_MATCHED");
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(invocations(&mock), 0, "a filter miss sends nothing");

    let obs = observer(&recorder, "only-x1");
    let pending = tokio::spawn(async move { obs.call::<Value>(json!({ "x": 1 })).await });
    let sent = frame(&mock, |m| m["function_id"] == "t::one").await;
    assert_eq!(sent["activation"], json!({ "observer_id": "only-x1" }));
    assert!(
        sent.get("namespace").is_none(),
        "D13: no namespace on an owner call"
    );
    assert_eq!(sent["data"], json!({ "x": 1 }));
    mock.send_to_client(json!({
        "type": "invocationresult", "invocation_id": sent["invocation_id"],
        "function_id": "t::one", "result": { "status_code": 200 }
    }));
    let result = tokio::time::timeout(WAIT, pending)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(result, json!({ "status_code": 200 }));

    // After removal the observer is not active any more.
    let obs = observer(&recorder, "all-a");
    mock.send_to_client(json!({
        "type": "observerremoved", "function_id": "src::events", "revision": 9, "id": "all-a"
    }));
    until("removed", || !obs.is_active()).await;
    let err = obs.call::<Value>(json!({})).await.unwrap_err();
    assert_eq!(err.invocation_error().unwrap().code, "OBSERVER_NOT_ACTIVE");
    // `emit` on a removed observer is a no-op: an owner's fan-out goes on.
    assert!(!obs.emit(&json!({})).unwrap());

    // A late rejection stops deliveries at once and tells the engine.
    let obs = observer(&recorder, "all-b");
    obs.reject("UNKNOWN_DB", "db `primary` was removed");
    assert!(!obs.is_active());
    let ack = frame(&mock, |m| m["type"] == "observerack" && m["id"] == "all-b").await;
    assert_eq!(ack["error"]["code"], "UNKNOWN_DB");
    assert_eq!(ack["revision"], 1);
    iii.shutdown_async().await;
}

#[tokio::test]
async fn emit_and_call_are_for_owner_functions_only() {
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "owner");
    connected(&mock, &iii, true).await;
    let recorder = Recorder::default();
    hosted(
        &mock,
        &iii,
        "auto::f",
        echo().observe(Observe::auto().handler(recorder.clone())),
        json!([view("o", json!({}), json!(["input"]), "success")]),
    )
    .await;
    until("added", || !recorder.observers.lock().unwrap().is_empty()).await;
    let obs = recorder.observers.lock().unwrap()[0].clone();
    let err = obs.emit(&json!({})).unwrap_err();
    assert_eq!(err.invocation_error().unwrap().code, "NOT_OWNER_DELIVERED");
    let err = obs.call::<Value>(json!({})).await.unwrap_err();
    assert_eq!(err.invocation_error().unwrap().code, "NOT_OWNER_DELIVERED");
    iii.shutdown_async().await;
}

#[tokio::test]
async fn emits_while_disconnected_flush_after_reconnect() {
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "database");
    connected(&mock, &iii, true).await;
    let (_function, recorder) = owner_setup(&mock, &iii).await;
    mock.close_active_connection();
    until("disconnect", || {
        iii.get_connection_state() != iii_sdk::runtime::IIIConnectionState::Connected
    })
    .await;
    assert!(
        observer(&recorder, "all-a")
            .emit(&json!({ "while": "offline" }))
            .unwrap()
    );
    mock.clear();
    frame(&mock, registered("src::events")).await;
    hello(&mock, true);
    confirm(
        &mock,
        "src::events",
        1,
        true,
        json!([{ "id": "all-a", "args": {}, "filters": {}, "target": "t::two",
                 "capture": ["input"], "on": "success" }]),
    );
    let report = frame(&mock, is("invocationsobserved")).await;
    assert_eq!(report["items"][0]["input"], json!({ "while": "offline" }));
    iii.shutdown_async().await;
}

#[tokio::test]
async fn unknown_engine_messages_are_ignored() {
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "owner");
    connected(&mock, &iii, true).await;
    mock.send_to_client(json!({ "type": "fromthefuture", "payload": [1, 2, 3] }));
    let function = hosted(&mock, &iii, "u::f", echo(), json!([])).await;
    assert!(
        function.local_dispatch_active(),
        "the connection kept working"
    );
    iii.shutdown_async().await;
}

#[tokio::test]
async fn sync_handlers_run_inline_on_the_callers_task() {
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "owner");
    connected(&mock, &iii, true).await;
    let thread: Arc<Mutex<Option<std::thread::ThreadId>>> = Arc::default();
    hosted(
        &mock,
        &iii,
        "s::f",
        RegisterFunction::new({
            let thread = thread.clone();
            move |input: Value| {
                *thread.lock().unwrap() = Some(std::thread::current().id());
                Ok::<_, Error>(input)
            }
        }),
        json!([]),
    )
    .await;
    iii.trigger(TriggerRequest::new("s::f", json!(1)))
        .await
        .unwrap();
    assert_eq!(*thread.lock().unwrap(), Some(std::thread::current().id()));
    iii.shutdown_async().await;
}

#[tokio::test]
async fn observe_options_policy_reaches_the_wire() {
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "observer");
    connected(&mock, &iii, true).await;
    let handle = iii.observe_with(
        Source::function("cron::on-tick").args(json!({ "expression": "* * * * * *" })),
        iii_sdk::Target::function("local::h").namespace("default"),
        ObserverOptions::default()
            .max_fires(3)
            .expires_in(Duration::from_secs(60))
            .metadata(json!({ "__binding": "b" })),
    );
    let sent = frame(&mock, |m| m["type"] == "observe" && m["id"] == handle.id()).await;
    assert_eq!(
        sent["source"]["args"],
        json!({ "expression": "* * * * * *" })
    );
    assert_eq!(
        sent["target"],
        json!({ "function_id": "local::h", "namespace": "default" })
    );
    assert_eq!(sent["max_fires"], 3);
    assert_eq!(sent["expires_in_ms"], 60_000);
    assert_eq!(sent["metadata"], json!({ "__binding": "b" }));
    assert!(
        uuid::Uuid::parse_str(handle.id()).is_ok(),
        "a UUID v4 by default"
    );
    iii.shutdown_async().await;
}

// ───────────────────────────── review round 1 ────────────────────────────

fn ack_for<'a>(acks: &'a [Value], id: &str, revision: u64) -> Option<&'a Value> {
    acks.iter()
        .find(|a| a["id"] == id && a["revision"] == revision)
}

/// A late rejection echoes the revision the engine last announced, also
/// after a reconnect moved it; the engine's `observerremoved` then runs the
/// removal hook.
#[tokio::test]
async fn a_late_rejection_echoes_the_latest_announced_revision() {
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "owner");
    connected(&mock, &iii, true).await;
    let recorder = Recorder::default();
    let observers = json!([
        view("a", json!({}), json!(["input"]), "success"),
        view("b", json!({}), json!(["input"]), "success")
    ]);
    let function = hosted(
        &mock,
        &iii,
        "lr::f",
        echo().observe(Observe::owner().handler(recorder.clone())),
        observers.clone(),
    )
    .await;
    acked(&mock, &[("a", 1), ("b", 1)]).await;

    mock.clear();
    mock.close_active_connection();
    frame(&mock, registered("lr::f")).await;
    hello(&mock, true);
    confirm(&mock, "lr::f", 5, true, observers);
    acked(&mock, &[("a", 5), ("b", 5)]).await;
    assert!(
        ack_for(&acks(&mock), "a", 5).is_some(),
        "re-acked at the new revision"
    );

    let a = observer(&recorder, "a");
    a.reject("UNKNOWN_DB", "gone");
    let ack = frame(&mock, |m| {
        m["type"] == "observerack" && m["error"].is_object()
    })
    .await;
    assert_eq!(ack["id"], "a");
    assert_eq!(ack["revision"], 5, "the revision the engine announced last");
    assert_eq!(function.observers().len(), 1, "out of the set at once");
    assert!(
        !a.emit(&json!({})).unwrap(),
        "emit on a rejected observer is a no-op"
    );

    mock.send_to_client(json!({
        "type": "observerremoved", "function_id": "lr::f", "revision": 6, "id": "a"
    }));
    until("on_observer_removed", || {
        recorder
            .events
            .lock()
            .unwrap()
            .contains(&"remove a".to_string())
    })
    .await;
    assert!(observer(&recorder, "b").is_active());
    iii.shutdown_async().await;
}

/// A rejection made while disconnected is sent again when the engine lists
/// the observer after the reconnect, and the observer stays out until a fresh
/// `observeradded`.
#[tokio::test]
async fn a_late_rejection_survives_a_reconnect() {
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "owner");
    connected(&mock, &iii, true).await;
    let recorder = Recorder::default();
    let observers = json!([view("a", json!({}), json!(["input"]), "success")]);
    let function = hosted(
        &mock,
        &iii,
        "ls::f",
        echo().observe(Observe::owner().handler(recorder.clone())),
        observers.clone(),
    )
    .await;
    frame(&mock, is("observerack")).await;

    mock.clear();
    mock.close_active_connection();
    until("disconnect", || {
        iii.get_connection_state() != iii_sdk::runtime::IIIConnectionState::Connected
    })
    .await;
    let a = observer(&recorder, "a");
    a.reject("UNKNOWN_DB", "gone while offline");
    assert!(!function.is_observed());

    frame(&mock, registered("ls::f")).await;
    hello(&mock, true);
    confirm(&mock, "ls::f", 3, true, observers);
    let ack = frame(&mock, |m| {
        m["type"] == "observerack" && m["revision"] == 3 && m["error"].is_object()
    })
    .await;
    assert_eq!(ack["error"]["code"], "UNKNOWN_DB");
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(!function.is_observed(), "not reactivated by the replay");
    assert_eq!(
        *recorder.events.lock().unwrap(),
        vec!["add a"],
        "no hook ran again"
    );

    // A fresh `observeradded` brings it back.
    mock.send_to_client(added("ls::f", 4, "a", json!({})));
    until("re-added", || function.is_observed()).await;
    assert_eq!(
        *recorder.events.lock().unwrap(),
        vec!["add a", "remove a", "add a"]
    );
    iii.shutdown_async().await;
}

/// Emitted events carry the trace context of the invocation they come from.
#[tokio::test]
async fn emits_carry_the_invocations_trace_context() {
    const TRACEPARENT: &str = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "database");
    connected(&mock, &iii, true).await;
    let (_function, recorder) = owner_setup(&mock, &iii).await;
    let seen: Arc<Mutex<Option<String>>> = Arc::default();
    hosted(
        &mock,
        &iii,
        "tc::execute",
        RegisterFunction::new_async({
            let obs = observer(&recorder, "all-a");
            let seen = seen.clone();
            move |event: Value| {
                let obs = obs.clone();
                let seen = seen.clone();
                async move {
                    *seen.lock().unwrap() = current_invocation().and_then(|c| c.traceparent);
                    // from a task spawned with the context, as a database
                    // commit hook would
                    tokio::spawn(scope(current_invocation(), async move { obs.emit(&event) }))
                        .await
                        .unwrap()?;
                    Ok::<_, Error>(Value::Null)
                }
            }
        }),
        json!([]),
    )
    .await;
    mock.clear();
    mock.send_to_client(json!({
        "type": "invokefunction", "invocation_id": "21111111-2222-4333-8444-555555555555",
        "function_id": "tc::execute", "data": { "row": 1 }, "traceparent": TRACEPARENT
    }));
    let report = frame(&mock, is("invocationsobserved")).await;
    let item = &report["items"][0];
    let traceparent = item["traceparent"].as_str().expect("a traceparent");
    assert!(
        traceparent.starts_with("00-4bf92f3577b34da6a3ce929d0e0e4736-"),
        "{traceparent}"
    );
    assert_eq!(seen.lock().unwrap().as_deref(), Some(traceparent));
    iii.shutdown_async().await;
}

/// A function recursing into itself locally goes through the engine past 64
/// nested local calls instead of growing the stack.
#[tokio::test]
async fn deep_local_recursion_falls_back_to_the_engine() {
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "owner");
    connected(&mock, &iii, true).await;
    hosted(
        &mock,
        &iii,
        "rec::f",
        RegisterFunction::new_async({
            let iii = iii.clone();
            move |input: Value| {
                let iii = iii.clone();
                async move {
                    let n = input["n"].as_u64().unwrap_or(0);
                    if n == 0 {
                        return Ok::<_, Error>(json!(0));
                    }
                    let request = TriggerRequest {
                        timeout_ms: Some(300),
                        ..TriggerRequest::new("rec::f", json!({ "n": n - 1 }))
                    };
                    // boxed: the nested call's future is as deep as the call
                    Box::pin(iii.trigger(request)).await
                }
            }
        }),
        json!([]),
    )
    .await;
    mock.clear();
    let out = iii
        .trigger(TriggerRequest::new("rec::f", json!({ "n": 30 })))
        .await
        .unwrap();
    assert_eq!(out, json!(0), "shallow recursion stays local");
    assert_eq!(invocations(&mock), 0);

    let _ = iii
        .trigger(TriggerRequest::new("rec::f", json!({ "n": 100 })))
        .await;
    let sent: Vec<Value> = frames(&mock, "invokefunction")
        .into_iter()
        .filter(|m| m["function_id"] == "rec::f")
        .collect();
    assert_eq!(sent.len(), 1, "{sent:#?}");
    assert_eq!(sent[0]["data"], json!({ "n": 36 }), "the 65th nested call");
    iii.shutdown_async().await;
}

/// Reports of a function another worker took over (`FUNCTION_NAMESPACE_CONFLICT`
/// on the replay) are released, so they do not hold the buffer.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_conflict_releases_the_functions_reports() {
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "owner");
    connected(&mock, &iii, true).await;
    let all = json!([view("all", json!({}), json!(["input"]), "success")]);
    let lost = hosted(&mock, &iii, "cf::lost", echo(), all.clone()).await;
    hosted(&mock, &iii, "cf::kept", echo(), all.clone()).await;
    // Everything of the first connection is recorded before the clear.
    acked_for(&mock, "cf::lost", &[("all", 1)]).await;
    acked_for(&mock, "cf::kept", &[("all", 1)]).await;
    until("observed", || lost.is_observed()).await;

    mock.clear();
    mock.close_active_connection();
    until("disconnect", || {
        iii.get_connection_state() != iii_sdk::runtime::IIIConnectionState::Connected
    })
    .await;
    // fill the whole buffer while offline
    for i in 0..10_000 {
        iii.trigger(TriggerRequest::new("cf::lost", json!(i)))
            .await
            .unwrap();
    }
    frame(&mock, registered("cf::kept")).await;
    hello(&mock, true);
    mock.send_to_client(json!({
        "type": "registrationrejected", "code": "FUNCTION_NAMESPACE_CONFLICT",
        "namespace": "default", "function_id": "cf::lost", "owner_worker_id": "w-other"
    }));
    confirm(&mock, "cf::kept", 2, true, all);
    until("the conflict", || !lost.local_dispatch_active()).await;
    acked_for(&mock, "cf::kept", &[("all", 2)]).await;
    for i in 0..1_000 {
        iii.trigger(TriggerRequest::new("cf::kept", json!(i)))
            .await
            .unwrap();
        if i % 100 == 0 {
            tokio::task::yield_now().await;
        }
    }
    let delivered = |m: &[Value], function_id: &str| -> usize {
        m.iter()
            .filter(|f| message_type(f) == Some("invocationsobserved"))
            .flat_map(|f| f["items"].as_array().unwrap().clone())
            .filter(|i| i["function_id"] == function_id)
            .count()
    };
    let msgs = mock
        .wait_for(|m| delivered(m, "cf::kept") >= 1_000, WAIT)
        .await;
    assert_eq!(delivered(&msgs, "cf::kept"), 1_000, "nothing of it dropped");
    assert_eq!(
        delivered(&msgs, "cf::lost"),
        0,
        "the conflicted reports are gone"
    );
    iii.shutdown_async().await;
}

// ───────────────────────────── review round 2 ────────────────────────────

/// Reports made while disconnected go out as soon as the function is
/// confirmed again, without waiting for a slow `on_observer_added`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reports_do_not_wait_for_slow_hooks() {
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "owner");
    connected(&mock, &iii, true).await;
    let recorder = Recorder::default();
    let a = view("a", json!({}), json!(["input"]), "success");
    hosted(
        &mock,
        &iii,
        "sh::f",
        echo().observe(Observe::auto().handler(recorder.clone())),
        json!([a.clone()]),
    )
    .await;
    acked(&mock, &[("a", 1)]).await;

    mock.clear();
    mock.close_active_connection();
    until("disconnect", || {
        iii.get_connection_state() != iii_sdk::runtime::IIIConnectionState::Connected
    })
    .await;
    iii.trigger(TriggerRequest::new("sh::f", json!("offline")))
        .await
        .unwrap();
    frame(&mock, registered("sh::f")).await;
    hello(&mock, true);
    let mut slow = view("slow", json!({}), json!(["input"]), "success");
    slow["args"] = json!({ "delay_ms": 1500 });
    confirm(&mock, "sh::f", 2, true, json!([a, slow]));
    let msgs = acked(&mock, &[("slow", 2)]).await;
    let _ = msgs;
    let all = mock.received_messages();
    let report_at = all
        .iter()
        .position(is("invocationsobserved"))
        .expect("the report went out");
    let slow_ack_at = all
        .iter()
        .position(|m| message_type(m) == Some("observerack") && m["id"] == "slow")
        .unwrap();
    let a_ack_at = all
        .iter()
        .position(|m| message_type(m) == Some("observerack") && m["id"] == "a")
        .unwrap();
    assert!(a_ack_at < report_at, "behind the re-ack of what it matched");
    assert!(report_at < slow_ack_at, "not behind the slow hook");
    assert_eq!(all[report_at]["items"][0]["input"], "offline");
    iii.shutdown_async().await;
}

/// A hook that holds the observer until the test lets it go.
#[derive(Clone, Default)]
struct HeldHook {
    seen: Arc<Mutex<Option<Observer>>>,
    release: Arc<tokio::sync::Notify>,
}

#[async_trait]
impl ObserveHandler for HeldHook {
    async fn on_observer_added(&self, obs: &Observer) -> Result<(), ObserveRejection> {
        *self.seen.lock().unwrap() = Some(obs.clone());
        self.release.notified().await;
        Ok(())
    }
}

/// A rejection made while `on_observer_added` still runs wins over its success.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_rejection_during_the_hook_wins() {
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "owner");
    connected(&mock, &iii, true).await;
    let hook = HeldHook::default();
    let function = hosted(
        &mock,
        &iii,
        "rh::f",
        echo().observe(Observe::owner().handler(hook.clone())),
        json!([]),
    )
    .await;
    mock.clear();
    mock.send_to_client(added("rh::f", 2, "mid", json!({})));
    until("the hook", || hook.seen.lock().unwrap().is_some()).await;
    let obs = hook.seen.lock().unwrap().clone().unwrap();
    std::thread::spawn(move || obs.reject("NOPE", "rejected mid-hook"))
        .join()
        .unwrap();
    hook.release.notify_one();
    let ack = frame(&mock, is("observerack")).await;
    assert_eq!(ack["id"], "mid");
    assert_eq!(ack["revision"], 2);
    assert_eq!(ack["error"]["code"], "NOPE");
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(acks(&mock).len(), 1, "no success ack for it");
    assert!(!function.is_observed());
    iii.shutdown_async().await;
}

/// Local dispatch serializes nothing: no 64 MiB message limit applies.
/// Remote calls up to that limit are fragmented, so only a larger payload
/// is refused locally.
#[tokio::test]
async fn a_local_call_takes_a_payload_larger_than_a_message() {
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "owner");
    connected(&mock, &iii, true).await;
    hosted(&mock, &iii, "big::f", echo(), json!([])).await;
    let big = "x".repeat(64 * 1024 * 1024 + 1);
    let out = iii
        .trigger(TriggerRequest::new("big::f", json!(big)))
        .await
        .unwrap();
    assert_eq!(out.as_str().map(str::len), Some(big.len()));
    let err = iii
        .trigger(TriggerRequest::new("big::f", json!(big)).dispatch(Dispatch::Remote))
        .await
        .unwrap_err();
    assert_eq!(err.invocation_error().unwrap().code, "payload_too_large");
    iii.shutdown_async().await;
}

/// `dispose` never races a replay into resurrecting the observer: one never
/// sent stays unsent, one disposed offline is unobserved, not replayed.
#[tokio::test]
async fn dispose_wins_over_the_replay() {
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "observer");
    frame(&mock, |m| m["function_id"] == "engine::workers::register").await;
    let never_sent = iii.observe(Source::function("a::b"), "c::d");
    never_sent.dispose().await.unwrap();
    hello(&mock, true);
    let offline = iii.observe(Source::function("a::b"), "c::e");
    frame(&mock, |m| m["type"] == "observe" && m["id"] == offline.id()).await;
    assert!(
        frames(&mock, "observe")
            .iter()
            .all(|m| m["id"] != never_sent.id())
    );

    mock.clear();
    mock.close_active_connection();
    until("disconnect", || {
        iii.get_connection_state() != iii_sdk::runtime::IIIConnectionState::Connected
    })
    .await;
    offline.dispose().await.unwrap();
    frame(&mock, |m| m["function_id"] == "engine::workers::register").await;
    hello(&mock, true);
    frame(&mock, |m| {
        m["type"] == "unobserve" && m["id"] == offline.id()
    })
    .await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        frames(&mock, "observe").is_empty(),
        "a disposed observer is not replayed"
    );
    iii.shutdown_async().await;
}

/// Observe messages queued on a dropped connection are dropped when the next
/// engine does not speak observe.
#[tokio::test]
async fn held_observe_messages_are_dropped_for_an_engine_without_observe() {
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "observer");
    connected(&mock, &iii, true).await;
    let handle = iii.observe(Source::function("a::b"), "c::d");
    frame(&mock, |m| m["type"] == "observe" && m["id"] == handle.id()).await;
    mock.clear();
    mock.close_active_connection();
    until("disconnect", || {
        iii.get_connection_state() != iii_sdk::runtime::IIIConnectionState::Connected
    })
    .await;
    handle.dispose().await.unwrap();
    frame(&mock, |m| m["function_id"] == "engine::workers::register").await;
    hello(&mock, false);
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(frames(&mock, "unobserve").is_empty(), "held, then dropped");
    assert!(frames(&mock, "observe").is_empty());
    iii.shutdown_async().await;
}

/// `caller` and `observe_depth` carry through the task hop every 8 nested
/// local calls.
#[tokio::test]
async fn the_context_survives_the_task_hops_of_deep_local_calls() {
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "owner");
    connected(&mock, &iii, true).await;
    let seen: Arc<Mutex<Vec<iii_sdk::InvocationCtx>>> = Arc::default();
    hosted(
        &mock,
        &iii,
        "deep::f",
        RegisterFunction::new_async({
            let iii = iii.clone();
            let seen = seen.clone();
            move |n: Value| {
                let iii = iii.clone();
                let seen = seen.clone();
                async move {
                    seen.lock().unwrap().push(current_invocation().unwrap());
                    let n = n.as_u64().unwrap();
                    if n == 0 {
                        return Ok::<_, Error>(json!("bottom"));
                    }
                    Box::pin(iii.trigger(TriggerRequest::new("deep::f", json!(n - 1)))).await
                }
            }
        }),
        json!([]),
    )
    .await;
    mock.clear();
    mock.send_to_client(json!({
        "type": "invokefunction", "invocation_id": "31111111-2222-4333-8444-555555555555",
        "function_id": "deep::f", "data": 20, "observe_depth": 3,
        "caller": { "worker_id": "w-x", "worker_name": "x", "function_id": "x::y" }
    }));
    let result = frame(&mock, is("invocationresult")).await;
    assert_eq!(result["result"], "bottom");
    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 21);
    assert!(seen.iter().all(|ctx| ctx.observe_depth == 3), "depth kept");
    for pair in seen.windows(2) {
        let caller = pair[1].caller.clone().unwrap();
        assert_eq!(caller.function_id.as_deref(), Some("deep::f"));
        assert_eq!(
            caller.invocation_id.as_deref(),
            Some(pair[0].invocation_id.as_str()),
            "each level names the one above it"
        );
    }
    assert_eq!(invocations(&mock), 0, "all local");
    iii.shutdown_async().await;
}

// ───────────────────────────── review round 3 ────────────────────────────

/// A rejected observer stays rejected when the replay lists it with a changed
/// view; only a fresh `observeradded` brings it back.
#[tokio::test]
async fn a_rejection_survives_a_changed_view_on_replay() {
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "owner");
    connected(&mock, &iii, true).await;
    let recorder = Recorder::default();
    let function = hosted(
        &mock,
        &iii,
        "cv::f",
        echo().observe(Observe::owner().handler(recorder.clone())),
        json!([view("a", json!({}), json!(["input"]), "success")]),
    )
    .await;
    acked_for(&mock, "cv::f", &[("a", 1)]).await;
    observer(&recorder, "a").reject("GONE", "removed");
    frame(&mock, |m| {
        m["type"] == "observerack" && m["error"]["code"] == "GONE"
    })
    .await;

    mock.clear();
    mock.close_active_connection();
    frame(&mock, registered("cv::f")).await;
    hello(&mock, true);
    let changed = view(
        "a",
        json!({ "$and": [{ "input.n": { "$gt": 1 } }] }),
        json!(["input", "caller"]),
        "success",
    );
    confirm(&mock, "cv::f", 3, true, json!([changed]));
    let acks = acked_for(&mock, "cv::f", &[("a", 3)]).await;
    assert!(
        acks.iter().all(|a| a["error"]["code"] == "GONE"),
        "re-acked with its rejection: {acks:#?}"
    );
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(!function.is_observed(), "not reactivated");
    assert_eq!(
        *recorder.events.lock().unwrap(),
        vec!["add a"],
        "no hook ran"
    );

    mock.send_to_client(added("cv::f", 4, "a", json!({})));
    until("re-added", || function.is_observed()).await;
    assert_eq!(
        *recorder.events.lock().unwrap(),
        vec!["add a", "remove a", "add a"]
    );
    iii.shutdown_async().await;
}

// ───────────────────────────── review round 4 ────────────────────────────

/// A hook that holds the observer until released, and records removals.
#[derive(Clone, Default)]
struct HeldAndRemoved {
    held: HeldHook,
    removed: Arc<Mutex<Vec<String>>>,
}

#[async_trait]
impl ObserveHandler for HeldAndRemoved {
    async fn on_observer_added(&self, obs: &Observer) -> Result<(), ObserveRejection> {
        self.held.on_observer_added(obs).await
    }

    async fn on_observer_removed(&self, obs: &Observer) {
        self.removed.lock().unwrap().push(obs.id().to_string());
    }
}

/// A `FUNCTION_NAMESPACE_CONFLICT` while `on_observer_added` runs: the add is
/// undone when the callback returns (no success ack, never active, removal
/// hook run).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_conflict_during_an_add_hook_undoes_the_add() {
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "owner");
    connected(&mock, &iii, true).await;
    let hook = HeldAndRemoved::default();
    let function = hosted(
        &mock,
        &iii,
        "ca::f",
        echo().observe(Observe::owner().handler(hook.clone())),
        json!([]),
    )
    .await;
    mock.clear();
    mock.send_to_client(added("ca::f", 2, "late", json!({})));
    until("the hook", || hook.held.seen.lock().unwrap().is_some()).await;
    mock.send_to_client(json!({
        "type": "registrationrejected", "code": "FUNCTION_NAMESPACE_CONFLICT",
        "namespace": "default", "function_id": "ca::f", "owner_worker_id": "w-other"
    }));
    until("the conflict", || !function.local_dispatch_active()).await;
    hook.held.release.notify_one();
    until("the add undone", || {
        hook.removed.lock().unwrap().contains(&"late".to_string())
    })
    .await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(acks(&mock).is_empty(), "no ack for it: {:#?}", acks(&mock));
    assert!(!function.is_observed());
    assert!(!hook.held.seen.lock().unwrap().clone().unwrap().is_active());
    iii.shutdown_async().await;
}
