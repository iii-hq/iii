//! The "failed calls with their input" recipe (docs/next/using-iii/observers.mdx),
//! over real WebSockets:
//!
//! ```json
//! { "capture": ["input", "error"], "filters": { "error": { "$exists": true } } }
//! ```
//!
//! `capture: [input, error]` fires on both outcomes (decision 59: a capture
//! naming `input` fires on success too); the filter keeps the failures only.
//! The observing worker runs a current SDK (`observe.capture`, decision 60).
//!
//! - Engine-routed path: the engine evaluates the filter of every routed call
//!   (a host without `observe.v2`, or a capture host whose answer narrows
//!   nothing) and activates nothing for a success.
//! - SDK-host path: the engine tells a capture host the recipe exactly
//!   (`capture`, `filters`, `fires_on: [success, error]`), so the host can
//!   evaluate it locally; the failure it reports delivers input + error. That
//!   the host reports no success is the SDK's part
//!   (sdk/packages/node/iii/tests/observe-error-filter-recipe.test.ts).

mod observe_support;

use iii::observer::ObserversConfig;
use observe_support::{
    Client, OBSERVE_CAPTURE, OBSERVE_V1, OBSERVE_V2, TestEngine, connect, connect_with, evaluated,
    registered, spawn_engine,
};
use serde_json::{Value, json};
use uuid::Uuid;

fn recipe(function_id: &str) -> Value {
    json!({
        "function_id": function_id,
        "capture": ["input", "error"],
        "filters": { "error": { "$exists": true } },
    })
}

fn observer(id: &str, source: Value, target: &str) -> Value {
    json!({ "id": id, "source": source, "target": { "function_id": target } })
}

fn fires(t: &TestEngine, id: &str) -> u64 {
    t.engine.observers.fires(id).unwrap_or_default()
}

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

fn invoked_already(client: &mut Client, function_id: &str) -> bool {
    client.has_seen(|f| f["type"] == "invokefunction" && f["function_id"] == function_id)
}

/// A routed success, answered with `result`, after which the engine has
/// evaluated it.
async fn routed_success(t: &TestEngine, host: &mut Client, caller: &mut Client, function: &str) {
    let before = t.engine.observers.evaluated();
    let call = caller.call(function, json!({ "amount": 1 })).await;
    let invoke = host.invoked(function).await;
    host.reply(&invoke, json!({ "charged": 1 })).await;
    caller.result(call).await;
    evaluated(&t.engine, before + 1).await;
}

/// The envelope the recipe's target receives for a failed call: the input
/// and the error, nothing it did not capture.
fn assert_failure_envelope(envelope: &Value, id: &str, input: Value) {
    assert_eq!(envelope["observer_id"], id, "{envelope}");
    assert_eq!(envelope["status"], "error", "{envelope}");
    assert_eq!(envelope["input"], input, "{envelope}");
    assert_eq!(envelope["error"]["code"], "CARD_DECLINED", "{envelope}");
    for absent in ["output", "caller", "metadata"] {
        assert!(envelope.get(absent).is_none(), "{absent}: {envelope}");
    }
}

/// Engine-routed path: whatever the host (a 0.25 host without features, an
/// `observe.v1` host, or a current SDK host whose answer names no `matched`),
/// the engine evaluates the recipe's filter: successes activate nothing and
/// never reach the observing worker; each failure activates the target with
/// the input and the error. A control observer with the same capture and no
/// filter fires on the successes too, so the filter is what keeps them out.
#[tokio::test]
async fn the_engine_routes_only_failed_calls_to_the_error_filter_recipe() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let mut checkout = connect(&t, "checkout", true).await;
    let kinds: [(&str, &[&str]); 3] = [
        ("v025", &[]),
        ("v1", OBSERVE_V1),
        ("capture", OBSERVE_CAPTURE),
    ];
    for (kind, features) in kinds {
        let function = format!("{kind}::charge");
        let mut host = host_with(&t, &format!("host-{kind}"), features, &[&function]).await;
        let failed = format!("sink-{kind}::failed");
        let every = format!("sink-{kind}::every");
        let mut sink = host_with(
            &t,
            &format!("sink-{kind}"),
            OBSERVE_CAPTURE,
            &[&failed, &every],
        )
        .await;
        let id = format!("recipe-{kind}");
        let control = format!("control-{kind}");
        sink.observe_active(observer(&id, recipe(&function), &failed))
            .await;
        sink.observe_active(observer(
            &control,
            json!({ "function_id": function, "capture": ["input", "error"] }),
            &every,
        ))
        .await;

        routed_success(&t, &mut host, &mut checkout, &function).await;
        let activation = sink.invoked(&every).await;
        assert_eq!(activation["data"]["status"], "success", "[{kind}]");
        sink.reply(&activation, Value::Null).await;
        assert!(!invoked_already(&mut sink, &failed), "[{kind}] success");
        assert_eq!(fires(&t, &id), 0, "[{kind}] a success never fires it");

        let before = t.engine.observers.evaluated();
        let call = checkout.call(&function, json!({ "amount": 2 })).await;
        let invoke = host.invoked(&function).await;
        host.reply_error(&invoke, "CARD_DECLINED", "declined").await;
        checkout.result(call).await;
        evaluated(&t.engine, before + 1).await;
        let activation = sink.invoked(&failed).await;
        assert_failure_envelope(&activation["data"], &id, json!({ "amount": 2 }));
        sink.reply(&activation, Value::Null).await;
        let activation = sink.invoked(&every).await;
        assert_eq!(activation["data"]["status"], "error", "[{kind}]");
        sink.reply(&activation, Value::Null).await;

        routed_success(&t, &mut host, &mut checkout, &function).await;
        let activation = sink.invoked(&every).await;
        sink.reply(&activation, Value::Null).await;
        assert!(!invoked_already(&mut sink, &failed), "[{kind}] success");

        assert_eq!(fires(&t, &id), 1, "[{kind}] failures only");
        assert_eq!(fires(&t, &control), 3, "[{kind}] the control fires on all");
    }
}

/// SDK-host path, engine side: a current SDK host is told the recipe as it
/// was observed (`capture`, `filters`, `fires_on: [success, error]`, no
/// legacy `on`), which is what it evaluates locally; the failure it reports
/// naming the recipe activates the target with the input and the error.
#[tokio::test]
async fn a_capture_host_is_told_the_recipe_and_its_reported_failure_delivers_input_and_error() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let mut payments = host_with(&t, "payments", OBSERVE_CAPTURE, &["payments::charge"]).await;
    let mut sink = host_with(&t, "sink", OBSERVE_CAPTURE, &["sink::failed"]).await;
    sink.observe_active(observer(
        "recipe",
        recipe("payments::charge"),
        "sink::failed",
    ))
    .await;

    let added = payments
        .expect("observeradded", |f| {
            f["type"] == "observeradded" && f["observer"]["id"] == "recipe"
        })
        .await;
    let view = &added["observer"];
    assert_eq!(view["capture"], json!(["input", "error"]), "{view}");
    assert_eq!(view["fires_on"], json!(["success", "error"]), "{view}");
    // Normalized to the canonical `$and` form hosts receive.
    assert_eq!(
        view["filters"],
        json!({ "$and": [{ "error": { "$exists": true } }] }),
        "{view}"
    );
    assert!(view.get("on").is_none(), "{view}");

    let local = Uuid::new_v4().to_string();
    payments
        .send(json!({
            "type": "invocationsobserved",
            "items": [{
                "function_id": "payments::charge",
                "invocation_id": local,
                "matched": ["recipe"],
                "status": "error",
                "input": { "amount": 3 },
                "error": { "code": "CARD_DECLINED", "message": "declined" },
                "started_at_ms": 1,
                "duration_ms": 0.5,
                "observe_depth": 0,
            }],
        }))
        .await;
    let activation = sink.invoked("sink::failed").await;
    assert_eq!(activation["data"]["invocation_id"], local.as_str());
    assert_failure_envelope(&activation["data"], "recipe", json!({ "amount": 3 }));
    sink.reply(&activation, Value::Null).await;
    assert_eq!(fires(&t, "recipe"), 1);
}

/// The recipe needs a current SDK on the observing side: from a worker
/// without `observe.capture` (an rc SDK here) it keeps the v0.25 semantics
/// (decision 60): no `on` means successes only, and a success never has an
/// `error`, so it never fires, on failures either.
#[tokio::test]
async fn from_a_worker_without_observe_capture_the_recipe_never_fires() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let mut payments = host_with(&t, "payments", OBSERVE_V1, &["payments::charge"]).await;
    let mut sink = host_with(&t, "sink", OBSERVE_V2, &["sink::failed"]).await;
    let mut checkout = connect(&t, "checkout", true).await;
    sink.observe_active(observer(
        "recipe",
        recipe("payments::charge"),
        "sink::failed",
    ))
    .await;

    routed_success(&t, &mut payments, &mut checkout, "payments::charge").await;
    let before = t.engine.observers.evaluated();
    let call = checkout
        .call("payments::charge", json!({ "amount": 2 }))
        .await;
    let invoke = payments.invoked("payments::charge").await;
    payments
        .reply_error(&invoke, "CARD_DECLINED", "declined")
        .await;
    checkout.result(call).await;
    evaluated(&t.engine, before + 1).await;
    assert!(!invoked_already(&mut sink, "sink::failed"));
    assert_eq!(fires(&t, "recipe"), 0);
}
