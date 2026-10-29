//! observe against a fake engine: features, `observe()` replay, local
//! dispatch, the current invocation, reports, hooks at both levels, deliver
//! hooks and keyed delivery, routed calls, calls through an observer,
//! `private`, `standby` and reconnect, asserted on the frames the SDK puts on
//! the socket.
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
use iii_sdk::runtime::{FunctionRef, WorkerMetadata};
use iii_sdk::{
    AddedHook, Call, CallAction, CallRequest, Capture, Dispatch, Error, IIIClient, InitOptions,
    Keys, Match, ObserveRejection, Observer, ObserverOptions, ObserverState, On, RegisterFunction,
    RemovedHook, Source, WorkerIdentityMode, current_invocation, register_worker, scope,
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

/// The engine's hello on this connection: an `observe.v2` engine, or none.
fn hello(mock: &MockEngine, observe: bool) {
    hello_with(
        mock,
        if observe {
            json!(["observe.v1", "observe.v2", "local-dispatch.v1"])
        } else {
            json!([])
        },
    );
}

/// The engine's hello with these features (`workerregistered-v1.json`'s for
/// a 0.25 engine).
fn hello_with(mock: &MockEngine, features: Value) {
    let mut msg = json!({ "type": "workerregistered", "worker_id": "w-1", "reattach_token": "t" });
    if features.as_array().is_some_and(|f| !f.is_empty()) {
        msg["features"] = features;
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

/// The engine's hello on the worker's new connection, processed by the
/// worker when this returns.
async fn connected(mock: &MockEngine, iii: &IIIClient, observe: bool) {
    frame(mock, |m| {
        m["type"] == "invokefunction" && m["function_id"] == "engine::workers::register"
    })
    .await;
    hello(mock, observe);
    until("the hello", || {
        iii.engine_supports_observe() == Some(observe)
    })
    .await;
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

/// A call the engine routes to this worker, as an `observe.v2` engine sends
/// it: with an `invocation_id`, the caller it stamped, and the
/// `_caller_worker_id` stamp in `data`.
fn route_call(mock: &MockEngine, function_id: &str, invocation_id: &str, input: Value) {
    let mut data = input;
    data["_caller_worker_id"] = json!("2b8d4f6a-1c3e-4a5b-9d7f-0e2c4a6b8d1f");
    mock.send_to_client(json!({
        "type": "invokefunction", "invocation_id": invocation_id,
        "function_id": function_id, "data": data,
        "caller": { "worker_id": "2b8d4f6a-1c3e-4a5b-9d7f-0e2c4a6b8d1f", "worker_name": "checkout",
                    "function_id": "checkout::confirm",
                    "invocation_id": "5e7a9c1b-2d4f-4b6a-8c0e-1f3a5c7e9b20" }
    }));
}

/// The answer to the routed call `invocation_id`.
async fn answer_to(mock: &MockEngine, invocation_id: &str) -> Value {
    frame(mock, |m| {
        m["type"] == "invocationresult" && m["invocation_id"] == invocation_id
    })
    .await
}

// ───────────────────────────── features ──────────────────────────────────

#[tokio::test]
async fn features_are_declared_on_workers_register() {
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "owner");
    let register = frame(&mock, |m| m["function_id"] == "engine::workers::register").await;
    assert_eq!(
        register["data"]["features"],
        json!([
            "observe.v1",
            "observe.v2",
            "local-dispatch.v1",
            "large-messages.v1"
        ])
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

    // A function with hooks registers as a plain function; nothing runs locally.
    let recorder = Recorder::default();
    iii.on_observer_added_for("o::plain", recorder.clone());
    let function = iii.register_function("o::plain", echo());
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

    // the string shorthand: every successful call, with the defaults
    let minimal = iii.observe_with(
        "orders::archive",
        "search::reindex-order",
        ObserverOptions::default().id("d94f3a5b-6c7d-4e8f-a01b-2c3d4e5f6a7b"),
    );
    let on_error = iii.observe_with(
        Source::function("payments::charge")
            .filters(json!({ "input.amount": { "$gte": 500 } }))
            .capture([Capture::Input, Capture::Error, Capture::Caller])
            .on(On::Error),
        "alerts::payment-failed",
        ObserverOptions::default()
            .id("c81e2f4a-5b6c-4d7e-9f0a-1b2c3d4e5f60")
            .max_in_flight(4)
            .metadata(json!({ "team": "payments-oncall" })),
    );
    let sent = frame(&mock, |m| m["type"] == "observe" && m["id"] == minimal.id()).await;
    assert_eq!(sent, fixture("observe-minimal.json"));
    let sent = frame(&mock, |m| {
        m["type"] == "observe" && m["id"] == on_error.id()
    })
    .await;
    assert_eq!(sent, fixture("observe-on-error.json"));

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

/// An observer made once the engine's hello was processed goes out at once,
/// to an `observe.v2` engine as to a 0.25 one.
#[tokio::test]
async fn an_observer_made_after_the_hello_is_sent_at_once() {
    for features in [
        json!(["observe.v1", "observe.v2", "local-dispatch.v1"]),
        fixture("workerregistered-v1.json")["features"].clone(),
    ] {
        let mock = MockEngine::start().await;
        let iii = worker(&mock, "observer");
        frame(&mock, |m| m["function_id"] == "engine::workers::register").await;
        hello_with(&mock, features.clone());
        until("the hello", || iii.engine_supports_observe() == Some(true)).await;
        let handle = iii.observe("a::b", "c::d");
        frame(&mock, |m| m["type"] == "observe" && m["id"] == handle.id()).await;
        assert_eq!(frames(&mock, "observe").len(), 1, "features {features}");
        iii.shutdown_async().await;
    }
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
        CallRequest {
            function_id: "l::f".into(),
            payload: json!({ "n": 1 }),
            action: Some(CallAction::Void),
            timeout_ms: None,
        }
        .dispatch(dispatch)
    };
    iii.call(void(Dispatch::Auto)).await.unwrap();
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
        .call(CallRequest::new("l::f", json!({ "n": 2 })))
        .await
        .unwrap();
    assert_eq!(out, json!({ "n": 2 }));
    iii.call(void(Dispatch::Auto)).await.unwrap();
    until("void local call", || calls.load(Ordering::SeqCst) == 2).await;
    assert_eq!(invocations(&mock), 0);

    // Dispatch::Remote, an enqueue and another namespace go through the engine.
    iii.call(void(Dispatch::Remote)).await.unwrap();
    let enqueue = CallRequest {
        function_id: "l::f".into(),
        payload: json!({}),
        action: Some(CallAction::Enqueue { queue: "q".into() }),
        timeout_ms: Some(50),
    };
    let _ = iii.call(enqueue).await;
    iii.call(void(Dispatch::Auto).namespace("other"))
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
    iii.call(void(Dispatch::Auto)).await.unwrap();
    frame(&mock, |m| m["function_id"] == "l::f").await;
    iii.shutdown_async().await;
}

/// Local calls the same way as the zero-wire loop: the mock reads on other
/// threads, and the loop yields so nothing it sends can go unseen.
async fn local_calls(iii: &IIIClient, function_id: &str, n: u64) {
    for i in 0..n {
        let out = iii
            .call(CallRequest::new(function_id, json!(i)))
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
        .call(CallRequest::new("v::f", original.clone()))
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
        .call(CallRequest::new("e::fail", json!({})))
        .await
        .unwrap_err();
    let inv = err.invocation_error().expect("a remote-shaped error");
    assert_eq!(inv.code, "invocation_failed");
    assert_eq!(inv.message, "handler error: boom");
    assert!(inv.stacktrace.is_some());

    let err = iii
        .call(CallRequest::new("e::coded", json!({})))
        .await
        .unwrap_err();
    let inv = err.invocation_error().unwrap();
    assert_eq!((inv.code.as_str(), inv.stacktrace), ("OUT_OF_STOCK", None));

    let slow = CallRequest {
        timeout_ms: Some(50),
        ..CallRequest::new("e::slow", json!({}))
    };
    assert!(matches!(iii.call(slow).await, Err(Error::Timeout)));
    until("the slow handler to finish", || {
        finished.load(Ordering::SeqCst) == 1
    })
    .await;

    let err = iii
        .call(CallRequest::new("e::panics", json!({})))
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
                iii.call(CallRequest::new("c::inner", input.clone()))
                    .await?;
                // remote call from a spawned task, with the context carried
                let ctx = current_invocation();
                tokio::spawn(scope(ctx, {
                    let iii = iii.clone();
                    async move {
                        let _ = iii
                            .call(CallRequest {
                                function_id: "other::fn".into(),
                                payload: json!({}),
                                action: Some(CallAction::Void),
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
    iii.call(CallRequest {
        function_id: "x::y".into(),
        payload: json!({}),
        action: Some(CallAction::Void),
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
    let call = |input: Value| iii.call(CallRequest::new("p::charge", input));
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
    assert!(
        item.get("caller").is_none(),
        "a call made outside any handler has no caller"
    );
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
        iii.call(CallRequest::new("b::f", json!(i))).await.unwrap();
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
        iii.call(CallRequest::new("r::f", json!(i))).await.unwrap();
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
impl AddedHook for Recorder {
    async fn on_added(&self, obs: Observer) -> Result<(), ObserveRejection> {
        if let Some(delay) = obs.args_value().get("delay_ms").and_then(Value::as_u64) {
            tokio::time::sleep(Duration::from_millis(delay)).await;
        }
        if let Some(code) = obs.args_value().get("reject").and_then(Value::as_str) {
            self.events
                .lock()
                .unwrap()
                .push(format!("reject {}", obs.id()));
            let message = obs.args_value()["message"]
                .as_str()
                .unwrap_or("invalid day-of-week field");
            return Err(ObserveRejection::new(code, message));
        }
        if obs.args_value().get("plain_error").is_some() {
            return Err(Error::Handler("no".into()).into());
        }
        self.events
            .lock()
            .unwrap()
            .push(format!("add {}", obs.id()));
        self.observers.lock().unwrap().push(obs);
        Ok(())
    }
}

#[async_trait]
impl RemovedHook for Recorder {
    async fn on_removed(&self, obs: Observer) {
        self.events
            .lock()
            .unwrap()
            .push(format!("remove {}", obs.id()));
        self.observers.lock().unwrap().retain(|o| o != &obs);
    }
}

/// Sets `hooks` as the added and removed hooks of `function_id`, before it
/// is registered.
fn set_hooks<H: AddedHook + RemovedHook + Clone>(iii: &IIIClient, function_id: &str, hooks: &H) {
    iii.on_observer_added_for(function_id, hooks.clone())
        .on_observer_removed_for(function_id, hooks.clone());
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
    set_hooks(&iii, "cron::on-tick", &recorder);
    hosted(&mock, &iii, "cron::on-tick", echo(), json!([])).await;

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

    // removed, then re-added with a rejection: the error ack fixture
    mock.send_to_client(json!({
        "type": "observerremoved", "function_id": "cron::on-tick", "namespace": "default",
        "revision": 14, "id": obs.id()
    }));
    until("removed", || recorder.observers.lock().unwrap().is_empty()).await;
    mock.clear();
    let expected = fixture("observerack-error.json");
    let mut rejected = fixture("observeradded.json");
    rejected["revision"] = expected["revision"].clone();
    rejected["observer"]["id"] = expected["id"].clone();
    rejected["observer"]["args"] = json!({
        "reject": expected["error"]["code"], "message": expected["error"]["message"]
    });
    mock.send_to_client(rejected);
    let ack = frame(&mock, is("observerack")).await;
    assert_eq!(ack, expected);
    iii.shutdown_async().await;
}

#[tokio::test]
async fn hooks_run_one_at_a_time_in_revision_order() {
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "owner");
    connected(&mock, &iii, true).await;
    let recorder = Recorder::default();
    set_hooks(&iii, "h::f", &recorder);
    let function = hosted(&mock, &iii, "h::f", echo(), json!([])).await;

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

// ───────────────────────────── reconnect ─────────────────────────────────

#[tokio::test]
async fn reconnect_diffs_the_set_and_keeps_serving_locally() {
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "owner");
    connected(&mock, &iii, true).await;
    let recorder = Recorder::default();
    set_hooks(&iii, "d::f", &recorder);
    let function = hosted(
        &mock,
        &iii,
        "d::f",
        echo(),
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
        .call(CallRequest::new("d::f", json!("offline")))
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

/// An observer the engine announces again with the same args is re-acked
/// without running any hook, and served with the filters it was announced
/// with: listed by `functionregistered` or by `observeradded`. The handle its
/// added hook got is the same observer. Changed args are a removal and then
/// an add (decision 44).
#[tokio::test]
async fn a_replay_with_the_same_args_runs_no_hook() {
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "owner");
    connected(&mock, &iii, true).await;
    let recorder = Recorder::default();
    set_hooks(&iii, "ra::f", &recorder);
    let function = hosted(
        &mock,
        &iii,
        "ra::f",
        echo(),
        json!([observer_with_args("a", json!({ "kind": "a" }))]),
    )
    .await;
    acked_for(&mock, "ra::f", &[("a", 1)]).await;
    let held = observer(&recorder, "a");
    let above = |n: i64| {
        json!({ "id": "a", "args": { "kind": "a" }, "target": "t::target",
                "filters": { "$and": [{ "input.n": { "$gt": n } }] },
                "capture": ["input", "caller"], "on": "success" })
    };
    let observed = |invocation_id: &'static str, n: i64| {
        let mock = &mock;
        async move {
            route_call(mock, "ra::f", invocation_id, json!({ "n": n }));
            ids(&answer_to(mock, invocation_id).await["observed"])
        }
    };

    confirm(&mock, "ra::f", 2, true, json!([above(1)]));
    acked_for(&mock, "ra::f", &[("a", 2)]).await;
    assert!(
        observed("a1b2c3d4-0001-4000-8000-000000000001", 1)
            .await
            .is_empty()
    );
    assert_eq!(
        observed("a1b2c3d4-0001-4000-8000-000000000002", 2).await,
        vec!["a"]
    );

    mock.send_to_client(json!({
        "type": "observeradded", "function_id": "ra::f", "namespace": "default",
        "revision": 3, "observer": above(5)
    }));
    acked_for(&mock, "ra::f", &[("a", 3)]).await;
    assert!(
        observed("a1b2c3d4-0001-4000-8000-000000000003", 2)
            .await
            .is_empty()
    );
    assert_eq!(
        observed("a1b2c3d4-0001-4000-8000-000000000004", 6).await,
        vec!["a"]
    );
    assert_eq!(
        *recorder.events.lock().unwrap(),
        vec!["add a"],
        "no hook ran"
    );
    assert_eq!(function.observers()[0].filters(), &above(5)["filters"]);
    assert_eq!(
        function.observers()[0].capture(),
        &[Capture::Input, Capture::Caller]
    );

    // the handle the added hook got: active, and its late rejection is the
    // observer's, at the revision announced last
    assert!(held.is_active());
    held.reject("GONE", "config reloaded");
    let rejection = frame(&mock, |m| {
        m["type"] == "observerack" && m["error"]["code"] == "GONE"
    })
    .await;
    assert_eq!(rejection["revision"], 3);
    until("rejected", || !function.is_observed()).await;

    // a fresh observeradded brings it back as a new observer
    mock.send_to_client(added("ra::f", 4, "a", json!({ "kind": "a" })));
    acked_for(&mock, "ra::f", &[("a", 4)]).await;
    until("re-added", || function.is_observed()).await;
    // changed args: removed, then added
    confirm(
        &mock,
        "ra::f",
        5,
        true,
        json!([observer_with_args("a", json!({ "kind": "b" }))]),
    );
    acked_for(&mock, "ra::f", &[("a", 5)]).await;
    until("the args changed", || {
        recorder.events.lock().unwrap().len() == 5
    })
    .await;
    assert_eq!(
        *recorder.events.lock().unwrap(),
        vec!["add a", "remove a", "add a", "remove a", "add a"]
    );
    assert_eq!(observer(&recorder, "a").args_value()["kind"], "b");
    iii.shutdown_async().await;
}

// ───────────────────────────── match and deliver hooks ───────────────────

/// `src::events` with three observers, each wanting the calls of the `kind`
/// its args name.
async fn kinds_setup(mock: &MockEngine, iii: &IIIClient) -> (FunctionRef, Recorder) {
    let recorder = Recorder::default();
    set_hooks(iii, "src::events", &recorder);
    iii.on_observer_match_for(
        "src::events",
        Match::predicate(|obs, call| obs.args_value()["kind"] == call.input["kind"]),
    );
    let function = hosted(
        mock,
        iii,
        "src::events",
        RegisterFunction::new(|event: Value| Ok::<_, Error>(event)),
        json!([
            {
                "id": "only-x1", "args": { "kind": "a" }, "target": "t::one",
                "filters": { "$and": [{ "input.x": { "$eq": 1 } }] },
                "capture": ["input", "caller"], "on": "success"
            },
            { "id": "all-a", "args": { "kind": "a" }, "filters": {}, "target": "t::two",
              "capture": ["input"], "on": "success" },
            { "id": "all-b", "args": { "kind": "b" }, "filters": {}, "target": "t::three",
              "capture": ["input", "output", "caller", "metadata"], "on": "success" }
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

/// The match hook picks the observers, iii applies their filters, and one
/// local call is one report item listing every observer delivered.
#[tokio::test]
async fn a_match_hook_picks_the_observers_and_iii_applies_their_filters() {
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "database");
    connected(&mock, &iii, true).await;
    let _ = kinds_setup(&mock, &iii).await;
    mock.clear();

    // kind a: both a-observers are picked, only-x1's filter says no
    let out = iii
        .call(
            CallRequest::new("src::events", json!({ "kind": "a", "x": 2 }))
                .metadata(json!({ "m": 1 })),
        )
        .await
        .unwrap();
    assert_eq!(
        out,
        json!({ "kind": "a", "x": 2 }),
        "the caller gets the handler's value"
    );
    // kind b
    iii.call(CallRequest::new(
        "src::events",
        json!({ "kind": "b", "x": 1 }),
    ))
    .await
    .unwrap();
    let items = mock.wait_for(|m| report_items_in(m).len() >= 2, WAIT).await;
    let items = report_items_in(&items);
    assert_eq!(items.len(), 2, "{items:#?}");
    assert_eq!(items[0]["matched"], json!(["all-a"]));
    assert_eq!(items[0]["input"], json!({ "kind": "a", "x": 2 }));
    assert!(items[0].get("metadata").is_none(), "not captured by all-a");
    assert_eq!(items[1]["matched"], json!(["all-b"]));
    // all-b captures input and output of a source: the value travels once
    assert_eq!(items[1]["input"], json!({ "kind": "b", "x": 1 }));
    assert_eq!(items[1]["output_is_input"], true);
    assert!(items[1].get("output").is_none());
    assert!(
        items[1].get("caller").is_none(),
        "a call made outside any handler has no caller"
    );
    iii.shutdown_async().await;
}

fn report_items_in(messages: &[Value]) -> Vec<Value> {
    messages
        .iter()
        .filter(|f| message_type(f) == Some("invocationsobserved"))
        .flat_map(|f| f["items"].as_array().unwrap().clone())
        .collect()
}

/// `Observer::matches` applies `on` and the filters, as iii's delivery does.
#[tokio::test]
async fn an_observer_matches_by_on_and_filters() {
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "http");
    connected(&mock, &iii, true).await;
    let (_function, recorder) = kinds_setup(&mock, &iii).await;
    let only_x1 = observer(&recorder, "only-x1");
    let call = |x: i64| Call {
        function_id: "src::events".into(),
        input: json!({ "kind": "a", "x": x }),
        output: Some(json!({ "kind": "a", "x": x })),
        ..Call::default()
    };
    assert!(only_x1.matches(&call(1)));
    assert!(!only_x1.matches(&call(2)), "the filter says no");
    let failed = Call {
        status: iii_sdk::InvocationStatus::Error,
        output: None,
        error: Some(iii_sdk::ObservedError {
            code: "E".into(),
            message: "failed".into(),
        }),
        ..call(1)
    };
    assert!(!only_x1.matches(&failed), "on: success");
    iii.shutdown_async().await;
}

/// A call through an observer, made from a deliver hook: the fixture frame,
/// the target's result back, and the hook's invocation as its caller.
#[tokio::test]
async fn a_call_through_an_observer_is_the_fixture_frame() {
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "http");
    connected(&mock, &iii, true).await;
    let recorder = Recorder::default();
    set_hooks(&iii, "http::on-request", &recorder);
    iii.on_observer_deliver_for("http::on-request", {
        let iii = iii.clone();
        move |call: Call, observers: Vec<Observer>| {
            let iii = iii.clone();
            async move {
                let Some(route) = observers.first() else {
                    return Ok(json!({ "status_code": 404 }));
                };
                if !route.matches(&call) {
                    return Ok(json!({ "status_code": 422 }));
                }
                iii.call(CallRequest::observer(route, call.input.clone()))
                    .await
            }
        }
    });
    hosted(
        &mock,
        &iii,
        "http::on-request",
        RegisterFunction::new(|request: Value| Ok::<_, Error>(request)).private(),
        json!([{
            "id": "b6c4e2a0-9f1d-4b3a-8e7c-6d5f4e3a2b1c",
            "args": { "method": "GET", "path": "/users/:id" },
            "filters": { "$and": [{ "input.headers.x-tenant": { "$eq": "acme" } }] },
            "target": "users::get", "capture": ["input"], "on": "success"
        }]),
    )
    .await;
    acked(&mock, &[("b6c4e2a0-9f1d-4b3a-8e7c-6d5f4e3a2b1c", 1)]).await;
    mock.clear();

    let fixture_call = fixture("invokefunction-observer-call.json");
    let request = fixture_call["data"].clone();
    let pending = tokio::spawn({
        let iii = iii.clone();
        async move {
            iii.call(CallRequest::new("http::on-request", request))
                .await
        }
    });
    let sent = frame(&mock, |m| m["function_id"] == "users::get").await;
    for key in ["type", "function_id", "data", "activation"] {
        assert_eq!(sent[key], fixture_call[key], "{key}: {sent:#}");
    }
    assert!(
        sent.get("namespace").is_none(),
        "no namespace on a call through an observer"
    );
    assert!(sent.get("observe_depth").is_none());
    assert_eq!(sent["caller"]["function_id"], "http::on-request");
    let hook_invocation = sent["caller"]["invocation_id"].clone();
    assert!(
        hook_invocation.is_string(),
        "the hook runs in the function's invocation"
    );
    let mut result = fixture("invocationresult.json");
    result["invocation_id"] = sent["invocation_id"].clone();
    mock.send_to_client(result.clone());
    let response = tokio::time::timeout(WAIT, pending)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(
        response, result["result"],
        "the target's result is the caller's"
    );

    // a filter miss answers without calling anyone
    mock.clear();
    let mut other = fixture_call["data"].clone();
    other["headers"]["x-tenant"] = json!("other");
    let response = iii
        .call(CallRequest::new("http::on-request", other))
        .await
        .unwrap();
    assert_eq!(response, json!({ "status_code": 422 }));
    assert_eq!(invocations(&mock), 0);

    // the engine's refusal is the call's error
    let obs = observer(&recorder, "b6c4e2a0-9f1d-4b3a-8e7c-6d5f4e3a2b1c");
    let refused = tokio::spawn({
        let iii = iii.clone();
        let obs = obs.clone();
        async move { iii.call(CallRequest::observer(&obs, json!({}))).await }
    });
    let sent = frame(&mock, |m| m["function_id"] == "users::get").await;
    let mut error = fixture("invocationresult-not-function-owner.json");
    error["invocation_id"] = sent["invocation_id"].clone();
    mock.send_to_client(error);
    let err = tokio::time::timeout(WAIT, refused)
        .await
        .unwrap()
        .unwrap()
        .unwrap_err();
    assert_eq!(err.invocation_error().unwrap().code, "NOT_FUNCTION_OWNER");
    iii.shutdown_async().await;
}

/// A provider sends a new observer its initial state once it is accepted:
/// the void fixture frame, after the ack.
#[tokio::test]
async fn an_accepted_observer_gets_its_initial_state() {
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "storefront");
    connected(&mock, &iii, true).await;
    iii.on_observer_added_for("stock::on-changed", {
        let iii = iii.clone();
        move |obs: Observer| {
            let iii = iii.clone();
            async move {
                tokio::spawn(async move {
                    obs.accepted().await.expect("accepted");
                    let initial =
                        CallRequest::observer(&obs, json!({ "sku": "A-1", "available": 12 }));
                    iii.call(initial.action(CallAction::Void)).await
                });
                Ok(())
            }
        }
    });
    hosted(
        &mock,
        &iii,
        "stock::on-changed",
        echo().private(),
        json!([{
            "id": "868c8f79-e34f-4390-8ca6-7904b54ba84a", "args": {}, "filters": {},
            "target": "storefront::sync-stock", "capture": ["input"], "on": "success"
        }]),
    )
    .await;
    let sent = frame(&mock, |m| m["function_id"] == "storefront::sync-stock").await;
    assert_eq!(sent, fixture("invokefunction-observer-call-void.json"));
    let msgs = mock.received_messages();
    let ack_at = msgs.iter().position(is("observerack")).unwrap();
    let call_at = msgs
        .iter()
        .position(|m| m["function_id"] == "storefront::sync-stock")
        .unwrap();
    assert!(ack_at < call_at, "the call follows the ack");

    // a rejected observer is never accepted
    let refused: Arc<Mutex<Option<String>>> = Arc::default();
    iii.on_observer_added_for("stock::refused", {
        let refused = refused.clone();
        move |obs: Observer| {
            let refused = refused.clone();
            async move {
                tokio::spawn(async move {
                    let err = obs.accepted().await.unwrap_err();
                    *refused.lock().unwrap() = Some(err.invocation_error().unwrap().code.clone());
                });
                Err(ObserveRejection::new("NO_STOCK", "not tracked"))
            }
        }
    });
    hosted(
        &mock,
        &iii,
        "stock::refused",
        echo(),
        json!([view("r", json!({}), json!(["input"]), "success")]),
    )
    .await;
    until("refused", || refused.lock().unwrap().is_some()).await;
    assert_eq!(refused.lock().unwrap().as_deref(), Some("NO_STOCK"));
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
    iii.call(CallRequest::new("s::f", json!(1))).await.unwrap();
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
    set_hooks(&iii, "lr::f", &recorder);
    let function = hosted(&mock, &iii, "lr::f", echo(), observers.clone()).await;
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
    assert!(!a.is_active(), "a rejected observer is delivered nothing");

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
    set_hooks(&iii, "ls::f", &recorder);
    let function = hosted(&mock, &iii, "ls::f", echo(), observers.clone()).await;
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

/// Reports carry the trace context of the invocation the call was made in.
#[tokio::test]
async fn reports_carry_the_invocations_trace_context() {
    const TRACEPARENT: &str = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "database");
    connected(&mock, &iii, true).await;
    hosted(
        &mock,
        &iii,
        "tc::on-row",
        echo().private(),
        json!([view(
            "all",
            json!({}),
            json!(["input", "caller"]),
            "success"
        )]),
    )
    .await;
    acked(&mock, &[("all", 1)]).await;
    let seen: Arc<Mutex<Option<String>>> = Arc::default();
    hosted(
        &mock,
        &iii,
        "tc::execute",
        RegisterFunction::new_async({
            let iii = iii.clone();
            let seen = seen.clone();
            move |row: Value| {
                let iii = iii.clone();
                let seen = seen.clone();
                async move {
                    *seen.lock().unwrap() = current_invocation().and_then(|c| c.traceparent);
                    iii.call(CallRequest::new("tc::on-row", row)).await?;
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
    assert_eq!(
        item["caller"]["function_id"], "tc::execute",
        "made in tc::execute's invocation"
    );
    assert!(seen.lock().unwrap().is_some());
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
                    let request = CallRequest {
                        timeout_ms: Some(300),
                        ..CallRequest::new("rec::f", json!({ "n": n - 1 }))
                    };
                    // boxed: the nested call's future is as deep as the call
                    Box::pin(iii.call(request)).await
                }
            }
        }),
        json!([]),
    )
    .await;
    mock.clear();
    let out = iii
        .call(CallRequest::new("rec::f", json!({ "n": 30 })))
        .await
        .unwrap();
    assert_eq!(out, json!(0), "shallow recursion stays local");
    assert_eq!(invocations(&mock), 0);

    let _ = iii
        .call(CallRequest::new("rec::f", json!({ "n": 100 })))
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
        iii.call(CallRequest::new("cf::lost", json!(i)))
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
        iii.call(CallRequest::new("cf::kept", json!(i)))
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
    set_hooks(&iii, "sh::f", &recorder);
    hosted(&mock, &iii, "sh::f", echo(), json!([a.clone()])).await;
    acked(&mock, &[("a", 1)]).await;

    mock.clear();
    mock.close_active_connection();
    until("disconnect", || {
        iii.get_connection_state() != iii_sdk::runtime::IIIConnectionState::Connected
    })
    .await;
    iii.call(CallRequest::new("sh::f", json!("offline")))
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
impl AddedHook for HeldHook {
    async fn on_added(&self, obs: Observer) -> Result<(), ObserveRejection> {
        *self.seen.lock().unwrap() = Some(obs);
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
    iii.on_observer_added_for("rh::f", hook.clone());
    let function = hosted(&mock, &iii, "rh::f", echo(), json!([])).await;
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
        .call(CallRequest::new("big::f", json!(big)))
        .await
        .unwrap();
    assert_eq!(out.as_str().map(str::len), Some(big.len()));
    let err = iii
        .call(CallRequest::new("big::f", json!(big)).dispatch(Dispatch::Remote))
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
                    Box::pin(iii.call(CallRequest::new("deep::f", json!(n - 1)))).await
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
    set_hooks(&iii, "cv::f", &recorder);
    let function = hosted(
        &mock,
        &iii,
        "cv::f",
        echo(),
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
impl AddedHook for HeldAndRemoved {
    async fn on_added(&self, obs: Observer) -> Result<(), ObserveRejection> {
        self.held.on_added(obs).await
    }
}

#[async_trait]
impl RemovedHook for HeldAndRemoved {
    async fn on_removed(&self, obs: Observer) {
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
    set_hooks(&iii, "ca::f", &hook);
    let function = hosted(&mock, &iii, "ca::f", echo(), json!([])).await;
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

// ───────────────────────────── hooks at both levels ──────────────────────

type Events = Arc<Mutex<Vec<String>>>;

/// An added hook that records `"{label} added {id}"`, and rejects an
/// observer whose args name its label under `reject`.
fn added_hook(
    events: &Events,
    label: &'static str,
) -> impl Fn(
    Observer,
)
    -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<(), ObserveRejection>> + Send>>
+ Send
+ Sync
+ 'static {
    let events = events.clone();
    move |obs: Observer| {
        let events = events.clone();
        Box::pin(async move {
            if obs.args_value()["reject"] == label {
                return Err(ObserveRejection::new("NOPE", format!("{label} says no")));
            }
            events
                .lock()
                .unwrap()
                .push(format!("{label} added {}", obs.id()));
            Ok(())
        })
    }
}

/// A removed hook that records `"{label} removed {id}"`.
fn removed_hook(
    events: &Events,
    label: &'static str,
) -> impl Fn(Observer) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>>
+ Send
+ Sync
+ 'static {
    let events = events.clone();
    move |obs: Observer| {
        let events = events.clone();
        Box::pin(async move {
            events
                .lock()
                .unwrap()
                .push(format!("{label} removed {}", obs.id()));
        })
    }
}

fn observer_with_args(id: &str, args: Value) -> Value {
    json!({ "id": id, "args": args, "filters": {}, "target": "t::target",
            "capture": ["input"], "on": "success" })
}

fn events_of(events: &Events) -> Vec<String> {
    events.lock().unwrap().clone()
}

/// The worker's added hook runs, then the function's; removal runs the
/// function's, then the worker's. The worker's hooks run for every function
/// it hosts.
#[tokio::test]
async fn hooks_run_at_both_levels_in_order() {
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "owner");
    connected(&mock, &iii, true).await;
    let events: Events = Arc::default();
    iii.on_observer_added(added_hook(&events, "worker"))
        .on_observer_removed(removed_hook(&events, "worker"))
        .on_observer_added_for("both::f", added_hook(&events, "function"))
        .on_observer_removed_for("both::f", removed_hook(&events, "function"));
    hosted(
        &mock,
        &iii,
        "both::f",
        echo(),
        json!([view("o", json!({}), json!(["input"]), "success")]),
    )
    .await;
    acked_for(&mock, "both::f", &[("o", 1)]).await;
    mock.send_to_client(json!({
        "type": "observerremoved", "function_id": "both::f", "revision": 2, "id": "o"
    }));
    until("the removal", || events_of(&events).len() == 4).await;
    assert_eq!(
        events_of(&events),
        vec![
            "worker added o",
            "function added o",
            "function removed o",
            "worker removed o"
        ]
    );

    events.lock().unwrap().clear();
    let other = hosted(
        &mock,
        &iii,
        "other::f",
        echo(),
        json!([view("p", json!({}), json!(["input"]), "success")]),
    )
    .await;
    acked_for(&mock, "other::f", &[("p", 1)]).await;
    other.unregister();
    until("the removal", || events_of(&events).len() == 2).await;
    assert_eq!(
        events_of(&events),
        vec!["worker added p", "worker removed p"]
    );
    iii.shutdown_async().await;
}

/// A rejected observer: the removed hooks of the levels whose added hook
/// already resolved run, and no other.
#[tokio::test]
async fn a_rejection_unwinds_the_levels_that_resolved() {
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "owner");
    connected(&mock, &iii, true).await;
    let events: Events = Arc::default();
    iii.on_observer_added(added_hook(&events, "worker"))
        .on_observer_removed(removed_hook(&events, "worker"))
        .on_observer_added_for("un::f", added_hook(&events, "function"))
        .on_observer_removed_for("un::f", removed_hook(&events, "function"));
    hosted(&mock, &iii, "un::f", echo(), json!([])).await;

    // the function's hook rejects: the worker's setup is undone
    mock.send_to_client(added("un::f", 2, "late", json!({ "reject": "function" })));
    let acks = acked(&mock, &[("late", 2)]).await;
    assert_eq!(acks[0]["error"]["code"], "NOPE");
    until("the unwinding", || events_of(&events).len() == 2).await;
    assert_eq!(
        events_of(&events),
        vec!["worker added late", "worker removed late"]
    );

    // the worker's hook rejects: nothing resolved, nothing to undo
    events.lock().unwrap().clear();
    mock.send_to_client(added("un::f", 3, "early", json!({ "reject": "worker" })));
    let acks = acked(&mock, &[("early", 3)]).await;
    assert!(
        acks.iter()
            .any(|a| a["id"] == "early" && a["error"]["code"] == "NOPE")
    );
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(events_of(&events).is_empty(), "{:?}", events_of(&events));
    iii.shutdown_async().await;
}

/// The removed hooks run exactly once for every accepted observer, and never
/// for one that was not accepted.
#[tokio::test]
async fn removal_runs_once_per_accepted_observer() {
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "owner");
    connected(&mock, &iii, true).await;
    let recorder = Recorder::default();
    set_hooks(&iii, "once::f", &recorder);
    let function = hosted(
        &mock,
        &iii,
        "once::f",
        echo(),
        json!([
            observer_with_args("a", json!({})),
            observer_with_args("r", json!({ "reject": "NOPE" }))
        ]),
    )
    .await;
    acked(&mock, &[("a", 1), ("r", 1)]).await;
    let removed = |n: usize| {
        let recorder = recorder.clone();
        move || {
            recorder
                .events
                .lock()
                .unwrap()
                .iter()
                .filter(|e| e.starts_with("remove"))
                .count()
                == n
        }
    };

    // an accepted observer, removed twice: once
    for revision in [2, 3] {
        mock.send_to_client(json!({
            "type": "observerremoved", "function_id": "once::f", "revision": revision, "id": "a"
        }));
    }
    // a rejected one: never
    mock.send_to_client(json!({
        "type": "observerremoved", "function_id": "once::f", "revision": 4, "id": "r"
    }));
    until("a removed", removed(1)).await;

    // rejected late: removed when the engine removes it, and not again on a
    // replay that does not list it
    mock.send_to_client(added("once::f", 5, "b", json!({})));
    until("b added", || function.is_observed()).await;
    observer(&recorder, "b").reject("GONE", "config reloaded");
    mock.send_to_client(json!({
        "type": "observerremoved", "function_id": "once::f", "revision": 6, "id": "b"
    }));
    until("b removed", removed(2)).await;
    confirm(&mock, "once::f", 7, true, json!([]));

    // still held when the function is unregistered
    mock.send_to_client(added("once::f", 8, "c", json!({})));
    until("c added", || function.is_observed()).await;
    function.unregister();
    until("c removed", removed(3)).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(
        *recorder.events.lock().unwrap(),
        vec![
            "add a", "reject r", "remove a", "add b", "remove b", "add c", "remove c"
        ]
    );
    iii.shutdown_async().await;
}

/// Hooks run one at a time per function, and concurrently across functions:
/// a hook of one function that waits for a hook of another finishes.
#[tokio::test]
async fn hooks_of_different_functions_run_concurrently() {
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "owner");
    connected(&mock, &iii, true).await;
    let release = Arc::new(tokio::sync::Notify::new());
    iii.on_observer_added_for("wait::f", {
        let release = release.clone();
        move |_obs: Observer| {
            let release = release.clone();
            async move {
                release.notified().await;
                Ok(())
            }
        }
    });
    iii.on_observer_added_for("free::f", {
        let release = release.clone();
        move |_obs: Observer| {
            let release = release.clone();
            async move {
                release.notify_one();
                Ok(())
            }
        }
    });
    hosted(&mock, &iii, "wait::f", echo(), json!([])).await;
    hosted(&mock, &iii, "free::f", echo(), json!([])).await;
    mock.send_to_client(added("wait::f", 2, "waits", json!({})));
    mock.send_to_client(added("free::f", 2, "frees", json!({})));
    let acks = acked(&mock, &[("waits", 2), ("frees", 2)]).await;
    assert!(acks.iter().all(|a| a.get("error").is_none()), "{acks:#?}");
    iii.shutdown_async().await;
}

/// An added hook set once a function has active observers runs for each of
/// them; one that fails rejects its observer late.
#[tokio::test]
async fn a_hook_set_late_is_caught_up() {
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "owner");
    connected(&mock, &iii, true).await;
    let function = hosted(
        &mock,
        &iii,
        "late::f",
        echo(),
        json!([
            observer_with_args("o1", json!({})),
            observer_with_args("o2", json!({ "reject": "function" }))
        ]),
    )
    .await;
    acked(&mock, &[("o1", 1), ("o2", 1)]).await;
    until("both active", || function.observers().len() == 2).await;
    mock.clear();

    let events: Events = Arc::default();
    function.on_observer_added(added_hook(&events, "function"));
    let rejection = frame(&mock, |m| {
        m["type"] == "observerack" && m["id"] == "o2" && m["error"].is_object()
    })
    .await;
    assert_eq!(rejection["error"]["code"], "NOPE");
    assert_eq!(rejection["revision"], 1);
    until("caught up", || events_of(&events).len() == 1).await;
    assert_eq!(events_of(&events), vec!["function added o1"]);
    until("o2 out", || function.observers().len() == 1).await;
    assert_eq!(
        acks(&mock).len(),
        1,
        "an accepted observer is not acked again"
    );
    iii.shutdown_async().await;
}

// ───────────────────────────── matching by keys ──────────────────────────

/// The key index follows adds and removes: a call matches the observers
/// filed under its keys, `is_observed_for` answers by key, and filters still
/// apply.
#[tokio::test]
async fn the_key_index_follows_adds_and_removes() {
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "cron");
    connected(&mock, &iii, true).await;
    iii.on_observer_match_for(
        "cron::on-tick",
        Match::keys(
            |obs| {
                obs.args_value()["expression"]
                    .as_str()
                    .unwrap_or("*")
                    .into()
            },
            |call| Keys::from(vec![call.input["expression"].as_str().unwrap_or(""), "*"]),
        ),
    );
    let on_tick = hosted(
        &mock,
        &iii,
        "cron::on-tick",
        echo().private(),
        json!([
            observer_with_args("nine", json!({ "expression": "A" })),
            observer_with_args("five", json!({ "expression": "B" })),
            observer_with_args("every", json!({}))
        ]),
    )
    .await;
    acked(&mock, &[("nine", 1), ("five", 1), ("every", 1)]).await;
    let tick = |expression: &str, n: i64| Call {
        input: json!({ "expression": expression, "n": n }),
        ..Call::default()
    };
    assert!(on_tick.is_observed_for(&tick("A", 0)));
    assert!(
        on_tick.is_observed_for(&tick("C", 0)),
        "every is filed under *"
    );

    let delivered = |input: Value| {
        let iii = iii.clone();
        let mock = &mock;
        async move {
            mock.clear();
            iii.call(CallRequest::new("cron::on-tick", input))
                .await
                .unwrap();
            let messages = mock
                .wait_for(|m| !report_items_in(m).is_empty(), WAIT)
                .await;
            let mut matched: Vec<String> = report_items_in(&messages)[0]["matched"]
                .as_array()
                .unwrap()
                .iter()
                .map(|id| id.as_str().unwrap().to_string())
                .collect();
            matched.sort();
            matched
        }
    };
    assert_eq!(
        delivered(json!({ "expression": "A" })).await,
        vec!["every", "nine"]
    );
    assert_eq!(
        delivered(json!({ "expression": "B" })).await,
        vec!["every", "five"]
    );

    // removed: unfiled
    mock.send_to_client(json!({
        "type": "observerremoved", "function_id": "cron::on-tick", "revision": 2, "id": "every"
    }));
    until("every unfiled", || !on_tick.is_observed_for(&tick("C", 0))).await;
    assert_eq!(delivered(json!({ "expression": "A" })).await, vec!["nine"]);

    // added with a filter: filed, and its filter applies
    mock.send_to_client(json!({
        "type": "observeradded", "function_id": "cron::on-tick", "namespace": "default",
        "revision": 3,
        "observer": { "id": "late-a", "args": { "expression": "A" },
                      "filters": { "$and": [{ "input.n": { "$gt": 1 } }] },
                      "target": "t::target", "capture": ["input"], "on": "success" }
    }));
    acked(&mock, &[("late-a", 3)]).await;
    assert_eq!(
        delivered(json!({ "expression": "A", "n": 1 })).await,
        vec!["nine"]
    );
    assert_eq!(
        delivered(json!({ "expression": "A", "n": 2 })).await,
        vec!["late-a", "nine"]
    );
    iii.shutdown_async().await;
}

// ───────────────────────────── routed calls ──────────────────────────────

fn ids(value: &Value) -> Vec<String> {
    let mut ids: Vec<String> = value
        .as_array()
        .map(|ids| {
            ids.iter()
                .map(|id| id.as_str().unwrap().to_string())
                .collect()
        })
        .unwrap_or_default();
    ids.sort();
    ids
}

/// An `observe.v2` host delivers a routed call itself: the matches ride the
/// answer, and nothing else is sent.
#[tokio::test]
async fn a_routed_call_rides_its_answer() {
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "orders");
    connected(&mock, &iii, true).await;
    let expected = fixture("invocationresult-observed.json");
    let result = expected["result"].clone();
    let observed = ids(&expected["observed"]);
    hosted(
        &mock,
        &iii,
        "orders::create",
        RegisterFunction::new(move |_: Value| Ok::<_, Error>(result.clone())),
        json!([
            {
                "id": observed[0], "args": {}, "target": "t::a", "capture": ["input"],
                "filters": { "$and": [{ "input.qty": { "$gte": 1 } }] }, "on": "success"
            },
            observer_with_args(&observed[1], json!({})),
            { "id": "on-error", "args": {}, "filters": {}, "target": "t::b",
              "capture": ["error"], "on": "error" },
            { "id": "big", "args": {}, "target": "t::c", "capture": ["input"],
              "filters": { "$and": [{ "input.qty": { "$gte": 100 } }] }, "on": "success" }
        ]),
    )
    .await;
    acked(&mock, &[("on-error", 1), ("big", 1)]).await;
    mock.clear();

    let invocation = expected["invocation_id"].as_str().unwrap();
    route_call(
        &mock,
        "orders::create",
        invocation,
        json!({ "sku": "A-1", "qty": 2 }),
    );
    let answer = answer_to(&mock, invocation).await;
    for key in ["type", "invocation_id", "function_id", "result"] {
        assert_eq!(answer[key], expected[key], "{key}: {answer:#}");
    }
    assert_eq!(ids(&answer["observed"]), observed);
    assert!(answer.get("error").is_none());

    // the filters pick: only the unfiltered observer matches qty 0
    route_call(
        &mock,
        "orders::create",
        "2f3a5c7e-9b0d-4f2a-8c4e-6a8c0e2b4d6f",
        json!({ "sku": "A-1", "qty": 0 }),
    );
    let filtered = answer_to(&mock, "2f3a5c7e-9b0d-4f2a-8c4e-6a8c0e2b4d6f").await;
    assert_eq!(ids(&filtered["observed"]), vec![observed[1].clone()]);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        frames(&mock, "invocationsobserved").is_empty(),
        "a match costs no frame"
    );
    iii.shutdown_async().await;
}

/// The error the handler threw rides its answer with the `on: error`
/// observers it matched.
#[tokio::test]
async fn a_routed_error_rides_the_error_answer() {
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "payments");
    connected(&mock, &iii, true).await;
    let expected = fixture("invocationresult-observed-error.json");
    hosted(
        &mock,
        &iii,
        "payments::charge",
        RegisterFunction::new(|_: Value| {
            Err::<Value, _>(Error::Remote {
                code: "CARD_DECLINED".into(),
                message: "card declined by issuer".into(),
                stacktrace: None,
            })
        }),
        json!([
            { "id": expected["observed"][0], "args": {}, "target": "alerts::payment-failed",
              "filters": { "$and": [{ "input.amount": { "$gte": 500 } }] },
              "capture": ["input", "error", "caller"], "on": "error" },
            observer_with_args("on-success", json!({}))
        ]),
    )
    .await;
    acked(&mock, &[("on-success", 1)]).await;
    let invocation = expected["invocation_id"].as_str().unwrap();
    route_call(
        &mock,
        "payments::charge",
        invocation,
        json!({ "amount": 1250 }),
    );
    let answer = answer_to(&mock, invocation).await;
    assert_eq!(answer["error"]["code"], expected["error"]["code"]);
    assert_eq!(answer["error"]["message"], expected["error"]["message"]);
    assert!(answer.get("result").is_none());
    assert_eq!(answer["observed"], expected["observed"]);
    iii.shutdown_async().await;
}

/// A deliver hook answers a routed call itself: the answer is its value,
/// with no `observed` and no report item, and it gets the observers the
/// match hook says want the call.
#[tokio::test]
async fn a_deliver_hook_answers_routed_calls_and_iii_delivers_nothing() {
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "queue");
    connected(&mock, &iii, true).await;
    let got: Arc<Mutex<Vec<String>>> = Arc::default();
    iii.on_observer_match_for(
        "queue::on-message",
        Match::keys(
            |obs| obs.id().into(),
            |call| {
                call.metadata
                    .as_ref()
                    .and_then(|metadata| metadata["subscriber"].as_str())
                    .into()
            },
        ),
    )
    .on_observer_deliver_for("queue::on-message", {
        let got = got.clone();
        move |call: Call, observers: Vec<Observer>| {
            let got = got.clone();
            async move {
                *got.lock().unwrap() = observers.iter().map(|o| o.id().to_string()).collect();
                let Some(subscriber) = observers.first() else {
                    return Err(Error::Remote {
                        code: "OBSERVER_NOT_ACTIVE".into(),
                        message: "subscriber removed".into(),
                        stacktrace: None,
                    });
                };
                Ok(json!({ "acked_by": subscriber.id(), "matches": subscriber.matches(&call) }))
            }
        }
    });
    hosted(
        &mock,
        &iii,
        "queue::on-message",
        echo().private(),
        json!([
            observer_with_args("sub-a", json!({ "topic": "emails" })),
            observer_with_args("sub-b", json!({ "topic": "emails" }))
        ]),
    )
    .await;
    acked(&mock, &[("sub-a", 1), ("sub-b", 1)]).await;
    mock.clear();

    mock.send_to_client(json!({
        "type": "invokefunction", "invocation_id": "4f3a5c7e-9b0d-4f2a-8c4e-6a8c0e2b4d6f",
        "function_id": "queue::on-message", "data": { "to": "ada@example.com" },
        "metadata": { "subscriber": "sub-b" }
    }));
    let answer = answer_to(&mock, "4f3a5c7e-9b0d-4f2a-8c4e-6a8c0e2b4d6f").await;
    assert_eq!(
        answer["result"],
        json!({ "acked_by": "sub-b", "matches": true })
    );
    assert!(answer.get("observed").is_none(), "{answer:#}");
    assert_eq!(
        *got.lock().unwrap(),
        vec!["sub-b"],
        "the matched observer only"
    );

    // its error is the caller's
    mock.send_to_client(json!({
        "type": "invokefunction", "invocation_id": "5f3a5c7e-9b0d-4f2a-8c4e-6a8c0e2b4d6f",
        "function_id": "queue::on-message", "data": {},
        "metadata": { "subscriber": "gone" }
    }));
    let answer = answer_to(&mock, "5f3a5c7e-9b0d-4f2a-8c4e-6a8c0e2b4d6f").await;
    assert_eq!(answer["error"]["code"], "OBSERVER_NOT_ACTIVE");
    assert!(answer.get("observed").is_none());

    // and a local call: no report
    iii.call(
        CallRequest::new("queue::on-message", json!({})).metadata(json!({ "subscriber": "sub-a" })),
    )
    .await
    .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(frames(&mock, "invocationsobserved").is_empty());
    iii.shutdown_async().await;
}

/// A call a deliver hook makes that times out, rethrown, keeps `TIMEOUT` as
/// the caller's error, as in Node and Python: on a routed call's answer and
/// on a local call.
#[tokio::test]
async fn a_deliver_hooks_rethrown_timeout_keeps_its_code() {
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "http");
    connected(&mock, &iii, true).await;
    iii.on_observer_deliver_for("http::on-request", {
        let iii = iii.clone();
        move |call: Call, observers: Vec<Observer>| {
            let iii = iii.clone();
            async move {
                let route = observers.first().expect("one route");
                // The engine never answers: the call times out.
                iii.call(
                    CallRequest::observer(route, call.input.clone())
                        .timeout(Duration::from_millis(50)),
                )
                .await
            }
        }
    });
    hosted(
        &mock,
        &iii,
        "http::on-request",
        echo().private(),
        json!([observer_with_args("route-a", json!({ "path": "/a" }))]),
    )
    .await;
    acked(&mock, &[("route-a", 1)]).await;

    route_call(
        &mock,
        "http::on-request",
        "6f3a5c7e-9b0d-4f2a-8c4e-6a8c0e2b4d6f",
        json!({ "path": "/a" }),
    );
    let answer = answer_to(&mock, "6f3a5c7e-9b0d-4f2a-8c4e-6a8c0e2b4d6f").await;
    assert_eq!(answer["error"]["code"], "TIMEOUT", "{answer:#}");
    assert!(answer.get("observed").is_none());

    let err = iii
        .call(CallRequest::new(
            "http::on-request",
            json!({ "path": "/a" }),
        ))
        .await
        .unwrap_err();
    assert_eq!(err.invocation_error().unwrap().code, "TIMEOUT", "{err}");
    iii.shutdown_async().await;
}

/// A match hook narrows a routed call's observers; the matches ride its
/// answer.
#[tokio::test]
async fn a_match_hook_narrows_a_routed_calls_observers() {
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "cron");
    connected(&mock, &iii, true).await;
    iii.on_observer_match_for(
        "cron::routed",
        Match::predicate(|obs, call| obs.args_value()["expression"] == call.input["expression"]),
    );
    hosted(
        &mock,
        &iii,
        "cron::routed",
        echo(),
        json!([
            observer_with_args("nine", json!({ "expression": "A" })),
            observer_with_args("five", json!({ "expression": "B" }))
        ]),
    )
    .await;
    acked(&mock, &[("nine", 1), ("five", 1)]).await;
    route_call(
        &mock,
        "cron::routed",
        "6f3a5c7e-9b0d-4f2a-8c4e-6a8c0e2b4d6f",
        json!({ "expression": "B" }),
    );
    let answer = answer_to(&mock, "6f3a5c7e-9b0d-4f2a-8c4e-6a8c0e2b4d6f").await;
    assert_eq!(answer["observed"], json!(["five"]));
    assert_eq!(answer["result"]["expression"], "B");
    iii.shutdown_async().await;
}

// ──────────────────── engines without observe.v2, private, standby ───────

/// A 0.25 engine: functions that need `observe.v2` stay unregistered, the
/// engine evaluates routed calls, and a report of a source carries `output`.
#[tokio::test]
async fn an_engine_without_observe_v2_evaluates_routed_calls() {
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "database");
    frame(&mock, |m| m["function_id"] == "engine::workers::register").await;
    hello_with(
        &mock,
        fixture("workerregistered-v1.json")["features"].clone(),
    );
    let held = iii.register_function("v1::private", echo().private());
    let plain = hosted(
        &mock,
        &iii,
        "v1::source",
        echo(),
        json!([{ "id": "o", "args": {}, "filters": {}, "target": "t::a",
                 "capture": ["input", "output"], "on": "success" }]),
    )
    .await;
    acked(&mock, &[("o", 1)]).await;
    until("observed", || plain.is_observed()).await;

    // a routed call: the engine evaluates it, the answer has no `observed`
    route_call(
        &mock,
        "v1::source",
        "3f3a5c7e-9b0d-4f2a-8c4e-6a8c0e2b4d6f",
        json!({ "n": 1 }),
    );
    let answer = answer_to(&mock, "3f3a5c7e-9b0d-4f2a-8c4e-6a8c0e2b4d6f").await;
    assert!(answer.get("observed").is_none(), "{answer:#}");
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(frames(&mock, "invocationsobserved").is_empty());

    // a local call of a source: reported with its output
    iii.call(CallRequest::new("v1::source", json!({ "n": 2 })))
        .await
        .unwrap();
    let report = frame(&mock, is("invocationsobserved")).await;
    let item = &report["items"][0];
    assert_eq!(item["input"], json!({ "n": 2 }));
    assert_eq!(item["output"], json!({ "n": 2 }));
    assert!(item.get("output_is_input").is_none());

    // the observe API keeps working
    let handle = iii.observe("v1::source", "t::a");
    frame(&mock, |m| m["type"] == "observe" && m["id"] == handle.id()).await;
    assert!(
        frames(&mock, "registerfunction")
            .iter()
            .all(|m| m["id"] != "v1::private"),
        "a private function needs observe.v2"
    );
    assert!(!held.local_dispatch_active());
    iii.shutdown_async().await;
}

/// The registration of a private source, as the fixture pins it: top-level
/// `args_format` and `private`, no `observe` object, sent once this
/// connection's engine advertised `observe.v2`.
#[tokio::test]
async fn a_private_source_registers_with_the_fixture_shape() {
    #[derive(serde::Deserialize, schemars::JsonSchema)]
    #[serde(deny_unknown_fields)]
    #[allow(dead_code)]
    struct CronArgs {
        expression: String,
    }
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "cron");
    let expected = fixture("registerfunction-private.json");
    iii.register_function(
        "cron::on-tick",
        RegisterFunction::new(|tick: Value| Ok::<_, Error>(tick))
            .description(expected["description"].as_str().unwrap())
            .request_format(expected["request_format"].clone())
            .response_format(expected["response_format"].clone())
            .args_format_value(expected["args_format"].clone())
            .private(),
    );
    iii.register_function("vault::read-secret", echo().observable(false));
    frame(&mock, |m| m["function_id"] == "engine::workers::register").await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(
        frames(&mock, "registerfunction").is_empty(),
        "held for the hello"
    );
    hello(&mock, true);
    let sent = frame(&mock, registered("cron::on-tick")).await;
    assert_eq!(sent, expected);
    let closed = frame(&mock, registered("vault::read-secret")).await;
    assert_eq!(closed["observable"], false);
    assert!(closed.get("observe").is_none());

    // the generated schema is a JSON Schema of the args
    let generated = iii.register_function(
        "cron::typed",
        RegisterFunction::new(|tick: Value| Ok::<_, Error>(tick)).args_format::<CronArgs>(),
    );
    let sent = frame(&mock, registered("cron::typed")).await;
    assert_eq!(sent["args_format"]["required"], json!(["expression"]));
    assert_eq!(sent["args_format"]["additionalProperties"], false);
    generated.unregister();
    iii.shutdown_async().await;
}

/// A rejection that names a function refuses that one registration
/// (`registrationrejected-invalid-args-format.json`,
/// `registrationrejected-unsupported.json`): the connection stays open, a
/// call in flight completes, the worker's other functions keep serving, and
/// the refused function runs nowhere here.
#[tokio::test]
async fn a_refused_registration_costs_only_that_function() {
    for name in [
        "registrationrejected-invalid-args-format.json",
        "registrationrejected-unsupported.json",
    ] {
        let rejection = fixture(name);
        let refused_id = rejection["function_id"].as_str().unwrap().to_string();
        let mock = MockEngine::start().await;
        let iii = worker(&mock, "sources");
        connected(&mock, &iii, true).await;
        let kept = hosted(&mock, &iii, "kept::f", echo(), json!([])).await;
        let ran = Arc::new(AtomicUsize::new(0));
        let refused = iii.register_function(
            refused_id.as_str(),
            RegisterFunction::new({
                let ran = ran.clone();
                move |input: Value| {
                    ran.fetch_add(1, Ordering::SeqCst);
                    Ok::<_, Error>(input)
                }
            })
            .args_format_value(json!({ "type": "object" })),
        );
        frame(&mock, |m| {
            message_type(m) == Some("registerfunction") && m["id"] == refused_id.as_str()
        })
        .await;
        let in_flight = tokio::spawn({
            let iii = iii.clone();
            async move {
                iii.call(CallRequest::new("remote::f", json!({ "n": 1 })))
                    .await
            }
        });
        let call = frame(&mock, |m| {
            m["type"] == "invokefunction" && m["function_id"] == "remote::f"
        })
        .await;

        mock.send_to_client(rejection.clone());
        mock.send_to_client(json!({
            "type": "invocationresult", "invocation_id": call["invocation_id"],
            "function_id": "remote::f", "result": { "ok": true }
        }));
        let answer = tokio::time::timeout(WAIT, in_flight)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(answer.unwrap(), json!({ "ok": true }), "{name}");
        assert!(iii.fatal_error().is_none(), "{name}");
        assert_eq!(
            iii.get_connection_state(),
            iii_sdk::runtime::IIIConnectionState::Connected
        );

        // the other function keeps serving, locally and routed
        assert!(kept.local_dispatch_active());
        let local = iii
            .call(CallRequest::new("kept::f", json!({ "n": 2 })))
            .await
            .unwrap();
        assert_eq!(local, json!({ "n": 2 }));
        route_call(
            &mock,
            "kept::f",
            "6f3a5c7e-9b0d-4f2a-8c4e-6a8c0e2b4d6f",
            json!({ "n": 3 }),
        );
        let routed = answer_to(&mock, "6f3a5c7e-9b0d-4f2a-8c4e-6a8c0e2b4d6f").await;
        assert_eq!(routed["result"]["n"], 3, "{routed:#}");

        // the refused function: its calls go to the engine
        assert!(!refused.local_dispatch_active());
        mock.clear();
        iii.call(CallRequest {
            function_id: refused_id.clone(),
            payload: json!({}),
            action: Some(CallAction::Void),
            timeout_ms: None,
        })
        .await
        .unwrap();
        frame(&mock, |m| {
            m["type"] == "invokefunction" && m["function_id"] == refused_id.as_str()
        })
        .await;
        assert_eq!(ran.load(Ordering::SeqCst), 0, "{name}");
        iii.shutdown_async().await;
    }
}

/// On a 0.25 engine, a deliver hook set on a confirmed function withdraws
/// its registration: local dispatch turns off, its observers' removed hooks
/// run once, and the engine's answer to the withdrawn registration leaves it
/// unregistered. Removing the hook registers it again.
#[tokio::test]
async fn a_withdrawn_registration_is_released() {
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "database");
    frame(&mock, |m| m["function_id"] == "engine::workers::register").await;
    hello_with(
        &mock,
        fixture("workerregistered-v1.json")["features"].clone(),
    );
    until("the hello", || iii.engine_supports_observe() == Some(true)).await;
    let recorder = Recorder::default();
    set_hooks(&iii, "wd::f", &recorder);
    let ran = Arc::new(AtomicUsize::new(0));
    let function = hosted(
        &mock,
        &iii,
        "wd::f",
        RegisterFunction::new({
            let ran = ran.clone();
            move |input: Value| {
                ran.fetch_add(1, Ordering::SeqCst);
                Ok::<_, Error>(input)
            }
        }),
        json!([view("o", json!({}), json!(["input"]), "success")]),
    )
    .await;
    acked(&mock, &[("o", 1)]).await;
    until("observed", || function.is_observed()).await;

    function.on_observer_deliver(|_call: Call, _observers: Vec<Observer>| async move {
        Ok(json!("from the deliver hook"))
    });
    frame(&mock, |m| {
        m["type"] == "unregisterfunction" && m["id"] == "wd::f"
    })
    .await;
    assert!(!function.local_dispatch_active());
    until("released", || !function.is_observed()).await;
    until("removed", || {
        recorder
            .events
            .lock()
            .unwrap()
            .contains(&"remove o".to_string())
    })
    .await;

    // the engine's answer to the withdrawn registration, and an observer it
    // announced before the withdrawal reached it, change nothing; the answer
    // to a later call shows they were read
    mock.clear();
    confirm(
        &mock,
        "wd::f",
        2,
        true,
        json!([view("o2", json!({}), json!(["input"]), "success")]),
    );
    mock.send_to_client(added("wd::f", 3, "o3", json!({})));
    let in_flight = tokio::spawn({
        let iii = iii.clone();
        async move { iii.call(CallRequest::new("remote::f", json!({}))).await }
    });
    let call = frame(&mock, |m| {
        m["type"] == "invokefunction" && m["function_id"] == "remote::f"
    })
    .await;
    mock.send_to_client(json!({
        "type": "invocationresult", "invocation_id": call["invocation_id"],
        "function_id": "remote::f", "result": true
    }));
    tokio::time::timeout(WAIT, in_flight)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;
    iii.call(CallRequest {
        function_id: "wd::f".into(),
        payload: json!({}),
        action: Some(CallAction::Void),
        timeout_ms: None,
    })
    .await
    .unwrap();
    frame(&mock, |m| {
        m["type"] == "invokefunction" && m["function_id"] == "wd::f"
    })
    .await;
    assert_eq!(ran.load(Ordering::SeqCst), 0, "no call runs here");
    assert!(!function.local_dispatch_active());
    assert!(!function.is_observed());
    assert!(acks(&mock).is_empty(), "o2 and o3 are not acked");
    assert_eq!(
        *recorder.events.lock().unwrap(),
        vec!["add o", "remove o"],
        "removed once"
    );

    // without the deliver hook it registers again, and the engine's answer
    // turns local dispatch back on
    function.clear_observer_deliver();
    frame(&mock, registered("wd::f")).await;
    confirm(&mock, "wd::f", 3, true, json!([]));
    until("local dispatch", || function.local_dispatch_active()).await;
    iii.shutdown_async().await;
}

/// A standby registers again about every second while another host holds
/// the id, and runs its hooks once the engine confirms it.
#[tokio::test]
async fn a_standby_takes_over_when_the_host_leaves() {
    let mock = MockEngine::start().await;
    let iii = worker(&mock, "cron-2");
    connected(&mock, &iii, true).await;
    let recorder = Recorder::default();
    set_hooks(&iii, "solo::f", &recorder);
    let function = iii.register_function("solo::f", echo().standby());
    let plain = iii.register_function("solo::plain", echo());
    frame(&mock, registered("solo::f")).await;
    frame(&mock, registered("solo::plain")).await;
    let conflict = |id: &str| {
        json!({ "type": "registrationrejected", "code": "FUNCTION_NAMESPACE_CONFLICT",
                "namespace": "default", "function_id": id, "owner_worker_id": "w-host" })
    };
    mock.clear();
    let started = Instant::now();
    mock.send_to_client(conflict("solo::f"));
    mock.send_to_client(conflict("solo::plain"));
    frame(&mock, registered("solo::f")).await;
    let first = started.elapsed();
    assert!(
        first >= Duration::from_millis(700),
        "about a second later: {first:?}"
    );
    mock.clear();
    mock.send_to_client(conflict("solo::f"));
    frame(&mock, registered("solo::f")).await;

    // the host left: the engine accepts it, with the function's observers
    confirm(
        &mock,
        "solo::f",
        1,
        true,
        json!([view("kept", json!({}), json!(["input"]), "success")]),
    );
    acked(&mock, &[("kept", 1)]).await;
    until("taken over", || function.local_dispatch_active()).await;
    assert_eq!(*recorder.events.lock().unwrap(), vec!["add kept"]);
    tokio::time::sleep(Duration::from_millis(1_500)).await;
    assert_eq!(
        frames(&mock, "registerfunction")
            .iter()
            .filter(|m| m["id"] == "solo::f")
            .count(),
        1,
        "no retry once confirmed"
    );
    assert!(
        frames(&mock, "registerfunction")
            .iter()
            .all(|m| m["id"] != "solo::plain"),
        "only a standby registers again"
    );
    assert!(!plain.local_dispatch_active());
    iii.shutdown_async().await;
}
