//! Hosts that deliver their own calls (v2 §8.2), over real WebSockets.
//!
//! A worker that declared `observe.v2` lists, on its answer to every routed
//! call (`invocationresult.observed`), the observers its delivery matched.
//! The engine evaluates none of those calls: it activates each listed
//! observer of the invoked function that is active, building the envelope
//! from the input it kept and the answer, and never forwards the list. A
//! host's local calls arrive as `invocationsobserved` items; a routed call
//! never does (decision 38).

mod observe_support;

use iii::observer::ObserversConfig;
use observe_support::{
    Client, OBSERVE_V1, TestEngine, connect, connect_v2, connect_with, evaluated, eventually,
    registered, spawn_engine,
};
use serde_json::{Value, json};
use uuid::Uuid;

/// A host for `functions`, connected with `features` and registered.
async fn host_with(t: &TestEngine, name: &str, features: &[&str], functions: &[&str]) -> Client {
    let mut client = connect_with(t, name, features).await;
    for function in functions {
        client.register_function(function).await;
    }
    for function in functions {
        registered(&t.engine, "default", function).await;
    }
    client
}

/// A host that declared `observe.v2`.
async fn host(t: &TestEngine, name: &str, functions: &[&str]) -> Client {
    host_with(t, name, observe_support::OBSERVE_V2, functions).await
}

fn observer(id: &str, source: Value, target: &str) -> Value {
    json!({ "id": id, "source": source, "target": { "function_id": target } })
}

fn fires(t: &TestEngine, id: &str) -> u64 {
    t.engine.observers.fires(id).unwrap_or_default()
}

fn dropped(t: &TestEngine, id: &str, reason: &str) -> u64 {
    t.engine
        .observers
        .get_info(&t.engine, None, id)
        .ok()
        .and_then(|info| info.dropped.get(reason).copied())
        .unwrap_or_default()
}

/// Whether a frame for `function_id` reached `client` (after a barrier that
/// guarantees it would have by now).
fn invoked_already(client: &mut Client, function_id: &str) -> bool {
    client.has_seen(|f| f["type"] == "invokefunction" && f["function_id"] == function_id)
}

/// The host's delivery decides, not the engine's evaluation: the observer it
/// lists is activated though its filter would not match the call, the one it
/// leaves out is not though it has no filter, and ids that are not active
/// observers of the invoked function activate nothing. The caller's answer
/// never carries the list.
#[tokio::test]
async fn a_hosts_routed_answer_activates_the_observers_it_lists() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let mut orders = host(&t, "orders", &["orders::create", "orders::other"]).await;
    let mut sink = host(
        &t,
        "sink",
        &["sink::listed", "sink::unlisted", "sink::foreign"],
    )
    .await;
    let mut checkout = connect(&t, "checkout", true).await;
    sink.observe_active(observer(
        "listed",
        json!({ "function_id": "orders::create", "filters": { "input.n": { "$eq": 99 } } }),
        "sink::listed",
    ))
    .await;
    sink.observe_active(observer(
        "unlisted",
        json!({ "function_id": "orders::create" }),
        "sink::unlisted",
    ))
    .await;
    sink.observe_active(observer(
        "foreign",
        json!({ "function_id": "orders::other" }),
        "sink::foreign",
    ))
    .await;

    let before = t.engine.observers.evaluated();
    let call = checkout.call("orders::create", json!({ "n": 1 })).await;
    let invoke = orders.invoked("orders::create").await;
    assert!(invoke["invocation_id"].is_string());
    orders
        .reply_observed(
            &invoke,
            json!({ "id": 7 }),
            &["listed", "foreign", "listed", "no-such-observer"],
        )
        .await;

    let answer = checkout.result(call).await;
    assert_eq!(answer["result"], json!({ "id": 7 }));
    assert!(answer.get("observed").is_none(), "stripped: {answer}");

    let activation = sink.invoked("sink::listed").await;
    let envelope = &activation["data"];
    assert_eq!(envelope["observer_id"], "listed");
    assert_eq!(envelope["function_id"], "orders::create");
    assert_eq!(envelope["invocation_id"], invoke["invocation_id"]);
    assert_eq!(envelope["status"], "success");
    assert_eq!(
        envelope["input"],
        json!({ "n": 1 }),
        "as the caller sent it, without the engine's stamp"
    );
    assert_eq!(envelope["output"], json!({ "id": 7 }), "the host's result");
    assert_eq!(envelope["caller"]["worker_name"], "checkout");
    assert_eq!(
        envelope["caller"]["worker_id"],
        checkout.worker_id.to_string(),
        "the caller the engine forwarded the call with"
    );
    sink.reply(&activation, Value::Null).await;

    evaluated(&t.engine, before + 1).await;
    assert_eq!(fires(&t, "listed"), 1, "listed twice, activated once");
    assert_eq!(fires(&t, "unlisted"), 0, "not listed: not evaluated");
    assert_eq!(fires(&t, "foreign"), 0, "another function's observer");
    assert_eq!(dropped(&t, "foreign", "INACTIVE"), 0, "not charged to it");
    assert!(!invoked_already(&mut sink, "sink::unlisted"));
    assert!(!invoked_already(&mut sink, "sink::foreign"));
}

/// The engine sends every call it routes to a host with an invocation id,
/// a fire-and-forget one included, so every routed call has an answer that
/// can list its observers.
#[tokio::test]
async fn a_fire_and_forget_call_is_answered_and_delivered_too() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let mut audit = host(&t, "audit", &["audit::log"]).await;
    let mut sink = host(&t, "sink", &["sink::logs"]).await;
    let mut checkout = connect(&t, "checkout", true).await;
    sink.observe_active(observer(
        "logs",
        json!({ "function_id": "audit::log", "deliver": "input" }),
        "sink::logs",
    ))
    .await;

    checkout
        .send(json!({
            "type": "invokefunction",
            "function_id": "audit::log",
            "data": { "entry": 1 },
            "action": { "type": "void" },
        }))
        .await;
    let invoke = audit.invoked("audit::log").await;
    assert!(invoke["invocation_id"].is_string(), "{invoke}");
    audit.reply_observed(&invoke, Value::Null, &["logs"]).await;
    let activation = sink.invoked("sink::logs").await;
    assert_eq!(activation["data"], json!({ "entry": 1 }));
    sink.reply(&activation, Value::Null).await;
}

/// `observed` may ride an answer that carries `error`: the listed observer's
/// envelope says `status: error` with the host's error (decision 39), and the
/// caller gets the error alone.
#[tokio::test]
async fn an_error_answer_activates_the_on_error_observers_it_lists() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let mut payments = host(&t, "payments", &["payments::charge"]).await;
    let mut sink = host(&t, "sink", &["sink::failures"]).await;
    let mut checkout = connect(&t, "checkout", true).await;
    sink.observe_active(observer(
        "failures",
        json!({
            "function_id": "payments::charge",
            "on": "error",
            "capture": ["input", "error"]
        }),
        "sink::failures",
    ))
    .await;

    let call = checkout
        .call("payments::charge", json!({ "amount": 5 }))
        .await;
    let invoke = payments.invoked("payments::charge").await;
    payments
        .send(json!({
            "type": "invocationresult",
            "invocation_id": invoke["invocation_id"],
            "function_id": "payments::charge",
            "error": { "code": "CARD_DECLINED", "message": "declined" },
            "observed": ["failures"],
        }))
        .await;

    let answer = checkout.result(call).await;
    assert_eq!(answer["error"]["code"], "CARD_DECLINED");
    assert!(answer.get("observed").is_none() && answer.get("result").is_none());

    let activation = sink.invoked("sink::failures").await;
    let envelope = &activation["data"];
    assert_eq!(envelope["status"], "error");
    assert_eq!(envelope["error"]["code"], "CARD_DECLINED");
    assert_eq!(envelope["input"], json!({ "amount": 5 }));
    assert!(envelope.get("output").is_none());
    sink.reply(&activation, Value::Null).await;
}

/// The list is read from the connection that hosts the function only: an
/// answer anyone else sends (here the caller answering its own call) has its
/// result delivered as before, but activates nothing it lists
/// (`REPORTER_NOT_OWNER`).
#[tokio::test]
async fn a_list_from_a_connection_that_does_not_host_the_function_is_refused() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let mut orders = host(&t, "orders", &["orders::create"]).await;
    let mut sink = host(&t, "sink", &["sink::orders"]).await;
    let mut forger = connect_v2(&t, "forger").await;
    sink.observe_active(observer(
        "watched",
        json!({ "function_id": "orders::create" }),
        "sink::orders",
    ))
    .await;

    let call = forger.call("orders::create", json!({ "n": 1 })).await;
    let invoke = orders.invoked("orders::create").await;
    forger
        .send(json!({
            "type": "invocationresult",
            "invocation_id": call.to_string(),
            "function_id": "orders::create",
            "result": { "forged": true },
            "observed": ["watched"],
        }))
        .await;
    assert_eq!(forger.result(call).await["result"]["forged"], true);
    assert!(eventually(|| dropped(&t, "watched", "REPORTER_NOT_OWNER") == 1).await);
    assert_eq!(fires(&t, "watched"), 0);

    // The host's own answer finds nobody waiting. The engine handles one
    // connection's frames in order: once the host has the answer to a call
    // it sent next, its answer was handled.
    orders
        .reply_observed(&invoke, json!({}), &["watched"])
        .await;
    let barrier = orders.call("engine::workers::list", json!({})).await;
    orders.result(barrier).await;
    assert_eq!(fires(&t, "watched"), 0);
    assert!(!invoked_already(&mut sink, "sink::orders"));
}

/// A host without `observe.v2` delivers nothing: whatever it lists is
/// ignored and the engine evaluates the call's observers itself.
#[tokio::test]
async fn the_engine_evaluates_the_calls_of_a_host_without_observe_v2() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let mut orders = host_with(&t, "orders", OBSERVE_V1, &["orders::create"]).await;
    let mut sink = host(&t, "sink", &["sink::listed", "sink::unlisted"]).await;
    let mut checkout = connect(&t, "checkout", true).await;
    sink.observe_active(observer(
        "listed",
        json!({ "function_id": "orders::create", "filters": { "input.n": { "$eq": 99 } } }),
        "sink::listed",
    ))
    .await;
    sink.observe_active(observer(
        "unlisted",
        json!({ "function_id": "orders::create" }),
        "sink::unlisted",
    ))
    .await;

    let before = t.engine.observers.evaluated();
    let call = checkout.call("orders::create", json!({ "n": 1 })).await;
    let invoke = orders.invoked("orders::create").await;
    orders.reply_observed(&invoke, json!({}), &["listed"]).await;
    checkout.result(call).await;
    let activation = sink.invoked("sink::unlisted").await;
    sink.reply(&activation, Value::Null).await;
    evaluated(&t.engine, before + 1).await;
    assert_eq!(fires(&t, "listed"), 0, "its filter does not match");
    assert_eq!(fires(&t, "unlisted"), 1);
}

/// A host that declared `observe.v2` delivers its routed calls, so one that
/// ends without its answer activates nothing (decision 2): here its caller
/// leaves while the host runs it. The engine's `invocation_stopped` is no
/// error of the function: its `on: error` observer counts a `CANCELLED` drop
/// and does not fire, and the host's late answer finds nobody waiting.
#[tokio::test]
async fn a_call_its_caller_left_activates_nothing_for_a_host_that_delivers() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let mut orders = host(&t, "orders", &["orders::create"]).await;
    let mut sink = host(&t, "sink", &["sink::failures"]).await;
    let mut checkout = connect(&t, "checkout", true).await;
    sink.observe_active(observer(
        "failures",
        json!({ "function_id": "orders::create", "on": "error" }),
        "sink::failures",
    ))
    .await;

    checkout.call("orders::create", json!({ "n": 1 })).await;
    let invoke = orders.invoked("orders::create").await;
    checkout.disconnect().await;
    assert!(eventually(|| dropped(&t, "failures", "CANCELLED") == 1).await);

    orders
        .reply_observed(&invoke, json!({ "id": 7 }), &["failures"])
        .await;
    let barrier = orders.call("engine::workers::list", json!({})).await;
    orders.result(barrier).await;
    assert_eq!(fires(&t, "failures"), 0);
    assert!(!invoked_already(&mut sink, "sink::failures"));
}

/// A routed call the engine cannot send to its host (larger than the host
/// receives) never ran there: the caller gets `payload_too_large`, and an
/// `observe.v2` host's observers count a `CANCELLED` drop: the function
/// raised no error for them to observe.
#[tokio::test]
async fn a_call_never_sent_to_a_host_that_delivers_activates_nothing() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let mut orders = host(&t, "orders", &["orders::create"]).await;
    let mut sink = host(&t, "sink", &["sink::failures"]).await;
    let mut checkout = connect(&t, "checkout", true).await;
    sink.observe_active(observer(
        "failures",
        json!({ "function_id": "orders::create", "on": "error" }),
        "sink::failures",
    ))
    .await;
    t.engine
        .worker_registry
        .get_worker(&orders.worker_id)
        .expect("the host is connected")
        .receive_message_limit
        .store(1024, std::sync::atomic::Ordering::Relaxed);

    let call = checkout
        .call("orders::create", json!({ "blob": "x".repeat(4096) }))
        .await;
    assert_eq!(
        checkout.result(call).await["error"]["code"],
        "payload_too_large"
    );
    assert!(eventually(|| dropped(&t, "failures", "CANCELLED") == 1).await);
    assert_eq!(fires(&t, "failures"), 0);
    assert!(!invoked_already(&mut orders, "orders::create"));
    assert!(!invoked_already(&mut sink, "sink::failures"));
}

/// Every routed answer of an `observe.v2` host is read, a target's answer
/// to an activation and to a call through an observer included (decision
/// 36): each activates the target's own observers it lists, and the host
/// that called through the observer gets the result without the list.
#[tokio::test]
async fn a_targets_answer_activates_the_observers_it_lists() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let mut orders = host(&t, "orders", &["orders::create"]).await;
    let mut audit = host(&t, "audit", &["audit::record"]).await;
    let mut sink = host(&t, "sink", &["sink::second", "sink::third"]).await;
    let mut checkout = connect(&t, "checkout", true).await;
    sink.observe_active(observer(
        "first",
        json!({ "function_id": "orders::create" }),
        "audit::record",
    ))
    .await;
    sink.observe_active(observer(
        "second",
        json!({ "function_id": "audit::record", "deliver": "output" }),
        "sink::second",
    ))
    .await;

    // An activation of `first` reaches audit::record, whose host lists
    // `second` on its answer.
    let call = checkout.call("orders::create", json!({ "n": 1 })).await;
    let invoke = orders.invoked("orders::create").await;
    orders
        .reply_observed(&invoke, json!({ "id": 7 }), &["first"])
        .await;
    checkout.result(call).await;
    let activation = audit.invoked("audit::record").await;
    assert_eq!(activation["activation"]["observer_id"], "first");
    audit
        .reply_observed(&activation, json!({ "recorded": 1 }), &["second"])
        .await;
    let second = sink.invoked("sink::second").await;
    assert_eq!(second["data"], json!({ "recorded": 1 }));
    assert_eq!(second["observe_depth"], 2, "a chain of two observers");
    sink.reply(&second, Value::Null).await;

    // A call through `first`, made by the host of orders::create.
    sink.observe_active(observer(
        "third",
        json!({ "function_id": "audit::record", "deliver": "input" }),
        "sink::third",
    ))
    .await;
    let through = orders
        .call_with(
            "audit::record",
            json!({ "direct": true }),
            json!({ "activation": { "observer_id": "first" } }),
        )
        .await;
    let forwarded = audit.invoked("audit::record").await;
    assert_eq!(forwarded["activation"]["observer_id"], "first");
    audit
        .reply_observed(&forwarded, json!({ "recorded": 2 }), &["third"])
        .await;
    let answer = orders.result(through).await;
    assert_eq!(answer["result"], json!({ "recorded": 2 }));
    assert!(answer.get("observed").is_none(), "stripped: {answer}");
    let third = sink.invoked("sink::third").await;
    assert_eq!(third["data"], json!({ "direct": true }));
    sink.reply(&third, Value::Null).await;
}

/// A routed call's host reports its matches on the answer only: an
/// `invocationsobserved` item for a call the engine routed to it is dropped
/// as `INVALID_REPORT` (decision 38), whether it arrives before the answer or
/// after it, while an item for a call of its own is applied.
#[tokio::test]
async fn report_items_for_a_routed_call_are_dropped() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let mut orders = host(&t, "orders", &["orders::create"]).await;
    let mut sink = host(&t, "sink", &["sink::orders"]).await;
    let mut checkout = connect(&t, "checkout", true).await;
    sink.observe_active(observer(
        "watched",
        json!({ "function_id": "orders::create" }),
        "sink::orders",
    ))
    .await;

    let item = |invocation_id: &str| {
        json!({
            "function_id": "orders::create",
            "invocation_id": invocation_id,
            "matched": ["watched"],
            "status": "success",
            "input": { "n": 1 },
            "output": { "id": 7 },
            "started_at_ms": 1,
            "duration_ms": 0.5,
            "observe_depth": 0,
        })
    };
    let call = checkout.call("orders::create", json!({ "n": 1 })).await;
    let invoke = orders.invoked("orders::create").await;
    let routed = invoke["invocation_id"].as_str().unwrap().to_string();
    orders
        .send(json!({ "type": "invocationsobserved", "items": [item(&routed)] }))
        .await;
    orders
        .reply_observed(&invoke, json!({ "id": 7 }), &[])
        .await;
    checkout.result(call).await;
    assert!(eventually(|| dropped(&t, "watched", "INVALID_REPORT") == 1).await);
    // The report linger sends items after the answer: the call is still
    // known as routed.
    orders
        .send(json!({ "type": "invocationsobserved", "items": [item(&routed)] }))
        .await;
    assert!(eventually(|| dropped(&t, "watched", "INVALID_REPORT") == 2).await);
    assert_eq!(fires(&t, "watched"), 0);

    let local = Uuid::new_v4().to_string();
    orders
        .send(json!({ "type": "invocationsobserved", "items": [item(&local)] }))
        .await;
    let activation = sink.invoked("sink::orders").await;
    assert_eq!(activation["data"]["invocation_id"], local.as_str());
    sink.reply(&activation, Value::Null).await;
    assert_eq!(fires(&t, "watched"), 1);
}

/// A report item whose handler returned its input carries the value once,
/// with `output_is_input`: observers receive it as both `input` and
/// `output`. Neither ever carries the engine's `_caller_worker_id` stamp
/// (decision 40), whether an item or a routed answer brought the value.
#[tokio::test]
async fn observed_values_are_the_handlers_without_the_callers_stamp() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let mut database = connect_v2(&t, "database").await;
    database
        .register_function_with(json!({
            "type": "registerfunction",
            "id": "database::on-row-changed",
            "private": true,
        }))
        .await;
    registered(&t.engine, "default", "database::on-row-changed").await;
    let mut echo = host(&t, "echo", &["echo::back"]).await;
    let mut sink = host(&t, "sink", &["sink::rows", "sink::echoes"]).await;
    let mut caller = connect(&t, "caller", true).await;
    sink.observe_active(observer(
        "rows",
        json!({ "function_id": "database::on-row-changed", "capture": ["input", "output"] }),
        "sink::rows",
    ))
    .await;
    sink.observe_active(observer(
        "echoes",
        json!({ "function_id": "echo::back", "capture": ["input", "output"] }),
        "sink::echoes",
    ))
    .await;

    let change = json!({ "table": "orders", "op": "insert" });
    let mut stamped = change.clone();
    stamped["_caller_worker_id"] = json!("some-worker");
    database
        .send(json!({
            "type": "invocationsobserved",
            "items": [{
                "function_id": "database::on-row-changed",
                "invocation_id": Uuid::new_v4().to_string(),
                "matched": ["rows"],
                "status": "success",
                "input": stamped,
                "output_is_input": true,
                "started_at_ms": 1,
                "duration_ms": 0.25,
                "observe_depth": 0,
            }],
        }))
        .await;
    let row = sink.invoked("sink::rows").await;
    assert_eq!(row["data"]["input"], change);
    assert_eq!(row["data"]["output"], change, "filled from the input");
    sink.reply(&row, Value::Null).await;

    // A handler that returns its input returns the stamp the engine added
    // too: the caller gets it as the host sent it, the observers do not.
    let call = caller.call("echo::back", json!({ "n": 1 })).await;
    let invoke = echo.invoked("echo::back").await;
    assert_eq!(
        invoke["data"]["_caller_worker_id"],
        caller.worker_id.to_string()
    );
    echo.reply_observed(&invoke, invoke["data"].clone(), &["echoes"])
        .await;
    let answer = caller.result(call).await;
    assert_eq!(answer["result"], invoke["data"]);
    let echoed = sink.invoked("sink::echoes").await;
    assert_eq!(echoed["data"]["input"], json!({ "n": 1 }));
    assert_eq!(echoed["data"]["output"], json!({ "n": 1 }));
    sink.reply(&echoed, Value::Null).await;
}

/// A call to a function with no active observer costs the engine one load:
/// nothing is copied or evaluated, whatever its host answers. An id the host
/// lists anyway (an observer that was not active when the call began) is
/// counted, not activated.
#[tokio::test]
async fn an_unobserved_call_to_a_host_costs_nothing() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let mut orders = host(&t, "orders", &["orders::create"]).await;
    let mut checkout = connect(&t, "checkout", true).await;

    let evaluated_before = t.engine.observers.evaluated();
    for n in 0..20 {
        let call = checkout.call("orders::create", json!({ "n": n })).await;
        let invoke = orders.invoked("orders::create").await;
        orders.reply_observed(&invoke, json!({}), &[]).await;
        checkout.result(call).await;
    }
    assert_eq!(t.engine.observers.payload_copies(), 0);
    assert_eq!(t.engine.observers.evaluated(), evaluated_before);

    // Pending: its target is not registered yet.
    let mut watcher = connect(&t, "watcher", true).await;
    let result = watcher
        .observe(observer(
            "late",
            json!({ "function_id": "orders::create" }),
            "sink::later",
        ))
        .await;
    assert_eq!(result["state"], "pending");
    let call = checkout.call("orders::create", json!({ "n": 1 })).await;
    let invoke = orders.invoked("orders::create").await;
    orders.reply_observed(&invoke, json!({}), &["late"]).await;
    checkout.result(call).await;
    assert!(eventually(|| dropped(&t, "late", "INACTIVE") == 1).await);
    assert_eq!(t.engine.observers.payload_copies(), 0);
    assert_eq!(fires(&t, "late"), 0);
}

/// The observers' envelopes of one routed call, by observer: `count`
/// activations of `function_id` on `sink`, each answered.
async fn envelopes(sink: &mut Client, function_id: &str, count: usize) -> Vec<Value> {
    let mut envelopes = Vec::new();
    for _ in 0..count {
        let activation = sink.invoked(function_id).await;
        sink.reply(&activation, Value::Null).await;
        envelopes.push(activation["data"].clone());
    }
    envelopes
}

/// `capture: []` moves no payloads (A6): the engine copies nothing for an
/// observer that captures nothing and delivers the envelope, whether it
/// evaluates the call (a host without `observe.v2`) or the host lists the
/// observer. A filter on the input needs the input only where it is
/// evaluated: the engine copies it for the host without `observe.v2`, and
/// never for one that delivers its calls.
#[tokio::test]
async fn an_observer_that_captures_nothing_copies_no_payload() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let mut legacy = host_with(&t, "legacy", OBSERVE_V1, &["legacy::create"]).await;
    let mut orders = host(&t, "orders", &["orders::create"]).await;
    let mut sink = host(&t, "sink", &["sink::counted"]).await;
    let mut checkout = connect(&t, "checkout", true).await;
    for (id, function_id) in [("legacy", "legacy::create"), ("orders", "orders::create")] {
        sink.observe_active(observer(
            id,
            json!({ "function_id": function_id, "capture": [] }),
            "sink::counted",
        ))
        .await;
    }
    let no_payloads = |envelope: &Value| {
        ["input", "output", "error", "caller", "metadata"]
            .iter()
            .all(|key| envelope.get(key).is_none())
    };

    for _ in 0..5 {
        let call = checkout.call("legacy::create", json!({ "n": 1 })).await;
        let invoke = legacy.invoked("legacy::create").await;
        legacy.reply(&invoke, json!({ "id": 1 })).await;
        checkout.result(call).await;
        let call = checkout.call("orders::create", json!({ "n": 1 })).await;
        let invoke = orders.invoked("orders::create").await;
        orders
            .reply_observed(&invoke, json!({ "id": 2 }), &["orders"])
            .await;
        checkout.result(call).await;
        for envelope in envelopes(&mut sink, "sink::counted", 2).await {
            assert!(no_payloads(&envelope), "{envelope}");
        }
    }
    assert_eq!(t.engine.observers.payload_copies(), 0);

    for (id, function_id) in [
        ("legacy-n", "legacy::create"),
        ("orders-n", "orders::create"),
    ] {
        sink.observe_active(observer(
            id,
            json!({
                "function_id": function_id,
                "capture": [],
                "filters": { "input.n": { "$eq": 1 } }
            }),
            "sink::counted",
        ))
        .await;
    }
    let call = checkout.call("legacy::create", json!({ "n": 1 })).await;
    let invoke = legacy.invoked("legacy::create").await;
    legacy.reply(&invoke, json!({ "id": 1 })).await;
    checkout.result(call).await;
    assert_eq!(
        t.engine.observers.payload_copies(),
        1,
        "the engine evaluates the filter on the input"
    );
    let call = checkout.call("orders::create", json!({ "n": 1 })).await;
    let invoke = orders.invoked("orders::create").await;
    orders
        .reply_observed(&invoke, json!({ "id": 2 }), &["orders", "orders-n"])
        .await;
    checkout.result(call).await;
    assert_eq!(
        t.engine.observers.payload_copies(),
        1,
        "the host evaluated the filter: nothing to copy"
    );
    for envelope in envelopes(&mut sink, "sink::counted", 4).await {
        assert!(no_payloads(&envelope), "{envelope}");
    }
}
