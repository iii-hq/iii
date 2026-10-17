//! Owner sync and owner reports (protocol.md §2.3, §3.3–§3.5), over real
//! WebSockets: `functionregistered`, `observeradded` / `observerremoved` with
//! per-function revisions, `observerack` gating `active`, `invocationsobserved`,
//! owner delivery (`obs.call`) and `localinvocationstats`.
//!
//! The owners here ack by hand ([`Client::manual_acks`]), so every test
//! controls when (and whether) an observer goes `active`.

mod observe_support;

use iii::observer::ObserversConfig;
use iii::protocol::ObserverState;
use observe_support::{
    Client, Setup, TestEngine, connect, eventually, in_process, registered, spawn, spawn_engine,
    spawn_engine_with,
};
use serde_json::{Value, json};
use uuid::Uuid;

/// An `observe.v1` owner that acks by hand, hosting `functions`. Returns it
/// with each function's `functionregistered`.
async fn owner(t: &TestEngine, name: &str, functions: &[&str]) -> (Client, Vec<Value>) {
    let mut client = connect(t, name, true).await;
    client.manual_acks();
    let mut announced = Vec::new();
    for function in functions {
        client.register_function(function).await;
        announced.push(function_registered(&mut client, function).await);
    }
    (client, announced)
}

/// A host for `functions` that acks like an accepting SDK.
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

async fn removed(client: &mut Client, observer_id: &str) -> Value {
    let id = observer_id.to_string();
    client
        .expect("observerremoved", move |f| {
            f["type"] == "observerremoved" && f["id"] == id.as_str()
        })
        .await
}

fn is_removal(frame: &Value) -> bool {
    frame["type"] == "observerremoved"
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

async fn reject(
    client: &mut Client,
    function_id: &str,
    observer_id: &str,
    revision: &Value,
    code: &str,
) {
    client
        .send(json!({
            "type": "observerack",
            "function_id": function_id,
            "id": observer_id,
            "revision": revision,
            "error": { "code": code, "message": "rejected by the test owner" },
        }))
        .await;
}

/// `engine::observers::get` through `client`. The engine handles a
/// connection's frames in order, so the answer also means every frame
/// `client` sent before it (acks, reports) has been applied.
async fn get(client: &mut Client, id: &str) -> Value {
    let call = client
        .call("engine::observers::get", json!({ "id": id }))
        .await;
    client.result(call).await["result"].clone()
}

fn observer(id: &str, source: Value, target: &str) -> Value {
    json!({ "id": id, "source": source, "target": { "function_id": target } })
}

fn item(function_id: &str, matched: &[&str], status: &str) -> Value {
    json!({
        "function_id": function_id,
        "invocation_id": Uuid::new_v4().to_string(),
        "matched": matched,
        "status": status,
        "input": { "n": 1 },
        "output": { "ok": true },
        "started_at_ms": 1_760_350_000_000u64,
        "duration_ms": 0.25,
    })
}

fn report(items: Vec<Value>) -> Value {
    json!({ "type": "invocationsobserved", "items": items })
}

fn state(t: &TestEngine, id: &str) -> Option<ObserverState> {
    t.engine.observers.get(id).map(|o| o.state())
}

/// An observer of `function_id` from `watcher`, acked by `owner`: active.
async fn acked_observer(
    owner: &mut Client,
    watcher: &mut Client,
    frame: Value,
    function_id: &str,
) -> Value {
    let id = frame["id"].as_str().unwrap().to_string();
    assert_eq!(watcher.observe(frame).await["state"], "pending");
    let added = added(owner, &id).await;
    ack(owner, function_id, &id, &added["revision"]).await;
    watcher.state_becomes(&id, "active").await;
    added
}

#[tokio::test]
async fn an_observer_waits_for_its_owners_ack() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let (mut orders, announced) = owner(&t, "orders", &["orders::create"]).await;
    assert_eq!(announced[0]["namespace"], "default");
    assert_eq!(
        announced[0]["revision"], 1,
        "a new revision per announcement"
    );
    assert_eq!(announced[0]["local_dispatch"], true);
    assert_eq!(announced[0]["observers"], json!([]));
    let mut sink = host(&t, "sink", &["sink::orders"]).await;
    let mut checkout = connect(&t, "checkout", true).await;

    let mut frame = observer(
        "obs-gated",
        json!({ "function_id": "orders::create", "filters": { "input.n": { "$gt": 1 } } }),
        "sink::orders",
    );
    frame["metadata"] = json!({ "secret": "for the target only" });
    let result = sink.observe(frame).await;
    assert_eq!(result["state"], "pending", "{result}");

    let added = added(&mut orders, "obs-gated").await;
    assert_eq!(added["function_id"], "orders::create");
    assert_eq!(added["namespace"], "default");
    assert_eq!(added["revision"], 2);
    let view = &added["observer"];
    assert_eq!(view["target"], "sink::orders");
    assert_eq!(
        view["filters"],
        json!({ "$and": [{ "input.n": { "$gt": 1 } }] })
    );
    assert_eq!(view["args"], json!({}));
    assert_eq!(view["on"], "success");
    assert_eq!(view["capture"], json!(["input", "output", "caller"]));
    for key in ["metadata", "deliver", "worker", "max_in_flight", "legacy"] {
        assert!(
            view.get(key).is_none(),
            "`{key}` is not the owner's business: {view}"
        );
    }

    // Not acked: a routed call is not evaluated, and a report naming the
    // observer is dropped.
    assert!(
        t.engine
            .observers
            .watch("default", "orders::create")
            .is_none()
    );
    let call = checkout.call("orders::create", json!({ "n": 2 })).await;
    orders.serve("orders::create", json!({})).await;
    checkout.result(call).await;
    orders
        .send(report(vec![item(
            "orders::create",
            &["obs-gated"],
            "success",
        )]))
        .await;
    let info = get(&mut orders, "obs-gated").await;
    assert_eq!(info["state"], "pending");
    assert_eq!(info["fires"], 0);
    assert_eq!(info["dropped"]["INACTIVE"], 1, "{info}");

    ack(
        &mut orders,
        "orders::create",
        "obs-gated",
        &added["revision"],
    )
    .await;
    sink.state_becomes("obs-gated", "active").await;

    let call = checkout.call("orders::create", json!({ "n": 3 })).await;
    orders.serve("orders::create", json!({})).await;
    checkout.result(call).await;
    let activation = sink.invoked("sink::orders").await;
    assert_eq!(
        activation["data"]["input"]["n"], 3,
        "the first activation is after the ack"
    );
    assert_eq!(
        activation["metadata"],
        json!({ "secret": "for the target only" })
    );
    sink.reply(&activation, Value::Null).await;
}

#[tokio::test]
async fn an_owner_rejection_fails_the_observer_with_its_code() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let (mut orders, _) = owner(&t, "orders", &["orders::create"]).await;
    let mut sink = host(&t, "sink", &["sink::orders"]).await;

    sink.observe(observer(
        "obs-refused",
        json!({ "function_id": "orders::create" }),
        "sink::orders",
    ))
    .await;
    let added_frame = added(&mut orders, "obs-refused").await;
    reject(
        &mut orders,
        "orders::create",
        "obs-refused",
        &added_frame["revision"],
        "UNSUPPORTED_FILTER",
    )
    .await;
    let error = sink.state_becomes("obs-refused", "error").await;
    assert_eq!(
        error["reason"]["code"], "UNSUPPORTED_FILTER",
        "the owner's code"
    );
    assert!(
        t.engine
            .observers
            .watch("default", "orders::create")
            .is_none()
    );

    // The owner never added it, so it hears no removal: the next add is
    // the next revision.
    sink.observe(observer(
        "obs-next",
        json!({ "function_id": "orders::create" }),
        "sink::orders",
    ))
    .await;
    let next = added(&mut orders, "obs-next").await;
    assert_eq!(next["revision"], 3);
    assert!(!orders.has_seen(is_removal));
}

#[tokio::test]
async fn a_late_rejection_fails_the_observer_and_removes_it() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let (mut orders, _) = owner(&t, "orders", &["orders::create"]).await;
    let mut sink = host(&t, "sink", &["sink::orders"]).await;
    let frame = observer(
        "obs-late",
        json!({ "function_id": "orders::create" }),
        "sink::orders",
    );
    let added = acked_observer(&mut orders, &mut sink, frame, "orders::create").await;

    reject(
        &mut orders,
        "orders::create",
        "obs-late",
        &added["revision"],
        "ARGS_NO_LONGER_VALID",
    )
    .await;
    let error = sink.state_becomes("obs-late", "error").await;
    assert_eq!(error["reason"]["code"], "ARGS_NO_LONGER_VALID");
    let removal = removed(&mut orders, "obs-late").await;
    assert_eq!(removal["revision"], 3);
    assert_eq!(removal["function_id"], "orders::create");
    assert!(
        t.engine
            .observers
            .watch("default", "orders::create")
            .is_none()
    );
}

#[tokio::test]
async fn an_owner_that_never_acks_times_out() {
    let t = spawn_engine(ObserversConfig {
        owner_ack_timeout_ms: 100,
        ..ObserversConfig::default()
    })
    .await;
    let (mut orders, _) = owner(&t, "orders", &["orders::create"]).await;
    let mut sink = host(&t, "sink", &["sink::orders"]).await;
    sink.observe(observer(
        "obs-unacked",
        json!({ "function_id": "orders::create" }),
        "sink::orders",
    ))
    .await;
    added(&mut orders, "obs-unacked").await;

    let error = sink.state_becomes("obs-unacked", "error").await;
    assert_eq!(error["reason"]["code"], "OWNER_TIMEOUT");
    let removal = removed(&mut orders, "obs-unacked").await;
    assert_eq!(removal["revision"], 3, "told about it, so told it is gone");
}

#[tokio::test]
async fn revisions_increase_by_one_per_add_and_remove() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let (mut orders, announced) = owner(&t, "orders", &["orders::create"]).await;
    assert_eq!(announced[0]["revision"], 1);
    let mut sink = host(&t, "sink", &["sink::orders"]).await;
    let source = json!({ "function_id": "orders::create" });

    let a = acked_observer(
        &mut orders,
        &mut sink,
        observer("a", source.clone(), "sink::orders"),
        "orders::create",
    )
    .await;
    let b = acked_observer(
        &mut orders,
        &mut sink,
        observer("b", source.clone(), "sink::orders"),
        "orders::create",
    )
    .await;
    assert_eq!(
        (a["revision"].clone(), b["revision"].clone()),
        (json!(2), json!(3))
    );

    sink.send(json!({ "type": "unobserve", "id": "a" })).await;
    assert_eq!(removed(&mut orders, "a").await["revision"], 4);
    let call = sink
        .call("engine::observers::dispose", json!({ "id": "b" }))
        .await;
    assert_eq!(sink.result(call).await["result"]["disposed"], true);
    assert_eq!(removed(&mut orders, "b").await["revision"], 5);

    // An ack for an older announcement does not count.
    sink.observe(observer("c", source, "sink::orders")).await;
    let c = added(&mut orders, "c").await;
    assert_eq!(c["revision"], 6);
    ack(&mut orders, "orders::create", "c", &json!(4)).await;
    assert_eq!(
        get(&mut orders, "c").await["state"],
        "pending",
        "stale ack ignored"
    );
    ack(&mut orders, "orders::create", "c", &c["revision"]).await;
    sink.state_becomes("c", "active").await;
}

#[tokio::test]
async fn an_owner_coming_back_gets_the_set_again_and_acks_it() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let (mut orders, _) = owner(&t, "orders", &["orders::create"]).await;
    let mut sink = host(&t, "sink", &["sink::orders"]).await;
    let mut checkout = connect(&t, "checkout", true).await;
    let frame = observer(
        "obs-kept",
        json!({ "function_id": "orders::create", "capture": ["input"] }),
        "sink::orders",
    );
    acked_observer(&mut orders, &mut sink, frame, "orders::create").await;

    orders.disconnect().await;
    let pending = sink.state_becomes("obs-kept", "pending").await;
    assert_eq!(pending["reason"]["code"], "OWNER_UNAVAILABLE");

    let (mut orders, announced) = owner(&t, "orders-2", &["orders::create"]).await;
    let set = &announced[0];
    assert_eq!(set["observers"].as_array().unwrap().len(), 1, "{set}");
    assert_eq!(set["observers"][0]["id"], "obs-kept");
    assert_eq!(set["observers"][0]["capture"], json!(["input"]));
    assert_eq!(set["revision"], 3, "revisions continue for the function");
    assert_eq!(
        state(&t, "obs-kept"),
        Some(ObserverState::Pending),
        "the new owner has not acked it yet"
    );

    ack(&mut orders, "orders::create", "obs-kept", &set["revision"]).await;
    sink.state_becomes("obs-kept", "active").await;
    let call = checkout.call("orders::create", json!({ "n": 1 })).await;
    orders.serve("orders::create", json!({})).await;
    checkout.result(call).await;
    let activation = sink.invoked("sink::orders").await;
    sink.reply(&activation, Value::Null).await;
}

#[tokio::test]
async fn a_report_from_a_connection_that_does_not_own_the_function_is_dropped() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let (mut orders, _) = owner(&t, "orders", &["orders::create"]).await;
    let mut sink = host(&t, "sink", &["sink::orders"]).await;
    let frame = observer(
        "obs-owned",
        json!({ "function_id": "orders::create" }),
        "sink::orders",
    );
    acked_observer(&mut orders, &mut sink, frame, "orders::create").await;

    let mut intruder = connect(&t, "intruder", true).await;
    intruder
        .send(report(vec![item(
            "orders::create",
            &["obs-owned"],
            "success",
        )]))
        .await;
    let info = get(&mut intruder, "obs-owned").await;
    assert_eq!(info["dropped"]["REPORTER_NOT_OWNER"], 1, "{info}");
    assert_eq!(info["fires"], 0);
}

#[tokio::test]
async fn one_report_item_activates_every_observer_it_matched() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let (mut orders, _) = owner(&t, "orders", &["orders::create"]).await;
    let mut sink = host(&t, "sink", &["sink::a", "sink::b"]).await;
    let source = json!({ "function_id": "orders::create" });
    acked_observer(
        &mut orders,
        &mut sink,
        observer("a", source.clone(), "sink::a"),
        "orders::create",
    )
    .await;
    acked_observer(
        &mut orders,
        &mut sink,
        observer("b", source, "sink::b"),
        "orders::create",
    )
    .await;

    let mut coalesced = item(
        "orders::create",
        &["a", "b", "a", "not-an-observer"],
        "success",
    );
    coalesced["caller"] = json!({
        "worker_name": "forged",
        "function_id": "orders::local-caller",
        "invocation_id": "inv-local",
    });
    coalesced["observe_depth"] = json!(1);
    coalesced["metadata"] = json!({ "session": "s-1" });
    let invocation_id = coalesced["invocation_id"].clone();
    let mut no_id = item("orders::create", &["a"], "success");
    no_id.as_object_mut().unwrap().remove("invocation_id");
    orders
        .send(report(vec![
            no_id,
            item("orders::create", &["a"], "paused"),
            coalesced,
        ]))
        .await;

    // The bad items are dropped alone; the good one fires each observer once.
    assert_eq!(get(&mut orders, "a").await["fires"], 1);
    assert_eq!(get(&mut orders, "b").await["fires"], 1);

    let activation = sink.invoked("sink::a").await;
    let envelope = &activation["data"];
    assert_eq!(envelope["invocation_id"], invocation_id);
    assert_eq!(envelope["function_id"], "orders::create");
    assert_eq!(envelope["input"], json!({ "n": 1 }));
    assert_eq!(envelope["output"], json!({ "ok": true }));
    assert_eq!(
        envelope["caller"]["worker_name"], "orders",
        "stamped from the reporter"
    );
    assert_eq!(
        envelope["caller"]["worker_id"],
        orders.worker_id.to_string()
    );
    assert_eq!(envelope["caller"]["function_id"], "orders::local-caller");
    assert_eq!(activation["observe_depth"], 2, "the item's depth + 1");
    assert_eq!(activation["caller"]["function_id"], "orders::create");
    assert_eq!(activation["caller"]["invocation_id"], invocation_id);
    assert_eq!(activation["activation"]["observer_id"], "a");
    sink.reply(&activation, Value::Null).await;
    let other = sink.invoked("sink::b").await;
    assert_eq!(other["data"]["invocation_id"], invocation_id);
    sink.reply(&other, Value::Null).await;
}

/// Registers `http::on-request` (owner-delivered, routed by `path`) and
/// `http::auto` on an `observe.v1` owner that acks everything.
async fn http_owner(t: &TestEngine) -> Client {
    let mut http = connect(t, "http", true).await;
    http.register_function_with(json!({
        "type": "registerfunction",
        "id": "http::on-request",
        "observe": {
            "delivery": "owner",
            "args_format": {
                "type": "object",
                "required": ["path"],
                "properties": { "path": { "type": "string" } }
            }
        }
    }))
    .await;
    http.register_function("http::auto").await;
    registered(&t.engine, "default", "http::on-request").await;
    registered(&t.engine, "default", "http::auto").await;
    http
}

fn route(id: &str, path: &str, target: &str) -> Value {
    observer(
        id,
        json!({ "function_id": "http::on-request", "args": { "path": path } }),
        target,
    )
}

#[tokio::test]
async fn an_owner_delivery_goes_to_the_stored_target_and_answers_the_owner() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let mut http = http_owner(&t).await;
    let mut users = host(&t, "users", &["users::get"]).await;
    // The same id in another namespace: an owner naming that namespace on
    // `obs.call` still reaches the observer's target (D13).
    let mut acme = Client::open(t.port, "acme-users").await;
    acme.send(json!({
        "type": "invokefunction",
        "function_id": "engine::workers::register",
        "data": {
            "runtime": "test", "version": "0.25.0", "name": "acme-users",
            "namespace": "acme", "features": ["observe.v1"]
        },
        "action": { "type": "void" },
    }))
    .await;
    acme.register_function("users::get").await;
    registered(&t.engine, "acme", "users::get").await;

    let mut frame = route("route", "/users/:id", "users::get");
    frame["metadata"] = json!({ "team": "users" });
    users.observe_active(frame).await;

    let call = http
        .call_with(
            "users::get",
            json!({ "path": "/users/7" }),
            json!({
                "namespace": "acme",
                "activation": { "observer_id": "route" },
                "caller": { "function_id": "http::on-request", "invocation_id": "inv-request" }
            }),
        )
        .await;
    let delivered = users.invoked("users::get").await;
    assert_eq!(delivered["data"]["path"], "/users/7");
    assert_eq!(
        delivered["metadata"],
        json!({ "team": "users" }),
        "the observer's sidecar"
    );
    assert_eq!(delivered["observe_depth"], 1);
    assert_eq!(delivered["activation"]["observer_id"], "route");
    assert!(delivered["activation"]["activation_id"].is_string());
    assert_eq!(delivered["caller"]["worker_name"], "http");
    assert_eq!(delivered["caller"]["worker_id"], http.worker_id.to_string());
    assert_eq!(delivered["caller"]["function_id"], "http::on-request");
    users.reply(&delivered, json!({ "user": 7 })).await;

    let answer = http.result(call).await;
    assert_eq!(answer["result"], json!({ "user": 7 }), "{answer}");
    assert_eq!(get(&mut http, "route").await["fires"], 1);
    assert!(!acme.has_seen(|f| f["type"] == "invokefunction" && f["function_id"] == "users::get"));
}

#[tokio::test]
async fn owner_delivery_rejections() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let mut http = http_owner(&t).await;
    let mut users = host(&t, "users", &["users::get"]).await;
    let mut frame = route("route", "/users/:id", "users::get");
    frame["max_in_flight"] = json!(1);
    users.observe_active(frame).await;
    users
        .observe_active(observer(
            "auto-obs",
            json!({ "function_id": "http::auto" }),
            "users::get",
        ))
        .await;
    let waiting = users
        .observe(route("waiting", "/later", "users::missing"))
        .await;
    assert_eq!(waiting["state"], "pending");

    let obs_call = |observer_id: &str, function_id: &str, extra: Value| {
        let mut fields = json!({ "activation": { "observer_id": observer_id } });
        for (key, value) in extra.as_object().unwrap() {
            fields[key] = value.clone();
        }
        (function_id.to_string(), fields)
    };
    let cases = [
        (
            "OBSERVER_NOT_FOUND",
            obs_call("no-such-observer", "users::get", json!({})),
        ),
        (
            "NOT_OWNER_DELIVERED",
            obs_call("auto-obs", "users::get", json!({})),
        ),
        (
            "OBSERVER_NOT_ACTIVE",
            obs_call("waiting", "users::missing", json!({})),
        ),
        (
            "TARGET_MISMATCH",
            obs_call("route", "users::other", json!({})),
        ),
        (
            "LOOP_GUARD",
            obs_call("route", "users::get", json!({ "observe_depth": 8 })),
        ),
    ];
    for (code, (function_id, fields)) in cases {
        let call = http.call_with(&function_id, json!({}), fields).await;
        let answer = http.result(call).await;
        assert_eq!(answer["error"]["code"], code, "{answer}");
    }
    let mut stranger = connect(&t, "stranger", true).await;
    let call = stranger
        .call_with(
            "users::get",
            json!({}),
            json!({ "activation": { "observer_id": "route" } }),
        )
        .await;
    assert_eq!(
        stranger.result(call).await["error"]["code"],
        "NOT_FUNCTION_OWNER"
    );

    // One delivery in flight is the limit: the owner is waiting for the
    // result, so the next one fails instead of queueing.
    let (_, fields) = obs_call("route", "users::get", json!({}));
    let first = http
        .call_with("users::get", json!({ "n": 1 }), fields.clone())
        .await;
    let delivered = users.invoked("users::get").await;
    let second = http
        .call_with("users::get", json!({ "n": 2 }), fields.clone())
        .await;
    assert_eq!(http.result(second).await["error"]["code"], "OVERFLOW");
    users.reply(&delivered, json!("first")).await;
    assert_eq!(http.result(first).await["result"], "first");
    // The slot is free again.
    let third = http
        .call_with("users::get", json!({ "n": 3 }), fields)
        .await;
    let delivered = users.invoked("users::get").await;
    users.reply(&delivered, json!("third")).await;
    assert_eq!(http.result(third).await["result"], "third");

    let info = get(&mut http, "route").await;
    assert_eq!(info["dropped"]["LOOP_GUARD"], 1, "{info}");
    assert_eq!(info["dropped"]["OVERFLOW"], 1, "{info}");
    assert_eq!(info["fires"], 2);
}

#[tokio::test]
async fn an_owner_delivery_whose_target_never_answers_times_out() {
    let t = spawn_engine(ObserversConfig {
        activation_timeout_ms: 150,
        ..ObserversConfig::default()
    })
    .await;
    let mut http = http_owner(&t).await;
    let mut users = host(&t, "users", &["users::get"]).await;
    let mut frame = route("route", "/users/:id", "users::get");
    frame["max_in_flight"] = json!(1);
    users.observe_active(frame).await;

    let fields = json!({ "activation": { "observer_id": "route" } });
    let call = http
        .call_with("users::get", json!({}), fields.clone())
        .await;
    users.invoked("users::get").await; // never answered
    assert_eq!(http.result(call).await["error"]["code"], "TIMEOUT");
    let info = get(&mut http, "route").await;
    assert_eq!(info["activation_failures"]["TIMEOUT"], 1, "{info}");

    let call = http.call_with("users::get", json!({}), fields).await;
    let delivered = users.invoked("users::get").await;
    users.reply(&delivered, json!(true)).await;
    assert_eq!(
        http.result(call).await["result"],
        true,
        "the slot was freed"
    );
}

#[tokio::test]
async fn local_dispatch_is_off_under_rbac_middleware() {
    let t = spawn(Setup {
        middleware_function_id: Some("test::middleware".to_string()),
        ..Setup::default()
    })
    .await;
    in_process(&t.engine, "test::middleware", |_| Value::Null);
    let (_owner, announced) = owner(&t, "orders", &["orders::create"]).await;
    assert_eq!(
        announced[0]["local_dispatch"], false,
        "a local call would skip the middleware"
    );
}

#[tokio::test]
async fn local_dispatch_needs_the_connection_to_be_allowed_to_call_the_function() {
    let rbac = json!({ "expose_functions": ["match(\"api::*\")"] });
    let t = spawn_engine_with(ObserversConfig::default(), Some(rbac)).await;
    let (_owner, announced) = owner(&t, "jobs", &["api::job", "internal::job"]).await;
    assert_eq!(announced[0]["local_dispatch"], true);
    assert_eq!(announced[1]["local_dispatch"], false);

    let t = spawn_engine(ObserversConfig {
        local_dispatch: false,
        ..ObserversConfig::default()
    })
    .await;
    let (_owner, announced) = owner(&t, "jobs", &["api::job"]).await;
    assert_eq!(
        announced[0]["local_dispatch"], false,
        "observers.local_dispatch: false"
    );
}

#[tokio::test]
async fn a_legacy_owner_needs_no_acks() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let mut legacy = connect(&t, "legacy", false).await;
    legacy.register_function("legacy::fn").await;
    registered(&t.engine, "default", "legacy::fn").await;
    let _sink = host(&t, "sink", &["sink::legacy"]).await;
    let mut watcher = connect(&t, "watcher", true).await;
    watcher
        .observe_active(observer(
            "obs-legacy",
            json!({ "function_id": "legacy::fn" }),
            "sink::legacy",
        ))
        .await;

    get(&mut legacy, "obs-legacy").await;
    assert!(
        !legacy.has_seen(|f| { f["type"] == "functionregistered" || f["type"] == "observeradded" })
    );
}

/// A worker whose registrations landed before it declared its features (the
/// namespace grace fixed its namespace first) gets `functionregistered` when
/// it declares `observe.v1`, and from then on acks its observers.
#[tokio::test]
async fn features_declared_after_the_registrations_still_start_owner_sync() {
    let t = spawn(Setup {
        namespace_grace_ms: Some(50),
        ..Setup::default()
    })
    .await;
    let mut late = Client::open(t.port, "late").await;
    late.manual_acks();
    late.register_function("late::fn").await;
    registered(&t.engine, "default", "late::fn").await;
    let mut sink = host(&t, "sink", &["sink::late"]).await;
    // Nothing declared yet: an owner without observe, no ack needed.
    sink.observe_active(observer(
        "obs-early",
        json!({ "function_id": "late::fn" }),
        "sink::late",
    ))
    .await;

    late.announce(true).await;
    let set = function_registered(&mut late, "late::fn").await;
    assert_eq!(set["observers"][0]["id"], "obs-early", "{set}");
    sink.state_becomes("obs-early", "pending").await;
    ack(&mut late, "late::fn", "obs-early", &set["revision"]).await;
    sink.state_becomes("obs-early", "active").await;
}

#[tokio::test]
async fn local_invocation_stats_count_the_owners_functions_only() {
    use iii::workers::observability::metrics::get_metrics_accumulator;

    let t = spawn_engine(ObserversConfig::default()).await;
    let (mut owner, _) = owner(&t, "stats", &["stats-owner::fn"]).await;
    let stats = json!({
        "type": "localinvocationstats",
        "window_ms": 10_000,
        "functions": [
            { "function_id": "stats-owner::fn", "count": 5, "errors": 1, "duration_ms_sum": 2.5 },
            { "function_id": "stats-owner::not-mine", "count": 7 }
        ],
        "dropped_reports": [{ "function_id": "stats-owner::fn", "reason": "BUFFER_FULL", "count": 2 }]
    });
    owner.send(stats.clone()).await;
    get(&mut owner, "barrier").await;
    let acc = get_metrics_accumulator();
    assert_eq!(acc.get_function_count("stats-owner::fn"), Some(5));
    assert_eq!(acc.get_function_count("stats-owner::not-mine"), None);

    let mut other = connect(&t, "other", true).await;
    other.send(stats).await;
    get(&mut other, "barrier").await;
    assert_eq!(
        acc.get_function_count("stats-owner::fn"),
        Some(5),
        "only the owner reports a function's local invocations"
    );
}

#[tokio::test]
async fn a_disposed_observer_frees_its_owner_slot() {
    // At max_per_function, disposing one observer lets the next one in, and
    // the owner hears both.
    let t = spawn_engine(ObserversConfig {
        max_per_function: 1,
        ..ObserversConfig::default()
    })
    .await;
    let (mut orders, _) = owner(&t, "orders", &["orders::create"]).await;
    let mut sink = host(&t, "sink", &["sink::orders"]).await;
    let source = json!({ "function_id": "orders::create" });
    acked_observer(
        &mut orders,
        &mut sink,
        observer("first", source.clone(), "sink::orders"),
        "orders::create",
    )
    .await;
    let over = sink
        .observe(observer("second", source.clone(), "sink::orders"))
        .await;
    assert_eq!(over["error"]["code"], "LIMIT_EXCEEDED");

    sink.send(json!({ "type": "unobserve", "id": "first" }))
        .await;
    removed(&mut orders, "first").await;
    assert!(eventually(|| t.engine.observers.get("first").is_none()).await);
    acked_observer(
        &mut orders,
        &mut sink,
        observer("second", source, "sink::orders"),
        "orders::create",
    )
    .await;
}

/// The next owner-sync frame, in arrival order.
async fn next_sync(client: &mut Client) -> Value {
    client
        .expect("owner-sync frame", |f| {
            matches!(
                f["type"].as_str(),
                Some("functionregistered" | "observeradded" | "observerremoved")
            )
        })
        .await
}

/// The sequence protocol.md §2.3 fixes: the registration ack, then one
/// message per add and remove, each one revision up, and nothing else.
#[tokio::test]
async fn owner_sync_frames_arrive_in_protocol_order() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let mut orders = connect(&t, "orders", true).await;
    orders.manual_acks();
    orders.register_function("orders::create").await;
    let mut sink = host(&t, "sink", &["sink::orders"]).await;
    let source = json!({ "function_id": "orders::create" });

    let first = next_sync(&mut orders).await;
    assert_eq!(
        (first["type"].as_str(), first["revision"].as_u64()),
        (Some("functionregistered"), Some(1))
    );
    sink.observe(observer("a", source.clone(), "sink::orders"))
        .await;
    sink.observe(observer("b", source.clone(), "sink::orders"))
        .await;
    sink.send(json!({ "type": "unobserve", "id": "a" })).await;
    let mut seen = Vec::new();
    for _ in 0..3 {
        let frame = next_sync(&mut orders).await;
        let id = frame["observer"]["id"]
            .as_str()
            .or(frame["id"].as_str())
            .map(str::to_string);
        seen.push((
            frame["type"].as_str().unwrap().to_string(),
            id.unwrap(),
            frame["revision"].as_u64().unwrap(),
        ));
    }
    assert_eq!(
        seen,
        vec![
            ("observeradded".to_string(), "a".to_string(), 2),
            ("observeradded".to_string(), "b".to_string(), 3),
            ("observerremoved".to_string(), "a".to_string(), 4),
        ]
    );
    get(&mut orders, "b").await;
    assert!(!orders.has_seen(|f| matches!(
        f["type"].as_str(),
        Some("functionregistered" | "observeradded" | "observerremoved")
    )));
}

#[tokio::test]
async fn acks_that_answer_no_announcement_are_ignored() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let (mut orders, _) = owner(&t, "orders", &["orders::create"]).await;
    let mut sink = host(&t, "sink", &["sink::orders"]).await;
    sink.observe(observer(
        "obs",
        json!({ "function_id": "orders::create" }),
        "sink::orders",
    ))
    .await;
    let added_frame = added(&mut orders, "obs").await;

    // From a connection that does not own the function.
    let mut intruder = connect(&t, "intruder", true).await;
    ack(
        &mut intruder,
        "orders::create",
        "obs",
        &added_frame["revision"],
    )
    .await;
    assert_eq!(get(&mut intruder, "obs").await["state"], "pending");

    // The owner's, twice: the second changes nothing.
    ack(
        &mut orders,
        "orders::create",
        "obs",
        &added_frame["revision"],
    )
    .await;
    ack(
        &mut orders,
        "orders::create",
        "obs",
        &added_frame["revision"],
    )
    .await;
    sink.state_becomes("obs", "active").await;
    assert_eq!(get(&mut orders, "obs").await["state"], "active");
    assert!(!sink.has_seen(|f| f["type"] == "observerstatechanged" && f["id"] == "obs"));

    // For an observer that is gone.
    sink.send(json!({ "type": "unobserve", "id": "obs" })).await;
    removed(&mut orders, "obs").await;
    ack(&mut orders, "orders::create", "obs", &json!(99)).await;
    reject(&mut orders, "orders::create", "obs", &json!(99), "NOPE").await;
    let gone = orders
        .call("engine::observers::get", json!({ "id": "obs" }))
        .await;
    assert_eq!(
        orders.result(gone).await["error"]["code"],
        "OBSERVER_NOT_FOUND"
    );
}

#[tokio::test]
async fn a_conflicting_registration_gets_no_functionregistered() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let (_orders, _) = owner(&t, "orders", &["orders::create"]).await;
    let mut rival = connect(&t, "rival", true).await;
    rival.register_function("orders::create").await;
    let rejected = rival
        .expect("registrationrejected", |f| {
            f["type"] == "registrationrejected"
        })
        .await;
    assert_eq!(rejected["code"], "FUNCTION_NAMESPACE_CONFLICT");
    get(&mut rival, "barrier").await;
    assert!(!rival.has_seen(|f| f["type"] == "functionregistered"));
}

#[tokio::test]
async fn the_owner_hears_observerremoved_when_the_grace_period_ends() {
    let t = spawn_engine(ObserversConfig {
        observer_grace_ms: 100,
        ..ObserversConfig::default()
    })
    .await;
    let (mut orders, _) = owner(&t, "orders", &["orders::create"]).await;
    let _sink = host(&t, "sink", &["sink::orders"]).await;
    let mut watcher = connect(&t, "watcher", true).await;
    acked_observer(
        &mut orders,
        &mut watcher,
        observer(
            "obs-graced",
            json!({ "function_id": "orders::create" }),
            "sink::orders",
        ),
        "orders::create",
    )
    .await;
    watcher.disconnect().await;
    let removal = removed(&mut orders, "obs-graced").await;
    assert_eq!(removal["revision"], 3);
}

#[tokio::test]
async fn an_owner_without_observe_v1_hears_nothing() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let mut legacy = connect(&t, "legacy", false).await;
    legacy.register_function("legacy::fn").await;
    registered(&t.engine, "default", "legacy::fn").await;
    let mut sink = host(&t, "sink", &["sink::legacy"]).await;
    sink.observe_active(observer(
        "obs",
        json!({ "function_id": "legacy::fn" }),
        "sink::legacy",
    ))
    .await;
    sink.send(json!({ "type": "unobserve", "id": "obs" })).await;
    sink.state_becomes("obs", "disposed").await;
    get(&mut legacy, "barrier").await;
    assert!(!legacy.has_seen(|f| f["type"] == "observerremoved" || f["type"] == "observeradded"));
}

/// Revisions never go back within an engine's lifetime, even when the
/// function had no observers left while its owner was away.
#[tokio::test]
async fn revisions_continue_across_an_owner_reconnect_with_no_observers() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let (mut orders, _) = owner(&t, "orders", &["orders::create"]).await;
    let mut sink = host(&t, "sink", &["sink::orders"]).await;
    acked_observer(
        &mut orders,
        &mut sink,
        observer(
            "obs",
            json!({ "function_id": "orders::create" }),
            "sink::orders",
        ),
        "orders::create",
    )
    .await;
    sink.send(json!({ "type": "unobserve", "id": "obs" })).await;
    assert_eq!(removed(&mut orders, "obs").await["revision"], 3);

    orders.disconnect().await;
    assert!(
        eventually(|| t
            .engine
            .functions
            .get("default", "orders::create")
            .is_none())
        .await
    );
    let (_orders, announced) = owner(&t, "orders-2", &["orders::create"]).await;
    assert_eq!(announced[0]["revision"], 4, "{}", announced[0]);
}

/// A registration is announced once, however many paths try (the late
/// features hook racing the drain of the registration it catches up on).
#[tokio::test]
async fn a_registration_is_announced_once() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let (mut orders, _) = owner(&t, "orders", &["orders::create"]).await;
    let worker = t
        .engine
        .worker_registry
        .get_worker(&orders.worker_id)
        .unwrap();
    t.engine
        .observers
        .function_registered(&t.engine, &worker, "default", "orders::create");
    t.engine
        .observers
        .features_declared_late(&t.engine, &worker, "default")
        .await;
    get(&mut orders, "barrier").await;
    assert!(!orders.has_seen(|f| f["type"] == "functionregistered"));
}

/// The owner re-registering its function announces the observer again, with
/// a fresh ack timeout: the first announcement's timer does not fail it.
#[tokio::test]
async fn a_re_announcement_restarts_the_ack_timeout() {
    let timeout = 4_000;
    let t = spawn_engine(ObserversConfig {
        owner_ack_timeout_ms: timeout,
        ..ObserversConfig::default()
    })
    .await;
    let (mut orders, _) = owner(&t, "orders", &["orders::create"]).await;
    let mut sink = host(&t, "sink", &["sink::orders"]).await;
    sink.observe(observer(
        "obs",
        json!({ "function_id": "orders::create" }),
        "sink::orders",
    ))
    .await;
    added(&mut orders, "obs").await;
    // Re-registered at a fifth of the first announcement's timeout; checked
    // a tenth of it past that deadline, a tenth before the second's.
    tokio::time::sleep(std::time::Duration::from_millis(timeout / 5)).await;
    orders.register_function("orders::create").await;
    let again = function_registered(&mut orders, "orders::create").await;
    assert_eq!(again["observers"][0]["id"], "obs");

    // Past the first announcement's deadline: still waiting for the second.
    tokio::time::sleep(std::time::Duration::from_millis(timeout * 9 / 10)).await;
    assert_eq!(state(&t, "obs"), Some(ObserverState::Pending));
    ack(&mut orders, "orders::create", "obs", &again["revision"]).await;
    sink.state_becomes("obs", "active").await;
}

#[tokio::test]
async fn reports_apply_on_for_auto_functions_and_count_invalid_items() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let (mut orders, _) = owner(&t, "orders", &["orders::create"]).await;
    let mut sink = host(&t, "sink", &["sink::errors"]).await;
    acked_observer(
        &mut orders,
        &mut sink,
        observer(
            "obs-errors",
            json!({ "function_id": "orders::create", "on": "error" }),
            "sink::errors",
        ),
        "orders::create",
    )
    .await;

    orders
        .send(report(vec![
            item("orders::create", &["obs-errors"], "success"),
            item("orders::create", &["obs-errors"], "paused"),
        ]))
        .await;
    let info = get(&mut orders, "obs-errors").await;
    assert_eq!(info["fires"], 0, "on: error does not take a success");
    assert_eq!(info["dropped"]["INVALID_REPORT"], 1, "{info}");

    let mut failed = item("orders::create", &["obs-errors"], "error");
    failed["error"] = json!({ "code": "E", "message": "boom" });
    failed["caller"] = json!({
        "function_id": "orders::local",
        "parent": { "worker_name": "billing", "function_id": "billing::close" }
    });
    orders.send(report(vec![failed])).await;
    let activation = sink.invoked("sink::errors").await;
    assert_eq!(activation["data"]["status"], "error");
    assert_eq!(
        activation["data"]["caller"]["parent"],
        json!({ "worker_name": "billing", "function_id": "billing::close" }),
        "caller.parent passes through"
    );
    sink.reply(&activation, Value::Null).await;
}

#[tokio::test]
async fn owner_delivery_policy_and_target_failures() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let mut http = http_owner(&t).await;
    let mut users = host(&t, "users", &["users::get"]).await;
    let mut frame = route("once", "/once", "users::get");
    frame["max_fires"] = json!(1);
    users.observe_active(frame).await;
    let mut frame = route("brief", "/brief", "users::get");
    frame["expires_in_ms"] = json!(100);
    users.observe_active(frame).await;
    users
        .observe_active(route("route", "/users/:id", "users::get"))
        .await;
    let fields = |id: &str| json!({ "activation": { "observer_id": id } });

    // max_fires: the one delivery goes through, and the observer is gone.
    let call = http
        .call_with("users::get", json!({}), fields("once"))
        .await;
    let delivered = users.invoked("users::get").await;
    users.reply(&delivered, json!("once")).await;
    assert_eq!(http.result(call).await["result"], "once");
    users.state_becomes("once", "disposed").await;
    let call = http
        .call_with("users::get", json!({}), fields("once"))
        .await;
    assert_eq!(
        http.result(call).await["error"]["code"],
        "OBSERVER_NOT_FOUND"
    );

    // expires_in_ms
    users.state_becomes("brief", "disposed").await;
    let call = http
        .call_with("users::get", json!({}), fields("brief"))
        .await;
    assert_eq!(
        http.result(call).await["error"]["code"],
        "OBSERVER_NOT_FOUND"
    );

    // The target answers with an error: the owner gets it, counted.
    let call = http
        .call_with(
            "users::get",
            json!({}),
            json!({
                "activation": { "observer_id": "route" },
                "caller": {
                    "function_id": "http::on-request",
                    "parent": { "worker_name": "edge", "function_id": "edge::accept" }
                }
            }),
        )
        .await;
    let delivered = users.invoked("users::get").await;
    assert_eq!(
        delivered["caller"]["parent"],
        json!({ "worker_name": "edge", "function_id": "edge::accept" }),
        "caller.parent passes through an owner call"
    );
    users
        .reply_error(&delivered, "NOT_FOUND", "no such user")
        .await;
    assert_eq!(http.result(call).await["error"]["code"], "NOT_FOUND");

    // The target goes away mid-call.
    let call = http
        .call_with("users::get", json!({}), fields("route"))
        .await;
    users.invoked("users::get").await;
    users.disconnect().await;
    let answer = http.result(call).await;
    assert!(answer["error"]["code"].is_string(), "{answer}");
    let info = get(&mut http, "route").await;
    assert_eq!(info["activation_failures"]["TARGET_ERROR"], 1, "{info}");
    assert_eq!(
        info["activation_failures"]["TARGET_UNAVAILABLE"], 1,
        "{info}"
    );
}

/// The owner going away while its `obs.call` waits frees the slot, and is
/// not counted against the target.
#[tokio::test]
async fn an_owner_leaving_mid_call_frees_the_slot() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let http = http_owner(&t).await;
    let mut users = host(&t, "users", &["users::get"]).await;
    let mut frame = route("route", "/users/:id", "users::get");
    frame["max_in_flight"] = json!(1);
    users.observe_active(frame).await;

    let mut http = http;
    http.call_with(
        "users::get",
        json!({ "n": 1 }),
        json!({ "activation": { "observer_id": "route" } }),
    )
    .await;
    let stranded = users.invoked("users::get").await;
    http.disconnect().await;
    users.state_becomes("route", "pending").await;
    assert!(
        eventually(|| t.engine.observers.in_flight("route") == Some((0, 0))).await,
        "the slot is free"
    );

    let mut http = http_owner(&t).await;
    users.state_becomes("route", "active").await;
    let call = http
        .call_with(
            "users::get",
            json!({ "n": 2 }),
            json!({ "activation": { "observer_id": "route" } }),
        )
        .await;
    let delivered = users.invoked("users::get").await;
    assert_eq!(delivered["data"]["n"], 2);
    users.reply(&delivered, json!(2)).await;
    assert_eq!(http.result(call).await["result"], 2);
    users.reply(&stranded, json!(1)).await;
    let info = get(&mut http, "route").await;
    assert!(
        info["activation_failures"]
            .get("TARGET_UNAVAILABLE")
            .is_none(),
        "{info}"
    );
}

/// An `obs.call` with `action: void` gets no answer, and leaves nothing
/// tracked on the owner's connection.
#[tokio::test]
async fn a_void_owner_call_leaves_no_invocation_behind() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let mut http = http_owner(&t).await;
    let mut users = host(&t, "users", &["users::get"]).await;
    users
        .observe_active(route("route", "/users/:id", "users::get"))
        .await;
    http.call_with(
        "users::get",
        json!({}),
        json!({ "activation": { "observer_id": "route" }, "action": { "type": "void" } }),
    )
    .await;
    let delivered = users.invoked("users::get").await;
    users.reply(&delivered, json!("ignored")).await;
    let owner = t
        .engine
        .worker_registry
        .get_worker(&http.worker_id)
        .unwrap();
    let mut empty = false;
    for _ in 0..2_500 {
        if owner.invocation_count().await == 0 {
            empty = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }
    assert!(empty, "the owner's connection still tracks the call");
    get(&mut http, "barrier").await;
    assert!(!http.has_seen(|f| f["type"] == "invocationresult" && f["result"] == "ignored"));
}

/// Only the function's owner is announced to, whoever asks.
#[tokio::test]
async fn only_the_owner_is_announced_to() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let (_orders, _) = owner(&t, "orders", &["orders::create"]).await;
    let mut rival = connect(&t, "rival", true).await;
    let rival_worker = t
        .engine
        .worker_registry
        .get_worker(&rival.worker_id)
        .unwrap();
    t.engine
        .observers
        .function_registered(&t.engine, &rival_worker, "default", "orders::create");
    get(&mut rival, "barrier").await;
    assert!(!rival.has_seen(|f| f["type"] == "functionregistered"));
}

#[tokio::test]
async fn local_invocation_counts_saturate() {
    use iii::workers::observability::metrics::get_metrics_accumulator;

    let t = spawn_engine(ObserversConfig::default()).await;
    let (mut owner, _) = owner(&t, "stats", &["saturating::fn"]).await;
    for _ in 0..2 {
        owner
            .send(json!({
                "type": "localinvocationstats",
                "window_ms": 10_000,
                "functions": [{ "function_id": "saturating::fn", "count": u64::MAX }],
                "dropped_reports": [
                    { "function_id": "saturating::fn", "reason": "made_up", "count": u64::MAX }
                ]
            }))
            .await;
    }
    get(&mut owner, "barrier").await;
    assert_eq!(
        get_metrics_accumulator().get_function_count("saturating::fn"),
        Some(u64::MAX)
    );
}

/// An owner-delivered function's events are what its owner reports, stamped
/// with the owner's identity: only the owner invokes it, or anyone could
/// forge them. Auto functions are not restricted.
#[tokio::test]
async fn only_the_owner_invokes_an_owner_delivered_function() {
    use iii::engine::EngineTrait;

    let t = spawn_engine(ObserversConfig::default()).await;
    let mut http = http_owner(&t).await;
    let mut other = connect(&t, "other", true).await;

    let call = other
        .call("http::on-request", json!({ "path": "/forged" }))
        .await;
    let refused = other.result(call).await;
    assert_eq!(refused["error"]["code"], "FORBIDDEN", "{refused}");
    assert!(
        refused["error"]["message"]
            .as_str()
            .unwrap()
            .contains("owner-delivered")
    );
    let refused = t
        .engine
        .call("http::on-request", json!({ "path": "/engine" }))
        .await
        .unwrap_err();
    assert_eq!(refused.code, "FORBIDDEN", "the engine itself too");

    // Its owner, calling itself remotely.
    let call = http.call("http::on-request", json!({ "path": "/" })).await;
    http.serve("http::on-request", json!({ "status_code": 200 }))
        .await;
    assert_eq!(
        http.result(call).await["result"],
        json!({ "status_code": 200 })
    );

    // An auto function: anyone.
    let call = other.call("http::auto", json!({})).await;
    http.serve("http::auto", json!("auto")).await;
    assert_eq!(other.result(call).await["result"], "auto");
    assert!(!http.has_seen(|f| f["type"] == "invokefunction" && f["data"]["path"] == "/forged"));
}

/// An owner-delivered function cannot be an observer's target: every
/// activation would be refused. Refused at `observe`, and when the target
/// registers as owner-delivered later.
#[tokio::test]
async fn an_owner_delivered_function_is_no_target() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let _http = http_owner(&t).await;
    let _orders = host(&t, "orders", &["orders::create"]).await;
    let mut watcher = connect(&t, "watcher", true).await;
    let refused = watcher
        .observe(observer(
            "into-http",
            json!({ "function_id": "orders::create" }),
            "http::on-request",
        ))
        .await;
    assert_eq!(refused["error"]["code"], "FORBIDDEN", "{refused}");

    let pending = watcher
        .observe(observer(
            "into-later",
            json!({ "function_id": "orders::create" }),
            "late::router",
        ))
        .await;
    assert_eq!(pending["state"], "pending");
    let mut late = connect(&t, "late", true).await;
    late.register_function_with(json!({
        "type": "registerfunction",
        "id": "late::router",
        "observe": { "delivery": "owner" }
    }))
    .await;
    let error = watcher.state_becomes("into-later", "error").await;
    assert_eq!(error["reason"]["code"], "FORBIDDEN");
}

/// The owner-only rule holds for every way a call reaches the function: a
/// fire-and-forget call, and an old provider firing a trigger bound to it.
#[tokio::test]
async fn void_calls_and_trigger_fan_out_cannot_reach_an_owner_delivered_function() {
    let t = spawn_engine(ObserversConfig::default()).await;
    let mut http = http_owner(&t).await;
    let mut other = connect(&t, "other", true).await;
    other
        .send(json!({
            "type": "invokefunction",
            "function_id": "http::on-request",
            "data": { "path": "/void" },
            "action": { "type": "void" }
        }))
        .await;

    // An old-style provider fires a binding to it, as old providers do: by
    // invoking the bound function.
    let mut provider = connect(&t, "ticker", false).await;
    provider
        .send(json!({ "type": "registertriggertype", "id": "tick", "description": "old ticks" }))
        .await;
    assert!(
        eventually(|| t
            .engine
            .trigger_registry
            .trigger_types
            .contains_key(&iii::trigger::type_key("default", "tick")))
        .await
    );
    other
        .send(json!({
            "type": "registertrigger",
            "id": "t-tick",
            "trigger_type": "tick",
            "function_id": "http::on-request",
            "config": {}
        }))
        .await;
    let binding = provider
        .expect("registertrigger", |f| {
            f["type"] == "registertrigger" && f["id"] == "t-tick"
        })
        .await;
    let fire = provider
        .call(
            binding["function_id"].as_str().unwrap(),
            json!({ "path": "/tick" }),
        )
        .await;
    assert_eq!(provider.result(fire).await["error"]["code"], "FORBIDDEN");

    // The void call was handled before `other`'s later call; neither reached
    // the owner.
    let call = other
        .call("http::on-request", json!({ "path": "/after" }))
        .await;
    assert_eq!(other.result(call).await["error"]["code"], "FORBIDDEN");
    get(&mut http, "barrier").await;
    assert!(
        !http.has_seen(|f| f["type"] == "invokefunction" && f["function_id"] == "http::on-request")
    );
}
