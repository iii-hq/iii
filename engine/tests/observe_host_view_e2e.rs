//! What a function's host reads from its observers' announcements (CONTRACT
//! D5), over real WebSockets: every `observeradded` and every observer a
//! `functionregistered` lists carries `fires_on` (`["success"]`, `["error"]` or
//! `["success", "error"]`), and a host that did not declare
//! `observe.capture` (a 0.25 host, and an rc host that declared `observe.v2`
//! only) also gets the legacy `on` it decides the outcomes that fire from.

mod observe_support;

use iii::observer::ObserversConfig;
use observe_support::{Client, TestEngine, connect_with, eventually, spawn_engine};
use serde_json::{Value, json};

/// A host built on a new SDK: it reads `fires_on`.
const CAPTURE_HOST: &[&str] = &[
    "observe.v1",
    "observe.v2",
    "observe.capture",
    "local-dispatch.v1",
];
/// A host built on an rc SDK: `observe.v2`, no `observe.capture`.
const RC_HOST: &[&str] = &["observe.v1", "observe.v2", "local-dispatch.v1"];
/// A host built on a 0.25 SDK.
const V025_HOST: &[&str] = &["observe.v1", "local-dispatch.v1"];

/// One observer per outcome set: its id, `capture`, the `fires_on` it is
/// announced with and the legacy `on` a host without `observe.capture`
/// gets.
fn cases() -> Vec<(&'static str, Value, Value, Option<&'static str>)> {
    vec![
        (
            "obs-success",
            json!(["input", "output"]),
            json!(["success"]),
            None,
        ),
        (
            "obs-error",
            json!(["error"]),
            json!(["error"]),
            Some("error"),
        ),
        (
            "obs-complete",
            json!(["input", "error"]),
            json!(["success", "error"]),
            Some("complete"),
        ),
    ]
}

async fn function_registered(client: &mut Client, function_id: &str) -> Value {
    let id = function_id.to_string();
    client
        .expect("functionregistered", move |f| {
            f["type"] == "functionregistered" && f["id"] == id.as_str()
        })
        .await
}

async fn added(client: &mut Client, observer_id: &str) -> Value {
    let id = observer_id.to_string();
    client
        .expect("observeradded", move |f| {
            f["type"] == "observeradded" && f["observer"]["id"] == id.as_str()
        })
        .await
}

async fn ack(client: &mut Client, function_id: &str, observer_id: &str, revision: &Value) {
    client
        .send(json!({
            "type": "observerack",
            "function_id": function_id,
            "id": observer_id,
            "revision": revision,
        }))
        .await;
}

/// A host declaring `features` that acks by hand, with `orders::create`
/// registered and announced.
async fn orders_host(t: &TestEngine, name: &str, features: &[&str]) -> (Client, Value) {
    let mut host = connect_with(t, name, features).await;
    host.manual_acks();
    host.register_function("orders::create").await;
    let registered = function_registered(&mut host, "orders::create").await;
    (host, registered)
}

/// The views a host declaring `features` is announced the [`cases`] with:
/// each `observeradded`, then (after a new instance of the host takes the
/// function over) the `functionregistered` that lists them all.
async fn announced_views(features: &[&str]) -> (TestEngine, Client, Vec<Value>, Vec<Value>) {
    let t = spawn_engine(ObserversConfig::default()).await;
    let (mut orders, _) = orders_host(&t, "orders", features).await;
    // The observing worker runs a current SDK (CONTRACT D8).
    let mut sink = connect_with(&t, "sink", CAPTURE_HOST).await;
    sink.register_function("sink::orders").await;
    observe_support::registered(&t.engine, "default", "sink::orders").await;

    let mut added_views = Vec::new();
    for (id, capture, _, _) in cases() {
        let frame = json!({
            "id": id,
            "source": { "function_id": "orders::create", "capture": capture },
            "target": { "function_id": "sink::orders" },
        });
        assert_eq!(sink.observe(frame).await["state"], "pending", "{id}");
        let added = added(&mut orders, id).await;
        ack(&mut orders, "orders::create", id, &added["revision"]).await;
        sink.state_becomes(id, "active").await;
        added_views.push(added["observer"].clone());
    }

    orders.disconnect().await;
    assert!(
        eventually(|| t
            .engine
            .functions
            .get("default", "orders::create")
            .is_none())
        .await
    );
    let (_orders, registered) = orders_host(&t, "orders-2", features).await;
    let listed = registered["observers"]
        .as_array()
        .cloned()
        .unwrap_or_else(|| panic!("observers listed: {registered}"));
    (t, sink, added_views, listed)
}

/// Every view names the outcomes its observer fires on; `legacy` hosts also
/// get the `on` that says the same, and the others never get `on`.
fn assert_views(views: &[Value], legacy: bool, what: &str) {
    for (id, _, fires, on) in cases() {
        let view = views
            .iter()
            .find(|view| view["id"] == id)
            .unwrap_or_else(|| panic!("{what}: `{id}` not announced: {views:?}"));
        assert_eq!(view["fires_on"], fires, "{what}: `{id}` fires_on: {view}");
        assert!(
            view.get("fires").is_none(),
            "{what}: `{id}` no array under `fires`: {view}"
        );
        match (legacy, on) {
            (true, Some(on)) => assert_eq!(view["on"], on, "{what}: `{id}` on: {view}"),
            _ => assert!(
                view.get("on").is_none(),
                "{what}: `{id}` gets no `on`: {view}"
            ),
        }
    }
}

/// A host that declared `observe.capture` reads `fires`: it never gets the
/// legacy `on`, however its observers fire.
#[tokio::test]
async fn a_capture_host_gets_fires_and_never_the_legacy_on() {
    let (_t, _sink, added, listed) = announced_views(CAPTURE_HOST).await;
    assert_views(&added, false, "observeradded");
    assert_views(&listed, false, "functionregistered");
}

/// The rc-host bug: an rc SDK declares `observe.v2` but not
/// `observe.capture`, and decides the outcomes that fire from `on` alone.
/// Without `on`, its error-firing observer never saw a failure. It gets
/// `fires` and the legacy `on`: `error`, `complete`, none for success.
#[tokio::test]
async fn an_rc_host_that_declared_observe_v2_only_gets_the_legacy_on() {
    let (_t, _sink, added, listed) = announced_views(RC_HOST).await;
    let error = added.iter().find(|view| view["id"] == "obs-error").unwrap();
    assert_eq!(
        error["on"], "error",
        "an rc host decides firing from `on`: without it, `obs-error` fires on success: {error}"
    );
    assert_views(&added, true, "observeradded");
    assert_views(&listed, true, "functionregistered");
}

/// A 0.25 host (no `observe.v2`) gets the same as an rc host.
#[tokio::test]
async fn a_0_25_host_gets_fires_and_the_legacy_on() {
    let (_t, _sink, added, listed) = announced_views(V025_HOST).await;
    assert_views(&added, true, "observeradded");
    assert_views(&listed, true, "functionregistered");
}

/// Two hosts of different SDKs owning two functions each get the view their
/// own features call for, whatever the other declared.
#[tokio::test]
async fn each_host_gets_the_view_its_own_features_call_for() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let mut new_host = connect_with(&t, "new", CAPTURE_HOST).await;
    new_host.manual_acks();
    new_host.register_function("new::run").await;
    function_registered(&mut new_host, "new::run").await;
    let mut rc_host = connect_with(&t, "rc", RC_HOST).await;
    rc_host.manual_acks();
    rc_host.register_function("rc::run").await;
    function_registered(&mut rc_host, "rc::run").await;
    // The observing worker runs a current SDK (CONTRACT D8).
    let mut sink = connect_with(&t, "sink", CAPTURE_HOST).await;
    sink.register_function("sink::all").await;
    observe_support::registered(&t.engine, "default", "sink::all").await;

    for (host, function_id, legacy) in [
        (&mut new_host, "new::run", false),
        (&mut rc_host, "rc::run", true),
    ] {
        let id = format!("obs-{function_id}");
        let frame = json!({
            "id": id,
            "source": { "function_id": function_id, "capture": ["error"] },
            "target": { "function_id": "sink::all" },
        });
        assert_eq!(sink.observe(frame).await["state"], "pending");
        let view = added(host, &id).await["observer"].clone();
        assert_eq!(view["fires_on"], json!(["error"]), "{function_id}: {view}");
        assert!(view.get("fires").is_none(), "{function_id}: {view}");
        if legacy {
            assert_eq!(view["on"], "error", "{function_id}: {view}");
        } else {
            assert!(view.get("on").is_none(), "{function_id}: {view}");
        }
    }
}

/// `engine::observers::list` and `get` name the outcomes each observer
/// fires on next to its `capture`. `fires` there already counts the
/// observer's activations, so the outcomes are `fires_on`.
#[tokio::test]
async fn the_observer_listing_names_the_outcomes_each_observer_fires_on() {
    let (_t, mut sink, _, _) = announced_views(CAPTURE_HOST).await;
    let call = sink
        .call(
            "engine::observers::list",
            json!({ "function_id": "orders::create" }),
        )
        .await;
    let listed = sink.result(call).await["result"]["observers"].clone();
    let listed = listed.as_array().cloned().unwrap_or_default();
    assert_eq!(listed.len(), cases().len(), "{listed:?}");
    for (id, capture, fires, _) in cases() {
        let call = sink
            .call("engine::observers::get", json!({ "id": id }))
            .await;
        let info = sink.result(call).await["result"].clone();
        assert_eq!(info["fires_on"], fires, "`{id}`: {info}");
        assert_eq!(info["capture"], capture, "`{id}`: {info}");
        assert_eq!(info["fires"], 0, "the activation count stays: {info}");
        let in_list = listed.iter().find(|o| o["id"] == id).unwrap();
        assert_eq!(in_list["fires_on"], fires, "`{id}` listed: {in_list}");
    }
}
