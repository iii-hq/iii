//! Engine-hosted sources over real WebSockets: `engine::on-functions-changed`,
//! `engine::on-workers-changed` and `engine::on-observers-changed` are
//! observed like any function, and the engine calls them once per change of
//! its functions, workers or observers.

mod observe_support;

use iii::observer::ObserversConfig;
use observe_support::{Client, TestEngine, connect, eventually, registered, spawn_engine};
use serde_json::{Value, json};

async fn host(t: &TestEngine, name: &str, functions: &[&str]) -> Client {
    let mut client = connect(t, name, true).await;
    for function in functions {
        client.register_function(function).await;
    }
    for function in functions {
        registered(&t.engine, "default", function).await;
    }
    client
}

fn observer(id: &str, source: Value, target: &str) -> Value {
    json!({ "id": id, "source": source, "target": { "function_id": target } })
}

#[tokio::test]
async fn on_functions_changed_lists_the_functions_after_a_change() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let mut sink = host(&t, "sink", &["sink::functions"]).await;
    sink.observe_active(observer(
        "obs-functions",
        json!({ "function_id": "engine::on-functions-changed", "deliver": "input" }),
        "sink::functions",
    ))
    .await;

    let _orders = host(&t, "orders", &["orders::new"]).await;
    // Earlier changes may be coalesced into an event of their own; wait for
    // the one that lists the new function.
    loop {
        let activation = sink.invoked("sink::functions").await;
        sink.reply(&activation, Value::Null).await;
        let event = &activation["data"];
        assert_eq!(event["event"], "functions_changed");
        let listed = event["functions"]
            .as_array()
            .expect("functions")
            .iter()
            .any(|f| f["function_id"] == "orders::new" && f["worker_name"] == "orders");
        if listed {
            break;
        }
    }
}

#[tokio::test]
async fn on_workers_changed_reports_a_worker_announcing_itself() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let mut sink = host(&t, "sink", &["sink::workers"]).await;
    sink.observe_active(observer(
        "obs-workers",
        json!({
            "function_id": "engine::on-workers-changed",
            "deliver": "input",
            "filters": { "input.event": { "$eq": "worker_metadata_updated" } }
        }),
        "sink::workers",
    ))
    .await;

    let newcomer = connect(&t, "newcomer", true).await;
    let activation = sink.invoked("sink::workers").await;
    assert_eq!(
        activation["data"],
        json!({ "event": "worker_metadata_updated", "worker_id": newcomer.worker_id.to_string() })
    );
    assert_eq!(activation["caller"]["worker_name"], "iii-engine");
    sink.reply(&activation, Value::Null).await;
}

/// The next activation of `sink::changes`, answered.
async fn next_change(sink: &mut Client) -> Value {
    let activation = sink.invoked("sink::changes").await;
    sink.reply(&activation, Value::Null).await;
    activation
}

/// One call of `engine::on-observers-changed` per observer creation, with
/// the state it starts in, and per state change after it (decision 6), with
/// the `reason` when there is one; the engine is its caller.
#[tokio::test]
async fn on_observers_changed_reports_each_state_change() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let mut sink = host(&t, "sink", &["sink::changes", "sink::orders"]).await;
    sink.observe_active(observer(
        "obs-changes",
        json!({
            "function_id": "engine::on-observers-changed",
            "deliver": "input",
            "filters": { "input.observer_id": { "$eq": "watched" } }
        }),
        "sink::changes",
    ))
    .await;
    let orders = host(&t, "orders", &["orders::create"]).await;
    let mut watcher = connect(&t, "watcher", true).await;
    watcher
        .observe_active(observer(
            "watched",
            json!({ "function_id": "orders::create" }),
            "sink::orders",
        ))
        .await;

    // Created pending: its host has not acked it yet.
    let created = next_change(&mut sink).await;
    assert_eq!(created["data"]["state"], "pending");
    assert!(created["data"].get("reason").is_none(), "{created}");
    let active = next_change(&mut sink).await;
    assert_eq!(
        active["data"],
        json!({
            "observer_id": "watched",
            "function_id": "orders::create",
            "target": "sink::orders",
            "state": "active"
        })
    );
    assert_eq!(active["caller"]["worker_name"], "iii-engine");

    orders.disconnect().await;
    let pending = next_change(&mut sink).await;
    assert_eq!(pending["data"]["state"], "pending");
    assert_eq!(pending["data"]["reason"]["code"], "OWNER_UNAVAILABLE");

    watcher
        .send(json!({ "type": "unobserve", "id": "watched" }))
        .await;
    let disposed = next_change(&mut sink).await;
    assert_eq!(disposed["data"]["state"], "disposed");
    assert_eq!(disposed["data"]["reason"]["code"], "UNOBSERVED");
}

#[tokio::test]
async fn an_event_source_is_observed_not_called() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let mut sink = host(&t, "sink", &["sink::any"]).await;

    // Its `request_format` is the event: a path it does not have is refused.
    let typo = sink
        .observe(observer(
            "obs-typo",
            json!({
                "function_id": "engine::on-workers-changed",
                "filters": { "input.wroker_id": { "$eq": "x" } }
            }),
            "sink::any",
        ))
        .await;
    assert_eq!(typo["error"]["code"], "INVALID_FILTER_PATH", "{typo}");

    sink.observe_active(observer(
        "obs-every",
        json!({ "function_id": "engine::on-workers-changed", "on": "complete" }),
        "sink::any",
    ))
    .await;
    let call = sink
        .call(
            "engine::on-workers-changed",
            json!({ "worker_id": "forged" }),
        )
        .await;
    assert_eq!(sink.result(call).await["error"]["code"], "FORBIDDEN");
    assert_eq!(
        t.engine.observers.fires("obs-every"),
        Some(0),
        "a call is not one of its events"
    );
}

/// The engine hosts its sources (v2 §7): a worker registering one's id is
/// refused, the source keeps refusing calls, and its observers keep getting
/// the engine's events, before and after that worker leaves.
#[tokio::test]
async fn a_worker_cannot_host_an_engine_hosted_source() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let mut sink = host(&t, "sink", &["sink::workers"]).await;
    sink.observe_active(observer(
        "obs-connects",
        json!({
            "function_id": "engine::on-workers-changed",
            "deliver": "input",
            "filters": { "input.event": { "$eq": "worker_connected" } }
        }),
        "sink::workers",
    ))
    .await;

    let mut hijacker = connect(&t, "hijacker", true).await;
    hijacker
        .register_function("engine::on-workers-changed")
        .await;
    let rejected = hijacker
        .expect("registrationrejected", |f| {
            f["type"] == "registrationrejected" && f["function_id"] == "engine::on-workers-changed"
        })
        .await;
    assert_eq!(
        rejected["code"], "FUNCTION_NAMESPACE_CONFLICT",
        "{rejected}"
    );
    assert_eq!(rejected["owner_worker_id"], "", "no worker owns it");

    let mut caller = connect(&t, "caller", true).await;
    let call = caller
        .call(
            "engine::on-workers-changed",
            json!({ "event": "worker_connected", "worker_id": "forged" }),
        )
        .await;
    assert_eq!(caller.result(call).await["error"]["code"], "FORBIDDEN");
    assert!(
        !hijacker.has_seen(|f| {
            f["type"] == "functionregistered"
                || (f["type"] == "invokefunction"
                    && f["function_id"] == "engine::on-workers-changed")
        }),
        "the hijacker hosts nothing"
    );

    let hijacker_id = hijacker.worker_id;
    hijacker.disconnect().await;
    assert!(
        eventually(|| !t.engine.worker_registry.workers.contains_key(&hijacker_id)).await,
        "the hijacker is gone"
    );
    assert!(
        t.engine
            .functions
            .get("default", "engine::on-workers-changed")
            .is_some()
    );
    let visitor = connect(&t, "visitor", true).await;
    let visitor_id = visitor.worker_id.to_string();
    loop {
        let event = sink.invoked("sink::workers").await;
        sink.reply(&event, Value::Null).await;
        assert_ne!(event["data"]["worker_id"], "forged", "{event}");
        if event["data"]["worker_id"] == visitor_id.as_str() {
            break;
        }
    }
    assert_eq!(
        t.engine.observers.get("obs-connects").map(|o| o.state()),
        Some(iii::protocol::ObserverState::Active)
    );
}

#[tokio::test]
async fn on_workers_changed_reports_connects_and_disconnects() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let mut sink = host(&t, "sink", &["sink::workers"]).await;
    sink.observe_active(observer(
        "obs-lifecycle",
        json!({
            "function_id": "engine::on-workers-changed",
            "deliver": "input",
            "filters": { "input.event": { "$ne": "worker_metadata_updated" } }
        }),
        "sink::workers",
    ))
    .await;

    let visitor = connect(&t, "visitor", true).await;
    let id = visitor.worker_id.to_string();
    let connected = sink.invoked("sink::workers").await;
    assert_eq!(
        connected["data"],
        json!({ "event": "worker_connected", "worker_id": id })
    );
    sink.reply(&connected, Value::Null).await;

    visitor.disconnect().await;
    let disconnected = sink.invoked("sink::workers").await;
    assert_eq!(
        disconnected["data"],
        json!({ "event": "worker_disconnected", "worker_id": id })
    );
    sink.reply(&disconnected, Value::Null).await;
}

/// A target that registers a function on every activation (inside its
/// chain) stops at the depth limit: the burst's event has the depth of the
/// registrations in it.
#[tokio::test]
async fn an_on_functions_changed_target_that_registers_stops_at_the_depth_limit() {
    use iii::engine::{EngineTrait, Handler, RegisterFunctionRequest};
    use iii::function::FunctionResult;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let t = spawn_engine(ObserversConfig {
        max_observe_depth: 2,
        ..ObserversConfig::default()
    })
    .await;
    let calls = std::sync::Arc::new(AtomicUsize::new(0));
    {
        let engine = t.engine.clone();
        let calls = calls.clone();
        t.engine.register_function_handler(
            RegisterFunctionRequest {
                function_id: "test::grow".to_string(),
                description: None,
                request_format: None,
                response_format: None,
                metadata: None,
            },
            Handler::new(move |_input: Value| {
                let engine = engine.clone();
                let calls = calls.clone();
                async move {
                    let n = calls.fetch_add(1, Ordering::SeqCst);
                    engine.register_function_handler(
                        RegisterFunctionRequest {
                            function_id: format!("test::grown-{n}"),
                            description: None,
                            request_format: None,
                            response_format: None,
                            metadata: None,
                        },
                        Handler::new(|_: Value| async { FunctionResult::Success(None) }),
                    );
                    FunctionResult::Success(None)
                }
            }),
        );
    }
    let mut watcher = connect(&t, "watcher", true).await;
    watcher
        .observe_active(observer(
            "obs-grow",
            json!({ "function_id": "engine::on-functions-changed", "capture": [] }),
            "test::grow",
        ))
        .await;
    let _kick = host(&t, "kick", &["kick::fn"]).await;

    assert!(
        eventually(|| t
            .engine
            .observers
            .get_info(&t.engine, None, "obs-grow")
            .ok()
            .and_then(|info| info.dropped.get("LOOP_GUARD").copied())
            == Some(1))
        .await,
        "the loop guard stops it"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}
