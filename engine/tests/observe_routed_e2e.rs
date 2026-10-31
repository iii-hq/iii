//! Observers on the routed path (README §7 phase 1), over real WebSockets:
//! every call here goes caller → engine → host, and the engine evaluates the
//! observers' filters itself: the hosts here declare `observe.v1`, not
//! `observe.v2` (`observe_host_delivery_e2e` covers hosts that deliver their
//! own calls).
//!
//! Hosts that declare `observe.v1` ack their observers (the test client does
//! it like an SDK whose hooks accept everything), so an observer goes
//! `active` once its owner acked it; `observe_active` waits for that.
//!
//! The iii lang chapter on observation
//! (`spec/20-function-observation-and-reactive-semantics.md`) is covered by the
//! `lang_*` tests: a static observation (§1), the observation's identity and
//! `close()` (§4), the default payload being the output and the `.error`,
//! `.invocation` and `.completion` facets (§5).

mod observe_support;

use iii::observer::ObserversConfig;
use iii::protocol::ObserverState;
use observe_support::{
    Client, Setup, TestEngine, connect, connect_query, evaluated, eventually, in_process,
    registered, spawn, spawn_engine, spawn_engine_with, unrestricted_listener,
};
use serde_json::{Value, json};
use std::time::Duration;
use uuid::Uuid;

const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../sdk/fixtures/protocol/observe"
);

fn fixture_keys(name: &str) -> Vec<String> {
    let text = std::fs::read_to_string(format!("{FIXTURES}/{name}.json")).expect("fixture");
    let value: Value = serde_json::from_str(&text).expect("json");
    value.as_object().unwrap().keys().cloned().collect()
}

/// A host for `functions`, connected and registered.
async fn host(t: &TestEngine, name: &str, features: bool, functions: &[&str]) -> Client {
    let mut client = connect(t, name, features).await;
    for function in functions {
        client.register_function(function).await;
    }
    for function in functions {
        registered(&t.engine, "default", function).await;
    }
    client
}

fn fires(t: &TestEngine, id: &str) -> u64 {
    t.engine.observers.fires(id).unwrap_or_default()
}

fn info(t: &TestEngine, id: &str) -> Option<iii::observer::builtins::ObserverInfo> {
    t.engine.observers.get_info(&t.engine, None, id).ok()
}

fn dropped(t: &TestEngine, id: &str, reason: &str) -> u64 {
    info(t, id)
        .and_then(|info| info.dropped.get(reason).copied())
        .unwrap_or_default()
}

fn failed(t: &TestEngine, id: &str, reason: &str) -> u64 {
    info(t, id)
        .and_then(|info| info.activation_failures.get(reason).copied())
        .unwrap_or_default()
}

fn state(t: &TestEngine, id: &str) -> Option<ObserverState> {
    t.engine.observers.get(id).map(|o| o.state())
}

/// One call of `function_id` from `caller`, hosted by `host`, answered with
/// `result`. Returns once the caller has the result and, when the function is
/// observed, once the engine evaluated its observers (which happens after the
/// reply): from then on `fires()` and `dropped()` are settled.
async fn round_trip(
    t: &TestEngine,
    caller: &mut Client,
    host: &mut Client,
    function_id: &str,
    data: Value,
    result: Value,
) -> Value {
    let observed = t.engine.observers.watch("default", function_id).is_some();
    let before = t.engine.observers.evaluated();
    let id = caller.call(function_id, data).await;
    let invoke = host.serve(function_id, result).await;
    caller.result(id).await;
    if observed {
        evaluated(&t.engine, before + 1).await;
    }
    invoke
}

fn observer(id: &str, source: Value, target: &str) -> Value {
    json!({ "id": id, "source": source, "target": { "function_id": target } })
}

#[tokio::test]
async fn lang_static_observation_activates_the_target_with_the_envelope() {
    // payments::charge ~> audit::record, with a filter on the input.
    let t = spawn_engine(ObserversConfig::default()).await;
    let mut payments = host(&t, "payments", true, &["payments::charge"]).await;
    let mut audit = host(&t, "audit", true, &["audit::record"]).await;
    let mut checkout = connect(&t, "checkout", true).await;

    let mut frame = observer(
        "obs-charge",
        json!({ "function_id": "payments::charge", "filters": { "input.amount": { "$gte": 500 } } }),
        "audit::record",
    );
    frame["metadata"] = json!({ "team": "risk" });
    let result = audit.observe_active(frame).await;
    assert!(result.get("error").is_none());

    // The filter rejects this one: nothing fires.
    round_trip(
        &t,
        &mut checkout,
        &mut payments,
        "payments::charge",
        json!({ "amount": 100 }),
        json!({ "charged": 100 }),
    )
    .await;
    assert_eq!(fires(&t, "obs-charge"), 0);

    // A matching call, made from inside a handler (the SDK asserts its
    // function and invocation; the engine stamps the worker).
    let call = checkout
        .call_with(
            "payments::charge",
            json!({ "amount": 700 }),
            json!({ "caller": {
                "worker_name": "forged",
                "function_id": "checkout::confirm",
                "invocation_id": "inv-confirm"
            } }),
        )
        .await;
    let forwarded = payments.invoked("payments::charge").await;
    assert_eq!(
        forwarded["caller"]["worker_name"], "checkout",
        "D9: stamped"
    );
    assert_eq!(
        forwarded["caller"]["worker_id"],
        checkout.worker_id.to_string()
    );
    assert_eq!(forwarded["caller"]["function_id"], "checkout::confirm");
    assert!(
        forwarded.get("observe_depth").is_none(),
        "depth 0 is omitted"
    );
    assert!(forwarded.get("activation").is_none());
    payments
        .reply(&forwarded, json!({ "charged": 700, "status": "paid" }))
        .await;
    checkout.result(call).await;

    let activation = audit.invoked("audit::record").await;
    let envelope = &activation["data"];
    assert_eq!(envelope["observer_id"], "obs-charge");
    assert_eq!(envelope["function_id"], "payments::charge");
    assert_eq!(envelope["namespace"], "default");
    assert_eq!(envelope["status"], "success");
    assert_eq!(
        envelope["input"],
        json!({ "amount": 700 }),
        "as sent: no engine stamp"
    );
    assert_eq!(
        envelope["output"],
        json!({ "charged": 700, "status": "paid" })
    );
    assert_eq!(envelope["caller"]["worker_name"], "checkout");
    assert_eq!(envelope["caller"]["function_id"], "checkout::confirm");
    assert!(envelope["started_at_ms"].as_u64().is_some());
    assert!(envelope["duration_ms"].as_f64().is_some());
    assert_eq!(
        envelope["invocation_id"], forwarded["invocation_id"],
        "the observed invocation"
    );

    assert_eq!(activation["metadata"], json!({ "team": "risk" }), "sidecar");
    assert_eq!(activation["observe_depth"], 1);
    assert_eq!(activation["activation"]["observer_id"], "obs-charge");
    assert_eq!(
        activation["activation"]["activation_id"],
        envelope["activation_id"]
    );
    assert_eq!(
        activation["caller"],
        json!({
            "worker_name": "iii-engine",
            "function_id": "payments::charge",
            "invocation_id": forwarded["invocation_id"],
        })
    );
    // The frame is the one `invokefunction-activation.json` documents: an
    // ordinary forwarded call (with an invocation id, without `action`).
    let allowed = fixture_keys("invokefunction-activation");
    for key in activation.as_object().unwrap().keys() {
        assert!(
            allowed.contains(key),
            "unexpected key `{key}` in {activation}"
        );
    }
    assert!(activation.get("invocation_id").is_some() && activation.get("action").is_none());
    audit.reply(&activation, Value::Null).await;
    assert_eq!(fires(&t, "obs-charge"), 1);
}

#[tokio::test]
async fn a_worker_without_observe_v1_gets_no_new_fields() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let mut legacy = host(&t, "legacy", false, &["legacy::fn"]).await;
    let mut caller = connect(&t, "caller", true).await;

    let call = caller
        .call_with(
            "legacy::fn",
            json!({}),
            json!({ "caller": { "function_id": "caller::fn" }, "observe_depth": 2 }),
        )
        .await;
    let forwarded = legacy.invoked("legacy::fn").await;
    for key in ["caller", "observe_depth", "activation"] {
        assert!(
            forwarded.get(key).is_none(),
            "`{key}` sent to an old worker: {forwarded}"
        );
    }
    legacy.reply(&forwarded, json!(true)).await;
    caller.result(call).await;
}

#[tokio::test]
async fn filters_on_output_and_caller() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let mut billing = host(&t, "billing", true, &["billing::close-invoice"]).await;
    let mut sink = host(&t, "sink", true, &["sink::paid"]).await;
    let mut checkout = connect(&t, "checkout", true).await;
    let mut replayer = connect(&t, "replayer", true).await;

    sink.observe_active(observer(
        "obs-paid",
        json!({
            "function_id": "billing::close-invoice",
            "filters": {
                "output.status": { "$eq": "paid" },
                "caller.worker_name": { "$ne": "replayer" }
            }
        }),
        "sink::paid",
    ))
    .await;

    // Output does not match; then the caller does not.
    round_trip(
        &t,
        &mut checkout,
        &mut billing,
        "billing::close-invoice",
        json!({}),
        json!({ "status": "open" }),
    )
    .await;
    round_trip(
        &t,
        &mut replayer,
        &mut billing,
        "billing::close-invoice",
        json!({}),
        json!({ "status": "paid" }),
    )
    .await;
    assert_eq!(fires(&t, "obs-paid"), 0);

    round_trip(
        &t,
        &mut checkout,
        &mut billing,
        "billing::close-invoice",
        json!({}),
        json!({ "status": "paid" }),
    )
    .await;
    let activation = sink.invoked("sink::paid").await;
    assert_eq!(activation["data"]["output"]["status"], "paid");
    assert_eq!(activation["data"]["caller"]["worker_name"], "checkout");
    sink.reply(&activation, Value::Null).await;
    assert_eq!(fires(&t, "obs-paid"), 1);
}

#[tokio::test]
async fn lang_error_and_completion_facets() {
    // payments::charge.error ~> alerts::raise, and .completion ~> metrics::record.
    let t = spawn_engine(ObserversConfig::default()).await;
    let mut payments = host(&t, "payments", true, &["payments::charge"]).await;
    let mut sink = host(&t, "sink", true, &["alerts::raise", "metrics::record"]).await;
    let mut checkout = connect(&t, "checkout", true).await;

    sink.observe_active(observer(
        "obs-error",
        json!({ "function_id": "payments::charge", "on": "error", "capture": ["input", "error"] }),
        "alerts::raise",
    ))
    .await;
    sink.observe_active(observer(
        "obs-complete",
        json!({ "function_id": "payments::charge", "on": "complete", "capture": [] }),
        "metrics::record",
    ))
    .await;

    round_trip(
        &t,
        &mut checkout,
        &mut payments,
        "payments::charge",
        json!({ "amount": 1 }),
        json!({ "ok": true }),
    )
    .await;
    let completion = sink.invoked("metrics::record").await;
    assert_eq!(completion["data"]["status"], "success");
    for key in ["input", "output", "error", "caller", "metadata"] {
        assert!(
            completion["data"].get(key).is_none(),
            "capture: [] ships ids, status and timings only (D14): {completion}"
        );
    }
    assert!(completion["data"]["duration_ms"].as_f64().is_some());
    sink.reply(&completion, Value::Null).await;
    assert_eq!(fires(&t, "obs-error"), 0, "on: error ignores a success");

    let call = checkout
        .call("payments::charge", json!({ "amount": 2 }))
        .await;
    let invoke = payments.invoked("payments::charge").await;
    payments
        .reply_error(&invoke, "CARD_DECLINED", "declined")
        .await;
    checkout.result(call).await;

    let alert = sink.invoked("alerts::raise").await;
    assert_eq!(alert["data"]["status"], "error");
    assert_eq!(alert["data"]["error"]["code"], "CARD_DECLINED");
    assert_eq!(alert["data"]["input"], json!({ "amount": 2 }));
    assert!(alert["data"].get("output").is_none());
    sink.reply(&alert, Value::Null).await;
    let completion = sink.invoked("metrics::record").await;
    assert_eq!(completion["data"]["status"], "error");
    sink.reply(&completion, Value::Null).await;
}

#[tokio::test]
async fn lang_default_payload_and_invocation_facet() {
    // orders::complete ~> (order) => ...: the target gets the output itself;
    // orders::complete.invocation ~> ...: the full envelope.
    let t = spawn_engine(ObserversConfig::default()).await;
    let mut orders = host(&t, "orders", true, &["orders::complete"]).await;
    let mut sink = host(
        &t,
        "sink",
        true,
        &["receipts::render", "audit::write", "audit::input"],
    )
    .await;
    let mut checkout = connect(&t, "checkout", true).await;

    sink.observe_active(observer(
        "obs-output",
        json!({ "function_id": "orders::complete", "deliver": "output" }),
        "receipts::render",
    ))
    .await;
    sink.observe_active(observer(
        "obs-input",
        json!({ "function_id": "orders::complete", "deliver": "input" }),
        "audit::input",
    ))
    .await;
    sink.observe_active(observer(
        "obs-invocation",
        json!({
            "function_id": "orders::complete",
            "capture": ["input", "output", "error", "caller", "metadata"]
        }),
        "audit::write",
    ))
    .await;

    let call = checkout
        .call_with(
            "orders::complete",
            json!({ "order_id": "o-1" }),
            json!({ "metadata": { "session": "s-1" } }),
        )
        .await;
    orders
        .serve(
            "orders::complete",
            json!({ "order_id": "o-1", "total": 42 }),
        )
        .await;
    checkout.result(call).await;

    let render = sink.invoked("receipts::render").await;
    assert_eq!(render["data"], json!({ "order_id": "o-1", "total": 42 }));
    sink.reply(&render, Value::Null).await;
    let input = sink.invoked("audit::input").await;
    assert_eq!(
        input["data"],
        json!({ "order_id": "o-1" }),
        "deliver: input, as the caller sent it"
    );
    sink.reply(&input, Value::Null).await;
    let audit = sink.invoked("audit::write").await;
    let envelope = &audit["data"];
    assert_eq!(envelope["input"], json!({ "order_id": "o-1" }));
    assert_eq!(
        envelope["output"],
        json!({ "order_id": "o-1", "total": 42 })
    );
    assert_eq!(envelope["caller"]["worker_name"], "checkout");
    assert_eq!(envelope["metadata"], json!({ "session": "s-1" }));
    sink.reply(&audit, Value::Null).await;
}

#[tokio::test]
async fn lang_observation_identity_close_stops_activations() {
    // const watch = orders::complete ~> renderReceipt; watch.close()
    let t = spawn_engine(ObserversConfig::default()).await;
    let mut orders = host(&t, "orders", true, &["orders::complete"]).await;
    let mut sink = host(&t, "sink", true, &["receipts::render"]).await;
    let mut checkout = connect(&t, "checkout", true).await;

    sink.observe_active(observer(
        "watch",
        json!({ "function_id": "orders::complete" }),
        "receipts::render",
    ))
    .await;
    sink.send(json!({ "type": "unobserve", "id": "watch" }))
        .await;
    let disposed = sink.state_becomes("watch", "disposed").await;
    assert_eq!(disposed["reason"]["code"], "UNOBSERVED");
    assert!(t.engine.observers.get("watch").is_none());

    round_trip(
        &t,
        &mut checkout,
        &mut orders,
        "orders::complete",
        json!({}),
        json!({}),
    )
    .await;
    assert!(
        t.engine
            .observers
            .watch("default", "orders::complete")
            .is_none(),
        "nothing observes orders::complete any more"
    );
}

/// The observing worker unobserves, then observes again under the same id:
/// the new observer is created and fires. Only a replay of an observer
/// disposed without the worker asking answers with the disposal.
#[tokio::test]
async fn an_id_its_worker_unobserved_is_free_for_a_new_observer() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let mut orders = host(&t, "orders", true, &["orders::complete"]).await;
    let mut sink = host(&t, "sink", true, &["receipts::render"]).await;
    let mut checkout = connect(&t, "checkout", true).await;
    let frame = observer(
        "watch-again",
        json!({ "function_id": "orders::complete" }),
        "receipts::render",
    );

    sink.observe_active(frame.clone()).await;
    sink.send(json!({ "type": "unobserve", "id": "watch-again" }))
        .await;
    sink.state_becomes("watch-again", "disposed").await;
    assert!(t.engine.observers.get("watch-again").is_none());

    sink.observe_active(frame).await;
    assert_eq!(state(&t, "watch-again"), Some(ObserverState::Active));
    round_trip(
        &t,
        &mut checkout,
        &mut orders,
        "orders::complete",
        json!({ "order": 7 }),
        json!({}),
    )
    .await;
    let activation = sink.invoked("receipts::render").await;
    sink.reply(&activation, Value::Null).await;
    assert_eq!(fires(&t, "watch-again"), 1);
}

#[tokio::test]
async fn max_fires_disposes_after_the_last_fire() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let mut orders = host(&t, "orders", true, &["orders::create"]).await;
    let mut sink = host(&t, "sink", true, &["sink::once"]).await;
    let mut checkout = connect(&t, "checkout", true).await;

    let mut frame = observer(
        "obs-twice",
        json!({ "function_id": "orders::create" }),
        "sink::once",
    );
    frame["max_fires"] = json!(2);
    sink.observe_active(frame).await;
    for _ in 0..3 {
        round_trip(
            &t,
            &mut checkout,
            &mut orders,
            "orders::create",
            json!({}),
            json!({}),
        )
        .await;
    }
    for _ in 0..2 {
        let activation = sink.invoked("sink::once").await;
        sink.reply(&activation, Value::Null).await;
    }
    let disposed = sink.state_becomes("obs-twice", "disposed").await;
    assert_eq!(disposed["reason"]["code"], "MAX_FIRES");
    assert_eq!(disposed["fires"], 2);
    assert!(
        t.engine
            .observers
            .watch("default", "orders::create")
            .is_none(),
        "no third activation: the observer was disposed after its second fire"
    );
}

#[tokio::test]
async fn expiry_disposes_the_observer() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let _orders = host(&t, "orders", true, &["orders::create"]).await;
    let mut sink = host(&t, "sink", true, &["sink::expiring"]).await;
    let mut frame = observer(
        "obs-expiring",
        json!({ "function_id": "orders::create" }),
        "sink::expiring",
    );
    frame["expires_in_ms"] = json!(150);
    sink.observe(frame).await;
    let disposed = sink.state_becomes("obs-expiring", "disposed").await;
    assert_eq!(disposed["reason"]["code"], "EXPIRED");
}

#[tokio::test]
async fn in_flight_limit_queues_in_order_then_overflows() {
    let t = spawn_engine(ObserversConfig {
        max_queue_per_observer: 1,
        ..ObserversConfig::default()
    })
    .await;
    let mut orders = host(&t, "orders", true, &["orders::create"]).await;
    let mut sink = host(&t, "sink", true, &["sink::slow"]).await;
    let mut checkout = connect(&t, "checkout", true).await;

    let mut frame = observer(
        "obs-slow",
        json!({ "function_id": "orders::create", "capture": ["input"] }),
        "sink::slow",
    );
    frame["max_in_flight"] = json!(1);
    sink.observe_active(frame).await;
    for n in 1..=3 {
        round_trip(
            &t,
            &mut checkout,
            &mut orders,
            "orders::create",
            json!({ "n": n }),
            json!({}),
        )
        .await;
    }
    // 1 runs, 2 waits in the queue, 3 has nowhere to go.
    assert_eq!(dropped(&t, "obs-slow", "OVERFLOW"), 1);
    assert_eq!(fires(&t, "obs-slow"), 2);

    let first = sink.invoked("sink::slow").await;
    assert_eq!(first["data"]["input"]["n"], 1);
    assert_eq!(
        t.engine.observers.in_flight("obs-slow"),
        Some((1, 1)),
        "the second waits in the queue for the first to finish"
    );
    sink.reply(&first, Value::Null).await;
    let second = sink.invoked("sink::slow").await;
    assert_eq!(second["data"]["input"]["n"], 2, "arrival order");
    assert_eq!(t.engine.observers.in_flight("obs-slow"), Some((1, 0)));
    sink.reply(&second, Value::Null).await;
    assert!(eventually(|| t.engine.observers.in_flight("obs-slow") == Some((0, 0))).await);
}

/// A loop through a worker that does not speak `observe.v1` (it never
/// carries `observe_depth`): the engine takes the depth of the activation it
/// is running for the calls it makes meanwhile, so the guard still stops it.
#[tokio::test]
async fn the_loop_guard_holds_through_a_worker_without_observe_v1() {
    let t = spawn_engine(ObserversConfig {
        max_observe_depth: 2,
        ..ObserversConfig::default()
    })
    .await;
    let mut stepper = host(&t, "stepper", true, &["loop::step"]).await;
    let mut legacy = host(&t, "legacy", false, &["legacy::bounce"]).await;
    let mut caller = connect(&t, "caller", true).await;
    caller
        .observe_active(observer(
            "obs-bounce",
            json!({ "function_id": "loop::step" }),
            "legacy::bounce",
        ))
        .await;

    let first = caller.call("loop::step", json!({})).await;
    stepper.serve("loop::step", json!({})).await;
    caller.result(first).await;

    let mut pending = Vec::new();
    for expected_depth in 1..=2u64 {
        // An activation, answered only after the call it makes.
        let bounce = legacy.invoked("legacy::bounce").await;
        assert!(
            bounce.get("observe_depth").is_none(),
            "an old worker gets no depth"
        );
        let again = legacy.call("loop::step", json!({})).await;
        let step = stepper.serve("loop::step", json!({})).await;
        assert_eq!(
            step["observe_depth"], expected_depth,
            "inherited from the activation"
        );
        legacy.result(again).await;
        pending.push(bounce);
    }
    assert!(eventually(|| dropped(&t, "obs-bounce", "LOOP_GUARD") == 1).await);
    assert_eq!(fires(&t, "obs-bounce"), 2);
    for bounce in pending {
        legacy.reply(&bounce, Value::Null).await;
    }
}

#[tokio::test]
async fn a_target_that_never_answers_frees_its_slot_at_the_timeout() {
    let t = spawn_engine(ObserversConfig {
        activation_timeout_ms: 150,
        ..ObserversConfig::default()
    })
    .await;
    let mut orders = host(&t, "orders", true, &["orders::create"]).await;
    let mut sink = host(&t, "sink", true, &["sink::stuck"]).await;
    let mut checkout = connect(&t, "checkout", true).await;
    let mut frame = observer(
        "obs-stuck",
        json!({ "function_id": "orders::create" }),
        "sink::stuck",
    );
    frame["max_in_flight"] = json!(1);
    sink.observe_active(frame).await;

    round_trip(
        &t,
        &mut checkout,
        &mut orders,
        "orders::create",
        json!({}),
        json!({}),
    )
    .await;
    sink.invoked("sink::stuck").await; // never answered
    assert!(
        eventually(|| failed(&t, "obs-stuck", "TIMEOUT") == 1).await,
        "the activation times out"
    );
    let sink_id = sink.worker_id;
    let sink_conn = t.engine.worker_registry.get_worker(&sink_id).unwrap();
    assert_eq!(
        sink_conn.invocation_count().await,
        0,
        "the target stops tracking it"
    );

    // The slot is free: the next activation runs.
    round_trip(
        &t,
        &mut checkout,
        &mut orders,
        "orders::create",
        json!({}),
        json!({}),
    )
    .await;
    let next = sink.invoked("sink::stuck").await;
    sink.reply(&next, Value::Null).await;
}

#[tokio::test]
async fn a_target_that_disconnects_mid_activation_frees_its_slot() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let mut orders = host(&t, "orders", true, &["orders::create"]).await;
    let mut sink = host(&t, "sink", true, &["sink::flaky"]).await;
    let mut watcher = connect(&t, "watcher", true).await;
    let mut checkout = connect(&t, "checkout", true).await;
    let mut frame = observer(
        "obs-flaky",
        json!({ "function_id": "orders::create" }),
        "sink::flaky",
    );
    frame["max_in_flight"] = json!(1);
    watcher.observe_active(frame).await;

    round_trip(
        &t,
        &mut checkout,
        &mut orders,
        "orders::create",
        json!({}),
        json!({}),
    )
    .await;
    sink.invoked("sink::flaky").await;
    sink.disconnect().await;
    assert!(
        eventually(|| failed(&t, "obs-flaky", "TARGET_UNAVAILABLE") == 1).await,
        "the activation ends with its target"
    );

    let mut sink = host(&t, "sink-2", true, &["sink::flaky"]).await;
    watcher.state_becomes("obs-flaky", "active").await;
    round_trip(
        &t,
        &mut checkout,
        &mut orders,
        "orders::create",
        json!({}),
        json!({}),
    )
    .await;
    let next = sink.invoked("sink::flaky").await;
    sink.reply(&next, Value::Null).await;
}

#[tokio::test]
async fn loop_guard_drops_activations_past_the_max_depth() {
    // A target that calls back into what it observes: loop::step ~>
    // loop::bounce, and bounce calls step again, carrying its depth the way
    // an SDK does.
    let t = spawn_engine(ObserversConfig {
        max_observe_depth: 3,
        ..ObserversConfig::default()
    })
    .await;
    let mut worker = host(&t, "looper", true, &["loop::step", "loop::bounce"]).await;
    let mut caller = connect(&t, "caller", true).await;

    worker
        .observe_active(observer(
            "obs-loop",
            json!({ "function_id": "loop::step" }),
            "loop::bounce",
        ))
        .await;

    let first = caller.call("loop::step", json!({})).await;
    worker.serve("loop::step", json!({})).await;
    caller.result(first).await;

    for expected_depth in 1..=3u64 {
        let bounce = worker.invoked("loop::bounce").await;
        assert_eq!(bounce["observe_depth"], expected_depth);
        let depth = bounce["observe_depth"].clone();
        worker.reply(&bounce, Value::Null).await;
        let again = worker
            .call_with("loop::step", json!({}), json!({ "observe_depth": depth }))
            .await;
        worker.serve("loop::step", json!({})).await;
        worker.result(again).await;
    }
    // The step at depth 3 would activate at depth 4.
    assert!(eventually(|| dropped(&t, "obs-loop", "LOOP_GUARD") == 1).await);
    assert_eq!(fires(&t, "obs-loop"), 3);
}

#[tokio::test]
async fn a_target_keeps_the_observer_pending_until_it_registers() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let _orders = host(&t, "orders", true, &["orders::create"]).await;
    let mut sink = connect(&t, "sink", true).await;

    let result = sink
        .observe(observer(
            "obs-later",
            json!({ "function_id": "orders::create" }),
            "sink::later",
        ))
        .await;
    assert_eq!(result["state"], "pending");
    sink.register_function("sink::later").await;
    sink.state_becomes("obs-later", "active").await;

    // Unregistered again: suspended, and back when it returns.
    sink.send(json!({ "type": "unregisterfunction", "id": "sink::later" }))
        .await;
    let suspended = sink.state_becomes("obs-later", "suspended").await;
    assert_eq!(suspended["reason"]["code"], "TARGET_UNAVAILABLE");
    sink.register_function("sink::later").await;
    sink.state_becomes("obs-later", "active").await;
}

#[tokio::test]
async fn the_observed_function_going_away_parks_the_observer_until_it_returns() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let orders = host(&t, "orders", true, &["orders::create"]).await;
    let mut sink = host(&t, "sink", true, &["sink::orders"]).await;
    sink.observe_active(observer(
        "obs-owner",
        json!({ "function_id": "orders::create" }),
        "sink::orders",
    ))
    .await;

    orders.disconnect().await;
    let pending = sink.state_becomes("obs-owner", "pending").await;
    assert_eq!(pending["reason"]["code"], "OWNER_UNAVAILABLE");

    let mut orders_again = connect(&t, "orders-2", true).await;
    orders_again.register_function("orders::create").await;
    sink.state_becomes("obs-owner", "active").await;
}

#[tokio::test]
async fn the_observing_worker_reattaches_within_the_grace_period() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let mut orders = host(&t, "orders", true, &["orders::create"]).await;
    let mut sink = host(&t, "sink", true, &["sink::orders"]).await;
    let mut checkout = connect(&t, "checkout", true).await;
    let mut watcher = connect(&t, "watcher", true).await;

    let frame = observer(
        "obs-reattach",
        json!({ "function_id": "orders::create" }),
        "sink::orders",
    );
    watcher.observe_active(frame.clone()).await;
    round_trip(
        &t,
        &mut checkout,
        &mut orders,
        "orders::create",
        json!({}),
        json!({}),
    )
    .await;
    let activation = sink.invoked("sink::orders").await;
    sink.reply(&activation, Value::Null).await;
    assert_eq!(fires(&t, "obs-reattach"), 1);

    let (previous, token) = (watcher.worker_id, watcher.reattach_token.clone());
    watcher.disconnect().await;
    assert!(
        eventually(|| state(&t, "obs-reattach") == Some(ObserverState::Suspended)).await,
        "the observing worker left: suspended"
    );

    let mut watcher = Client::connect_with(t.port, "watcher", true, Some((previous, token))).await;
    let resumed = watcher.state_becomes("obs-reattach", "active").await;
    assert_eq!(resumed["fires"], 1, "counters intact");
    // The SDK replays its observers: a no-op answering the current state.
    assert_eq!(watcher.observe(frame).await["state"], "active");

    round_trip(
        &t,
        &mut checkout,
        &mut orders,
        "orders::create",
        json!({}),
        json!({}),
    )
    .await;
    let activation = sink.invoked("sink::orders").await;
    sink.reply(&activation, Value::Null).await;
    assert_eq!(fires(&t, "obs-reattach"), 2);
}

#[tokio::test]
async fn the_grace_period_ends_in_disposal() {
    let t = spawn_engine(ObserversConfig {
        observer_grace_ms: 100,
        ..ObserversConfig::default()
    })
    .await;
    let _orders = host(&t, "orders", true, &["orders::create"]).await;
    let _sink = host(&t, "sink", true, &["sink::orders"]).await;
    let mut watcher = connect(&t, "watcher", true).await;
    watcher
        .observe_active(observer(
            "obs-gone",
            json!({ "function_id": "orders::create" }),
            "sink::orders",
        ))
        .await;
    watcher.disconnect().await;
    assert!(
        eventually(|| t.engine.observers.get("obs-gone").is_none()).await,
        "disposed after the grace period"
    );
}

/// A replay of an observer disposed meanwhile answers the disposal instead of
/// re-creating it with fresh counters.
#[tokio::test]
async fn a_disposed_observer_is_not_resurrected_by_a_replay() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let mut orders = host(&t, "orders", true, &["orders::create"]).await;
    let mut sink = host(&t, "sink", true, &["sink::once"]).await;
    let mut checkout = connect(&t, "checkout", true).await;
    let mut frame = observer(
        "obs-once",
        json!({ "function_id": "orders::create" }),
        "sink::once",
    );
    frame["max_fires"] = json!(1);
    sink.observe_active(frame.clone()).await;
    round_trip(
        &t,
        &mut checkout,
        &mut orders,
        "orders::create",
        json!({}),
        json!({}),
    )
    .await;
    sink.state_becomes("obs-once", "disposed").await;

    let replay = sink.observe(frame).await;
    assert_eq!(replay["state"], "disposed");
    assert_eq!(replay["error"]["code"], "MAX_FIRES");
    assert!(t.engine.observers.get("obs-once").is_none());
}

/// A worker that comes back after `observer_grace_ms` reattaches and replays
/// its observers, as the SDKs do. The grace period disposed them, so the
/// replay is answered with that disposal: re-creating them with fresh
/// counters would let `max_fires` fire again and an observer outlive its
/// `expires_in_ms` (found by the 0.24.5-alpha.2 e2e run,
/// `repro/max-fires-after-grace.ts`).
#[tokio::test]
async fn a_worker_back_after_the_grace_period_does_not_resurrect_its_observers() {
    let t = spawn_engine(ObserversConfig {
        observer_grace_ms: 100,
        ..ObserversConfig::default()
    })
    .await;
    let mut orders = host(&t, "orders", true, &["orders::create"]).await;
    let mut sink = host(&t, "sink", true, &["sink::orders"]).await;
    let mut checkout = connect(&t, "checkout", true).await;
    let mut watcher = connect(&t, "watcher", true).await;

    let source = json!({ "function_id": "orders::create" });
    let mut capped = observer("obs-capped", source.clone(), "sink::orders");
    capped["max_fires"] = json!(3);
    let mut expiring = observer("obs-expiring", source, "sink::orders");
    expiring["expires_in_ms"] = json!(60_000);
    watcher.observe_active(capped.clone()).await;
    watcher.observe_active(expiring.clone()).await;
    for _ in 0..2 {
        round_trip(
            &t,
            &mut checkout,
            &mut orders,
            "orders::create",
            json!({}),
            json!({}),
        )
        .await;
        for _ in 0..2 {
            let activation = sink.invoked("sink::orders").await;
            sink.reply(&activation, Value::Null).await;
        }
    }
    assert_eq!(fires(&t, "obs-capped"), 2, "one fire left before max_fires");

    let (previous, token) = (watcher.worker_id, watcher.reattach_token.clone());
    watcher.disconnect().await;
    assert!(
        eventually(|| t.engine.observers.get("obs-capped").is_none()
            && t.engine.observers.get("obs-expiring").is_none())
        .await,
        "disposed when the grace period ended"
    );
    tokio::time::sleep(Duration::from_millis(250)).await;

    let mut watcher = Client::connect_with(t.port, "watcher", true, Some((previous, token))).await;
    for frame in [capped, expiring] {
        let replay = watcher.observe(frame).await;
        assert_eq!(replay["state"], "disposed", "{replay}");
        assert_eq!(replay["error"]["code"], "OBSERVER_DISCONNECTED");
    }
    assert!(t.engine.observers.get("obs-capped").is_none());
    assert!(t.engine.observers.get("obs-expiring").is_none());

    for _ in 0..5 {
        round_trip(
            &t,
            &mut checkout,
            &mut orders,
            "orders::create",
            json!({}),
            json!({}),
        )
        .await;
    }
    assert!(
        t.engine
            .observers
            .watch("default", "orders::create")
            .is_none(),
        "nothing observes orders::create any more"
    );
    assert!(!sink.has_seen(|frame| frame["function_id"] == "sink::orders"));
}

/// Observer ids are client-generated: a worker that cannot prove it is the
/// one that left (a restarted process, with no reattach token) creates its
/// observers again under the same ids.
#[tokio::test]
async fn a_restarted_worker_creates_its_observers_again_with_the_same_ids() {
    let t = spawn_engine(ObserversConfig {
        observer_grace_ms: 100,
        ..ObserversConfig::default()
    })
    .await;
    let _orders = host(&t, "orders", true, &["orders::create"]).await;
    let _sink = host(&t, "sink", true, &["sink::orders"]).await;
    let mut watcher = connect(&t, "watcher", true).await;
    let frame = observer(
        "obs-stable-id",
        json!({ "function_id": "orders::create" }),
        "sink::orders",
    );
    watcher.observe_active(frame.clone()).await;
    let previous = watcher.worker_id;
    watcher.disconnect().await;
    assert!(eventually(|| t.engine.observers.get("obs-stable-id").is_none()).await);

    // A wrong token proves nothing: no reattach, and the record stays.
    let mut restarted = Client::connect_with(
        t.port,
        "watcher",
        true,
        Some((previous, Uuid::new_v4().to_string())),
    )
    .await;
    restarted.observe_active(frame).await;
    assert_eq!(
        info(&t, "obs-stable-id").unwrap().worker.worker_id,
        restarted.worker_id.to_string()
    );
}

/// After `observers.disposed_retention_ms` the engine has forgotten the
/// disposal: a worker that comes back even later replays its observers as
/// new ones, as after an engine restart.
#[tokio::test]
async fn past_the_retention_a_late_worker_replays_its_observers_as_new() {
    let t = spawn_engine(ObserversConfig {
        observer_grace_ms: 100,
        disposed_retention_ms: 100,
        ..ObserversConfig::default()
    })
    .await;
    let _orders = host(&t, "orders", true, &["orders::create"]).await;
    let _sink = host(&t, "sink", true, &["sink::orders"]).await;
    let mut watcher = connect(&t, "watcher", true).await;
    let frame = observer(
        "obs-forgotten",
        json!({ "function_id": "orders::create" }),
        "sink::orders",
    );
    watcher.observe_active(frame.clone()).await;
    let (previous, token) = (watcher.worker_id, watcher.reattach_token.clone());
    watcher.disconnect().await;
    assert!(eventually(|| t.engine.observers.get("obs-forgotten").is_none()).await);
    // Past the grace period's tombstones and the retention.
    tokio::time::sleep(Duration::from_millis(400)).await;

    let mut watcher = Client::connect_with(t.port, "watcher", true, Some((previous, token))).await;
    watcher.observe_active(frame).await;
}

#[tokio::test]
async fn observe_rejections() {
    let t = spawn_engine(ObserversConfig {
        max_per_connection: 3,
        ..ObserversConfig::default()
    })
    .await;
    let mut orders = connect(&t, "orders", true).await;
    orders.register_function("orders::create").await;
    orders
        .register_function_with(json!({
            "type": "registerfunction",
            "id": "orders::typed",
            "request_format": {
                "type": "object", "additionalProperties": false,
                "properties": { "table": { "type": "string" } }
            }
        }))
        .await;
    orders
        .register_function_with(json!({
            "type": "registerfunction",
            "id": "vault::read",
            "observe": { "allow": false }
        }))
        .await;
    orders
        .register_function_with(json!({
            "type": "registerfunction",
            "id": "vault::secret",
            "observable": false
        }))
        .await;
    registered(&t.engine, "default", "vault::read").await;
    registered(&t.engine, "default", "vault::secret").await;
    let mut watcher = connect(&t, "watcher", true).await;

    let source = |extra: Value| {
        let mut source = json!({ "function_id": "orders::create" });
        for (key, value) in extra.as_object().unwrap() {
            source[key] = value.clone();
        }
        source
    };
    let cases = [
        (
            "SELF_TARGET",
            json!({ "source": source(json!({})), "target": { "function_id": "orders::create" } }),
        ),
        (
            "UNSUPPORTED",
            json!({ "source": source(json!({ "on": "start" })), "target": { "function_id": "sink::x" } }),
        ),
        (
            "UNSUPPORTED",
            json!({ "source": source(json!({ "deliver": "both" })), "target": { "function_id": "sink::x" } }),
        ),
        (
            "UNSUPPORTED",
            json!({ "source": source(json!({ "capture": ["input", "timing"] })), "target": { "function_id": "sink::x" } }),
        ),
        (
            "INVALID_FILTER",
            json!({ "source": source(json!({ "filters": { "input.op": "insert" } })), "target": { "function_id": "sink::x" } }),
        ),
        (
            "INVALID_ARGS",
            json!({ "source": source(json!({ "args": { "expression": "* * * * * *" } })), "target": { "function_id": "sink::x" } }),
        ),
        (
            "INVALID_ARGS",
            json!({ "source": source(json!({})), "target": { "function_id": "sink::x" }, "max_fires": 0 }),
        ),
        (
            "INVALID_ARGS",
            json!({ "source": { "function_id": " " }, "target": { "function_id": "sink::x" } }),
        ),
        (
            "INVALID_ARGS",
            json!({ "source": source(json!({})), "target": { "function_id": "" } }),
        ),
        (
            "INVALID_FILTER_PATH",
            json!({ "source": { "function_id": "orders::typed", "filters": { "input.tabel": { "$eq": "x" } } }, "target": { "function_id": "sink::x" } }),
        ),
        (
            "NOT_OBSERVABLE",
            json!({ "source": { "function_id": "vault::read" }, "target": { "function_id": "sink::x" } }),
        ),
        (
            "NOT_OBSERVABLE",
            json!({ "source": { "function_id": "vault::secret" }, "target": { "function_id": "sink::x" } }),
        ),
    ];
    for (n, (code, frame)) in cases.into_iter().enumerate() {
        let mut frame = frame;
        frame["id"] = json!(format!("bad-{n}"));
        let result = watcher.observe(frame.clone()).await;
        assert_eq!(result["state"], "error", "{frame} → {result}");
        assert_eq!(result["error"]["code"], code, "{frame} → {result}");
        assert!(
            t.engine.observers.get(&format!("bad-{n}")).is_none(),
            "a rejection stores nothing"
        );
    }

    // Replays: spelling out the defaults is the same spec; changing one is not.
    let good = observer("good", source(json!({})), "sink::x");
    assert_eq!(watcher.observe(good.clone()).await["state"], "pending");
    let explicit = json!({
        "id": "good",
        "source": source(json!({
            "namespace": "default", "on": "success", "deliver": "envelope",
            "capture": ["caller", "input", "output"], "args": {}, "filters": {}
        })),
        "target": { "function_id": "sink::x", "namespace": "default" },
        "max_in_flight": 64
    });
    assert_eq!(
        watcher.observe(explicit).await["state"],
        "pending",
        "a replay"
    );
    let mut changed = good.clone();
    changed["max_fires"] = json!(5);
    let conflict = watcher.observe(changed).await;
    assert_eq!(conflict["error"]["code"], "OBSERVER_ID_CONFLICT");
    let mut other = connect(&t, "other", true).await;
    let hijack = other.observe(good).await;
    assert_eq!(
        hijack["error"]["code"], "OBSERVER_ID_CONFLICT",
        "owned by another connection"
    );

    for n in 0..2 {
        let frame = observer(&format!("more-{n}"), source(json!({})), "sink::x");
        assert_eq!(watcher.observe(frame).await["state"], "pending");
    }
    let over = watcher
        .observe(observer("one-too-many", source(json!({})), "sink::x"))
        .await;
    assert_eq!(over["error"]["code"], "LIMIT_EXCEEDED");

    // Disposing frees a slot.
    watcher
        .send(json!({ "type": "unobserve", "id": "more-0" }))
        .await;
    watcher.state_becomes("more-0", "disposed").await;
    let again = watcher
        .observe(observer("fits-again", source(json!({})), "sink::x"))
        .await;
    assert_eq!(again["state"], "pending");
}

#[tokio::test]
async fn the_per_function_cap() {
    let t = spawn_engine(ObserversConfig {
        max_per_function: 1,
        ..ObserversConfig::default()
    })
    .await;
    let _orders = host(&t, "orders", true, &["orders::create"]).await;
    let mut watcher = connect(&t, "watcher", true).await;
    let first = watcher
        .observe(observer(
            "one",
            json!({ "function_id": "orders::create" }),
            "sink::x",
        ))
        .await;
    assert_eq!(first["state"], "pending");
    let second = watcher
        .observe(observer(
            "two",
            json!({ "function_id": "orders::create" }),
            "sink::y",
        ))
        .await;
    assert_eq!(second["error"]["code"], "LIMIT_EXCEEDED");
}

/// What `observe` could not check because the function was not registered yet
/// is checked when it registers: the observer goes `error` with the code.
#[tokio::test]
async fn a_function_registering_later_is_checked_then() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let _sink = host(&t, "sink", true, &["sink::x"]).await;
    let mut watcher = connect(&t, "watcher", true).await;
    for (id, source) in [
        (
            "late-args",
            json!({ "function_id": "late::plain", "args": { "x": 1 } }),
        ),
        ("late-allow", json!({ "function_id": "late::private" })),
        (
            "late-path",
            json!({ "function_id": "late::typed", "filters": { "input.tabel": { "$eq": 1 } } }),
        ),
    ] {
        let result = watcher.observe(observer(id, source, "sink::x")).await;
        assert_eq!(result["state"], "pending", "{id}: {result}");
        assert!(
            !result["warnings"].as_array().unwrap().is_empty(),
            "not verified yet"
        );
    }

    let mut later = connect(&t, "later", true).await;
    later.register_function("late::plain").await;
    later
        .register_function_with(json!({ "type": "registerfunction", "id": "late::private", "observe": { "allow": false } }))
        .await;
    later
        .register_function_with(json!({
            "type": "registerfunction", "id": "late::typed",
            "request_format": { "type": "object", "additionalProperties": false, "properties": { "table": {} } }
        }))
        .await;
    for (id, code) in [
        ("late-args", "INVALID_ARGS"),
        ("late-allow", "NOT_OBSERVABLE"),
        ("late-path", "INVALID_FILTER_PATH"),
    ] {
        let error = watcher.state_becomes(id, "error").await;
        assert_eq!(error["reason"]["code"], code, "{id}");
    }
}

/// A re-registration with other formats is checked again: a filter path the
/// new `request_format` rules out fails the observer.
#[tokio::test]
async fn a_re_registration_with_changed_formats_is_checked_again() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let closed = |property: &str| json!({ "type": "object", "additionalProperties": false, "properties": { property: {} } });
    let mut db = connect(&t, "db", true).await;
    db.register_function_with(
        json!({ "type": "registerfunction", "id": "db::rows", "request_format": closed("table") }),
    )
    .await;
    registered(&t.engine, "default", "db::rows").await;
    let _sink = host(&t, "sink", true, &["sink::rows"]).await;
    let mut watcher = connect(&t, "watcher", true).await;
    watcher
        .observe_active(observer(
            "obs-table",
            json!({ "function_id": "db::rows", "filters": { "input.table": { "$eq": "orders" } } }),
            "sink::rows",
        ))
        .await;

    db.disconnect().await;
    watcher.state_becomes("obs-table", "pending").await;
    let mut db = connect(&t, "db-2", true).await;
    db.register_function_with(
        json!({ "type": "registerfunction", "id": "db::rows", "request_format": closed("id") }),
    )
    .await;
    let error = watcher.state_becomes("obs-table", "error").await;
    assert_eq!(error["reason"]["code"], "INVALID_FILTER_PATH");
}

/// A private function is observable: its observers receive the calls its
/// host makes.
#[tokio::test]
async fn a_private_functions_own_calls_are_observed() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let mut http = connect(&t, "http", true).await;
    http.register_function_with(json!({
        "type": "registerfunction", "id": "http::on-request", "private": true
    }))
    .await;
    registered(&t.engine, "default", "http::on-request").await;
    let mut users = host(&t, "users", true, &["users::get"]).await;
    let mut watcher = connect(&t, "watcher", true).await;
    watcher
        .observe_active(observer(
            "route",
            json!({ "function_id": "http::on-request" }),
            "users::get",
        ))
        .await;

    // Its host, calling itself remotely: the routed hook evaluates the call.
    let call = http.call("http::on-request", json!({ "path": "/" })).await;
    http.serve("http::on-request", json!({ "path": "/" })).await;
    http.result(call).await;
    let activation = users.invoked("users::get").await;
    assert_eq!(activation["data"]["input"]["path"], "/");
    users.reply(&activation, Value::Null).await;
    assert_eq!(fires(&t, "route"), 1);
}

/// A 0.25 `observe` object asking for a delivery other than `auto` refuses
/// the registration with `UNSUPPORTED`, and an `args_format` that is not a
/// JSON Schema with `INVALID_ARGS_FORMAT`. A 0.25 `args_format` beside
/// `delivery: auto` is ignored.
#[tokio::test]
async fn unsupported_and_invalid_registrations_are_refused() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let mut worker = connect(&t, "worker", true).await;
    for (id, fields, code) in [
        (
            "bad::delivery",
            json!({ "observe": { "delivery": "broadcast" } }),
            "UNSUPPORTED",
        ),
        (
            "bad::owner",
            json!({ "observe": { "delivery": "owner", "args_format": { "type": "object" } } }),
            "UNSUPPORTED",
        ),
        (
            "bad::args",
            json!({ "args_format": { "type": "not-a-type" } }),
            "INVALID_ARGS_FORMAT",
        ),
    ] {
        let mut frame = json!({ "type": "registerfunction", "id": id });
        for (key, value) in fields.as_object().unwrap() {
            frame[key] = value.clone();
        }
        worker.register_function_with(frame).await;
        let rejected = worker
            .expect("registrationrejected", |f| {
                f["type"] == "registrationrejected" && f["function_id"] == id
            })
            .await;
        assert_eq!(rejected["code"], code, "{id}");
        assert!(t.engine.functions.get("default", id).is_none(), "{id}");
    }
    for frame in [
        json!({
            "type": "registerfunction", "id": "good::auto",
            "observe": { "delivery": "auto", "args_format": { "type": "object" } }
        }),
        json!({
            "type": "registerfunction", "id": "good::args",
            "args_format": { "type": "object" }
        }),
    ] {
        let id = frame["id"].as_str().unwrap().to_string();
        worker.register_function_with(frame).await;
        registered(&t.engine, "default", &id).await;
    }
    assert!(
        t.engine
            .observers
            .options("default", "good::auto")
            .args_format
            .is_none(),
        "a 0.25 args_format is ignored"
    );
    assert!(
        t.engine
            .observers
            .options("default", "good::args")
            .args_format
            .is_some()
    );
}

#[tokio::test]
async fn an_undecodable_observe_is_answered_not_dropped() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let mut watcher = connect(&t, "watcher", true).await;
    watcher
        .send(json!({ "type": "observe", "id": "broken", "source": "orders::create" }))
        .await;
    let result = watcher
        .expect("observeresult", |f| {
            f["type"] == "observeresult" && f["id"] == "broken"
        })
        .await;
    assert_eq!(result["state"], "error");
    assert_eq!(result["error"]["code"], "INVALID_ARGS");

    let id = Uuid::new_v4();
    watcher
        .send(json!({
            "type": "invokefunction", "invocation_id": id.to_string(),
            "function_id": "orders::create", "data": {}, "action": { "type": "fly" }
        }))
        .await;
    let result = watcher.result(id).await;
    assert!(result["error"]["code"].is_string(), "{result}");
}

#[tokio::test]
async fn rbac_gates_the_source_and_the_target() {
    let rbac = json!({
        "expose_functions": [
            "match(\"api::*\")",
            "match(\"engine::observers::*\")",
            { "match": "acme::*", "namespace": "acme" }
        ]
    });
    let t = spawn_engine_with(ObserversConfig::default(), Some(rbac)).await;
    let _host = host(
        &t,
        "host",
        true,
        &["api::orders", "api::sink", "secret::ledger"],
    )
    .await;
    let mut watcher = connect(&t, "watcher", true).await;

    for (n, (source, target)) in [
        (
            json!({ "function_id": "secret::ledger" }),
            json!({ "function_id": "api::sink" }),
        ),
        (
            json!({ "function_id": "api::orders" }),
            json!({ "function_id": "secret::ledger" }),
        ),
        // A rule scoped to `acme` does not answer in `default`, and the other
        // way round.
        (
            json!({ "function_id": "acme::orders" }),
            json!({ "function_id": "api::sink" }),
        ),
        (
            json!({ "function_id": "api::orders", "namespace": "acme" }),
            json!({ "function_id": "api::sink" }),
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let result = watcher
            .observe(json!({ "id": format!("denied-{n}"), "source": source, "target": target }))
            .await;
        assert_eq!(
            result["error"]["code"], "FORBIDDEN",
            "{source} ~> {target}: {result}"
        );
    }
    let scoped = watcher
        .observe(json!({
            "id": "scoped",
            "source": { "function_id": "acme::orders", "namespace": "acme" },
            "target": { "function_id": "api::sink" }
        }))
        .await;
    assert_eq!(
        scoped["state"], "pending",
        "allowed in its namespace: {scoped}"
    );
    watcher
        .observe_active(observer(
            "allowed",
            json!({ "function_id": "api::orders" }),
            "api::sink",
        ))
        .await;

    // With RBAC in force, only the observing worker disposes its observer.
    let mut other = connect(&t, "other", true).await;
    let call = other
        .call("engine::observers::dispose", json!({ "id": "allowed" }))
        .await;
    assert_eq!(other.result(call).await["error"]["code"], "FORBIDDEN");
    let call = watcher
        .call("engine::observers::dispose", json!({ "id": "allowed" }))
        .await;
    assert_eq!(watcher.result(call).await["result"]["disposed"], true);
    watcher.state_becomes("allowed", "disposed").await;
}

/// An activation runs under the session of the worker that created the
/// observer, not unrestricted: a restricted worker cannot use `deliver: input`
/// on `engine::observers::dispose` to dispose another worker's observer.
#[tokio::test]
async fn an_activation_cannot_dispose_another_workers_observer_under_rbac() {
    let rbac = json!({
        "expose_functions": ["match(\"api::*\")", "match(\"engine::observers::*\")"]
    });
    let t = spawn_engine_with(ObserversConfig::default(), Some(rbac)).await;
    let mut orders = host(&t, "orders", true, &["api::orders", "api::sink"]).await;
    let mut victim = connect(&t, "victim", true).await;
    victim
        .observe_active(observer(
            "victim-obs",
            json!({ "function_id": "api::orders" }),
            "api::sink",
        ))
        .await;
    let mut attacker = connect(&t, "attacker", true).await;
    attacker
        .observe_active(observer(
            "attack",
            json!({ "function_id": "api::orders", "deliver": "input" }),
            "engine::observers::dispose",
        ))
        .await;

    let mut caller = connect(&t, "caller", true).await;
    let call = caller
        .call("api::orders", json!({ "id": "victim-obs" }))
        .await;
    orders.serve("api::orders", json!({})).await;
    caller.result(call).await;

    assert!(eventually(|| { info(&t, "attack").is_some_and(|i| { i.fires == 1 }) }).await);
    // The activation of `attack` has had its chance; give it time to run.
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        info(&t, "victim-obs")
            .expect("the victim's observer is gone: the attacker disposed it")
            .state,
        "active",
        "the victim's observer survived the attacker's activation"
    );
}

/// A call through an observer runs as the observer's activations do, under
/// the session of the worker that created the observer: a host on a port
/// without RBAC that forwards its input through a restricted worker's
/// observer of `engine::observers::dispose` cannot dispose another worker's
/// observer. The host's own call of the same builtin can.
#[tokio::test]
async fn a_call_through_an_observer_runs_under_the_observing_workers_session() {
    let rbac = json!({
        "expose_functions": ["match(\"api::*\")", "match(\"h::*\")", "match(\"engine::observers::*\")"]
    });
    let t = spawn_engine_with(ObserversConfig::default(), Some(rbac)).await;
    let internal = unrestricted_listener(&t).await;
    let _orders = host(&t, "orders", true, &["api::orders", "api::sink"]).await;
    let mut victim = connect(&t, "victim", true).await;
    victim
        .observe_active(observer(
            "victim-obs",
            json!({ "function_id": "api::orders" }),
            "api::sink",
        ))
        .await;
    let mut fanout = Client::connect(internal, "fanout", true).await;
    fanout.register_function("h::fanout").await;
    registered(&t.engine, "default", "h::fanout").await;
    let mut attacker = connect(&t, "attacker", true).await;
    attacker
        .observe_active(observer(
            "attack",
            json!({ "function_id": "h::fanout", "deliver": "input" }),
            "engine::observers::dispose",
        ))
        .await;

    let call = fanout
        .call_with(
            "h::fanout",
            json!({ "id": "victim-obs" }),
            json!({ "activation": { "observer_id": "attack" } }),
        )
        .await;
    let answer = fanout.result(call).await;
    assert_eq!(answer["error"]["code"], "FORBIDDEN", "{answer}");
    assert_eq!(fires(&t, "attack"), 1, "the call went through the observer");
    assert_eq!(
        info(&t, "victim-obs").map(|i| i.state).as_deref(),
        Some("active"),
        "the victim's observer survived the call through the attacker's"
    );

    let call = fanout
        .call("engine::observers::dispose", json!({ "id": "victim-obs" }))
        .await;
    assert_eq!(fanout.result(call).await["result"]["disposed"], true);
}

/// A call through an observer carries no `_caller_worker_id` (decision 49),
/// so the host's identity authorizes nothing its target does: a host whose
/// call through another worker's observer of `engine::observers::dispose`
/// names the host's own observer leaves it alone, though the host may
/// dispose it itself.
#[tokio::test]
async fn a_call_through_an_observer_does_not_act_with_the_hosts_identity() {
    let rbac = json!({
        "expose_functions": ["match(\"api::*\")", "match(\"engine::observers::*\")"]
    });
    let t = spawn_engine_with(ObserversConfig::default(), Some(rbac)).await;
    let mut orders = host(&t, "orders", true, &["api::orders", "api::sink"]).await;
    orders
        .observe_active(observer(
            "host-obs",
            json!({ "function_id": "api::orders" }),
            "api::sink",
        ))
        .await;
    let mut attacker = connect(&t, "attacker", true).await;
    attacker
        .observe_active(observer(
            "attack",
            json!({ "function_id": "api::orders", "deliver": "input" }),
            "engine::observers::dispose",
        ))
        .await;
    let call = attacker
        .call("engine::observers::dispose", json!({ "id": "host-obs" }))
        .await;
    assert_eq!(attacker.result(call).await["error"]["code"], "FORBIDDEN");

    // What a deliver hook that forwards the call's input sends.
    let call = orders
        .call_with(
            "api::orders",
            json!({ "id": "host-obs" }),
            json!({ "activation": { "observer_id": "attack" } }),
        )
        .await;
    let answer = orders.result(call).await;
    assert_eq!(answer["error"]["code"], "FORBIDDEN", "{answer}");
    assert_eq!(
        info(&t, "host-obs").map(|i| i.state).as_deref(),
        Some("active"),
        "the host's observer survived the call through the attacker's"
    );

    let call = orders
        .call("engine::observers::dispose", json!({ "id": "host-obs" }))
        .await;
    assert_eq!(orders.result(call).await["result"]["disposed"], true);
}

/// A worker that reconnects with a narrower session no longer observes what it
/// may not invoke: the observer is disposed `FORBIDDEN` on reattach, not
/// re-parented under the new session.
#[tokio::test]
async fn a_reattach_with_a_narrower_session_disposes_the_observers_it_forbids() {
    let rbac = json!({
        "auth_function_id": "test::auth",
        "expose_functions": ["match(\"api::*\")", "match(\"engine::observers::*\")"]
    });
    let t = spawn_engine_with(ObserversConfig::default(), Some(rbac)).await;
    in_process(&t.engine, "test::auth", |input| {
        let narrowed = input["query_params"]["who"]
            .as_array()
            .is_some_and(|who| who.iter().any(|w| w == "narrowed"));
        if narrowed {
            json!({ "forbidden_functions": ["api::sink"] })
        } else {
            json!({})
        }
    });
    let _host = host(&t, "host", true, &["api::orders", "api::sink"]).await;
    let mut watcher = connect(&t, "watcher", true).await;
    watcher
        .observe_active(observer(
            "narrowing",
            json!({ "function_id": "api::orders" }),
            "api::sink",
        ))
        .await;
    let (previous, token) = (watcher.worker_id, watcher.reattach_token.clone());
    watcher.disconnect().await;
    assert!(
        eventually(|| state(&t, "narrowing") == Some(ObserverState::Suspended)).await,
        "the observing worker left: suspended"
    );

    let mut watcher = Client::open_query(t.port, "watcher", "who=narrowed").await;
    watcher
        .send(json!({
            "type": "reattach",
            "previous_worker_id": previous.to_string(),
            "reattach_token": token,
        }))
        .await;
    watcher.announce(true).await;
    assert!(
        eventually(|| t.engine.observers.get("narrowing").is_none()).await,
        "the new session may not invoke the target: disposed"
    );
    let disposed = watcher.state_becomes("narrowing", "disposed").await;
    assert_eq!(disposed["reason"]["code"], "FORBIDDEN", "{disposed}");
}

/// A reattach runs `rbac.on_observer_registration_function_id` again for each
/// observer it re-parents, with the registration as the worker asked for it
/// and the new session's context: an observer the hook now denies is
/// disposed `FORBIDDEN`, one it admits is kept as it is.
#[tokio::test]
async fn a_reattach_runs_the_observer_registration_hook_again() {
    let rbac = json!({
        "auth_function_id": "test::auth",
        "on_observer_registration_function_id": "test::hook",
        "expose_functions": ["match(\"api::*\")", "match(\"engine::observers::*\")"]
    });
    let t = spawn_engine_with(ObserversConfig::default(), Some(rbac)).await;
    in_process(&t.engine, "test::auth", |input| {
        let narrowed = input["query_params"]["who"]
            .as_array()
            .is_some_and(|who| who.iter().any(|w| w == "narrowed"));
        json!({ "context": { "narrowed": narrowed } })
    });
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    in_process(&t.engine, "test::hook", {
        let seen = seen.clone();
        move |input| {
            seen.lock().unwrap().push(input.clone());
            // Narrowed, the hook admits only observers of `api::orders`.
            if input["context"]["narrowed"] == json!(true)
                && input["function_id"] != json!("api::orders")
            {
                Value::Null
            } else {
                json!({})
            }
        }
    });
    let _host = host(
        &t,
        "host",
        true,
        &["api::orders", "api::refunds", "api::sink"],
    )
    .await;
    let mut watcher = connect(&t, "watcher", true).await;
    let mut kept = observer("kept", json!({ "function_id": "api::orders" }), "api::sink");
    kept["metadata"] = json!({ "team": "eu" });
    watcher.observe_active(kept).await;
    watcher
        .observe_active(observer(
            "denied",
            json!({ "function_id": "api::refunds" }),
            "api::sink",
        ))
        .await;
    seen.lock().unwrap().clear();
    let (previous, token) = (watcher.worker_id, watcher.reattach_token.clone());
    watcher.disconnect().await;
    assert!(
        eventually(|| state(&t, "denied") == Some(ObserverState::Suspended)).await,
        "the observing worker left: suspended"
    );

    let mut watcher = Client::open_query(t.port, "watcher", "who=narrowed").await;
    watcher
        .send(json!({
            "type": "reattach",
            "previous_worker_id": previous.to_string(),
            "reattach_token": token,
        }))
        .await;
    watcher.announce(true).await;
    let disposed = watcher.state_becomes("denied", "disposed").await;
    assert_eq!(disposed["reason"]["code"], "FORBIDDEN", "{disposed}");
    assert!(
        eventually(|| state(&t, "kept") == Some(ObserverState::Active)).await,
        "the hook admits it: kept"
    );
    let seen = seen.lock().unwrap().clone();
    let kept_input = seen
        .iter()
        .find(|input| input["observer_id"] == "kept")
        .expect("the hook ran for `kept`");
    assert_eq!(kept_input["function_id"], "api::orders");
    assert_eq!(kept_input["namespace"], "default");
    assert_eq!(kept_input["args"], json!({}));
    assert_eq!(kept_input["metadata"], json!({ "team": "eu" }));
    assert_eq!(kept_input["target_function_id"], "api::sink");
    assert_eq!(kept_input["filters"], Value::Null);
    assert_eq!(kept_input["context"], json!({ "narrowed": true }));
    assert_eq!(seen.len(), 2, "once per observer: {seen:?}");
}

/// `list` / `get` show only observers of functions the caller may invoke; the
/// rest do not exist for it.
#[tokio::test]
async fn rbac_filters_the_observers_builtins() {
    let rbac = json!({
        "auth_function_id": "test::auth",
        "expose_functions": ["match(\"api::*\")", "match(\"engine::observers::*\")"]
    });
    let t = spawn_engine_with(ObserversConfig::default(), Some(rbac)).await;
    in_process(&t.engine, "test::auth", |input| {
        let outsider = input["query_params"]["who"]
            .as_array()
            .is_some_and(|who| who.iter().any(|w| w == "outsider"));
        if outsider {
            json!({ "forbidden_functions": ["api::orders"] })
        } else {
            json!({})
        }
    });
    let _host = host(&t, "host", true, &["api::orders", "api::sink"]).await;
    let mut watcher = connect(&t, "watcher", true).await;
    watcher
        .observe_active(observer(
            "hidden",
            json!({ "function_id": "api::orders" }),
            "api::sink",
        ))
        .await;

    let mut outsider = connect_query(&t, "outsider", true, "who=outsider").await;
    let call = outsider.call("engine::observers::list", json!({})).await;
    let list = outsider.result(call).await;
    assert_eq!(list["result"]["observers"], json!([]), "{list}");
    for function in ["engine::observers::get", "engine::observers::dispose"] {
        let call = outsider.call(function, json!({ "id": "hidden" })).await;
        assert_eq!(
            outsider.result(call).await["error"]["code"],
            "OBSERVER_NOT_FOUND",
            "{function}"
        );
    }
    let call = watcher.call("engine::observers::list", json!({})).await;
    assert_eq!(
        watcher.result(call).await["result"]["observers"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

/// The registration hook can rewrite the target, and RBAC judges the result.
#[tokio::test]
async fn a_hook_rewriting_the_target_to_a_forbidden_function_is_refused() {
    let rbac = json!({
        "on_observer_registration_function_id": "test::hook",
        "expose_functions": ["match(\"api::*\")"]
    });
    let t = spawn_engine_with(ObserversConfig::default(), Some(rbac)).await;
    in_process(
        &t.engine,
        "test::hook",
        |_| json!({ "target_function_id": "secret::sink" }),
    );
    let _host = host(
        &t,
        "host",
        true,
        &["api::orders", "api::sink", "secret::sink"],
    )
    .await;
    let mut watcher = connect(&t, "watcher", true).await;
    let result = watcher
        .observe(observer(
            "rewritten",
            json!({ "function_id": "api::orders" }),
            "api::sink",
        ))
        .await;
    assert_eq!(result["error"]["code"], "FORBIDDEN", "{result}");
}

/// RBAC ran at `observe` against the target as it was then. Whoever registers
/// the target later must pass again, or it would receive the observed input,
/// output and caller of an observer that never authorized it.
#[tokio::test]
async fn a_target_re_registered_by_a_worker_rbac_refuses_fails_the_observer() {
    let rbac = json!({
        "expose_functions": [
            "match(\"orders::*\")",
            "match(\"engine::observers::*\")",
            { "metadata": { "public": true } }
        ]
    });
    let t = spawn_engine_with(ObserversConfig::default(), Some(rbac)).await;
    let _orders = host(&t, "orders", true, &["orders::create"]).await;
    let sink = connect(&t, "sink", true).await;
    let mut sink = sink;
    sink.register_function_with(json!({
        "type": "registerfunction", "id": "audit::sink", "metadata": { "public": true }
    }))
    .await;
    registered(&t.engine, "default", "audit::sink").await;
    let mut watcher = connect(&t, "watcher", true).await;
    watcher
        .observe_active(observer(
            "rebound",
            json!({ "function_id": "orders::create" }),
            "audit::sink",
        ))
        .await;

    sink.disconnect().await;
    watcher.state_becomes("rebound", "suspended").await;
    let mut impostor = connect(&t, "impostor", true).await;
    impostor.register_function("audit::sink").await;
    let error = watcher.state_becomes("rebound", "error").await;
    assert_eq!(error["reason"]["code"], "FORBIDDEN");
}

#[tokio::test]
async fn the_observers_builtins_list_get_and_dispose() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let _orders = host(&t, "orders", true, &["orders::create"]).await;
    let mut sink = host(&t, "sink", true, &["sink::orders"]).await;
    let mut frame = observer(
        "listed",
        json!({ "function_id": "orders::create", "filters": { "input.n": { "$gt": 1 } } }),
        "sink::orders",
    );
    frame["metadata"] = json!({ "secret": "not listed" });
    sink.observe_active(frame).await;

    assert_eq!(
        t.engine.observers.count_in_state(&ObserverState::Active),
        1,
        "iii.observers gauge"
    );
    let mut admin = connect(&t, "admin", true).await;
    let call = admin
        .call(
            "engine::observers::list",
            json!({ "function_id": "orders::create" }),
        )
        .await;
    let list = admin.result(call).await;
    let observers = list["result"]["observers"]
        .as_array()
        .expect("observers")
        .clone();
    assert_eq!(observers.len(), 1, "{list}");
    let info = &observers[0];
    assert_eq!(info["id"], "listed");
    assert_eq!(info["state"], "active");
    assert_eq!(info["target"]["function_id"], "sink::orders");
    assert_eq!(info["worker"]["worker_name"], "sink");
    assert_eq!(
        info["filters"],
        json!({ "$and": [{ "input.n": { "$gt": 1 } }] })
    );
    assert!(info.get("metadata").is_none());

    let call = admin
        .call("engine::observers::get", json!({ "id": "missing" }))
        .await;
    assert_eq!(
        admin.result(call).await["error"]["code"],
        "OBSERVER_NOT_FOUND"
    );
    let call = admin
        .call("engine::observers::get", json!({ "id": "listed" }))
        .await;
    assert_eq!(admin.result(call).await["result"]["id"], "listed");

    // No RBAC in force: every caller is unrestricted.
    let call = admin
        .call("engine::observers::dispose", json!({ "id": "listed" }))
        .await;
    assert_eq!(admin.result(call).await["result"]["disposed"], true);
    let disposed = sink.state_becomes("listed", "disposed").await;
    assert_eq!(disposed["reason"]["code"], "UNOBSERVED");
    assert_eq!(t.engine.observers.count_in_state(&ObserverState::Active), 0);
}

#[tokio::test]
async fn engine_hosted_functions_are_observable_too() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let mut sink = host(&t, "sink", true, &["sink::workers"]).await;
    sink.observe_active(observer(
        "obs-builtin",
        json!({ "function_id": "engine::workers::list", "capture": [] }),
        "sink::workers",
    ))
    .await;
    let mut caller = connect(&t, "caller", true).await;
    let call = caller.call("engine::workers::list", json!({})).await;
    caller.result(call).await;
    let activation = sink.invoked("sink::workers").await;
    assert_eq!(activation["data"]["function_id"], "engine::workers::list");
    sink.reply(&activation, Value::Null).await;
}

#[tokio::test]
async fn the_unobserved_routed_path_copies_no_payload() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let mut orders = host(&t, "orders", true, &["orders::create", "orders::other"]).await;
    let mut sink = host(&t, "sink", true, &["sink::orders"]).await;
    let mut checkout = connect(&t, "checkout", true).await;

    for _ in 0..20 {
        round_trip(
            &t,
            &mut checkout,
            &mut orders,
            "orders::create",
            json!({ "big": [1, 2, 3] }),
            json!({}),
        )
        .await;
    }
    assert_eq!(t.engine.observers.payload_copies(), 0, "nothing observed");

    // Observing another function, with a filter on the input, changes nothing
    // for this one.
    sink.observe_active(observer(
        "obs-other",
        json!({ "function_id": "orders::other", "filters": { "input.n": { "$eq": 1 } } }),
        "sink::orders",
    ))
    .await;
    round_trip(
        &t,
        &mut checkout,
        &mut orders,
        "orders::create",
        json!({ "n": 1 }),
        json!({}),
    )
    .await;
    assert_eq!(
        t.engine.observers.payload_copies(),
        0,
        "orders::create is not observed"
    );

    round_trip(
        &t,
        &mut checkout,
        &mut orders,
        "orders::other",
        json!({ "n": 1 }),
        json!({}),
    )
    .await;
    assert_eq!(
        t.engine.observers.payload_copies(),
        1,
        "one copy, for the one observed call"
    );
    let activation = sink.invoked("sink::orders").await;
    sink.reply(&activation, Value::Null).await;
}

/// A chain through an engine-hosted function keeps its depth: the loop guard
/// still counts the hop.
#[tokio::test]
async fn the_depth_survives_an_engine_hosted_hop() {
    use iii::engine::{EngineTrait, Handler, RegisterFunctionRequest};
    use iii::function::FunctionResult;

    let t = spawn(Setup {
        observers: ObserversConfig {
            max_observe_depth: 1,
            ..ObserversConfig::default()
        },
        ..Setup::default()
    })
    .await;
    // relay::hop is engine-hosted and calls orders::create itself.
    let engine = t.engine.clone();
    t.engine.register_function_handler(
        RegisterFunctionRequest {
            function_id: "relay::hop".to_string(),
            description: None,
            request_format: None,
            response_format: None,
            metadata: None,
        },
        Handler::new(move |input: Value| {
            let engine = engine.clone();
            async move {
                match engine.call("orders::create", input).await {
                    Ok(result) => FunctionResult::Success(result),
                    Err(error) => FunctionResult::Failure(error),
                }
            }
        }),
    );
    let mut orders = host(&t, "orders", true, &["orders::create"]).await;
    let mut sink = host(&t, "sink", true, &["sink::hop"]).await;
    let mut caller = connect(&t, "caller", true).await;
    sink.observe_active(observer(
        "obs-hop",
        json!({ "function_id": "orders::create" }),
        "sink::hop",
    ))
    .await;

    // Called from inside an activation (depth 1), relay::hop calls
    // orders::create at depth 1 too: its completion would activate at
    // depth 2, past the limit.
    let call = caller
        .call_with("relay::hop", json!({}), json!({ "observe_depth": 1 }))
        .await;
    let forwarded = orders.serve("orders::create", json!({})).await;
    assert_eq!(forwarded["observe_depth"], 1, "the hop carried the depth");
    caller.result(call).await;
    assert!(eventually(|| dropped(&t, "obs-hop", "LOOP_GUARD") == 1).await);
    assert_eq!(fires(&t, "obs-hop"), 0);
}
